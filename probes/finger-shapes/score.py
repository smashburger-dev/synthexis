"""Scores the finger-shape candidates of RESULT.md on bench dumps of block A.

  python3 score.py <dump dir> dev     # S1-S3 dev segments, all candidates
  python3 score.py <dump dir> S4 C2   # S4, one candidate

Dumps: ba_S1.jsonl .. ba_S4.jsonl from the release bench (students backend),
with "hand_<side>" = HandFeatures::values() (geometric fingers at 2..6) and
"hand_<side>_ext" = the student's finger probabilities.
"""
import json
import sys
from pathlib import Path

DATA = Path("/Users/no8/Desktop/life/Lifemaxxing/synthexis/data")
SESS = {"S1": "2026-10-27-S1", "S2": "2026-10-29-S2", "S3": "2026-11-01-S3", "S4": "2026-11-01-S4"}


def candidates(geo, ext):
    return {
        "C1": [e > 0.5 for e in ext],
        "C2": [g > 0.5 for g in geo],
        "C3": [ext[0] > 0.5] + [g > 0.5 for g in geo[1:]],
        "C4": [(e + g) / 2 > 0.5 for e, g in zip(ext, geo)],
    }


def segments(s, dev_only):
    segs = json.load(open(DATA / "recordings" / SESS[s] / "segments.json"))
    segs = [x for x in segs if x["block"] == "A" and not x["bad"]]
    if dev_only:
        m = json.load(open(DATA / "packed" / "manifest.json"))
        ids = set(m["sessions"][SESS[s]].get("dev_segments", []))
        segs = [x for x in segs if x["id"] in ids]
    return segs


def score(dump_dir, sessions, dev_only, names):
    tot = {n: {"frames": 0, "exact": 0, "finger": [0] * 5, "changes": 0, "seconds": 0.0} for n in names}
    by_mask = {}
    for s in sessions:
        lines = [json.loads(l) for l in open(Path(dump_dir) / f"ba_{s}.jsonl") if l.strip()]
        for seg in segments(s, dev_only):
            a = seg["start_s_cam0"] + seg["margin_s"][0]
            b = seg["end_s_cam0"] - seg["margin_s"][1]
            for side in ("L", "R"):
                label = seg["labels"].get(f"hand_{side}", "absent")
                if label == "absent":
                    continue
                want = [c == "1" for c in label]
                last = {n: None for n in names}
                for ln in lines:
                    if not (a <= ln["t"] <= b) or f"hand_{side}" not in ln:
                        continue
                    geo = ln[f"hand_{side}"][2:7]
                    ext = ln.get(f"hand_{side}_ext", geo)
                    cand = candidates(geo, ext)
                    for n in names:
                        m = cand[n]
                        t = tot[n]
                        t["frames"] += 1
                        ok = m == want
                        t["exact"] += ok
                        by_mask.setdefault((n, label), [0, 0])
                        by_mask[(n, label)][0] += ok
                        by_mask[(n, label)][1] += 1
                        for f in range(5):
                            t["finger"][f] += m[f] == want[f]
                        if last[n] is not None and last[n] != m:
                            t["changes"] += 1
                        last[n] = m
                for n in names:
                    tot[n]["seconds"] += max(0.0, b - a)
    for n in names:
        t = tot[n]
        f = max(t["frames"], 1)
        print(f"{n}: frames {t['frames']} exact {t['exact'] / f:.3f} per finger "
              f"{[round(x / f, 3) for x in t['finger']]} changes/s {t['changes'] / max(t['seconds'], 1e-9):.2f}")
    worst = {}
    for (n, label), (ok, k) in by_mask.items():
        worst.setdefault(n, []).append((ok / k, label, k))
    for n in names:
        rows = sorted(worst.get(n, []))[:5]
        print(f"  {n} weakest masks: " + ", ".join(f"{lab} {acc:.2f} (n={k})" for acc, lab, k in rows))


if __name__ == "__main__":
    d, which = sys.argv[1], sys.argv[2]
    if which == "dev":
        score(d, ["S1", "S2", "S3"], True, ["C1", "C2", "C3", "C4"])
    else:
        score(d, [which], False, sys.argv[3:] or ["C1", "C2", "C3", "C4"])
