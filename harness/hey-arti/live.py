"""Manual live builder seam; never invoked by deterministic checks or CI.

All repositories, host configuration, state and transcripts are private external
outputs. Authentication is inherited normally; no credentials are copied/read.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent
sys.dont_write_bytecode = True
sys.path.insert(0, str(ROOT))
import driver


# run show --wait uses outcome exit codes even after successful observation.
TERMINAL_STATUSES = {"GREEN", "RED", "INCOMPLETE", "YELLOW", "ERROR"}
RUN_PAGE_SIZE = 100  # Read every owned Run, not just the CLI's first page.


def settle_runs(candidate, area, commands=None):
    """Cancel active fixture Runs and require an authoritative terminal snapshot."""
    if commands is None:
        commands = driver.Commands()
    with driver.defer_interrupts():
        commands.join()
        _settle_runs(candidate, area, commands.run)
        commands.join()


def _settle_runs(candidate, area, run_command):
    base = [str(candidate), "--repo", str(area / "repo"), "--state-dir", str(area / "state")]
    env = dict(os.environ, ARTIFACTIZE_REMOTE="off")
    cleanup, failures = [], []
    try:
        offset = 0
        while True:
            listing = run_command(base + ["run", "list", "--json", "--limit", str(RUN_PAGE_SIZE),
                                          "--offset", str(offset)], env=env, capture_output=True,
                                  text=True, timeout=60, check=True)
            saved_runs = json.loads(listing.stdout)
            for saved in saved_runs:
                entry = {"run": saved["id"]}
                cleanup.append(entry)
                if saved["status"] not in TERMINAL_STATUSES:
                    # A failed cancel must not prevent joining/cancelling other Runs.
                    try:
                        cancelled = run_command(base + ["run", "cancel", saved["id"]], env=env,
                                                       capture_output=True, text=True, timeout=60)
                        entry.update(cancel_exit=cancelled.returncode, cancel_stdout=cancelled.stdout,
                                     cancel_stderr=cancelled.stderr)
                        if cancelled.returncode != 0:
                            raise RuntimeError("cancellation was not accepted")
                        acknowledgement = json.loads(cancelled.stdout)
                        if acknowledgement["run"] != saved["id"] or not (
                            acknowledgement["cancellation_requested"] or
                            acknowledgement["state"] in {status.lower() for status in TERMINAL_STATUSES} | {"cancelled"}
                        ):
                            raise RuntimeError("missing cancellation acknowledgement")
                    except Exception as error:
                        entry["cancel_error"] = str(error)
                        failures.append(error)
                try:
                    settled = run_command(base + ["run", "show", saved["id"], "--wait",
                                                      "--timeout-ms", "30000"], env=env,
                                             capture_output=True, text=True, timeout=45)
                    entry.update(settled_exit=settled.returncode, settled_stdout=settled.stdout,
                                 settled_stderr=settled.stderr)
                    result = json.loads(settled.stdout)
                    entry["result"] = result
                    if result["id"] != saved["id"] or result["status"] not in TERMINAL_STATUSES:
                        raise RuntimeError("Run did not reach a terminal state")
                    # Terminal publication can precede the worker's last SQLite/
                    # notification cleanup. Observe every owned worker's actual
                    # exit, including Runs that were terminal before listing.
                    control = result.get("control") or {}
                    worker = control.get("worker") or {}
                    state = worker.get("state")
                    if state == "admitted":
                        driver.wait_for_exit(worker["owner"])
                        worker_exit = "observed"
                    elif state == "starting":
                        # This terminal Run never admitted a worker, but its launcher
                        # can still hold the fixture during the admission handoff.
                        driver.wait_for_exit(worker["launcher"])
                        worker_exit = "not_admitted"
                    else:
                        raise RuntimeError(f"Unconfirmed Run worker state {state!r}; cannot prove safe fixture cleanup")
                    if (supervisor := control.get("supervisor")) is not None:
                        driver.wait_for_exit(supervisor)
                        entry["supervisor_exit"] = "observed"
                    entry["worker_exit"] = worker_exit
                except Exception as error:
                    entry["settled_error"] = str(error)
                    failures.append(error)
            if len(saved_runs) < RUN_PAGE_SIZE:
                break
            offset += len(saved_runs)
    except Exception as error:
        cleanup.append({"listing_error": str(error)})
        failures.append(error)
    finally:
        (area / "evidence/cleanup.json").write_text(json.dumps(cleanup, indent=2))
    if failures:
        raise ExceptionGroup(f"Run cleanup failed; working files retained at {area.as_posix()}", failures)


def archive_case(area, evidence_root, records):
    archive = evidence_root / area.name
    archive.mkdir(mode=0o700)
    entry = {area.name: archive.as_posix()}
    try:
        for name in ("repo", "evidence", "state"):
            source = area / name
            if source.is_symlink():
                raise ValueError(f"Cannot archive a linked case directory: {source.as_posix()}")
            if source.exists():
                shutil.copytree(source, archive / name, symlinks=True,
                                ignore=shutil.ignore_patterns(".mcp.json", "__pycache__"))
        # Publish the new index only after all files have been copied. Until
        # then the candidate index belongs to the disposable partial archive.
        pending = archive / "index.json.pending"
        pending.write_text(json.dumps({"version": 1, "cases": records | entry}, indent=2))
        pending.replace(evidence_root / "index.json")
    except BaseException as error:
        try:
            shutil.rmtree(archive)
        except OSError as cleanup_error:
            raise BaseExceptionGroup("Could not remove unpublished archive", [error, cleanup_error]) from None
        raise
    records.update(entry)
    return archive


def release_case_tools(area):
    # Keep the recoverable case but remove host inputs that are no longer used.
    paths = [area / "host", area / "bin"]
    repo = area / "repo"
    if not repo.is_symlink():
        paths.append(repo / ".mcp.json")
    failures = []
    for path in paths:
        try:
            if not path.is_symlink() and path.is_dir():
                shutil.rmtree(path)
            else:
                path.unlink(missing_ok=True)
        except OSError as error:
            failures.append(error)
    if failures:
        raise ExceptionGroup(f"Could not remove all host inputs at {area.as_posix()}", failures)


def save_unconfirmed_diagnostics(area, evidence_root):
    # The builder or a Run may still write to the case. Publish only files the
    # harness finished writing, not a repo/state snapshot or a manifest entry.
    archive = evidence_root / area.name / "evidence"
    archive.mkdir(mode=0o700, parents=True)
    (archive / "settlement-unconfirmed.txt").write_text(
        f"Host or Run exit is unconfirmed; working files retained at {area.as_posix()}\n")
    for name in ("failure.txt", "cleanup.json"):
        source = area / "evidence" / name
        if not source.parent.is_symlink() and not source.is_symlink() and source.is_file():
            shutil.copy2(source, archive / name, follow_symlinks=False)


def run(candidate, output, model, evidence_root):
    candidate = candidate.resolve(strict=True)
    evidence_root = evidence_root.resolve()
    evidence_root.mkdir(mode=0o700, parents=True, exist_ok=False)
    working = None
    records = {}
    claimed = set()
    cases = driver.cases()

    def release_remaining():
        # Claimed cases own their own finalizers. The others never entered a host.
        with driver.defer_interrupts():
            if working.exists():
                release_unclaimed(working, cases.keys() - claimed)
            driver.child_cleanup.fixtures.discard(release_remaining)

    try:
        with driver.defer_interrupts():
            working = driver.materialize(output)
            driver.child_cleanup.fixtures.add(release_remaining)
        skill = (ROOT.parents[1] / "agent-plugin/skills/hey-arti/SKILL.md").read_text()
        version = driver.run_command([str(candidate), "--version"], capture_output=True, text=True,
                                     timeout=60, check=True).stdout.strip()
        host_version = driver.run_command(["claude", "--version"], capture_output=True, text=True,
                                          timeout=60, check=True).stdout.strip()
        for case_id, case in cases.items():
            run_case(candidate, working / case_id, case, model, skill, version, host_version,
                     evidence_root, records, claimed)
    finally:
        if working is not None:
            release_remaining()
    return evidence_root


def release_unclaimed(working, unclaimed):
    with driver.defer_interrupts():
        failures = []
        for case_id in unclaimed:
            area = working / case_id
            if area.exists():
                try:
                    shutil.rmtree(area)
                except OSError as error:
                    failures.append(error)
        if not any(working.iterdir()):
            working.rmdir()
        if failures:
            raise ExceptionGroup(f"Could not remove unclaimed cases in {working.as_posix()}", failures)


def run_case(candidate, area, case, model, skill, version, host_version, evidence_root, records, claimed):
    case_id = area.name
    builder_started = False
    detached = driver.Descendants()
    commands = driver.Commands()
    host_commands = driver.Commands()
    host_exit_error = None
    archive = None
    keep_recovery = False
    failures = []

    def settle(*, final=False):
        try:
            host_commands.join()
            if builder_started:
                # Stop submissions before listing; do not kill admitted workers
                # before the owned Runs can acknowledge cancellation and settle.
                detached.stop_mcp_submitters(mcp_command)
                settle_runs(candidate, area, commands)
        except BaseException as error:
            if final:
                unconfirmed = [error]
                # Neither a host join failure nor a CLI failure discharges
                # ownership of the other trees. Keep the case even if this
                # fallback stops every process: settlement was not confirmed.
                for stop in (commands.join, detached.finish):
                    try:
                        stop()
                    except BaseException as cleanup_error:
                        unconfirmed.append(cleanup_error)
                if len(unconfirmed) > 1:
                    raise BaseExceptionGroup("Live case cleanup failed", unconfirmed) from None
            raise
        detached.finish()

    def archive_and_release():
        nonlocal archive, keep_recovery
        if archive is None:
            try:
                archive = archive_case(area, evidence_root, records)
            except BaseException as error:
                keep_recovery = True
                error.add_note(f"Working repository, state and diagnostics retained at {area.as_posix()}")
                try:
                    release_case_tools(area)
                except Exception as cleanup_error:
                    raise BaseExceptionGroup("Archival and disposable host cleanup failed", [error, cleanup_error])
                driver.child_cleanup.fixtures.discard(retry)
                raise
        shutil.rmtree(area)
        print(f"{case_id}: evidence {archive.as_posix()}; working files removed", flush=True)

    def retry():
        with driver.defer_interrupts():
            if area.exists():
                if keep_recovery:
                    release_case_tools(area)
                    return
                if archive is None:
                    settle(final=True)
                    # Replace diagnostic-only evidence with the authoritative snapshot.
                    diagnostic_only = evidence_root / case_id
                    if diagnostic_only.exists():
                        shutil.rmtree(diagnostic_only)
                # A published archive is authoritative even if deletion failed.
                archive_and_release()
            if area.parent.exists() and not any(area.parent.iterdir()):
                area.parent.rmdir()

    try:
        with driver.defer_interrupts():
            # The shutdown registry must own cleanup before the caller stops
            # treating this case as unclaimed.
            driver.child_cleanup.fixtures.add(retry)
            claimed.add(case_id)
        repo = area / "repo"
        host = area / "host"
        host.mkdir(mode=0o700)
        tools = area / "bin"
        tools.mkdir(mode=0o700)
        executable = tools / "artifactize"
        shutil.copy2(candidate, executable)
        env = dict(os.environ, CLAUDE_CONFIG_DIR=str(host),
                   PATH=str(tools) + os.pathsep + os.environ.get("PATH", ""),
                   ARTIFACTIZE_REMOTE="off")
        env.pop("CLAUDECODE", None)
        env.pop("CLAUDE_CODE_CHILD_SESSION", None)
        env.pop("CLAUDE_CODE_SESSION_ID", None)
        mcp_env = {"ARTIFACTIZE_REMOTE": "off"}
        # The local gateway may use a nonsecret API-key placeholder. Never invent
        # credentials for a remote provider or persist the inherited credential.
        gateway = os.environ.get("ANTHROPIC_BASE_URL", "")
        if gateway.startswith(("http://127.0.0.1:", "http://localhost:")):
            env.setdefault("ANTHROPIC_API_KEY", "local-proxy")
            mcp_env["ARTIFACTIZE_ANTHROPIC_BASE_URL"] = gateway
            mcp_env["ANTHROPIC_API_KEY"] = "local-proxy"
        command = ["claude", "mcp", "add", "artifactize", "--transport", "stdio", "--scope", "project"]
        for key, value in mcp_env.items():
            command.extend(["--env", f"{key}={value}"])
        mcp_command = [str(executable), "--state-dir", str(area / "state"), "mcp"]
        command.extend(["--", *mcp_command])
        try:
            registration = host_commands.run(command, cwd=repo, env=env, capture_output=True, text=True, timeout=60)
        except subprocess.TimeoutExpired as error:
            # TimeoutExpired may carry bytes even when text=True.
            captured = [value.decode(errors="replace") if isinstance(value, bytes) else value or ""
                        for value in (error.stdout, error.stderr)]
            (area / "evidence/registration.txt").write_text("".join(captured))
            raise
        (area / "evidence/registration.txt").write_text(registration.stdout + registration.stderr)
        if registration.returncode:
            raise RuntimeError(f"registration failed for {case_id}")
        # Accept the project server through the host's documented trust setting,
        # and allow only ordinary builder tools plus this server, never bypass.
        settings = {"enabledMcpjsonServers": ["artifactize"]}
        prompt = f"""You are the actual builder in a private hey-arti harness fixture.
