# /// script
# requires-python = ">=3.11"
# dependencies = ["pyyaml", "httpx>=0.27"]
# ///
"""Teacher data, done clean (parser v8, lever 3).

  uv run tools/mapping/teacher_v8.py check            # 100 dev_s5 rows, both orders
  uv run tools/mapping/teacher_v8.py label --order a  # pool, pass a
  uv run tools/mapping/teacher_v8.py label --order b  # pool, pass b
  uv run tools/mapping/teacher_v8.py merge [--src ds-v6p] [--out ds-v8t]

v7's teacher prompt (probes/real-eval/spark_parse.py) carried rules written
from calibration errors (pinch as a gate, both directions as one route,
chained targets) and picked its examples by the same findings. This prompt
is built from the label spec only: the frame grammar of
probes/mapping-pipeline/DESIGN.md and the ids and descriptions of
tools/mapping/ontology.yaml. Its examples are a seeded random draw from
ds-v6p train_cat, stratified by intent and target kind (the ontology's own
categories), without toggle rows that lack a switching word (labels that
contradict the ontology's toggle definition). Nothing in it comes from the
real set, calibration or eval.

DeepSeek V4.1 Flash (thinking off, temperature 0) labels each pool sentence
twice, with the same examples in two orders (a, b). merge keeps a row when
both passes agree on the route multiset (probes/real-eval/score.py canon);
its frame is the edit's when the edit agrees too (exact parameter ids,
modifiers and pointers), else the teacher's, projected like v7's. DeepSeek
only sees synthetic sentences.
"""

import argparse
import json
import random
import shutil
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import yaml

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(ROOT / "probes" / "real-eval"))
import project_labels as PL  # noqa: E402
import score as S  # noqa: E402
import spark  # noqa: E402
import spark_parse as P  # noqa: E402
import teacher_check as TC  # noqa: E402
import teacher_pool as TP  # noqa: E402

DATA = ROOT / "data" / "mapping"
OUT = DATA / "teach"
TRAIN = DATA / "train" / "ds-v6p" / "train_cat.jsonl"
N_EXAMPLES = 24

RULES = """You parse commands for synthexis, an audio plugin for FL Studio that turns camera-tracked hands, face, head, objects and movements into automation and MIDI. A producer types one sentence in English or German. Write the edit it asks for as a frame: a flat list of tokens.

## Frame language

- First token: the intent, one of {intents}.
- Then one `<route> ... </route>` per set of sources, holding the target or targets they drive. Inside a route:
  - sources `S:<source id>` (list below), each optionally followed by `side:left`, `side:right` or `side:both` when the sentence names a hand or side.
  - a held hand shape as source: `S:hand.shape` followed by `shape:<name>` ({shapes}) or `shape:m<5 bits>` for any other set of extended fingers, bits in the order thumb index middle ring pinky.
  - gates `G:<source id>` (with `shape:<name>` after `G:hand.shape`): {gate}
  - `INV`: {invert}
  - targets: {targets}
  - `mode:<m>` after a target: {mode}
  - `VALUE` once per value: {value}
  - names may follow as `TRACK =<word>`, `PLUGIN =<word>`, `PNAME =<word>`; they are optional.
- Polarity: every source id names the state that reads 1 (hand.openness is an open hand, hand.fist a closed fist).

## Output

One line per sentence, nothing else: `<id> <tokens separated by spaces>`. Write your best reading for every sentence; do not explain.

## Sources
"""


def onto() -> dict:
    return yaml.safe_load((HERE / "ontology.yaml").read_text())


def rules() -> str:
    o = onto()
    m = o["modifiers"]
    return RULES.format(
        intents=", ".join(f"I:{i['id']} ({i['desc']})" for i in o["intents"]),
        shapes=", ".join(s["id"] for s in o["shapes"]),
        gate=m["gate"], invert=m["invert"], mode=m["mode"], value=m["value"],
        targets="; ".join(f"`T:{t['id']}:<...>` {t['desc']}"
                          for t in o["targets"]))


def stratum(f: list) -> str:
    kinds = sorted({t.split(":")[1] for t in f if t.startswith("T:")})
    return f"{f[0]}|{','.join(kinds)}"


