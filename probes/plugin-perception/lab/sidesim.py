"""Side-rule simulator over raw dumps (see sidelab.py)."""
import math
from sidelab import frames, score, FW, FH

CACHE = {}


def get(s):
    if s not in CACHE:
        CACHE[s] = frames(s)
    return CACHE[s]


def wrist_ev(c):
    P = c.get("kp")
    if not P:
        return 0.0
    size = max(math.dist(P[0], P[9]), 1.2 * math.dist(P[5], P[17]), 1.0)
    return max(-1.0, min(1.0, (P[0][0] - c["cx"]) / size))


def baseline(F, expire=0.5, match=0.25, min_area=60):
    """The current rule (geom.rs HandTracker)."""
    tracks = [None, None]
    res = []
    for f in F:
        t = f["t"]
        hs = [c for c in f["cands"] if c["a"] >= min_area][:2]
        live = [tr if tr and t - tr[1] < expire else None for tr in tracks]
        sides = {}
        if len(hs) == 2:
            a, b = hs
            sides = {1: a, 0: b} if a["cx"] < b["cx"] else {0: a, 1: b}
        elif len(hs) == 1:
            p = hs[0]
            near = [(s, math.dist((p["cx"], p["cy"]), tr[0])) for s, tr in enumerate(live) if tr]
            near = [x for x in near if x[1] < match * FW]
            if near:
                s = min(near, key=lambda x: x[1])[0]
            else:
                ref = p["fface"] if p["fface"] is not None else FW / 2
                s = 1 if p["cx"] < ref else 0
            sides = {s: p}
        for s, p in sides.items():
            tracks[s] = ((p["cx"], p["cy"]), t)
        res.append((t, sides))
    return res


def belief_rule(F, expire=1.0, match=0.25, min_area=60, decay=0.9, w_pos=0.3, two_ratio=0.0,
                two_by_x=True, hyst=0.0, w_two=1.0, ev=wrist_ev):
    """Identity tracks carry a side belief (>0 = person's left) built from
    the wrist offset per frame; one hand takes its track's side, two hands
    split by image x and teach their tracks."""
    tracks = []  # dicts: pos, t, b, side
    res = []
    for f in F:
        t = f["t"]
        hs = [c for c in f["cands"] if c["a"] >= min_area][:2]
        if len(hs) == 2 and hs[1]["a"] < two_ratio * hs[0]["a"]:
            hs = hs[:1]
        tracks = [tr for tr in tracks if t - tr["t"] < expire]
        # greedy association by distance
        pairs = sorted(((math.dist((c["cx"], c["cy"]), tr["pos"]), i, j)
                        for i, c in enumerate(hs) for j, tr in enumerate(tracks)), key=lambda x: x[0])
        used_c, used_t, match_of = set(), set(), {}
        for d, i, j in pairs:
            if d < match * FW and i not in used_c and j not in used_t:
                used_c.add(i); used_t.add(j); match_of[i] = j
        for i, c in enumerate(hs):
            if i not in match_of:
                ref = c["fface"] if c["fface"] is not None else FW / 2
                prior = w_pos * max(-1.0, min(1.0, (c["cx"] - ref) / (0.25 * FW)))
                tracks.append({"pos": (c["cx"], c["cy"]), "t": t, "b": prior, "side": None})
                match_of[i] = len(tracks) - 1
        for i, c in enumerate(hs):
            tr = tracks[match_of[i]]
            tr["b"] = decay * tr["b"] + ev(c)
            tr["pos"] = (c["cx"], c["cy"]); tr["t"] = t
        sides = {}
        if len(hs) == 2 and two_by_x:
            a, b = hs
            ia, ib = match_of[0], match_of[1]
            if a["cx"] < b["cx"]:
                sides = {1: a, 0: b}; tracks[ia]["b"] -= w_two; tracks[ib]["b"] += w_two
            else:
                sides = {0: a, 1: b}; tracks[ia]["b"] += w_two; tracks[ib]["b"] -= w_two
            tracks[ia]["side"] = 1 if a["cx"] < b["cx"] else 0
            tracks[ib]["side"] = 1 - tracks[ia]["side"]
        else:
            for i, c in enumerate(hs):
                tr = tracks[match_of[i]]
                if tr["side"] is None or (tr["side"] == 1 and tr["b"] > hyst) or (tr["side"] == 0 and tr["b"] < -hyst):
                    tr["side"] = 0 if tr["b"] > 0 else 1
                s = tr["side"]
                if s in sides:
                    s = 1 - s
                sides[s] = c
        tracks = tracks[-2:] if len(tracks) > 2 else tracks
        res.append((t, sides))
    return res


def run(rule, sessions=("S1", "S2", "S3"), **kw):
    out = {}
    for s in sessions:
        F, segs = get(s)
        out[s] = score(rule(F, **kw), segs)
    return out
