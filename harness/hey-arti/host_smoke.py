"""Manual B2 host registration/tool-list smoke in disposable configuration."""
import argparse
import json
import os
from pathlib import Path
import selectors
import shutil
import subprocess
import sys
import time

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import driver

TOOLS = {"verify_start", "verify_status", "verify_wait", "verify_result", "verify_cancel",
         "status", "config_check", "run_list", "run_show", "request_list", "request_show",
         "session_show", "tools_check", "doctor", "models", "remote_status", "cache_show"}


def capture(command, repo, env, timeout, stdout_path, stderr_path=None):
    """Keep host diagnostics even when registration or inventory fails."""
    try:
        result = driver.run_command(command, cwd=repo, env=env, capture_output=True,
                                text=True, timeout=timeout)
    except subprocess.TimeoutExpired as error:
        stdout, stderr = (value.decode(errors="replace") if isinstance(value, bytes) else value or ""
                          for value in (error.stdout, error.stderr))
        stdout_path.write_text(stdout if stderr_path else stdout + stderr)
        if stderr_path:
            stderr_path.write_text(stderr)
        raise
    stdout_path.write_text(result.stdout if stderr_path else result.stdout + result.stderr)
    if stderr_path:
        stderr_path.write_text(result.stderr)
    result.check_returncode()
    return result


def smoke(host, candidate, output, model):
    candidate = candidate.resolve(strict=True)
    output = output.resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    stopped = True
    try:
        inventory(host, candidate, output, model)
    except driver.ProcessExitError:
        stopped = False
        raise
    finally:
        # Retain inputs if a host group cannot be joined; evidence is never deleted.
        if stopped:
            for name in ("repo", "config", "bin", "state"):
                path = output / name
                if path.exists():
                    shutil.rmtree(path)


def inventory(host, candidate, output, model):
    repo = output / "repo"
    repo.mkdir(mode=0o700)
    (repo / "index.artf").write_text('name = "smoke"\nbasis = true\n')
    config = output / "config"
    config.mkdir(mode=0o700)
    binary = output / "bin"
    binary.mkdir(mode=0o700)
    executable = binary / "artifactize"
    shutil.copy2(candidate, executable)
    env = dict(os.environ, PATH=str(binary) + os.pathsep + os.environ.get("PATH", ""), ARTIFACTIZE_REMOTE="off")
    env.pop("CLAUDECODE", None)
    env.pop("CLAUDE_CODE_CHILD_SESSION", None)
    env.pop("CLAUDE_CODE_SESSION_ID", None)
    env["CLAUDE_CONFIG_DIR" if host == "claude" else "CODEX_HOME"] = str(config)
    args = [host, "mcp", "add"]
    if host == "claude":
        args += ["--transport", "stdio", "--scope", "project"]
    args += ["artifactize", "--", str(executable), "--state-dir", str(output / "state"), "mcp"]
    capture(args, repo, env, 60, output / "registration.txt")
    capture([host, "mcp", "get", "artifactize"], repo, env, 60, output / "get.txt")
    if host == "claude":
        gateway = env.get("ANTHROPIC_BASE_URL", "")
        if gateway.startswith(("http://127.0.0.1:", "http://localhost:")):
            env.setdefault("ANTHROPIC_API_KEY", "local-proxy")
        result = capture([host, "-p", "--model", model, "--verbose", "--output-format", "stream-json",
                                 "--no-session-persistence", "--setting-sources", "", "--tools", "",
                                 "--strict-mcp-config", "--mcp-config", str(repo / ".mcp.json"), "--",
                                 "Do not call tools or edit anything. List the names of all artifactize MCP tools you have."],
                         repo, env, 180, output / "transcript.jsonl", output / "stderr.txt")
        initial = next(json.loads(line) for line in result.stdout.splitlines()
                       if json.loads(line).get("subtype") == "init")
        names = {tool.removeprefix("mcp__artifactize__") for tool in initial["tools"] if tool.startswith("mcp__artifactize__")}
    else:
        # App-server is the Codex host's real MCP runtime. Listing its inventory
        # needs no inference or authentication, so CI can run this when installed.
        with (output / "stderr.txt").open("w") as errors:
            process = driver.spawn([host, "app-server", "--stdio"], cwd=repo, env=env,
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors, text=True)
            messages = []
            try:
                # Read bounded pipe fragments, not an uncancellable readline
                # thread which can hang executor shutdown after a host timeout.
                with selectors.DefaultSelector() as reader:
                    reader.register(process.stdout, selectors.EVENT_READ)
                    pending = bytearray()
                    def rpc(identifier, method, params):
                        process.stdin.write(json.dumps({"id": identifier, "method": method, "params": params}) + "\n")
                        process.stdin.flush()
                        deadline = time.monotonic() + 90  # Existing host-RPC observation bound.
                        while True:
                            while b"\n" not in pending:
                                if not reader.select(max(0, deadline - time.monotonic())):
                                    raise TimeoutError("Codex host inventory observation timed out")
                                fragment = os.read(process.stdout.fileno(), 8192)
                                if not fragment:
                                    raise RuntimeError("Codex host closed before listing MCP tools")
                                pending.extend(fragment)
                            line, _, rest = pending.partition(b"\n")
                            pending[:] = rest
                            message = json.loads(line)
                            messages.append(message)
                            if message.get("id") == identifier:
                                if "error" in message:
                                    raise RuntimeError(message["error"])
                                return message["result"]
                    rpc(1, "initialize", {"clientInfo": {"name": "artifactize-smoke", "version": "1"}})
                    process.stdin.write(json.dumps({"method": "initialized"}) + "\n")
                    process.stdin.flush()
                    inventory = rpc(2, "mcpServerStatus/list", {"serverName": "artifactize", "detail": "toolsAndAuthOnly"})
                    names = set(next(item for item in inventory["data"] if item["name"] == "artifactize")["tools"])
            finally:
                # A broken input pipe must not skip process-group teardown.
                try:
                    process.stdin.close()
                finally:
                    try:
                        driver.close_child(process)
                    finally:
                        (output / "transcript.jsonl").write_text(
                            "".join(json.dumps(message) + "\n" for message in messages))
    if names != TOOLS:
        raise RuntimeError(f"{host}: expected 17 tools, got {sorted(names)}")
    (output / "result.json").write_text(json.dumps({"host": host, "tools": sorted(names), "passed": True}, indent=2))
    print(f"{host}: all 17 tools listed; {output.as_posix()}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", choices=["claude", "codex"], required=True)
    parser.add_argument("--artifactize", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--model", default="ocx-claude-native--gpt-6.1-sol")
    args = parser.parse_args()
    smoke(args.host, args.artifactize.resolve(strict=True), args.output, args.model)
