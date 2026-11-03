# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "pillow"]
# ///
"""Pack SAM-labelled sessions into tar shards for training (train.setup).

  uv run tools/train/pack.py [--sessions S1 S2 ...] [--sets seg,proxy,hand,face]
      [--out data/packed] [--workers 8] [--limit N]

Sets (one tar shard family each, about 2,000 samples per shard):
  seg    student seg (D10, D11): frame 384x216 JPEG + SAM class mask, plus
         the previous extracted frame and its mask for the prev-mask input
         channel and the temporal-consistency loss.
  proxy  proxy teacher (D9): frame 768x432 + mask, no previous frame.
  hand   128 px crops around SAM hand boxes, cut from the 1280x720 frame;
         finger state (5 bits thumb..pinky) only where label_ok, the segment
         is not excluded and the hand side matches; 21 teacher keypoints
         where the vision-keypoints tool matched a hand to that SAM instance:
         MediaPipe Hand Landmarker (Apache-2.0) first, SAM 3D Body where
         MediaPipe missed it. Apple Vision output must never be packed
         (DPLA 3.2, Noa 2026-11-02).
  face   128 px crop around the SAM face box; mask 0 other, 1 eye, 2 mouth.

--sets limits a rerun to some sets: only their shards and manifest entries
are rewritten.

Shard layout: <out>/<set>/<session>/<split>-NNNNN.tar, members
<key>.<ext> (webdataset convention, key = "<session>_<frame idx>[_hN]").
Sample order inside a (set, session, split) is shuffled with a fixed seed,
so each shard spans the whole session.

Split (train.setup.split): S4 (manifest split=heldout) is all 'heldout'.
In every other session, per block, the segments whose sha1(session/id)
sorts lowest are 'dev': max(1, round(0.15 * n)) of n >= 3 segments, none
for smaller blocks. Frames outside any segment take the split of the
nearest segment in time, so dev neighbours never leak into train.

Decisions where the spec is open (written down in probes/train-packer/RESULT.md):
  * mask downscale: nearest neighbour.
  * crops: square, side = 1.25 x max(box w, h) (hands) or 1.3 x (faces),
    centred on the box, zero padding outside the frame. Hands need a box
    side >= 32 px, faces >= 48 px.
  * hand side: teacher chirality when the crop has keypoints; otherwise
    two hands in a frame -> image-left is the person's right
    hand (recordings are unmirrored); one hand -> left or right of the face
    centre (fallback person box centre, then frame centre), shifted by half
    the face width toward the side the script expects when it expects one.
  * per-segment labels Noa confirmed wrong (S3 D_step_back, F_backlit, and
    F_dim before the light goes off) are dropped; the SAM masks stay. This
    replaces the earlier Spark QA exclusion list.
  * block H motion labels are stored as 'motion_user' (as scripted, mirrored
    user view) and 'motion_rec' (x flipped to the recorded frame: left<->right,
    cw<->ccw).
"""

import argparse
import hashlib
import io
import json
import random
import tarfile
import time
from collections import Counter, defaultdict
from multiprocessing import Pool
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "data"
SESSIONS = ["2026-10-27-S1", "2026-10-29-S2", "2026-11-01-S3", "2026-11-01-S4"]
SETS = ["seg", "proxy", "hand", "face"]
CLASSES = ["background", "person", "face", "eye", "mouth", "hand", "object"]
FACE_CLASSES = ["other", "eye", "mouth"]
SEG_SIZE = (384, 216)
PROXY_SIZE = (768, 432)
CROP = 128
HAND_PAD, FACE_PAD = 1.25, 1.3
HAND_MIN_PX, FACE_MIN_PX = 32, 48
SHARD_SAMPLES = 2000
DEV_FRACTION = 0.15
SEED = 20261102
JPEG_Q = 90
PREV_MAX_DT = 0.1  # s; previous extracted frame must be the direct neighbour at 15 fps

# Per-segment labels Noa confirmed wrong in his S3 spot-check (2026-11-02):
# he did not step back fully, and the backlight setup failed. The Spark QA
# list it replaces was wrong on 21 of 24 S3 flags.
EXCLUDED = {
    "2026-11-01-S3": {"D_step_back", "F_backlit"},
}
# Labels valid only from this time on (cam0 seconds): S3 F_dim, the light went
# off mid-segment; mean frame luminance falls from 69.5 at 641.0 s to 43.9 at
# 641.2 s (probes/train-packer/RESULT.md).
LABEL_FROM_S = {
    ("2026-11-01-S3", "F_dim"): 641.1,
}

