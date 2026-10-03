"""Exit 1 when a page uses a **bold term** that the glossary does not define.

Usage: check_terms.py GLOSSARY PAGE...
artifactize passes absolute paths after resolving the {references} in argv.
"""

import os
import re
import sys

glossary, *pages = sys.argv[1:]
with open(glossary, encoding="utf-8") as handle:
    defined = {line.split(":", 1)[0].strip().lower() for line in handle if line.strip()}

undefined = 0
for page in pages:
    label = os.path.join(os.path.basename(os.path.dirname(page)), os.path.basename(page))
    with open(page, encoding="utf-8") as handle:
        for number, line in enumerate(handle, 1):
            for term in re.findall(r"\*\*(.+?)\*\*", line):
                known = term.lower() in defined
                undefined += not known
                print(f"{label}:{number}: {term}: {'defined' if known else 'NOT DEFINED'}")

print(f"{undefined} undefined term(s)")
sys.exit(1 if undefined else 0)
