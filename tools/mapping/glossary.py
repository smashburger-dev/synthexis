# /// script
# requires-python = ">=3.11"
# dependencies = ["httpx", "pyyaml"]
# ///
"""Build the phrase glossary (en + de) via Muse Spark Open.

  uv run tools/mapping/glossary.py --run <name> --max-usd <cap>
      [--lang en,de] [--kinds source,param,...] [--limit-jobs N]
      [--dry-run]

Concepts are <kind>:<id> pairs derived from ontology.yaml and
edits.yaml. Jobs group up to 8 concepts of one kind and one language.
Writes data/mapping/glossary/<run>/{glossary.jsonl, summary.json,
ambiguous.json}.
"""

import argparse
import json
import sys
from collections import Counter
from pathlib import Path

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parent))
import spark  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
MAX_CONCEPTS_PER_JOB = 8
FINGERS = ["thumb", "index", "middle", "ring", "pinky"]

JOB_META: dict[str, dict] = {}

MOD_CONCEPTS = [
    ("mod:invert", "modifier",
     "the target moves opposite to the source"),
    ("mod:range.up_to", "modifier",
     "the route applies only up to a ceiling, 'up to half'"),
    ("mod:range.from", "modifier",
     "the route applies only above a floor, 'from 20 percent'"),
    ("mod:range.between", "modifier",
     "the route applies between two ends, 'between 20 and 80 percent'"),
    ("mod:curve.exp", "modifier", "exponential response curve"),
    ("mod:curve.log", "modifier", "logarithmic response curve"),
    ("mod:curve.s", "modifier", "smooth s-curve response"),
    ("mod:amount.0.25", "modifier", "route depth: subtle"),
    ("mod:amount.0.5", "modifier", "route depth: medium"),
    ("mod:amount.0.75", "modifier", "route depth: strong"),
    ("mod:amount.1.0", "modifier", "route depth: full"),
    ("mod:smooth.low", "modifier", "smoothing low, snappy"),
    ("mod:smooth.medium", "modifier", "smoothing medium"),
    ("mod:smooth.high", "modifier", "smoothing high, smooth"),
    ("mod:hold.hold", "modifier",
     "when the source disappears: hold the value"),
    ("mod:hold.reset", "modifier",
     "when the source disappears: reset to the start"),
    ("mod:hold.freeze", "modifier",
     "when the source disappears: freeze everything"),
    ("mod:quantize.steps", "modifier",
     "quantize the output to N steps"),
    ("mod:quantize.scale", "modifier",
     "quantize the output to the scale"),
    ("mod:delta.up", "modifier",
     "make the current value a bit more, raise it"),
    ("mod:delta.down", "modifier",
     "make the current value a bit less, lower it"),
    ("mod:gate", "modifier",
     "the route is only active while another source or shape is on"),
    ("mod:time", "modifier",
     "the change ramps over a time, e.g. over 4 bars"),
    ("mod:correction", "modifier",
     "self-correction words: no wait, I mean, äh nee, sorry meant"),
    ("ref:last_route", "ref",
     "the route just made: the same, that one, it"),
    ("ref:last_track", "ref",
     "the track just named: there, same track"),
]

SIDE_CONCEPTS = [
    ("side:left", "left hand or eye"),
    ("side:right", "right hand or eye"),
    ("side:both", "both hands"),
    ("side:either", "either hand"),
]

ROLE_CONCEPTS = [
    ("role:midi", "the hand plays MIDI notes or chords"),
    ("role:parameter", "the hand drives plugin parameters"),
    ("role:motion", "the hand triggers motion events"),
    ("role:off", "the hand does nothing"),
]


def shape_meaning(mask: str) -> str:
    on = [FINGERS[i] for i, b in enumerate(mask) if b == "1"]
    if not on:
        return "finger state: closed fist, no fingers extended"
    if len(on) == 5:
        return "finger state: open hand, all fingers extended"
    return "finger state: only " + " and ".join(on) + " extended"