# Hand keypoint teacher: named explicitly, never globbed from data/keypoints.
KEYPOINT_FILE = "keypoints_teacher.jsonl"
KEYPOINT_META = "teacher_meta.json"
KEYPOINT_SOURCES = {"mediapipe": "mediapipe-hand-landmarker", "sam3dbody": "sam-3d-body"}
# SAM 3D Body fingertips end short (hand about 10 % small, probes/hand-keypoints),
# so they get conf 0 and drop out of the keypoint loss.
SAM3DBODY_MASKED = {"thumb_tip", "index_tip", "middle_tip", "ring_tip", "little_tip"}
JOINTS = [
    "wrist",
    "thumb_cmc", "thumb_mp", "thumb_ip", "thumb_tip",
    "index_mcp", "index_pip", "index_dip", "index_tip",
    "middle_mcp", "middle_pip", "middle_dip", "middle_tip",
    "ring_mcp", "ring_pip", "ring_dip", "ring_tip",
    "little_mcp", "little_pip", "little_dip", "little_tip",
]


def mirror_motion(label):
    """User-view motion label -> recorded-frame label (x flipped)."""
    swap = {"left": "right", "right": "left", "cw": "ccw", "ccw": "cw"}
    return "_".join(swap.get(p, p) for p in label.split("_"))


def dev_segments(session, segments):
    by_block = defaultdict(list)
    for seg in segments:
        by_block[seg["block"]].append(seg["id"])
    dev = set()
    for ids in by_block.values():
        if len(ids) < 3:
            continue
        n_dev = max(1, int(DEV_FRACTION * len(ids) + 0.5))
        ranked = sorted(ids, key=lambda i: hashlib.sha1(f"{session}/{i}".encode()).hexdigest())
        dev.update(ranked[:n_dev])
    return dev


def load_keypoints(session):
    """SAM instance id '<chunk>/<file>#<n>' -> (joints 21x3 normalised with conf, chirality, source)."""
    d = DATA / "keypoints" / session / "cam0"
    if not (d / KEYPOINT_FILE).exists():
        return {}, None
    meta = json.loads((d / KEYPOINT_META).read_text())
    for key, name in KEYPOINT_SOURCES.items():
        assert meta["sources"][key]["source"] == name, meta["sources"][key]["source"]
    out = {}
    for line in (d / KEYPOINT_FILE).read_text().splitlines():
        for hand in json.loads(line)["hands"]:
            assert hand["source"] in KEYPOINT_SOURCES, hand["source"]
            masked = SAM3DBODY_MASKED if hand["source"] == "sam3dbody" else set()
            pts = [hand["joints"][n][:2] + [0.0 if n in masked else 1.0] for n in JOINTS]
            out[hand["sam_id"]] = (pts, hand["chirality"], hand["source"])
    licences = ", ".join(f"{v['source']}: {v['licence']}" for v in meta["sources"].values())
    return out, f"{KEYPOINT_FILE} ({licences})"


def load_session(session, limit=None):
    fdir = DATA / "frames" / session / "cam0"
    index = json.loads((fdir / "frames_index.json").read_text())
    segments = [s for s in json.loads((DATA / "recordings" / session / "segments.json").read_text())
                if not s.get("bad")]
    heldout = json.loads((DATA / "labels" / session / "manifest.json").read_text()).get("split") == "heldout"
    seg_by_id = {s["id"]: s for s in segments}
    dev = set() if heldout else dev_segments(session, segments)

    # SAM labels: chunk.json instances + painted PNG per frame
    labels = {}
    for chunk in sorted((DATA / "labels" / session / "cam0").glob("chunk_*")):
        for fname, rec in json.loads((chunk / "chunk.json").read_text())["frames"].items():
            labels[fname] = (chunk / fname.replace(".jpg", ".png"), rec["instances"])

    def split_of(frame):
        if heldout:
            return "heldout"
        sid = frame["segment_id"]
        if sid is None:
            t = frame["t"]
            near = min(segments, key=lambda s: max(s["start_s_cam0"] - t, t - s["end_s_cam0"], 0))
            sid = near["id"]
        return "dev" if sid in dev else "train"

    frames = [f for f in index["frames"] if f["file"] in labels]
    by_idx = {f["idx"]: f for f in frames}
    jobs = []
    for f in frames:
        sid = f["segment_id"]
        seg = seg_by_id.get(sid)
        excluded = (sid in EXCLUDED.get(session, set())
                    or f["t"] < LABEL_FROM_S.get((session, sid), float("-inf")))
        seg_labels = f["labels"] if f["label_ok"] and not excluded else None
        if seg_labels and "motion" in seg_labels:
            seg_labels = {"motion_user": seg_labels["motion"],
                          "motion_rec": mirror_motion(seg_labels["motion"])}
        prev = by_idx.get(f["idx"] - 1)
        if prev is not None and f["t"] - prev["t"] > PREV_MAX_DT:
            prev = None
        png, insts = labels[f["file"]]
        jobs.append({
            "session": session,
            "idx": f["idx"],
            "file": f["file"],
            "t": f["t"],
            "segment_id": sid,
            "block": seg["block"] if seg else None,
            "split": split_of(f),
            "label_ok": bool(f["label_ok"]),
            "excluded": excluded,
            "labels": seg_labels,
            "jpg": str(fdir / f["file"]),
            "png": str(png),
            "instances": [dict({k: i[k] for k in ("concept", "class_id", "box_xywh", "score")}, n=n)
                          for n, i in enumerate(insts)],
            "prev": {"idx": prev["idx"], "jpg": str(fdir / prev["file"]),
                     "png": str(labels[prev["file"]][0])} if prev else None,
        })
    if limit:
        jobs = jobs[:limit]
    return jobs, {"dev_segments": sorted(dev), "heldout": heldout}


