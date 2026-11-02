# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "sentencepiece", "protobuf", "numpy",
#                 "transformers>=4.48", "tokenizers"]
# ///
"""Parse free sentences with a trained parser run (CPU, constrained greedy).

  uv run tools/mapping/predict.py --run p-v3-c50-s0 "open my fist to open the filter"
  uv run tools/mapping/predict.py --run p-v3-c50-s0 --file data/mapping/eval/eval.txt
  uv run tools/mapping/predict.py --run p-v7a-s0,p-v7a-s1 [--beam 4] ...

A comma list of runs decodes their ensemble (frame_parser.beam, greedy at
--beam 1); runs must share tokenizer and frame vocab. --f16 loads each
run's model.f16.pt (the shipped weights, computed in f32) instead of
model.pt; the plugin runtime's parity test uses it.

Prints one line per sentence: confidence exp(frame log-prob), the sentence,
and the frame with pointer spans as =text. --jsonl writes the same as JSON.
"""

import argparse
import json
import math
import sys
from pathlib import Path

import torch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import frame_parser as fp  # noqa: E402
import rules as R  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
TRAIN = ROOT / "data" / "mapping" / "train"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", default="p-v3-c50-s0")
    ap.add_argument("--file", default=None)
    ap.add_argument("--jsonl", default=None)
    ap.add_argument("--beam", type=int, default=1)
    ap.add_argument("--alpha", type=float, default=0.0)
    ap.add_argument("--f16", action="store_true")
    ap.add_argument("--rules", default="", help="rules.py names, e.g. conv")
    ap.add_argument("text", nargs="*")
    a = ap.parse_args()
    texts = list(a.text)
    if a.file:
        texts += [t.strip() for t in Path(a.file).read_text().splitlines()
                  if t.strip()]

    loaded = [fp.load_run(TRAIN / "runs" / r) for r in a.run.split(",")]
    models = [m for m, _, _ in loaded]
    if a.f16:
        for m, r in zip(models, a.run.split(",")):
            sd = torch.load(TRAIN / "runs" / r / "model.f16.pt",
                            map_location="cpu")
            m.load_state_dict({k: v.float() for k, v in sd.items()})
    sp, vocab = loaded[0][1], loaded[0][2]
    assert all(v == vocab for _, _, v in loaded), "frame vocabs differ"
    grammar = fp.Grammar(vocab)
    cpu = torch.device("cpu")
    if len(models) == 1 and a.beam == 1:
        decode = lambda ss: fp.greedy(models[0], grammar, ss, cpu)  # noqa
    else:
        decode = lambda ss: fp.beam(models, grammar, ss, cpu, a.beam,  # noqa
                                    a.alpha)

    samples = []
    for t in texts:
        ids, offs = fp.tokenize(sp, t)
        samples.append({"text": t, "ids": ids[:fp.MAX_SRC - 1],
                        "offs": offs[:fp.MAX_SRC - 1]})
    rows = []
    with torch.no_grad():
        for s, (seq, lp) in zip(samples, decode(samples)):
            frame = R.apply(fp.items(seq, s, vocab), s["text"], a.rules)
            rows.append({"text": s["text"], "conf": round(math.exp(lp), 3),
                         "frame": frame})
            print(f"{math.exp(lp):.3f}  {s['text']}\n       {' '.join(frame)}")
    if a.jsonl:
        with open(a.jsonl, "w") as f:
            for r in rows:
                f.write(json.dumps(r, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
