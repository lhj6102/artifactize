import runpy
from pathlib import Path
import sys

root = Path(__file__).resolve().parent
sys.dont_write_bytecode = True
component = sys.argv[1] if len(sys.argv) > 1 else "all"
assert component in ("all", "pricing", "receipt", "label")
if component in ("all", "pricing", "receipt"):
    # Exercise the consumer's real import against this private fixture's library.
    sys.path.insert(0, str(root / "library"))
    from price import total
    assert total(3) == 21
    if component in ("all", "receipt"):
        consumer = runpy.run_path(str(root / "app/receipt.py"))
        assert consumer["receipt"](3) == "Total: 21"
if component in ("all", "label"):
    assert (root / "independent/label.txt").read_text(encoding="utf-8").strip()
