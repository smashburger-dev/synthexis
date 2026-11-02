# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "sentencepiece", "protobuf", "numpy"]
# ///
"""Effect of tools/mapping/normalize.py rules on dev sets and calibration rows.

  uv run probes/decode-rules/measure.py --runs p-v5-s0,p-v5-s1

Dev sets: route_edit of the saved predictions with and without each rule,
paired counts of sentences won and lost. Real set: calibration rows only
(exact route match, probes/real-eval/score.py); eval rows are not read.
"""

import argparse
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / "tools" / "mapping"), str(ROOT / "probes" / "real-eval")]
import frame_parser as fp  # noqa: E402
import normalize as nz  # noqa: E402
import score as S  # noqa: E402

SPLITS = ("dev", "dev_s2", "dev_s3", "dev_s4", "challenge")
VARIANTS = {"none": (), "pinch": ("pinch",), "pronoun": ("pronoun",),
            "both": ("pinch", "pronoun")}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", default="p-v5-s0,p-v5-s1")
    a = ap.parse_args()
    runs = a.runs.split(",")
    rt = 0
    for sp in SPLITS:
        res = {v: [0, 0, 0] for v in VARIANTS}       # right, won, lost
        n = 0
        for run in runs:
            f = ROOT / "data/mapping/train/runs" / run / f"{sp}_preds.jsonl"
            for line in open(f):
                r = json.loads(line)
                rt += nz._flat(*nz._routes(r["pred"])) != r["pred"]
                base = fp.compare(r["pred"], r["gold"])["route_edit"]
                n += 1
                for v, rules in VARIANTS.items():
                    ok = fp.compare(nz.apply(r["pred"], rules),
                                    r["gold"])["route_edit"]
                    res[v][0] += ok
                    res[v][1] += ok and not base
                    res[v][2] += base and not ok
        print(sp, n, "  ".join(f"{v} {c[0] / n:.4f} +{c[1]}/-{c[2]}"
                               for v, c in res.items()))
    print("round-trip mismatches", rt)
    rows = [json.loads(line) for line in open(S.EVAL / "expanded.jsonl")]
    gold = json.loads((S.EVAL / "gold-routes.json").read_text())
    for run in runs:
        ps = [json.loads(line) for line in
              open(S.EVAL / f"preds-{run}.jsonl")]
        out = []
        for v, rules in VARIANTS.items():
            k = n = 0
            for r, p in zip(rows, ps):
                if r["split"] != "calibration":
                    continue
                n += 1
                k += S.canon(nz.apply(p["frame"], rules)) in S.golds(r, gold)
            out.append(f"{v} {k}/{n}")
        print("calibration", run, "  ".join(out))


if __name__ == "__main__":
    main()
