"""Helpers over bench dumps: segments, windows, finger cues, ramps, One Euro.

Dump names are relative to paths.WORK (data/lab by default).
"""
import json, math, os
from paths import REC, S, WORK as SP
FINGERS = [[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12], [13, 14, 15, 16], [17, 18, 19, 20]]

def sub(a, b): return (a[0] - b[0], a[1] - b[1])
def ang(a, b): return abs(math.atan2(a[0] * b[1] - a[1] * b[0], a[0] * b[0] + a[1] * b[1]))

def bends(kp):
    """Per finger: summed joint bend (rad); thumb also tip reach / palm."""
    w = kp[0]
    palm = max(math.dist(w, kp[9]), 1.2 * math.dist(kp[5], kp[17]), 1.0)
    out = []
    for ch in FINGERS:
        segs = [sub(kp[ch[0]], w), sub(kp[ch[1]], kp[ch[0]]), sub(kp[ch[2]], kp[ch[1]]), sub(kp[ch[3]], kp[ch[2]])]
        out.append(sum(ang(segs[j], segs[j + 1]) for j in range(3)))
    reach = math.dist(kp[4], kp[5]) / palm
    return out, reach

def segments(s):
    return {x["id"]: x for x in json.load(open(REC + S[s] + "/segments.json")) if not x["bad"]}

def frames(path):
    return [json.loads(l) for l in open(os.path.join(SP, path))]

def window(F, sg, margin=True):
    m0, m1 = sg.get("margin_s", [0, 0]) if margin else (0, 0)
    a, b = sg["start_s_cam0"] + m0, sg["end_s_cam0"] - m1
    return [f for f in F if a <= f["t"] <= b]

def hand_raw(f):
    c = [c for c in f.get("raw", {}).get("cands", []) if c["a"] >= 60 and c.get("kp")]
    if not c: return None
    c = max(c, key=lambda c: c["a"])
    k = c["kp"]; kp = [(k[2 * i], k[2 * i + 1]) for i in range(21)]
    return bends(kp)

def triangle(t, period, start_closed=True):
    ph = (t % period) / period
    v = 2 * ph if ph < 0.5 else 2 - 2 * ph
    return v if start_closed else 1 - v

def corr(a, b):
    n = len(a); ma = sum(a) / n; mb = sum(b) / n
    va = sum((x - ma) ** 2 for x in a); vb = sum((y - mb) ** 2 for y in b)
    return sum((x - ma) * (y - mb) for x, y in zip(a, b)) / math.sqrt(va * vb + 1e-12)

def best_lag(ts, vals, t0, period=4.0, lags=None):
    """Lag (s) of the movement behind a triangle starting closed at t0."""
    lags = lags or [i / 30 for i in range(-45, 46)]
    return max(((corr(vals, [triangle(t - t0 - L, period) for t in ts]), L) for L in lags))

def finger_cues(kp):
    """Per finger (thumb..pinky): bend (rad), tip reach from its base / palm,
    tip reach from the wrist / palm."""
    w = kp[0]
    palm = max(math.dist(w, kp[9]), 1.2 * math.dist(kp[5], kp[17]), 1.0)
    out = []
    for n, ch in enumerate(FINGERS):
        segs = [sub(kp[ch[0]], w), sub(kp[ch[1]], kp[ch[0]]), sub(kp[ch[2]], kp[ch[1]]), sub(kp[ch[3]], kp[ch[2]])]
        bend = sum(ang(segs[j], segs[j + 1]) for j in range(3))
        base = kp[5] if n == 0 else kp[ch[0]]
        out.append((bend, math.dist(kp[ch[3]], base) / palm, math.dist(kp[ch[3]], w) / palm))
    return out

def labelled_hands(s, prefix="ba"):
    """(bits, cues per finger) for one-label frames of block A, largest cand."""
    F = frames(f"{prefix}_{s}.jsonl"); G = segments(s)
    out = []
    for sg in G.values():
        if sg["block"] != "A": continue
        labs = [sg["labels"].get(f"hand_{k}") for k in "LR"]
        labs = [l for l in labs if l not in (None, "absent")]
        if len(labs) != 1: continue
        for f in window(F, sg):
            c = [c for c in f.get("raw", {}).get("cands", []) if c["a"] >= 60 and c.get("kp")]
            if not c: continue
            k = max(c, key=lambda c: c["a"])["kp"]
            out.append((labs[0], finger_cues([(k[2 * i], k[2 * i + 1]) for i in range(21)]), sg["id"]))
    return out

class OneEuro:
    def __init__(self, mc=1.0, beta=1.0, dc=1.0):
        self.mc, self.beta, self.dc, self.x, self.dx = mc, beta, dc, None, 0.0
    @staticmethod
    def a(c, dt):
        tau = 1 / (2 * math.pi * max(c, 1e-3)); return 1 / (1 + tau / max(dt, 1e-6))
    def step(self, x, dt):
        if self.x is None:
            self.x = x; return x
        dx = (x - self.x) / dt
        self.dx = self.a(self.dc, dt) * dx + (1 - self.a(self.dc, dt)) * self.dx
        c = self.mc + self.beta * abs(self.dx)
        self.x = self.a(c, dt) * x + (1 - self.a(c, dt)) * self.x
        return self.x

def lin(v, a, b): return min(1, max(0, (v - a) / (b - a)))
ANC = {1: ((3.79, 0.54), (0.93, 1.99)), 2: ((3.64, 0.52), (0.72, 2.04)), 3: ((3.56, 0.51), (0.59, 1.87)), 4: ((3.79, 0.70), (0.54, 1.60))}
def open_new(c):
    v = [0.5 * (1 - lin(c[0][0], 0.2, 1.4)) + 0.5 * lin(c[0][1], 0.35, 0.9)]
    v += [0.5 * lin(c[fi][0], *ANC[fi][0]) + 0.5 * lin(c[fi][2], *ANC[fi][1]) for fi in range(1, 5)]
    return sum(v) / 5
def openness_series(F, a, b):
    out = []
    for f in F:
        if not (a <= f["t"] <= b): continue
        c = [c for c in f.get("raw", {}).get("cands", []) if c["a"] >= 60 and c.get("kp")]
        if c:
            k = max(c, key=lambda c: c["a"])["kp"]; out.append((f["t"], open_new(finger_cues([(k[2 * i], k[2 * i + 1]) for i in range(21)]))))
    return out
