# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "numpy", "transformers==4.57.6", "tokenizers",
#                 "sentencepiece", "protobuf"]
# ///
"""Extra text for distilling the parser ensemble into one student (v10 lean).

  uv run tools/mapping/distill.py pool --ds ds-v10c
  uv run probes/real-eval/leak.py --runs kd-pool
  uv run tools/mapping/distill.py label --runs p-v10-s0,p-v10-s1,p-v10-s2,p-v10-s3 \
      --ds ds-v10c --out ds-v10k

pool: synthetic sentences no dataset uses: the validator drops of every
command run (drops.jsonl, slot markup removed) and sentences left out of
training. Rows of a dev edit id (any dev split, twins included), dev texts
and training texts are left out. Writes data/mapping/commands/kd-pool/
sentences.jsonl, so leak.py can drop near-copies of the real set.

label: decodes the pool without leak drops by the ensemble (greedy over the
mean log-softmax, no rules) and writes <out>/pool.jsonl in dataset format
(bio null, edit ids kd...) next to copies of the --ds files. The real set
is never read here.
"""

import argparse
import glob
import json
import re
import shutil
import sys
from pathlib import Path

import torch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import frame_parser as fp  # noqa: E402

DATA = Path(__file__).resolve().parents[2] / "data" / "mapping"
TRAIN = DATA / "train"
POOL = DATA / "commands" / "kd-pool"
DEV = ("dev", "challenge", "dev_s2", "dev_s3", "dev_s4", "dev_s5", "dev_s6",
       "dev_s7")
SLOT = re.compile(r"\[\[([^\]|]*)\|[^\]]*\]\]")


def rows(path: Path):
    for line in path.open():
        yield json.loads(line)


def pool(a) -> None:
    ds = TRAIN / a.ds
    seen = {r["text"] for r in rows(ds / "train_cat.jsonl")}
    dev_ids = set()
    for split in DEV:
        for r in rows(ds / f"{split}.jsonl"):
            dev_ids.add(r["edit_id"].rstrip("b"))
            seen.add(r["text"])
    out, why = {}, {"dev": 0, "seen": 0, "markup": 0, "length": 0}
    for f in sorted(glob.glob(str(DATA / "commands" / "*" / "*.jsonl"))):
        name = Path(f).name
        if name not in ("sentences.jsonl", "drops.jsonl") or "kd-pool" in f:
            continue
        for r in rows(Path(f)):
            eid = r.get("edit_id") or r["id"].split(".")[0]
            text = r.get("text") or SLOT.sub(r"\1", r.get("raw") or "")
            text = re.sub(r"\s+", " ", text).strip()
            if eid.rstrip("b") in dev_ids:
                why["dev"] += 1
            elif text in seen or text in out:
                why["seen"] += 1
            elif re.search(r"\[\[|\]\]|\|", text):
                why["markup"] += 1
            elif not 3 <= len(text.split()) <= 60:
                why["length"] += 1
            else:
                out[text] = eid
    POOL.mkdir(parents=True, exist_ok=True)
    with (POOL / "sentences.jsonl").open("w") as f:
        for i, (text, eid) in enumerate(out.items()):
            f.write(json.dumps({"id": f"kd{i:06d}", "edit_id": eid,
                                "text": text}, ensure_ascii=False) + "\n")
    print(f"pool {len(out)} sentences; left out {why}")


def label(a) -> None:
    dev = torch.device(a.device)
    loaded = [fp.load_run(TRAIN / "runs" / r, dev) for r in a.runs.split(",")]
    models = [m for m, _, _ in loaded]
    sp, vocab = loaded[0][1], loaded[0][2]
    grammar = fp.Grammar(vocab)
    drop = set(json.loads((POOL / "leak_drop.json").read_text()))
    samples, lost = [], 0
    for r in rows(POOL / "sentences.jsonl"):
        if r["id"] in drop:
            continue
        ids, offs = fp.tokenize(sp, r["text"])
        if len(ids) >= fp.MAX_SRC:
            lost += 1
            continue
        samples.append({"id": r["id"], "text": r["text"], "ids": ids,
                        "offs": offs})
    print(f"labelling {len(samples)} (leak drops {len(drop)}, too long "
          f"{lost})", flush=True)
    out = TRAIN / a.out
    out.mkdir(parents=True, exist_ok=True)
    for f in (TRAIN / a.ds).glob("*.json*"):
        if not (out / f.name).exists():
            shutil.copy(f, out / f.name)
    with (out / "pool.jsonl").open("w") as f:
        for k in range(0, len(samples), 512):
            part = samples[k:k + 512]
            for s, (seq, _) in zip(part, fp.beam(models, grammar, part, dev,
                                                 1, 0.0)):
                f.write(json.dumps(record(s, seq, vocab),
                                   ensure_ascii=False) + "\n")
            print(f"{k + len(part)} / {len(samples)}", flush=True)


def record(s: dict, seq: list, vocab: list) -> dict:
    """Combined ids -> dataset record with pointer spans as char offsets."""
    fv = len(vocab)
    byte_char = {b: i for i, b in enumerate(fp.char_to_byte(s["text"]))}
    frame, i = [], 0
    while i < len(seq):
        t = seq[i]
        if t >= fv:
            j = seq[i + 1] if i + 1 < len(seq) and seq[i + 1] >= fv else t
            frame.append([byte_char[s["offs"][t - fv][0]],
                          byte_char[s["offs"][j - fv][1]]])
            i += 2
            continue
        frame.append(vocab[t])
        i += 1
    return {"id": s["id"], "edit_id": "kd" + s["id"][2:], "text": s["text"],
            "register": "kd", "lang": "", "frame": frame, "bio": None,
            "aug": False}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=("pool", "label"))
    ap.add_argument("--ds", default="ds-v10c")
    ap.add_argument("--runs", default="p-v10-s0,p-v10-s1,p-v10-s2,p-v10-s3")
    ap.add_argument("--out", default="ds-v10k")
    ap.add_argument("--device", default="mps")
    a = ap.parse_args()
    pool(a) if a.cmd == "pool" else label(a)


if __name__ == "__main__":
    main()
