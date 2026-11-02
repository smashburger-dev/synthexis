# /// script
# requires-python = ">=3.11"
# ///
"""Route-level score of a parser run on the real-sentence set.

  uv run tools/mapping/predict.py --run <run> --file <texts> --jsonl <preds>
  uv run probes/real-eval/score.py --preds data/mapping/eval/preds-<run>.jsonl

Gold: data/mapping/eval/gold-routes.json for Noa's originals; each derived
row gets its original's routes with its delta applied (side, finger,
source, invert and route swaps; parameter, value, track and language
changes leave the routes as they are). A preset is one route of its own
(P:<preset>, with the side). A route is one target with its
sources (side attached; a single finger, shown or extended, as F:<finger>;
other finger states as 5-bit masks), gates, INV, the target kind, mode
step/ramp and the number of values. Which parameter a route binds is the
resolver's job and is not scored here. Exact match of the route multiset.
"""

import argparse
import collections
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
EVAL = ROOT / "data" / "mapping" / "eval"
FINGERS = ["thumb", "index", "middle", "ring", "pinky"]
MASKS = {"fist": "00000", "open": "11111", "point": "01000",
         "thumbs_up": "10000", "peace": "01100", "three": "01110",
         "four": "01111", "rock": "01001", "shaka": "10001",
         "l_shape": "11000", "pinky_up": "00001",
         "thumb_index_middle": "11100"}


def source(head: str, attrs: dict) -> str:
    v = head[2:]
    if v == "hand.shape" and attrs.get("shape"):
        sh = attrs["shape"]
        m = sh[1:] if sh.startswith("m") else MASKS.get(sh, sh)
        out = f"F:{FINGERS[m.index('1')]}" if m.count("1") == 1 \
            else f"S:hand.shape|shape:m{m}"
    elif re.fullmatch(r"hand\.finger\.\w+", v):
        out = f"F:{v.split('.')[-1]}"
    else:
        out = head
    if attrs.get("side") and attrs["side"] != "either":
        out += f"|side:{attrs['side']}"
    return out


def target(t: str, mode: str | None) -> str:
    kind = t.split(":")[1]
    if kind == "toggle" or (kind == "param" and mode in ("toggle",
                                                         "momentary")):
        return "T:switch"
    if kind == "midi.prop":
        return t
    return f"T:{kind}"


def canon(frame: list) -> collections.Counter:
    """Frame tokens -> multiset of routes, one per target."""
    routes, cur, rest_side, preset = [], None, None, None
    for t in frame:
        if t == "<route>":
            cur = []
        elif t == "</route>":
            routes.append(cur)
            cur = None
        elif cur is not None:
            cur.append(t)
        elif t.startswith("side:"):
            rest_side = t[5:]          # finger set: one side for all routes
        elif t.startswith("preset:"):
            preset = t[7:]
    out = collections.Counter()
    for toks in routes:
        srcs, gates, tgts, inv, vals = [], [], [], False, 0
        i = 0
        while i < len(toks):
            t = toks[i]
            attrs = {}
            j = i + 1
            while j < len(toks) and toks[j].split(":")[0] in (
                    "side", "shape", "object", "mode"):
                k, v = toks[j].split(":", 1)
                attrs[k] = v
                j += 1
            if t.startswith("S:"):
                if t == "S:hand.fist":
                    t, inv = "S:hand.openness", not inv
                if rest_side and "side" not in attrs:
                    attrs["side"] = rest_side
                srcs.append(source(t, attrs))
            elif t.startswith("G:"):
                g = "G:hand.openness" if t == "G:hand.fist" else t
                if t == "G:hand.shape" and attrs.get("shape"):
                    g = source("S:hand.shape", {"shape": attrs["shape"]})
                gates.append(g)
            elif t.startswith("T:"):
                tgts.append((target(t, attrs.get("mode")), attrs.get("mode")))
            elif t == "INV":
                inv = not inv
            elif t == "VALUE":
                vals += 1
            i = j
        for tk, mode in tgts or [("T:none", None)]:
            r = srcs + gates + [tk] + (["INV"] if inv else []) \
                + ["VALUE"] * vals
            if mode in ("step", "ramp") and tk == "T:param":
                r.append(f"mode:{mode}")
            out[tuple(sorted(r))] += 1
    if preset:                          # a preset counts as one route of its own
        out[(f"P:{preset}" + (f"|side:{rest_side}" if rest_side else ""),)] += 1
    return out


