# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow"]
# ///
"""Ties hand keypoints to SAM hand instances and writes stats, per source.

Sources: mediapipe (tools/keypoints/mediapipe_hands.py) and sam3dbody
(tools/keypoints/sam3d_body.py); <src> below is the source name.

Reads  data/keypoints/<session>/cam0/<src>.jsonl
       data/frames/<session>/cam0/frames_index.json
       data/labels/<session>/cam0/chunk_*/chunk.json   (SAM 3.1, image mode)
       data/recordings/<session>/segments.json
Writes data/keypoints/<session>/cam0/keypoints_<src>.jsonl, one line per frame:
         {file, t, sam, hands: [{chirality, score, joints, box, sam_match, train_ok}],
          sam_unmatched: [sam_id, ...]}
       data/keypoints/<session>/cam0/stats_<src>.json
       data/keypoints/<session>/contact_sheet_<src>.jpg  (24 frames, skeletons drawn)
The file names carry the source on purpose: a packer must name the keypoint
source it trains on, never pick up whatever lies in data/keypoints.

Coordinates are normalised to the unmirrored frame, origin top-left; joints
are [x, y, z] (MediaPipe's relative depth z; 0 for sam3dbody). `box` is the xywh bound of
the 21 joints. A keypoint hand matches a SAM hand when the IoU of its box
(grown by BOX_PAD of its longer side per side, since joints sit inside the
hand outline) and the SAM box is at least MATCH_IOU; pairs are assigned
one-to-one by best total IoU. SAM instances in image mode have no track id,
so a SAM hand is named "<chunk>/<file>#<n>" with n the index in that frame's
instance list. `sam` is false for frames SAM never labelled (pauses); those
hands get sam_match null like misses.

`train_ok` marks hands fit as teacher targets: matched to a SAM hand and
not overlapping the frame's other keypoint hand (box IoU > OVERLAP_IOU, two
skeletons on one hand). MediaPipe's score does not filter: false hands on
shoulders, neck, forehead and chin score 0.95 to 0.97, but SAM finds no hand
there.

SAM 3D Body always predicts both hands, hidden ones too, so for sam3dbody
only SAM-matched hands count as visible in the checks, recall numbers and
`visible_hands_per_frame`; unmatched hands are drawn grey.

Chirality check: recordings are unmirrored, so anatomical right appears on
the image left. Expected chirality comes from the segment labels: one-hand
segments (one of hand_L / hand_R is "absent", or B_R_* / B_L_* ramps) and
two-hand A segments, where the hand with the smaller wrist x is the right one.

Usage: uv run tools/keypoints/postprocess.py [--source mediapipe|sam3dbody] <session>...
"""

import itertools
import json
import random
import sys
from collections import Counter, defaultdict
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "data"
SOURCE = "mediapipe"  # set from --source
BOTH_HANDS_ALWAYS = {"sam3dbody"}
BOX_PAD = 0.15
MATCH_IOU = 0.3
OVERLAP_IOU = 0.3
FINGERS = ["thumb", "index", "middle", "ring", "little"]
CHAINS = [["wrist", "thumb_cmc", "thumb_mp", "thumb_ip", "thumb_tip"]] + [
    ["wrist", f"{f}_mcp", f"{f}_pip", f"{f}_dip", f"{f}_tip"] for f in FINGERS[1:]
]
COLOR = {"right": (255, 140, 0), "left": (0, 200, 255), "unknown": (255, 0, 255)}


def r4(v):
    return round(v, 4)


def visible(h):
    return h["sam_match"] is not None if SOURCE in BOTH_HANDS_ALWAYS else True


def vis(r):
    return [h for h in r["hands"] if visible(h)]


def joint_box(joints):
    xs = [p[0] for p in joints.values()]
    ys = [p[1] for p in joints.values()]
    return [min(xs), min(ys), max(xs) - min(xs), max(ys) - min(ys)]


def pad(box):
    x, y, w, h = box
    p = BOX_PAD * max(w, h)
    return [x - p, y - p, w + 2 * p, h + 2 * p]