Use the canonical skill below. The agreed proposal/consent is already supplied,
so apply it without requesting a new Human authorization. Repository: {repo.as_posix()}.
Candidate: {candidate.as_posix()}; model available for Agent evals: anthropic/{model}.
No plugin packaging. Leave speculate off for these fixture checks; preserve the agreed Human boundary.
MCP registration must supply all 20 tools; demonstrate config_check, config_graph (including impact) and status.
Use verify_watch for active machine work if the host exposes progress notifications;
otherwise bounded verify_wait is the supported fallback. Inspect runtime evidence
with request_logs (tail/range/search) when diagnosing a failed runtime eval.
A live update or one eval result is not the Run final validation outcome.
Read tools return the CLI JSON in entries.text; append pages then parse JSON.
Do not inspect any credentials or change anything outside this private case.
Use MCP first; do not execute project programs except through runtime evals/owned
Agent tool sessions. Shell may inspect/edit files and create the read-only-token CI.
Case goals: {json.dumps(case)}.
Establish the declarations, run a baseline via verify_start, and keep real saved
Run evidence. Demonstrate unchanged reuse, an independent edit, the prescribed
local edit ({case['edit_path']}: {case['edit_before']!r} -> {case['edit_after']!r}),
a judgment change, and the agreed evolution/split, observing statuses and actual
Run outcomes. Repair runtime expectations (not behavior) if the local edit needs it.
Include meaningful runtime checks for behavior. If an Agent review is needed,
use the real backend/model above and narrow tools, not simulated verdicts.
For the monorepo preserve the existing Human gate; exercise the session-bound
preview with a real Agent review and artifactize-owned cleanup. Once only Human
requests remain, record compact handoff and yield; NEVER claim/submit/release a
Human request or forge evidence. Pending Human is expected, not a failure. You may
submit independent scoped Runs for reuse/evolution checks before the final handoff.
Do not wait indefinitely: wait on active machine work with verify_wait; on Human
handoff inspect request_list then yield. Record the Run IDs, statuses, request IDs,
reuse comparisons, changed declarations, CI and cleanup evidence in
{(repo / 'builder-evidence.json').as_posix()} (valid JSON, no invented success).
Finish with concise limitations if anything could not be demonstrated.

