# /// script
# requires-python = ">=3.11"
# dependencies = ["modal>=1.6"]
# ///
"""SAM 3D Body (Meta, SAM License) hand keypoints on Modal, as the perc.hand teacher.

  uv run tools/keypoints/sam3d_body.py pilot [--max-usd 0.5]
  uv run tools/keypoints/sam3d_body.py run <session>... --max-usd <cap> [--bf16] [--stride 1]

Model: facebook/sam-3d-body-dinov3 (DINOv3-H+ 840M) at HF revision MODEL_REV,
code facebookresearch/sam-3d-body at CODE_COMMIT. Inference "full": body
decoder, then the hand decoder on each hand crop. Prompt: the SAM 3.1
person box of the frame (no detector, no FOV model, default intrinsics);
frames SAM never labelled get the full frame.

Frames: every indexed frame where SAM 3.1 or MediaPipe saw a hand (37,207
of 51,517 on S1 to S4). They are already on the synthexis-recordings volume
(frames/<session>/cam0, uploaded by tools/labels/run.py).

Writes data/keypoints/<session>/cam0/sam3dbody.jsonl in the same per-frame
schema as mediapipe.jsonl: {file, t, hands: [{chirality, score, joints}]}
with joints {name: [x, y, 0]} normalised to the unmirrored frame. The body
model always predicts both hands, also hidden ones, so `score` is null and
tools/keypoints/postprocess.py --source sam3dbody keeps only hands that
match a SAM hand box. Plus sam3dbody_meta.json (revisions, timing, cost).

Cost guards (AGENTS.md money rules):
  * up-front estimate with ASSUMED_S_PER_FRAME, refused above --max-usd
  * pilot: the first job measures s/frame; if the re-estimate exceeds
    --max-usd the run stops before the rest is spawned
  * rolling launch: no new job once finished + in-flight container
    seconds (load time included) reach --max-usd; running jobs finish
GPU rate L40S 0.000542 $/s (tools/labels/run.py), times OVERHEAD for CPU,
memory and container start.
"""

import argparse
import json
import sys
import time
from pathlib import Path

import modal

CODE_COMMIT = "b5c765a0d89d789985e186d396315e7590887b94"
MODEL_REPO = "facebook/sam-3d-body-dinov3"
MODEL_REV = "11aaa346c7204874a1cbafe3d39a979080b2c55a"
RATE = 0.000542
OVERHEAD = 1.15
ASSUMED_S_PER_FRAME = 0.30
JOB_FRAMES = 300
MAX_CONTAINERS = 4
WEIGHTS = "/weights"
VOL = "/vol"

FINGERS = ["thumb", "index", "middle", "ring", "little"]
# MHR70 order per hand (sam_3d_body/metadata/mhr70.py): per finger tip,
# first, second, third joint (first = next to the tip), then the wrist.
# Index of the right thumb tip is 21, of the left thumb tip 42.
JOINT_NAMES = {
    "thumb": ["thumb_tip", "thumb_ip", "thumb_mp", "thumb_cmc"],
    **{f: [f"{f}_tip", f"{f}_dip", f"{f}_pip", f"{f}_mcp"] for f in FINGERS[1:]},
}
HAND_BASE = {"right": 21, "left": 42}

image = (
    modal.Image.debian_slim(python_version="3.11")
    .apt_install("git", "libgl1", "libglib2.0-0")
    .pip_install(
        "torch==2.10.0",
        "torchvision",
        extra_index_url="https://download.pytorch.org/whl/cu128",
    )
    .pip_install(
        "pytorch-lightning", "opencv-python-headless", "yacs", "einops", "timm",
        "roma", "omegaconf", "braceexpand", "huggingface_hub", "scikit-image",
        "numpy",
    )
    .run_commands(
        "git clone https://github.com/facebookresearch/sam-3d-body.git /opt/sam-3d-body",
        f"cd /opt/sam-3d-body && git checkout {CODE_COMMIT}",
    )
    .env({"PYTHONPATH": "/opt/sam-3d-body", "HF_HUB_CACHE": f"{WEIGHTS}/hf-cache"})
)
weights_vol = modal.Volume.from_name("sam3d-body-weights", create_if_missing=True)
recordings_vol = modal.Volume.from_name("synthexis-recordings")
app = modal.App("synthexis-sam3d-body", image=image)