def iou(a, b):
    ix = max(0.0, min(a[0] + a[2], b[0] + b[2]) - max(a[0], b[0]))
    iy = max(0.0, min(a[1] + a[3], b[1] + b[3]) - max(a[1], b[1]))
    inter = ix * iy
    union = a[2] * a[3] + b[2] * b[3] - inter
    return inter / union if union > 0 else 0.0


def assign(kboxes, sboxes):
    """One-to-one pairs (ki, si, iou) with the best total IoU >= MATCH_IOU."""
    best, best_sum = [], 0.0
    n = max(len(kboxes), len(sboxes))
    for perm in itertools.permutations(range(n)):
        pairs = []
        for ki, si in enumerate(perm):
            if ki >= len(kboxes) or si >= len(sboxes):
                continue
            v = iou(pad(kboxes[ki]), sboxes[si])
            if v >= MATCH_IOU:
                pairs.append((ki, si, v))
        total = sum(p[2] for p in pairs)
        if total > best_sum:
            best, best_sum = pairs, total
    return best


def load_sam(session):
    """file -> (chunk name, [(instance index, normalised box, score)]) for hand instances.

    S1 chunks store box_xywh normalised, S2 to S4 in pixels; the RLE size
    gives the frame size for the pixel case.
    """
    sam = {}
    for chunk in sorted((DATA / "labels" / session / "cam0").glob("chunk_*/chunk.json")):
        for file, rec in json.loads(chunk.read_text())["frames"].items():
            hands = []
            for i, inst in enumerate(rec["instances"]):
                if inst["concept"] != "hand":
                    continue
                box = inst["box_xywh"]
                if max(box) > 1.5:
                    h, w = inst["rle"]["size"]
                    box = [box[0] / w, box[1] / h, box[2] / w, box[3] / h]
                hands.append((i, box, inst["score"]))
            sam[file] = (chunk.parent.name, hands)
    return sam


def expected_hands(seg_id, labels):
    """Hands the protocol puts in view: {"right"/"left": True}, or None if unknown."""
    if seg_id and seg_id.startswith(("B_R_", "B_L_")) and not labels:
        return {"right": True} if seg_id[2] == "R" else {"left": True}
    if not labels or "hand_R" not in labels:
        return None
    exp = {}
    if labels["hand_R"] != "absent":
        exp["right"] = True
    if labels.get("hand_L") != "absent":
        exp["left"] = True
    return exp


def process(session):
    out_dir = DATA / "keypoints" / session / "cam0"
    raw = [json.loads(l) for l in (out_dir / f"{SOURCE}.jsonl").read_text().splitlines()]
    index = json.loads((DATA / "frames" / session / "cam0" / "frames_index.json").read_text())["frames"]
    segs = {s["id"]: s for s in json.loads((DATA / "recordings" / session / "segments.json").read_text())}
    sam = load_sam(session)
    assert len(raw) == len(index) and all(r["file"] == f["file"] for r, f in zip(raw, index))

    lines, rows = [], []
    for r, f in zip(raw, index):
        chunk, shands = sam.get(f["file"], (None, []))
        kboxes = [joint_box(h["joints"]) for h in r["hands"]]
        pairs = assign(kboxes, [b for _, b, _ in shands])
        by_k = {ki: (si, v) for ki, si, v in pairs}
        hands = []
        for ki, h in enumerate(r["hands"]):
            m = None
            if ki in by_k:
                si, v = by_k[ki]
                n, _, score = shands[si]
                m = {"sam_id": f"{chunk}/{f['file']}#{n}", "iou": r4(v), "score": r4(score or 0)}
            hands.append({
                "chirality": h["chirality"],
                "score": h["score"],
                "joints": h["joints"],
                "box": [r4(v) for v in kboxes[ki]],
                "sam_match": m,
            })
        overlap = len(kboxes) == 2 and iou(kboxes[0], kboxes[1]) > OVERLAP_IOU
        for h in hands:
            h["train_ok"] = h["sam_match"] is not None and not overlap
        matched_s = {si for _, si, _ in pairs}
        unmatched = [f"{chunk}/{f['file']}#{n}" for si, (n, _, _) in enumerate(shands) if si not in matched_s]
        line = {"file": f["file"], "t": f["t"], "sam": chunk is not None, "hands": hands, "sam_unmatched": unmatched}
        lines.append(json.dumps(line, separators=(",", ":")))
        seg = segs.get(f["segment_id"]) if f["segment_id"] else None
        rows.append({
            "file": f["file"],
            "block": seg["block"] if seg else "gap",
            "seg_id": f["segment_id"],
            "labels": f["labels"],
            "label_ok": f["label_ok"],
            "sam": chunk is not None,
            "n_sam": len(shands),
            "hands": hands,
        })
    (out_dir / f"keypoints_{SOURCE}.jsonl").write_text("\n".join(lines) + "\n")

    stats = make_stats(session, rows)
    (out_dir / f"stats_{SOURCE}.json").write_text(json.dumps(stats, indent=1))
    contact_sheet(session, rows)
    return stats


