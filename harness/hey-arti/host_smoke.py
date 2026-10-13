"""Manual B2 host registration/tool-list smoke in disposable configuration."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import shutil
import subprocess

TOOLS = {"verify_start", "verify_status", "verify_wait", "verify_result", "verify_cancel",
         "status", "config_check", "run_list", "run_show", "request_list", "request_show",
         "session_show", "tools_check", "doctor", "models", "remote_status", "cache_show"}


def smoke(host, candidate, output, model):
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    repo = output / "repo"
    repo.mkdir(mode=0o700)
    (repo / "index.artf").write_text('name = "smoke"\nbasis = true\n')
    config = output / "config"
    config.mkdir(mode=0o700)
    binary = output / "bin"
    binary.mkdir(mode=0o700)
    shutil.copy2(candidate, binary / candidate.name)
    env = dict(os.environ, PATH=str(binary) + os.pathsep + os.environ.get("PATH", ""), ARTIFACTIZE_REMOTE="off")
    env.pop("CLAUDECODE", None)
    env.pop("CLAUDE_CODE_CHILD_SESSION", None)
    env.pop("CLAUDE_CODE_SESSION_ID", None)
    env["CLAUDE_CONFIG_DIR" if host == "claude" else "CODEX_HOME"] = str(config)
    args = [host, "mcp", "add"]
    if host == "claude":
        args += ["--transport", "stdio", "--scope", "project"]
    args += ["artifactize", "--", "artifactize", "--state-dir", str(output / "state"), "mcp"]
    registered = subprocess.run(args, cwd=repo, env=env, capture_output=True, text=True, timeout=60, check=True)
    (output / "registration.txt").write_text(registered.stdout + registered.stderr)
    got = subprocess.run([host, "mcp", "get", "artifactize"], cwd=repo, env=env, capture_output=True, text=True, timeout=60, check=True)
    (output / "get.txt").write_text(got.stdout + got.stderr)
    if host == "claude":
        gateway = env.get("ANTHROPIC_BASE_URL", "")
        if gateway.startswith(("http://127.0.0.1:", "http://localhost:")):
            env.setdefault("ANTHROPIC_API_KEY", "local-proxy")
        result = subprocess.run([host, "-p", "--model", model, "--verbose", "--output-format", "stream-json",
                                 "--no-session-persistence", "--setting-sources", "", "--tools", "",
                                 "--strict-mcp-config", "--mcp-config", str(repo / ".mcp.json"), "--",
                                 "Do not call tools or edit anything. List the names of all artifactize MCP tools you have."],
                                cwd=repo, env=env, capture_output=True, text=True, timeout=180, check=True)
        (output / "transcript.jsonl").write_text(result.stdout)
        (output / "stderr.txt").write_text(result.stderr)
        initial = next(json.loads(line) for line in result.stdout.splitlines()
                       if json.loads(line).get("subtype") == "init")
        names = {tool.removeprefix("mcp__artifactize__") for tool in initial["tools"] if tool.startswith("mcp__artifactize__")}
    else:
        # App-server is the Codex host's real MCP runtime. Listing its inventory
        # needs no inference or authentication, so CI can run this when installed.
        with (output / "stderr.txt").open("w") as errors:
            process = subprocess.Popen([host, "app-server", "--stdio"], cwd=repo, env=env,
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors, text=True)
            messages = []
            with ThreadPoolExecutor() as reader:
                def rpc(identifier, method, params):
                    process.stdin.write(json.dumps({"id": identifier, "method": method, "params": params}) + "\n")
                    process.stdin.flush()
                    while True:
                        line = reader.submit(process.stdout.readline).result(timeout=90)
                        if not line:
                            raise RuntimeError("Codex host closed before listing MCP tools")
                        message = json.loads(line)
                        messages.append(message)
                        if message.get("id") == identifier:
                            if "error" in message:
                                raise RuntimeError(message["error"])
                            return message["result"]
                try:
                    rpc(1, "initialize", {"clientInfo": {"name": "artifactize-smoke", "version": "1"}})
                    process.stdin.write(json.dumps({"method": "initialized"}) + "\n")
                    process.stdin.flush()
                    inventory = rpc(2, "mcpServerStatus/list", {"serverName": "artifactize", "detail": "toolsAndAuthOnly"})
                    names = set(next(item for item in inventory["data"] if item["name"] == "artifactize")["tools"])
                finally:
                    process.stdin.close()
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=10)
            (output / "transcript.jsonl").write_text("".join(json.dumps(message) + "\n" for message in messages))
    if names != TOOLS:
        raise RuntimeError(f"{host}: expected 17 tools, got {sorted(names)}")
    (output / "result.json").write_text(json.dumps({"host": host, "tools": sorted(names), "passed": True}, indent=2))
    print(f"{host}: all 17 tools listed; {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", choices=["claude", "codex"], required=True)
    parser.add_argument("--artifactize", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--model", default="ocx-claude-native--gpt-6.1-sol")
    args = parser.parse_args()
    smoke(args.host, args.artifactize.resolve(strict=True), args.output, args.model)
