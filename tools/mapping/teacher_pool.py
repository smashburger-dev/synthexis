# /// script
# requires-python = ">=3.11"
# dependencies = ["pyyaml", "httpx>=0.27"]
# ///
"""Teacher data from the sentences the command filter dropped.

  uv run tools/mapping/teacher_pool.py pool  [--runs cmd-s2,cmd-s3,cmd-s4b,cmd-s5]
  uv run tools/mapping/teacher_pool.py ds    [--workers 16]
  SYNTHEXIS_BASETEN_CAP=75 uv run tools/mapping/teacher_pool.py spark --max-usd 5.5
  uv run tools/mapping/teacher_pool.py merge [--src ds-v6p] [--out ds-v7p] [--weight 2]

The command batches dropped about a third of Spark's sentences, mostly
because a slot was not marked: the sentence said "get really bright"
instead of naming the tone knob, or left the side unsaid. Those are the
sentences that sound like people. Each one has three readings:
  edit      what the generator asked for (the edit, projected on the text)
  ds        DeepSeek V4.1 Flash parsing the plain text (spark_parse prompt)
  spark     Muse Spark Open, only where edit and ds disagree, within budget
A sentence is kept when two readings agree on the route multiset
(probes/real-eval/score.py canon). Its frame is the edit's when the edit
is in the majority (exact parameter ids and pointer spans), else
Spark's. Pool rows get no BIO labels (bio null: ignored in the loss).
Dev edits (dataset.py is_dev, seed 13) and near-copies of the real set
(word 6-gram or ratio >= 0.7, as probes/real-eval/leak.py) are left out.
DeepSeek only sees these synthetic sentences.
Files under data/mapping/teach/; merge writes <out> = <src> with the kept
rows appended to train_cat (x weight) and train_all.
"""

import argparse
import difflib
import json
import random
import re
import shutil
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(ROOT / "probes" / "real-eval"))
import dataset as D  # noqa: E402
import project_labels as PL  # noqa: E402
import score as S  # noqa: E402
import spark  # noqa: E402
import spark_parse as P  # noqa: E402
import teacher_check as TC  # noqa: E402

DATA = ROOT / "data" / "mapping"
OUT = DATA / "teach"
RE_MARK = re.compile(r"\[\[([^\[\]|]+?)\|([a-z]+\d+)\]\]")
EDITS = {"cmd-s2": "edits-s2", "cmd-s3": "edits-s3", "cmd-s4b": "edits-s4b",
         "cmd-s5": "edits-s5"}
PRIORITY = ["cmd-s5", "cmd-s4b", "cmd-s3", "cmd-s2"]


def jl(p: Path) -> list:
    return [json.loads(line) for line in open(p)] if p.exists() else []


def plain(raw: str) -> tuple[str, list]:
    """Strip [[text|slot]] marks -> (text, spans)."""
    parts, spans, cur = [], [], 0
    for m in RE_MARK.finditer(raw):
        parts.append(raw[cur:m.start()])
        start = sum(len(p) for p in parts)
        spans.append({"start": start, "end": start + len(m.group(1)),
                      "slot": m.group(2)})
        parts.append(m.group(1))
        cur = m.end()
    parts.append(raw[cur:])
    text = "".join(parts)
    cut = len(text) - len(text.lstrip())
    for sp in spans:                    # spans follow the stripped text
        sp["start"] -= cut
        sp["end"] -= cut
    return text.strip(), spans


def cmd_pool(a) -> None:
    ev = [re.findall(r"\w+", json.loads(line)["text"].casefold())
          for line in open(DATA / "eval" / "expanded.jsonl")]
    evg = set()
    for w in ev:
        evg |= {" ".join(w[i:i + 6]) for i in range(len(w) - 5)}
    seen, rows, stats = set(), [], {}
    for run in a.runs.split(","):
        edits = {e["id"]: e for e in jl(DATA / "commands" / EDITS[run]
                                         / "edits.jsonl")}
        st = {"dropped": 0, "dev": 0, "leak": 0, "dup": 0, "kept": 0}
        for d in jl(DATA / "commands" / run / "drops.jsonl"):
            st["dropped"] += 1
            e = edits.get(d["edit_id"])
            if e is None or not d.get("raw") or \
                    not 1 <= d["v"] <= len(e["variants"]):
                continue
            if D.is_dev(d["edit_id"], 13):
                st["dev"] += 1
                continue
            text, spans = plain(d["raw"])
            if not 3 <= len(text) <= 400 or text.casefold() in seen or \
                    "|" in text:            # leftover slot id: not a sentence
                st["dup"] += 1
                continue
            w = re.findall(r"\w+", text.casefold())
            sm = difflib.SequenceMatcher(None, b=w)
            near = {" ".join(w[i:i + 6]) for i in range(len(w) - 5)} & evg
            for e2 in ev:
                if near:
                    break
                sm.set_seq1(e2)
                if sm.real_quick_ratio() >= 0.7 and sm.quick_ratio() >= 0.7 \
                        and sm.ratio() >= 0.7:
                    near = True
            if near:
                st["leak"] += 1
                continue
            seen.add(text.casefold())
            rows.append({"pid": f"{d['edit_id']}.{d['v']}", "run": run,
                         "edit_id": d["edit_id"], "reason": d["reason"],
                         "lang": e["lang"],
                         "register": e["variants"][d["v"] - 1]["register"],
                         "text": text, "spans": spans})
            st["kept"] += 1
        stats[run] = st
        print(run, st)
    OUT.mkdir(parents=True, exist_ok=True)
    with open(OUT / "pool.jsonl", "w") as w:
        for r in rows:
            w.write(json.dumps(r, ensure_ascii=False) + "\n")
    (OUT / "pool_stats.json").write_text(json.dumps(stats, indent=1))


