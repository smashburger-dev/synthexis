# /// script
# requires-python = ">=3.11"
# dependencies = ["pyyaml", "httpx"]
# ///
"""Round-trip check: send generated sentences back through Spark
(prompts/roundtrip.md) and compare its parsed edit with the gold one.

  uv run tools/mapping/roundtrip.py --run rt-pilot
      --sentences-run cmd-v2-pilot --max-usd 0.5 [--all] [--seed 3]
"""

import argparse
import json
import random
import re
import sys
from collections import Counter
from pathlib import Path

import yaml

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import spark  # noqa: E402

ROOT = HERE.parents[1]
JOB_SIZE = 25
JOB_META: dict = {}
RE_LINE = re.compile(r"^(\S+)\s+(\{.*\})\s*$")

FOLD = {"hand.fist": ("hand.openness", True),
        "eye.closed": ("eye.open", True)}


def system_prompt(onto: dict) -> str:
    tax = "\n".join(f"{t['id']} | {t['quantity']}"
                    for t in onto["taxonomy"])
    srcs = "\n".join(f"{s['id']} | {s['desc']}"
                     for s in onto["sources"])
    fams = ", ".join(onto.get("families", []))
    prompt = (HERE / "prompts" / "roundtrip.md").read_text()
    return (f"{prompt}\n{tax}\n\n{srcs}\n\nfamilies: {fams}\n"
            f"refs: last_route (the route just made), "
            f"last_track (the track just named)")


def fold_src(s: str):
    """(source, side, flip) — folds fist->openness etc."""
    side = None
    if ":" in s:
        s, side = s.rsplit(":", 1)
    flip = False
    if s in FOLD:
        s, flip = FOLD[s]
    return s, side, flip


def gold_routes(e: dict) -> list:
    """[ (sources {(id,side,flip)}, has_inv, targets {id}) ]"""
    out = []
    for r in e["routes"]:
        srcs, tgts = set(), set()
        if r.get("ref"):
            srcs.add(("ref", None, False))
            out.append({"srcs": srcs, "inv": False, "tgts": tgts,
                        "from_to": None})
            continue
        for sid in r.get("src", []):
            s = e["slots"][sid]
            out_s, out_side, out_flip = fold_src(s["value"])
            srcs.add((out_s, s.get("side") or out_side, out_flip))
        inv = any(e["slots"][m]["kind"] == "invert"
                  for m in r.get("mods", []))
        for sid in r.get("tgt", []):
            s = e["slots"][sid]
            t = s.get("target")
            if t == "param":
                tgts.add(s["value"])
            elif t == "toggle":
                tgts.add(f"toggle:{s['value']}")
            elif t == "macro":
                tgts.add("macro")
            elif t == "midi.note":
                tgts.add("midi.note")
            elif t == "midi.pattern":
                tgts.add("midi.pattern")
            elif t == "midi.chord":
                tgts.add("midi.chord")
            elif t == "midi.cc":
                tgts.add("midi.cc")
        out.append({"srcs": srcs, "inv": inv, "tgts": tgts,
                    "from_to": None})
    if e["intent"] == "replace":
        fr = e["slots"].get("from1", {}).get("value")
        to = e["slots"].get("to1", {}).get("value")
        srcs, tgts = set(), set()
        for sid, s in e["slots"].items():
            if s["kind"] == "source":
                o, sd, fl = fold_src(s["value"])
                srcs.add((o, s.get("side") or sd, fl))
        return [{"srcs": srcs, "inv": False, "tgts": tgts,
                 "from_to": (fr, to)}]
    return out


