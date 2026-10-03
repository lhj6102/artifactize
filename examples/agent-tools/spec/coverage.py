"""json-protocol Agent tool: report which spec lines cite each requirement ID.

artifactize writes one request to stdin:
  {"version": 1, "context": {"artifactPath": "...", ...}, "args": {"id": "R2"}}
"id" is optional. argv[1] is the requirements file, resolved from
{reqs}/requirements.md. stdout must be one JSON object of content blocks; the
JSON block reaches the reviewer as structured data, not flattened text.
"""

import json
import os
import re
import sys

request = json.load(sys.stdin)
wanted = request["args"].get("id")

requirements = []
with open(sys.argv[1], encoding="utf-8") as handle:
    for line in handle:
        match = re.match(r"- (R\d+): (.+)", line.strip())
        if match:
            requirements.append({"id": match[1], "text": match[2]})

known = [requirement["id"] for requirement in requirements]
if wanted is not None and wanted not in known:
    # An authored error: exit 0 with isError and exactly one text block.
    message = f"Unknown requirement {wanted}. Known IDs: {', '.join(known)}."
    print(json.dumps({"isError": True, "content": [{"type": "text", "text": message}]}))
    sys.exit(0)

spec_path = os.path.join(request["context"]["artifactPath"], "spec.md")
with open(spec_path, encoding="utf-8") as handle:
    spec = handle.read().splitlines()

for requirement in requirements:
    cites = re.compile(rf"\b{requirement['id']}\b")
    requirement["citedBy"] = [
        {"line": number, "text": text}
        for number, text in enumerate(spec, 1)
        if cites.search(text)
    ]

observation = {
    "requirements": [r for r in requirements if wanted in (None, r["id"])],
    "uncited": [r["id"] for r in requirements if not r["citedBy"]],
}
print(json.dumps({"content": [{"type": "json", "data": observation}]}))
