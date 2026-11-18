"""Stdlib-only hey-arti fixture driver.

Source fixtures never contain declarations. Live builders run manually outside CI.
"""

import argparse
import atexit
from contextlib import contextmanager
import os
from pathlib import Path, PurePosixPath
import runpy
import selectors
import shutil
from secrets import token_hex
import signal
import subprocess
import sys
import time
import threading
import tomllib

ROOT = Path(__file__).resolve().parent
# Cleanup watchdog only; no caller or suite timeout determines an owned lifetime.
EXIT_BOUND = 30


class Terminated(KeyboardInterrupt):
    """SIGTERM delivered as an interruption so finally blocks can join fixtures."""

    exit_code = 128 + signal.SIGTERM


@contextmanager
def term_as_interrupt():
    def interrupted(number, frame):
        raise Terminated("SIGTERM")

    previous = signal.signal(signal.SIGTERM, interrupted)
    try:
        yield
    finally:
        signal.signal(signal.SIGTERM, previous)


@contextmanager
def defer_interrupts():
    """Delay handled signals while publishing ownership or joining a fixture."""
    if threading.current_thread() is not threading.main_thread():
        yield
        return
    pending = []
    previous = {}
    completed = False
    try:
        for number in (signal.SIGINT, signal.SIGTERM):
            handler = signal.getsignal(number)
            if callable(handler):
                previous[number] = signal.signal(number, lambda signum, frame: pending.append((signum, frame)))
        yield
        completed = True
    finally:
        for number, handler in previous.items():
            signal.signal(number, handler)
        # A pending interrupt must never mask an unconfirmed-exit failure.
        if pending and completed:
            signum, frame = pending[0]
            previous[signum](signum, frame)


def process_descriptor(owner):
    """Pin a saved Linux process identity, not an unrelated reused PID."""
    if not hasattr(os, "pidfd_open"):
        raise RuntimeError("Owned worker exit observation requires Linux pidfd support")
    try:
        descriptor = os.pidfd_open(owner["pid"])
    except ProcessLookupError:
        return None
    try:
        fields = Path(f"/proc/{owner['pid']}/stat").read_bytes().rsplit(b")", 1)[1].split()
        # procfs starttime is field 22; the tail after the name starts at field 3.
        same_process = int(fields[19]) == owner["start_time"]
        if same_process and fields[0] not in {b"Z", b"X", b"x"}:
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


def wait_for_exit(owner, *, timeout=EXIT_BOUND):
    descriptor = None
    try:
        with defer_interrupts():
            descriptor = process_descriptor(owner)
        if descriptor is not None and not exit_ready(descriptor, timeout):
            raise RuntimeError(f"Owned process {owner['pid']} did not exit; preserve its working files")
    finally:
        if descriptor is not None:
            os.close(descriptor)


