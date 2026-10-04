"""Exit 1 unless every custom property in a CSS file is a hex color."""
import re
import sys

tokens = re.findall(r"(--[\w-]+):\s*([^;]+);", open(sys.argv[1], encoding="utf-8").read())
bad = [name for name, value in tokens if not re.fullmatch(r"#[0-9A-Fa-f]{6}", value.strip())]
print(f"{len(tokens)} tokens, {len(bad)} not hex: {', '.join(bad) or 'none'}")
sys.exit(1 if bad else 0)
