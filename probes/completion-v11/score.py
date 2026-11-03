# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "numpy"]
# ///
"""Simulated chips and pinning on dumped v11 frames (see RESULT.md).

  uv run probes/completion-v11/score.py --tools <v11 tools/mapping> \
      --rows rows.jsonl --completion data/mapping/completion [--name run]

Assigns oracle sources to gold SRC spans and classes to gold TGT spans, runs
plugin/complete/examples/sim_v11.rs (cargo, release) on them, and scores the
unpinned and pinned frames with frame_parser.compare.
"""

import argparse
import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

PLUGIN = Path(__file__).resolve().parents[2] / "plugin"
FOLD = {"hand.fist": "hand.openness", "eye.closed": "eye.open"}
# target chips (probes/completion-targets)
TARGET_KINDS = {"midi.note", "midi.chord", "midi.pattern", "midi.cc", "macro"}


def routes(frame):
    out, cur = [], None
    for t in frame:
        if t == "<route>":
            cur = []
        elif t == "</route>":
            out.append(cur or [])
            cur = None
        elif cur is not None:
            cur.append(t)
    return out


def sources(route):
    """Every S: token of a route (fan-in has several), folded, with the
    shape and the side that follow it."""
    out = []
    for i, t in enumerate(route):
        if t.startswith("S:"):
            shape = side = None
            for u in route[i + 1:]:
                if u.startswith("S:"):
                    break
                if u.startswith("shape:"):
                    shape = u[6:]
                if u.startswith("side:"):
                    side = u[5:]
            out.append((FOLD.get(t[2:], t[2:]), shape, side))
    return out


def assign(row):
    """-> (src spans with sources, tgt spans with classes), [] when unclear."""
    rs = routes(row["gold"])
    srcs = [(b, e) for b, e, k in row["bio"] if k == "SRC"]
    tgts = [(b, e) for b, e, k in row["bio"] if k == "TGT"]
    distinct = []
    for r in rs:
        for s in sources(r):
            if s not in distinct:
                distinct.append(s)
    src = []
    if srcs and len(srcs) == len(distinct):
        src = [[b, e, s, sh, sd] for (b, e), (s, sh, sd) in zip(srcs, distinct)]
    elif srcs and len(distinct) == 1:
        src = [[b, e, *distinct[0]] for b, e in srcs]
    ts = [t for r in rs for t in r if t.startswith("T:")]
    if len(ts) != len(tgts):
        ts = [next((t for t in r if t.startswith("T:")), None) for r in rs]
    tgt, tk = [], []
    if tgts and len(ts) == len(tgts):
        for (b, e), t in zip(tgts, ts):
            if t and t.startswith("T:param:"):
                tgt.append([b, e, t[8:], False])
            elif t and t.startswith("T:toggle:"):
                tgt.append([b, e, "ctl.switch", True])
            elif t and t[2:].split(":")[0] in TARGET_KINDS:
                tk.append([b, e, t[2:].split(":")[0]])
    return src, tgt, tk, [d[0] for d in distinct]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--tools", required=True)
    ap.add_argument("--rows", required=True)
    ap.add_argument("--completion", required=True)
    ap.add_argument("--name", default="run")
    ap.add_argument("--sim", default=None, help="a built sim_v11 binary (default: build it)")
    ap.add_argument("--out", default=None, help="JSON with the numbers")
    ap.add_argument("--dump", default=None, help="JSONL of rows with their pinned frames")
    a = ap.parse_args()
    sys.path.insert(0, a.tools)
    import frame_parser as fp

    rows = [json.loads(l) for l in open(a.rows)]
    sim_in = []
    for r in rows:
        src, tgt, tk, gsrc = assign(r)
        sim_in.append({"text": r["text"], "pred": r["pred"], "pred_bytes": r["pred_bytes"],
                       "src": src, "tgt": tgt, "tk": tk, "gold_sources": gsrc})
    payload = "".join(json.dumps(x, ensure_ascii=False) + "\n" for x in sim_in)
    if not a.sim:
        subprocess.run(["cargo", "build", "--release", "-q", "-p", "synthexis-complete", "--example", "sim_v11"],
                       cwd=PLUGIN, check=True)
    sim = a.sim or str(PLUGIN / "target/release/examples/sim_v11")
    res = subprocess.run([sim, a.completion],
                         input=payload, capture_output=True, text=True, check=True)
    outs = [json.loads(l) for l in res.stdout.splitlines()]
    assert len(outs) == len(rows)

    if a.dump:
        with open(a.dump, "w") as f:
            for r, x, o in zip(rows, sim_in, outs):
                f.write(json.dumps({**r, **o, "src": x["src"], "tgt": x["tgt"]}, ensure_ascii=False) + "\n")
    agg = defaultdict(lambda: defaultdict(float))
    for r, o in zip(rows, outs):
        g = agg[r["split"]]
        g["n"] += 1
        g["base"] += fp.compare(r["pred"], r["gold"])["route_edit"]
        for k in ("G", "P", "GP", "GPT"):
            g[f"pin_{k}"] += fp.compare(o[k], r["gold"])["route_edit"]
        for k in ("src_total", "src_hits", "src_ghost", "words", "false_ghost", "tk_total", "tk_hits"):
            g[k] += o[k]
    table = {}
    for split, g in agg.items():
        n = g["n"]
        table[split] = {
            "n": int(n),
            **{k: round(g[k] / n, 4) for k in ("base", "pin_G", "pin_P", "pin_GP", "pin_GPT")},
            "T8": round(g["tk_hits"] / max(g["tk_total"], 1), 4),
            "R3": round(g["src_hits"] / max(g["src_total"], 1), 4),
            "R1": round(g["src_ghost"] / max(g["src_total"], 1), 4),
            "F": round(g["false_ghost"] / max(g["words"], 1), 4),
        }
    keys = ("base", "pin_G", "pin_P", "pin_GP", "pin_GPT", "T8", "R3", "R1", "F")
    mean = {k: round(sum(t[k] for t in table.values()) / len(table), 4) for k in keys}
    print(f"{a.name}: " + " ".join(f"{k} {mean[k]}" for k in keys))
    for split, t in table.items():
        print(f"  {split:9} " + " ".join(f"{k} {t[k]}" for k in keys))
    if a.out:
        Path(a.out).write_text(json.dumps({"name": a.name, "mean": mean, "splits": table}, indent=1))


if __name__ == "__main__":
    main()
