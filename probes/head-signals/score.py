"""Score the face and head signals of a bench dump against the rules in
docs/session-2026-11-04-head.md.

  python3 probes/head-signals/score.py <frames.jsonl> <segments.json> [--json]

<frames.jsonl> comes from the plugin's offline bench (SYNTHEXIS_BENCH,
plugin/synthexis/src/engine/bench.rs), backend hybrid, block C with a lead-in.
Only the inner part of each segment counts (start + margin[0] .. end - margin[1]).
Standard library only.
"""

import argparse
import json
import statistics as st

# FaceFeatures::values order (signal::FACE_IDS)
IDX = {"mouth": 0, "smile": 1, "eye_L": 2, "eye_R": 3, "yaw": 6, "pitch": 7, "roll": 8}


def load(path):
    return [json.loads(line) for line in open(path) if line.strip()]


def window(frames, seg):
    m0, m1 = seg.get("margin_s", [0, 0])
    a, b = seg["start_s_cam0"] + m0, seg["end_s_cam0"] - m1
    return [f for f in frames if a <= f["t"] <= b]


def q(v, p):
    v = sorted(v)
    return v[int(p * (len(v) - 1))] if v else float("nan")


def score(frames, segs):
    by = {s["id"]: window(frames, s) for s in segs if s["block"] == "C" and not s["bad"]}
    vals = lambda seg, k: [f["face"][IDX[k]] for f in by.get(seg, []) if "face" in f]
    med = lambda seg, k: q(vals(seg, k), 0.5)
    span = lambda seg, k: q(vals(seg, k), 0.95) - q(vals(seg, k), 0.05)
    n_all = sum(len(w) for w in by.values())
    seen = sum(1 for w in by.values() for f in w if "face" in f) / max(n_all, 1)
    m = {"face_seen": seen}
    for k in IDX:
        v = vals("C_neutral", k)
        m[f"{k}.neutral_jitter"] = st.median(abs(a - b) for a, b in zip(v, v[1:])) if len(v) > 2 else float("nan")
        m[f"{k}.neutral_med"] = med("C_neutral", k)
        m[f"{k}.neutral_span"] = span("C_neutral", k)
    m["mouth.wide_med"] = med("C_mouth_wide", "mouth")
    m["mouth.sweep_span"] = span("C_mouth_sweep", "mouth")
    for seg in ("C_smile", "C_yaw_sweep", "C_pitch_sweep", "C_roll_sweep"):
        m[f"mouth.p90_{seg[2:]}"] = q(vals(seg, "mouth"), 0.9)
    for e in ("eye_L", "eye_R"):
        for seg in ("C_eyes_closed", "C_wink_L", "C_wink_R"):
            m[f"{e}.{seg[2:]}_med"] = med(seg, e)
        m[f"{e}.blink_p5"] = q(vals("C_blink", e), 0.05)
    for k in ("yaw", "pitch", "roll"):
        seg = f"C_{k}_sweep"
        m[f"{k}.sweep_span"] = span(seg, k)
        m[f"{k}.sweep_p5"] = q(vals(seg, k), 0.05)
        m[f"{k}.sweep_p95"] = q(vals(seg, k), 0.95)
        for other in ("yaw", "pitch", "roll"):
            if other != k:
                m[f"{k}.span_in_{other}_sweep"] = span(f"C_{other}_sweep", k)
    m["mouth.turned_p90"] = q(vals("C_turned_mouth", "mouth"), 0.9)
    m["mouth.turned_p10"] = q(vals("C_turned_mouth", "mouth"), 0.1)
    return m


def rules(m):
    """(signal, rule, pass) per rule of the session doc."""
    out = [("all", "face seen >= 0.95", m["face_seen"] >= 0.95)]
    for k in IDX:
        out.append((k, "neutral jitter <= 0.01", m[f"{k}.neutral_jitter"] <= 0.01))
    out += [
        ("mouth", "neutral med <= 0.10", m["mouth.neutral_med"] <= 0.10),
        ("mouth", "wide med >= 0.80", m["mouth.wide_med"] >= 0.80),
        ("mouth", "sweep span >= 0.70", m["mouth.sweep_span"] >= 0.70),
    ]
    for seg in ("smile", "yaw_sweep", "pitch_sweep", "roll_sweep"):
        out.append(("mouth", f"p90 in {seg} <= 0.30", m[f"mouth.p90_{seg}"] <= 0.30))
    for e in ("eye_L", "eye_R"):
        out.append((e, "neutral med >= 0.60", m[f"{e}.neutral_med"] >= 0.60))
        out.append((e, "eyes_closed med <= 0.20", m[f"{e}.eyes_closed_med"] <= 0.20))
        out.append((e, "blink p5 <= 0.20", m[f"{e}.blink_p5"] <= 0.20))
    out += [
        ("eye_L", "wink_L: L <= 0.30", m["eye_L.wink_L_med"] <= 0.30),
        ("eye_L", "wink_L: R >= 0.50", m["eye_R.wink_L_med"] >= 0.50),
        ("eye_R", "wink_R: R <= 0.30", m["eye_R.wink_R_med"] <= 0.30),
        ("eye_R", "wink_R: L >= 0.50", m["eye_L.wink_R_med"] >= 0.50),
    ]
    for k, need, lo, hi in (("yaw", 0.6, 0.35, 0.65), ("pitch", 0.5, 0.4, 0.6), ("roll", 0.6, 0.35, 0.65)):
        out.append((k, f"sweep span >= {need}", m[f"{k}.sweep_span"] >= need))
        out.append((k, f"sweep p5 < {lo} and p95 > {hi}", m[f"{k}.sweep_p5"] < lo and m[f"{k}.sweep_p95"] > hi))
        out.append((k, "neutral span <= 0.10", m[f"{k}.neutral_span"] <= 0.10))
        out.append((k, "neutral med in 0.35..0.65", 0.35 <= m[f"{k}.neutral_med"] <= 0.65))
        for other in ("yaw", "pitch", "roll"):
            if other != k:
                out.append((k, f"span in {other}_sweep <= 0.40", m[f"{k}.span_in_{other}_sweep"] <= 0.40))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("frames")
    ap.add_argument("segments")
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args()
    m = score(load(a.frames), json.load(open(a.segments)))
    r = rules(m)
    if a.json:
        print(json.dumps({"metrics": m, "rules": r}))
        return
    for k, v in m.items():
        print(f"{k}: {v:.3f}")
    fails = [x for x in r if not x[2]]
    print(f"rules: {len(r) - len(fails)}/{len(r)} pass")
    for s, rule, _ in fails:
        print(f"  FAIL {s}: {rule}")


if __name__ == "__main__":
    main()