CANONICAL SKILL
{skill}
"""
        (area / "evidence/prompt.txt").write_text(prompt)
        with (area / "evidence/transcript.jsonl").open("w") as transcript, (area / "evidence/stderr.txt").open("w") as errors:
            builder_started = True
            result = host_commands.run(["claude", "-p", "--model", model, "--verbose", "--output-format", "stream-json",
                                     "--no-session-persistence", "--setting-sources", "", "--settings", json.dumps(settings),
                                     "--strict-mcp-config", "--mcp-config", str(repo / ".mcp.json"),
                                     "--allowedTools", "Read", "Write", "Edit", "Glob", "Grep", "mcp__artifactize__*", "--", prompt],
                                    cwd=repo, env=env, stdout=transcript, stderr=errors, timeout=3600,
                                    detached=detached)
        record = {"mode": "live", "host": host_version, "model": model, "candidate": version,
                  "candidate_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
                  "exit_code": result.returncode, "case": case_id,
                  "summary_available": (repo / "builder-evidence.json").is_file(),
                  "transcript": "evidence/transcript.jsonl", "summary": "builder-evidence.json"}
        (area / "evidence/live.json").write_text(json.dumps(record, indent=2))
    except BaseException as error:
        if isinstance(error, driver.ProcessExitError):
            host_exit_error = error
        failures.append(error)
        (area / "evidence/failure.txt").write_text(f"{type(error).__name__}: {error}\n")
    finally:
        if case_id in claimed:
            with driver.defer_interrupts():
                try:
                    if host_exit_error is not None:
                        raise host_exit_error
                    settle()
                except BaseException as error:
                    if error is not host_exit_error:
                        failures.append(error)
                    try:
                        save_unconfirmed_diagnostics(area, evidence_root)
                        print(f"{case_id}: settlement unconfirmed; working files retained at {area.as_posix()}", flush=True)
                    except Exception as diagnostic_error:
                        failures.append(diagnostic_error)
                else:
                    try:
                        archive_and_release()
                    except BaseException as error:
                        failures.append(error)
                    else:
                        driver.child_cleanup.fixtures.discard(retry)
    if failures:
        if len(failures) == 1 and isinstance(failures[0], driver.Terminated):
            raise failures[0]
        raise BaseExceptionGroup(f"Live fixture failed; evidence at {(evidence_root / case_id).as_posix()}", failures)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifactize", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--model", default="ocx-claude-native--gpt-6.2-sol")
    parser.add_argument("--evidence-root", required=True, type=Path)
    args = parser.parse_args()
    try:
        with driver.term_as_interrupt():
            run(args.artifactize.resolve(strict=True), args.output, args.model, args.evidence_root)
    except driver.Terminated as error:
        raise SystemExit(error.exit_code) from None
