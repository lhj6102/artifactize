from pathlib import Path
import sys
from xml.etree import ElementTree

root = Path(__file__).resolve().parent
component = sys.argv[1] if len(sys.argv) > 1 else "all"
assert component in ("all", "content", "theme", "banner")
if component in ("all", "content"):
    assert (root / "site/content/index.md").read_text(encoding="utf-8").startswith("# ")
if component in ("all", "theme"):
    assert "background" in (root / "site/theme/style.css").read_text(encoding="utf-8")
if component in ("all", "banner"):
    assert ElementTree.parse(root / "assets/banner.svg").getroot().tag.endswith("svg")
    assert "CC0" in (root / "assets/LICENSE.txt").read_text(encoding="utf-8")
