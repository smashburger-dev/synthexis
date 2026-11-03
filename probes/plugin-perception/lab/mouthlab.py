"""Mouth open cue on S1-S3 block C (hybrid dumps mc_{s}.jsonl in paths.WORK).

Compares the inner-lip minor/major ratio (face_raw[2], the old cue) with
the inner-lip gap across the eye line per eye distance (eye_px[9] in the
bench output, mouth_px[3] in geom.rs, the cue behind MOUTH_GAP): anchors
per session, and with the given range, the share of the slow sweep between
0.2 and 0.8 and above 0.9, and readings in a smile and with the mouth shut.

  python3 mouthlab.py [lo hi]      (default: geom.rs MOUTH_GAP 0.06 0.6)
"""
import sys
from ramps import frames, segments, window

lo, hi = (float(sys.argv[1]), float(sys.argv[2])) if len(sys.argv) > 2 else (0.06, 0.6)
q = lambda x, p: sorted(x)[int(p * (len(x) - 1))] if x else float("nan")
lin = lambda v: min(1.0, max(0.0, (v - lo) / (hi - lo)))
SHUT = ["C_neutral", "C_blink", "C_wink_L", "C_wink_R", "C_eyes_closed", "C_yaw_sweep", "C_pitch_sweep", "C_roll_sweep"]
for s in ("S1", "S2", "S3"):
    F = [f for f in frames(f"mc_{s}.jsonl") if "face" in f and len(f["eye_px"]) >= 10]
    G = segments(s)
    gap = lambda sg: [f["eye_px"][9] for f in window(F, G[sg])]
    ratio = lambda sg: [f["face_raw"][2] for f in window(F, G[sg])]
    print(f"{s} ratio: neutral {q(ratio('C_neutral'), .5):.3f} smile {q(ratio('C_smile'), .5):.3f} wide {q(ratio('C_mouth_wide'), .5):.3f}"
          f" | gap: neutral {q(gap('C_neutral'), .5):.3f} smile {q(gap('C_smile'), .5):.3f} wide {q(gap('C_mouth_wide'), .5):.3f} sweep p97 {q(gap('C_mouth_sweep'), .97):.3f}")
    sw = [lin(x) for x in gap("C_mouth_sweep")]
    shut = {sg: q([lin(x) for x in gap(sg)], .9) for sg in SHUT if sg in G}
    print(f"   lin({lo}, {hi}): sweep 0.2-0.8 {sum(.2 <= x <= .8 for x in sw) / len(sw):.2f} >0.9 {sum(x > .9 for x in sw) / len(sw):.2f}"
          f" smile {q([lin(x) for x in gap('C_smile')], .5):.2f} wide {q([lin(x) for x in gap('C_mouth_wide')], .5):.2f}"
          f" | shut-mouth p90: " + " ".join(f"{k[2:]} {v:.2f}" for k, v in shut.items()))