def derive(routes: list, delta: list) -> list:
    rs = [list(r) for r in routes]

    def sub(f):
        return [[f(t) for t in r] for r in rs]

    for d in delta:
        k, _, v = d.partition(":")
        a, _, b = v.partition(">")
        if k == "side" and a == "any":
            rs = sub(lambda t: t + f"|side:{b}" if t[:2] in ("S:", "F:")
                     and "|side:" not in t else t)
        elif k == "side":
            rs = sub(lambda t: t.replace(f"|side:{a}", f"|side:{b}"))
        elif k in ("finger", "fingers") and "+" in a:
            ma = "".join("1" if f in a.split("+") else "0" for f in FINGERS)
            mb = "".join("1" if f in b.split("+") else "0" for f in FINGERS)
            rs = sub(lambda t: t.replace(f"shape:m{ma}", f"shape:m{mb}"))
        elif k in ("finger", "fingers"):
            m = dict(zip(a.split(","), b.split(",")))
            rs = sub(lambda t: re.sub(      # F:thumb, S:hand.finger.thumb.tilt
                r"^(F:|S:hand\.finger\.)(\w+)", lambda x: x.group(1) + m.get(
                    x.group(2), x.group(2)), t))
        elif k == "source" and "," in a:     # pinch+roll,spread>hand.y,openness
            rs = [[t for t in r if t != "G:hand.pinch"] for r in rs]
            rs = sub(lambda t: t.replace("S:hand.roll", "S:hand.y")
                     .replace("S:hand.spread", "S:hand.openness"))
        elif k == "source":
            rs = sub(lambda t: t.replace(f"S:{a}", f"S:{b}"))
        elif k == "invert":
            rs = [[t for t in r if t != "INV"] if "INV" in r else r + ["INV"]
                  for r in rs]
        elif d == "routes:swap-sides":
            rs = sub(lambda t: t.replace("|side:left", "|side:X")
                     .replace("|side:right", "|side:left")
                     .replace("|side:X", "|side:right"))
        elif d == "routes:swap-sources":
            s0 = [t for t in rs[0] if t[:2] in ("S:", "F:")]
            s1 = [t for t in rs[1] if t[:2] in ("S:", "F:")]
            rs = [[t for t in rs[0] if t not in s0] + s1,
                  [t for t in rs[1] if t not in s1] + s0]
    return rs


def golds(row: dict, gold: dict) -> list:
    g = gold[str(row["src"])]
    alts = g["alt"] if isinstance(g, dict) else [g]
    delta = [] if row["kind"] in ("orig", "style") else row["delta"]
    return [collections.Counter(tuple(sorted(r)) for r in derive(a, delta))
            for a in alts]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--preds", required=True)
    a = ap.parse_args()
    rows = [json.loads(line) for line in open(EVAL / "expanded.jsonl")]
    preds = [json.loads(line) for line in open(a.preds)]
    gold = json.loads((EVAL / "gold-routes.json").read_text())
    res = collections.defaultdict(lambda: [0, 0])
    for r, p in zip(rows, preds):
        assert r["text"] == p["text"], r["id"]
        ok = canon(p["frame"]) in golds(r, gold)
        for key in ((r["split"], "all"), (r["split"], r["kind"]),
                    (r["split"], r["lang"])):
            res[key][0] += ok
            res[key][1] += 1
    for sp in ("calibration", "eval"):
        print(sp + ": " + ", ".join(
            f"{k} {res[sp, k][0]}/{res[sp, k][1]}"
            for k in ("all", "orig", "translate", "swap", "style", "en", "de")
            if res[sp, k][1]))


if __name__ == "__main__":
    main()
