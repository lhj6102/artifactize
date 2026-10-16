"""Read-only integrity check of the real desktop captures and their saved evidence.

The README's Agent eval reviews the images themselves; this checks their bytes,
PNG records, capture provenance and honest unclaimed example-state description.
"""
import hashlib
import json
from pathlib import Path
import re
import struct
import sys
import zlib

CAPTURES = {
    "monitor.png": (1440, 760),
    "monitor-narrow.png": (760, 760),
    "review.png": (1440, 760),
    "review-narrow.png": (760, 760),
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def png_size(data):
    require(data.startswith(b"\x89PNG\r\n\x1a\n"), "capture is not PNG")
    offset = 8
    size = None
    compressed = []
    while offset < len(data):
        require(offset + 12 <= len(data), "truncated PNG chunk")
        length, kind = struct.unpack(">I4s", data[offset:offset + 8])
        end = offset + 12 + length
        require(end <= len(data), "truncated PNG payload")
        payload = data[offset + 8:end - 4]
        crc = struct.unpack(">I", data[end - 4:end])[0]
        require(zlib.crc32(kind + payload) & 0xffffffff == crc, "PNG CRC mismatch")
        if kind == b"IHDR":
            require(size is None and offset == 8 and length == 13, "invalid PNG header")
            width, height, depth, color, method, filter_method, interlace = struct.unpack(">IIBBBBB", payload)
            require(depth == 8 and color in (2, 6) and method == filter_method == interlace == 0,
                    "expected an ordinary RGB/RGBA desktop capture")
            size = (width, height)
        elif kind == b"IDAT":
            compressed.append(payload)
        elif kind == b"IEND":
            require(length == 0 and end == len(data) and size is not None and compressed,
                    "invalid PNG end")
            pixels = zlib.decompress(b"".join(compressed))
            channels = 3 if color == 2 else 4
            stride = width * channels + 1
            require(len(pixels) == stride * height, "invalid PNG pixel data")
            require(all(pixels[row * stride] <= 4 for row in range(height)), "invalid PNG filter")
            return size
        offset = end
    raise ValueError("PNG has no end chunk")


def check(directory):
    evidence_path = directory / "evidence.json"
    require(evidence_path.is_file() and not evidence_path.is_symlink(), "evidence must be a regular file")
    evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
    require(evidence["kind"] == "real running desktop application screenshots", "wrong capture kind")
    require(re.fullmatch(r"[0-9a-f]{40}", evidence["buildCommit"]), "missing capture build commit")
    require(evidence["version"].startswith("artifactize "), "missing real CLI version")
    require("Isolated example project" in evidence["data"] and "Not production review approvals" in evidence["data"],
            "the demo state must be identified honestly")
    require(evidence["humanActions"] == "None: no claim, release, or submission.", "captures performed Human actions")
    requests = evidence["requests"]
    require(len(requests) == 3 and all(request["claim"] is None for request in requests), "captures contain claims")
    by_eval = {request["evalId"]: request for request in requests}
    require(set(by_eval) == {"docs/lint", "release/signoff", "ship/check"}, "unexpected demo requests")
    run = by_eval["release/signoff"]["runId"]
    require(re.fullmatch(r"run-[A-Za-z0-9_-]+", run), "invalid demo Run")
    require(all(request["runId"] == run and request["id"].startswith(run + "-") for request in requests),
            "capture requests belong to different Runs")
    require(by_eval["docs/lint"]["status"] == "GREEN" and by_eval["docs/lint"]["result"]["exitCode"] == 0,
            "the real runtime fixture did not pass")
    require(by_eval["release/signoff"]["status"] == "WAITING_HUMAN" and by_eval["release/signoff"]["result"] is None,
            "a Human sign-off must remain unsubmitted")
    require(by_eval["ship/check"]["status"] == "WAIT_DEPENDENCY" and by_eval["ship/check"]["result"] is None,
            "the dependent fixture must still wait")
    frames = evidence["screenshots"]
    require(len(frames) == len(CAPTURES) and {frame["file"] for frame in frames} == set(CAPTURES),
            "missing or duplicate named normal/narrow captures")
    for frame in frames:
        path = directory / frame["file"]
        require(path.is_file() and not path.is_symlink(), "capture must be a regular file")
        data = path.read_bytes()
        require(hashlib.sha256(data).hexdigest() == frame["sha256"], f"capture hash mismatch: {path.name}")
        require(png_size(data) == CAPTURES[path.name], f"wrong capture dimensions: {path.name}")
        require("Artifactize" in frame["visibleText"] and "WAITING_HUMAN" in frame["visibleText"],
                "capture evidence lacks the real app's waiting preview")
    return {"captures": len(frames), "buildCommit": evidence["buildCommit"], "run": run}


if __name__ == "__main__":
    try:
        directory = Path(sys.argv[1]) if len(sys.argv) == 2 else Path(__file__).resolve().parent
        print(json.dumps(check(directory)))
    except (OSError, ValueError, KeyError, TypeError, struct.error, zlib.error) as error:
        print(f"Screenshot integrity failed: {error}", file=sys.stderr)
        sys.exit(1)