def encode(img, fmt):
    buf = io.BytesIO()
    if fmt == "jpg":
        img.save(buf, "JPEG", quality=JPEG_Q)
    else:
        img.save(buf, "PNG", optimize=False, compress_level=6)
    return buf.getvalue()


def boxes_px(job, concept, w, h):
    out = []
    for i in job["instances"]:
        if i["concept"] != concept or not i.get("box_xywh"):
            continue
        x, y, bw, bh = i["box_xywh"]
        # S1 video-mode boxes are normalised to 0..1; image mode (S2-S4 and
        # the S1 mug/pen redo) stores pixels.
        if max(i["box_xywh"]) <= 1.5:
            x, y, bw, bh = x * w, y * h, bw * w, bh * h
        out.append({"box": [x, y, bw, bh], "score": i["score"], "n": i["n"]})
    return sorted(out, key=lambda b: -(b["score"] or 0))


def square(box, pad):
    x, y, bw, bh = box
    side = pad * max(bw, bh)
    cx, cy = x + bw / 2, y + bh / 2
    return [cx - side / 2, cy - side / 2, side]


def crop(img, sq, resample):
    x0, y0, side = sq
    box = tuple(int(round(v)) for v in (x0, y0, x0 + side, y0 + side))
    return img.crop(box).resize((CROP, CROP), resample)


def hand_sides(hands, ref, expected):
    """Person's side per hand; image-left is the person's right (unmirrored)."""
    if len(hands) >= 2:
        order = sorted(range(len(hands)), key=lambda k: hands[k]["box"][0] + hands[k]["box"][2] / 2)
        sides = [None] * len(hands)
        sides[order[0]], sides[order[-1]] = "R", "L"
        return sides
    # One hand: left or right of the reference centre. When the script expects
    # exactly one hand, that side wins up to half a reference width past the
    # centre, so a hand held in front of the face keeps its label.
    ref_x, tol = ref[0] + ref[2] / 2, 0.0
    if len(expected) == 1:
        tol = ref[2] / 2 if expected[0] == "R" else -ref[2] / 2
    return ["R" if h["box"][0] + h["box"][2] / 2 < ref_x + tol else "L" for h in hands]


def finger_label(job, side):
    labels = job["labels"] or {}
    value = labels.get(f"hand_{side}")
    if isinstance(value, str) and len(value) == 5 and set(value) <= {"0", "1"}:
        return [int(c) for c in value]
    return None


