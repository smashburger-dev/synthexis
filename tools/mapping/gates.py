# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "numpy", "sentencepiece", "protobuf"]
# ///
"""Regression gates between parser runs on the dev sets (stored preds only).

  uv run tools/mapping/gates.py --base p-v3-c50-s0,p-v3-c50-s1 \
      --cand p-v4a-s0,p-v4a-s1 [--labels orig,proj]

Runs are paired by position (seed with seed); the pairs are pooled into one
paired test per slice. Gold always comes from ds-v3; 'proj' scores both
sides after project_labels.project (what the text supports); pointer spans
are rendered with each run's own tokenizer. Slices:
language x field (route_edit, intent, sources, targets, invert, scope) and
language x register (route_edit). Exact McNemar test on the discordant
pairs; a slice that drops with p < 0.05 is flagged. Many slices are
tested, so a flag is a review item, not proof.
"""

import argparse
import collections
import json
import math
import sys
from pathlib import Path

import sentencepiece as spm

sys.path.insert(0, str(Path(__file__).resolve().parent))
import frame_parser as fp  # noqa: E402
import project_labels as pl  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
TRAIN = ROOT / "data" / "mapping" / "train"
SPLITS = ["dev", "dev_s2", "dev_s3"]
FIELDS = ["route_edit", "intent", "sources", "targets", "invert", "scope"]


def mcnemar(b: int, c: int) -> float:
    n = b + c
    if n == 0:
        return 1.0
    k = min(b, c)
    return min(1.0, 2 * sum(math.comb(n, i) for i in range(k + 1)) / 2 ** n)


def gold() -> dict:
    out = {}
    for sp in SPLITS:
        for line in open(TRAIN / "ds-v3" / f"{sp}.jsonl"):
            r = json.loads(line)
            out[r["id"]] = r
    return out


def render(frame: list, text: str, sp) -> list:
    """ds frame -> tokens with pointer spans as =text, the way the run's
    tokenizer covers them (same as frame_parser.items for gold)."""
    ids, offs = fp.tokenize(sp, text)
    cb = fp.char_to_byte(text)
    out = []
    for t in frame:
        if isinstance(t, str):
            out.append(t)
            continue
        pos = fp.covering(offs, cb[t[0]], cb[t[1]])
        out.append("=" + fp.norm(fp.span_text(text, offs, pos[0], pos[-1]))
                   if pos else "=")
    return out


def scored(run: str, gd: dict, obj: dict, labels: str) -> dict:
    meta = json.loads((TRAIN / "runs" / run / "meta.json").read_text())
    sp = spm.SentencePieceProcessor(model_file=str(ROOT / meta["tokenizer"]))
    out = {}
    for split in SPLITS:
        for line in open(TRAIN / "runs" / run / f"{split}_preds.jsonl"):
            r = json.loads(line)
            g = gd[r["id"]]
            pt, gt = r["pred"], render(g["frame"], g["text"], sp)
            if labels == "proj":
                pt, gt = (pl.project(pt, g["text"], obj),
                          pl.project(gt, g["text"], obj))
            out[r["id"]] = fp.compare(pt, gt)
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True)
    ap.add_argument("--cand", required=True)
    ap.add_argument("--labels", default="orig,proj")
    a = ap.parse_args()
    base, cand = a.base.split(","), a.cand.split(",")
    gd, obj = gold(), pl.objects()
    report = {}
    for labels in a.labels.split(","):
        pairs = []                       # (id, base result, cand result)
        for b, c in zip(base, cand):
            sb, sc = scored(b, gd, obj, labels), scored(c, gd, obj, labels)
            pairs += [(i, sb[i], sc[i]) for i in sb if i in sc]
        slices = collections.defaultdict(list)
        for i, rb, rc in pairs:
            g = gd[i]
            for f in FIELDS:
                slices[(g["lang"], f)].append((rb[f], rc[f]))
            slices[(g["lang"], "reg:" + g["register"])].append(
                (rb["route_edit"], rc["route_edit"]))
        allp = [(rb["route_edit"], rc["route_edit"]) for _, rb, rc in pairs]
        rows = {}
        for key, v in [(("all", "route_edit"), allp)] + sorted(slices.items()):
            n = len(v)
            b = sum(x and not y for x, y in v)
            c = sum(y and not x for x, y in v)
            rows["/".join(key)] = {
                "n": n, "base": round(sum(x for x, _ in v) / n, 4),
                "cand": round(sum(y for _, y in v) / n, 4),
                "lost": b, "won": c, "p": round(mcnemar(b, c), 4)}
        flags = [k for k, r in rows.items() if r["cand"] < r["base"]
                 and r["p"] < 0.05]
        report[labels] = {"rows": rows, "flags": flags}
        al = rows["all/route_edit"]
        print(f"\n{labels}: route_edit {al['base']} -> {al['cand']} "
              f"(n {al['n']}, won {al['won']}, lost {al['lost']}, "
              f"p {al['p']}); drops with p < 0.05: {flags or 'none'}")
        for k, r in rows.items():
            if k.endswith(tuple(FIELDS)) or r["p"] < 0.05:
                print(f"  {k:28s} n {r['n']:5d} {r['base']:.3f} -> "
                      f"{r['cand']:.3f}  +{r['won']}/-{r['lost']} p {r['p']}")
    (TRAIN / "runs" / cand[0] / f"gates-vs-{base[0]}.json").write_text(
        json.dumps(report, indent=1))


if __name__ == "__main__":
    main()