def summarize(rows):
    hands = [h for r in rows for h in r["hands"]]
    sam_rows = [r for r in rows if r["sam"]]
    k_in_sam = [h for r in sam_rows for h in r["hands"]]
    n_sam = sum(r["n_sam"] for r in sam_rows)
    k_matched = sum(1 for h in k_in_sam if h["sam_match"])
    counts = Counter(len(vis(r)) for r in rows)
    scores = [h["score"] for h in hands if h["score"] is not None]
    return {
        "frames": len(rows),
        "frames_by_hand_count": {str(k): counts.get(k, 0) for k in (0, 1, 2)},
        "frames_with_hand": r4(sum(1 for r in rows if vis(r)) / max(len(rows), 1)),
        "visible_hands_per_frame": r4(sum(len(vis(r)) for r in rows) / max(len(rows), 1)),
        "hands_per_frame": r4(len(hands) / max(len(rows), 1)),
        "train_ok_hands": sum(1 for h in hands if h["train_ok"]),
        "train_ok_share": r4(sum(1 for h in hands if h["train_ok"]) / max(len(hands), 1)),
        "mean_handedness_score": r4(sum(scores) / len(scores)) if scores else None,
        "sam_frames": len(sam_rows),
        "sam_hands": n_sam,
        "keypoint_hands_in_sam_frames": len(k_in_sam),
        # share of keypoint hands (in SAM-labelled frames) tied to a SAM hand
        "match_rate_keypoints": r4(k_matched / max(len(k_in_sam), 1)),
        # share of SAM hands that got a keypoint set
        "match_rate_sam": r4(k_matched / max(n_sam, 1)),
        "mean_match_iou": r4(
            sum(h["sam_match"]["iou"] for h in k_in_sam if h["sam_match"]) / max(k_matched, 1)
        ),
    }


def agree(h, want):
    if h["chirality"] not in ("left", "right"):
        return "unknown"
    return "agree" if h["chirality"] == want else "disagree"


def chirality_check(rows):
    """Agreement of keypoint chirality with the anatomical hand from the protocol."""
    res = defaultdict(Counter)
    for r in rows:
        exp = expected_hands(r["seg_id"], r["labels"])
        if exp is None:
            continue
        hands = vis(r)
        if len(exp) == 1:
            want = next(iter(exp))
            for h in hands:
                res[f"one_hand_{r['block']}"][agree(h, want)] += 1
        elif len(exp) == 2 and len(hands) == 2:
            # unmirrored frame: the hand further left in the image is the right hand
            a, b = sorted(hands, key=lambda h: h["joints"]["wrist"][0])
            res["two_hands_A"][agree(a, "right")] += 1
            res["two_hands_A"][agree(b, "left")] += 1
            res["two_hands_A_frames"]["same_label" if a["chirality"] == b["chirality"] else "distinct"] += 1
    out, tot = {}, Counter()
    for kind, c in sorted(res.items()):
        if kind.endswith("_frames"):
            out[kind] = dict(c)
            continue
        tot.update(c)
        n = c["agree"] + c["disagree"]
        out[kind] = {**c, "agreement": r4(c["agree"] / n) if n else None}
    n = tot["agree"] + tot["disagree"]
    out["all"] = {**tot, "agreement": r4(tot["agree"] / n) if n else None}
    return out


