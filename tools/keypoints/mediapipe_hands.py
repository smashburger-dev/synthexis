# /// script
# requires-python = ">=3.11,<3.13"
# dependencies = ["mediapipe==1.2.0", "numpy"]
# ///
"""Runs MediaPipe Hand Landmarker (Apache-2.0) on every indexed frame of a session.

Reads  data/frames/<session>/cam0/frames_index.json and its jpgs
       data/models/hand_landmarker.task (float16 bundle from Google's
       mediapipe-models bucket, sha256 checked below)
Writes data/keypoints/<session>/cam0/mediapipe.jsonl, one line per frame in
       index order: {file, t, hands: [{chirality, mp_handedness, score,
       joints: {name: [x, y, z]}}]}
       data/keypoints/<session>/cam0/mediapipe_meta.json

Image mode: every frame is detected on its own, no tracking state, so a
label never depends on the frames before it. x, y are normalised to the
unmirrored frame, origin top-left; z is MediaPipe's relative depth (wrist = 0,
same scale as x). MediaPipe gives no per-joint confidence; `score` is the
handedness score of the hand.

MediaPipe's docs say handedness assumes a mirrored selfie image, but on our
unmirrored frames it already names the anatomical hand (S1 one-hand A
segments, first 600 frames: 279 of 282 "Right" in A_R_*), so `chirality` is
`mp_handedness` lower-cased. postprocess.py measures this on all frames.

Not Apple Vision: Apple's terms forbid training on Vision output.

Usage: uv run tools/keypoints/mediapipe_hands.py <session> [--workers 8] [--limit N]
"""

import argparse
import hashlib
import json
import time
from multiprocessing import Pool
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "data"
MODEL = DATA / "models" / "hand_landmarker.task"
MODEL_SHA256 = "fbc2a30080c3c557093b5ddfc334698132eb341044ccee322ccf8bcf3607cde1"
JOINTS = [
    "wrist",
    "thumb_cmc", "thumb_mp", "thumb_ip", "thumb_tip",
    "index_mcp", "index_pip", "index_dip", "index_tip",
    "middle_mcp", "middle_pip", "middle_dip", "middle_tip",
    "ring_mcp", "ring_pip", "ring_dip", "ring_tip",
    "little_mcp", "little_pip", "little_dip", "little_tip",
]  # MediaPipe's landmark order; thumb MCP is called thumb_mp, pinky little

_landmarker = None


def _init():
    global _landmarker
    from mediapipe.tasks.python import BaseOptions
    from mediapipe.tasks.python.vision import HandLandmarker, HandLandmarkerOptions, RunningMode

    _landmarker = HandLandmarker.create_from_options(HandLandmarkerOptions(
        base_options=BaseOptions(model_asset_path=str(MODEL)),
        running_mode=RunningMode.IMAGE,
        num_hands=2,
    ))


def _detect(job):
    import mediapipe as mp

    path, file, t = job
    res = _landmarker.detect(mp.Image.create_from_file(path))
    hands = []
    for lms, hd in zip(res.hand_landmarks, res.handedness):
        cat = hd[0]
        hands.append({
            "chirality": cat.category_name.lower(),
            "mp_handedness": cat.category_name,
            "score": round(cat.score, 4),
            "joints": {n: [round(p.x, 4), round(p.y, 4), round(p.z, 4)] for n, p in zip(JOINTS, lms)},
        })
    return json.dumps({"file": file, "t": t, "hands": hands}, separators=(",", ":"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("session")
    ap.add_argument("--workers", type=int, default=8)
    ap.add_argument("--limit", type=int)
    args = ap.parse_args()

    sha = hashlib.sha256(MODEL.read_bytes()).hexdigest()
    if sha != MODEL_SHA256:
        raise SystemExit(f"model sha256 {sha} != {MODEL_SHA256}")
    frames_dir = DATA / "frames" / args.session / "cam0"
    frames = json.loads((frames_dir / "frames_index.json").read_text())["frames"][: args.limit]
    out_dir = DATA / "keypoints" / args.session / "cam0"
    out_dir.mkdir(parents=True, exist_ok=True)

    jobs = [(str(frames_dir / f["file"]), f["file"], f["t"]) for f in frames]
    start = time.monotonic()
    with Pool(args.workers, initializer=_init) as pool:
        lines = []
        for i, line in enumerate(pool.imap(_detect, jobs, chunksize=32), 1):
            lines.append(line)
            if i % 2000 == 0:
                print(f"{i}/{len(jobs)} {time.monotonic() - start:.0f}s", flush=True)
    run_s = time.monotonic() - start
    (out_dir / "mediapipe.jsonl").write_text("\n".join(lines) + "\n")

    import mediapipe as mp
    meta = {
        "source": "mediapipe-hand-landmarker",
        "licence": "Apache-2.0",
        "mediapipe": mp.__version__,
        "model": MODEL.name,
        "model_sha256": sha,
        "running_mode": "IMAGE",
        "num_hands": 2,
        "frames": len(jobs),
        "workers": args.workers,
        "run_s": round(run_s, 1),
        "ms_per_frame": round(run_s * 1000 / max(len(jobs), 1), 2),
    }
    (out_dir / "mediapipe_meta.json").write_text(json.dumps(meta, indent=1))
    print(json.dumps(meta))


if __name__ == "__main__":
    main()
