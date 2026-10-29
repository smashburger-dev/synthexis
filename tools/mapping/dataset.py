# /// script
# requires-python = ">=3.11"
# dependencies = ["pyyaml"]
# ///
"""Parser datasets: frames, BIO spans, dev split, glossary augmentation.

  uv run tools/mapping/dataset.py [--out ds-v1] [--seed 13]

Reads cmd-s1 sentences, the rt-s1-all round-trip, kw-s1 keyword sentences,
gloss-v1 and the ch-s1 challenge set from data/mapping/. Writes
data/mapping/train/<out>/{train_filtered,train_all,dev,challenge}.jsonl
and counts.json. Deterministic for a given seed.

Record: {id, edit_id, text, frame, bio, aug}. frame is a list of closed
tokens and pointer items [start_char, end_char] (span pointers, see
DESIGN.md "Training interfaces"); bio is a list of [start, end, tag].
Dev = 10 % of base edit ids (twin "b" edits stay with the original),
built from clean sentences only.
"""

import argparse
import hashlib
import json
import random
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import commands  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "data" / "mapping"
INF = 1 << 30

BIO_TAG = {
    "source": "SRC", "target": "TGT", "target_from": "TGT",
    "target_to": "TGT", "track": "TRACK", "plugin": "PLUGIN",
    "param_name": "PNAME", "range_lo": "NUM", "range_hi": "NUM",
    "amount": "AMT", "curve": "MOD", "smooth": "MOD", "hold": "MOD",
    "quantize": "MOD", "invert": "MOD", "note": "NOTE", "gate": "GATE",
    "time": "TIME", "ref": "REF", "rejected": "REJ", "role": "ROLE",
    "side": "ROLE", "preset": "PRESET", "key": "KEY", "scale": "SCALE",
    "undo": "UNDO", "delta": "DELTA"}
PTR_KIND = {"plugin": "PLUGIN", "param_name": "PNAME", "note": "NOTE",
            "time": "TIME", "track": "TRACK"}


def num(v) -> str:
    return f"{float(v):g}"


def target_tok(s: dict) -> str:
    v = s.get("value")
    if s["target"] == "macro" and v:
        v = re.sub(r"\D", "", str(v)) or v
    return f"T:{s['target']}:{'' if v is None else v}"


def slot_tokens(s: dict, span) -> list | None:
    """Closed tokens plus pointer items for one slot; None = unusable."""
    k = s["kind"]
    if k == "source":
        out = ["S:" + s["value"]]
        for a in ("side", "object", "shape"):
            if s.get(a):
                out.append(f"{a}:{s[a]}")
        return out
    if k == "target":
        return [target_tok(s)] + ([f"mode:{s['mode']}"]
                                  if s.get("mode") else [])
    if k in ("target_from", "target_to"):
        return ["FROM" if k == "target_from" else "TO", target_tok(s)]
    if k == "track" and str(s["value"]).startswith("ref:"):
        return ["TRACK:ref"]
    if k in PTR_KIND:
        return None if span is None else [PTR_KIND[k], [span[0], span[1]]]
    if k == "gate":
        out = ["G:" + s["value"]]
        for a in ("side", "shape"):
            if s.get(a):
                out.append(f"{a}:{s[a]}")
        return out
    simple = {"invert": lambda v: "INV", "ref": lambda v: "REF",
              "range_lo": lambda v: "lo:" + num(v),
              "range_hi": lambda v: "hi:" + num(v),
              "amount": lambda v: "amt:" + num(v),
              "delta": lambda v: f"delta:{float(v):+g}",
              "role": lambda v: f"role:{v or 'none'}"}
    if k in simple:
        return [simple[k](s["value"])]
    return [f"{k}:{s['value']}"]   # curve smooth hold quantize side ...


def build(text: str, spans: list, e: dict):
    """-> (frame, bio) or (None, reason)."""
    first = {}
    for sp in spans:
        if sp["slot"] not in first or sp["start"] < first[sp["slot"]][0]:
            first[sp["slot"]] = (sp["start"], sp["end"])
    slots = e["slots"]
    kinds = list(BIO_TAG)

    def key(sid):
        return (first.get(sid, (INF,))[0], kinds.index(slots[sid]["kind"]))

    def items(sids):
        out = []
        for sid in sorted(sids, key=key):
            t = slot_tokens(slots[sid], first.get(sid))
            if t is None:
                return None
            out += t
        return out

    frame = [f"I:{e['intent']}"]
    used = set()
    routes = []
    for r in e["routes"]:
        sids = ([r["ref"]] if r.get("ref") else []) + r["src"] + r["tgt"] \
            + r["scope"] + r["mods"]
        used.update(sids)
        routes.append((min((key(s) for s in sids), default=(INF, 0)), sids))
    for _, sids in sorted(routes, key=lambda x: x[0]):
        body = items(sids)
        if body is None:
            return None, "unspanned pointer slot"
        frame += ["<route>"] + body + ["</route>"]
    rest = [s for s in slots if s not in used
            and slots[s]["kind"] != "rejected"]
    body = items(rest)
    if body is None:
        return None, "unspanned pointer slot"
    frame += body
    if e["payload"].get("scope"):
        frame.append("scope:" + e["payload"]["scope"])
    bio = [[sp["start"], sp["end"], BIO_TAG[slots[sp["slot"]]["kind"]]]
           for sp in spans if sp["slot"] in slots]
    return frame, bio


