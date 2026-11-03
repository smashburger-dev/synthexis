"""Offline lab for the single-hand side rule on raw bench dumps."""
import json, math
from paths import REC, WORK as SP
SESS = {"S1": "2026-10-27-S1", "S2": "2026-10-29-S2", "S3": "2026-11-01-S3", "S4": "2026-11-01-S4"}
FW, FH = 1280, 720
KX, KY = FW / 384, FH / 216


def load(s, prefix="ba"):
    fr = [json.loads(l) for l in open(f"{SP}/{prefix}_{s}.jsonl")]
    segs = [x for x in json.load(open(f"{REC}/{SESS[s]}/segments.json")) if x["block"] == "A" and not x["bad"]]
    return fr, segs


def label_at(segs, t):
    """(seg, labels) of the segment whose inner window holds t, else None."""
    for sg in segs:
        m0, m1 = sg.get("margin_s", [0, 0])
        if sg["start_s_cam0"] + m0 <= t <= sg["end_s_cam0"] - m1:
            return sg
    return None


def cand_feats(c, face):
    """Per-candidate cues; positions in frame px."""
    cx, cy = c["c"][0] * KX, c["c"][1] * KY
    d = {"a": c["a"], "cx": cx, "cy": cy}
    d["bd_dx"] = (c["bd"][0] - c["c"][0]) * KX if c["bd"][2] > 0 else 0.0
    d["bd_dy"] = (c["bd"][1] - c["c"][1]) * KY if c["bd"][2] > 0 else 0.0
    d["bd_n"] = c["bd"][2]
    d["bl_dx"] = (c["bl"][0] - c["c"][0]) * KX if c["bl"][1] > 0 else 0.0
    d["bl_n"] = c["bl"][1]
    kp = c["kp"]
    if len(kp) == 42:
        P = [(kp[2 * i], kp[2 * i + 1]) for i in range(21)]
        w, i5, m9, p17, t4 = P[0], P[5], P[9], P[17], P[4]
        d["ax_dx"] = m9[0] - w[0]
        d["ax_dy"] = m9[1] - w[1]
        a = (i5[0] - w[0], i5[1] - w[1]); b = (p17[0] - w[0], p17[1] - w[1])
        d["chir"] = a[0] * b[1] - a[1] * b[0]
        d["thumb_dx"] = t4[0] - p17[0]
        d["wrist_dx"] = w[0] - cx
        d["kp"] = P
    d["fface"] = face[0] * KX if face else None
    return d


def frames(s, prefix="ba"):
    fr, segs = load(s, prefix)
    out = []
    for f in fr:
        sg = label_at(segs, f["t"])
        raw = f.get("raw", {"cands": []})
        face = raw.get("face")
        out.append({"t": f["t"], "seg": sg, "cands": [cand_feats(c, face) for c in raw["cands"]], "face": face})
    return out, segs


def score(res, segs):
    """res: list of (t, {side_index: cand}); side 0 = L, 1 = R.
    Returns labelled-side rate and wrong-side rate like eval.py."""
    n = pres = wrong = 0
    by_t = {round(t, 3): sd for t, sd in res}
    for sg in segs:
        m0, m1 = sg.get("margin_s", [0, 0]); a, b = sg["start_s_cam0"] + m0, sg["end_s_cam0"] - m1
        for side, si in (("L", 0), ("R", 1)):
            lab = sg["labels"].get(f"hand_{side}")
            if lab in (None, "absent"):
                continue
            other = "R" if side == "L" else "L"
            for t, sd in res:
                if a <= t <= b:
                    n += 1
                    if si in sd:
                        pres += 1
                    if sg["labels"].get(f"hand_{other}") == "absent" and (1 - si) in sd and si not in sd:
                        wrong += 1
    return round(pres / max(n, 1), 3), round(wrong / max(n, 1), 3)
