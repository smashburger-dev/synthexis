# /// script
# requires-python = ">=3.11"
# ///
"""Noa's label conventions of 2026-11-01 (docs/parser-v9-plan.md), applied
to every split of a dataset so training and dev labels agree.

  uv run tools/mapping/conventions.py --src ds-v9p --out ds-v9c

1. pinch: S:hand.pinch on a route that has another hand movement as source
   becomes G:hand.pinch (a hold, never pinch strength); its side token goes
   with it.
2. finger state: two or more single-finger sources on one route of a
   sentence that says they are shown together (at the same time, together,
   both, gleichzeitig, zusammen, beide) become one S:hand.shape with the
   5-bit mask of those fingers.
3. one encoding per movement (parser v11): S:hand.fist becomes
   S:hand.openness and S:eye.closed becomes S:eye.open, with the route's
   INV flipped, as probes/real-eval/score.py canon reads them. Gates stay.
Routes whose targets move in different directions are already separate in
the labels (one INV per route); stage 6 adds such sentences. Writes
<out>/conventions.json with the number of changed rows per split.
"""

import argparse
import json
import re
import shutil
from pathlib import Path

DATA = Path(__file__).resolve().parents[2] / "data" / "mapping" / "train"
MOVES = {"S:hand.x", "S:hand.y", "S:hand.roll", "S:hand.depth",
         "S:hand.pitch", "S:hand.speed", "S:hands.distance"}
FINGERS = ["thumb", "index", "middle", "ring", "pinky"]
TOGETHER = re.compile(r"at the same time|together|\bboth\b|gleichzeitig|"
                      r"zusammen|\bbeide", re.I)


def routes(frame: list) -> list:
    """-> [(start, end)] index ranges of route bodies."""
    out, start = [], None
    for i, t in enumerate(frame):
        if t == "<route>":
            start = i + 1
        elif t == "</route>" and start is not None:
            out.append((start, i))
            start = None
    return out


def pinch_gate(frame: list) -> bool:
    changed = False
    for a, b in routes(frame):
        body = frame[a:b]
        if "S:hand.pinch" in body and MOVES & {t for t in body
                                                if isinstance(t, str)}:
            i = a + body.index("S:hand.pinch")
            frame[i] = "G:hand.pinch"
            changed = True
    return changed


FOLD = {"S:hand.fist": "S:hand.openness", "S:eye.closed": "S:eye.open"}


def fold(frame: list) -> list | None:
    """Closing the fist is the hand's openness inverted."""
    if not any(isinstance(t, str) and t in FOLD for t in frame):
        return None
    out, last = [], 0
    for a, b in routes(frame):
        body = frame[a:b]
        flips = sum(isinstance(t, str) and t in FOLD for t in body)
        if not flips:
            continue
        body = [FOLD.get(t, t) if isinstance(t, str) else t for t in body]
        if flips % 2:
            body = ([t for t in body if t != "INV"] if "INV" in body
                    else body + ["INV"])
        out += frame[last:a] + body
        last = b
    return out + frame[last:]


def finger_state(frame: list, text: str) -> list | None:
    if not TOGETHER.search(text):
        return None
    out, changed, last = [], False, 0
    for a, b in routes(frame):
        body = frame[a:b]
        idx = [k for k, t in enumerate(body) if isinstance(t, str) and
               re.fullmatch(r"S:hand\.finger\.(thumb|index|middle|ring|pinky)", t)]
        if len(idx) < 2 or any(isinstance(body[k + 1], str) and
                               body[k + 1].startswith("side:")
                               for k in idx if k + 1 < len(body)):
            continue
        mask = "".join("1" if any(body[k] == f"S:hand.finger.{f}" for k in idx)
                       else "0" for f in FINGERS)
        new = [t for k, t in enumerate(body) if k not in idx[1:]]
        new[idx[0]:idx[0] + 1] = ["S:hand.shape", f"shape:m{mask}"]
        out += frame[last:a] + new
        last = b
        changed = True
    return out + frame[last:] if changed else None


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    src, dst = DATA / a.src, DATA / a.out
    if dst.exists():
        shutil.rmtree(dst)
    dst.mkdir(parents=True)
    stats = {}
    for f in sorted(src.glob("*.jsonl")):
        n = {"rows": 0, "pinch": 0, "finger_state": 0, "fold": 0}
        with open(dst / f.name, "w") as w:
            for line in open(f):
                r = json.loads(line)
                n["rows"] += 1
                n["pinch"] += pinch_gate(r["frame"])
                fs = finger_state(r["frame"], r["text"])
                if fs is not None:
                    r["frame"] = fs
                    n["finger_state"] += 1
                fd = fold(r["frame"])
                if fd is not None:
                    r["frame"] = fd
                    n["fold"] += 1
                w.write(json.dumps(r, ensure_ascii=False) + "\n")
        stats[f.stem] = n
    for f in src.glob("*.json"):
        shutil.copy(f, dst / f.name)
    (dst / "conventions.json").write_text(json.dumps(stats, indent=1))
    print(json.dumps(stats))


if __name__ == "__main__":
    main()
