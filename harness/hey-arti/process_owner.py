"""Linux process ownership and the private per-command subreaper entry point."""

import ctypes
import json
import os
from pathlib import Path
import selectors
import signal
import socket
import subprocess
import sys
import time

# A watchdog bounds failed cleanup, not the lifetime of a running command.
EXIT_BOUND = 30
# Linux prctl(2) request to adopt this subtree's orphans, including zombies.
PR_SET_CHILD_SUBREAPER = 36
# Control packets contain only a PID, status, error, or the MCP command to stop.
PACKET_LIMIT = 65536


def stat_fields(pid):
    # procfs starts the tail at field 3; the process name can itself contain ')'.
    return Path(f"/proc/{pid}/stat").read_bytes().rsplit(b")", 1)[1].split()


def process_descriptor(owner):
    """Pin a saved Linux process identity, not an unrelated reused PID."""
    try:
        descriptor = os.pidfd_open(owner["pid"])
    except ProcessLookupError:
        return None
    try:
        if int(stat_fields(owner["pid"])[19]) == owner["start_time"]:
            # A zombie group leader may still have live threads.
            return descriptor
    except FileNotFoundError:
        pass
    except BaseException:
        os.close(descriptor)
        raise
    os.close(descriptor)
    return None


def exit_ready(descriptor, timeout):
    with selectors.DefaultSelector() as observation:
        observation.register(descriptor, selectors.EVENT_READ)
        return bool(observation.select(timeout))


def send(channel, packet):
    channel.sendall(json.dumps(packet).encode())


def receive(channel):
    packet = channel.recv(PACKET_LIMIT)
    return json.loads(packet) if packet else None


def stop(pid, start_time, deadline):
    descriptor = process_descriptor({"pid": pid, "start_time": start_time})
    if descriptor is not None:
        try:
            try:
                signal.pidfd_send_signal(descriptor, signal.SIGKILL)
            except ProcessLookupError:
                pass
            if not exit_ready(descriptor, max(0, deadline - time.monotonic())):
                raise RuntimeError(f"Owned process {pid} did not exit")
        finally:
            os.close(descriptor)


def process_table():
    for path in Path("/proc").glob("[0-9]*/stat"):
        pid = int(path.parent.name)
        try:
            fields = stat_fields(pid)
        except (FileNotFoundError, ProcessLookupError):
            continue
        yield pid, (int(fields[1]), int(fields[3]), int(fields[19]))


def drain_tree():
    """Kill direct children until the kernel proves the entire subtree is gone."""
    deadline = time.monotonic() + EXIT_BOUND
    while True:
        try:
            # Consuming any child status is safe here: all children belong to
            # this command, and an exited leader's status is saved separately.
            if os.waitpid(-1, os.WNOHANG)[0]:
                continue
        except ChildProcessError:
            return
        for pid, (parent, _, started) in process_table():
            if parent == os.getpid():
                stop(pid, started, deadline)
        # Each stopped parent exposes its surviving children for the next pass.
        if time.monotonic() >= deadline:
            raise RuntimeError("Owned process tree did not drain")


def drain_matches(select_members):
    deadline = time.monotonic() + EXIT_BOUND
    empty = False
    while True:
        members = list(select_members(dict(process_table())))
        if not members and empty:
            return
        empty = not members
        for pid, started in members:
            stop(pid, started, deadline)
            try:
                os.waitpid(pid, os.WNOHANG)
            except ChildProcessError:
                # Its own parent may still be alive. The final drain reaps it
                # if it later becomes this subreaper's child.
                pass
        if time.monotonic() >= deadline:
            raise RuntimeError("Owned process set did not drain")


def in_session(table, leader):
    for pid, (parent, session, started) in table.items():
        # Include adopted corpses for reaping, and other members only while
        # they can still run. A corpse may still belong to a live parent.
        if pid != leader and session == leader and (parent == os.getpid() or not exited(pid, started)):
            yield pid, started


def exited(pid, started):
    descriptor = process_descriptor({"pid": pid, "start_time": started})
    if descriptor is None:
        return True
    try:
        return exit_ready(descriptor, 0)
    finally:
        os.close(descriptor)


def submitters(table, argv):
    for pid, (parent, _, started) in table.items():
        ancestor = parent
        while ancestor != os.getpid() and ancestor in table:
            next_parent, _, ancestor_started = table[ancestor]
            if next_parent == ancestor or ancestor_started > started:
                break
            ancestor = next_parent
        if ancestor != os.getpid():
            continue
        try:
            if Path(f"/proc/{pid}/cmdline").read_bytes() == argv:
                yield pid, started
        except (FileNotFoundError, ProcessLookupError):
            pass


def become_subreaper():
    prctl = ctypes.CDLL(None, use_errno=True).prctl
    prctl.argtypes = [ctypes.c_int, *([ctypes.c_ulong] * 4)]
    if prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0:
        error = ctypes.get_errno()
        raise OSError(error, f"Cannot adopt fixture descendants: {os.strerror(error)}")


def serve(channel, command, inherited, deferred):
    try:
        become_subreaper()
        child = subprocess.Popen(command, pass_fds=inherited, start_new_session=True)
    except OSError as error:
        send(channel, {"launch_error": str(error), "errno": error.errno})
        return

    def stop_command(*, preserve_session_id=False):
        if child.returncode is None:
            if preserve_session_id:
                # Keep the exited leader unreaped while its session ID is used.
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                descriptor = os.pidfd_open(child.pid)
                try:
                    if not exit_ready(descriptor, EXIT_BOUND):
                        raise RuntimeError("Command leader did not exit")
                finally:
                    os.close(descriptor)
                drain_matches(lambda table: in_session(table, child.pid))
            else:
                child.kill()
            child.wait(timeout=EXIT_BOUND)
        return child.returncode

    finished = False
    try:
        # Only the application should keep the data pipes and fixture fds open.
        for descriptor in set(inherited) - {0, 1, 2}:
            os.close(descriptor)
        with open(os.devnull, "r+b") as null:
            for descriptor in range(3):
                os.dup2(null.fileno(), descriptor)
        send(channel, {"pid": child.pid})
        while request := receive(channel):
            operation, *args = request
            try:
                if operation == "stop":
                    result = stop_command(preserve_session_id=deferred)
                    if not deferred:
                        drain_tree()
                        finished = True
                elif operation == "submitters":
                    argv = b"\0".join(os.fsencode(arg) for arg in args) + b"\0"
                    drain_matches(lambda table: submitters(table, argv))
                    result = None
                elif operation == "finish":
                    result = stop_command()
                    drain_tree()
                    finished = True
                else:
                    raise ValueError(f"Unknown process ownership operation: {operation!r}")
            except Exception as error:
                send(channel, {"error": str(error)})
            else:
                send(channel, {"result": result})
                if finished:
                    return
    finally:
        if not finished:
            stop_command()
            drain_tree()


if __name__ == "__main__":
    _, endpoint, descriptors, deferred, *command = sys.argv
    with socket.socket(fileno=int(endpoint)) as channel:
        serve(channel, command, tuple(json.loads(descriptors)), deferred == "deferred")