def pred_routes(pred: dict) -> list:
    out = []
    for r in pred.get("routes") or []:
        if not isinstance(r, dict):
            continue
        srcs, tgts = set(), set()
        src_list = [x for x in (r.get("src") or []) if isinstance(x, str)]
        tgt_list = [x for x in (r.get("tgt") or []) if isinstance(x, str)]
        for raw in src_list:
            s, side, flip = fold_src(raw)
            srcs.add((s, side, flip))
        from_to = None
        if r.get("from") or r.get("to"):
            from_to = (r.get("from"), r.get("to"))
        if "ref" in tgt_list:
            srcs.add(("ref", None, False))
        for t in tgt_list:
            if t.startswith("toggle:") or t == "ref":
                tgts.add(t)
            elif t.startswith("macro"):
                tgts.add("macro")
            elif t in ("midi.note", "midi.chord", "midi.pattern",
                       "midi.cc"):
                tgts.add(t)
            else:
                tgts.add(t)
        out.append({"srcs": srcs, "inv": bool(r.get("inv")),
                    "tgts": tgts, "from_to": from_to})
    return out


def xor_flip(routes: list, field: str) -> list:
    """fold flips into the invert flag for comparison."""
    out = []
    for r in routes:
        flip = any(f for (_s, _sd, f) in r["srcs"])
        plain = {(s, sd) for (s, sd, _f) in r["srcs"]}
        out.append({"srcs": plain,
                    "inv": bool(r["inv"]) != flip,
                    "tgts": r["tgts"]})
    return out


def xor_one(r: dict) -> dict:
    flip = any(f for (_s, _sd, f) in r["srcs"])
    return {"srcs": {(s, sd) for (s, sd, _f) in r["srcs"]},
            "inv": bool(r["inv"]) != flip,
            "tgts": r["tgts"], "from_to": r["from_to"]}


def route_diff(gr: dict, pr: dict) -> int:
    """Mismatch count between one gold and one pred route."""
    d = 0
    if gr["srcs"] != pr["srcs"]:
        d += 1
    if gr["inv"] != pr["inv"]:
        d += 1
    if gr["tgts"] != pr["tgts"]:
        d += 1
    if gr["from_to"] != pr["from_to"]:
        d += 1
    return d


def best_match(gold: list, pred: list) -> list:
    """Greedy order-free pairing of gold and pred routes."""
    pairs = []
    used = set()
    for gi, gr in enumerate(gold):
        best, bj = None, None
        for pj, pr in enumerate(pred):
            if pj in used:
                continue
            d = route_diff(gr, pr)
            if best is None or d < best:
                best, bj = d, pj
        if bj is not None:
            used.add(bj)
            pairs.append((gi, bj))
    return pairs


def compare(e: dict, pred: dict) -> dict:
    """-> dict of field -> (gold, pred) for mismatches."""
    bad = {}
    if pred.get("intent") != e["intent"]:
        bad["intent"] = (e["intent"], pred.get("intent"))
        return bad
    g = [xor_one(r) for r in gold_routes(e)]
    p = [xor_one(r) for r in pred_routes(pred)]
    if len(g) != len(p):
        bad["routes"] = (len(g), len(p))
        return bad
    for gi, pj in best_match(g, p):
        gr, pr = g[gi], p[pj]
        if gr["srcs"] != pr["srcs"]:
            bad[f"r{gi+1}.sources"] = (sorted(gr["srcs"]),
                                       sorted(pr["srcs"]))
        if gr["inv"] != pr["inv"]:
            bad[f"r{gi+1}.invert"] = (gr["inv"], pr["inv"])
        if gr["tgts"] != pr["tgts"]:
            bad[f"r{gi+1}.targets"] = (sorted(gr["tgts"]),
                                       sorted(pr["tgts"]))
        if gr["from_to"] != pr["from_to"]:
            bad[f"r{gi+1}.from_to"] = (gr["from_to"],
                                       pr["from_to"])
    return bad