def build_concepts(onto: dict, edits: dict) -> list[dict]:
    out = []

    def add(cid, kind, meaning):
        out.append({"c": cid, "kind": kind, "meaning": meaning})

    for s in onto["sources"]:
        add(f"source:{s['id']}", "source", s["desc"])
    for s in onto["shapes"]:
        add(f"shape:{s['id']}", "shape", shape_meaning(s["mask"]))
    for t in onto["taxonomy"]:
        first = t["desc"].split(". ")[0].rstrip(".")
        add(f"param:{t['id']}", "param", f"{t['quantity']}: {first}.")
    for t in onto["targets"]:
        add(f"target:{t['id']}", "target", t["desc"])
    for t in onto["intents"]:
        add(f"intent:{t['id']}", "intent", t["desc"])
    for cid, kind, meaning in MOD_CONCEPTS:
        add(cid, kind, meaning)
    for f_ in onto.get("families", []):
        add(f"family:{f_}", "family",
            f"an effect family: the {f_} (its bypass or the effect itself)")
    for t in onto["tracks"]:
        add(f"track:{t}", "track",
            f"the {t} channel/track role in a project")
    for p in onto["presets"]:
        add(f"preset:{p['id']}", "preset", p["desc"])
    for o in edits["objects"]:
        add(f"object:{o}", "object", f"a tracked object: {o}")
    for cid, meaning in SIDE_CONCEPTS:
        add(cid, "side", meaning)
    for cid, meaning in ROLE_CONCEPTS:
        add(cid, "role", meaning)
    for sc in edits["scales"]:
        add(f"scale:{sc}", "scale", f"the {sc} scale")
    return out


def system_prompt(onto: dict) -> str:
    lines = ["", "## Ontology", "taxonomy (id | quantity | meaning):"]
    for t in onto["taxonomy"]:
        lines.append(f"{t['id']} | {t['quantity']} | {t['desc']}")
    lines.append("sources (id | meaning):")
    for s in onto["sources"]:
        lines.append(f"{s['id']} | {s['desc']}")
    lines.append("shapes (id | extended fingers):")
    for s in onto["shapes"]:
        lines.append(f"{s['id']} | {shape_meaning(s['mask'])}")
    lines.append("targets (id | meaning):")
    for t in onto["targets"]:
        lines.append(f"{t['id']} | {t['desc']}")
    lines.append("intents (id | meaning):")
    for t in onto["intents"]:
        lines.append(f"{t['id']} | {t['desc']}")
    lines.append("presets (id | meaning):")
    for p in onto["presets"]:
        lines.append(f"{p['id']} | {p['desc']}")
    return (HERE / "prompts" / "glossary.md").read_text() \
        + "\n" + "\n".join(lines)


def build_jobs(concepts: list, langs: list, sys_prompt: str):
    by_key: dict[tuple, list] = {}
    for c in concepts:
        by_key.setdefault((c["kind"],), []).append(c)
    jobs = []
    # chunk index outer, language inner -> --limit-jobs 2 covers en+de
    for (kind,), cl in sorted(by_key.items()):
        for start in range(0, len(cl), MAX_CONCEPTS_PER_JOB):
            part = cl[start:start + MAX_CONCEPTS_PER_JOB]
            for lang in langs:
                jid = f"{lang}-{kind}-{start // MAX_CONCEPTS_PER_JOB}"
                lines = [f"language: {lang}"]
                for c in part:
                    lines.append(
                        f'- id={c["c"]} kind={c["kind"]} '
                        f'meaning="{c["meaning"]}"')
                JOB_META[jid] = {"lang": lang, "concepts": part}
                jobs.append(spark.Job(
                    id=jid,
                    messages=[
                        {"role": "system", "content": sys_prompt},
                        {"role": "user", "content": "\n".join(lines)},
                    ],
                    est_out_tokens=len(part) * 35 * 18 + 2000))
    return jobs


