#!/bin/sh
exec python3 -c 'import json, pathlib, sys
context = json.load(sys.stdin)
value = pathlib.Path(context["family"]["material"][0]).read_text().strip()
print(context["artifactId"] + ":" + value)'
