"""A local ACP v1 peer and Verify-only MCP client, with no network or Google credentials."""

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


def config(current, thought):
    options = [{"id": "account-model", "category": "model", "type": "select",
                "name": "Model",
                "currentValue": current, "options": [
                    {"value": "first-model", "name": "First"},
                    {"group": "other", "name": "Other", "options": [
                        {"value": "chosen-model", "name": "Chosen"}
                    ]}
                ]}]

    if current == "chosen-model":
        options.append({"id": "think", "category": "thought_level", "type": "select",
                        "name": "Thought level",
                        "currentValue": thought, "options": [
                            {"value": "balanced", "name": "Balanced"},
                            {"value": "deep-thinking", "name": "Deep"},
                        ]})
    return options


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
        tools = {item["name"] for item in tool(child, 2, "tools/list", {})["tools"]}
        assert {"verify_start", "verify_wait", "run_show", "status"} <= tools, tools
        # Native ACP bridge must NEVER expose Artifactize file/shell mutation tools.
        assert not ({"read", "write", "edit", "run_command"} & tools), tools
        return child
    except BaseException:
        child.kill()
        child.wait()
        raise


def prompt(request, child, model, cwd):
    assert model == "chosen-model", model
    params = request["params"]
    session = params["sessionId"]
    text = params["prompt"][0]["text"]

    def update(event):
        write({"method": "session/update", "params": {"sessionId": session, "update": event}})

    def chunk(part):
        update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": part}})

    if text.endswith("first message"):
        update({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "Inspecting workspace."}})
        update({"sessionUpdate": "plan", "entries": [
            {"content": "Read notes", "status": "completed", "priority": "high"},
            {"content": "Run verify", "status": "in_progress", "priority": "medium"},
        ]})
        chunk("Reading ")
        chunk("notes.")
        # Request interactive permission and expect user approval via Permission CAS.
        write({"id": "perm-1", "method": "session/request_permission", "params": {
            "sessionId": session,
            "toolCall": {"toolCallId": "native-read", "title": "Read notes.txt", "kind": "read"},
            "options": [
                {"optionId": "permit", "kind": "allow_once", "name": "Allow once"},
                {"optionId": "deny", "kind": "reject_once", "name": "Deny"},
            ],
        }})
        reply = receive()
        assert reply["id"] == "perm-1", reply
        assert reply["result"]["outcome"] == {"outcome": "selected", "optionId": "permit"}, reply

        notes_text = (cwd / "notes.txt").read_text(encoding="utf-8")
        update({"sessionUpdate": "tool_call", "toolCallId": "native-read", "title": "Read notes.txt",
                "kind": "read", "status": "in_progress",
                "locations": [{"path": "notes.txt", "line": 1}]})
        update({"sessionUpdate": "tool_call_update", "toolCallId": "native-read", "status": "completed",
                "content": [
                    {"type": "content", "content": {"type": "text", "text": notes_text}},
                    {"type": "diff", "path": "notes.txt", "oldText": "fixture notes", "newText": "fixture notes"},
                ]})

        result = tool(child, 4, "tools/call", {
            "name": "verify_start",
            "arguments": {"scope": {"kind": "all"}, "request_key": "agent-retry-1"},
        })
        assert not result["isError"], result
        (ROOT / "pids").write_text(f"{os.getpid()} {child.pid}", encoding="utf-8")
        chunk("Waiting for Stop.")
        cancel = receive()
        assert cancel == {"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": session}}, cancel
        (ROOT / "cancelled").write_text(session, encoding="utf-8")
        # Ignore EOF to prove Stop reaps a stubborn process, not just its pipe.
        import threading
        threading.Event().wait()
    elif text.endswith("continue after restart"):
        assert "first message" in text and "Reading notes." in text, text
        (ROOT / "warm-pid").write_text(str(os.getpid()), encoding="utf-8")
        chunk("Ready again.")
        answer(request, {"stopReason": "end_turn"})
    elif text.endswith("warm follow-up"):
        (ROOT / "warm-followup-pid").write_text(str(os.getpid()), encoding="utf-8")
        chunk("Warm pool reused.")
        answer(request, {"stopReason": "end_turn"})
    else:
        raise AssertionError(f"unexpected prompt: {text!r}")


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
    thought = "balanced"
    child = None
    active_cwd = None
    try:
        while (request := receive()) is not None:
            method = request["method"]
            if method == "initialize":
                assert request["params"]["protocolVersion"] == 1
                assert request["params"]["clientInfo"]["name"] == "artifactize"
                assert request["params"]["clientCapabilities"] == {
                    "fs": {"readTextFile": False, "writeTextFile": False},
                    "terminal": False,
                }
                capabilities = {"loadSession": True}
                if mode != "no-logout":
                    capabilities["auth"] = {"logout": {}}
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
                active_cwd = cwd
                assert cwd.is_absolute()
                servers = params["mcpServers"]
                if servers:
                    # Live Chat turn: runs in trusted worktree with one-turn Verify bridge.
                    assert (cwd / "notes.txt").exists(), f"expected trusted worktree cwd, got {cwd}"
                    assert len(servers) == 1
                    if child is not None:
                        child.stdin.close()
                        child.wait(timeout=5)
                    child = open_tools(servers[0])
                else:
                    # Probe/models/status: MUST run in an empty tempdir, never the worktree.
                    assert not (cwd / "notes.txt").exists(), f"probe must not use worktree: {cwd}"
                answer(request, {"sessionId": "fixture-session", "configOptions": config(model, thought),
                                 "models": {"availableModels": [{"modelId": "legacy-only", "name": "Legacy only"}]}})
            elif method == "session/load":
                params = request["params"]
                cwd = Path(params["cwd"])
                active_cwd = cwd
                assert params["sessionId"] == "fixture-session"
                servers = params["mcpServers"]
                if child is not None:
                    child.stdin.close()
                    child.wait(timeout=5)
                child = open_tools(servers[0]) if servers else None
                (ROOT / "loaded").write_text(params["sessionId"], encoding="utf-8")
                # Emit a replayed history update that the client MUST suppress during LoadPhase.
                write({"method": "session/update", "params": {
                    "sessionId": "fixture-session",
                    "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "REPLAYED_NOISE"}},
                }})
                answer(request, {"configOptions": config(model, thought)})
            elif method == "session/set_config_option":
                params = request["params"]
                if params["configId"] == "account-model":
                    model = params["value"]
                else:
                    assert params["configId"] == "think" and model == "chosen-model", params
                    thought = params["value"]
                answer(request, {"configOptions": config(model, thought)})
            elif method == "session/prompt":
                assert child is not None and thought == "deep-thinking", thought
                prompt(request, child, model, active_cwd)
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