class OwnedChild(subprocess.Popen):
    """Keep the Linux group leader unreaped until its group is drained."""

    group_drained = False

    def _leader_exited(self, timeout):
        # WNOWAIT proves this is still our unreaped child, not a recycled PID.
        if os.waitid(os.P_PID, self.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None:
            return True
        descriptor = None
        try:
            with defer_interrupts():
                descriptor = os.pidfd_open(self.pid)
            return exit_ready(descriptor, timeout)
        finally:
            if descriptor is not None:
                os.close(descriptor)

    def _wait(self, timeout):
        # Popen.communicate() also calls _wait directly on KeyboardInterrupt.
        # Intercept both paths so neither can release the PGID before teardown.
        if not self.group_drained:
            if not self._leader_exited(None if timeout is None else max(0, timeout)):
                raise subprocess.TimeoutExpired(self.args, timeout)
            self.drain()
        return super()._wait(timeout)

    def drain(self):
        if self.group_drained:
            return
        deadline = time.monotonic() + EXIT_BOUND
        # A live or zombie leader still reserves its PID until we reap it.
        os.waitid(os.P_PID, self.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
        try:
            os.killpg(self.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        if not self._leader_exited(max(0, deadline - time.monotonic())):
            raise RuntimeError("Group leader did not exit")
        drain_members(self.pid, None if self.deferred else self.descendants, deadline)
        self.group_drained = True


class Descendants:
    """Identify cooperating fixture descendants that start a new Linux session.

    Process-tree membership alone is lost on reparenting. Commands inherit a
    private ownership marker across exec; deliberately detached Artifactize Runs
    keep their marker but are handed to the caller's authoritative Run cleanup.
    This is fixture teardown, not containment of programs that scrub their env.
    """

    def __init__(self):
        self.token = token_hex(16)
        self.marker = f"HEY_ARTI_FIXTURE_OWNER={self.token}".encode("ascii")

    def environment(self, env):
        return dict(os.environ if env is None else env, HEY_ARTI_FIXTURE_OWNER=self.token)

    def contains(self, proc):
        try:
            if proc.stat().st_uid != os.geteuid():
                return False
            return self.marker in (proc / "environ").read_bytes().split(b"\0")
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            return False

    def _finish_matching(self, matches):
        with defer_interrupts():
            try:
                drain_members(None, self, time.monotonic() + EXIT_BOUND, matches=matches)
            except BaseException as error:
                raise ProcessExitError("Detached fixture processes did not settle") from error

    def stop_mcp_submitters(self, command):
        # The configured MCP command is the detached Run submitter. Match its
        # full invocation, not the executable shared by admitted Run workers.
        argv = b"\0".join(os.fsencode(argument) for argument in command) + b"\0"
        self._finish_matching(lambda proc: (proc / "cmdline").read_bytes() == argv)

    def finish(self):
        self._finish_matching(None)


def spawn(command, *, detached=None, **options):
    """Launch a Linux child; its cleanup owns a session and inherited marker."""
    if sys.platform != "linux":
        raise RuntimeError("The hey-arti harness runs on Linux only.")
    descendants = detached if detached is not None else Descendants()
    options["start_new_session"] = True
    options["env"] = descendants.environment(options.get("env"))
    process = OwnedChild(command, **options)
    process.descendants = descendants
    process.deferred = detached is not None
    return process


class ChildCleanup:
    """Own unreleased children, including their pipes, until shutdown retries."""

    def __init__(self):
        self.pending = set()

    def add(self, process):
        self.pending.add(process)

    def close(self, process):
        close_child(process)
        self.pending.discard(process)

    def retry(self):
        for process in tuple(self.pending):
            try:
                self.close(process)
            except BaseException as error:
                print(f"Could not release owned child {process.pid} at shutdown: {error}", file=sys.stderr)


child_cleanup = ChildCleanup()
atexit.register(child_cleanup.retry)


@contextmanager
def owned_child(command, *, on_spawn=None, **options):
    process = None
    try:
        try:
            with defer_interrupts():
                process = spawn(command, **options)
                child_cleanup.add(process)
                if on_spawn is not None:
                    on_spawn(process)
            yield process
        finally:
            if process is not None:
                with defer_interrupts():
                    child_cleanup.close(process)
    except BaseException as error:
        # The signal may arrive before entering the teardown guard. Preserve
        # ownership in that case rather than letting the caller drop its inputs.
        if process is not None and process.returncode is None and not isinstance(error, ProcessExitError):
            raise ProcessExitError(f"Owned child {process.pid} exit is unconfirmed", process=process) from error
        raise


class ProcessExitError(RuntimeError):
    """An owned process may still use its working files; retain them."""

    def __init__(self, message, *, process=None):
        super().__init__(message)
        self.process = process


def stop_child(process):
    """Stop and join the owned group before any pipes or working files disappear."""
    with defer_interrupts():
        try:
            process.drain()
            process.wait(timeout=EXIT_BOUND)
        except BaseException as error:
            # Unexpected interruption is not evidence that exit was observed.
            raise ProcessExitError(f"Owned child group {process.pid} did not settle", process=process) from error


def owned_members(session, descendants, matches):
    for stat in Path("/proc").glob("[0-9]*/stat"):
        try:
            fields = stat.read_bytes().rsplit(b")", 1)[1].split()
            if fields[0] in {b"Z", b"X", b"x"}:
                continue
            if (int(fields[3]) == session or (descendants and descendants.contains(stat.parent))) and (
                matches is None or matches(stat.parent)
            ):
                yield {"pid": int(stat.parent.name), "start_time": int(fields[19])}
        except (FileNotFoundError, ProcessLookupError):
            continue


def drain_members(session, descendants, deadline, *, matches=None):
    # Recheck the owned scope after every join, including one fresh scan after
    # an empty pass to catch a replacement forked during enumeration. Activity
    # outside the fixture must not make its cleanup depend on system load.
    empty = False
    while True:
        members = list(owned_members(session, descendants, matches))
        if not members and empty:
            return
        empty = not members
        for member in members:
            descriptor = None
            try:
                with defer_interrupts():
                    descriptor = process_descriptor(member)
                if descriptor is None:
                    continue
                try:
                    signal.pidfd_send_signal(descriptor, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                if not exit_ready(descriptor, max(0, deadline - time.monotonic())):
                    raise RuntimeError(f"Owned process {member['pid']} did not exit")
            finally:
                if descriptor is not None:
                    os.close(descriptor)
        if time.monotonic() >= deadline:
            raise RuntimeError("Owned process set did not drain")


def close_child(process):
    with defer_interrupts():
        stop_child(process)
        # Close only after exit: a pipe reader can otherwise block close indefinitely.
        failures = []
        for stream in (process.stdin, process.stdout, process.stderr):
            if stream is not None and not stream.closed:
                try:
                    stream.close()
                except (OSError, ValueError) as error:
                    failures.append(error)
        if failures:
            raise ExceptionGroup("Owned process exited, but pipe cleanup failed", failures)


def run_command(command, *, timeout, check=False, capture_output=False, input=None, **options):
    """subprocess.run's seam, with failure-safe ownership of the entire child group."""
    if capture_output:
        options.update(stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if input is not None:
        options["stdin"] = subprocess.PIPE
    with owned_child(command, **options) as process:
        try:
            stdout, stderr = process.communicate(input=input, timeout=timeout)
        except subprocess.TimeoutExpired as error:
            stop_child(process)
            try:
                stdout, stderr = process.communicate(timeout=EXIT_BOUND)
            except subprocess.TimeoutExpired as drain_error:
                raise ProcessExitError("Command output did not close after group exit") from drain_error
            raise subprocess.TimeoutExpired(command, timeout, output=stdout, stderr=stderr) from error
        result = subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
        if check:
            result.check_returncode()
        return result


def run_tests(candidate):
    # Run in this process: an outer timeout must never kill only test_driver and
    # bypass its fixture cleanup. The suite owns its own subprocesses and tempdirs.
    arguments = [str(ROOT / "test_driver.py")]
    if candidate:
        arguments += ["--artifactize", str(candidate.resolve(strict=True))]
    previous = sys.argv
    try:
        sys.argv = arguments
        runpy.run_path(str(ROOT / "test_driver.py"), run_name="__main__")
    except SystemExit as error:
        return error.code
    finally:
        sys.argv = previous
    return 0


def cases():
    with (ROOT / "cases.toml").open("rb") as stream:
        data = tomllib.load(stream)
    if data["version"] != 1:
        raise ValueError("unsupported case version")
    result = {case["id"]: case for case in data["cases"]}
    if len(result) != len(data["cases"]):
        raise ValueError("duplicate case ID")
    return result


def inside(root, relative):
    """No traversal, symlinks or host-selected absolute paths."""
    path = PurePosixPath(relative)
    if (not relative or len(relative) > 4096 or path.is_absolute()
            or "\\" in relative or ":" in relative or any(ord(char) < 32 for char in relative)):
        raise ValueError("expected a case-relative path")
    if any(part in ("", ".", "..") for part in relative.split("/")):
        raise ValueError("invalid path component")
    target = root
    for part in path.parts:
        target = target / part
        if target.is_symlink():
            raise ValueError("symlinks are not admitted")
    target.resolve().relative_to(root.resolve())
    return target


def private_directory(path):
    path.mkdir(mode=0o700, parents=True, exist_ok=True)


def materialize(destination):
    """Copy ordinary inputs to a fresh, external, private case area."""
    destination = destination.resolve()
    try:
        destination.relative_to(ROOT.parents[1])
    except ValueError:
        pass
    else:
        raise ValueError("case outputs must be outside the project")
    destination.mkdir(mode=0o700, exist_ok=False)
    try:
        for case in cases().values():
            source = inside(ROOT, case["source"])
            for path in source.rglob("*"):
                if path.is_symlink() or not (path.is_dir() or path.is_file()):
                    raise ValueError("fixtures must contain only ordinary files")
                if path.name.endswith(".artf"):
                    raise ValueError("source fixtures must not contain declarations")
            target = destination / case["id"]
            private_directory(target)
            shutil.copytree(source, target / "repo")
            private_directory(target / "state")
            private_directory(target / "evidence")
    except BaseException:
        shutil.rmtree(destination)
        raise
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="operation", required=True)
    check = commands.add_parser("check", help="Run deterministic tests, never a live builder")
    check.add_argument("--artifactize", type=Path, help="Explicit candidate binary for CLI declaration seams")
    options = parser.parse_args()
    return run_tests(options.artifactize)


if __name__ == "__main__":
    raise SystemExit(main())
