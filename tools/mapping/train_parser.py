# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "sentencepiece", "protobuf", "numpy",
#                 "transformers>=4.48", "tokenizers"]
# ///
"""Train parser v1 (frame decoder with span pointers) on this Mac.

  uv run tools/mapping/train_parser.py --name p-filt-s0
      [--train train_filtered] [--frac 1.0] [--seed 0] [--steps 12000]
      [--overfit 64] [--device mps] [--whash 1]

Data from data/mapping/train/<ds>/, tokenizer data/mapping/train/<tok>/.
Evaluates dev every --eval-every steps (grammar-constrained greedy) for
the raw weights and their EMA, keeps the best dev route-edit state of
either, then scores dev, the challenge set, dev_s2 to dev_s8 (if the
dataset has them). Writes data/mapping/train/runs/<name>/{model.pt, meta.json,
metrics.json, <split>_preds.jsonl, log.txt}.
--frac keeps a deterministic fraction of base edit ids (learning curve).
--overfit N trains and scores on the first N training samples only.
--whash 1 adds the word hash embedding (char 3-5-grams of the whole word,
about 1.07M parameters) to every subword embedding.
--backbone <name> replaces the encoder with a pretrained model from
data/mapping/backbones/<name> (its tokenizer instead of --tok, d = its
width, decoder heads/ff by --heads/--ff); it trains at --enc-lr.
--hf-layers N keeps only the lowest N backbone layers.
--train takes a comma list of files in --ds (e.g. train_cat,pool).
--teachers r1,r2,... distils their ensemble (mean of log-softmax, as
decoded) into the student: loss (1 - kd) CE + kd T^2 KL(teacher || student)
on frame and pointer steps, the same on the BIO head. --init <run> starts
from that run's weights.
"""

import argparse
import copy
import hashlib
import json
import math
import random
import sys
import time
from pathlib import Path

import numpy as np
import sentencepiece as spm
import torch
import torch.nn.functional as F

sys.path.insert(0, str(Path(__file__).resolve().parent))
import frame_parser as fp  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
TRAIN = ROOT / "data" / "mapping" / "train"


def keep_frac(edit_id: str, frac: float) -> bool:
    h = hashlib.sha1(f"lc:{edit_id.rstrip('b')}".encode()).hexdigest()
    return int(h[:8], 16) % 1000 < frac * 1000