def examples(seed: int = 0) -> list:
    """Seeded draw, one row per (intent, target kinds) stratum in turn."""
    rows = [json.loads(line) for line in open(TRAIN)]
    rng = random.Random(seed)
    rng.shuffle(rows)
    by, seen = {}, set()
    for r in rows:
        f = [t for t in r["frame"] if isinstance(t, str)]
        toggle = any(t.startswith("T:toggle") for t in f)
        if len(r["text"]) > 240 or r["text"] in seen or \
                (toggle and not P.STRICT.search(r["text"])) or \
                PL.unswitched(f, r["text"]):
            continue
        seen.add(r["text"])
        by.setdefault(stratum(f), []).append(r)
    keys = sorted(by)
    rng.shuffle(keys)
    out = []
    while len(out) < N_EXAMPLES and any(by[k] for k in keys):
        for k in keys:
            if by[k] and len(out) < N_EXAMPLES:
                out.append(by[k].pop())
    return out


def system_prompt(order: str) -> str:
    o = onto()
    srcs = "\n".join(f"{s['id']} | {s['desc']}" for s in o["sources"])
    fams = ", ".join(o.get("families", []))
    ex = examples()
    if order == "b":
        ex = ex[::-1]
    shown = "\n".join(f"{r['text']}\n-> {P.render(r['frame'], r['text'])}\n"
                      for r in ex)
    return (f"{rules()}{srcs}\n\nEffect families for T:toggle: {fams}\n\n"
            f"## Examples\n\n{shown}")


def label(rows: list, order: str, workers: int, idkey: str) -> tuple:
    """-> ({id: tokens}, usd). Chunks of P.JOB_SIZE, one retry on a bad reply."""
    sysmsg, k = system_prompt(order), TC.key()
    cs = [rows[i:i + P.JOB_SIZE] for i in range(0, len(rows), P.JOB_SIZE)]
    got, spent = {}, [0.0]

    def one(c):
        ids = {r[idkey] for r in c}
        msgs = [{"role": "system", "content": sysmsg},
                {"role": "user", "content": "\n".join(
                    f"{r[idkey]} {r['text']}" for r in c)}]
        for _ in range(2):
            text, usd = TC.deepseek(msgs, False, k)
            spent[0] += usd
            try:
                return P.make_parse({"x": ids})(spark.Job("x", [], 0), text)
            except spark.ParseError:
                pass
        return {}

    with ThreadPoolExecutor(workers) as ex:
        for i, part in enumerate(ex.map(one, cs)):
            got |= part
            if (i + 1) % 50 == 0:
                print(f"  {i + 1}/{len(cs)} jobs, {spent[0]:.3f} $", flush=True)
    return got, spent[0]


def cmd_check(a) -> None:
    rows = [json.loads(line) for line in open(TC.DEV)]
    rows = [r for r in rows if r["frame"][0] == "I:map"]
    rows = random.Random(0).sample(rows, 100)
    gold = {r["id"]: S.canon([t for t in r["frame"] if isinstance(t, str)])
            for r in rows}
    preds, usd = {}, 0.0
    for order in ("a", "b"):
        preds[order], u = label(rows, order, 4, "id")
        usd += u
        ok = sum(S.canon(preds[order].get(i, [])) == g for i, g in gold.items())
        print(f"order {order}: exact {ok}/100, parsed {len(preds[order])}")
    agree = [i for i in gold if i in preds["a"] and i in preds["b"] and
             S.canon(preds["a"][i]) == S.canon(preds["b"][i])]
    ok = sum(S.canon(preds["a"][i]) == gold[i] for i in agree)
    print(f"a and b agree on {len(agree)}/100, of those {ok} match the gold; "
          f"{usd:.4f} $")


