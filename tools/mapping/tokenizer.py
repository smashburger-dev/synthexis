# /// script
# requires-python = ">=3.11"
# dependencies = ["sentencepiece"]
# ///
"""SentencePiece unigram 8k with byte fallback for the parser.

  uv run tools/mapping/tokenizer.py [--ds ds-v1] [--out tok-v1]

Corpus: train_all sentences of the dataset (never dev, challenge or
Noa's files), gloss-v1 phrases, harvested AU and VST3 plugin and
parameter names (unique). Writes data/mapping/train/<out>/sp.model and
summary.json (corpus counts, pieces per training sentence).
"""

import argparse
import json
from pathlib import Path

import sentencepiece as spm

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "data" / "mapping"


def names(path: Path, keys: tuple) -> set:
    out = set()
    with path.open() as f:
        for line in f:
            r = json.loads(line)
            for k in keys:
                v = r.get(k)
                if isinstance(v, str) and 1 < len(v) < 80:
                    out.add(v)
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ds", default="ds-v1")
    ap.add_argument("--out", default="tok-v1")
    ap.add_argument("--vocab", type=int, default=8000)
    args = ap.parse_args()
    out = DATA / "train" / args.out
    out.mkdir(parents=True, exist_ok=True)

    sents = [json.loads(l)["text"] for l in
             (DATA / "train" / args.ds / "train_all.jsonl").open()]
    gloss = [json.loads(l)["p"] for l in
             (DATA / "glossary" / "gloss-v1" / "glossary.jsonl").open()]
    harvested = set()
    for h in ("au", "vst3"):
        harvested |= names(DATA / "harvest" / h / "plugins.jsonl",
                           ("name", "manufacturer_name"))
        harvested |= names(DATA / "harvest" / h / "params.jsonl", ("name",))
    corpus = out / "corpus.txt"
    lines = sents + gloss + sorted(harvested)
    corpus.write_text("\n".join(l.replace("\n", " ") for l in lines) + "\n")

    spm.SentencePieceTrainer.train(
        input=str(corpus), model_prefix=str(out / "sp"),
        vocab_size=args.vocab, model_type="unigram", byte_fallback=True,
        character_coverage=0.9995, normalization_rule_name="nmt_nfkc_cf",
        input_sentence_size=2_000_000, shuffle_input_sentence=True,
        split_digits=True, num_threads=8, pad_id=0, unk_id=1, bos_id=2,
        eos_id=3, minloglevel=2)
    sp = spm.SentencePieceProcessor(model_file=str(out / "sp.model"))
    lens = [len(sp.encode(s)) for s in sents]
    summary = {"sentences": len(sents), "glossary": len(gloss),
               "harvested_names": len(harvested),
               "pieces_mean": round(sum(lens) / len(lens), 1),
               "pieces_max": max(lens)}
    (out / "summary.json").write_text(json.dumps(summary, indent=1))
    corpus.unlink()
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
