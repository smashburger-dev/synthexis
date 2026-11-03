"""Check the plugin's perception signals against a recording's segment labels.

  python3 probes/plugin-perception/eval.py <frames.jsonl> <segments.json> [--name students]

<frames.jsonl> comes from the plugin's offline bench (SYNTHEXIS_BENCH, see
plugin/synthexis/src/engine/bench.rs). Block A gives the finger state per
side, block C the face actions. Only the inner part of each segment counts
(start + margin[0] .. end - margin[1]). Standard library only.
"""

import argparse
import json
import statistics as st

HAND = {"openness": 0, "finger0": 2, "finger_count": 19}
FACE = {"mouth": 0, "smile": 1, "eye_L": 2, "eye_R": 3, "yaw": 6, "pitch": 7, "roll": 8}


def load(path):
    return [json.loads(line) for line in open(path) if line.strip()]


def window(frames, seg):
    m0, m1 = seg.get("margin_s", [0, 0])
    a, b = seg["start_s_cam0"] + m0, seg["end_s_cam0"] - m1
    return [f for f in frames if a <= f["t"] <= b]


def mean(xs):
    return st.mean(xs) if xs else float("nan")


def hand_block(frames, segs):
    present, wrong_side, finger_ok, finger_n, count_err, pairs = 0, 0, 0, 0, [], []
    n_frames = 0
    for seg in segs:
        if seg["block"] != "A" or seg["bad"]:
            continue
        w = window(frames, seg)
        for side in "LR":
            label = seg["labels"].get(f"hand_{side}")
            other = "R" if side == "L" else "L"
            if label in (None, "absent"):
                continue
            bits = [int(c) for c in label]
            vals = []
            for f in w:
                n_frames += 1
                h = f.get(f"hand_{side}")
                if seg["labels"].get(f"hand_{other}") == "absent" and f.get(f"hand_{other}") and not h:
                    wrong_side += 1
                if not h:
                    continue
                present += 1
                ext = h[HAND["finger0"]:HAND["finger0"] + 5]
                finger_ok += sum((e > 0.5) == bool(b) for e, b in zip(ext, bits))
                finger_n += 5
                count_err.append(abs(h[HAND["finger_count"]] * 5 - sum(bits)))
                vals.append(h[HAND["openness"]])
            if vals:
                pairs.append((sum(bits) / 5, mean(vals)))
    corr = st.correlation(*zip(*pairs)) if len(pairs) > 2 else float("nan")
    return {
        "hand_frames": n_frames,
        "present_rate": present / max(n_frames, 1),
        "wrong_side_rate": wrong_side / max(n_frames, 1),
        "finger_accuracy": finger_ok / max(finger_n, 1),
        "finger_count_mae": mean(count_err),
        "openness_vs_extended_corr": corr,
    }


def face_block(frames, segs):
    by = {s["id"]: window(frames, s) for s in segs if s["block"] == "C" and not s["bad"]}

    def avg(seg, key):
        return mean([f["face"][FACE[key]] for f in by.get(seg, []) if "face" in f])

    def spread(seg, key):
        v = sorted(f["face"][FACE[key]] for f in by.get(seg, []) if "face" in f)
        return v[int(0.95 * (len(v) - 1))] - v[int(0.05 * (len(v) - 1))] if len(v) > 10 else float("nan")

    seen = [("face" in f) for w in by.values() for f in w]
    return {
        "face_present_rate": sum(seen) / max(len(seen), 1),
        "mouth_open neutral / wide": (avg("C_neutral", "mouth"), avg("C_mouth_wide", "mouth")),
        "eye_L neutral / closed / wink_L / wink_R": tuple(
            avg(s, "eye_L") for s in ("C_neutral", "C_eyes_closed", "C_wink_L", "C_wink_R")),
        "eye_R neutral / closed / wink_L / wink_R": tuple(
            avg(s, "eye_R") for s in ("C_neutral", "C_eyes_closed", "C_wink_L", "C_wink_R")),
        "smile neutral / smile": (avg("C_neutral", "smile"), avg("C_smile", "smile")),
        "yaw p5-p95 in yaw_sweep / neutral": (spread("C_yaw_sweep", "yaw"), spread("C_neutral", "yaw")),
        "pitch p5-p95 in pitch_sweep / neutral": (spread("C_pitch_sweep", "pitch"), spread("C_neutral", "pitch")),
        "roll p5-p95 in roll_sweep / neutral": (spread("C_roll_sweep", "roll"), spread("C_neutral", "roll")),
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("frames")
    ap.add_argument("segments")
    ap.add_argument("--name", default="")
    a = ap.parse_args()
    frames, segs = load(a.frames), json.load(open(a.segments))
    out = {"name": a.name, **hand_block(frames, segs), **face_block(frames, segs)}
    for k, v in out.items():
        if isinstance(v, float):
            v = round(v, 3)
        elif isinstance(v, tuple):
            v = tuple(round(x, 3) for x in v)
        print(f"{k}: {v}")


if __name__ == "__main__":
    main()
