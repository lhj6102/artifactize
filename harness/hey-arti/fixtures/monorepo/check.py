import json
import runpy
from pathlib import Path
import sys

root = Path(__file__).resolve().parent
component = sys.argv[1] if len(sys.argv) > 1 else "all"
assert component in ("all", "api", "ui", "contract")
contract = json.loads((root / "contract/response.json").read_text(encoding="utf-8"))
assert contract == {"required": ["message"], "message_type": "string"}
if component in ("all", "api", "ui"):
    api = runpy.run_path(str(root / "api/handler.py"))
    response = api["response"]()
    assert set(contract["required"]) <= response.keys()
    assert isinstance(response["message"], str)
    if component in ("all", "ui"):
        ui = runpy.run_path(str(root / "ui/view.py"))
        assert ui["render"](response) == "Hello orchard"