@app.cls(
    gpu="L40S",
    timeout=1800,
    max_containers=MAX_CONTAINERS,
    secrets=[modal.Secret.from_name("huggingface")],
    volumes={WEIGHTS: weights_vol, VOL: recordings_vol},
)
class Body3D:
    @modal.enter()
    def load(self):
        import torch
        from huggingface_hub import snapshot_download
        from sam_3d_body import SAM3DBodyEstimator, load_sam_3d_body

        t0 = time.time()
        torch.backends.cuda.matmul.allow_tf32 = True
        torch.backends.cudnn.allow_tf32 = True
        local = snapshot_download(MODEL_REPO, revision=MODEL_REV)
        weights_vol.commit()
        model, cfg = load_sam_3d_body(
            f"{local}/model.ckpt", device="cuda", mhr_path=f"{local}/assets/mhr_model.pt"
        )
        self.estimator = SAM3DBodyEstimator(model, cfg)
        self.load_s = time.time() - t0
        self.calls = 0

    @modal.method()
    def infer(self, session: str, items: list, bf16: bool = False) -> dict:
        """items: [[file, [x0, y0, x1, y1] person box in pixels], ...]"""
        import contextlib

        import cv2
        import numpy as np
        import torch

        t0 = time.time()
        out = []
        ctx = torch.autocast("cuda", dtype=torch.bfloat16) if bf16 else contextlib.nullcontext()
        for file, box in items:
            img = cv2.imread(f"{VOL}/frames/{session}/cam0/{file}")
            if img is None:
                out.append({"file": file, "error": "unreadable"})
                continue
            h, w = img.shape[:2]
            rgb = cv2.cvtColor(img, cv2.COLOR_BGR2RGB)
            with ctx:
                res = self.estimator.process_one_image(
                    rgb, bboxes=np.array([box], dtype=np.float32), inference_type="full"
                )
            if not res:
                out.append({"file": file, "people": 0})
                continue
            kp = np.asarray(res[0]["pred_keypoints_2d"], dtype=np.float32)
            hands = []
            for side, base in HAND_BASE.items():
                joints = {"wrist": kp[base + 20]}
                for fi, f in enumerate(FINGERS):
                    for ji, name in enumerate(JOINT_NAMES[f]):
                        joints[name] = kp[base + 4 * fi + ji]
                hands.append({
                    "chirality": side,
                    "joints": {n: [round(float(p[0]) / w, 4), round(float(p[1]) / h, 4)] for n, p in joints.items()},
                })
            out.append({"file": file, "hands": hands})
        torch.cuda.synchronize()
        self.calls += 1
        return {
            "results": out,
            "wall_s": time.time() - t0,
            "load_s": self.load_s if self.calls == 1 else 0.0,
        }


# ---------------------------------------------------------------- local side

# the container imports this file as /root/sam3d_body.py, so only resolve locally
DATA = Path(__file__).resolve().parents[2] / "data" if modal.is_local() else Path("/nonexistent")
SESSIONS = ["2026-10-27-S1", "2026-10-29-S2", "2026-11-01-S3", "2026-11-01-S4"]


def frame_jobs(session: str, stride: int) -> list:
    """[(file, t, person box px)] for frames where SAM or MediaPipe saw a hand."""
    index = json.loads((DATA / "frames" / session / "cam0" / "frames_index.json").read_text())["frames"]
    person, sam_hand = {}, set()
    for chunk in sorted((DATA / "labels" / session / "cam0").glob("chunk_*/chunk.json")):
        for file, rec in json.loads(chunk.read_text())["frames"].items():
            for inst in rec["instances"]:
                if inst["concept"] == "hand":
                    sam_hand.add(file)
                if inst["concept"] == "person":
                    b = inst["box_xywh"]
                    if max(b) <= 1.5:
                        hh, ww = inst["rle"]["size"]
                        b = [b[0] * ww, b[1] * hh, b[2] * ww, b[3] * hh]
                    person[file] = [b[0], b[1], b[0] + b[2], b[1] + b[3]]
    mp_hand = set()
    mp_path = DATA / "keypoints" / session / "cam0" / "mediapipe.jsonl"
    for line in mp_path.read_text().splitlines():
        rec = json.loads(line)
        if rec["hands"]:
            mp_hand.add(rec["file"])
    picked = [f for f in index if f["file"] in sam_hand | mp_hand][::stride]
    return [(f["file"], f["t"], person.get(f["file"], [0, 0, 1280, 720])) for f in picked]


