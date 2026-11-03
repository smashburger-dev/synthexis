"""Pooled block A scores on the packer's dev segments of S1-S3.

  python3 devscore.py 'ba_{s}.jsonl' ['other_{s}.jsonl' ...]

Each pattern names one bench dump per session ({s} = S1, S2, S3), relative
to paths.WORK. present x finger = correct finger bits per labelled hand
frame; it decided the seg model and the pair gate (RESULT.md).
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import eval as E  # noqa: E402
from paths import WORK, dev_segments  # noqa: E402

segs = {s: dev_segments(s) for s in ("S1", "S2", "S3")}
print("dev block A segments", sum(1 for v in segs.values() for x in v if x["block"] == "A" and not x["bad"]))
for pat in sys.argv[1:]:
    n = pres = wrong = fok = fn = cmae = 0.0
    per = []
    for s, sg in segs.items():
        h = E.hand_block(E.load(os.path.join(WORK, pat.format(s=s))), sg)
        k = h["hand_frames"]
        n += k
        pres += h["present_rate"] * k
        wrong += h["wrong_side_rate"] * k
        fok += h["finger_accuracy"] * h["present_rate"] * k
        fn += h["present_rate"] * k
        cmae += h["finger_count_mae"] * h["present_rate"] * k
        per.append(round(h["finger_accuracy"], 3))
    print(f"{pat}: frames {n:.0f} present {pres / n:.3f} wrong side {wrong / n:.4f} finger {fok / fn:.3f} "
          f"count MAE {cmae / fn:.3f} present x finger {fok / n:.3f} | finger per session {per}")