def dist(a, b):
    # x and y are normalised separately; scale y by 720/1280 so distances are isotropic
    return ((a[0] - b[0]) ** 2 + ((a[1] - b[1]) * 0.5625) ** 2) ** 0.5


def extended(j, finger):
    """Heuristic finger state from keypoints: tip further out than the middle joint."""
    if finger == "thumb":
        return dist(j["thumb_tip"], j["little_mcp"]) > dist(j["thumb_mp"], j["little_mcp"]) * 1.2
    return dist(j[f"{finger}_tip"], j["wrist"]) > dist(j[f"{finger}_pip"], j["wrist"])


def finger_check(rows):
    """Per-finger agreement of the keypoint heuristic with the 5-bit A-block labels.

    Only label_ok A frames, hands whose chirality names a labelled hand.
    """
    res = {f: Counter() for f in FINGERS}
    exact = Counter()
    for r in rows:
        if r["block"] != "A" or not r["label_ok"]:
            continue
        for h in vis(r):
            bits = r["labels"].get("hand_R" if h["chirality"] == "right" else "hand_L")
            if not bits or bits == "absent":
                continue
            ok_all = True
            for f, bit in zip(FINGERS, bits):
                ok = extended(h["joints"], f) == (bit == "1")
                res[f]["agree" if ok else "disagree"] += 1
                ok_all &= ok
            exact["agree" if ok_all else "disagree"] += 1
    out = {f: r4(c["agree"] / max(c.total(), 1)) for f, c in res.items()}
    out["all_five"] = r4(exact["agree"] / max(exact.total(), 1))
    out["hands"] = exact.total()
    return out


def recall_by_state(rows):
    """Per 5-bit finger state in label_ok A frames: share of labelled hands found with that chirality."""
    res = defaultdict(Counter)
    for r in rows:
        if r["block"] != "A" or not r["label_ok"]:
            continue
        found = {h["chirality"] for h in vis(r)}
        for key, side in (("hand_R", "right"), ("hand_L", "left")):
            bits = r["labels"].get(key)
            if bits and bits != "absent":
                res[bits]["found" if side in found else "missed"] += 1
    return {b: r4(c["found"] / c.total()) for b, c in sorted(res.items())}


def recall_check(rows):
    """Frames where the detector finds at least as many hands as the protocol puts in view."""
    res = defaultdict(Counter)
    for r in rows:
        exp = expected_hands(r["seg_id"], r["labels"])
        if not exp:
            continue
        res[f"{len(exp)}_expected"]["ok" if len(vis(r)) >= len(exp) else "short"] += 1
    return {k: {**c, "rate": r4(c["ok"] / (c["ok"] + c["short"]))} for k, c in sorted(res.items())}


def make_stats(session, rows):
    by_block = defaultdict(list)
    for r in rows:
        by_block[r["block"]].append(r)
    return {
        "session": session,
        "source": SOURCE,
        "match_rule": {"box_pad": BOX_PAD, "min_iou": MATCH_IOU},
        **summarize(rows),
        "by_block": {b: summarize(rs) for b, rs in sorted(by_block.items())},
        "label_ok_frames": summarize([r for r in rows if r["label_ok"]]),
        "expected_hands_found": recall_check(rows),
        "chirality": chirality_check(rows),
        "finger_state_heuristic": finger_check(rows),
        "recall_by_finger_state_A": recall_by_state(rows),
        "detector": json.loads((DATA / "keypoints" / session / "cam0" / f"{SOURCE}_meta.json").read_text()),
    }


