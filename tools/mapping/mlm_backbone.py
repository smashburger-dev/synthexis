# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "numpy", "transformers==4.57.6", "tokenizers"]
# ///
"""Domain post-training of the parser encoder: masked-language modelling
on in-domain synthetic text (parser v8, lever 2).

  uv run tools/mapping/mlm_backbone.py corpus --out mlm/corpus-v1.txt \
      [--extra data/mapping/teach/fresh.jsonl]
  python tools/mapping/modal_parser.py mlm mmbert-small-dapt1 \
      "--corpus mlm/corpus-v1.txt --epochs 2"

corpus: unique sentences of ds-v6p train_all and the teacher pool
(data/mapping/teach/pool.jsonl), plus --extra jsonl files with a "text"
field; drops every sentence that also occurs in a synthetic dev split
and, as a guard, any near-copy of the real set (word 6-gram or ratio
>= 0.7, the teacher_pool.py check). Never reads the real set into the
corpus. Writes data/mapping/train/<out>, one sentence per line.

train: ModernBertForMaskedLM from data/mapping/backbones/<--base>, whole
sentences (no packing), --mask of the tokens masked (80/10/10), AdamW
with warmup and linear decay, bf16 on cuda. Saves the model plus the base
tokenizer files to backbones/<--name>; load_run and train_parser read it
like the base backbone (AutoModel strips the MLM head).
"""

import argparse
import difflib
import json
import math
import random
import re
import shutil
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
import frame_parser as fp  # noqa: E402

DATA = ROOT / "data" / "mapping"
TRAIN = DATA / "train"
DEV_SPLITS = ("dev", "challenge", "dev_s2", "dev_s3", "dev_s4", "dev_s5")


def texts(p: Path) -> list:
    return [json.loads(line)["text"] for line in open(p)]


def leak_filter(rows: list) -> tuple[list, int]:
    """Drop near-copies of the real set (as teacher_pool.cmd_pool)."""
    ev = [re.findall(r"\w+", json.loads(line)["text"].casefold())
          for line in open(DATA / "eval" / "expanded.jsonl")]
    evg = set()
    for w in ev:
        evg |= {" ".join(w[i:i + 6]) for i in range(len(w) - 5)}
    out, lost = [], 0
    for t in rows:
        w = re.findall(r"\w+", t.casefold())
        near = bool({" ".join(w[i:i + 6]) for i in range(len(w) - 5)} & evg)
        sm = difflib.SequenceMatcher(None, b=w)
        for e in ev:
            if near:
                break
            sm.set_seq1(e)
            near = sm.real_quick_ratio() >= 0.7 and sm.quick_ratio() >= 0.7 \
                and sm.ratio() >= 0.7
        if near:
            lost += 1
        else:
            out.append(t)
    return out, lost


def cmd_corpus(a) -> None:
    ds = TRAIN / a.ds
    dev = set()
    for s in DEV_SPLITS:
        dev |= set(texts(ds / f"{s}.jsonl"))
    srcs = {"train_all": texts(ds / "train_all.jsonl"),
            "pool": texts(DATA / "teach" / "pool.jsonl")}
    for p in a.extra:
        srcs[Path(p).stem] = texts(Path(p))
    seen, rows, stats = set(), [], {}
    for name, ts in srcs.items():
        new = [t.strip() for t in ts]
        new = [t for t in new if t and t not in seen and t not in dev]
        seen |= set(new)
        kept, lost = leak_filter(new) if name != "train_all" else (new, 0)
        stats[name] = {"unique": len(new), "leak": lost}
        rows += kept
    random.Random(0).shuffle(rows)
    out = TRAIN / a.out
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(r.replace("\n", " ") for r in rows) + "\n")
    stats["lines"] = len(rows)
    print(json.dumps(stats))
    (out.with_suffix(".json")).write_text(json.dumps(stats, indent=1))


