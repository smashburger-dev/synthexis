"""Eye openness from landmark geometry + pixel cues, fitted leave-one-session-out."""
import json, statistics as st
from collections import deque
from paths import REC as R, WORK as SP
S = {"S1": "2026-10-27-S1", "S2": "2026-10-29-S2", "S3": "2026-11-01-S3", "S4": "2026-11-01-S4"}
TARGET = {"C_neutral": (1, 1), "C_eyes_closed": (0, 0), "C_wink_L": (0, 1), "C_wink_R": (1, 0)}


def load(s, prefix="mc"):
    fr = [json.loads(l) for l in open(f"{SP}/{prefix}_{s}.jsonl")]
    segs = [x for x in json.load(open(R + S[s] + "/segments.json")) if x["block"] == "C" and not x["bad"]]
    return [f for f in fr if "face_raw" in f], segs


def seg_of(segs, t):
    for sg in segs:
        m0, m1 = sg.get("margin_s", [0, 0])
        if sg["start_s_cam0"] + m0 <= t <= sg["end_s_cam0"] - m1:
            return sg["id"]
    return None


class Ref:
    """Running quantile over the last `win` seconds."""
    def __init__(self, win, q):
        self.win, self.q, self.buf = win, q, deque()

    def push(self, t, v):
        self.buf.append((t, v))
        while self.buf and t - self.buf[0][0] > self.win:
            self.buf.popleft()
        vals = sorted(x for _, x in self.buf)
        return vals[int((len(vals) - 1) * self.q)]


def features(fr, win=20.0, rq=0.9, pq=0.1):
    """Per frame: (t, [feats L], [feats R]) with adaptive references."""
    rref = Ref(win, rq)
    pref = [Ref(win, pq), Ref(win, pq)]
    out = []
    for f in fr:
        r = f["face_raw"][:2]
        px = f["eye_px"]
        q = [px[2], px[5]]
        rr = rref.push(f["t"], max(r))
        feats = []
        for e in (0, 1):
            qb = pref[e].push(f["t"], q[e])
            feats.append([r[e] / max(rr, 1e-3), q[e] - qb, r[1 - e] / max(rr, 1e-3), q[1 - e] - pref[1 - e].buf[-1][1] if False else 0.0])
        # pixel difference between the eyes, after their own baselines
        dq = (q[0] - pref[0].push(f["t"], q[0])) - (q[1] - pref[1].push(f["t"], q[1]))
        feats[0][3] = dq
        feats[1][3] = -dq
        out.append((f["t"], feats))
    return out
