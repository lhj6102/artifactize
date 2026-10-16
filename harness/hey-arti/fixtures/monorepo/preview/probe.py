"""Read-only preview session; artifactize owns its lifetime."""
import json
import os
import runpy
from pathlib import Path
import socket
import sys

request = json.load(sys.stdin)
assert request["version"] == 1
session = Path(os.environ["ARTIFACTIZE_SESSION_DIR"])
operation = sys.argv[1]
if operation == "start":
    with socket.socket() as server:
        server.bind(("127.0.0.1", 0))
        server.listen()
        (session / "preview-port").write_text(str(server.getsockname()[1]), encoding="ascii")
        print("ready", flush=True)
        with server.accept()[0] as client:
            assert client.recv(16) == b"stop"
elif operation == "inspect":
    root = Path(__file__).resolve().parents[1]
    api = runpy.run_path(str(root / "api/handler.py"))
    ui = runpy.run_path(str(root / "ui/view.py"))
    rendered = ui["render"](api["response"]())
    print(json.dumps({"content": [{"type": "json", "data": {"rendered": rendered}}]}))
elif operation == "stop":
    with socket.create_connection(("127.0.0.1", int((session / "preview-port").read_text(encoding="ascii"))), timeout=5) as client:
        client.sendall(b"stop")
else:
    raise ValueError("unknown operation")
