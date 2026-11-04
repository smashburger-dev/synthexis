"""Replays the finger calibration (plugin calib.rs) offline: old rule
(pooled medians of every folded and every shown frame) against the new one
(pose medians, misread folded poses left out). Mirror of calib.rs; keep
both in step.

  python3 calib_replay.py noa              # Noa's run of 11-04 from the log
  python3 calib_replay.py <dump dir> S1,S2,S3

Replay on a recording: block A holds five of the seven calibration poses
per hand (fist, open, thumb, index, pinky alone; no middle or ring alone).
The calibration takes the first 2 s after the leading margin of the first
segment of each pose and hand; every block A frame of the session with that
hand labelled is then scored as exact mask (value > threshold, no gate).
"""
import json
import sys
from pathlib import Path

DATA = Path("/Users/no8/Desktop/life/Lifemaxxing/synthexis/data/recordings")
SESS = {"S1": "2026-10-27-S1", "S2": "2026-10-29-S2", "S3": "2026-11-01-S3", "S4": "2026-11-01-S4"}
POSES = ["00000", "11111", "10000", "01000", "00100", "00010", "00001"]
MIN_GAP, MIN_SAMPLES, BAND = 0.12, 8, 0.1


def bit(mask, f):
    return mask[f] == "1"


def median(v):
    v = sorted(x for x in v if x == x)
    return v[len(v) // 2] if len(v) >= MIN_SAMPLES else None


def old_rule(frames):
    """frames: {pose: [[5 values], ...]} for one hand -> thresholds."""
    thr = []
    for f in range(5):
        shown = [v[f] for p, vs in frames.items() for v in vs if bit(p, f)]
        folded = [v[f] for p, vs in frames.items() for v in vs if not bit(p, f)]
        s, d = median(shown), median(folded)
        thr.append(min(max((s + d) / 2, 0.15), 0.85) if s is not None and d is not None and s - d >= MIN_GAP else 0.5)
    return thr


def new_rule(frames):
    med = {p: [median([v[f] for v in vs]) for f in range(5)] for p, vs in frames.items()}
    thr = []
    for f in range(5):
        folded = [med[p][f] for p in POSES if p in med and not bit(p, f) and med[p][f] is not None]
        if not folded:
            thr.append(0.5)
            continue
        low = min(folded)
        own = med.get(POSES[2 + f], [None] * 5)[f]
        own = own if own is not None and own - low >= MIN_GAP else None
        opn = med.get("11111", [None] * 5)[f]
        opn = opn if opn is not None and opn - low >= MIN_GAP else None
        shown = min(own, opn) if own is not None and opn is not None else (own if own is not None else opn)
        if shown is None:
            thr.append(0.5)
            continue
        mid = max((shown + low) / 2, 0.5)
        high = max([low] + [x for x in folded if x < mid])
        t = min(max((shown + high) / 2, 0.15), 0.85)
        thr.append(min(max(t, high + BAND), shown - BAND) if shown - high >= 2 * BAND else 0.5)
    return thr


def read(v, thr):
    return "".join("1" if x > t else "0" for x, t in zip(v, thr))


def noa():
    l = {"10000": ([1.00, 0.00, 0.00, 0.00, 0.00], [0.37, 0.00, 0.00, 0.00, 0.00]),
         "01000": ([0.00, 1.00, 0.00, 0.00, 0.00], [0.03, 0.94, 0.03, 0.00, 0.00]),
         "00100": ([0.00, 0.98, 0.05, 0.00, 0.00], [0.00, 0.93, 0.10, 0.00, 0.00]),
         "00010": ([0.00, 0.96, 0.00, 0.00, 0.18], [0.20, 0.88, 0.14, 0.12, 0.32]),
         "00001": ([0.00, 0.14, 0.00, 0.00, 0.59], [0.21, 0.57, 0.12, 0.04, 0.29])}
    r = {"10000": ([1.00, 0.01, 0.00, 0.00, 0.00], [0.62, 0.00, 0.00, 0.00, 0.00]),
         "01000": ([0.00, 1.00, 0.00, 0.00, 0.00], [0.50, 1.00, 0.00, 0.00, 0.00]),
         "00100": ([0.00, 0.96, 0.01, 0.00, 0.00], [0.00, 1.00, 0.03, 0.03, 0.00]),
         "00010": ([0.00, 0.70, 0.00, 0.00, 0.02], [0.28, 0.99, 0.10, 0.13, 0.27]),
         "00001": ([0.02, 0.00, 0.00, 0.00, 1.00], [0.44, 0.12, 0.03, 0.03, 0.98])}
    for name, hand in (("L", l), ("R", r)):
        frames = {p: [[(a + b) / 2 for a, b in zip(s, g)]] * 30 for p, (s, g) in hand.items()}
        for rule in (old_rule, new_rule):
            thr = rule(frames)
            reads = {p: read(vs[0], thr) for p, vs in frames.items()}
            print(name, rule.__name__, [round(t, 2) for t in thr], " ".join(f"{p}->{x}" for p, x in reads.items()))


def replay(dump, sessions):
    tot = {"flat": [0, 0], "old": [0, 0], "new": [0, 0]}
    for s in sessions:
        lines = [json.loads(x) for x in open(Path(dump) / f"ba_{s}.jsonl") if x.strip()]
        segs = [x for x in json.load(open(DATA / SESS[s] / "segments.json")) if x["block"] == "A" and not x["bad"]]
        for side in "LR":
            def vals(a, b):
                out = []
                for ln in lines:
                    if a <= ln["t"] <= b and f"hand_{side}" in ln:
                        geo = ln[f"hand_{side}"][2:7]
                        ext = ln.get(f"hand_{side}_ext", geo)
                        out.append([(e + g) / 2 for e, g in zip(ext, geo)])
                return out
            frames = {}
            for seg in segs:
                lab = seg["labels"].get(f"hand_{side}", "absent")
                if lab in POSES and lab not in frames:
                    a = seg["start_s_cam0"] + seg["margin_s"][0]
                    frames[lab] = vals(a, a + 2.0)
            thr = {"flat": [0.5] * 5, "old": old_rule(frames), "new": new_rule(frames)}
            print(s, side, "poses", sorted(frames), {k: [round(x, 2) for x in v] for k, v in thr.items() if k != "flat"})
            for seg in segs:
                lab = seg["labels"].get(f"hand_{side}", "absent")
                if lab == "absent":
                    continue
                a, b = seg["start_s_cam0"] + seg["margin_s"][0], seg["end_s_cam0"] - seg["margin_s"][1]
                for v in vals(a, b):
                    for k, t in thr.items():
                        tot[k][0] += read(v, t) == lab
                        tot[k][1] += 1
    for k, (ok, n) in tot.items():
        print(f"{k}: exact {ok / max(n, 1):.3f} of {n}")


if __name__ == "__main__":
    if sys.argv[1] == "noa":
        noa()
    else:
        replay(sys.argv[1], sys.argv[2].split(","))
