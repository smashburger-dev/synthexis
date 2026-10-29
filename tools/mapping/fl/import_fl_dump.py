# /// script
# requires-python = ">=3.11"
# ///
"""Convert an FL harvest dump into plugins/params.jsonl under
data/mapping/harvest/fl/ so label.py --harvest fl can read it.

  uv run tools/mapping/fl/import_fl_dump.py [dump.json]

Defaults to the newest file in ~/Documents/synthexis-harvest/.
"""

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
OUT = ROOT / "data" / "mapping" / "harvest" / "fl"
T5 = [0.0, 0.2, 0.5, 0.8, 1.0]


def main() -> None:
    if len(sys.argv) > 1:
        src = Path(sys.argv[1]).expanduser()
    else:
        d = Path("~/Documents/synthexis-harvest").expanduser()
        src = max(d.glob("*.json"), key=lambda p: p.stat().st_mtime)
    doc = json.loads(src.read_text())
    OUT.mkdir(parents=True, exist_ok=True)

    plugins_f = (OUT / "plugins.jsonl").open("w")
    params_f = (OUT / "params.jsonl").open("w")
    n_pl = n_pa = 0
    for i, pl in enumerate(doc.get("plugins") or []):
        name = pl.get("name") or "?"
        pid = f"Image-Line/{name}"
        prec = {
            "type": "fl", "subtype": "fl", "manufacturer": "ilgm",
            "name": name, "manufacturer_name": "Image-Line",
            "type_name": "FL plugin", "version": 0, "tags": [],
            "location": pl.get("location"),
            "key": f"fl/{pl.get('location')}",
            "plugin_id": pid, "status":
                "ok" if pl.get("params") else "no_params",
            "n_params": len(pl.get("params") or []),
            "load_ms": 0, "attempt": 1, "ts": doc.get("ts"),
        }
        plugins_f.write(json.dumps(prec) + "\n")
        n_pl += 1
        for p in pl.get("params") or []:
            samples = p.get("samples") or []
            # samples may be sparse [[t, str]...]; fill the 11-grid
            grid = {round(t, 2): s for t, s in samples}
            val = p.get("value")
            # nearest sampled string stands in for the default string
            dstr = None
            if isinstance(val, (int, float)) and grid:
                t = min(grid, key=lambda x: abs(x - val))
                dstr = grid[t] or None
            params_f.write(json.dumps({
                "plugin_id": pid, "index": p["index"],
                "address": p["index"], "identifier": str(p["index"]),
                "name": p.get("name") or "", "group": [],
                "unit": "fl", "unit_name": None,
                "min": 0.0, "max": 1.0,
                "default": val if isinstance(val, (int, float)) else 0.0,
                "flags": ["readable", "writable"],
                "samples": [[t / 10, grid.get(round(t / 10, 2))]
                            for t in range(11)],
                "default_string": dstr}) + "\n")
            n_pa += 1
    plugins_f.close()
    params_f.close()
    print(f"{src}: {n_pl} plugins, {n_pa} params -> {OUT}")


if __name__ == "__main__":
    main()
