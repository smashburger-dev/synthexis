# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "sentencepiece", "protobuf", "numpy"]
# ///
"""What a plugin picker is worth: resolve the target span of the stored
parses (held-out seeds 1 and 2) against the picked plugin only, against the
6 probe plugins, and against a project with 24 more random plugins. The
plugin name in the sentence is ignored; only the target span is used.

  uv run probes/open-vocab-mapping/picker.py
"""

import json
import random
import sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import resolve as rv  # noqa: E402


def pool(pids: set) -> dict:
    out = defaultdict(set)
    for fmt in ("au", "vst3"):
        for line in open(rv.MAP / "harvest" / fmt / "params.jsonl"):
            p = json.loads(line)
            if p["plugin_id"] in pids and not rv.SKIP.match(p["name"]):
                out[p["plugin_id"]].add(p["name"])
    return out


def top1(rows: list, cands: list) -> float:
    ok = 0
    for r in rows:
        span = " ".join(r["parse"]["tgt"])
        best = max(cands, key=lambda c: rv.sim(span, c[1]))
        ok += best == (r["plugin_id"], r["gold"][0])
    return ok / len(rows)


def main() -> None:
    rows = [json.loads(line) for s in (1, 2)
            for line in open(rv.MAP / "open-vocab" / f"eval-s{s}.jsonl")]
    six = set(rv.PLUGINS.values())
    allp = sorted({json.loads(line)["plugin_id"] for line in
                   open(rv.MAP / "harvest" / "vst3" / "plugins.jsonl")} - six - {""})
    extra = set(random.Random(0).sample(allp, 24))
    params = pool(six | extra)
    print(f"{len(rows)} sentences (seeds 1, 2)")
    picked = sum(top1([r], [(r["plugin_id"], n) for n in params[r["plugin_id"]]])
                 for r in rows) / len(rows)
    print(f"  picked plugin only: top-1 {picked:.3f}")
    for name, pids in (("6 plugins", six), ("30 plugins", six | extra)):
        cands = [(pid, n) for pid in pids for n in params[pid]]
        print(f"  {name} ({len(cands)} params): top-1 {top1(rows, cands):.3f}")


if __name__ == "__main__":
    main()
