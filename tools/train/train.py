# /// script
# requires-python = ">=3.12"
# dependencies = ["torch>=2.7.0", "timm>=1.0.15", "numpy", "pillow"]
# ///
"""Train, evaluate and gate the perception students (train.students).

  uv run tools/train/train.py --student seg --name seg-a-s0 --epochs 12 [--seed 0]
      [--batch 32] [--lr 2e-3] [--device mps|cuda|cpu] [--max-minutes M]
      [--max-usd 1.5 --usd-per-hour 0.80]
  uv run tools/train/train.py --student hand --name hand-a-s0 --gate      # dev calibrates, S4 gates
  uv run tools/train/train.py --student face --name face-a-s0 --export    # CoreML FP16 -> data/models

One code path for the Mac (MPS) and Modal (CUDA, tools/train/modal_students.py).
Reads data/packed/<student>/<session>/<split>-NNNNN.tar through a streaming
tar reader with a shuffle buffer; workers take whole shards. AdamW + linear
warm-up + cosine, bf16 autocast, checkpoint per epoch (last.pt, best.pt by
the dev metric), dev evaluation per epoch. Runs land in data/train/runs/<name>/
(metrics.jsonl, last.pt, best.pt, gate.json).

Data rules (probes/train-students/RESULT.md, step 0):
  * train reads only train shards; dev picks the checkpoint and calibrates
    finger thresholds; S4 (heldout) is read only by --gate.
  * a train frame whose previous frame is a dev frame (40 at segment edges)
    is treated as a first frame, so no dev pixels enter training.
  * frames without a previous frame get a zero prev-mask input and no
    temporal loss.
  * --ignore-eye S2: eye pixels of these sessions become IGNORE in seg and
    face targets. S2 by default (glasses, the lens is painted as eye; Noa
    2026-11-02); S1 is a candidate (video-mode eye masks are about 0.7x the
    image-mode masks of S3 and S4).

Augmentation (live camera, not the four recordings): colour (brightness,
contrast, saturation, white balance, sensor noise), scale and shift jitter
(crops also rotate +-10 deg, so student-derived boxes at runtime still fit),
horizontal flip (no output carries a side: keypoints flip x and keep their
joint order, finger states stay). The prev-mask input is zero on 30 % of
samples (first live frame) and otherwise shifted up to 8 px (motion and
student error).

Spend guards for paid runs (--usd-per-hour > 0): the launcher refuses an
up-front estimate above --max-usd; after epoch 1 the run re-estimates the
total from the measured epoch and stops if it exceeds --max-usd; every step
checks elapsed cost and stops at --max-usd. Checkpoints survive every stop.
"""

import argparse
import io
import json
import math
import os
import random
import sys
import tarfile
import time
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parent))
import students as st  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
SESSIONS = ["2026-10-27-S1", "2026-10-29-S2", "2026-11-01-S3", "2026-11-01-S4"]
SEG_CLASSES = ["background", "person", "face", "eye", "mouth", "hand", "object"]
FACE_CLASSES = ["other", "eye", "mouth"]
EYE = {"seg": 3, "face": 1}
GATE = {"seg": {"person": 0.85, "face": 0.85, "hand": 0.85},  # budget.quality
        "face": {"eye": 0.70, "mouth": 0.70},
        "hand": {"finger_state_accuracy": 0.95}}
PREV_ZERO_P = 0.3
PREV_SHIFT_PX = 8
WARMUP_FRAC = 0.03


# --- data -------------------------------------------------------------------

def shard_files(data, student, split):
    return [p for s in SESSIONS for p in sorted((data / student / s).glob(f"{split}-*.tar"))]


def iter_tar(path):
    """(key, {ext: bytes}) per sample; members of one sample are adjacent."""
    key, cur = None, {}
    with tarfile.open(path) as tar:
        for m in tar:
            k, ext = m.name.split(".", 1)
            if key is not None and k != key:
                yield key, cur
                cur = {}
            key = k
            cur[ext] = tar.extractfile(m).read()
    if key is not None:
        yield key, cur


def frame_keys(files):
    """(session, idx) of every sample in these shards, from member names only."""
    out = set()
    for f in files:
        with tarfile.open(f) as tar:
            for name in tar.getnames():
                if name.endswith(".json"):
                    sess, idx = name.split(".")[0].split("_")[:2]
                    out.add((sess, int(idx)))
    return out


def _u8(data, mode="RGB"):
    img = Image.open(io.BytesIO(data))
    return np.asarray(img.convert(mode) if mode else img)


def decode_seg(s, meta, cfg):
    image = torch.from_numpy(_u8(s["jpg"]).copy()).permute(2, 0, 1)
    mask = torch.from_numpy(_u8(s["mask.png"], None).astype(np.uint8))
    has_prev = meta["has_prev"] and (meta["session"], meta.get("prev_idx")) not in cfg["blocked_prev"]
    if has_prev:
        prev = torch.from_numpy(_u8(s["prev.jpg"]).copy()).permute(2, 0, 1)
        prev_mask = torch.from_numpy(_u8(s["prev_mask.png"], None).astype(np.uint8))
    else:
        prev, prev_mask = torch.zeros_like(image), torch.zeros_like(mask)
    if meta["session"][-2:] in cfg["ignore_eye"]:
        mask[mask == EYE["seg"]] = st.IGNORE
    return {"image": image, "mask": mask, "prev_image": prev, "prev_mask": prev_mask,
            "has_prev": torch.tensor(float(has_prev)), "session": SESSIONS.index(meta["session"]),
            "idx": meta["idx"], "prev_idx": meta.get("prev_idx") or -1}


