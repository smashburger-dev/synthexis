"""Score the Pong pointing shot (pong::shot::Point) against segment labels.

  python3 probes/pong-shot/score.py --rec <data/recordings> out/shots_S2.jsonl out/shots_S3.jsonl

Each shots file comes from the bench with SYNTHEXIS_BENCH_SHOTS (see
plugin/synthexis/src/engine/bench.rs); its session (S2, S3, S4) is the
suffix of the file name. The rule is in RESULT.md. Standard library only.
"""

import argparse
import json
import re
from pathlib import Path

POINT = {"01000", "11000"}
LEAD_S = 1.0
SRCS = ("gate", "plain")


def windows(segs):
    """Each segment owns the time from the end of the one before (at most
    LEAD_S before its start) to its own end."""
    out, prev_end = [], None
    for s in sorted(segs, key=lambda s: s["start_s_cam0"]):
        a = s["start_s_cam0"] - LEAD_S
        if prev_end is not None:
            a = max(a, prev_end)
        out.append((a, s["end_s_cam0"], s))
        prev_end = s["end_s_cam0"]
    return out


def find(wins, t):
    return next((w for w in wins if w[0] < t <= w[1]), None)


def label(seg, side):
    return seg["labels"].get(f"hand_{side}")


def score(session, segs, events, src):
    wins = windows(segs)
    shots = [e for e in events if e["src"] == src and e["ev"] == "shot"]
    noshots = [e for e in events if e["src"] == src and e["ev"] == "noshot"]
    false, unscored, hits = [], [], {}
    for e in shots:
        w = find(wins, e["t"])
        if w is None:
            unscored.append(e)
            continue
        seg = w[2]
        if seg["bad"]:
            continue
        if label(seg, e["side"]) in POINT:
            hits.setdefault((seg["id"], e["side"]), []).append(e)
        else:
            false.append((seg, e))
    minutes = sum(b - a for a, b, s in wins if not s["bad"]) / 60.0

    def targets(block, want):
        rows = []
        for a, b, s in wins:
            if s["bad"] or s["block"] != block:
                continue
            for side in "LR":
                if label(s, side) == want:
                    got = hits.get((s["id"], side), [])
                    miss = [n for n in noshots if n["side"] == side and a < n["t"] <= b]
                    rows.append((session, s, side, got, miss))
        return rows

    return {
        "true": targets("A", "01000"),
        "gun": targets("A", "11000"),
        "count": targets("B", "01000"),
        "false": [(session, s, e) for s, e in false],
        "unscored": [(session, e) for e in unscored],
        "minutes": minutes,
    }


def load(path):
    return [json.loads(line) for line in open(path) if line.strip()]


def session_of(path):
    m = re.search(r"(S\d+)", Path(path).stem)
    return m.group(1) if m else Path(path).stem


def seg_file(rec, session):
    hits = sorted(Path(rec).glob(f"*-{session}/segments.json"))
    if not hits:
        raise SystemExit(f"no recording for {session} under {rec}")
    return hits[0]


def rate(rows):
    n = sum(1 for r in rows if r[3])
    return n, len(rows)


def report(results, src):
    merged = {k: [] for k in ("true", "gun", "count", "false", "unscored")}
    minutes = 0.0
    for r in results:
        for k in merged:
            merged[k] += r[k]
        minutes += r["minutes"]
    print(f"### {src}\n")
    n, of = rate(merged["true"])
    ok = of and n / of >= 0.8
    print(f"(a) index-alone segments with a shot: {n}/{of} = {n / max(of, 1):.0%} -> {'PASS' if ok else 'FAIL'} (>= 80 %)")
    for session, s, side, got, miss in merged["true"]:
        first = f"first at {got[0]['t'] - s['start_s_cam0']:+.2f} s" if got else "none"
        why = ", ".join(f"{m['t']:.2f}s {m['why']} proj={(m['feat'] or {}).get('proj')}" for m in miss)
        print(f"  {session} {s['id']} {side}: {len(got)} shot(s), {first}" + (f"; turns without a shot: {why}" if why else ""))
    for name, key in (("thumb+index (11000), block A", "gun"), ("index alone in B counts (2 s)", "count")):
        n2, of2 = rate(merged[key])
        print(f"  info, {name}: {n2}/{of2} with a shot")
    nf = len(merged["false"])
    per = nf / minutes if minutes else float("nan")
    print(f"(b) false shots: {nf} in {minutes:.1f} min = {per:.2f}/min -> {'PASS' if per <= 1.0 else 'FAIL'} (<= 1/min)")
    by_block = {}
    for session, s, e in merged["false"]:
        by_block[s["block"]] = by_block.get(s["block"], 0) + 1
    print("  per block: " + ", ".join(f"{b} {c}" for b, c in sorted(by_block.items())))
    for session, s, e in merged["false"]:
        lab = label(s, e["side"]) or next(iter(s["labels"].values()))
        print(f"  {session} {s['id']} {e['side']} t={e['t']:.2f} ({e['t'] - s['start_s_cam0']:+.2f} s in) label={lab} at={e['at']} dir={e['dir']}")
    for session, e in merged["unscored"]:
        print(f"  unscored (between segments): {session} {e['side']} t={e['t']:.2f}")
    print()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("shots", nargs="+")
    ap.add_argument("--rec", required=True)
    args = ap.parse_args()
    for src in SRCS:
        results = []
        for path in args.shots:
            session = session_of(path)
            segs = json.load(open(seg_file(args.rec, session)))
            results.append(score(session, segs, load(path), src))
        report(results, src)


if __name__ == "__main__":
    main()