def is_dev(edit_id: str, seed: int) -> bool:
    base = edit_id.rstrip("b")
    h = hashlib.sha1(f"{seed}:{base}".encode()).hexdigest()
    return int(h[:8], 16) % 10 == 0


def load_jsonl(p: Path) -> list:
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()]


def gloss_index() -> dict:
    """(lang, concept) -> noun phrases with polarity +1."""
    idx = defaultdict(list)
    for r in load_jsonl(DATA / "glossary" / "gloss-v1" / "glossary.jsonl"):
        if r["pol"] == 1 and r["f"] == "n" and "|" not in r["p"] \
                and "[" not in r["p"]:
            idx[(r["lang"], r["c"])].append(r["p"])
    return idx


def concept(s: dict) -> str | None:
    if s["kind"] == "source" and not (s.get("side") or s.get("object")
                                       or s.get("shape")):
        return "source:" + s["value"]
    if s["kind"] in ("target", "target_from", "target_to") \
            and s["target"] == "param":
        return "param:" + s["value"]
    return None


def augment(rec: dict, gidx: dict, rng: random.Random):
    """Span substitution with same-concept, same-polarity glossary noun
    phrases (at most two spans), re-validated with the side check."""
    e, text, spans = rec["edit"], rec["text"], rec["spans"]
    cand = []
    for i, sp in enumerate(spans):
        s = e["slots"].get(sp["slot"])
        c = s and concept(s)
        if not c or len(sp["text"].split()) > 4:
            continue
        opts = [p for p in gidx.get((rec["lang"], c), [])
                if p.lower() != sp["text"].lower()]
        if opts:
            cand.append((i, opts))
    if not cand:
        return None
    pick = dict(rng.sample(cand, min(2, len(cand))))
    new_spans, parts, cursor, shift = [], [], 0, 0
    for i, sp in sorted(enumerate(spans), key=lambda x: x[1]["start"]):
        if sp["start"] < cursor:
            return None             # overlapping marks: skip
        parts.append(text[cursor:sp["start"]])
        word = rng.choice(pick[i]) if i in pick else sp["text"]
        start = sp["start"] + shift
        new_spans.append({**sp, "start": start, "end": start + len(word),
                          "text": word})
        shift += len(word) - (sp["end"] - sp["start"])
        parts.append(word)
        cursor = sp["end"]
    parts.append(text[cursor:])
    new = "".join(parts)
    if rng.random() < 0.3:
        new = new.lower()
        new_spans = [{**s, "text": new[s["start"]:s["end"]]}
                     for s in new_spans]
    # Text outside the marks is unchanged (casing aside), so the negative
    # check of the original sentence still holds; re-run the side check.
    if commands.side_check(e, new_spans, new):
        return None
    return {**rec, "id": rec["id"] + ".aug", "text": new,
            "spans": new_spans, "aug": True}


def to_record(r: dict) -> tuple:
    frame, bio = build(r["text"], r["spans"], r["edit"])
    if frame is None:
        return None, bio
    return {"id": r["id"], "edit_id": r["edit_id"], "text": r["text"],
            "register": r.get("register"), "lang": r.get("lang"),
            "frame": frame, "bio": bio, "aug": r.get("aug", False)}, None


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="ds-v1")
    ap.add_argument("--seed", type=int, default=13)
    args = ap.parse_args()
    rng = random.Random(args.seed)

    cmd = load_jsonl(DATA / "commands" / "cmd-s1" / "sentences.jsonl")
    kw = load_jsonl(DATA / "commands" / "kw-s1" / "sentences.jsonl")
    ch = load_jsonl(DATA / "commands" / "ch-s1" / "sentences.jsonl")
    rt = {r["id"]: r["mismatch"] for r in load_jsonl(
        DATA / "roundtrip" / "rt-s1-all" / "roundtrip.jsonl")}

    def clean(r):   # keep targets-only mismatches (mostly lay wording)
        m = rt.get(r["id"])
        return m is not None and all(
            re.fullmatch(r"r\d+\.targets", k) for k in m)

    gidx = gloss_index()
    counts, drops = Counter(), Counter()
    sets = defaultdict(list)
    for r in cmd + kw:
        kwd = r["register"] == "keyword"
        dev = is_dev(r["edit_id"], args.seed)
        ok = kwd or clean(r)
        names = (["dev"] if ok else []) if dev else \
            (["train_all", "train_filtered"] if ok else ["train_all"])
        if not names:
            continue
        recs = [r]
        if not dev:
            a = augment(r, gidx, rng)
            if a:
                recs.append(a)
        for x in recs:
            rec, why = to_record(x)
            if rec is None:
                drops[why] += 1
                continue
            for n in names:
                sets[n].append(rec)
    for r in ch:
        rec, why = to_record(r)
        if rec is None:
            drops["challenge " + why] += 1
            continue
        sets["challenge"].append(rec)

    out = DATA / "train" / args.out
    out.mkdir(parents=True, exist_ok=True)
    for n, recs in sets.items():
        with (out / f"{n}.jsonl").open("w") as f:
            for rec in recs:
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        counts[n] = len(recs)
        counts[n + "_aug"] = sum(r["aug"] for r in recs)
        counts[n + "_edits"] = len({r["edit_id"].rstrip("b") for r in recs})
    lens = [len(r["frame"]) for r in sets["train_all"]]
    summary = {"seed": args.seed, "counts": dict(counts),
               "drops": dict(drops),
               "frame_len_max": max(lens),
               "frame_len_mean": round(sum(lens) / len(lens), 1)}
    (out / "counts.json").write_text(json.dumps(summary, indent=1))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