def decode_face(s, meta, cfg):
    image = torch.from_numpy(_u8(s["jpg"]).copy()).permute(2, 0, 1)
    mask = torch.from_numpy(_u8(s["mask.png"], None).astype(np.uint8))
    if meta["session"][-2:] in cfg["ignore_eye"]:
        mask[mask == EYE["face"]] = st.IGNORE
    return {"image": image, "mask": mask, "session": SESSIONS.index(meta["session"])}


def decode_hand(s, meta, cfg):
    fingers, kps = meta["fingers"], meta.get("keypoints")
    if fingers is None and kps is None:
        return None  # nothing to learn from
    pts = kps or [[0.0, 0.0, 0.0]] * st.NUM_JOINTS
    return {"image": torch.from_numpy(_u8(s["jpg"]).copy()).permute(2, 0, 1),
            "fingers": torch.tensor(fingers or [0] * 5, dtype=torch.float32),
            "finger_mask": torch.tensor(1.0 if fingers else 0.0),
            "kp": torch.tensor([p[:2] for p in pts], dtype=torch.float32),
            "kp_conf": torch.tensor([p[2] for p in pts], dtype=torch.float32),
            "kp_sam3d": torch.tensor(float(meta.get("kp_source") == "sam3dbody")),
            "session": SESSIONS.index(meta["session"])}


DECODE = {"seg": decode_seg, "face": decode_face, "hand": decode_hand}


class Shards(torch.utils.data.IterableDataset):
    """Streams samples from tar shards; each worker owns whole shards."""

    def __init__(self, files, student, cfg, shuffle_buffer=0, seed=0, limit=None):
        self.files, self.student, self.cfg = files, student, cfg
        self.buffer, self.seed, self.limit, self.epoch = shuffle_buffer, seed, limit, 0

    def __iter__(self):
        info = torch.utils.data.get_worker_info()
        wid, nw = (info.id, info.num_workers) if info else (0, 1)
        files = list(self.files)
        rng = random.Random(f"{self.seed}/{self.epoch}/{wid}")
        if self.buffer:
            random.Random(f"{self.seed}/{self.epoch}").shuffle(files)
        files = files[wid::nw]
        limit = None if self.limit is None else self.limit // nw + (wid < self.limit % nw)
        decode, buf, n = DECODE[self.student], [], 0
        for f in files:
            for _, s in iter_tar(f):
                meta = json.loads(s["json"])
                item = decode(s, meta, self.cfg)
                if item is None:
                    continue
                if limit is not None and n >= limit:
                    break
                n += 1
                if not self.buffer:
                    yield item
                    continue
                if len(buf) < self.buffer:
                    buf.append(item)
                    continue
                i = rng.randrange(len(buf))
                yield buf[i]
                buf[i] = item
            if limit is not None and n >= limit:
                break
        rng.shuffle(buf)
        yield from buf


def loader(ds, batch, workers, device, train):
    return torch.utils.data.DataLoader(ds, batch_size=batch, num_workers=workers, drop_last=train,
                                       pin_memory=device == "cuda", persistent_workers=False,
                                       prefetch_factor=4 if workers else None)


def count_samples(files, student, cfg):
    if student != "hand":  # every seg and face sample trains
        return sum(sum(1 for n in tarfile.open(f).getnames() if n.endswith(".json")) for f in files)
    n = 0
    for f in files:
        for _, s in iter_tar(f):
            m = json.loads(s["json"])
            n += m["fingers"] is not None or m.get("keypoints") is not None
    return n


# --- augmentation (on the device, batched) ------------------------------------

def _u(gen, n, lo, hi, device):
    return torch.rand(n, generator=gen, device="cpu").to(device) * (hi - lo) + lo


def random_theta(n, kind, gen, device):
    """Affine maps output -> input in normalised coords, plus flip flags."""
    flip = torch.rand(n, generator=gen) < 0.5
    if kind == "seg":  # zoom in only: the full frame has no border to pad
        s = _u(gen, n, 1.0, 1.25, device)
        rot = torch.zeros(n, device=device)
    else:
        s = _u(gen, n, 0.9, 1.1, device)
        rot = _u(gen, n, -math.radians(10), math.radians(10), device)
    a = torch.zeros(n, 2, 2, device=device)
    a[:, 0, 0], a[:, 0, 1] = torch.cos(rot) / s, -torch.sin(rot) / s
    a[:, 1, 0], a[:, 1, 1] = torch.sin(rot) / s, torch.cos(rot) / s
    a[flip.to(device), :, 0] *= -1
    if kind == "seg":
        room = 1 - 1 / s
        t = torch.stack([_u(gen, n, -1, 1, device) * room, _u(gen, n, -1, 1, device) * room], 1)
    else:
        t = torch.stack([_u(gen, n, -0.12, 0.12, device), _u(gen, n, -0.12, 0.12, device)], 1)
    return torch.cat([a, t.unsqueeze(2)], 2)


