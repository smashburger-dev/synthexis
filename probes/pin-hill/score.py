# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "numpy"]
# ///
"""What is left when the user chips everything (Noa, 11-03): every gold SRC
span gets a gesture chip with its gold source, shape and side, every gold
TGT span a parameter chip of its class or a target chip of its kind.
pin.rs (plugin/parser/examples/pin_eval) pins them on the dumped v11
frames; route_edit then measures only what the chips leave open: intent,
direction (INV), ranges, modes, routing. Never reads the real set.

  uv run probes/pin-hill/score.py --rows data/mapping/pin-hill/rows.jsonl [--name run] [--dump out.jsonl]

rows.jsonl: probes/completion-v11/dump.py output.
"""

import argparse
import json
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools" / "mapping"))
sys.path.insert(0, str(ROOT / "probes" / "completion-v11"))
import frame_parser as fp  # noqa: E402
import score as cv11  # noqa: E402


def sources(route):
    """cv11.sources, but a gate (G:) ends the source too: its shape and side
    are the gate's ("blink ... only while my hand is open" gave the blink
    chip shape:open)."""
    out = []
    for i, t in enumerate(route):
        if t.startswith("S:"):
            shape = side = None
            for u in route[i + 1:]:
                if u.startswith(("S:", "G:", "T:")):
                    break
                if u.startswith("shape:"):
                    shape = u[6:]
                if u.startswith("side:"):
                    side = u[5:]
            out.append((cv11.FOLD.get(t[2:], t[2:]), shape, side))
    return out


cv11.sources = sources
assign = cv11.assign

BIN = ROOT / "plugin" / "target" / "release" / "examples" / "pin_eval"
BUCKETS = ("INV", "side:", "S:", "G:", "shape:", "T:", "lo:", "hi:", "amt:", "delta:", "curve:", "mode:",
           "hold:", "smooth:", "quantize:", "VALUE", "NOTE", "CHORD", "TIME", "TRACK", "PLUGIN", "PNAME",
           "FROM", "TO", "REF", "BIP", "object:", "key:", "scale:", "role:", "preset:", "scope:")


def bucket(u) -> str:
    u = u if isinstance(u, str) else " ".join(u)
    return next((b for b in BUCKETS if u.startswith(b)), "other")


def pins(row, mode: str) -> list:
    src, tgt, tk, _ = assign(row)
    out = []
    if "g" in mode:
        out += [[b, e, "g", s, sh, sd if "s" in mode else None] for b, e, s, sh, sd in src]
    if "p" in mode:
        # x: the chip's class matches nothing the parser wrote
        out += [[b, e, "p", "none" if "x" in mode and not sw else c, sw] for b, e, c, sw in tgt]
    if "t" in mode:
        out += [[b, e, "t", k] for b, e, k in tk]
    return out


def norm(frame: list) -> list:
    """chip_edit: a pinned parameter's class or toggle family is not used."""
    return ["T:param" if t.startswith("T:param:") else "T:toggle" if t.startswith("T:toggle:") else t
            for t in frame]


def chip_ok(pred, gold) -> bool:
    return fp.compare(norm(pred), norm(gold))["route_edit"]


def diff(pred, gold) -> dict:
    """Why a frame misses route_edit: intent, route count, and the units that
    differ between the routes that do not match exactly."""
    p, g = fp.structure(pred), fp.structure(gold)
    why = Counter()
    if p["intent"] != g["intent"]:
        why["intent"] += 1
    np_, ng = sum(p["routes"].values()), sum(g["routes"].values())
    if np_ != ng:
        why["route count"] += 1
    pr, gr = p["routes"] - g["routes"], g["routes"] - p["routes"]
    pu = Counter(u for r in pr.elements() for u in r)
    gu = Counter(u for r in gr.elements() for u in r)
    for u in (pu - gu) + (gu - pu):
        why[bucket(u)] += 1
    if p["rest"] != g["rest"]:
        why["outside routes"] += 1
    if pr and gr and not (pu - gu) and not (gu - pu):
        why["split or merge"] += 1
    return why


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--rows", required=True)
    ap.add_argument("--name", default="run")
    ap.add_argument("--modes", default="gpts,gpt,gp")
    ap.add_argument("--dump", default=None)
    ap.add_argument("--bin", default=str(BIN))
    a = ap.parse_args()
    rows = [json.loads(l) for l in open(a.rows)]
    modes = a.modes.split(",")
    framed = {}
    for m in modes:
        # b: with the BIO head's spans (pin_spans)
        payload = "".join(json.dumps({"pred": r["pred"], "pred_bytes": r["pred_bytes"], "pins": pins(r, m),
                                      "text": r["text"], "spans": r.get("pred_bio", []) if "b" in m else []},
                                     ensure_ascii=False) + "\n" for r in rows)
        res = subprocess.run([a.bin], input=payload, capture_output=True, text=True, check=True)
        framed[m] = [json.loads(l)["frame"] for l in res.stdout.splitlines()]
        assert len(framed[m]) == len(rows)
    agg = defaultdict(lambda: defaultdict(float))
    why = Counter()
    for k, r in enumerate(rows):
        g = agg[r["split"]]
        g["n"] += 1
        g["base"] += fp.compare(r["pred"], r["gold"])["route_edit"]
        for m in modes:
            g[m] += fp.compare(framed[m][k], r["gold"])["route_edit"]
        for m in modes:
            g["ce_" + m] += chip_ok(framed[m][k], r["gold"])
        g["chip_edit"] += chip_ok(framed[modes[0]][k], r["gold"])
        if not chip_ok(framed[modes[0]][k], r["gold"]):
            why.update(diff(norm(framed[modes[0]][k]), norm(r["gold"])))
    keys = ["base"] + modes + ["chip_edit"] + ["ce_" + m for m in modes]
    table = {s: {k: g[k] / g["n"] for k in keys} for s, g in agg.items()}
    mean = {k: sum(t[k] for t in table.values()) / len(table) for k in keys}
    print(f"{a.name}: " + " ".join(f"{k} {mean[k]:.5f}" for k in keys))
    for s, t in table.items():
        print(f"  {s:9} n {int(agg[s]['n']):5} " + " ".join(f"{k} {t[k]:.4f}" for k in keys))
    n_wrong = sum(1 for k, r in enumerate(rows) if not chip_ok(framed[modes[0]][k], r["gold"]))
    print(f"  chip_edit wrong: {n_wrong} of {len(rows)} rows; units that differ (rows can count several):")
    print("   " + ", ".join(f"{b} {c}" for b, c in why.most_common()))
    if a.dump:
        with open(a.dump, "w") as f:
            for k, r in enumerate(rows):
                f.write(json.dumps({"split": r["split"], "id": r["id"], "text": r["text"], "gold": r["gold"],
                                    "pred": r["pred"], **{m: framed[m][k] for m in modes},
                                    "pins": pins(r, modes[0])}, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
