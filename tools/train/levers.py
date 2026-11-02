# /// script
# requires-python = ">=3.12"
# dependencies = ["torch>=2.7.0", "timm>=1.0.15", "numpy", "pillow"]
# ///
"""Compare lever runs against their control on the leave-one-session-out
validation (train.py --holdout), and calibrate the face eye post-processing.

  uv run tools/train/levers.py summary hl fl sl      # mean, SE per lever (run prefix -<lever>-s<seed>)
  uv run tools/train/levers.py eye fl-aug            # eye bias x dilation grid on the held-out session

The rule (train.setup.runs): accept a lever only above max(1 point, 2 SE)
over the control. S4 is never read here.
"""

import json
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F

sys.path.insert(0, str(Path(__file__).resolve().parent))
import students as st  # noqa: E402
import train as T  # noqa: E402

RUNS = T.ROOT / "data" / "train" / "runs"


def best_record(run):
    recs = [json.loads(line) for line in (RUNS / run / "metrics.jsonl").read_text().splitlines()]
    return max(recs, key=lambda r: r["dev"]["score"])


def measures(run):
    r = best_record(run)
    d = r["dev"]
    if "variants_state_accuracy" in d:  # hand
        out = {f"state_{k}": v for k, v in d["variants_state_accuracy"].items()}
        out["kp_px"] = d["kp_px_mean"]
        return out
    if "iou_crop" in d:  # face
        return {"eye": d["iou_crop"]["eye"], "mouth": d["iou_crop"]["mouth"]}
    s = json.loads((RUNS / run / "summary.json").read_text())
    out = {f"sam_{c}": d["iou_sam"][c] for c in ("person", "face", "hand", "eye", "mouth")}
    out.update({f"zero_{c}": d["iou_zero"][c] for c in ("person", "face", "hand")})
    if "val_sequential" in s:
        out.update({f"own_{c}": s["val_sequential"]["iou_own"][c] for c in ("person", "face", "hand", "mouth")})
    return out


def summary(prefixes):
    for prefix in prefixes:
        groups = defaultdict(list)
        for d in sorted(RUNS.glob(f"{prefix}-*")):
            if not (d / "metrics.jsonl").exists():
                continue
            lever = d.name[len(prefix) + 1:].rsplit("-s", 1)[0] if "-s" in d.name[len(prefix):] else d.name[len(prefix) + 1:]
            groups[lever].append(measures(d.name))
        ctrl = groups.get("ctrl")
        print(f"== {prefix} (val = held-out session)")
        for lever, rows in sorted(groups.items()):
            keys = rows[0].keys()
            cells = []
            for k in keys:
                v = np.array([r[k] for r in rows if r.get(k) is not None], dtype=float)
                if not len(v):
                    continue
                se = v.std(ddof=1) / np.sqrt(len(v)) if len(v) > 1 else float("nan")
                delta = ""
                if ctrl and lever != "ctrl":
                    c = np.array([r[k] for r in ctrl if r.get(k) is not None], dtype=float)
                    if len(c):
                        delta = f" ({100 * (v.mean() - c.mean()):+.1f})"
                cells.append(f"{k} {v.mean():.3f}+-{se:.3f}{delta}")
            print(f"  {lever:8s} n={len(rows)} " + " | ".join(cells))


@torch.no_grad()
def eye_grid(run, limit=4000):
    """Eye IoU on the held-out session over eye bias x dilation; the face model
    of a holdout run never saw that session, as a live user is never seen."""
    args = json.loads((RUNS / run / "args.json").read_text())
    holdout = args["holdout"]
    dev = "mps" if torch.backends.mps.is_available() else "cpu"
    ck = torch.load(RUNS / run / "best.pt", map_location="cpu", weights_only=False)
    m = st.FaceStudent(pretrained=False)
    m.load_state_dict(ck["model"])
    m = m.to(dev).eval()
    files = T.shard_files(T.ROOT / "data/packed", "face", "train", T.session_names(holdout)) + \
        T.shard_files(T.ROOT / "data/packed", "face", "dev", T.session_names(holdout))
    cfg = {"ignore_eye": {"S2"}, "blocked_prev": set()}
    grid = [(b, k) for b in (0.0, 0.5, 1.0, 1.5, 2.0, 3.0) for k in (1, 3, 5)]
    cms = {g: torch.zeros(3, 3, dtype=torch.long) for g in grid}
    for raw in T.loader(T.Shards(files, "face", cfg, limit=limit), 64, 0, dev, False):
        b = T.prepare(raw, "face", dev)
        logits = m(b["image"]).float()
        for bias, k in grid:
            eye = logits[:, 1:2] + bias
            if k > 1:
                eye = F.max_pool2d(eye, k, stride=1, padding=k // 2)
            pred = torch.cat([logits[:, :1], eye, logits[:, 2:]], 1).argmax(1)
            cms[(bias, k)] += T.confusion(pred, b["mask"], 3)
    res = {f"{b}/{k}": T.iou(cm, T.FACE_CLASSES) for (b, k), cm in cms.items()}
    best = max(res, key=lambda g: res[g]["eye"])
    for g, v in res.items():
        print(f"  bias/dilate {g}: eye {v['eye']} mouth {v['mouth']}" + ("  <- best" if g == best else ""))
    (RUNS / run / "eye_grid.json").write_text(json.dumps({"holdout": holdout, "grid": res, "best": best}, indent=1))
    return best


if __name__ == "__main__":
    if sys.argv[1] == "summary":
        summary(sys.argv[2:])
    else:
        for r in sys.argv[2:]:
            print(r, "best", eye_grid(r))