def warp(x, theta, mode):
    grid = F.affine_grid(theta, list(x.shape), align_corners=False)
    return F.grid_sample(x, grid, mode=mode, padding_mode="zeros", align_corners=False)


def warp_label(m, theta):
    return warp(m.unsqueeze(1).float(), theta, "nearest").squeeze(1).round().long()


def warp_keypoints(kp, conf, theta):
    """Crop coords 0..1 through the inverse of the output->input map."""
    a, t = theta[:, :, :2], theta[:, :, 2]
    det = a[:, 0, 0] * a[:, 1, 1] - a[:, 0, 1] * a[:, 1, 0]
    inv = torch.stack([torch.stack([a[:, 1, 1], -a[:, 0, 1]], 1),
                       torch.stack([-a[:, 1, 0], a[:, 0, 0]], 1)], 1) / det.view(-1, 1, 1)
    n = kp * 2 - 1 - t.unsqueeze(1)
    out = (torch.einsum("bij,bkj->bki", inv, n) + 1) / 2
    inside = ((out >= 0) & (out <= 1)).all(-1)
    return out, conf * inside


def photometric(gen, n, device):
    p = {"b": _u(gen, n, 0.6, 1.4, device), "c": _u(gen, n, 0.7, 1.3, device),
         "s": _u(gen, n, 0.6, 1.4, device), "noise": _u(gen, n, 0.0, 0.03, device),
         "gain": torch.rand(n, 3, generator=gen).to(device) * 0.2 + 0.9}
    return p


def apply_photometric(x, p, gen):
    v = lambda t: t.view(-1, 1, 1, 1)  # noqa: E731
    x = x * p["gain"].view(-1, 3, 1, 1) * v(p["b"])
    mean = x.mean((1, 2, 3), keepdim=True)
    x = (x - mean) * v(p["c"]) + mean
    grey = x.mean(1, keepdim=True)
    x = (x - grey) * v(p["s"]) + grey
    x = x + torch.randn(x.shape, generator=gen).to(x.device) * v(p["noise"])
    return x.clamp(0, 1)


def prepare(batch, student, device, gen=None):
    """uint8 CPU batch -> float device batch; augments when gen is given."""
    b = {k: v.to(device, non_blocking=True) for k, v in batch.items() if torch.is_tensor(v)}
    image = b["image"].float() / 255
    n = len(image)
    if student == "seg":
        prev = b["prev_image"].float() / 255
        mask, prev_mask = b["mask"].long(), b["prev_mask"].long()
        if gen is not None:
            theta = random_theta(n, "seg", gen, device)
            image, prev = warp(image, theta, "bilinear"), warp(prev, theta, "bilinear")
            mask, prev_mask = warp_label(mask, theta), warp_label(prev_mask, theta)
            p = photometric(gen, n, device)
            image, prev = apply_photometric(image, p, gen), apply_photometric(prev, p, gen)
            shift = torch.zeros(n, 2, 3, device=device)
            shift[:, 0, 0] = shift[:, 1, 1] = 1
            shift[:, 0, 2] = _u(gen, n, -1, 1, device) * 2 * PREV_SHIFT_PX / image.shape[-1]
            shift[:, 1, 2] = _u(gen, n, -1, 1, device) * 2 * PREV_SHIFT_PX / image.shape[-2]
            prev_in = warp_label(prev_mask, shift)
            keep = (torch.rand(n, generator=gen) >= PREV_ZERO_P).to(device).float() * b["has_prev"]
        else:
            prev_in, keep = prev_mask, b["has_prev"]
        prev_in = prev_in.clamp(max=st.NUM_CLASSES - 1).unsqueeze(1).float() / (st.NUM_CLASSES - 1)
        return {"image": image, "mask": mask, "prev_image": prev, "prev_mask": prev_mask,
                "prev_mask_in": prev_in * keep.view(-1, 1, 1, 1), "has_prev": b["has_prev"]}
    if student == "face":
        mask = b["mask"].long()
        if gen is not None:
            theta = random_theta(n, "crop", gen, device)
            image, mask = warp(image, theta, "bilinear"), warp_label(mask, theta)
            image = apply_photometric(image, photometric(gen, n, device), gen)
        return {"image": image, "mask": mask}
    kp, conf = b["kp"], b["kp_conf"]
    if gen is not None:
        theta = random_theta(n, "crop", gen, device)
        image = warp(image, theta, "bilinear")
        kp, conf = warp_keypoints(kp, conf, theta)
        image = apply_photometric(image, photometric(gen, n, device), gen)
    return {"image": image, "fingers": b["fingers"], "finger_mask": b["finger_mask"], "kp": kp, "kp_conf": conf}


