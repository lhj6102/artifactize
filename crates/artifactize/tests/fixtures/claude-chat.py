"""A Claude Code metadata probe and one Chat turn, without an account or network."""

import json
import sys


def write(line):
    print(json.dumps(line), flush=True)


args = sys.argv[1:]
model = "claude-fixture"
initialize = json.loads(sys.stdin.readline())
assert initialize["request"]["subtype"] == "initialize"
write({
    "type": "control_response",
    "response": {
        "subtype": "success",
        "request_id": initialize["request_id"],
        "response": {
            "claude_code_version": "2.99.0",
            "models": [
                {"value": model, "displayName": "Claude fixture", "supportsEffort": True,
                 "supportedEffortLevels": ["low", "high"], "defaultEffort": "high"},
                {"value": "claude-no-effort", "supportsEffort": False},
            ],
        },
    },
})
if "--model" in args:
    assert args[args.index("--model") + 1] == model, args
    assert args[args.index("--effort") + 1] == "low", args
    user = json.loads(sys.stdin.readline())
    assert user["message"]["content"][0]["text"] == "Ready?", user
    write({"type": "assistant", "message": {
        "id": "msg-fixture", "model": model, "content": [{"type": "text", "text": "Ready."}],
    }})
    write({"type": "result", "subtype": "success", "is_error": False, "result": "Ready."})
# The client owns when the subprocess exits.
sys.stdin.read()
