# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "numpy", "transformers==4.57.6", "tokenizers",
#                 "sentencepiece", "protobuf"]
# ///
"""Score a decoder (greedy, beam, seed ensemble) on the synthetic dev sets.

  uv run tools/mapping/decode_eval.py --runs p-v7a-s0,p-v7a-s1 --beam 4 \
      --name ens-b4 [--alpha 0.0] [--ds ds-v6p] [--device mps] [--rules 'conv;conv,pronoun']

Reads only synthetic splits of --ds (default dev, challenge, dev_s2 to
dev_s5; --splits adds dev_s6 for ds-v9c), never the real set. --beam 1 with one run is plain greedy. --rules also scores the
predictions after decode-time rules (rules.py; variants split by ';'), same
decode, one pass. Writes
data/mapping/train/decode/<name>.json (route_edit and exact per split).
"""

import argparse
import json
import sys
import time
from collections import defaultdict
from pathlib import Path

import torch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import frame_parser as fp  # noqa: E402
import rules as R  # noqa: E402

TRAIN = Path(__file__).resolve().parents[2] / "data" / "mapping" / "train"
SPLITS = ("dev", "challenge", "dev_s2", "dev_s3", "dev_s4", "dev_s5")


def decoder(models, grammar, device, k: int, alpha: float):
    if k == 1 and len(models) == 1:
        return lambda ss: fp.greedy(models[0], grammar, ss, device)
    return lambda ss: fp.beam(models, grammar, ss, device, k, alpha)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", required=True)
    ap.add_argument("--name", required=True)
    ap.add_argument("--beam", type=int, default=1)
    ap.add_argument("--alpha", type=float, default=0.0)
    ap.add_argument("--ds", default="ds-v6p")
    ap.add_argument("--splits", default=",".join(SPLITS))
    ap.add_argument("--device", default="mps")
    ap.add_argument("--rules", default="")
    a = ap.parse_args()
    dev = torch.device(a.device)
    loaded = [fp.load_run(TRAIN / "runs" / r, dev) for r in a.runs.split(",")]
    models = [m for m, _, _ in loaded]
    sp, vocab = loaded[0][1], loaded[0][2]
    assert all(v == vocab for _, _, v in loaded), "frame vocabs differ"
    # a gold token the runs never saw (newer dataset) becomes <pad>: a miss
    tok_id = defaultdict(int, {t: i for i, t in enumerate(vocab)})
    grammar = fp.Grammar(vocab)
    dec = decoder(models, grammar, dev, a.beam, a.alpha)
    out = {"runs": a.runs, "beam": a.beam, "alpha": a.alpha, "ds": a.ds}
    for split in a.splits.split(","):
        data, _ = fp.load(sp, TRAIN / a.ds / f"{split}.jsonl", tok_id)
        t0 = time.time()
        met, rows = fp.evaluate(models[0], grammar, data, vocab, dev, dec)
        out[split] = {"route_edit": met["route_edit"]["acc"],
                      "exact": met["exact"]["acc"], "n": met["n"]}
        for rs in filter(None, a.rules.split(";")):
            cmp = [fp.compare(R.apply(r["pred"], r["text"], rs), r["gold"])
                   for r in rows]
            out[split][rs] = {f: round(sum(c[f] for c in cmp) / len(cmp), 4)
                              for f in ("route_edit", "exact")}
            print(f"{a.name} {split} +{rs}: {out[split][rs]}")
        print(f"{a.name} {split}: route_edit {met['route_edit']['acc']} "
              f"exact {met['exact']['acc']} ({time.time() - t0:.0f} s)",
              flush=True)
    vals = [out[s]["route_edit"] for s in a.splits.split(",")]
    out["mean"] = round(sum(vals) / len(vals), 4)
    print(f"{a.name} mean route_edit {out['mean']}")
    for rs in filter(None, a.rules.split(";")):
        vals = [out[s][rs]["route_edit"] for s in a.splits.split(",")]
        out[f"mean {rs}"] = round(sum(vals) / len(vals), 4)
        print(f"{a.name} +{rs} mean route_edit {out[f'mean {rs}']}")
    d = TRAIN / "decode"
    d.mkdir(exist_ok=True)
    (d / f"{a.name}.json").write_text(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