# --- models -------------------------------------------------------------------

def build(student, args, pretrained=True):
    if student == "seg":
        return st.SegStudent(pretrained=pretrained)
    if student == "face":
        return st.FaceStudent(pretrained=pretrained)
    return st.HandStudent(pretrained=pretrained, backbone=args.hand_backbone)


def loss_fn(student, model, batch):
    if student == "seg":
        return st.seg_loss(model, batch)
    if student == "face":
        return st.face_loss(model, batch)
    return st.hand_loss(model, batch)


def autocast(device):
    if device == "cpu":
        return torch.autocast(device_type="cpu", enabled=False)
    return torch.autocast(device_type=device, dtype=torch.bfloat16)


def sync(device):
    if device == "mps":
        torch.mps.synchronize()
    elif device == "cuda":
        torch.cuda.synchronize()


# --- evaluation ---------------------------------------------------------------

def confusion(pred, target, k):
    # on the CPU: MPS bincount trips the macOS GPU watchdog ("Impacting Interactivity")
    pred, target = pred.cpu().long(), target.cpu().long()
    valid = target != st.IGNORE
    return torch.bincount(target[valid] * k + pred[valid], minlength=k * k).view(k, k)


def iou(cm, names):
    cm = cm.double()
    inter, union = cm.diag(), cm.sum(0) + cm.sum(1) - cm.diag()
    return {n: round(float(inter[i] / union[i]), 4) if union[i] > 0 else None for i, n in enumerate(names)}


def state(bits):
    return "".join(str(int(b)) for b in bits)


@torch.no_grad()
def evaluate(student, model, files, args, cfg, limit=None, sequential=False):
    model.eval()
    dev = args.device
    if student == "seg" and sequential:
        return eval_seg_sequential(model, files, args, cfg)
    ds = Shards(files, student, cfg, limit=limit)
    out = {"samples": 0}
    k = {"seg": st.NUM_CLASSES, "face": st.NUM_FACE_CLASSES}.get(student)
    cms = defaultdict(lambda: torch.zeros(k, k, dtype=torch.long)) if k else None
    probs, labels, kp_err, kp_src = [], [], [], []
    for raw in loader(ds, args.eval_batch, args.workers, dev, False):
        b = prepare(raw, student, dev)
        sess = raw["session"]
        out["samples"] += len(sess)
        with autocast(dev):
            if student == "seg":
                for mode in args.eval_prev:
                    prev_in = b["prev_mask_in"] if mode == "sam" else torch.zeros_like(b["prev_mask_in"])
                    pred = model(b["image"], prev_in).argmax(1)
                    for si in sess.unique().tolist():
                        sel = (sess == si).to(dev)
                        cms[(mode, si)] += confusion(pred[sel], b["mask"][sel], k)
            elif student == "face":
                pred = model(b["image"]).argmax(1)
                for si in sess.unique().tolist():
                    sel = (sess == si).to(dev)
                    cms[("crop", si)] += confusion(pred[sel], b["mask"][sel], k)
            else:
                kp, logits = model(b["image"])
        if student == "hand":
            fm = b["finger_mask"] > 0
            probs.append(torch.sigmoid(logits.float())[fm].cpu())
            labels.append(b["fingers"][fm].cpu())
            w = b["kp_conf"] > st.KP_MIN_CONF
            err = (kp.float() - b["kp"]).norm(dim=-1) * 128
            kp_err.append(err[w].cpu())
            kp_src.append(raw["kp_sam3d"].to(dev).unsqueeze(1).expand_as(w)[w].cpu())
    if student in ("seg", "face"):
        names = SEG_CLASSES if student == "seg" else FACE_CLASSES
        modes = sorted({m for m, _ in cms})
        for mode in modes:
            total = sum(v for (m, _), v in cms.items() if m == mode)
            out[f"iou_{mode}"] = iou(total, names)
            out[f"iou_{mode}_per_session"] = {SESSIONS[si][-2:]: iou(v, names) for (m, si), v in cms.items() if m == mode}
        main = "sam" if "sam" in modes else modes[0]
        keys = ["person", "face", "hand"] if student == "seg" else ["eye", "mouth"]
        out["score"] = round(float(np.mean([out[f"iou_{main}"][c] or 0 for c in keys])), 4)
        return out
    probs, labels = torch.cat(probs).numpy(), torch.cat(labels).numpy()
    err, src = torch.cat(kp_err).numpy(), torch.cat(kp_src).numpy()
    out["kp_px_mean"] = round(float(err.mean()), 2) if len(err) else None
    out["kp_px_median"] = round(float(np.median(err)), 2) if len(err) else None
    out["kp_px_mean_mediapipe"] = round(float(err[src == 0].mean()), 2) if (src == 0).any() else None
    out["kp_px_mean_sam3d"] = round(float(err[src == 1].mean()), 2) if (src == 1).any() else None
    out["finger_samples"] = int(len(labels))
    out["finger"] = finger_metrics(probs, labels, [0.5] * 5)
    out["_probs"], out["_labels"] = probs, labels
    out["score"] = out["finger"]["state_accuracy"]
    return out


