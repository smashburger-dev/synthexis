"""Parameter names of the FL stock plugins that stock_labels.py does not
label yet, with three value strings each, for hand labels made away from
the Mac (the dump itself stays out of git, like every harvest):

  python3 tools/mapping/fl/stock_names.py ~/Documents/synthexis-harvest/20261113-stock-merged.json \
      > tools/mapping/fl/stock_names.tsv

Input: a dump in the layout import_fl_dump.py reads ({"plugins": [{"name",
"params": [{"index", "name", "value", "samples": [[t, text], ...]}]}]}).
Output: plugin, index, name, and the value strings FL showed at 0, 0.5
and 1 (empty where the dump has none). Plugins already in
stock_labels.tsv, plugins without parameters and names that are
placeholders (export_paramsense.placeholder: MIDI CC slots, control
characters, "unused") are left out.
"""

import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def placeholder(name: str) -> bool:
    """Same rule as export_paramsense.placeholder and fl::is_placeholder."""
    if any(ord(c) < 32 or ord(c) == 127 for c in name):
        return True
    n = name.strip().lower()
    return bool(re.match(r"midi cc($|[ #0-9])", n) or re.match(r"cc[ #0-9]", n)
                or re.search(r" chan \d+$", n) or re.fullmatch(r"reserved ?\d*", n)
                or "(reserved)" in n or n in ("unused", "internal"))


def clean(s) -> str:
    return re.sub(r"[\t\r\n]+", " ", str(s or "")).strip()


def main() -> None:
    dump = json.loads(Path(sys.argv[1]).expanduser().read_text(encoding="utf-8"))
    done = {line.split("\t")[0] for line in (HERE / "stock_labels.tsv").read_text(encoding="utf-8").splitlines()[1:]}
    print("plugin\tindex\tname\tat_0\tat_0.5\tat_1")
    seen = set()
    for pl in dump.get("plugins") or []:
        name = pl.get("name") or ""
        if name in done or name in seen or not pl.get("params"):
            continue
        seen.add(name)
        for p in pl["params"]:
            if not clean(p.get("name")) or placeholder(p.get("name") or ""):
                continue
            grid = {round(float(t), 2): s for t, s in (p.get("samples") or [])}
            vals = [clean(grid.get(t)) for t in (0.0, 0.5, 1.0)]
            print("\t".join([clean(name), str(p["index"]), clean(p["name"]), *vals]))


if __name__ == "__main__":
    main()
