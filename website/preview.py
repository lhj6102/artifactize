#!/usr/bin/env python3
"""Open a private, landing-only preview; stop and clean up when it is idle."""

from functools import partial
from http import HTTPStatus
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import signal
import sys
import tempfile
import threading
import time
import webbrowser

# The preview owns only its temporary layout, never generated files in this worktree.
sys.dont_write_bytecode = True
from layout import layout


# A launch tool outlives the reviewer: close forgotten previews after half an hour.
IDLE_SECONDS = 30 * 60
# Recheck stop signals and idle time even when no connection arrives.
POLL_SECONDS = 1
# A stalled client must not keep a handler's files open during shutdown.
REQUEST_TIMEOUT_SECONDS = 10


class LandingHandler(SimpleHTTPRequestHandler):
    def setup(self):
        self.request.settimeout(REQUEST_TIMEOUT_SECONDS)
        super().setup()

    def parse_request(self) -> bool:
        parsed = super().parse_request()
        self.server.last_request = time.monotonic()
        return parsed

    def send_error(self, code, message=None, explain=None):
        if code != HTTPStatus.NOT_FOUND:
            return super().send_error(code, message, explain)
        body = (Path(self.directory) / "404.html").read_bytes()
        self.send_response(HTTPStatus.NOT_FOUND)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def log_message(self, format, *args):
        print(format % args, flush=True)


class LandingServer(ThreadingHTTPServer):
    # server_close joins handlers before TemporaryDirectory removes their inputs.
    daemon_threads = False

    def __init__(self, directory: Path):
        super().__init__(("127.0.0.1", 0), partial(LandingHandler, directory=str(directory)))
        self.last_request = time.monotonic()
        self.timeout = POLL_SECONDS


def main() -> None:
    print("Landing preview only: /docs/ is not part of this preview.", flush=True)
    stopped = threading.Event()
    previous = {}

    def stop(signum, frame):
        print(f"Stopping landing preview (signal {signum}).", flush=True)
        stopped.set()

    try:
        for name in (signal.SIGINT, signal.SIGTERM):
            previous[name] = signal.signal(name, stop)
        with tempfile.TemporaryDirectory(prefix="artifactize-landing-preview-") as temporary:
            directory = Path(temporary)
            layout(directory)
            with LandingServer(directory) as server:
                url = f"http://127.0.0.1:{server.server_port}/"
                print(f"Preview: {url} (temporary files: {directory})", flush=True)
                if not webbrowser.open(url):
                    print("Browser did not open; visit the preview URL above.", flush=True)
                while not stopped.is_set():
                    if time.monotonic() - server.last_request >= IDLE_SECONDS:
                        print("No requests for 30 minutes; stopping. Restart with the preview Human tool or python3 website/preview.py.", flush=True)
                        break
                    server.handle_request()
        print("Landing preview closed; temporary files removed.", flush=True)
    finally:
        for name, handler in previous.items():
            signal.signal(name, handler)


if __name__ == "__main__":
    main()