def chunks(rows: list) -> list:
    return [rows[i:i + P.JOB_SIZE] for i in range(0, len(rows), P.JOB_SIZE)]


def messages(sysmsg: str, chunk: list) -> list:
    return [{"role": "system", "content": sysmsg},
            {"role": "user", "content": "\n".join(
                f"{r['pid']} {r['text']}" for r in chunk)}]


def cmd_ds(a) -> None:
    rows = jl(OUT / "pool.jsonl")
    done = {r["pid"] for r in jl(OUT / "ds.jsonl")}
    todo = [r for r in rows if r["pid"] not in done]
    sysmsg, k = P.system_prompt(), TC.key()
    cs = chunks(todo)
    print(f"ds: {len(rows)} pool, {len(done)} done, {len(cs)} jobs")
    spent, n_ok = [0.0], [0]
    out = open(OUT / "ds.jsonl", "a")

    def one(c):
        ids = {r["pid"] for r in c}
        for _ in range(2):
            text, usd = TC.deepseek(messages(sysmsg, c), False, k)
            spent[0] += usd
            try:
                got = P.make_parse({"x": ids})(spark.Job("x", [], 0), text)
                break
            except spark.ParseError:
                got = {}
        for pid, f in got.items():
            out.write(json.dumps({"pid": pid, "frame": f}) + "\n")
        out.flush()
        n_ok[0] += 1
        if n_ok[0] % 50 == 0:
            print(f"  {n_ok[0]}/{len(cs)} jobs, {spent[0]:.3f} $", flush=True)

    with ThreadPoolExecutor(a.workers) as ex:
        list(ex.map(one, cs))
    out.close()
    print(f"ds done: {len(jl(OUT / 'ds.jsonl'))} labels, {spent[0]:.3f} $")


def edit_frame(r: dict, e: dict):
    """The edit's frame on this text, pointer slots located by their value."""
    spans = [s for s in r["spans"] if s["slot"] in e["slots"]]
    marked = {s["slot"] for s in spans}
    low = r["text"].casefold()
    for sid, s in e["slots"].items():
        if s["kind"] in D.PTR_KIND and sid not in marked:
            v = str(s.get("value") or "").casefold()
            i = low.find(v) if v else -1
            if i < 0:
                return None
            spans.append({"start": i, "end": i + len(v), "slot": sid})
    frame, _ = D.build(r["text"], spans, e)
    return frame


def teacher_frame(toks: list, text: str, vocab: set):
    """Teacher tokens -> dataset frame; '=word' pointers located in text."""
    out, low = [], text.casefold()
    for t in toks:
        if t.startswith("="):
            w = t[1:].replace("_", " ").casefold()
            i = low.find(w)
            if i < 0:
                if out and out[-1] in fp_kinds():
                    out.pop()
                continue
            out.append([i, i + len(w)])
        elif t in vocab:
            out.append(t)
        elif t.startswith("T:"):
            return None                 # unknown target id
    return out


def fp_kinds() -> tuple:
    return ("TRACK", "PLUGIN", "PNAME", "NOTE", "TIME", "CHORD", "VALUE")


def readings() -> tuple:
    rows = jl(OUT / "pool.jsonl")
    ds = {r["pid"]: r["frame"] for r in jl(OUT / "ds.jsonl")}
    edits = {}
    for run in EDITS:
        for e in jl(DATA / "commands" / EDITS[run] / "edits.jsonl"):
            edits[e["id"]] = e
    obj = PL.objects()
    for r in rows:
        f = edit_frame(r, edits[r["edit_id"]])
        r["edit"] = PL.project(f, r["text"], obj) if f else None
        r["ds"] = ds.get(r["pid"])
    return rows, obj


