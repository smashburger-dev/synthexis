"""Local post-processing of SAM 3.1 raw labels.

Reads raw_<concept>.json files (per-frame COCO RLE instances produced by
the GPU worker) and applies everything that does not need a GPU:

  * top-k per class (person 1, object 1, hand 2)
  * eye/mouth instances must be >=80% inside the same frame's face union
  * hand-area filter: per chunk and track (obj_id; score-rank fallback)
    drop instances larger than 2.5x the track's median area
  * semantic uint8 PNG painting in priority order
    person < face < object < hand < mouth < eye
  * chunk.json (filtered instances) + stats.json

No GPU, no network — pure local CPU.
"""

import json
from pathlib import Path

import numpy as np
from PIL import Image
from pycocotools import mask as mask_utils

CLASS_ID = {
    "person": 1,
    "face": 2,
    "eye": 3,
    "mouth": 4,
    "hand": 5,
    "object": 6,
}
PAINT_ORDER = ["person", "face", "object", "hand", "mouth", "eye"]
KEEP_TOP_K = {
    "person": 1,
    "face": None,
    "object": 1,
    "hand": 2,
    "mouth": None,
    "eye": None,
}
FACE_INSIDE_MIN = 0.8
HAND_AREA_OUTLIER = 2.5


def _decode(inst):
    rle = {
        "size": inst["rle"]["size"],
        "counts": inst["rle"]["counts"].encode("ascii"),
    }
    return mask_utils.decode(rle).astype(bool)


def _topk(inst, k):
    if k is None or len(inst) <= k:
        return inst
    return sorted(inst, key=lambda x: -(x["score"] or 0))[:k]


def _hand_area_filter(per_frame, dropped_counter):
    """Drop hand instances > HAND_AREA_OUTLIER x the track's median area.

    Track = obj_id when present in the raw output, else the instance's
    score rank within its frame (rank 0/1 = pseudo-track).
    """
    tracks = {}
    for fn, insts in per_frame.items():
        hands = [i for i in insts if i["cls"] == "hand"]
        for rank, inst in enumerate(hands):
            key = inst["obj_id"] if inst.get("obj_id") is not None else ("rank", rank)
            tracks.setdefault(key, []).append((fn, inst))
    for key, members in tracks.items():
        if len(members) < 4:
            continue
        areas = np.array([i["mask"].sum() for _, i in members])
        med = np.median(areas)
        for (fn, inst), a in zip(members, areas):
            if a > HAND_AREA_OUTLIER * med:
                per_frame[fn].remove(inst)
                dropped_counter[0] += 1


def process_chunk(chunk_dir: Path, frame_names: list[str], frames_dir: Path) -> dict:
    """Filter + paint one chunk dir of raw_<concept>.json files.

    Returns the stats dict that is also written to stats.json.
    """
    raws = {}
    cls_of = {}
    gpu_calls = {}
    for p in sorted(chunk_dir.glob("raw_*.json")):
        d = json.loads(p.read_text())
        concept = d["concept"]
        raws[concept] = d["frames"]
        cls_of[concept] = d.get("cls", concept)
        if d.get("call_id") and d.get("method_gpu_s") is not None:
            gpu_calls[d["call_id"]] = d["method_gpu_s"]

    # decode masks once; instances carry their decoded mask under "mask"
    per_frame = {}  # concept -> {frame_name: [inst]}
    for concept, frames in raws.items():
        per_frame[concept] = {}
        for fn in frame_names:
            insts = []
            for inst in frames.get(fn, []):
                i = dict(inst)
                i["concept"] = concept
                i["cls"] = cls_of[concept]
                i["mask"] = _decode(inst)
                insts.append(i)
            per_frame[concept][fn] = _topk(insts, KEEP_TOP_K.get(cls_of[concept]))

    # face union per frame
    face_union = {}
    for concept, cls in cls_of.items():
        if cls != "face":
            continue
        for fn in frame_names:
            insts = per_frame[concept][fn]
            face_union[fn] = (
                np.logical_or.reduce([i["mask"] for i in insts])
                if insts
                else None
            )

    # eye/mouth must be >=80% inside face
    face_removed = 0
    for concept, cls in cls_of.items():
        if cls not in ("eye", "mouth"):
            continue
        for fn in frame_names:
            fu = face_union.get(fn)
            kept = []
            for inst in per_frame[concept][fn]:
                inside = (
                    fu is not None
                    and (inst["mask"] & fu).sum() / max(1, inst["mask"].sum())
                    >= FACE_INSIDE_MIN
                )
                if inside:
                    kept.append(inst)
                else:
                    face_removed += 1
            per_frame[concept][fn] = kept

    # hand-area outlier filter across the whole chunk
    hand_dropped = [0]
    all_frames = {}
    for fn in frame_names:
        all_frames[fn] = [
            inst
            for concept in per_frame
            for inst in per_frame[concept][fn]
        ]
    _hand_area_filter(all_frames, hand_dropped)

    # paint + filtered instance records (all_frames[fn] is the final set)
    chunk_json = {"frames": {}}
    det_counts = {
        c: {"frames_with_det": 0, "instances": 0} for c in raws
    }
    for fn in frame_names:
        sem = None
        recs = []
        by_cls = sorted(all_frames[fn], key=lambda i: PAINT_ORDER.index(i["cls"]))
        for inst in by_cls:
            if sem is None:
                h, w = inst["mask"].shape
                sem = np.zeros((h, w), np.uint8)
            sem[inst["mask"]] = CLASS_ID[inst["cls"]]
            recs.append(
                {
                    "concept": inst["concept"],
                    "class_id": CLASS_ID[inst["cls"]],
                    "obj_id": inst.get("obj_id"),
                    "box_xywh": inst.get("box_xywh"),
                    "score": inst.get("score"),
                    "rle": inst["rle"],
                }
            )
            det_counts[inst["concept"]]["instances"] += 1
        for concept in det_counts:
            if any(i["concept"] == concept for i in by_cls):
                det_counts[concept]["frames_with_det"] += 1
        if sem is None:
            ref = Image.open(frames_dir / fn)
            sem = np.zeros((ref.height, ref.width), np.uint8)
        Image.fromarray(sem, mode="L").save(
            chunk_dir / (Path(fn).stem + ".png")
        )
        chunk_json["frames"][fn] = {"instances": recs}

    (chunk_dir / "chunk.json").write_text(json.dumps(chunk_json))
    stats = {
        "chunk": chunk_dir.name,
        "frames": len(frame_names),
        "det_counts": det_counts,
        "face_filter_removed": face_removed,
        "hand_filter_dropped": hand_dropped[0],
        "gpu_s": round(sum(gpu_calls.values()), 2),
        "gpu_calls": len(gpu_calls),
    }
    (chunk_dir / "stats.json").write_text(json.dumps(stats))
    return stats
