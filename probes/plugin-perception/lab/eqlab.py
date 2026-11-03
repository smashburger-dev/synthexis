"""Equalised finger curve: anchors from block A, CDF of ramp frames between."""
from ramps import *
from paths import dev_ids
import statistics as st, json, pickle, os, bisect
q = lambda x, p: sorted(x)[int(p * (len(x) - 1))]
dev = {s: dev_ids(s) for s in ("S1", "S2", "S3")}
CACHE = os.path.join(SP, "eq_cache.pkl")
if os.path.exists(CACHE):
    H, RAMP = pickle.load(open(CACHE, "rb"))
else:
    H = [(s, h) for s in ("S1", "S2", "S3") for h in labelled_hands(s)]
    RAMP = {}
    for s in ("S1", "S2", "S3"):
        F = frames(f"bc_{s}.jsonl"); G = segments(s)
        for side in "RL":
            sg = G[f"B_{side}_open"]; ser = []
            for f in F:
                if not (sg["start_s_cam0"] <= f["t"] <= sg["end_s_cam0"]): continue
                c = [c for c in f.get("raw", {}).get("cands", []) if c["a"] >= 60 and c.get("kp")]
                if c:
                    k = max(c, key=lambda c: c["a"])["kp"]; ser.append((f["t"], finger_cues([(k[2 * i], k[2 * i + 1]) for i in range(21)])))
            RAMP[s, side] = ser
    pickle.dump((H, RAMP), open(CACHE, "wb"))
TR = [h for s, h in H if h[2] not in dev[s]]

def make(p_fold, p_ext, eq_w, knots=10):
    """Per finger 1-4: u = mean of bend and reach, each linear between the
    p_fold quantile of folded and the p_ext quantile of extended frames.
    Then a CDF map over ramp frames with 0 < u < 1, blended by eq_w."""
    anc = {}
    for fi in range(1, 5):
        fb = [h[1][fi][0] for h in TR if h[0][fi] == "0"]; eb = [h[1][fi][0] for h in TR if h[0][fi] == "1"]
        fr = [h[1][fi][2] for h in TR if h[0][fi] == "0"]; er = [h[1][fi][2] for h in TR if h[0][fi] == "1"]
        # bend: folded is high; reach: folded is low
        anc[fi] = ((q(eb, 1 - p_ext), q(fb, 1 - p_fold)), (q(fr, p_fold), q(er, p_ext)))
    def u(fi, c):
        (be, bf), (rf, re) = anc[fi]
        return 0.5 * (bf - c[0]) / (bf - be) + 0.5 * (c[2] - rf) / (re - rf)
    xs = sorted(u(fi, c[fi]) for ser in RAMP.values() for _, c in ser for fi in range(1, 5))
    xs = [x for x in xs if 0 < x < 1]
    kx = [0.0] + [xs[int(i / knots * (len(xs) - 1))] for i in range(1, knots)] + [1.0]
    ky = [i / knots for i in range(knots + 1)]
    def cdf(x):
        x = min(1, max(0, x)); j = min(bisect.bisect_right(kx, x), knots) - 1
        return ky[j] + (ky[j + 1] - ky[j]) * (x - kx[j]) / max(kx[j + 1] - kx[j], 1e-9)
    def finger(fi, c):
        x = min(1, max(0, u(fi, c)))
        return (1 - eq_w) * x + eq_w * cdf(x)
    return finger, anc, kx

THUMB = lambda c: 0.5 * (1 - min(1, max(0, (c[0] - 0.2) / 1.2))) + 0.5 * min(1, max(0, (c[1] - 0.35) / 0.55))

def openness(finger, c):
    return (THUMB(c[0]) + sum(finger(fi, c[fi]) for fi in range(1, 5))) / 5

def evaluate(finger):
    # dev finger accuracy (fingers 1-4 by the curve, thumb unchanged)
    ok = n = 0
    for s, h in H:
        if h[2] not in dev[s]: continue
        for fi in range(1, 5):
            ok += (finger(fi, h[1][fi]) > 0.5) == (h[0][fi] == "1"); n += 1
    # open / fist readings and hold noise (block A, all frames)
    op = {"11111": [], "00000": []}
    for s, h in H:
        if h[0] in op: op[h[0]].append(openness(finger, h[1]))
    # transitions on ramps: 20->80% time of the openness
    durs = []; mid = []
    for ser in RAMP.values():
        ts = [t for t, _ in ser]; v = [openness(finger, c) for _, c in ser]
        v = [st.median(v[max(0, i - 2): i + 3]) for i in range(len(v))]
        mid += [.2 <= x <= .8 for x in v]
        i = 0
        while i < len(v) - 1:
            if v[i] < 0.2 or v[i] > 0.8:
                low = v[i] < 0.2; j = i
                while j < len(v) and ((v[j] < 0.2) if low else (v[j] > 0.8)): j += 1
                k = j
                while k < len(v) and 0.2 <= v[k] <= 0.8: k += 1
                if k < len(v) and ((v[k] > 0.8) if low else (v[k] < 0.2)): durs.append(ts[k] - ts[max(j - 1, 0)])
                i = k + 1
            else: i += 1
    return {"dev fingers 1-4": round(ok / n, 3), "open med": round(q(op["11111"], .5), 2), "open p25": round(q(op["11111"], .25), 2), "fist med": round(q(op["00000"], .5), 2), "fist p75": round(q(op["00000"], .75), 2),
            "ramp mid": round(sum(mid) / len(mid), 2), "20-80% ms": round(1000 * st.median(durs)) if durs else None, "n trans": len(durs)}
