# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "sentencepiece", "protobuf", "numpy",
#                 "transformers>=4.48", "tokenizers"]
# ///
"""Parse free sentences with a trained parser run (CPU, constrained greedy).

  uv run tools/mapping/predict.py --run p-v3-c50-s0 "open my fist to open the filter"
  uv run tools/mapping/predict.py --run p-v3-c50-s0 --file data/mapping/eval/eval.txt

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

ROOT = Path(__file__).resolve().parents[2]
TRAIN = ROOT / "data" / "mapping" / "train"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", default="p-v3-c50-s0")
    ap.add_argument("--file", default=None)
    ap.add_argument("--jsonl", default=None)
    ap.add_argument("text", nargs="*")
    a = ap.parse_args()
    texts = list(a.text)
    if a.file:
        texts += [t.strip() for t in Path(a.file).read_text().splitlines()
                  if t.strip()]

    model, sp, vocab = fp.load_run(TRAIN / "runs" / a.run)
    grammar = fp.Grammar(vocab)

    samples = []
    for t in texts:
        ids, offs = fp.tokenize(sp, t)
        samples.append({"text": t, "ids": ids[:fp.MAX_SRC - 1],
                        "offs": offs[:fp.MAX_SRC - 1]})
    rows = []
    with torch.no_grad():
        for s, (seq, lp) in zip(samples, fp.greedy(model, grammar, samples,
                                                   torch.device("cpu"))):
            frame = fp.items(seq, s, vocab)
            rows.append({"text": s["text"], "conf": round(math.exp(lp), 3),
                         "frame": frame})
            print(f"{math.exp(lp):.3f}  {s['text']}\n       {' '.join(frame)}")
    if a.jsonl:
        with open(a.jsonl, "w") as f:
            for r in rows:
                f.write(json.dumps(r, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
