# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "numpy", "transformers==4.57.6", "tokenizers",
#                 "sentencepiece", "protobuf"]
# ///
"""Decode the synthetic dev sets of ds-v11c with a run ensemble and keep, per
row, what the completion harness needs: the predicted frame after rules.py,
its pointer spans as byte ranges, the gold frame, and the gold SRC and TGT
spans (bio) as byte ranges. Never reads the real set.

  uv run probes/completion-v11/dump.py --tools <v11 tools/mapping> \
      --runs p-v11-s0-q8,p-v11-s1-q8,p-v11-s2-q8,p-v11-s3-q8 --out rows.jsonl

--tools: a checkout of tools/mapping from claude/parser-v11-de5d19 or later
(rules.py, conventions.py); --data: data/mapping of the main checkout.
"""

import argparse
import json
import sys
import time
from collections import defaultdict
from pathlib import Path

import torch

SPLITS = "dev,challenge,dev_s2,dev_s3,dev_s4,dev_s5,dev_s6,dev_s7,dev_s8"


class Span(str):
    """Span text that remembers its byte range through rules.py."""

    bytes = None


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--tools", required=True)
    ap.add_argument("--data", default=str(Path(__file__).resolve().parents[2] / "data" / "mapping"))
    ap.add_argument("--runs", required=True)
    ap.add_argument("--ds", default="ds-v11c")
    ap.add_argument("--splits", default=SPLITS)
    ap.add_argument("--rules", default="conv,pronoun")
    ap.add_argument("--device", default="cpu")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    sys.path.insert(0, a.tools)
    import frame_parser as fp
    import rules as R

    train = Path(a.data) / "train"
    dev = torch.device(a.device)
    loaded = [fp.load_run(train / "runs" / r, dev) for r in a.runs.split(",")]
    models = [m for m, _, _ in loaded]
    sp, vocab = loaded[0][1], loaded[0][2]
    tok_id = defaultdict(int, {t: i for i, t in enumerate(vocab)})
    grammar = fp.Grammar(vocab)
    fv = len(vocab)

    def items(seq, s):
        out, i = [], 0
        while i < len(seq):
            t = seq[i]
            if t >= fv:
                j = seq[i + 1] - fv if i + 1 < len(seq) and seq[i + 1] >= fv else t - fv
                sp_ = Span("=" + fp.norm(fp.span_text(s["text"], s["offs"], t - fv, j)))
                sp_.bytes = (s["offs"][t - fv][0], s["offs"][j][1])
                out.append(sp_)
                i += 2
                continue
            out.append(vocab[t])
            i += 1
        return out

    with open(a.out, "w") as f:
        for split in a.splits.split(","):
            path = train / a.ds / f"{split}.jsonl"
            recs = {json.loads(l)["id"]: json.loads(l) for l in path.open()}
            data, _ = fp.load(sp, path, tok_id)
            t0 = time.time()
            with torch.no_grad():
                preds = fp.beam(models, grammar, data, dev, 1, 0.0)
            for s, (seq, lp) in zip(data, preds):
                raw = items(seq, s)
                pred = R.apply(list(raw), s["text"], a.rules)
                # rules.py rebuilds tokens: give each span back the byte range
                # of the first unused raw span with the same text
                pool = [t for t in raw if isinstance(t, Span)]
                ranges = []
                for t in pred:
                    hit = next((r for r in pool if str(r) == str(t)), None) if str(t).startswith("=") else None
                    if hit is not None:
                        pool.remove(hit)
                    ranges.append(list(hit.bytes) if hit is not None else None)
                gold = fp.items(fp.gold_seq(s, fv), s, vocab)
                cb = fp.char_to_byte(s["text"])
                rec = recs[s["id"]]
                f.write(json.dumps({
                    "split": split, "id": s["id"], "text": s["text"], "logp": lp,
                    "pred": [str(t) for t in pred],
                    "pred_bytes": ranges,
                    "gold": gold,
                    "bio": [[cb[b], cb[e], k] for b, e, k in rec.get("bio") or []],
                }, ensure_ascii=False) + "\n")
            print(f"{split}: {len(data)} rows, {time.time() - t0:.0f} s", flush=True)


if __name__ == "__main__":
    main()
