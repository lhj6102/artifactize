"""Stdlib-only hey-arti fixture driver.

Source fixtures never contain declarations. Live builders run manually outside CI.
"""

import argparse
import atexit
from contextlib import contextmanager
import json
import os
from pathlib import Path, PurePosixPath
import runpy
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time
import tomllib
import weakref

ROOT = Path(__file__).resolve().parent
sys.dont_write_bytecode = True
sys.path.insert(0, str(ROOT))
import process_owner

EXIT_BOUND = process_owner.EXIT_BOUND
process_descriptor = process_owner.process_descriptor
exit_ready = process_owner.exit_ready


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


class CommandPipes(subprocess.Popen):
    """Use Popen's existing pipe I/O, but wait for the command, not its reaper."""

    def _wait(self, timeout):
        return self.command.wait(timeout=timeout)

    def join_reaper(self):
        if super()._wait(EXIT_BOUND) != 0:
            raise RuntimeError("Fixture subreaper exited without confirming cleanup")


class OwnedChild:
    """The command's per-launch subreaper guarantees ownership across fork and exit."""

    def __init__(self, command, *, detached=None, **options):
        self.args = command
        self.returncode = None
        self.pid = None
        self.descriptor = None
        self.channel = None
        self.pending = None
        self.deferred = detached is not None
        # Bound control I/O independently of the application's timeout.
        inherited = options.pop("pass_fds", ())
        parent, child = socket.socketpair(type=socket.SOCK_SEQPACKET)
        with parent as channel, child:
            channel.settimeout(EXIT_BOUND)
            self.pipes = CommandPipes(
                [sys.executable, "-I", process_owner.__file__, str(child.fileno()), json.dumps(inherited),
                 "deferred" if detached is not None else "immediate", *command],
                pass_fds=(*inherited, child.fileno()), start_new_session=True, **options)
            self.pipes.command = weakref.proxy(self)
            child.close()
            try:
                reply = process_owner.receive(channel)
                if reply is None:
                    raise RuntimeError("Fixture subreaper did not announce a command")
                if "launch_error" in reply:
                    raise OSError(reply["errno"], reply["launch_error"])
                self.pid = reply["pid"]
                self.descriptor = os.pidfd_open(self.pid)
                self.channel = channel.dup()
                if detached is not None:
                    detached.children.add(self)
            except BaseException:
                # EOF asks the reaper to drain even if startup failed after exec.
                channel.close()
                if self.channel is not None:
                    self.channel.close()
                    self.channel = None
                try:
                    self.drain()
                except BaseException as cleanup_error:
                    child_cleanup.add(self)
                    raise ProcessExitError("Fixture launch cleanup was not confirmed", process=self) from cleanup_error
                for stream in (self.stdin, self.stdout, self.stderr):
                    if stream is not None:
                        stream.close()
                raise

    @property
    def stdin(self):
        return self.pipes.stdin

    @stdin.setter
    def stdin(self, stream):
        self.pipes.stdin = stream

    @property
    def stdout(self):
        return self.pipes.stdout

    @property
    def stderr(self):
        return self.pipes.stderr

    def communicate(self, **options):
        return self.pipes.communicate(**options)

    def _reply(self):
        reply = process_owner.receive(self.channel)
        if reply is None:
            raise RuntimeError("Fixture subreaper did not confirm cleanup")
        operation, self.pending = self.pending[0], None
        if "error" in reply:
            raise RuntimeError(reply["error"])
        if operation in {"stop", "finish"}:
            self.returncode = reply["result"]
            if self.descriptor is not None:
                os.close(self.descriptor)
                self.descriptor = None
        if operation == "finish" or (operation == "stop" and not self.deferred):
            self.channel.close()
            self.channel = None

    def request(self, *request):
        with defer_interrupts():
            previous = self.pending
            if previous is not None:
                try:
                    self._reply()
                except RuntimeError:
                    if self.pending is not None or previous == request:
                        raise
                if previous == request:
                    return
            if self.channel is not None:
                process_owner.send(self.channel, request)
                self.pending = request
                self._reply()

    def drain(self):
        if self.channel is not None and self.returncode is None:
            self.request("stop")
        if self.channel is None:
            self.pipes.join_reaper()
            if self.descriptor is not None:
                os.close(self.descriptor)
                self.descriptor = None

    def finish(self):
        self.request("finish")
        self.pipes.join_reaper()

    def wait(self, timeout=None):
        if self.returncode is None and self.descriptor is not None and self.pending is None:
            if not exit_ready(self.descriptor, None if timeout is None else max(0, timeout)):
                raise subprocess.TimeoutExpired(self.args, timeout)
        self.drain()
        return self.returncode