def make_parse():
    def parse(job: spark.Job, text: str):
        out = {}
        for line in text.splitlines():
            m = RE_LINE.match(line.strip())
            if not m:
                continue
            try:
                out[m.group(1)] = json.loads(m.group(2))
            except json.JSONDecodeError:
                continue
        if len(out) < len(JOB_META[job.id]["sents"]) // 2:
            raise spark.ParseError(
                f"only {len(out)} of "
                f"{len(JOB_META[job.id]['sents'])} parsed")
        return out
    return parse


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--sentences-run", required=True)
    ap.add_argument("--max-usd", type=float, required=True)
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--seed", type=int, default=3)
    ap.add_argument("--workers", type=int, default=12)
    args = ap.parse_args()

    onto = yaml.safe_load(
        (HERE / "ontology.yaml").read_text())
    s_path = (ROOT / "data" / "mapping" / "commands" /
              args.sentences_run / "sentences.jsonl")
    sents = [json.loads(l) for l in s_path.read_text().splitlines()
             if l.strip()]

    rng = random.Random(args.seed)
    if args.all:
        chosen = sents
    else:
        inv_ids = {s["id"] for s in sents
                   if any(sl.get("kind") == "invert"
                          for sl in s["edit"]["slots"].values())}
        chosen = [s for s in sents
                  if s["id"] in inv_ids or rng.random() < 0.05]
    print(f"{len(sents)} sentences, {len(chosen)} selected")

    sys_prompt = system_prompt(onto)
    jobs = []
    for start in range(0, len(chosen), JOB_SIZE):
        part = chosen[start:start + JOB_SIZE]
        jid = f"j{len(jobs):04d}"
        user = "\n".join(f"{s['id']} {s['text']}" for s in part)
        JOB_META[jid] = {"sents": part}
        jobs.append(spark.Job(
            id=jid,
            messages=[{"role": "system", "content": sys_prompt},
                      {"role": "user", "content": user}],
            est_out_tokens=len(part) * 60 + 2000))

    out_dir = ROOT / "data" / "mapping" / "roundtrip" / args.run
    out_dir.mkdir(parents=True, exist_ok=True)
    if args.workers and False:
        pass
    parsed = spark.run(
        args.run, jobs, make_parse(), max_usd=args.max_usd,
        workers=args.workers, reasoning_effort="low")

    agree = {"intent": [0, 0], "sources": [0, 0],
             "invert": [0, 0], "targets": [0, 0],
             "routes": [0, 0]}
    drops = []
    with (out_dir / "roundtrip.jsonl").open("w") as f:
        for jid, res in parsed.items():
            for s in JOB_META[jid]["sents"]:
                sid = s["id"]
                pred = res.get(sid)
                agree["intent"][1] += 1
                agree["sources"][1] += 1
                agree["invert"][1] += 1
                agree["targets"][1] += 1
                agree["routes"][1] += 1
                if pred is None:
                    bad = {"missing": True}
                else:
                    bad = compare(s["edit"], pred)
                rec = {"id": sid, "mismatch": bad}
                f.write(json.dumps(rec) + "\n")
                if not bad:
                    for k in agree:
                        agree[k][0] += 1
                else:
                    if "intent" in bad:
                        agree["sources"][0] += 1
                        agree["invert"][0] += 1
                        agree["targets"][0] += 1
                        agree["routes"][0] += 1
                    else:
                        agree["intent"][0] += 1
                        if not any(k.endswith(".sources")
                                   or k == "routes" for k in bad):
                            agree["sources"][0] += 1
                        if not any(k.endswith(".invert")
                                   or k == "routes" for k in bad):
                            agree["invert"][0] += 1
                        if not any(k.endswith(".targets")
                                   or k == "routes" for k in bad):
                            agree["targets"][0] += 1
                        if "routes" not in bad:
                            agree["routes"][0] += 1
                    if any(k in bad for k in
                           ("intent", "routes")) or any(
                               k.endswith((".sources", ".targets",
                                           ".invert"))
                               for k in bad):
                        drops.append(sid)
    (out_dir / "drop_ids.json").write_text(json.dumps(drops))
    summary = {
        "sentences": len(chosen),
        "parsed": sum(1 for _ in parsed),
        "agreement": {k: round(v[0] / v[1], 4) if v[1] else None
                      for k, v in agree.items()},
        "dropped": len(drops),
    }
    (out_dir / "summary.json").write_text(
        json.dumps(summary, indent=1))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
