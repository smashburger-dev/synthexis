# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""One teacher file per session: MediaPipe first, SAM 3D Body where MediaPipe has no hand.

Reads  data/keypoints/<session>/cam0/keypoints_mediapipe.jsonl and keypoints_sam3dbody.jsonl
Writes data/keypoints/<session>/cam0/keypoints_teacher.jsonl, one line per indexed frame:
         {file, t, hands: [{sam_id, source, chirality, joints, box}]}
       data/keypoints/<session>/cam0/teacher_meta.json

Only train_ok hands. Per SAM hand id the MediaPipe hand wins: on open and
extended fingers it reaches the real fingertips, while SAM 3D Body's tips end
short of them (median hand 10 % smaller, 18 px lower on S3) and its finger
states match the protocol less often. SAM 3D Body fills the SAM hands
MediaPipe missed, mostly fists, occluded hands and hands holding objects.
Chirality: SAM 3D Body's when it has the same hand (99.7 to 100 % vs the
protocol), else MediaPipe's. joints are [x, y] normalised to the
unmirrored frame; MediaPipe's z is dropped.

Usage: uv run tools/keypoints/merge.py <session>...
"""

import json
import sys
from collections import Counter
from pathlib import Path

DATA = Path(__file__).resolve().parents[2] / "data"


def load(session, src):
    path = DATA / "keypoints" / session / "cam0" / f"keypoints_{src}.jsonl"
    return {r["file"]: r for r in map(json.loads, path.read_text().splitlines())}


def ok(rec):
    return {h["sam_match"]["sam_id"]: h for h in rec["hands"] if h["train_ok"]} if rec else {}


def merge(session):
    mp, s3 = load(session, "mediapipe"), load(session, "sam3dbody")
    index = json.loads((DATA / "frames" / session / "cam0" / "frames_index.json").read_text())["frames"]
    counts, lines = Counter(), []
    for f in index:
        hm, hs = ok(mp.get(f["file"])), ok(s3.get(f["file"]))
        hands = []
        for sid in sorted(hm.keys() | hs.keys()):
            src, h = ("mediapipe", hm[sid]) if sid in hm else ("sam3dbody", hs[sid])
            chir = hs[sid]["chirality"] if sid in hs else h["chirality"]
            counts[src] += 1
            if sid in hm and sid in hs:
                counts["both"] += 1
                counts["chirality_differs"] += hm[sid]["chirality"] != hs[sid]["chirality"]
            hands.append({
                "sam_id": sid, "source": src, "chirality": chir,
                "joints": {n: p[:2] for n, p in h["joints"].items()}, "box": h["box"],
            })
        lines.append(json.dumps({"file": f["file"], "t": f["t"], "hands": hands}, separators=(",", ":")))
    out = DATA / "keypoints" / session / "cam0"
    (out / "keypoints_teacher.jsonl").write_text("\n".join(lines) + "\n")
    meta = {
        "session": session,
        "rule": "per SAM hand: MediaPipe train_ok hand, else SAM 3D Body train_ok hand; chirality from SAM 3D Body when present",
        "hands": counts["mediapipe"] + counts["sam3dbody"],
        "from_mediapipe": counts["mediapipe"], "from_sam3dbody": counts["sam3dbody"],
        "both_sources": counts["both"], "chirality_differs_where_both": counts["chirality_differs"],
        "sources": {
            "mediapipe": json.loads((out / "mediapipe_meta.json").read_text()),
            "sam3dbody": json.loads((out / "sam3dbody_meta.json").read_text()),
        },
    }
    (out / "teacher_meta.json").write_text(json.dumps(meta, indent=1))
    return meta


if __name__ == "__main__":
    for s in sys.argv[1:]:
        m = merge(s)
        print(s, {k: m[k] for k in ("hands", "from_mediapipe", "from_sam3dbody", "both_sources", "chirality_differs_where_both")})
