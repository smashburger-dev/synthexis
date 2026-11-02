# /// script
# requires-python = ">=3.12"
# dependencies = ["torch>=2.14.1", "timm>=1.0.30", "coremltools>=9.0", "numpy", "pillow"]
# ///
"""Free local preflight before a paid training run (train.setup.preflight).

  uv run tools/train/preflight.py [--session 2026-10-27-S1] [--samples 64]
      [--steps 20] [--device mps]

For the seg and the hand student: load 64 packed samples, check shapes and
label ranges, train 20 steps (AdamW + cosine, bf16 autocast if the device
supports it) and require a falling loss, then export to CoreML FP16 and
compare the CoreML prediction with PyTorch on one sample. Writes
data/preflight/preflight.json and the two .mlpackage files. Exit 1 on any
failed check.
"""

import argparse
import json
import sys
import time
from pathlib import Path

import coremltools as ct
import numpy as np
import torch
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parent))
import students as st  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
PACKED = ROOT / "data" / "packed"
OUT = ROOT / "data" / "preflight"
BATCH = 16
LR = 2e-3


def check(cond, msg, report):
    report["checks"].append({"ok": bool(cond), "check": msg})
    print(("ok   " if cond else "FAIL ") + msg, flush=True)


def autocast_ctx(device):
    try:
        ctx = torch.autocast(device_type=device, dtype=torch.bfloat16)
        with ctx:
            torch.ones(2, device=device) @ torch.ones(2, device=device)
        return lambda: torch.autocast(device_type=device, dtype=torch.bfloat16), "bf16"
    except Exception as e:  # noqa: BLE001
        print(f"bf16 autocast unavailable on {device}: {e!r}; running fp32")
        return lambda: torch.autocast(device_type=device, enabled=False), "fp32"


def to(batch, device):
    return {k: v.to(device) for k, v in batch.items()}


def train(name, model, batches, loss_fn, steps, device, report):
    model.to(device).train()
    opt = torch.optim.AdamW(model.parameters(), lr=LR, weight_decay=0.01)
    sched = torch.optim.lr_scheduler.CosineAnnealingLR(opt, T_max=steps)
    amp, precision = autocast_ctx(device)
    losses, parts, t0 = [], [], time.time()
    for step in range(steps):
        batch = to(batches[step % len(batches)], device)
        with amp():
            loss, part = loss_fn(model, batch)
        opt.zero_grad(set_to_none=True)
        loss.backward()
        opt.step()
        sched.step()
        losses.append(round(loss.item(), 4))
        parts.append({k: round(v, 4) for k, v in part.items()})
    if device == "mps":
        torch.mps.synchronize()
    seconds = time.time() - t0
    first, last = float(np.mean(losses[:3])), float(np.mean(losses[-3:]))
    report[name].update(losses=losses, loss_parts_first=parts[0], loss_parts_last=parts[-1],
                        precision=precision, train_s=round(seconds, 1),
                        ms_per_step=round(1000 * seconds / steps, 1))
    print(f"{name}: loss {losses[0]:.3f} -> {losses[-1]:.3f} ({precision}, {seconds:.1f} s)")
    check(all(np.isfinite(losses)), f"{name}: all losses finite", report)
    check(last < 0.8 * first, f"{name}: loss falls (mean last 3 {last:.3f} < 0.8 x mean first 3 {first:.3f})",
          report)


def export(name, model, example, inputs):
    model = model.float().cpu().eval()
    with torch.no_grad():
        traced = torch.jit.freeze(torch.jit.trace(model, example))
    mlmodel = ct.convert(traced, inputs=inputs, convert_to="mlprogram",
                         compute_precision=ct.precision.FLOAT16,
                         minimum_deployment_target=ct.target.macOS15)
    OUT.mkdir(parents=True, exist_ok=True)
    path = OUT / f"{name}.mlpackage"
    mlmodel.save(str(path))
    return mlmodel, path


def pil(t):
    return Image.fromarray((t.permute(1, 2, 0).numpy() * 255).round().astype(np.uint8))


