"""Stdlib-only hey-arti fixture and review-tool driver.

Source fixtures never contain declarations. Live builders run manually outside CI;
review sessions import their actual outputs, never substitute deterministic evidence.
"""

import argparse
import json
import os
from pathlib import Path, PurePosixPath
import runpy
import selectors
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent
# Keep command-tool responses comfortably below the MCP/reviewer byte cap.
PAGE_BYTES = 16 * 1024
RESPONSE_BYTES = 48 * 1024
REQUEST_BYTES = 64 * 1024
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

    Windows is refused: once a leader exits, nothing here would still own its
    descendants, so their working files could not be released safely."""
    if os.name != "posix":
        raise RuntimeError("The hey-arti harness runs on Linux and macOS only.")
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


def _stop_child(process):
    owners = []
    if sys.platform == "linux":
        for stat in Path("/proc").glob("[0-9]*/stat"):
            try:
                fields = stat.read_text().rsplit(")", 1)[1].split()
                if int(fields[2]) == process.pid and fields[0] not in {"Z", "X", "x"}:
                    owners.append({"pid": int(stat.parent.name), "start_time": int(fields[19])})
            except (FileNotFoundError, ProcessLookupError):
                continue
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait(timeout=EXIT_BOUND)
    for owner in owners:
        wait_for_exit(owner)


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


def envelope():
    raw = sys.stdin.buffer.read(REQUEST_BYTES + 1)
    if len(raw) > REQUEST_BYTES:
        raise ValueError("request exceeds the tool envelope limit")
    request = json.loads(raw)
    if request.get("version") != 1 or not isinstance(request.get("args"), dict):
        raise ValueError("expected a version-1 JSON tool envelope")
    return request["args"]


def session_directory():
    directory = Path(os.environ["ARTIFACTIZE_SESSION_DIR"]).resolve(strict=True)
    if not directory.is_dir():
        raise ValueError("missing artifactize session directory")
    return directory


def emit(data=None, error=None):
    if error is None:
        result = {"content": [{"type": "json", "data": data}]}
    else:
        result = {"isError": True, "content": [{"type": "text", "text": error}]}
    encoded = json.dumps(result, ensure_ascii=True, separators=(",", ":"))
    if len(encoded.encode("utf-8")) > RESPONSE_BYTES:
        raise ValueError("response exceeds the reviewer limit; narrow the request")
    print(encoded)


def area(directory):
    data = json.loads((directory / "session.json").read_text(encoding="utf-8"))
    root = Path(data["area"])
    # Only the area allocated by start can be read. The metadata isn't a path API.
    if root.parent != directory or not root.name.startswith("cases-"):
        raise ValueError("invalid session case area")
    if root.is_symlink() or not root.is_dir():
        raise ValueError("session case area is unavailable")
    return root


def case_result(directory, case_id):
    case = cases().get(case_id)
    if case is None:
        raise ValueError("unknown case")
    root = area(directory) / case_id
    source = inside(ROOT, case["source"])
    files = sorted(path.relative_to(source).as_posix() for path in source.rglob("*") if path.is_file())
    # No successful fake-host record is ever promoted to live evidence.
    evidence_path = root / "evidence" / "live.json"
    evidence = {"available": False, "reason": "Live builder pass has not been wired/run."}
    if evidence_path.is_file():
        if evidence_path.is_symlink() or evidence_path.stat().st_size > PAGE_BYTES:
            raise ValueError("invalid or oversized live evidence")
        record = json.loads(evidence_path.read_text(encoding="utf-8"))
        if record.get("mode") != "live":
            raise ValueError("deterministic evidence cannot stand in for a live builder pass")
        evidence = {"available": True, "record": record}
    return {"case": case_id, "goal": case["goal"], "consent": case["consent"],
            "obligations": case["obligations"], "evolve": case["evolve"],
            "repository": (root / "repo").as_posix(), "source_files": files, "builder_files": sorted(path.relative_to(root / "repo").as_posix() for path in (root / "repo").rglob("*") if path.is_file() and not any(part.startswith(".") for part in path.relative_to(root / "repo").parts)), "live_evidence": evidence}


def read_result(directory, case_id, relative, offset=0, limit=PAGE_BYTES):
    if case_id not in cases():
        raise ValueError("unknown case")
    if type(offset) is not int or offset < 0 or type(limit) is not int or not 1 <= limit <= PAGE_BYTES:
        raise ValueError("invalid byte range")
    if not isinstance(relative, str):
        raise ValueError("expected a source path")
    # CI workflow declarations are review inputs; other hidden/credential files aren't.
    workflow = relative.startswith(".github/workflows/") and relative.endswith((".yml", ".yaml"))
    components = PurePosixPath(relative).parts
    if any(part.startswith(".") for part in (components[1:] if workflow else components)):
        raise ValueError("hidden files are not reviewer inputs")
    root = area(directory) / case_id / "repo"
    path = inside(root, relative)
    if not path.is_file():
        raise ValueError("expected a regular case file")
    with path.open("rb") as stream:
        size = path.stat().st_size
        if offset > size:
            raise ValueError("offset past end of file")
        stream.seek(offset)
        raw = stream.read(limit + 1)
    if b"\x00" in raw:
        raise ValueError("binary inputs are not supported by read")
    page = raw[:limit]
    # An offset must begin at a UTF-8 boundary; trim only an incomplete final codepoint.
    while True:
        try:
            text = page.decode("utf-8")
            break
        except UnicodeDecodeError as error:
            if error.reason != "unexpected end of data" or error.end != len(page):
                raise ValueError("input/range is not valid UTF-8") from error
            page = page[:error.start]
    if raw and not page:
        raise ValueError("limit is too small for one UTF-8 character")
    next_offset = offset + len(page)
    return {"case": case_id, "path": relative, "offset": offset, "text": text,
            "next_offset": next_offset if next_offset < size else None}


def import_evidence(destination):
    """Import manual-host output before readiness; reviewers cannot choose a host path."""
    index = Path(os.environ.get("HEY_ARTI_EVIDENCE_INDEX", ROOT / "live-evidence.json"))
    if not index.exists():
        return
    if index.is_symlink() or index.stat().st_size > PAGE_BYTES:
        raise ValueError("invalid live evidence index")
    manifest = json.loads(index.read_text(encoding="utf-8"))
    if manifest.get("version") != 1 or not isinstance(manifest.get("cases"), dict):
        raise ValueError("invalid live evidence manifest")
    for case_id, location in manifest["cases"].items():
        if case_id not in cases():
            raise ValueError("unknown live evidence case")
        source = Path(location).resolve(strict=True)
        record_path = source / "evidence/live.json"
        if record_path.is_symlink() or record_path.stat().st_size > PAGE_BYTES:
            raise ValueError("invalid live evidence record")
        record = json.loads(record_path.read_text(encoding="utf-8"))
        if record.get("mode") != "live" or record.get("case") != case_id:
            raise ValueError("deterministic evidence cannot stand in for a live builder pass")
        target = destination / case_id
        shutil.rmtree(target / "repo")
        private_directory(target / "repo")
        for path in (source / "repo").rglob("*"):
            relative = path.relative_to(source / "repo").as_posix()
            parts = PurePosixPath(relative).parts
            if any(part.startswith(".") or part == "__pycache__" for part in parts):
                if not (relative.startswith(".github/workflows/") and relative.endswith((".yml", ".yaml"))):
                    continue
            if path.is_symlink():
                raise ValueError("live builder inputs must not contain symlinks")
            if path.is_file():
                output = inside(target / "repo", relative)
                output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
                shutil.copyfile(path, output)
        # Expose only known evidence products, not host configuration/state/credentials.
        private_directory(target / "repo/evidence")
        for name in ("transcript.jsonl", "registration.txt", "live.json", "cleanup.json"):
            input_path = source / "evidence" / name
            if input_path.is_symlink() or not input_path.is_file():
                raise ValueError("missing regular live evidence product")
            shutil.copyfile(input_path, target / "repo/evidence" / name)
        for name in ("transcript.jsonl", "live.json", "cleanup.json", "runs-before-cleanup.json"):
            followup = source / "evidence/inspection-recheck" / name
            if followup.exists():
                if followup.is_symlink() or not followup.is_file():
                    raise ValueError("invalid inspection follow-up product")
                private_directory(target / "repo/evidence/inspection-recheck")
                shutil.copyfile(followup, target / "repo/evidence/inspection-recheck" / name)
        shutil.copyfile(record_path, target / "evidence/live.json")


def start(directory):
    # Allocate below the artifactize-owned private session directory, never /tmp's root.
    parent = Path(tempfile.mkdtemp(prefix="cases-", dir=directory))
    parent.rmdir()
    case_area = materialize(parent)
    try:
        import_evidence(case_area)
        with socket.socket() as server:
            server.bind(("127.0.0.1", 0))
            server.listen()
            (directory / "session.json").write_text(json.dumps({
                "area": case_area.as_posix(), "port": server.getsockname()[1],
            }), encoding="utf-8")
            # Cases and endpoint are ready before artifactize sees this newline.
            print("ready", flush=True)
            with server.accept()[0] as client:
                client.settimeout(5)
                if client.recv(16) != b"stop":
                    raise ValueError("invalid session stop request")
    finally:
        shutil.rmtree(case_area)
        (directory / "session.json").unlink(missing_ok=True)


def stop(directory):
    metadata = directory / "session.json"
    if not metadata.exists():
        return
    data = json.loads(metadata.read_text(encoding="utf-8"))
    with socket.create_connection(("127.0.0.1", data["port"]), timeout=5) as client:
        client.sendall(b"stop")
    # Artifactize joins the start process and owns final directory/process cleanup.


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="operation", required=True)
    check = commands.add_parser("check", help="Run deterministic tests, never a live builder")
    check.add_argument("--artifactize", type=Path, help="Explicit candidate binary for CLI declaration seams")
    for operation in ("start", "case", "read", "stop"):
        commands.add_parser(operation)
    options = parser.parse_args()
    if options.operation == "check":
        return run_tests(options.artifactize)
    try:
        args = envelope()
        directory = session_directory()
        if options.operation == "start":
            if args:
                raise ValueError("start accepts no reviewer arguments")
            start(directory)
        elif options.operation == "stop":
            if args:
                raise ValueError("stop accepts no reviewer arguments")
            stop(directory)
        elif options.operation == "case":
            if set(args) != {"case"}:
                raise ValueError("case requires only a case ID")
            emit(case_result(directory, args["case"]))
        elif options.operation == "read":
            if not {"case", "path"} <= args.keys() or not args.keys() <= {"case", "path", "offset", "limit"}:
                raise ValueError("invalid read arguments")
            emit(read_result(directory, args["case"], args["path"], args.get("offset", 0), args.get("limit", PAGE_BYTES)))
    except (OSError, ValueError, KeyError, TypeError) as error:
        if options.operation in ("case", "read"):
            # Don't echo host paths, request values or possibly credential-bearing exception text.
            emit(error="Harness input unavailable or invalid; use a listed case and a narrow source path.")
            return 0
        print(f"Harness {options.operation} failed ({type(error).__name__}).", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