def cmd_train(a) -> None:
    import torch
    from transformers import AutoModelForMaskedLM

    random.seed(a.seed)
    torch.manual_seed(a.seed)
    dev = torch.device(a.device)
    tok = fp.HFTok(a.base).tok
    mask_id = tok.token_to_id("[MASK]") or tok.token_to_id("<mask>")
    pad_id = 0
    special = {i for i in (0, 1, 2, 3, mask_id)}
    vocab = tok.get_vocab_size()
    lines = [t for t in (TRAIN / a.corpus).read_text().splitlines() if t]
    ids = [e.ids[:fp.HF_MAX_SRC] for e in tok.encode_batch(lines)]
    model = AutoModelForMaskedLM.from_pretrained(fp.backbone_dir(a.base)).to(dev)
    steps = math.ceil(len(ids) / a.bs) * a.epochs
    nd = [p for n, p in model.named_parameters() if p.dim() < 2 or "norm" in n]
    wd = [p for n, p in model.named_parameters()
          if not (p.dim() < 2 or "norm" in n)]
    opt = torch.optim.AdamW([{"params": wd, "weight_decay": 0.01},
                             {"params": nd, "weight_decay": 0.0}],
                            lr=a.lr, betas=(0.9, 0.98))
    warm = max(1, int(0.06 * steps))
    sched = torch.optim.lr_scheduler.LambdaLR(
        opt, lambda s: min((s + 1) / warm, max(0.0, (steps - s) /
                                                max(1, steps - warm))))
    held = ids[:a.held]                 # MLM loss before and after, fixed masks
    train = ids[a.held:]

    def batch(rows: list, gen: torch.Generator):
        L = max(len(r) for r in rows)
        x = torch.full((len(rows), L), pad_id, dtype=torch.long)
        for i, r in enumerate(rows):
            x[i, :len(r)] = torch.tensor(r)
        cand = (x != pad_id) & ~torch.isin(x, torch.tensor(sorted(special)))
        m = cand & (torch.rand(x.shape, generator=gen) < a.mask)
        y = torch.where(m, x, torch.full_like(x, -100))
        r = torch.rand(x.shape, generator=gen)
        xm = x.clone()
        xm[m & (r < 0.8)] = mask_id
        rnd = m & (r >= 0.8) & (r < 0.9)
        xm[rnd] = torch.randint(5, vocab, (int(rnd.sum()),), generator=gen)
        return xm.to(dev), (x != pad_id).long().to(dev), y.to(dev)

    @torch.no_grad()
    def held_loss() -> float:
        model.eval()
        gen = torch.Generator().manual_seed(123)
        tot, n = 0.0, 0
        for k in range(0, len(held), 128):
            xm, am, y = batch(held[k:k + 128], gen)
            with torch.autocast("cuda", dtype=torch.bfloat16,
                                enabled=dev.type == "cuda"):
                out = model(input_ids=xm, attention_mask=am, labels=y)
            c = int((y != -100).sum())
            tot += float(out.loss) * c
            n += c
        model.train()
        return tot / max(1, n)

    log = []
    h0 = held_loss()
    print(f"lines {len(ids)} steps {steps} held loss {h0:.4f}", flush=True)
    log.append({"step": 0, "held": h0})
    gen = torch.Generator().manual_seed(a.seed)
    model.train()
    t0, step = time.time(), 0
    for ep in range(a.epochs):
        order = list(range(len(train)))
        random.shuffle(order)
        for k in range(0, len(order), a.bs):
            xm, am, y = batch([train[i] for i in order[k:k + a.bs]], gen)
            with torch.autocast("cuda", dtype=torch.bfloat16,
                                enabled=dev.type == "cuda"):
                loss = model(input_ids=xm, attention_mask=am, labels=y).loss
            opt.zero_grad(set_to_none=True)
            loss.backward()
            torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
            opt.step()
            sched.step()
            step += 1
            if step % 200 == 0:
                print(f"step {step}/{steps} loss {float(loss):.4f} "
                      f"{(time.time() - t0) / step * 1000:.0f} ms/step",
                      flush=True)
        h = held_loss()
        print(f"epoch {ep + 1} held loss {h:.4f}", flush=True)
        log.append({"step": step, "held": h})
    dst = fp.backbone_dir(a.name)
    dst.mkdir(parents=True, exist_ok=True)
    model.save_pretrained(dst, safe_serialization=True)
    src = fp.backbone_dir(a.base)
    for f in ("tokenizer.json", "tokenizer_config.json",
              "special_tokens_map.json"):
        if (src / f).exists():
            shutil.copy(src / f, dst / f)
    (dst / "mlm.json").write_text(json.dumps(
        {"base": a.base, "corpus": a.corpus, "args": vars(a), "log": log,
         "minutes": round((time.time() - t0) / 60, 1)}, indent=1))
    print(f"saved {dst}", flush=True)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["corpus", "train"])
    ap.add_argument("--ds", default="ds-v6p")
    ap.add_argument("--out", default="mlm/corpus-v1.txt")
    ap.add_argument("--extra", nargs="*", default=[])
    ap.add_argument("--corpus", default="mlm/corpus-v1.txt")
    ap.add_argument("--base", default="mmbert-small-de-en-32k")
    ap.add_argument("--name", default="mmbert-small-dapt1")
    ap.add_argument("--epochs", type=int, default=2)
    ap.add_argument("--bs", type=int, default=64)
    ap.add_argument("--lr", type=float, default=5e-5)
    ap.add_argument("--mask", type=float, default=0.3)
    ap.add_argument("--held", type=int, default=2000)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--device", default="cuda")
    a = ap.parse_args()
    {"corpus": cmd_corpus, "train": cmd_train}[a.cmd](a)


if __name__ == "__main__":
    main()
