# /// script
# requires-python = ">=3.11"
# ///
"""Arm table and paired tests on the real-sentence set (parser v8).

  uv run probes/real-eval/arms.py table p-v7a-s0 p-v7a-s1 ens-v7a ...
  uv run probes/real-eval/arms.py paired p-v7a-s0,p-v7a-s1 ens-v8,ens-v8b

Reads data/mapping/eval/preds-<name>.jsonl (tools/mapping/predict.py).
table: per arm calibration, eval all, eval originals, eval route F1 and
eval "right or one part wrong" (route multiset equals the gold after
removing all targets, or after removing all sources, gates and INV).
paired: exact match of candidate against base prediction files on the same
rows, seed i against seed i when both lists are equally long, else every
pair (McNemar, exact binomial), plus the exact-match
difference with a 95 % bootstrap over groups (5000 draws, seed 0).
"""

import collections
import json
import math
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import score as S  # noqa: E402

ROWS = [json.loads(line) for line in open(S.EVAL / "expanded.jsonl")]
GOLD = json.loads((S.EVAL / "gold-routes.json").read_text())


def preds(name: str) -> list:
    ps = [json.loads(line) for line in open(S.EVAL / f"preds-{name}.jsonl")]
    assert [p["text"] for p in ps] == [r["text"] for r in ROWS], name
    return [S.canon(p["frame"]) for p in ps]


def strip(c: collections.Counter, drop) -> collections.Counter:
    return collections.Counter(tuple(t for t in r if not drop(t))
                               for r in c.elements())


def is_target(t: str) -> bool:
    return t.startswith("T:")


def is_gesture(t: str) -> bool:
    return t[:2] in ("S:", "F:", "G:") or t == "INV"


def one_part(c, golds) -> bool:
    return c in golds or any(
        strip(c, is_target) == strip(g, is_target) or
        strip(c, is_gesture) == strip(g, is_gesture) for g in golds)


def hits(name: str) -> list:
    return [c in S.golds(r, GOLD) for r, c in zip(ROWS, preds(name))]


def table(names: list) -> None:
    print("| run | calibration | eval all | eval orig | eval route F1 "
          "| eval right or one part wrong |")
    print("|---|---|---|---|---|---|")
    for n in names:
        k = collections.Counter()
        tp = pp = gg = 0
        for r, c in zip(ROWS, preds(n)):
            gs = S.golds(r, GOLD)
            ok = c in gs
            sp = r["split"]
            k[sp] += ok
            k[sp, "n"] += 1
            if sp == "eval":
                k["orig"] += ok and r["kind"] == "orig"
                k["orig", "n"] += r["kind"] == "orig"
                k["one"] += one_part(c, gs)
                best = max(gs, key=lambda g: sum((c & g).values()))
                tp += sum((c & best).values())
                pp += sum(c.values())
                gg += sum(best.values())
        print(f"| {n} | {k['calibration']}/{k['calibration', 'n']} "
              f"| {k['eval']}/{k['eval', 'n']} | {k['orig']}/{k['orig', 'n']} "
              f"| {2 * tp / (pp + gg):.3f} | {k['one']}/{k['eval', 'n']} |")


def paired(base: list, cand: list) -> None:
    B, C = [hits(n) for n in base], [hits(n) for n in cand]
    for split in ("calibration", "eval"):
        idx = [i for i, r in enumerate(ROWS) if r["split"] == split]
        pairs = list(zip(B, C)) if len(B) == len(C) else \
            [(b, c) for b in B for c in C]
        won = sum(c[i] and not b[i] for b, c in pairs for i in idx)
        lost = sum(b[i] and not c[i] for b, c in pairs for i in idx)
        n = won + lost
        p = min(1.0, 2 * sum(math.comb(n, j) for j in
                             range(min(won, lost) + 1)) / 2 ** n) if n else 1.0

        def diff(sel: list) -> float:
            mb = sum(b[i] for b in B for i in sel) / (len(sel) * len(B))
            mc = sum(c[i] for c in C for i in sel) / (len(sel) * len(C))
            return mc - mb

        groups = sorted({ROWS[i]["group"] for i in idx})
        by = {g: [i for i in idx if ROWS[i]["group"] == g] for g in groups}
        rng, ds = random.Random(0), []
        for _ in range(5000):
            ds.append(diff([i for g in (rng.choice(groups) for _ in groups)
                            for i in by[g]]))
        ds.sort()
        print(f"{split}: won {won} lost {lost} p {p:.4f} exact diff "
              f"{diff(idx):+.3f} group bootstrap [{ds[125]:+.3f}, "
              f"{ds[4875]:+.3f}]")


def main() -> None:
    if sys.argv[1] == "table":
        table(sys.argv[2:])
    else:
        paired(sys.argv[2].split(","), sys.argv[3].split(","))


if __name__ == "__main__":
    main()