def run_seg(samples, args, report):
    rep = report["seg"]
    rep["samples"] = len(samples)
    check(len(samples) == args.samples, f"seg: {len(samples)} samples with a previous frame", report)
    batches = [st.seg_batch(samples[i:i + BATCH]) for i in range(0, len(samples), BATCH)]
    b = batches[0]
    h, w = 216, 384
    check(tuple(b["image"].shape[1:]) == (3, h, w), f"seg: image shape {tuple(b['image'].shape)}", report)
    check(tuple(b["mask"].shape[1:]) == (h, w), f"seg: mask shape {tuple(b['mask'].shape)}", report)
    labels = torch.cat([x["mask"].unique() for x in batches]).unique().tolist()
    rep["mask_values"] = labels
    check(min(labels) >= 0 and max(labels) < st.NUM_CLASSES, f"seg: mask values {labels} in 0..6", report)
    check(0 <= b["prev_mask_in"].min() and b["prev_mask_in"].max() <= 1, "seg: prev-mask channel in 0..1", report)
    check(0 <= b["image"].min() and b["image"].max() <= 1, "seg: image in 0..1", report)

    model = st.SegStudent(pretrained=True)
    rep["params"] = sum(p.numel() for p in model.parameters())
    with torch.no_grad():
        out = model.eval()(b["image"][:2], b["prev_mask_in"][:2])
    check(tuple(out.shape) == (2, st.NUM_CLASSES, h, w), f"seg: logits shape {tuple(out.shape)}", report)
    train("seg", model, batches, st.seg_loss, args.steps, args.device, report)

    example = (b["image"][:1], b["prev_mask_in"][:1])
    mlmodel, path = export("seg_mnv4s_384x216_pm", model, example, [
        ct.ImageType(name="image", shape=(1, 3, h, w), color_layout=ct.colorlayout.RGB, scale=1 / 255),
        ct.TensorType(name="prev_mask", shape=(1, 1, h, w), dtype=np.float16),
    ])
    img = pil(b["image"][0])
    image_q = torch.from_numpy(np.asarray(img).copy()).permute(2, 0, 1).float().unsqueeze(0) / 255
    with torch.no_grad():
        ref = model(image_q, b["prev_mask_in"][:1]).argmax(1).numpy()
    pred = mlmodel.predict({"image": img, "prev_mask": b["prev_mask_in"][:1].numpy().astype(np.float16)})
    cm = np.asarray(next(iter(pred.values()))).argmax(1)
    agree = float((cm == ref).mean())
    rep.update(coreml=str(path.relative_to(ROOT)), coreml_argmax_agreement=round(agree, 4))
    check(agree >= 0.98, f"seg: CoreML vs PyTorch argmax agreement {agree:.4f} >= 0.98", report)


def run_hand(samples, args, report):
    rep = report["hand"]
    rep["samples"] = len(samples)
    rep["with_fingers"] = sum(1 for s in samples if s["meta"]["fingers"])
    rep["with_keypoints"] = sum(1 for s in samples if s["meta"].get("keypoints"))
    check(len(samples) == args.samples, f"hand: {len(samples)} samples", report)
    check(rep["with_fingers"] == len(samples), f"hand: {rep['with_fingers']} samples carry finger states", report)
    batches = [st.hand_batch(samples[i:i + BATCH]) for i in range(0, len(samples), BATCH)]
    b = batches[0]
    check(tuple(b["image"].shape[1:]) == (3, 128, 128), f"hand: image shape {tuple(b['image'].shape)}", report)
    fingers = torch.cat([x["fingers"] for x in batches]).unique().tolist()
    check(set(fingers) <= {0.0, 1.0}, f"hand: finger values {fingers} in {{0, 1}}", report)
    check(rep["with_keypoints"] == 0, f"hand: {rep['with_keypoints']} samples carry keypoints (must be 0)", report)

    model = st.HandStudent(pretrained=True)
    rep["params"] = sum(p.numel() for p in model.parameters())
    with torch.no_grad():
        k, f = model.eval()(b["image"][:2])
    check(tuple(k.shape) == (2, 21, 2) and tuple(f.shape) == (2, 5),
          f"hand: outputs {tuple(k.shape)} + {tuple(f.shape)}", report)
    train("hand", model, batches, st.hand_loss, args.steps, args.device, report)

    mlmodel, path = export("hand_mnv4s_128", model, (b["image"][:1],), [
        ct.ImageType(name="image", shape=(1, 3, 128, 128), color_layout=ct.colorlayout.RGB, scale=1 / 255),
    ])
    img = pil(b["image"][0])
    image_q = torch.from_numpy(np.asarray(img).copy()).permute(2, 0, 1).float().unsqueeze(0) / 255
    with torch.no_grad():
        rk, rf = model(image_q)
    pred = mlmodel.predict({"image": img})
    outs = sorted(pred.values(), key=lambda v: np.asarray(v).size)  # (1,5) then (1,21,2)
    diff_f = float(np.abs(np.asarray(outs[0]) - rf.numpy()).max())
    diff_k = float(np.abs(np.asarray(outs[1]) - rk.numpy()).max())
    rep.update(coreml=str(path.relative_to(ROOT)), coreml_max_abs_diff_fingers=round(diff_f, 4),
               coreml_max_abs_diff_keypoints=round(diff_k, 4))
    check(diff_k < 0.02 and diff_f < 0.1,
          f"hand: CoreML vs PyTorch max abs diff keypoints {diff_k:.4f} < 0.02, finger logits {diff_f:.4f} < 0.1",
          report)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--session", default="2026-10-27-S1")
    ap.add_argument("--samples", type=int, default=64)
    ap.add_argument("--steps", type=int, default=20)
    ap.add_argument("--device", default="mps" if torch.backends.mps.is_available() else "cpu")
    args = ap.parse_args()
    torch.manual_seed(0)
    report = {"args": vars(args), "torch": torch.__version__, "coremltools": ct.__version__,
              "backbone": st.BACKBONE, "checks": [], "seg": {}, "hand": {}}

    shard = PACKED / "seg" / args.session / "train-00000.tar"
    run_seg(st.read_samples(shard, args.samples, keep=lambda s: s["meta"]["has_prev"]), args, report)
    shard = PACKED / "hand" / args.session / "train-00000.tar"
    run_hand(st.read_samples(shard, args.samples, keep=lambda s: s["meta"]["fingers"] is not None),
             args, report)

    ok = all(c["ok"] for c in report["checks"])
    report["passed"] = ok
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "preflight.json").write_text(json.dumps(report, indent=1))
    print("PREFLIGHT " + ("PASSED" if ok else "FAILED"))
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
