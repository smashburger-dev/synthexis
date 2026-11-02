# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Agreement between two keypoint sources on the same SAM hands.

Reads  data/keypoints/<session>/cam0/keypoints_<a>.jsonl and keypoints_<b>.jsonl
       (tools/keypoints/postprocess.py), frames_index.json, segments.json
Writes data/keypoints/<session>/cam0/compare_<a>_<b>.json

Only frames that source b processed count (SAM 3D Body ran on every second
SAM-hand frame). A pair is the two sources' hands matched to the same SAM
hand id, both train_ok. Error per joint: pixel distance divided by the hand
size (longer side of the mean of the two joint boxes, in pixels). PCK@0.1:
share of joints within 10 % of the hand size.

Usage: uv run tools/keypoints/compare.py [--a mediapipe] [--b sam3dbody] <session>...
"""

import argparse
import json
from collections import defaultdict
from pathlib import Path

DATA = Path(__file__).resolve().parents[2] / "data"
W, H = 1280, 720


def load(session, src):
    path = DATA / "keypoints" / session / "cam0" / f"keypoints_{src}.jsonl"
    return {r["file"]: r for r in map(json.loads, path.read_text().splitlines())}


def by_sam(rec):
    return {h["sam_match"]["sam_id"]: h for h in rec["hands"] if h["train_ok"]}


def pct(vals, q):
    vals = sorted(vals)
    return round(vals[min(len(vals) - 1, int(q * len(vals)))], 4) if vals else None


def summary(errs):
    flat = [e for v in errs.values() for e in v]
    return {
        "joints": len(flat),
        "median": pct(flat, 0.5),
        "p90": pct(flat, 0.9),
        "pck_0.1": round(sum(e < 0.1 for e in flat) / len(flat), 4) if flat else None,
    }


def compare(session, a, b):
    ka, kb = load(session, a), load(session, b)
    index = json.loads((DATA / "frames" / session / "cam0" / "frames_index.json").read_text())["frames"]
    segs = {s["id"]: s for s in json.loads((DATA / "recordings" / session / "segments.json").read_text())}
    raw_b = DATA / "keypoints" / session / "cam0" / f"{b}.jsonl"
    processed = {r["file"] for r in map(json.loads, raw_b.read_text().splitlines()) if r["hands"]}

    per_joint = defaultdict(list)
    per_block = defaultdict(lambda: defaultdict(list))
    counts = defaultdict(lambda: defaultdict(int))
    for f in index:
        if f["file"] not in processed:
            continue
        block = segs[f["segment_id"]]["block"] if f["segment_id"] else "gap"
        ha, hb = by_sam(ka[f["file"]]), by_sam(kb[f["file"]])
        c = counts[block]
        c["frames"] += 1
        c[f"{a}_train_ok"] += len(ha)
        c[f"{b}_train_ok"] += len(hb)
        for sid in ha.keys() & hb.keys():
            pa, pb = ha[sid], hb[sid]
            c["pairs"] += 1
            c["chirality_same"] += pa["chirality"] == pb["chirality"]
            size = max((pa["box"][2] + pb["box"][2]) / 2 * W, (pa["box"][3] + pb["box"][3]) / 2 * H, 1.0)
            for name, ja in pa["joints"].items():
                jb = pb["joints"][name]
                e = (((ja[0] - jb[0]) * W) ** 2 + ((ja[1] - jb[1]) * H) ** 2) ** 0.5 / size
                per_joint[name].append(e)
                per_block[block][name].append(e)
    total = defaultdict(int)
    for c in counts.values():
        for k, v in c.items():
            total[k] += v
    out = {
        "session": session, "a": a, "b": b,
        "counts": dict(total),
        "chirality_agreement": round(total["chirality_same"] / max(total["pairs"], 1), 4),
        "all": summary(per_joint),
        "per_joint": {n: summary({n: v}) for n, v in per_joint.items()},
        "per_block": {k: {**summary(v), **counts[k]} for k, v in sorted(per_block.items())},
    }
    (DATA / "keypoints" / session / "cam0" / f"compare_{a}_{b}.json").write_text(json.dumps(out, indent=1))
    return out


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--a", default="mediapipe")
    ap.add_argument("--b", default="sam3dbody")
    ap.add_argument("sessions", nargs="+")
    args = ap.parse_args()
    for s in args.sessions:
        o = compare(s, args.a, args.b)
        print(s, json.dumps({"counts": o["counts"], "chirality_agreement": o["chirality_agreement"], "all": o["all"]}))
