# /// script
# requires-python = ">=3.11"
# ///
"""List FL's plugin-database favourites -> data/mapping/fl_favorites.json.

  uv run tools/mapping/fl/favorites.py
"""

import json
from pathlib import Path

DB = (Path("~/Documents/Image-Line/FL Studio/Presets/Plugin database")
      .expanduser())
OUT = (Path(__file__).resolve().parents[3]
       / "data" / "mapping" / "fl_favorites.json")


def main() -> None:
    out = {}
    for group in ("Generators", "Effects"):
        names = sorted(
            p.stem for p in (DB / group).rglob("*.fst")) \
            if (DB / group).is_dir() else []
        out[group.lower()] = names
        print(f"{group}: {len(names)}")
    OUT.write_text(json.dumps(out, indent=1))
    print(f"-> {OUT}")


if __name__ == "__main__":
    main()
