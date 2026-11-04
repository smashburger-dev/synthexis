"""Head cue lab: simple head cues per block C segment, against the
`vision` backend's own yaw / pitch / roll (Apple's angles, eval reference
only, never fitted on). The cues (`head_px`: nose tip along and across the
eye line, contour asymmetry, lips across, tip over lips, eye-line angle)
came from the lab build of 2026-11-04, not from the shipped bench; this
script is where the vision backend's yaw and pitch signs were checked
(vision.rs).

  python3 probes/head-signals/headlab.py <cue.jsonl> <vis.jsonl> <segments.json>

Prints per cue: neutral median, p5 / p95 in each head sweep, and the
Pearson correlation with Vision's angle of the same axis in its sweep
(frames matched by time).
"""

import json
import statistics as st
import sys

CUES = ["tip_x", "tip_y", "asym", "lip_y", "ratio", "roll_rad"]
AXIS = {"yaw": 6, "pitch": 7, "roll": 8}


def load(p):
    return [json.loads(l) for l in open(p) if l.strip()]


def window(frames, seg):
    m0, m1 = seg.get("margin_s", [0, 0])
    a, b = seg["start_s_cam0"] + m0, seg["end_s_cam0"] - m1
    return [f for f in frames if a <= f["t"] <= b]


def q(v, p):
    v = sorted(v)
    return v[int(p * (len(v) - 1))] if v else float("nan")


def corr(a, b):
    return st.correlation(a, b) if len(a) > 10 and st.pstdev(a) > 0 and st.pstdev(b) > 0 else float("nan")


def main():
    cue, vis, segs = load(sys.argv[1]), load(sys.argv[2]), json.load(open(sys.argv[3]))
    vt = {round(f["t"], 2): f for f in vis if "face" in f}
    by = {s["id"]: window(cue, s) for s in segs if s["block"] == "C" and not s["bad"]}
    for k, name in enumerate(CUES):
        row = [f"{name:9s}"]
        neu = [f["head_px"][k] for f in by["C_neutral"] if "head_px" in f and any(f["head_px"])]
        row.append(f"neutral {q(neu, 0.5):+.3f} (p5..p95 {q(neu, 0.05):+.3f}..{q(neu, 0.95):+.3f})")
        for ax in AXIS:
            v = [f["head_px"][k] for f in by[f"C_{ax}_sweep"] if "head_px" in f and any(f["head_px"])]
            row.append(f"{ax} {q(v, 0.05):+.3f}..{q(v, 0.95):+.3f}")
        print("  ".join(row))
    print("correlation with Vision's angle in its sweep (cue -> axis):")
    for ax, i in AXIS.items():
        fr = [f for f in by[f"C_{ax}_sweep"] if "head_px" in f and any(f["head_px"]) and round(f["t"], 2) in vt]
        ref = [vt[round(f["t"], 2)]["face"][i] for f in fr]
        cur = [f["face"][i] for f in fr]
        cs = "  ".join(f"{n} {corr([f['head_px'][k] for f in fr], ref):+.2f}" for k, n in enumerate(CUES))
        print(f"  {ax:5s} n={len(fr)}  current {corr(cur, ref):+.2f}  {cs}")
    print("Vision's own spread (p5..p95) per sweep, the physical crosstalk:")
    vby = {s["id"]: window(vis, s) for s in segs if s["block"] == "C" and not s["bad"]}
    for ax in AXIS:
        parts = []
        for ax2, i in AXIS.items():
            v = [f["face"][i] for f in vby[f"C_{ax}_sweep"] if "face" in f]
            parts.append(f"{ax2} {q(v, 0.95) - q(v, 0.05):.2f}")
        neu = [f["face"][AXIS[ax]] for f in vby["C_neutral"] if "face" in f]
        print(f"  {ax}_sweep: " + ", ".join(parts) + f"   (neutral {ax} med {q(neu, 0.5):.2f})")


if __name__ == "__main__":
    main()
