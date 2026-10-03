#!/usr/bin/python3
"""Offline Claude CLI fixture; the MCP server is the real artifactize binary."""
import json
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

args = sys.argv[1:]
mode = os.environ.get("FAKE_MODE", "success")
log = Path(os.environ["FAKE_LOG"])
prompt = sys.stdin.read()
with log.open("a") as file:
    file.write(json.dumps({"args": args, "env": dict(os.environ), "prompt": prompt, "cwd": os.getcwd()}) + "\n")

def arg(name):
    return args[args.index(name) + 1]

def emit(value):
    print(json.dumps(value), flush=True)

raw = arg("--mcp-config")
config = json.loads(raw if raw.startswith("{") else Path(raw).read_text())
repair = not config["mcpServers"]
model = arg("--model")
tools = []
server = None

if mode in ("hang", "ignore-term"):
    if mode == "ignore-term":
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
    descendant = subprocess.Popen(["/bin/sh", "-c", "trap '' TERM; exec sleep 60"])
    Path(os.environ["FAKE_PIDS"]).write_text(f"{os.getpid()} {descendant.pid}")
    time.sleep(60)
    sys.exit(1)

if not repair:
    definition = config["mcpServers"]["artifactize"]
    server = subprocess.Popen([definition["command"]] + definition["args"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    def rpc(method, params, identifier=None):
        message = {"jsonrpc": "2.0", "method": method, "params": params}
        if identifier is not None:
            message["id"] = identifier
        server.stdin.write(json.dumps(message) + "\n")
        server.stdin.flush()
        if identifier is not None:
            while True:
                response = json.loads(server.stdout.readline())
                if response.get("id") == identifier:
                    assert "error" not in response, response
                    return response["result"]
    rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "fake-claude", "version": "1"}}, 1)
    rpc("notifications/initialized", {})
    tools = ["mcp__artifactize__" + tool["name"] for tool in rpc("tools/list", {}, 2)["tools"]]

init_tools = tools + (["EndConversation"] if mode == "control-tool" and tools else [])
if mode == "bad-tools" or (mode == "repair-tools" and repair):
    init_tools.append("Bash")
if mode == "missing-tools":
    init_tools = []
emit({"type": "system", "subtype": "init", "tools": init_tools, "model": model})

usage = {"input_tokens": 10, "output_tokens": 0, "cache_read_input_tokens": 3, "cache_creation_input_tokens": 2}

def start(identifier, actual=model):
    emit({"type": "stream_event", "event": {"type": "message_start", "message": {"id": identifier, "model": actual, "usage": usage}}})

def assistant(identifier, content):
    emit({"type": "assistant", "message": {"id": identifier, "model": model, "content": content, "usage": usage, "stop_reason": None}})

def stop(reason, output=5):
    emit({"type": "stream_event", "event": {"type": "message_delta", "delta": {"stop_reason": reason}, "usage": {"output_tokens": output}}})
    emit({"type": "stream_event", "event": {"type": "message_stop"}})

if mode == "slow-tool":
    start("tool-turn")
    assistant("tool-turn", [{"type": "tool_use", "id": "call-0", "name": tools[0], "input": {}}])
    rpc("tools/call", {"name": tools[0].removeprefix("mcp__artifactize__"), "arguments": {}}, 3)
    time.sleep(60)

turns = 1
if tools and not repair and mode not in ("bad-tools", "bad-model", "missing-tools", "budget-stream", "budget-start"):
    start("tool-turn")
    calls = 2 if mode in ("tool-budget", "tool-budget-repair") else 1
    for index in range(calls):
        content = [{"type": "tool_use", "id": f"call-{index}", "name": tools[0], "input": {"path": "evidence.txt"}}]
        assistant("tool-turn", content)
        result = rpc("tools/call", {"name": tools[0].removeprefix("mcp__artifactize__"), "arguments": {"path": "evidence.txt"}}, index + 3)
        emit({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": f"call-{index}", "content": result}]}})
    stop("tool_use")
    turns += 1

if mode == "budget-start":
    usage["input_tokens"] = 100_000
start("final", "wrong-model" if mode == "bad-model" else model)
if mode == "duplicate-call":
    assistant("final", [{"type": "tool_use", "id": "call-0", "name": tools[0], "input": {"path": "evidence.txt"}}])
if mode == "budget-start":
    time.sleep(60)
text = '{"verdict":"GREEN","note":"reviewed"}'
if (mode in ("repair", "repair-tools", "tool-budget-repair") and not repair) or mode == "bad-verdict":
    text = "```json\n" + text + "\n```"
assistant("final", [{"type": "text", "text": text}])
assistant("final", [{"type": "text", "text": text}])
stop("max_tokens" if mode == "incomplete" else "end_turn", 100_000 if mode == "budget-stream" else 5)
if mode == "budget-stream":
    time.sleep(60)
if server:
    server.stdin.close()
    server.wait(timeout=5)
if mode == "no-result":
    sys.exit(0)
if mode == "malformed":
    print("not json", flush=True)
    sys.exit(0)
result = {"type": "result", "subtype": "success", "is_error": False, "result": text,
          "usage": {"input_tokens": turns * 10, "output_tokens": turns * 5, "cache_read_input_tokens": turns * 3, "cache_creation_input_tokens": turns * 2},
          "modelUsage": {model: {"inputTokens": turns * 10, "outputTokens": turns * 5, "cacheReadInputTokens": turns * 3, "cacheCreationInputTokens": turns * 2}},
          "total_cost_usd": 0.002}
if mode == "budget-result":
    result["usage"]["output_tokens"] = 100_000
if mode == "bad-model-usage":
    result["modelUsage"] = {"other-model": {"inputTokens": 10}}
if mode == "terminal-error":
    result.update(subtype="error_during_execution", is_error=True, errors=["account quota exhausted"])
if mode == "mismatched-result":
    result["result"] = '{"verdict":"RED"}'
emit(result)
sys.exit(9 if mode == "exit-error" else 0)