def kd_loss(model, teachers, x, bio, yin, yout, bio_w, kd, temp):
    """Gold CE mixed with KL to the teachers' ensemble distribution, which
    is softmax of the mean member log-softmax (what greedy decodes)."""
    logits, bl = model(x, yin)
    with torch.no_grad():
        tl = tb = 0.0
        for t in teachers:
            a, b = t(x, yin)
            tl = tl + F.log_softmax(a.float() / temp, -1)
            tb = tb + F.log_softmax(b.float() / temp, -1)
        pt = F.softmax(tl / len(teachers), -1)
        pb = F.softmax(tb / len(teachers), -1)

    def kl(student, target, keep):
        ls = F.log_softmax(student.float() / temp, -1)
        k = F.kl_div(ls, target, reduction="none").sum(-1)
        return (k * keep).sum() / keep.sum().clamp(min=1) * temp * temp

    l1 = F.cross_entropy(logits.reshape(-1, logits.shape[-1]),
                         yout.reshape(-1), ignore_index=-100)
    has_bio = (bio != -100).any()
    l2 = F.cross_entropy(bl.reshape(-1, len(fp.BIO)), bio.reshape(-1),
                         ignore_index=-100) if has_bio else bl.sum() * 0
    src = (x != fp.PAD).float()
    src[:, 0] = 0
    k1 = kl(logits, pt, (yout != -100).float())
    k2 = kl(bl, pb, src)
    loss = (1 - kd) * (l1 + bio_w * l2) + kd * (k1 + bio_w * k2)
    return loss, l1, l2


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--name", required=True)
    ap.add_argument("--ds", default="ds-v1")
    ap.add_argument("--tok", default="tok-v1")
    ap.add_argument("--train", default="train_filtered")
    ap.add_argument("--frac", type=float, default=1.0)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--steps", type=int, default=12000)
    ap.add_argument("--bs", type=int, default=64)
    ap.add_argument("--lr", type=float, default=5e-4)
    ap.add_argument("--warmup", type=int, default=600)
    ap.add_argument("--bio-w", type=float, default=0.5)
    ap.add_argument("--eval-every", type=int, default=2000)
    ap.add_argument("--ema", type=float, default=0.999)
    ap.add_argument("--overfit", type=int, default=0)
    ap.add_argument("--device", default="mps")
    ap.add_argument("--whash", type=int, default=0)
    ap.add_argument("--backbone", default="")
    ap.add_argument("--enc-lr", type=float, default=5e-5)
    ap.add_argument("--heads", type=int, default=6)
    ap.add_argument("--ff", type=int, default=1536)
    ap.add_argument("--amp", type=int, default=0)     # bf16 autocast on cuda
    ap.add_argument("--hf-layers", type=int, default=0)
    ap.add_argument("--teachers", default="")
    ap.add_argument("--kd", type=float, default=0.5)
    ap.add_argument("--kd-t", type=float, default=1.0)
    ap.add_argument("--init", default="")
    a = ap.parse_args()

    random.seed(a.seed)
    np.random.seed(a.seed)
    torch.manual_seed(a.seed)
    dev = torch.device(a.device)
    out = TRAIN / "runs" / a.name
    out.mkdir(parents=True, exist_ok=True)
    logf = (out / "log.txt").open("w")

    def log(msg: str) -> None:
        line = f"[{time.strftime('%H:%M:%S')}] {msg}"
        print(line, flush=True)
        logf.write(line + "\n")
        logf.flush()

    ds = TRAIN / a.ds
    tok_path = TRAIN / a.tok / "sp.model"
    if a.backbone:
        sp = fp.HFTok(a.backbone)
        fp.MAX_SRC = fp.HF_MAX_SRC
    else:
        sp = spm.SentencePieceProcessor(model_file=str(tok_path))
    vocab = fp.frame_vocab([ds / f"{n}.jsonl" for n in
                            ("train_all", "dev", "challenge", "dev_s2",
                             "dev_s3", "dev_s4", "dev_s5", "dev_s6", "dev_s7",
                             "dev_s8")
                            if (ds / f"{n}.jsonl").exists()])
    tok_id = {t: i for i, t in enumerate(vocab)}
    fv = len(vocab)
    train, lost_tr = [], 0
    for name in a.train.split(","):
        part, lost = fp.load(sp, ds / f"{name}.jsonl", tok_id)
        train += part
        lost_tr += lost
    devset, lost_dev = fp.load(sp, ds / "dev.jsonl", tok_id)
    chal, lost_ch = fp.load(sp, ds / "challenge.jsonl", tok_id)
    extra = {n: fp.load(sp, ds / f"{n}.jsonl", tok_id)[0]
             for n in ("dev_s2", "dev_s3", "dev_s4", "dev_s5", "dev_s6", "dev_s7",
                       "dev_s8")
             if (ds / f"{n}.jsonl").exists()}
    train = [s for s in train if keep_frac(s["edit_id"], a.frac)]
    if a.overfit:
        train = train[:a.overfit]
        devset = train
        chal, extra = [], {}
    log(f"train {len(train)} (lost {lost_tr}) dev {len(devset)} "
        f"(lost {lost_dev}) challenge {len(chal)} (lost {lost_ch}) "
        f"frame vocab {fv}")

    if a.backbone:
        import transformers
        d = transformers.AutoConfig.from_pretrained(
            fp.backbone_dir(a.backbone)).hidden_size
        model = fp.Parser(sp.get_piece_size(), fv, d=d, heads=a.heads,
                          ff=a.ff, whash=a.whash, hf=a.backbone,
                          hf_layers=a.hf_layers).to(dev)
    else:
        model = fp.Parser(sp.get_piece_size(), fv, whash=a.whash).to(dev)
    if a.init:
        model.load_state_dict(torch.load(TRAIN / "runs" / a.init / "model.pt",
                                         map_location=dev))
    teachers = []
    for t in filter(None, a.teachers.split(",")):
        tm, _, tv = fp.load_run(TRAIN / "runs" / t, dev)
        assert tv == vocab, f"teacher {t} has another frame vocab"
        teachers.append(tm.eval().requires_grad_(False))
    n_params = sum(p.numel() for p in model.parameters())
    log(f"params {n_params / 1e6:.2f}M device {a.device}")
    emb = ("src.", "spos.", "tgt.", "tpos.", "wh.")
    groups = {}
    for n, p in model.named_parameters():
        hf = n.startswith("hf.")
        wd = p.dim() >= 2 and not n.startswith(emb) and "norm" not in n \
            and "embeddings" not in n
        groups.setdefault((hf, wd), []).append(p)
    opt = torch.optim.AdamW(
        [{"params": ps, "weight_decay": 0.01 if wd else 0.0,
          "scale": a.enc_lr / a.lr if hf else 1.0}
         for (hf, wd), ps in groups.items()], lr=a.lr, betas=(0.9, 0.98))

    def lr_at(step: int) -> float:
        if step < a.warmup:
            return a.lr * (step + 1) / a.warmup
        t = (step - a.warmup) / max(1, a.steps - a.warmup)
        return a.lr * (0.05 + 0.95 * 0.5 * (1 + math.cos(math.pi * t)))

    grammar = fp.Grammar(vocab)
    ema = copy.deepcopy(model)          # weight EMA, scored next to raw
    ema_p, raw_p = list(ema.parameters()), list(model.parameters())
    order, cursor = [], 0
    best, best_state, best_step, best_kind = -1.0, None, 0, ""
    t0 = time.time()
    run = torch.zeros(3, device=dev)
    for step in range(a.steps):
        model.train()
        if cursor + a.bs > len(order):
            order = list(range(len(train)))
            random.shuffle(order)
            cursor = 0
        idx = order[cursor:cursor + a.bs]
        cursor += a.bs
        x, bio, yin, yout, wh = fp.collate([train[i] for i in idx], fv, dev)
        for g in opt.param_groups:
            g["lr"] = lr_at(step) * g["scale"]
        with torch.autocast("cuda", dtype=torch.bfloat16, enabled=bool(a.amp)):
            if teachers:
                loss, l1, l2 = kd_loss(model, teachers, x, bio, yin, yout,
                                       a.bio_w, a.kd, a.kd_t)
            else:
                loss, l1, l2 = fp.loss_fn(model, x, bio, yin, yout, a.bio_w,
                                          wh if a.whash else None)
        opt.zero_grad(set_to_none=True)
        loss.backward()
        torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        opt.step()
        with torch.no_grad():
            torch._foreach_lerp_(ema_p, raw_p,
                                 1 - min(a.ema, (1 + step) / (10 + step)))
            run += torch.stack([loss.detach(), l1.detach(), l2.detach()])
        if (step + 1) % 100 == 0:       # one device sync per 100 steps
            el = time.time() - t0
            rl, r1, r2 = (run / 100).tolist()
            log(f"step {step + 1} loss {rl:.4f} frame {r1:.4f} bio {r2:.4f} "
                f"lr {lr_at(step):.2e} {el / (step + 1) * 1000:.0f} ms/step")
            run.zero_()
        if (step + 1) % a.eval_every == 0 or step + 1 == a.steps:
            for kind, m in (("raw", model), ("ema", ema)):
                met, _ = fp.evaluate(m, grammar, devset, vocab, dev)
                score = met["route_edit"]["acc"]
                log(f"eval step {step + 1} {kind} dev exact "
                    f"{met['exact']['acc']} route_edit {score} "
                    f"intent {met['intent']['acc']}")
                if score > best:
                    best, best_step, best_kind = score, step + 1, kind
                    best_state = {k: v.detach().cpu().clone()
                                  for k, v in m.state_dict().items()}

    model.load_state_dict(best_state)
    train_min = (time.time() - t0) / 60
    metrics = {"name": a.name, "args": vars(a), "params": n_params,
               "train_samples": len(train), "best_step": best_step,
               "best_kind": best_kind,
               "train_minutes": round(train_min, 1),
               "lost": {"train": lost_tr, "dev": lost_dev,
                        "challenge": lost_ch}}
    for split, data in [("dev", devset), ("challenge", chal),
                        *extra.items()]:
        if not data:
            continue
        met, rows = fp.evaluate(model, grammar, data, vocab, dev)
        metrics[split] = met
        with (out / f"{split}_preds.jsonl").open("w") as f:
            for r in rows:
                f.write(json.dumps(r, ensure_ascii=False) + "\n")
        log(f"{split}: exact {met['exact']['acc']} {met['exact']['ci95']} "
            f"route_edit {met['route_edit']['acc']} "
            f"{met['route_edit']['ci95']} cov@0.95 "
            f"{met['coverage_at_0.95']}")
    (out / "metrics.json").write_text(json.dumps(metrics, indent=1))
    if not a.overfit:
        torch.save(best_state, out / "model.pt")
        (out / "meta.json").write_text(json.dumps(
            {"config": model.cfg, "frame_vocab": vocab, "bio": fp.BIO,
             "tokenizer": a.backbone or str(tok_path.relative_to(ROOT)),
             "max_src": fp.MAX_SRC, "max_steps": fp.MAX_STEPS,
             "max_span": fp.MAX_SPAN}, indent=1))
    log(f"done in {train_min:.1f} min, best dev route_edit {best} "
        f"at step {best_step} ({best_kind})")


if __name__ == "__main__":
    main()
