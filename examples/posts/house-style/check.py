"""Check one post against the house style.

Usage: check.py POST BANNED_WORDS MAX_WORDS
Exit 1 when the post has no "# " title, has more than MAX_WORDS words, or uses
any banned word. Every file Artifact uses this same read-only checker.
"""

import os
import re
import sys

post, banned_file, max_words = sys.argv[1], sys.argv[2], int(sys.argv[3])
with open(post, encoding="utf-8") as handle:
    lines = handle.read().splitlines()
with open(banned_file, encoding="utf-8") as handle:
    banned = {line.strip().lower() for line in handle if line.strip()}

words = re.findall(r"[A-Za-z0-9'-]+", " ".join(lines[1:]))
problems = []
if not lines or not lines[0].startswith("# "):
    problems.append("the first line is not a '# ' title")
if len(words) > max_words:
    problems.append(f"{len(words)} words, more than {max_words}")
problems += [f"banned word: {word}" for word in words if word.lower() in banned]

print(f"{os.path.basename(post)}: {len(words)}/{max_words} words")
for problem in problems:
    print(problem)
sys.exit(1 if problems else 0)