def process(args):
    job, sets = args
    key = f"{job['session']}_{job['idx']:06d}"
    frame = Image.open(job["jpg"]).convert("RGB")
    mask = Image.open(job["png"])
    w, h = frame.size
    meta = {k: job[k] for k in ("session", "idx", "file", "t", "segment_id", "block", "split",
                                "label_ok", "excluded", "labels")}
    out = defaultdict(list)
    stats = defaultdict(Counter)

    if "seg" in sets or "proxy" in sets:
        present = np.bincount(np.asarray(mask).ravel(), minlength=len(CLASSES))
        for c, n in enumerate(present[:len(CLASSES)]):
            if n:
                stats["frames_with_class"][CLASSES[c]] += 1
                stats["pixels"][CLASSES[c]] += int(n)

    if "seg" in sets:
        sample = {"jpg": encode(frame.resize(SEG_SIZE, Image.BILINEAR), "jpg"),
                  "mask.png": encode(mask.resize(SEG_SIZE, Image.NEAREST), "png")}
        m = dict(meta, has_prev=job["prev"] is not None)
        if job["prev"]:
            pf = Image.open(job["prev"]["jpg"]).convert("RGB")
            sample["prev.jpg"] = encode(pf.resize(SEG_SIZE, Image.BILINEAR), "jpg")
            sample["prev_mask.png"] = encode(Image.open(job["prev"]["png"]).resize(SEG_SIZE, Image.NEAREST), "png")
            m["prev_idx"] = job["prev"]["idx"]
            stats["seg"]["with_prev"] += 1
        sample["json"] = json.dumps(m).encode()
        out["seg"].append((key, sample))

    if "proxy" in sets:
        out["proxy"].append((key, {"jpg": encode(frame.resize(PROXY_SIZE, Image.BILINEAR), "jpg"),
                                   "mask.png": encode(mask.resize(PROXY_SIZE, Image.NEAREST), "png"),
                                   "json": json.dumps(meta).encode()}))

    faces = boxes_px(job, "face", w, h)
    if "hand" in sets:
        hands = [b for b in boxes_px(job, "hand", w, h) if max(b["box"][2:]) >= HAND_MIN_PX][:2]
        persons = boxes_px(job, "person", w, h)
        ref = faces[0]["box"] if faces else persons[0]["box"] if persons else [w / 2, 0, 0, 0]
        expected = [sd for sd in "RL" if finger_label(job, sd)]
        for n, (hb, geo_side) in enumerate(zip(hands, hand_sides(hands, ref, expected))):
            sq = square(hb["box"], HAND_PAD)
            kp = job["keypoints"].get(hb["n"])
            # Teacher chirality is anatomical and agrees with the protocol on
            # 99 % or more of hands (probes/hand-keypoints); geometry is the fallback.
            side = kp[1][0].upper() if kp else geo_side
            fingers = finger_label(job, side)
            m = dict(meta, side=side, side_source="kp_chirality" if kp else "geometry",
                     sam_box=[round(v, 1) for v in hb["box"]], sam_score=hb["score"],
                     crop_box=[round(v, 1) for v in sq], fingers=fingers, keypoints=None, kp_source=None)
            if kp:
                x0, y0, side_px = sq
                m["keypoints"] = [[round((px * w - x0) / side_px, 4), round((py * h - y0) / side_px, 4), c]
                                  for px, py, c in kp[0]]
                m["kp_source"] = kp[2]
                stats["hand"]["with_keypoints"] += 1
                stats["kp_source"][kp[2]] += 1
                stats["geometry_vs_kp_side"][f"{geo_side}_{side}"] += 1
            out["hand"].append((f"{key}_h{n}", {"jpg": encode(crop(frame, sq, Image.BILINEAR), "jpg"),
                                                "json": json.dumps(m).encode()}))
            stats["hand"]["crops"] += 1
            stats["hand"][f"side_{side}"] += 1
            if fingers:
                stats["hand"]["with_fingers"] += 1
                stats["finger_state"]["".join(map(str, fingers))] += 1

    if "face" in sets and faces and max(faces[0]["box"][2:]) >= FACE_MIN_PX:
        sq = square(faces[0]["box"], FACE_PAD)
        sem = np.asarray(crop(mask, sq, Image.NEAREST))
        fm = np.zeros_like(sem)
        fm[sem == 3] = 1
        fm[sem == 4] = 2
        face_label = (job["labels"] or {}).get("face")
        m = dict(meta, face=face_label, sam_box=[round(v, 1) for v in faces[0]["box"]],
                 crop_box=[round(v, 1) for v in sq])
        out["face"].append((key, {"jpg": encode(crop(frame, sq, Image.BILINEAR), "jpg"),
                                  "mask.png": encode(Image.fromarray(fm), "png"),
                                  "json": json.dumps(m).encode()}))
        stats["face"]["crops"] += 1
        for c in (1, 2):
            if (fm == c).any():
                stats["face"][f"with_{FACE_CLASSES[c]}"] += 1
        if face_label:
            stats["face_label"][face_label] += 1
    return job["split"], out, stats


class ShardWriter:
    def __init__(self, directory, split):
        self.dir, self.split = directory, split
        self.n, self.tar, self.shards = 0, None, []

    def write(self, key, sample):
        if self.tar is None or self.n == SHARD_SAMPLES:
            self.close()
            path = self.dir / f"{self.split}-{len(self.shards):05d}.tar"
            self.tar = tarfile.open(path, "w")
            self.shards.append({"name": path.name, "samples": 0})
            self.n = 0
        for ext, data in sample.items():
            info = tarfile.TarInfo(f"{key}.{ext}")
            info.size, info.mtime = len(data), 0
            self.tar.addfile(info, io.BytesIO(data))
        self.n += 1
        self.shards[-1]["samples"] += 1

    def close(self):
        if self.tar is not None:
            self.tar.close()
            last = self.shards[-1]
            last["bytes"] = (self.dir / last["name"]).stat().st_size
            self.tar = None