def finger_metrics(probs, labels, thresholds):
    pred = (probs >= np.array(thresholds)).astype(int)
    lab = labels.astype(int)
    exact = (pred == lab).all(1)
    true_states = [state(r) for r in lab]
    pred_states = [state(r) for r in pred]
    conf = defaultdict(Counter)
    for t, p in zip(true_states, pred_states):
        conf[t][p] += 1
    recall = {t: round(c[t] / sum(c.values()), 4) for t, c in sorted(conf.items())}
    return {"thresholds": [round(float(t), 3) for t in thresholds],
            "state_accuracy": round(float(exact.mean()), 4) if len(exact) else None,
            "per_finger_accuracy": [round(float(x), 4) for x in (pred == lab).mean(0)] if len(lab) else None,
            "per_state_recall": recall,
            "states_present": len(conf),
            "confusion": {t: dict(c.most_common()) for t, c in sorted(conf.items())}}


def calibrate(probs, labels):
    """Per-finger threshold with the best per-finger accuracy on dev."""
    grid = np.linspace(0.05, 0.95, 91)
    out = []
    for f in range(5):
        acc = [((probs[:, f] >= g) == labels[:, f]).mean() for g in grid]
        out.append(float(grid[int(np.argmax(acc))]))
    return out


@torch.no_grad()
def eval_seg_sequential(model, files, args, cfg):
    """Live-like: frames in time order, the prev-mask input is the student's own
    previous prediction (zeros after a gap), as the plugin will run it."""
    dev, k = args.device, st.NUM_CLASSES
    items = []  # encoded bytes only; decoded frames of S4 would need about 6 GB
    for f in files:
        for _, s in iter_tar(f):
            m = json.loads(s["json"])
            items.append((SESSIONS.index(m["session"]), m["idx"], m.get("prev_idx"), s["jpg"], s["mask.png"]))
    items.sort(key=lambda x: (x[0], x[1]))
    cms = defaultdict(lambda: torch.zeros(k, k, dtype=torch.long))
    last_idx, last_sess, last_pred = None, None, None
    for sess, idx, prev_idx, jpg, png in items:
        image = torch.from_numpy(_u8(jpg).copy()).permute(2, 0, 1).to(dev).float().unsqueeze(0) / 255
        mask = torch.from_numpy(_u8(png, None).astype(np.int64)).to(dev).unsqueeze(0)
        if SESSIONS[sess][-2:] in cfg["ignore_eye"]:
            mask[mask == EYE["seg"]] = st.IGNORE
        if last_pred is not None and sess == last_sess and prev_idx == last_idx:
            prev_in = last_pred.unsqueeze(1).float() / (k - 1)
        else:
            prev_in = torch.zeros(1, 1, *image.shape[-2:], device=dev)
        with autocast(dev):
            pred = model(image, prev_in).argmax(1)
        cms[sess] += confusion(pred, mask, k)
        last_idx, last_sess, last_pred = idx, sess, pred
    total = sum(cms.values())
    out = {"samples": len(items), "iou_own": iou(total, SEG_CLASSES),
           "iou_own_per_session": {SESSIONS[s][-2:]: iou(v, SEG_CLASSES) for s, v in cms.items()}}
    out["score"] = round(float(np.mean([out["iou_own"][c] or 0 for c in ("person", "face", "hand")])), 4)
    return out


def strip(d):
    return {k: v for k, v in d.items() if not k.startswith("_")}


