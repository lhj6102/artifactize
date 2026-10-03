"""Print a reuse identity for one Artifact.

artifactize sends {"version": 1, "artifactId": "..."} on stdin and runs this
script from the Artifact's folder. The arguments are the folders the review
reads. The identity is the Artifact ID plus a SHA-256 over every file name and
file content below those folders, so any edit gives a new identity and a new
review, and an unchanged folder reuses the saved result.
"""

import hashlib
import json
import os
import sys

context = json.load(sys.stdin)
digest = hashlib.sha256()
for root in sys.argv[1:]:
    for folder, subfolders, files in os.walk(root):
        subfolders.sort()
        for name in sorted(files):
            path = os.path.join(folder, name)
            digest.update(os.path.relpath(path, root).encode() + b"\0")
            with open(path, "rb") as handle:
                digest.update(handle.read() + b"\0")
    digest.update(b"\1")
print(f"{context['artifactId']}:sha256:{digest.hexdigest()}")