def canon(f) -> object:
    return None if f is None else S.canon([t for t in f if isinstance(t, str)])


def cmd_spark(a) -> None:
    rows, _ = readings()
    need = [r for r in rows if r["ds"] is not None
            and canon(r["edit"]) != canon(r["ds"])]
    need.sort(key=lambda r: PRIORITY.index(r["run"]))
    print(f"spark: {len(need)} rows where edit and ds disagree")
    sysmsg = P.system_prompt()
    jobs, ids = [], {}
    for i, c in enumerate(chunks(need)):
        jid = f"tp-{i:04d}"
        ids[jid] = {r["pid"] for r in c}
        jobs.append(spark.Job(jid, messages(sysmsg, c),
                              est_out_tokens=60 * len(c) + 4000))
    per_job = 0.047                       # measured: teacher_check, high
    jobs = jobs[:int(a.max_usd / per_job)]
    print(f"spark: sending {len(jobs)} jobs ({len(jobs) * P.JOB_SIZE} rows)")
    try:
        out = spark.run("teach-v7", jobs, P.make_parse(ids),
                        max_usd=a.max_usd, workers=8, pilot=2,
                        reasoning_effort="high")
    except spark.AbortError as e:
        print("spark stopped:", e)
        out = spark.run("teach-v7", [], P.make_parse(ids),
                        max_usd=a.max_usd)
    with open(OUT / "spark.jsonl", "w") as w:
        for p in out.values():
            for pid, f in p.items():
                w.write(json.dumps({"pid": pid, "frame": f}) + "\n")
    print(f"spark: {sum(len(p) for p in out.values())} labels")


def cmd_merge(a) -> None:
    rows, obj = readings()
    sp = {r["pid"]: r["frame"] for r in jl(OUT / "spark.jsonl")}
    src, dst = DATA / "train" / a.src, DATA / "train" / a.out
    vocab = set()
    for f in src.glob("*.jsonl"):
        for line in open(f):
            vocab |= {t for t in json.loads(line)["frame"]
                      if isinstance(t, str)}
    kept, why = [], {"edit+ds": 0, "edit+spark": 0, "ds+spark": 0,
                     "no majority": 0, "no ds": 0, "unusable": 0}
    for r in rows:
        if r["ds"] is None:
            why["no ds"] += 1
            continue
        ce, cd = canon(r["edit"]), canon(r["ds"])
        s = sp.get(r["pid"])
        cs = canon(s) if s is not None else None
        frame = None
        if ce is not None and ce == cd:
            frame, k = r["edit"], "edit+ds"
        elif ce is not None and cs is not None and ce == cs:
            frame, k = r["edit"], "edit+spark"
        elif cs is not None and cs == cd:
            frame = teacher_frame(s, r["text"], vocab)
            frame = PL.project(frame, r["text"], obj) if frame else None
            k = "ds+spark"
        else:
            why["no majority"] += 1
            continue
        if not frame or canon(frame) == canon([]) or not all(
                isinstance(t, str) or 0 <= t[0] < t[1] <= len(r["text"])
                for t in frame):
            why["unusable"] += 1
            continue
        why[k] += 1
        kept.append({"id": "t" + r["pid"], "edit_id": r["edit_id"],
                     "text": r["text"], "register": r["register"],
                     "lang": r["lang"], "frame": frame, "bio": None,
                     "aug": False, "teach": k})
    print(why, "kept", len(kept))
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(src, dst)
    with open(OUT / "kept.jsonl", "w") as w:
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
    (OUT / "merge.json").write_text(json.dumps(
        {"src": a.src, "out": a.out, "weight": a.weight, **why,
         "kept": len(kept), "time": time.ctime()}, indent=1))


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["pool", "ds", "spark", "merge"])
    ap.add_argument("--runs", default="cmd-s2,cmd-s3,cmd-s4b,cmd-s5")
    ap.add_argument("--workers", type=int, default=16)
    ap.add_argument("--max-usd", type=float, default=5.5)
    ap.add_argument("--src", default="ds-v6p")
    ap.add_argument("--out", default="ds-v7p")
    ap.add_argument("--weight", type=int, default=2)
    a = ap.parse_args()
    {"pool": cmd_pool, "ds": cmd_ds, "spark": cmd_spark,
     "merge": cmd_merge}[a.cmd](a)


if __name__ == "__main__":
    main()