def cost(wall_s: float) -> float:
    return wall_s * RATE * OVERHEAD


def pilot(args):
    """Same 24 frames in fp32 and bf16: s/frame and the joint difference in px."""
    jobs = frame_jobs("2026-10-29-S2", 1)
    sample = jobs[:: max(1, len(jobs) // 24)][:24]
    est = cost(2 * len(sample) * ASSUMED_S_PER_FRAME + 240)
    print(f"pilot up-front estimate ${est:.3f} (cap ${args.max_usd})")
    if est > args.max_usd:
        sys.exit("pilot estimate above cap")
    items = [[f, b] for f, _, b in sample]
    with modal.enable_output(), app.run():
        body = Body3D()
        warm = body.infer.remote("2026-10-29-S2", items[:2])
        a = body.infer.remote("2026-10-29-S2", items)
        b = body.infer.remote("2026-10-29-S2", items, bf16=True)
    spent = cost(warm["load_s"] + warm["wall_s"] + a["wall_s"] + b["wall_s"])
    diffs = []
    for ra, rb in zip(a["results"], b["results"]):
        for ha, hb in zip(ra.get("hands", []), rb.get("hands", [])):
            for n in ha["joints"]:
                dx = (ha["joints"][n][0] - hb["joints"][n][0]) * 1280
                dy = (ha["joints"][n][1] - hb["joints"][n][1]) * 720
                diffs.append((dx * dx + dy * dy) ** 0.5)
    diffs.sort()
    rep = {
        "frames": len(items),
        "load_s": round(warm["load_s"], 1),
        "s_per_frame_fp32": round(a["wall_s"] / len(items), 3),
        "s_per_frame_bf16": round(b["wall_s"] / len(items), 3),
        "bf16_vs_fp32_px_median": round(diffs[len(diffs) // 2], 2) if diffs else None,
        "bf16_vs_fp32_px_p95": round(diffs[int(len(diffs) * 0.95)], 2) if diffs else None,
        "pilot_usd": round(spent, 4),
        "estimate_all_fp32_usd": round(cost(37207 * a["wall_s"] / len(items) + MAX_CONTAINERS * warm["load_s"]), 2),
        "estimate_all_bf16_usd": round(cost(37207 * b["wall_s"] / len(items) + MAX_CONTAINERS * warm["load_s"]), 2),
    }
    out = DATA / "keypoints" / "sam3dbody_pilot.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({"report": rep, "fp32": a["results"], "bf16": b["results"]}))
    print(json.dumps(rep, indent=1))


def run(args):
    jobs = []
    for s in args.sessions:
        frames = frame_jobs(s, args.stride)
        for i in range(0, len(frames), JOB_FRAMES):
            jobs.append((s, frames[i : i + JOB_FRAMES]))
    n_frames = sum(len(fr) for _, fr in jobs)
    s_pf = args.s_per_frame
    est = cost(n_frames * s_pf + MAX_CONTAINERS * 120)
    print(f"{len(jobs)} jobs, {n_frames} frames, up-front estimate ${est:.2f} at {s_pf} s/frame (cap ${args.max_usd})")
    if est > args.max_usd:
        sys.exit("estimate above cap; raise --stride or the cap")

    results = {s: {} for s in args.sessions}
    spent_s, loads, started = 0.0, 0, time.time()
    with modal.enable_output(), app.run():
        body = Body3D()
        pending = list(jobs)
        in_flight = []

        def launch():
            s, frames = pending.pop(0)
            items = [[f, b] for f, _, b in frames]
            in_flight.append((body.infer.spawn(s, items, args.bf16), s, frames, time.time()))

        # pilot guard: first job alone, then re-estimate from its measured speed
        launch()
        call, s, frames, t0 = in_flight.pop()
        r = call.get()
        spent_s += r["wall_s"] + r["load_s"]
        loads += 1
        for rec in r["results"]:
            results[s][rec["file"]] = rec
        measured = r["wall_s"] / len(frames)
        re_est = cost(spent_s + (n_frames - len(frames)) * measured + (MAX_CONTAINERS - 1) * r["load_s"])
        print(f"pilot job: {measured:.3f} s/frame, load {r['load_s']:.0f} s, re-estimate ${re_est:.2f}")
        if re_est > args.max_usd:
            print("re-estimate above cap: stopping after the pilot job")
            pending.clear()

        while pending or in_flight:
            in_flight_s = sum(time.time() - t for *_, t in in_flight)
            while pending and len(in_flight) < MAX_CONTAINERS and cost(spent_s + in_flight_s) < args.max_usd:
                launch()
                in_flight_s = sum(time.time() - t for *_, t in in_flight)
            if pending and cost(spent_s + in_flight_s) >= args.max_usd:
                print(f"spend guard: ${cost(spent_s + in_flight_s):.2f} reached, {len(pending)} jobs not launched")
                pending.clear()
            done = []
            for item in in_flight:
                try:
                    r = item[0].get(timeout=0)
                except Exception as e:  # modal raises its own TimeoutError while the call runs
                    if "Timeout" in type(e).__name__:
                        continue
                    raise
                done.append(item)
                spent_s += r["wall_s"] + r["load_s"]
                loads += r["load_s"] > 0
                for rec in r["results"]:
                    results[item[1]][rec["file"]] = rec
                print(f"  {item[1]} +{len(item[2])} frames, spent ${cost(spent_s):.2f}", flush=True)
            for item in done:
                in_flight.remove(item)
            time.sleep(2)

    for s in args.sessions:
        write_session(s, results[s], args, spent_s, time.time() - started)
    print(f"done: container seconds {spent_s:.0f}, about ${cost(spent_s):.2f}")


def write_session(session, by_file, args, spent_s, run_s):
    index = json.loads((DATA / "frames" / session / "cam0" / "frames_index.json").read_text())["frames"]
    out_dir = DATA / "keypoints" / session / "cam0"
    lines, n_done = [], 0
    for f in index:
        rec = by_file.get(f["file"])
        hands = []
        if rec and "hands" in rec:
            n_done += 1
            hands = [
                {"chirality": h["chirality"], "score": None,
                 "joints": {n: [x, y, 0.0] for n, (x, y) in h["joints"].items()}}
                for h in rec["hands"]
            ]
        lines.append(json.dumps({"file": f["file"], "t": f["t"], "hands": hands}, separators=(",", ":")))
    (out_dir / "sam3dbody.jsonl").write_text("\n".join(lines) + "\n")
    meta = {
        "source": "sam-3d-body",
        "licence": "SAM License (model), MHR Apache-2.0",
        "model": MODEL_REPO, "model_rev": MODEL_REV, "code_commit": CODE_COMMIT,
        "inference_type": "full", "prompt": "SAM 3.1 person box", "bf16": args.bf16,
        "stride": args.stride, "frames_index": len(index), "frames_processed": n_done,
        "gpu": "L40S", "run_container_s_all_sessions": round(spent_s, 1),
        "run_usd_all_sessions_est": round(cost(spent_s), 3), "run_wall_s": round(run_s, 1),
    }
    (out_dir / "sam3dbody_meta.json").write_text(json.dumps(meta, indent=1))


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("pilot")
    p.add_argument("--max-usd", type=float, default=0.5)
    r = sub.add_parser("run")
    r.add_argument("sessions", nargs="+", choices=SESSIONS)
    r.add_argument("--max-usd", type=float, required=True)
    r.add_argument("--bf16", action="store_true")
    r.add_argument("--stride", type=int, default=1)
    r.add_argument("--s-per-frame", type=float, default=ASSUMED_S_PER_FRAME,
                   help="up-front assumption; take it from the pilot")
    args = ap.parse_args()
    {"pilot": pilot, "run": run}[args.cmd](args)


if __name__ == "__main__":
    main()