# --- train ----------------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--student", choices=["seg", "hand", "face"], required=True)
    ap.add_argument("--name", required=True)
    ap.add_argument("--data", type=Path, default=ROOT / "data" / "packed")
    ap.add_argument("--runs", type=Path, default=ROOT / "data" / "train" / "runs")
    ap.add_argument("--epochs", type=int, default=10)
    ap.add_argument("--batch", type=int, default=None)
    ap.add_argument("--eval-batch", type=int, default=64)
    ap.add_argument("--lr", type=float, default=2e-3)
    ap.add_argument("--wd", type=float, default=0.01)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--workers", type=int, default=min(8, os.cpu_count() or 1))
    ap.add_argument("--shuffle-buffer", type=int, default=1000)
    ap.add_argument("--device", default="cuda" if torch.cuda.is_available() else
                    "mps" if torch.backends.mps.is_available() else "cpu")
    ap.add_argument("--ignore-eye", default="S2", help="sessions whose eye pixels are IGNORE, e.g. S1,S2")
    ap.add_argument("--hand-backbone", default=st.BACKBONE)
    ap.add_argument("--eval-prev", default="sam,zero", help="seg dev eval: prev-mask input modes")
    ap.add_argument("--eval-limit", type=int, default=None, help="dev samples per epoch eval")
    ap.add_argument("--max-steps", type=int, default=None, help="stop after this many steps (timing)")
    ap.add_argument("--max-minutes", type=float, default=None)
    ap.add_argument("--max-usd", type=float, default=None)
    ap.add_argument("--usd-per-hour", type=float, default=0.0, help="> 0 turns the spend guards on")
    ap.add_argument("--resume", action="store_true")
    ap.add_argument("--gate", action="store_true", help="calibrate on dev, gate best.pt on S4")
    ap.add_argument("--export", action="store_true", help="best.pt -> CoreML FP16 in data/models")
    ap.add_argument("--recal-bn", type=int, default=0, metavar="N",
                    help="seg: recompute BatchNorm running stats of best.pt over N augmented train batches")
    args = ap.parse_args()
    args.eval_prev = args.eval_prev.split(",")
    args.batch = args.batch or {"seg": 32, "hand": 64, "face": 64}[args.student]
    run = args.runs / args.name
    run.mkdir(parents=True, exist_ok=True)
    data = args.data
    cfg = {"ignore_eye": set(filter(None, args.ignore_eye.split(","))), "blocked_prev": set()}
    # evaluation always drops only the S2 eye (glasses), so runs that ignore
    # more sessions in training stay comparable
    eval_cfg = {"ignore_eye": {"S2"}, "blocked_prev": set()}

    if args.recal_bn:
        return recal(args, run, cfg)
    if args.gate:
        return gate(args, run, eval_cfg)
    if args.export:
        return export(args, run)

    t_start = time.time()
    random.seed(args.seed)
    np.random.seed(args.seed)
    torch.manual_seed(args.seed)
    train_files = shard_files(data, args.student, "train")
    dev_files = shard_files(data, args.student, "dev")
    assert train_files and dev_files, data
    assert not any("heldout" in f.name for f in train_files + dev_files)
    if args.student == "seg":
        # train frames whose previous frame is a dev frame: no dev pixels in training
        cfg["blocked_prev"] = frame_keys(shard_files(data, "seg", "dev") + shard_files(data, "seg", "heldout"))
    n_train = count_samples(train_files, args.student, cfg)
    steps_per_epoch = n_train // args.batch
    total_steps = steps_per_epoch * args.epochs
    print(f"{args.student}: {n_train} train samples, {steps_per_epoch} steps/epoch, {total_steps} steps, "
          f"batch {args.batch}, device {args.device}, {len(train_files)} train shards", flush=True)

    model = build(args.student, args).to(args.device)
    params = sum(p.numel() for p in model.parameters())
    opt = torch.optim.AdamW(model.parameters(), lr=args.lr, weight_decay=args.wd)
    warm = max(1, int(WARMUP_FRAC * total_steps))
    sched = torch.optim.lr_scheduler.LambdaLR(opt, lambda s: min(1.0, (s + 1) / warm) * 0.5 *
                                              (1 + math.cos(math.pi * min(1.0, s / max(1, total_steps)))))
    start_epoch, best, step = 0, -1.0, 0
    if args.resume and (run / "last.pt").exists():
        ck = torch.load(run / "last.pt", map_location=args.device, weights_only=False)
        model.load_state_dict(ck["model"])
        opt.load_state_dict(ck["opt"])
        sched.load_state_dict(ck["sched"])
        start_epoch, best, step = ck["epoch"] + 1, ck["best"], ck["step"]
        print(f"resumed at epoch {start_epoch}, step {step}", flush=True)
    (run / "args.json").write_text(json.dumps({**{k: str(v) if isinstance(v, Path) else v
                                                  for k, v in vars(args).items()},
                                               "params": params, "n_train": n_train,
                                               "steps_per_epoch": steps_per_epoch}, indent=1))
    ds = Shards(train_files, args.student, cfg, shuffle_buffer=args.shuffle_buffer, seed=args.seed)
    gen = torch.Generator().manual_seed(args.seed)
    stop = None
    usd = lambda: (time.time() - t_start) / 3600 * args.usd_per_hour  # noqa: E731

    for epoch in range(start_epoch, args.epochs):
        ds.epoch = epoch
        model.train()
        t0, wait, losses, parts = time.time(), 0.0, [], defaultdict(list)
        t_data = time.time()
        n_ep = 0
        for raw in loader(ds, args.batch, args.workers, args.device, True):
            wait += time.time() - t_data
            b = prepare(raw, args.student, args.device, gen)
            with autocast(args.device):
                loss, part = loss_fn(args.student, model, b)
            opt.zero_grad(set_to_none=True)
            loss.backward()
            opt.step()
            sched.step()
            step += 1
            n_ep += 1
            losses.append(loss.item())
            for k, v in part.items():
                parts[k].append(v)
            if n_ep % 50 == 0:
                el = time.time() - t0
                print(f"  ep {epoch} step {n_ep}/{steps_per_epoch} loss {np.mean(losses[-50:]):.4f} "
                      + " ".join(f"{k} {np.mean(v[-50:]):.4f}" for k, v in parts.items())
                      + f" | {1000 * el / n_ep:.0f} ms/step, data wait {100 * wait / el:.0f} %"
                      + (f", ${usd():.3f}" if args.usd_per_hour else ""), flush=True)
            if args.max_usd and args.usd_per_hour and usd() >= args.max_usd:
                stop = f"spend stop at ${usd():.3f} >= cap {args.max_usd}"
            if args.max_minutes and time.time() - t_start > 60 * args.max_minutes:
                stop = stop or f"time stop after {args.max_minutes} min"
            if args.max_steps and n_ep >= args.max_steps:
                stop = stop or f"step stop after {n_ep} steps"
            if stop or n_ep >= steps_per_epoch:
                break
            t_data = time.time()
        sync(args.device)
        train_s = time.time() - t0
        t1 = time.time()
        ev = evaluate(args.student, model, dev_files, args, eval_cfg, limit=args.eval_limit)
        eval_s = time.time() - t1
        rec = {"epoch": epoch, "steps": n_ep, "step_total": step, "train_s": round(train_s, 1),
               "ms_per_step": round(1000 * train_s / max(1, n_ep), 1),
               "samples_per_s": round(n_ep * args.batch / train_s, 1),
               "data_wait_pct": round(100 * wait / train_s, 1), "eval_s": round(eval_s, 1),
               "loss": round(float(np.mean(losses)), 4) if losses else None,
               **{k: round(float(np.mean(v)), 4) for k, v in parts.items()},
               "lr": sched.get_last_lr()[0], "dev": strip(ev), "elapsed_s": round(time.time() - t_start, 1)}
        if args.usd_per_hour:
            rec["usd"] = round(usd(), 4)
        with open(run / "metrics.jsonl", "a") as f:
            f.write(json.dumps(rec) + "\n")
        print(f"epoch {epoch}: train {train_s:.0f} s ({rec['ms_per_step']} ms/step, data wait "
              f"{rec['data_wait_pct']} %), eval {eval_s:.0f} s, dev score {ev['score']}", flush=True)
        ck = {"model": model.state_dict(), "opt": opt.state_dict(), "sched": sched.state_dict(),
              "epoch": epoch, "step": step, "best": max(best, ev["score"]), "args": vars(args) | {
                  "data": str(args.data), "runs": str(args.runs)}}
        torch.save(ck, run / "last.pt")
        if ev["score"] > best:
            best = ev["score"]
            torch.save({"model": model.state_dict(), "epoch": epoch, "score": best,
                        "student": args.student, "hand_backbone": args.hand_backbone}, run / "best.pt")
        if epoch == start_epoch and args.usd_per_hour and args.max_usd and not stop:
            per_epoch = time.time() - t0
            projected = usd() + (args.epochs - epoch - 1) * per_epoch / 3600 * args.usd_per_hour
            print(f"pilot re-estimate: ${projected:.3f} for {args.epochs} epochs (cap {args.max_usd})", flush=True)
            if projected > args.max_usd:
                stop = f"pilot abort: re-estimate ${projected:.3f} > cap {args.max_usd}"
        if stop:
            print("STOP: " + stop, flush=True)
            break
    summary = {"name": args.name, "student": args.student, "best_dev_score": best, "stopped": stop,
               "elapsed_s": round(time.time() - t_start, 1), "usd": round(usd(), 4) if args.usd_per_hour else 0}
    (run / "summary.json").write_text(json.dumps(summary, indent=1))
    print(json.dumps(summary))


