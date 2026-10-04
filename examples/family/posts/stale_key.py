"""Print the staleKey of one post instance.

artifactize runs this from the shared posts folder with this on stdin:
  {"version": 1, "artifactId": "tip", "family": {"name": "posts", "material": ["tip.md"]}}
argv[1] is the banned-word list, resolved from {style}/banned.txt.

The shared files and the banned list change every instance's staleKey. An
instance's own entry in instances.json and its own material change only that
instance, so editing tip.md re-runs tip/style and reuses the other results.
"""

import hashlib
import json
import sys

context = json.load(sys.stdin)
instance = context["artifactId"]
digest = hashlib.sha256()


def add(label, data):
    digest.update(label.encode() + b"\0" + data + b"\0")


for shared in ["artifactize.json", "check.py", "stale_key.py"]:
    with open(shared, "rb") as handle:
        add(shared, handle.read())
with open(sys.argv[1], "rb") as handle:
    add("banned.txt", handle.read())
with open("instances.json", encoding="utf-8") as handle:
    entry = json.load(handle)[instance]
add("instances.json", json.dumps(entry, sort_keys=True).encode())
for material in context["family"]["material"]:
    with open(material, "rb") as handle:
        add(material, handle.read())
print(f"{instance}:sha256:{digest.hexdigest()}")
