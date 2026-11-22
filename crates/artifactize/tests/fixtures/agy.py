"""A local ACP peer and MCP client, with no network or Google credentials."""

import json
import os
from pathlib import Path
import subprocess
import sys
import traceback

ROOT = Path(__file__).parent
MODE = ROOT / "mode"
URL = (
    "https://accounts.google.com/o/oauth2/auth?response_type=code&state=fixture"
    "&redirect_uri=https%3A%2F%2Fantigravity.google%2Foauth-callback"
)


def write(message, stream=sys.stdout):
    print(json.dumps({"jsonrpc": "2.0", **message}), file=stream, flush=True)


def receive(stream=sys.stdin):
    line = stream.readline()
    return json.loads(line) if line else None


def answer(request, payload):
    write({"id": request["id"], "result": payload})


def config(current):
    return [{"id": "account-model", "category": "model", "type": "select",
             "currentValue": current, "options": [
                 {"value": "first-model", "name": "First"},
                 {"group": "other", "name": "Other", "options": [
                     {"value": "chosen-model", "name": "Chosen"}
                 ]}
             ]}]


def tool(child, request_id, method, params):
    write({"id": request_id, "method": method, "params": params}, child.stdin)
    reply = receive(child.stdout)
    assert reply is not None and reply["id"] == request_id, reply
    return reply["result"]


def open_tools(server):
    assert server["name"] == "artifactize"
    env = {**os.environ, **{item["name"]: item["value"] for item in server["env"]}}
    child = subprocess.Popen([server["command"], *server["args"]], env=env,
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             encoding="utf-8")
    try:
        result = tool(child, 1, "initialize", {
            "protocolVersion": "2025-03-26", "clientInfo": {"name": "fixture", "version": "1"},
            "capabilities": {},
        })
        assert result["capabilities"] == {"tools": {}}
        write({"method": "notifications/initialized"}, child.stdin)
        tools = tool(child, 2, "tools/list", {})["tools"]
        assert {"read", "write", "edit", "run_command", "verify_start", "verify_wait"} <= {
            item["name"] for item in tools
        }
        return child
    except BaseException:
        child.kill()
        child.wait()
        raise


def prompt(request, child, model):
    assert model == "chosen-model", model
    params = request["params"]
    session = params["sessionId"]
    text = params["prompt"][0]["text"]

    def update(event):
        write({"method": "session/update", "params": {"sessionId": session, "update": event}})

    def chunk(part):
        update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": part}})

    if text.endswith("first message"):
        chunk("Reading ")
        chunk("notes.")
        write({"id": "permission", "method": "session/request_permission", "params": {
            "sessionId": session,
            "toolCall": {"toolCallId": "native", "title": "Foreign tool", "kind": "execute"},
            "options": [{"optionId": "permit", "kind": "allow_once", "name": "Allow"}],
        }})
        reply = receive()
        assert reply["id"] == "permission" and reply["result"]["outcome"]["outcome"] == "cancelled"
        update({"sessionUpdate": "tool_call", "toolCallId": "native-notice", "title": "read", "status": "pending"})
        result = tool(child, 3, "tools/call", {"name": "read", "arguments": {"path": "notes.txt"}})
        assert not result["isError"] and "fixture notes" in str(result), result
        result = tool(child, 4, "tools/call", {"name": "verify_start", "arguments": {"scope": {"kind": "all"}}})
        assert not result["isError"], result
        (ROOT / "pids").write_text(f"{os.getpid()} {child.pid}", encoding="utf-8")
        chunk("Waiting for Stop.")
        cancel = receive()
        assert cancel == {"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": session}}, cancel
        (ROOT / "cancelled").write_text(session, encoding="utf-8")
        # Ignore EOF to prove Stop reaps a stubborn process, not just its pipe.
        import threading
        threading.Event().wait()
    else:
        assert "first message" in text and "Reading notes." in text and "verify_start" in text, text
        assert text.endswith("continue after restart")
        chunk("Ready again.")
        answer(request, {"stopReason": "end_turn"})


def run():
    mode = MODE.read_text(encoding="utf-8")
    if sys.argv[1:] == ["--version"]:
        print("1.5.0" if mode == "old" else "1.5.14", flush=True)
        return
    if sys.argv[1:] == ["acp", "--login"]:
        print("not a Google login: https://example.invalid/", flush=True)
        print("Open this link: " + URL, file=sys.stderr, flush=True)
        assert sys.stdin.readline().strip() == "fixture-code"
        MODE.write_text("signed-in", encoding="utf-8")
        return
    assert sys.argv[1] == "acp" and "--login" not in sys.argv
    model = "first-model"
    child = None
    try:
        while (request := receive()) is not None:
            method = request["method"]
            if method == "initialize":
                assert request["params"]["protocolVersion"] == 1
                assert request["params"]["clientInfo"]["name"] == "artifactize"
                assert request["params"]["clientCapabilities"] == {}
                capabilities = {"auth": {"logout": {}}} if mode != "no-logout" else {}
                answer(request, {"protocolVersion": 1, "agentCapabilities": capabilities})
            elif method == "session/new":
                if mode == "auth-link":
                    print("Open the following link to authenticate the ACP server: " + URL, flush=True)
                    continue
                problem = {"signed-out": (-32000, "Authentication required"),
                           "quota": (-32603, "reason:QUOTA_EXHAUSTED"),
                           "rate": (429, "Rate limit exceeded"),
                           "failed": (-32603, "Something else failed")}.get(mode)
                if problem:
                    write({"id": request["id"], "error": {"code": problem[0], "message": problem[1]}})
                    continue
                params = request["params"]
                cwd = Path(params["cwd"])
                assert cwd.is_absolute() and not (cwd / "notes.txt").exists()
                servers = params["mcpServers"]
                if servers:
                    assert len(servers) == 1
                    child = open_tools(servers[0])
                answer(request, {"sessionId": "fixture-session", "configOptions": config(model),
                                 "models": {"availableModels": [{"modelId": "legacy-only", "name": "Legacy only"}]}})
            elif method == "session/set_config_option":
                params = request["params"]
                assert params["configId"] == "account-model"
                model = params["value"]
                answer(request, {"configOptions": config(model)})
            elif method == "session/prompt":
                assert child is not None
                prompt(request, child, model)
            elif method == "logout":
                MODE.write_text("signed-out", encoding="utf-8")
                answer(request, {})
            else:
                raise AssertionError(method)
    finally:
        if child is not None:
            child.stdin.close()
            child.wait(timeout=5)


if __name__ == "__main__":
    try:
        run()
    except Exception:
        (ROOT / "fixture-error").write_text(traceback.format_exc(), encoding="utf-8")
        raise
