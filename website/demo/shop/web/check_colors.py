"""Exit 1 when a page uses a var(--token) that the token file does not define."""
import re
import sys

page, tokens = (open(path, encoding="utf-8").read() for path in sys.argv[1:3])
defined = set(re.findall(r"(--[\w-]+):", tokens))
used = set(re.findall(r"var\((--[\w-]+)\)", page))
missing = sorted(used - defined)
print(f"{len(used)} tokens used, undefined: {', '.join(missing) or 'none'}")
sys.exit(1 if missing else 0)