def load_best(args, run):
    path = run / "best.pt" if (run / "best.pt").exists() else run / "best_raw.pt"
    ck = torch.load(path, map_location="cpu", weights_only=False)
    args.hand_backbone = ck.get("hand_backbone", args.hand_backbone)
    model = build(ck["student"], args, pretrained=False)
    model.load_state_dict(ck["model"])
    return model.to(args.device).eval(), ck


def recal(args, run, cfg):
    """Repair drifted BatchNorm running stats (seg-a-s0: the temporal pass updated
    them with an all-zero prev channel). Train data only, same augmentation and
    prev-mask dropout as training; the original stays as best_raw.pt."""
    model, ck = load_best(args, run)
    files = shard_files(args.data, args.student, "train")
    cfg["blocked_prev"] = frame_keys(shard_files(args.data, "seg", "dev") + shard_files(args.data, "seg", "heldout"))
    ds = Shards(files, args.student, cfg, shuffle_buffer=args.shuffle_buffer, seed=args.seed + 1000)
    gen = torch.Generator().manual_seed(args.seed + 1000)

    def batches():
        for i, raw in enumerate(loader(ds, args.batch, args.workers, args.device, True)):
            if i == args.recal_bn:
                return
            b = prepare(raw, args.student, args.device, gen)
            yield (b["image"], b["prev_mask_in"]) if args.student == "seg" else (b["image"],)

    if not (run / "best_raw.pt").exists():
        (run / "best.pt").rename(run / "best_raw.pt")
    with autocast(args.device):
        st.recalibrate_bn(model, batches())
    torch.save({**ck, "model": model.state_dict(), "bn_recalibrated_batches": args.recal_bn}, run / "best.pt")
    ev = evaluate(args.student, model, shard_files(args.data, args.student, "dev"), args,
                  {"ignore_eye": {"S2"}, "blocked_prev": set()})
    (run / "recal.json").write_text(json.dumps({"batches": args.recal_bn, "dev": strip(ev)}, indent=1))
    print(json.dumps({k: v for k, v in ev.items() if k.startswith("iou_") and "session" not in k}, indent=1))


