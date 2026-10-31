# /// script
# requires-python = ">=3.11"
# ///
"""Project dataset labels onto what the sentence says (ds-v3 -> ds-v4p).

  uv run tools/mapping/project_labels.py [--src ds-v3] [--out ds-v4p]

The parser outputs only what the text expresses; the runtime binds the rest
(camera objects, plugin type, user defaults). Rules, from the error analysis
in probes/parser-errors/RESULT.md:
  object   object:X -> object:any when no glossary phrase of X is in the text
  mode     mode:momentary only with a hold cue, mode:toggle only with a
           flip cue; otherwise dropped (the user's default decides)
  invert   INV dropped when no invert cue is in the text
  gate     G:hand.fist -> G:hand.shape shape:fist (one encoding)
  side     side:either dropped (same as no side)
  pitch    T:param:osc.pitch and T:param:fx.pitch -> T:param:pitch (the
           resolver splits them by plugin type)
Every file of the source dataset is written with projected frames; counts
per rule go to <out>/projection.json. project() is shared with gates.py.
"""

import argparse
import collections
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "data" / "mapping"
INV = re.compile(
    r"invert|inverse|revers|backward|opposite|flip|upside|the less|less |"
    r"lower|down|decreas|reduc|clos|shut|quieter|darker|drier|fewer|smaller|"
    r"shorter|softer|umgekehrt|invers|rückwärts|gegen|weniger|leiser|kleiner|"
    r"schließ|schliess|runter|dunkler|kürzer|tiefer|niedriger|zu ", re.I)
MOM = re.compile(r"hold|while|as long as|momentar|only when|press|keep|"
                 r"solange|halt|während|gedrückt|nur wenn", re.I)
TOG = re.compile(r"toggle|each time|every time|on and off|on or off|again|"
                 r"and un|an und aus|ein und aus|jedes ?mal|umschalt|nochmal|"
                 r"erneut|und wieder|und un", re.I)
PITCH = {"T:param:osc.pitch": "T:param:pitch",
         "T:param:fx.pitch": "T:param:pitch"}


def objects() -> dict:
    gl = collections.defaultdict(set)
    for line in open(DATA / "glossary" / "gloss-v1" / "glossary.jsonl"):
        g = json.loads(line)
        if g["c"].startswith("object:"):
            gl[g["c"][7:]].add(g["p"].casefold())
    return gl


def project(frame: list, text: str, obj: dict, stats=None) -> list:
    """Frame items (tokens, or pointer spans as lists) -> projected frame."""
    low, out = text.casefold(), []

    def hit(rule: str) -> None:
        if stats is not None:
            stats[rule] += 1

    for t in frame:
        if not isinstance(t, str):
            out.append(t)
        elif t == "side:either":
            hit("side")
        elif t == "G:hand.fist":
            out += ["G:hand.shape", "shape:fist"]
            hit("gate")
        elif t.startswith("object:") and t != "object:any" and not any(
                p in low for p in obj.get(t[7:], set()) | {t[7:]}):
            out.append("object:any")
            hit("object")
        elif (t == "mode:momentary" and not MOM.search(text)) or \
                (t == "mode:toggle" and not TOG.search(text)):
            hit("mode")
        elif t == "INV" and not INV.search(text):
            hit("invert")
        elif t in PITCH:
            out.append(PITCH[t])
            hit("pitch")
        else:
            out.append(t)
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", default="ds-v3")
    ap.add_argument("--out", default="ds-v4p")
    a = ap.parse_args()
    src, out = DATA / "train" / a.src, DATA / "train" / a.out
    out.mkdir(parents=True, exist_ok=True)
    obj = objects()
    report = {}
    for f in sorted(src.glob("*.jsonl")):
        stats, changed, n = collections.Counter(), 0, 0
        with open(out / f.name, "w") as w:
            for line in open(f):
                r = json.loads(line)
                new = project(r["frame"], r["text"], obj, stats)
                changed += new != r["frame"]
                n += 1
                r["frame"] = new
                w.write(json.dumps(r, ensure_ascii=False) + "\n")
        report[f.stem] = {"rows": n, "changed": changed, **stats}
        print(f.stem, report[f.stem])
    (out / "projection.json").write_text(json.dumps(
        {"src": a.src, "files": report}, indent=1))


if __name__ == "__main__":
    main()