def make_parse():
    def parse(job: spark.Job, text: str):
        meta = JOB_META[job.id]
        ids = {c["c"] for c in meta["concepts"]}
        got: dict[str, list] = {c: [] for c in ids}
        seen: set = set()
        for line in text.splitlines():
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                continue
            c, p = r.get("c"), r.get("p")
            if c not in ids or not isinstance(p, str) or not p.strip() \
                    or len(p) > 80:
                continue
            if r.get("f") not in ("n", "v") \
                    or r.get("pol") not in (1, -1):
                continue
            key = (c, p.casefold())
            if key in seen:
                continue
            seen.add(key)
            rec = {"p": p.strip(), "f": r["f"], "pol": r["pol"]}
            if r.get("typo"):
                rec["typo"] = 1
            got[c].append(rec)
        n_ok = sum(1 for v in got.values() if len(v) >= 10)
        if n_ok < len(ids) / 2:
            raise spark.ParseError(
                f"{n_ok}/{len(ids)} concepts with >=10 phrases")
        return got
    return parse


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--max-usd", type=float, required=True)
    ap.add_argument("--lang", default="en,de")
    ap.add_argument("--kinds", default="")
    ap.add_argument("--limit-jobs", type=int, default=0)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--reasoning-effort", default="low")
    args = ap.parse_args()

    onto = yaml.safe_load((HERE / "ontology.yaml").read_text())
    edits = yaml.safe_load((HERE / "edits.yaml").read_text())
    concepts = build_concepts(onto, edits)
    if args.kinds:
        kinds = {k.strip() for k in args.kinds.split(",") if k.strip()}
        concepts = [c for c in concepts if c["kind"] in kinds]
    langs = [l.strip() for l in args.lang.split(",") if l.strip()]
    sys_prompt = system_prompt(onto)
    jobs = build_jobs(concepts, langs, sys_prompt)
    if args.limit_jobs:
        jobs = jobs[: args.limit_jobs]

    est = sum(spark._estimate_usd(j) for j in jobs)
    n_concepts = sum(len(JOB_META[j.id]["concepts"]) for j in jobs)
    print(f"{len(concepts)} concepts x {len(langs)} languages -> "
          f"{len(jobs)} jobs ({n_concepts} concept instances), "
          f"estimate ${est:.3f}")

    out_dir = ROOT / "data" / "mapping" / "glossary" / args.run
    out_dir.mkdir(parents=True, exist_ok=True)
    if args.dry_run:
        with (out_dir / "sample_jobs.txt").open("w") as f:
            f.write("SYSTEM PROMPT:\n" + sys_prompt + "\n\n")
            for j in jobs[:3]:
                f.write(f"=== JOB {j.id} ===\n"
                        + j.messages[1]["content"] + "\n\n")
        print(f"dry-run: wrote {out_dir / 'sample_jobs.txt'}")
        return

    parsed = spark.run(
        args.run, jobs, make_parse(), max_usd=args.max_usd,
        workers=8, temperature=0.7,
        reasoning_effort=args.reasoning_effort)

    rows = []
    phrase_concepts: dict[str, set] = {}
    per_concept: dict[tuple, int] = Counter()
    for jid, got in parsed.items():
        meta = JOB_META[jid]
        kind_of = {c["c"]: c["kind"] for c in meta["concepts"]}
        seen = set()
        for c, phrases in got.items():
            for rec in phrases:
                key = (meta["lang"], c, rec["p"].casefold())
                if key in seen:
                    continue
                seen.add(key)
                rows.append({
                    "lang": meta["lang"], "c": c, "kind": kind_of[c],
                    "p": rec["p"], "f": rec["f"], "pol": rec["pol"],
                    **({"typo": 1} if rec.get("typo") else {})})
                phrase_concepts.setdefault(
                    (meta["lang"], rec["p"].casefold()), set()).add(c)
                per_concept[(meta["lang"], c)] += 1

    rows.sort(key=lambda r: (r["lang"], r["c"]))
    with (out_dir / "glossary.jsonl").open("w") as f:
        for r in rows:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")

    counts = sorted(per_concept.values())
    import statistics
    ambiguous = [
        {"lang": lang, "p": p, "concepts": sorted(cs)}
        for (lang, p), cs in sorted(phrase_concepts.items())
        if len(cs) >= 2]
    (out_dir / "ambiguous.json").write_text(
        json.dumps(ambiguous, indent=1, ensure_ascii=False))
    under15 = sorted(f"{lang}:{c}={n}"
                     for (lang, c), n in per_concept.items() if n < 15)
    summary = {
        "phrases": len(rows),
        "concepts_with_phrases": len(per_concept),
        "per_concept": {
            "min": counts[0] if counts else 0,
            "median": statistics.median(counts) if counts else 0,
            "max": counts[-1] if counts else 0,
        },
        "under_15": under15,
        "ambiguous_phrases": len(ambiguous),
        "spend_usd": 0.0,
    }
    spend = 0.0
    lp = spark._ledger_path()
    if lp.exists():
        for line in lp.read_text().splitlines():
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                continue
            if r.get("run") == args.run:
                spend += float(r.get("cost_usd") or 0)
    summary["spend_usd"] = round(spend, 4)
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=1))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
