"""Stdlib-only hey-arti fixture driver.

Source fixtures never contain declarations. Live builders run manually outside CI.
"""

import argparse
import os
from pathlib import Path, PurePosixPath
import runpy
import selectors
import shutil
import signal
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parent
# Cleanup watchdog only; no caller or suite timeout determines an owned lifetime.
EXIT_BOUND = 30


def process_descriptor(owner):
    """Pin a saved Linux process identity, not an unrelated reused PID."""
    if not hasattr(os, "pidfd_open"):
        raise RuntimeError("Owned worker exit observation requires Linux pidfd support")
    try:
        descriptor = os.pidfd_open(owner["pid"])
    except ProcessLookupError:
        return None
    try:
        fields = Path(f"/proc/{owner['pid']}/stat").read_text().rsplit(")", 1)[1].split()
        # procfs starttime is field 22; the tail after the name starts at field 3.
        same_process = int(fields[19]) == owner["start_time"]
        if same_process and fields[0] not in {"Z", "X", "x"}:
            return descriptor
    except FileNotFoundError:
        pass
    except BaseException:
        os.close(descriptor)
        raise
    os.close(descriptor)
    return None


def wait_for_exit(owner):
    descriptor = process_descriptor(owner)
    if descriptor is None:
        return
    try:
        with selectors.DefaultSelector() as observation:
            observation.register(descriptor, selectors.EVENT_READ)
            if not observation.select(EXIT_BOUND):
                raise RuntimeError(f"Owned process {owner['pid']} did not exit; preserve its working files")
    finally:
        os.close(descriptor)


def spawn(command, **options):
    """Each harness subprocess owns a fresh process group, including its children.

    Only Linux is supported: descendants are observed through /proc, so elsewhere
    nothing here would still own them once the leader exits, and their working
    files could not be released safely."""
    if sys.platform != "linux":
        raise RuntimeError("The hey-arti harness runs on Linux only.")
    options["start_new_session"] = True
    return subprocess.Popen(command, **options)


class ProcessExitError(RuntimeError):
    """An owned process may still use its working files; retain them."""


def stop_child(process):
    """Stop and join the owned group before any pipes or working files disappear."""
    try:
        _stop_child(process)
    except Exception as error:
        raise ProcessExitError(f"Owned child group {process.pid} did not settle") from error


def group_members(group):
    """The live members of a process group, by saved identity."""
    members = []
    for stat in Path("/proc").glob("[0-9]*/stat"):
        try:
            fields = stat.read_text().rsplit(")", 1)[1].split()
            if int(fields[2]) == group and fields[0] not in {"Z", "X", "x"}:
                members.append({"pid": int(stat.parent.name), "start_time": int(fields[19])})
        except (FileNotFoundError, ProcessLookupError):
            continue
    return members


def _stop_child(process):
    # A member forked while the group was scanned is still in it: signal and scan again
    # until no member is left, so none outlives teardown.
    while True:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=EXIT_BOUND)
        members = group_members(process.pid)
        if not members:
            return
        for member in members:
            wait_for_exit(member)


def close_child(process):
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
    process = spawn(command, **options)
    try:
        try:
            stdout, stderr = process.communicate(input=input, timeout=timeout)
        except subprocess.TimeoutExpired as error:
            stop_child(process)
            stdout, stderr = process.communicate()
            raise subprocess.TimeoutExpired(command, timeout, output=stdout, stderr=stderr) from error
        result = subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
        if check:
            result.check_returncode()
        return result
    finally:
        close_child(process)


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
    destination.mkdir(mode=0o700, parents=True, exist_ok=False)
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