def pick_frames(rows, rng):
    """About 24 frames: A finger states, fast H moves, B ramps, C, two failure cases."""
    def by_seg(cands, k):
        segs = sorted({r["seg_id"] for r in cands})
        rng.shuffle(segs)
        picks = []
        for s in segs[:k]:
            pool = [r for r in cands if r["seg_id"] == s]
            picks.append(pool[len(pool) // 2])
        return picks

    a = [r for r in rows if r["block"] == "A" and r["label_ok"]]
    b = [r for r in rows if r["block"] == "B" and r["labels"] is not None]
    c = [r for r in rows if r["block"] == "C" and r["label_ok"]]
    # fast H frames: largest wrist travel since the previous frame
    speed = []
    for prev, r in zip(rows, rows[1:]):
        if r["block"] == "H" and vis(r) and vis(prev):
            d = max(
                abs(h["joints"]["wrist"][0] - p["joints"]["wrist"][0])
                + abs(h["joints"]["wrist"][1] - p["joints"]["wrist"][1])
                for h in vis(r) for p in vis(prev)
            )
            speed.append((d, r))
    speed.sort(key=lambda x: -x[0])
    h_fast, seen = [], set()
    for _, r in speed:
        if r["seg_id"] not in seen:
            h_fast.append(r)
            seen.add(r["seg_id"])
        if len(h_fast) == 6:
            break
    fails = [
        r for r in rows
        if (exp := expected_hands(r["seg_id"], r["labels"])) and len(vis(r)) < len(exp)
    ]
    rng.shuffle(fails)
    n_a = 10 if not h_fast else 8
    n_b = 4 if b else 0
    picks = by_seg(a, n_a) + h_fast + by_seg(b, n_b) + by_seg(c, 3) + fails[:2]
    rest = [r for r in rows if vis(r) and r not in picks]
    rng.shuffle(rest)
    return (picks + rest)[:24]


def draw_tile(session, r, tw=480, th=270):
    img = Image.open(DATA / "frames" / session / "cam0" / r["file"]).convert("RGB").resize((tw, th))
    d = ImageDraw.Draw(img)
    for h in r["hands"]:
        col = COLOR.get(h["chirality"], COLOR["unknown"]) if visible(h) else (150, 150, 150)
        j = h["joints"]
        for chain in CHAINS:
            d.line([(j[n][0] * tw, j[n][1] * th) for n in chain], fill=col, width=2)
        for x, y, _ in j.values():
            d.ellipse([x * tw - 2, y * th - 2, x * tw + 2, y * th + 2], fill=col)
        x, y, _, _ = h["box"]
        score = f" {h['score']:.2f}" if h["score"] is not None else ""
        tag = f"{h['chirality'][0].upper()}{score}" + (" m" if h["sam_match"] else " -")
        d.text((x * tw, max(0, y * th - 11)), tag, fill=col)
    lab = r["labels"] or {}
    txt = f"{r['file'][6:12]} {r['seg_id'] or '-'} R:{lab.get('hand_R', '')} L:{lab.get('hand_L', '')}"
    d.rectangle([0, 0, tw, 13], fill=(0, 0, 0))
    d.text((3, 1), txt, fill=(255, 255, 255))
    return img


def contact_sheet(session, rows):
    picks = pick_frames(rows, random.Random(0))
    cols, tw, th = 4, 480, 270
    sheet = Image.new("RGB", (cols * tw, ((len(picks) + cols - 1) // cols) * th))
    for i, r in enumerate(picks):
        sheet.paste(draw_tile(session, r, tw, th), ((i % cols) * tw, (i // cols) * th))
    sheet.save(DATA / "keypoints" / session / f"contact_sheet_{SOURCE}.jpg", quality=88)


if __name__ == "__main__":
    argv = sys.argv[1:]
    if argv[:1] == ["--source"]:
        SOURCE, argv = argv[1], argv[2:]
    for s in argv:
        st = process(s)
        print(s, json.dumps({k: st[k] for k in ("frames", "hands_per_frame", "match_rate_keypoints",
                                                 "match_rate_sam")}),
              "chirality", st["chirality"]["all"], "fingers", st["finger_state_heuristic"])