def cmd_label(a) -> None:
    rows = TP.jl(OUT / "pool.jsonl")
    path = OUT / f"v8-ds-{a.order}.jsonl"
    done = {r["pid"] for r in TP.jl(path)}
    todo = [r for r in rows if r["pid"] not in done]
    print(f"label {a.order}: {len(rows)} pool, {len(done)} done")
    got, usd = label(todo, a.order, a.workers, "pid")
    with open(path, "a") as w:
        for pid, f in got.items():
            w.write(json.dumps({"pid": pid, "frame": f}) + "\n")
    print(f"label {a.order}: {len(got)} new labels, {usd:.3f} $")


def canon(f):
    """TP.canon, None for a malformed teacher frame (</route> unopened,
    a broken finger mask)."""
    try:
        return TP.canon(f)
    except (TypeError, IndexError, ValueError):
        return None


def cmd_merge(a) -> None:
    rows = TP.jl(OUT / "pool.jsonl")
    ra = {r["pid"]: r["frame"] for r in TP.jl(OUT / "v8-ds-a.jsonl")}
    rb = {r["pid"]: r["frame"] for r in TP.jl(OUT / "v8-ds-b.jsonl")}
    edits = {}
    for run in TP.EDITS:
        for e in TP.jl(DATA / "commands" / TP.EDITS[run] / "edits.jsonl"):
            edits[e["id"]] = e
    obj = PL.objects()
    src, dst = DATA / "train" / a.src, DATA / "train" / a.out
    vocab = set()
    for f in src.glob("*.jsonl"):
        for line in open(f):
            vocab |= {t for t in json.loads(line)["frame"]
                      if isinstance(t, str)}
    kept, why = [], {"a+b+edit": 0, "a+b": 0, "disagree": 0, "missing": 0,
                     "unusable": 0}
    for r in rows:
        if r["pid"] not in ra or r["pid"] not in rb:
            why["missing"] += 1
            continue
        ca, cb = canon(ra[r["pid"]]), canon(rb[r["pid"]])
        if ca is None or ca != cb:
            why["disagree"] += 1
            continue
        f = TP.edit_frame(r, edits[r["edit_id"]])
        ef = PL.project(f, r["text"], obj) if f else None
        if ef is not None and TP.canon(ef) == ca:
            frame, k = ef, "a+b+edit"
        else:
            frame = TP.teacher_frame(ra[r["pid"]], r["text"], vocab)
            frame = PL.project(frame, r["text"], obj) if frame else None
            k = "a+b"
        if not frame or TP.canon(frame) == TP.canon([]) or not all(
                isinstance(t, str) or 0 <= t[0] < t[1] <= len(r["text"])
                for t in frame):
            why["unusable"] += 1
            continue
        why[k] += 1
        kept.append({"id": "u" + r["pid"], "edit_id": r["edit_id"],
                     "text": r["text"], "register": r["register"],
                     "lang": r["lang"], "frame": frame, "bio": None,
                     "aug": False, "teach": k})
    print(why, "kept", len(kept))
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(src, dst)
    with open(OUT / "v8-kept.jsonl", "w") as w:
        for r in kept:
            w.write(json.dumps(r, ensure_ascii=False) + "\n")
    rng = random.Random(0)
    for name, times in (("train_cat", a.weight), ("train_all", 1)):
        rows_ = [json.loads(line) for line in open(dst / f"{name}.jsonl")]
        rows_ += kept * times
        rng.shuffle(rows_)
        with open(dst / f"{name}.jsonl", "w") as w:
            for x in rows_:
                w.write(json.dumps(x, ensure_ascii=False) + "\n")
    (OUT / "v8-merge.json").write_text(json.dumps(
        {"src": a.src, "out": a.out, "weight": a.weight, **why,
         "kept": len(kept), "time": time.ctime()}, indent=1))


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["check", "label", "merge", "prompt"])
    ap.add_argument("--order", default="a", choices=["a", "b"])
    ap.add_argument("--workers", type=int, default=16)
    ap.add_argument("--src", default="ds-v6p")
    ap.add_argument("--out", default="ds-v8t")
    ap.add_argument("--weight", type=int, default=2)
    a = ap.parse_args()
    if a.cmd == "prompt":
        print(system_prompt(a.order))
        return
    {"check": cmd_check, "label": cmd_label, "merge": cmd_merge}[a.cmd](a)


if __name__ == "__main__":
    main()
