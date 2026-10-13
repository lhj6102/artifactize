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


def run(candidate, output, model, evidence_root):
    evidence_root.mkdir(mode=0o700, parents=True, exist_ok=False)
    output = driver.materialize(output)
    skill = (ROOT.parents[1] / "crates/artifactize-core/integrations/hey-arti/SKILL.md").read_text()
    version = subprocess.check_output([str(candidate), "--version"], text=True).strip()
    host_version = subprocess.check_output(["claude", "--version"], text=True).strip()
    records = {}
    for case_id, case in driver.cases().items():
        area = output / case_id
        repo = area / "repo"
        host = area / "host"
        host.mkdir(mode=0o700)
        tools = area / "bin"
        tools.mkdir(mode=0o700)
        shutil.copy2(candidate, tools / candidate.name)
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
        command.extend(["--", "artifactize", "--state-dir", str(area / "state"), "mcp"])
        registration = subprocess.run(command, cwd=repo, env=env, capture_output=True, text=True, timeout=60)
        (area / "evidence/registration.txt").write_text(registration.stdout + registration.stderr)
        if registration.returncode:
            raise RuntimeError(f"registration failed for {case_id}")
        # Accept the project server through the host's documented trust setting,
        # and allow only ordinary builder tools plus this server, never bypass.
        settings = {"enabledMcpjsonServers": ["artifactize"]}
        prompt = f"""You are the actual builder in a private hey-arti harness fixture.
Use the canonical skill below. The agreed proposal/consent is already supplied,
so apply it without requesting a new Human authorization. Repository: {repo}.
Candidate: {candidate}; model available for Agent evals: anthropic/{model}.
No plugin packaging. Do not use speculate/defer_human (this candidate rejects true).
MCP registration must supply all 17 tools; demonstrate config_check and status.
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
{repo / 'builder-evidence.json'} (valid JSON, no invented success).
Finish with concise limitations if anything could not be demonstrated.

CANONICAL SKILL
{skill}
"""
        (area / "evidence/prompt.txt").write_text(prompt)
        with (area / "evidence/transcript.jsonl").open("w") as transcript, (area / "evidence/stderr.txt").open("w") as errors:
            result = subprocess.run(["claude", "-p", "--model", model, "--verbose", "--output-format", "stream-json",
                                     "--no-session-persistence", "--setting-sources", "", "--settings", json.dumps(settings),
                                     "--strict-mcp-config", "--mcp-config", str(repo / ".mcp.json"),
                                     "--allowedTools", "Read", "Write", "Edit", "Glob", "Grep", "mcp__artifactize__*", "--", prompt],
                                    cwd=repo, env=env, stdout=transcript, stderr=errors, timeout=3600)
        record = {"mode": "live", "host": host_version, "model": model, "candidate": version,
                  "candidate_sha256": hashlib.sha256((tools / candidate.name).read_bytes()).hexdigest(),
                  "exit_code": result.returncode, "case": case_id,
                  "summary_available": (repo / "builder-evidence.json").is_file(),
                  "transcript": "evidence/transcript.jsonl", "summary": "builder-evidence.json"}
        (area / "evidence/live.json").write_text(json.dumps(record, indent=2))
        # Only the harness driver (not the builder) explicitly cancels leftover
        # private fixture Runs after preserving the handoff. Never sign off.
        base = [str(candidate), "--repo", str(repo), "--state-dir", str(area / "state")]
        listing = subprocess.run(base + ["run", "list", "--json"], env=env, capture_output=True, text=True, timeout=60)
        cleanup = []
        if listing.returncode == 0:
            for saved in json.loads(listing.stdout):
                if saved["status"] == "RUNNING":
                    cancelled = subprocess.run(base + ["run", "cancel", saved["id"]], env=env, capture_output=True, text=True, timeout=60)
                    settled = subprocess.run(base + ["run", "show", saved["id"], "--wait", "--timeout-ms", "30000"], env=env, capture_output=True, text=True, timeout=45)
                    cleanup.append({"run": saved["id"], "cancel_exit": cancelled.returncode, "settled_exit": settled.returncode,
                                    "result": json.loads(settled.stdout) if settled.stdout.strip() else None})
        (area / "evidence/cleanup.json").write_text(json.dumps(cleanup, indent=2))
        archive = evidence_root / case_id
        archive.mkdir(mode=0o700)
        for name in ("repo", "evidence", "state"):
            source = area / name
            if source.exists():
                shutil.copytree(source, archive / name, ignore=shutil.ignore_patterns(".mcp.json", "__pycache__"))
        records[case_id] = str(archive)
        # Publish availability after each completed host, even when later hosts fail.
        manifest = json.dumps({"version": 1, "cases": records}, indent=2)
        (evidence_root / "index.json").write_text(manifest)
        (ROOT / "live-evidence.json").write_text(manifest)
        print(f"{case_id}: host exit {result.returncode}; evidence {area / 'evidence'}", flush=True)
    shutil.rmtree(output)
    return evidence_root


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifactize", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--model", default="ocx-claude-native--gpt-6.1-sol")
    parser.add_argument("--evidence-root", required=True, type=Path)
    args = parser.parse_args()
    run(args.artifactize.resolve(strict=True), args.output, args.model, args.evidence_root)