def pack_session(session, sets, out_root, pool, limit):
    t0 = time.time()
    jobs, info = load_session(session, limit)
    keypoints, kp_source = load_keypoints(session) if "hand" in sets else ({}, None)
    for j in jobs:
        sam_ids = {i["n"]: f"{Path(j['png']).parent.name}/{j['file']}#{i['n']}" for i in j["instances"]}
        j["keypoints"] = {n: keypoints[sid] for n, sid in sam_ids.items() if sid in keypoints}
    by_split = defaultdict(list)
    for j in jobs:
        by_split[j["split"]].append(j)
    entry = {s: {} for s in sets}
    for split, split_jobs in sorted(by_split.items()):
        random.Random(f"{SEED}/{session}/{split}").shuffle(split_jobs)
        writers, totals = {}, defaultdict(Counter)
        for name in sets:
            d = out_root / name / session
            d.mkdir(parents=True, exist_ok=True)
            for old in d.glob(f"{split}-*.tar"):
                old.unlink()
            writers[name] = ShardWriter(d, split)
        args = ((j, sets) for j in split_jobs)
        for _, out, stats in pool.imap(process, args, chunksize=16):
            for name, samples in out.items():
                for key, sample in samples:
                    writers[name].write(key, sample)
            for group, c in stats.items():
                totals[group].update(c)
        for name in sets:
            writers[name].close()
            shards = writers[name].shards
            groups = {"seg": ["frames_with_class", "pixels", "seg"], "proxy": ["frames_with_class", "pixels"],
                      "hand": ["hand", "finger_state", "kp_source", "geometry_vs_kp_side"], "face": ["face", "face_label"]}[name]
            entry[name][split] = {
                "samples": sum(s["samples"] for s in shards),
                "frames": len(split_jobs),
                "shards": shards,
                "bytes": sum(s.get("bytes", 0) for s in shards),
                **{g: dict(sorted(totals[g].items())) for g in groups if totals[g]},
            }
    if "hand" in entry:
        entry["hand"]["keypoint_source"] = kp_source
    seconds = round(time.time() - t0, 1)
    print(f"{session}: {len(jobs)} frames, {seconds} s, " +
          ", ".join(f"{n} {sum(v['samples'] for k, v in entry[n].items() if isinstance(v, dict))}" for n in sets),
          flush=True)
    return entry, info, seconds


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sessions", nargs="+", default=SESSIONS)
    ap.add_argument("--sets", default=",".join(SETS))
    ap.add_argument("--out", type=Path, default=DATA / "packed")
    ap.add_argument("--workers", type=int, default=8)
    ap.add_argument("--limit", type=int, default=None, help="frames per session, for a quick test")
    a = ap.parse_args()
    sets = [s for s in a.sets.split(",") if s]
    assert set(sets) <= set(SETS), sets
    a.out.mkdir(parents=True, exist_ok=True)
    man_path = a.out / "manifest.json"
    manifest = json.loads(man_path.read_text()) if man_path.exists() else {"sets": {}, "sessions": {}}
    manifest.update({
        "classes": CLASSES, "face_classes": FACE_CLASSES, "joints": JOINTS,
        "finger_bits": "thumb,index,middle,ring,pinky; 1 = extended",
        "sizes": {"seg": SEG_SIZE, "proxy": PROXY_SIZE, "crop": CROP},
        "shard_samples": SHARD_SAMPLES, "seed": SEED, "dev_fraction": DEV_FRACTION,
        "excluded_segments": {s: sorted(v) for s, v in EXCLUDED.items()},
        "labels_from_s": {f"{s}/{sid}": t for (s, sid), t in LABEL_FROM_S.items()},
    })
    with Pool(a.workers) as pool:
        for session in a.sessions:
            entry, info, seconds = pack_session(session, sets, a.out, pool, a.limit)
            for name in sets:
                manifest["sets"].setdefault(name, {})[session] = entry[name]
            manifest["sessions"][session] = dict(info, last_pack_s=seconds, last_pack_sets=sets,
                                                 limit=a.limit)
    man_path.write_text(json.dumps(manifest, indent=1))
    print(f"manifest: {man_path}")


if __name__ == "__main__":
    main()
