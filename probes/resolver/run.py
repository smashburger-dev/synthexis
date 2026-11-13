"""Resolver eval (RESULT.md): builds the rows and plugin tables, runs the
Rust example resolve_eval, scores it.

  python3 probes/resolver/run.py template 0          # dev seed
  python3 probes/resolver/run.py template 1 2        # held out, once
  python3 probes/resolver/run.py real calibration    # may be looked at
  python3 probes/resolver/run.py real eval           # once, frozen code

Writes rows, tables and outputs to data/mapping/resolver/ (not in git).
"""

import json
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAP = ROOT / "data" / "mapping"
OUT = MAP / "resolver"
SKIP = re.compile(r"^(x \(reserved\)|reserved\d*|midi cc|cc ?\d+)", re.I)
BIN = ROOT / "plugin" / "target" / "release" / "examples" / "resolve_eval"


def known() -> dict:
    """known.tsv: casefold plugin name -> {casefold param name: class}."""
    out, cur = defaultdict(dict), ""
    for line in open(MAP / "completion" / "known.tsv"):
        line = line.rstrip("\n")
        # a parameter can be named "#000" too (Kontakt): a plugin line has no tab
        if "\t" in line:
            n, c = line.split("\t", 1)
            out[cur][n] = c
        elif line.startswith("#"):
            cur = line[1:]
    return out


def tables(need: set) -> dict:
    """plugin_id -> [[index, name, class]]: the exported completion file,
    else the harvest names with known.tsv labels (sense.rs order; names
    without a label get "" here, param-sense is not run)."""
    vocab = json.loads((MAP / "completion" / "vocab.json").read_text())
    out = {}
    for p in vocab["plugins"]:
        f = json.loads((MAP / "completion" / "plugins" / p["file"]).read_text())
        out[p["plugin_id"]] = [[r["i"], r["n"], r["c"]] for r in f["params"]
                               if not SKIP.match(r["n"])]
    rest = need - set(out)
    if rest:
        lab, names = known(), defaultdict(dict)
        for fmt in ("au", "vst3"):
            for line in open(MAP / "harvest" / fmt / "params.jsonl"):
                p = json.loads(line)
                if p["plugin_id"] in rest and not SKIP.match(p["name"]):
                    names[p["plugin_id"]].setdefault(p["name"], p["index"])
        for pid, ns in names.items():
            k = lab.get(pid.split("/")[-1].casefold(), {})
            out[pid] = sorted([i, n, k.get(n.strip().casefold(), "")] for n, i in ns.items())
    return out


def rows(kind: str, which: list) -> list:
    if kind == "template":
        out = []
        for s in which:
            for k, line in enumerate(open(MAP / "open-vocab" / f"eval-s{s}.jsonl")):
                r = json.loads(line)
                out.append({"id": f"s{s}-{k}", "text": r["text"],
                            "plugin_id": r["plugin_id"], "gold": r["gold"]})
        return out
    out = []
    for line in open(MAP / "eval" / "expanded.jsonl"):
        r = json.loads(line)
        if r["split"] in which and r["plugin"] and r["params"]:
            out.append({"id": r["id"], "text": r["text"], "plugin_id": r["plugin"],
                        "gold": r["params"], "split": r["split"]})
    return out


def bound(pred: dict) -> list:
    return [t["name"] for r in pred["routes"] for t in r["targets"]
            if t["token"].startswith(("T:param:", "T:toggle:"))]


def main() -> None:
    kind, which = sys.argv[1], sys.argv[2:]
    OUT.mkdir(parents=True, exist_ok=True)
    rs = rows(kind, which)
    tab = tables({r["plugin_id"] for r in rs})
    rs = [r for r in rs if r["plugin_id"] in tab]
    tag = f"{kind}-{'-'.join(which)}"
    (OUT / "plugins.json").write_text(json.dumps(tab))
    with open(OUT / f"{tag}.rows.jsonl", "w") as f:
        for r in rs:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")
    subprocess.run([str(BIN), "--dir", str(MAP / "runtime"), "--plugins", str(OUT / "plugins.json"),
                    "--rows", str(OUT / f"{tag}.rows.jsonl"), "--out", str(OUT / f"{tag}.out.jsonl")],
                   check=True)
    preds = [json.loads(line) for line in open(OUT / f"{tag}.out.jsonl")]
    per = defaultdict(lambda: [0, 0, 0, 0])   # plugin -> [rows, hit/exact, gold, recalled]
    vias = defaultdict(int)
    for r, p in zip(rs, preds):
        names = bound(p)
        for rt in p["routes"]:
            for t in rt["targets"]:
                if t["via"]:
                    vias[t["via"]] += 1
        h = per[r["plugin_id"]]
        h[0] += 1
        if kind == "template":
            h[1] += bool(names) and names[0] in r["gold"]
        else:
            got = {n for n in names if n}
            h[1] += got == set(r["gold"])
            h[2] += len(set(r["gold"]))
            h[3] += len(set(r["gold"]) & got)
        r["pred"] = {"names": names, "describe": p["describe"], "frame": p["frame"]}
    with open(OUT / f"{tag}.scored.jsonl", "w") as f:
        for r in rs:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")
    n = sum(h[0] for h in per.values())
    if kind == "template":
        print(f"{tag}: {n} rows, top-1 {sum(h[1] for h in per.values()) / n:.3f}")
        for pid, h in sorted(per.items()):
            print(f"  {pid:50s} {h[1]}/{h[0]}")
    else:
        g, rec = sum(h[2] for h in per.values()), sum(h[3] for h in per.values())
        print(f"{tag}: {n} rows, gold recall {rec}/{g} = {rec / g:.3f}, "
              f"row exact {sum(h[1] for h in per.values())}/{n}")
        for pid, h in sorted(per.items()):
            print(f"  {pid:50s} exact {h[1]}/{h[0]}  recall {h[3]}/{h[2]}")
    print("  bound via", dict(vias))


if __name__ == "__main__":
    main()