def gate(args, run, cfg):
    """Dev for calibration, S4 once against budget.quality."""
    model, ck = load_best(args, run)
    s = args.student
    dev = evaluate(s, model, shard_files(args.data, s, "dev"), args, cfg)
    held_files = shard_files(args.data, s, "heldout")
    assert held_files and all("2026-11-01-S4" in str(f) for f in held_files)
    if s == "seg":
        held = evaluate(s, model, held_files, args, cfg)
        held_seq = evaluate(s, model, held_files, args, cfg, sequential=True)
        ious = held_seq["iou_own"]
        result = {"dev": dev, "heldout": held, "heldout_sequential": held_seq,
                  "gate": {c: {"iou": ious[c], "min": m, "pass": (ious[c] or 0) >= m} for c, m in GATE["seg"].items()},
                  "eye_mouth_note": "seg eye/mouth at 384x216 reported, gated on the face student (D10)"}
    elif s == "face":
        held = evaluate(s, model, held_files, args, cfg)
        ious = held["iou_crop"]
        result = {"dev": dev, "heldout": held,
                  "gate": {c: {"iou": ious[c], "min": m, "pass": (ious[c] or 0) >= m} for c, m in GATE["face"].items()}}
    else:
        thr = calibrate(dev["_probs"], dev["_labels"])
        dev["finger_calibrated"] = finger_metrics(dev["_probs"], dev["_labels"], thr)
        held = evaluate(s, model, held_files, args, cfg)
        held["finger_calibrated"] = finger_metrics(held["_probs"], held["_labels"], thr)
        acc = held["finger_calibrated"]["state_accuracy"]
        result = {"dev": strip(dev), "heldout": strip(held), "thresholds": thr,
                  "gate": {"finger_state_accuracy": {"value": acc, "min": 0.95, "pass": acc >= 0.95}}}
    result["epoch"] = ck["epoch"]
    result["passed"] = all(g["pass"] for g in result["gate"].values())
    (run / "gate.json").write_text(json.dumps(result, indent=1, default=str))
    print(json.dumps(result["gate"], indent=1), "\nGATE " + ("PASSED" if result["passed"] else "FAILED"))


def export(args, run):
    """best.pt -> data/models/<name>.mlpackage through the preflight export path."""
    import coremltools as ct
    import preflight as pf
    model, ck = load_best(args, run)
    model = model.float().cpu().eval()
    out = ROOT / "data" / "models"
    name = args.student  # plugin/install.sh picks data/models/<student>*.mlpackage
    img = lambda h, w: ct.ImageType(name="image", shape=(1, 3, h, w),  # noqa: E731
                                    color_layout=ct.colorlayout.RGB, scale=1 / 255)
    x = torch.rand(1, 3, 216, 384) if args.student == "seg" else torch.rand(1, 3, 128, 128)
    if args.student == "seg":
        example = (x, torch.zeros(1, 1, 216, 384))
        inputs = [img(216, 384), ct.TensorType(name="prev_mask", shape=(1, 1, 216, 384), dtype=np.float16)]
    else:
        example, inputs = (x,), [img(128, 128)]
    mlmodel, path = pf.export(name, model, example, inputs, out=out)
    # agreement with PyTorch on a real dev sample
    s = next(iter_tar(shard_files(args.data, args.student, "dev")[0]))[1]
    pimg = Image.open(io.BytesIO(s["jpg"])).convert("RGB")
    xt = torch.from_numpy(np.asarray(pimg).copy()).permute(2, 0, 1).float().unsqueeze(0) / 255
    report = {"name": name, "epoch": ck["epoch"], "path": str(path)}
    with torch.no_grad():
        if args.student == "seg":
            pm = torch.zeros(1, 1, 216, 384)
            ref = model(xt, pm).argmax(1).numpy()
            pred = mlmodel.predict({"image": pimg, "prev_mask": pm.numpy().astype(np.float16)})
            report["argmax_agreement"] = float((np.asarray(next(iter(pred.values()))).argmax(1) == ref).mean())
        elif args.student == "face":
            ref = model(xt).argmax(1).numpy()
            pred = mlmodel.predict({"image": pimg})
            report["argmax_agreement"] = float((np.asarray(next(iter(pred.values()))).argmax(1) == ref).mean())
        else:
            rk, rf = model(xt)
            outs = sorted(mlmodel.predict({"image": pimg}).values(), key=lambda v: np.asarray(v).size)
            report["max_abs_diff_fingers"] = float(np.abs(np.asarray(outs[0]) - rf.numpy()).max())
            report["max_abs_diff_keypoints"] = float(np.abs(np.asarray(outs[1]) - rk.numpy()).max())
    report["spec"] = {"inputs": [str(i) for i in mlmodel.get_spec().description.input],
                      "outputs": [o.name for o in mlmodel.get_spec().description.output]}
    (run / "export.json").write_text(json.dumps(report, indent=1))
    print(json.dumps(report, indent=1))


if __name__ == "__main__":
    main()