class Descendants:
    """Keep deferred subtrees owned while the caller settles admitted Runs."""

    def __init__(self):
        self.children = set()

    def stop_mcp_submitters(self, command):
        self._visit(lambda process: process.request("submitters", *command))

    def finish(self):
        def finish(process):
            process.finish()
            self.children.discard(process)

        self._visit(finish)

    def _visit(self, visit):
        with defer_interrupts():
            failures = []
            for process in tuple(self.children):
                try:
                    visit(process)
                except BaseException as error:
                    failures.append(error)
            if failures:
                raise ProcessExitError("Detached fixture processes did not settle") from BaseExceptionGroup(
                    "Detached process cleanup failed", failures)


def require_pidfd():
    # Probe the calling process: no fixture may start until its owner can
    # open, signal and observe pidfds. Signal 0 checks support without delivery.
    try:
        with defer_interrupts():
            descriptor = os.pidfd_open(os.getpid())
            try:
                signal.pidfd_send_signal(descriptor, 0)
                exit_ready(descriptor, 0)
            finally:
                os.close(descriptor)
    except (AttributeError, OSError) as error:
        raise RuntimeError("The hey-arti harness requires Linux pidfd open, signal and exit observation support") from error


def spawn(command, *, detached=None, **options):
    """Launch only after proving Linux pidfd support in the harness."""
    if sys.platform != "linux":
        raise RuntimeError("The hey-arti harness runs on Linux only.")
    require_pidfd()
    return OwnedChild(command, detached=detached, **options)


class ChildCleanup:
    """Own unreleased children, including their pipes, until shutdown retries."""

    def __init__(self):
        self.pending = set()
        self.fixtures = set()

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
        for release in tuple(self.fixtures):
            try:
                release()
                self.fixtures.discard(release)
            except BaseException as error:
                print(f"Could not release fixture at shutdown: {error}", file=sys.stderr)


child_cleanup = ChildCleanup()
atexit.register(child_cleanup.retry)


@contextmanager
def owned_child(command, *, on_spawn=None, **options):
    process = None
    try:
        try:
            with defer_interrupts():
                try:
                    process = spawn(command, **options)
                except ProcessExitError as error:
                    # Even a launch that never completed can own a subprocess.
                    # Publish its handle before restoring interrupt handlers.
                    if error.process is not None:
                        child_cleanup.add(error.process)
                        if on_spawn is not None:
                            on_spawn(error.process)
                    raise
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
                raise ProcessExitError("Command output did not close after group exit", process=process) from drain_error
            raise subprocess.TimeoutExpired(command, timeout, output=stdout, stderr=stderr) from error
        result = subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
        if check:
            result.check_returncode()
        return result


class Commands:
    """Retain commands that might still be using the caller's private inputs."""

    def __init__(self):
        self.pending = set()
        self.unidentified = []

    @contextmanager
    def track(self):
        spawned = None

        def register(process):
            nonlocal spawned
            spawned = process
            self.pending.add(process)

        try:
            yield register
        except ProcessExitError as error:
            if error.process is not None:
                self.pending.add(error.process)
            elif spawned is None:
                self.unidentified.append(error)
            raise
        else:
            self.pending.discard(spawned)

    def run(self, command, **options):
        with self.track() as register:
            return run_command(command, on_spawn=register, **options)

    @contextmanager
    def child(self, command, **options):
        with self.track() as register, owned_child(command, on_spawn=register, **options) as process:
            yield process

    def join(self):
        failures = list(self.unidentified)
        for process in tuple(self.pending):
            try:
                child_cleanup.close(process)
                self.pending.remove(process)
            except BaseException as error:
                failures.append(error)
        if failures:
            raise BaseExceptionGroup("Fixture commands did not settle", failures)


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
    # The in-process tests share the fixture retry registry.
    sys.modules["driver"] = sys.modules[__name__]
    raise SystemExit(main())
