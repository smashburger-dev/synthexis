# /// script
# requires-python = ">=3.11"
# dependencies = ["httpx", "pyyaml"]
# ///
"""Verbalise structured edits through Muse Spark Open.

  uv run tools/mapping/commands.py --run <name> --edits-run <name>
      --max-usd <cap> [--limit-jobs N] [--workers 12] [--dry-run]

Reads data/mapping/commands/<edits-run>/edits.jsonl, renders jobs of
edits.yaml:edits_per_job edits, validates the [[mark|slot]] output,
and writes sentences.jsonl / drops.jsonl / summary.json into
data/mapping/commands/<run>/.
"""

import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parent))
import spark  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
FINGERS = ["thumb", "index", "middle", "ring", "pinky"]
AMOUNT_WORD = {0.25: "subtle", 0.5: "medium", 0.75: "strong",
               1.0: "full"}
CURVE_WORD = {"exp": "exponential", "log": "logarithmic",
              "s": "s-curve"}
SMOOTH_WORD = {"low": "low (snappy)", "medium": "medium",
               "high": "high (smooth)"}
HOLD_WORD = {"hold": "hold the value", "reset": "reset",
             "freeze": "freeze"}
MODE_WORD = {
    "momentary": "on only while the source is on",
    "toggle": "each time the source fires it flips on or off",
    "step": "each time the source fires, the parameter jumps to its next "
            "setting",
    "ramp": "each time the source fires, the parameter starts moving "
            "slowly; the next time it moves back"}
RE_LINE = re.compile(r"^E(\d+)\.(\d+):\s*(.+)$")
RE_MARK = re.compile(r"\[\[([^\[\]|]+?)\|([a-z]+\d+)\]\]")

JOB_META: dict[str, dict] = {}


def pct(v) -> str:
    return f"{round(v * 100):d} %"


def shape_fingers(onto: dict, shape_id: str) -> str:
    masks = {s["id"]: s["mask"] for s in onto["shapes"]}
    mask = masks.get(shape_id) or (shape_id[1:] if shape_id.startswith("m")
                                   else "")
    on = [FINGERS[i] for i, b in enumerate(mask) if b == "1"]
    return (", ".join(on) + " extended, the others folded") if on else \
        ("" if not mask else "none")


def source_desc(onto: dict, sid: str) -> str:
    for s in onto["sources"]:
        if s["id"] == sid:
            return s["desc"]
    return ""


def quantity(onto: dict, tid: str) -> str:
    for t in onto["taxonomy"]:
        if t["id"] == tid:
            return t["quantity"]
    return ""


def tax_desc(onto: dict, tid: str) -> str:
    """First sentence of the taxonomy desc (rendered to the model)."""
    for t in onto["taxonomy"]:
        if t["id"] == tid:
            return (t.get("desc") or t["quantity"]).split(". ")[0]
    return ""


def preset_desc(onto: dict, pid: str) -> str:
    for p in onto.get("presets", []):
        if p["id"] == pid:
            return p["desc"]
    return ""


def midi_prop_desc(onto: dict, pid: str) -> str:
    for p in onto.get("midi_props", []):
        if p["id"] == pid:
            return p["desc"]
    return ""


def slot_line(sid: str, s: dict, onto: dict) -> str:
    k = s["kind"]
    if k == "source":
        out = f"{sid} source {s['value']}"
        if s.get("side"):
            out += f" side={s['side']}"
        if s.get("object"):
            out += f" object={s['object']}"
        if s.get("shape"):
            out += (f" shape={s['shape']} "
                    f"({shape_fingers(onto, s['shape'])})")
        out += f" ({source_desc(onto, s['value'])})"
        if s.get("say"):
            out += f' say: "{s["say"]}"'
        return out
    if k == "target":
        t = s["target"]
        if t == "param":
            out = (f"{sid} param {s['value']} "
                   f"({tax_desc(onto, s['value'])})")
        elif t == "macro":
            out = f'{sid} macro "{s["value"]}"'
        elif t == "midi.note":
            out = f"{sid} midi note"
        elif t == "midi.pattern":
            out = f'{sid} midi pattern "{s["value"]}"'
        elif t == "midi.chord":
            out = f"{sid} midi chords"
        elif t == "midi.prop":
            out = (f"{sid} midi generator {s['value']} "
                   f"({midi_prop_desc(onto, s['value'])})")
        elif t == "toggle":
            out = f"{sid} toggle {s['value']}"
        elif t == "xy":
            out = (f"{sid} xy pad (the plugin's two-axis pad or its two "
                   f"macros: left-right drives one, up-down the other)")
        else:  # midi.cc
            out = f"{sid} midi cc {s['value']}"
        if s.get("mode"):
            out += f" mode={s['mode']}"
            if s["mode"] in MODE_WORD:
                out += f" ({MODE_WORD[s['mode']]})"
        if s.get("say"):
            out += f' say: "{s["say"]}"'
        return out
    if k == "value":
        return f"{sid} value {s['value']}"
    if k == "track":
        if s["value"] == "ref:last_track":
            return f"{sid} track ref last track"
        return f'{sid} track "{s["value"]}"'
    if k == "plugin":
        return f'{sid} plugin "{s["value"]}"'
    if k == "param_name":
        return f'{sid} param name "{s["value"]}"'
    if k == "invert":
        return f"{sid} inverted"
    if k == "bipolar":
        return (f"{sid} centred (the middle position changes nothing, "
                f"one direction lowers the target, the other raises it)")
    if k == "chord":
        return f"{sid} chord {s['value']}"
    if k == "range_lo":
        return f"{sid} {pct(s['value'])}"
    if k == "range_hi":
        return f"{sid} {pct(s['value'])}"
    if k == "amount":
        return f"{sid} amount {AMOUNT_WORD[s['value']]}"
    if k == "curve":
        return f"{sid} curve {CURVE_WORD[s['value']]}"
    if k == "smooth":
        return f"{sid} smoothing {SMOOTH_WORD[s['value']]}"
    if k == "hold":
        return (f"{sid} when the source disappears: "
                f"{HOLD_WORD[s['value']]}")
    if k == "quantize":
        if s["value"] == "scale":
            return f"{sid} snap to the scale"
        return f"{sid} steps {s['value']}"
    if k == "note":
        return f"{sid} note {s['value']}"
    if k == "side":
        return f"{sid} side {s['value']}"
    if k == "role":
        return f"{sid} role {s['value']}"
    if k == "preset":
        d = preset_desc(onto, s["value"])
        return (f"{sid} preset {s['value']} ({d})" if d
                else f"{sid} preset {s['value']}")
    if k == "key":
        return f"{sid} key {s['value']}"
    if k == "scale":
        return f"{sid} scale {s['value']}"
    if k == "undo":
        return f"{sid} {s['value']}"
    if k == "delta":
        w = {0.1: "a bit more", 0.25: "more", 0.5: "much more",
             -0.1: "a bit less", -0.25: "less",
             -0.5: "much less"}[s["value"]]
        return f"{sid} change {w}"
    if k == "gate":
        out = f"{sid} only while {s['value']}"
        if s.get("shape"):
            out += (f" shape={s['shape']} "
                    f"({shape_fingers(onto, s['shape'])})")
        if s.get("side"):
            out += f" side={s['side']}"
        out += f" ({source_desc(onto, s['value'])})"
        return out
    if k == "time":
        return f"{sid} time {s['value']}"
    if k == "ref":
        return f"{sid} ref last route"
    if k == "rejected":
        return f"{sid} rejected {s['value']} instead of {s['of']}"
    if k == "target_from":
        return (f"{sid} old target param {s['value']} "
                f"({tax_desc(onto, s['value'])})")
    if k == "target_to":
        return (f"{sid} new target param {s['value']} "
                f"({tax_desc(onto, s['value'])})")
    return f"{sid} {k} {s.get('value')}"


def render_edit(k: int, e: dict, onto: dict,
                twin_of: int | None = None) -> str:
    var = " | ".join(
        f"{i + 1} {v['register']},{v['noise']},{v['frame']}"
        for i, v in enumerate(e["variants"]))
    head = f"E{k} lang={e['lang']} variants: {var}"
    if twin_of is not None:
        head += f" twin of E{twin_of}"
    lines = [head, f"  intent {e['intent']}"]
    if e["intent"] == "replace":
        has_s = any(sid.startswith("s") for sid in e["slots"])
        r = "r1: s1 -> from1 => to1" if has_s else "r1: from1 => to1"
        lines.append(f"  {r}")
    else:
        for i, route in enumerate(e["routes"], 1):
            if route.get("ref"):
                line = f"r{i}: {route['ref']}"
            else:
                line = f"r{i}: {', '.join(route['src'])} -> " \
                       f"{', '.join(route['tgt'])}"
            if route.get("scope"):
                line += f" | on {', '.join(route['scope'])}"
            if route.get("mods"):
                line += f" | {' '.join(route['mods'])}"
            lines.append(f"  {line}")
    for sid, s in e["slots"].items():
        lines.append(f"  {slot_line(sid, s, onto)}")
    return "\n".join(lines)


def build_jobs(edits: list, per_job: int, sys_prompt: str):
    jobs = []
    for start in range(0, len(edits), per_job):
        part = edits[start:start + per_job]
        jid = f"j{len(jobs):04d}"
        id_to_k = {e["id"]: i + 1 for i, e in enumerate(part)}
        user = "\n".join(
            render_edit(
                i + 1, e, ONTO,
                twin_of=id_to_k.get(e["id"][:-1])
                if e["id"].endswith("b") else None)
            for i, e in enumerate(part))
        JOB_META[jid] = {"edits": part}
        jobs.append(spark.Job(
            id=jid,
            messages=[
                {"role": "system", "content": sys_prompt},
                {"role": "user", "content": user}],
            est_out_tokens=sum(len(e["variants"]) for e in part) * 50 + 2000))
    return jobs


ONTO: dict = {}
NEGATIVE: dict = {}   # concept -> [distinctive phrase]


def load_negative(gloss_dir: Path) -> None:
    """Distinctive glossary phrases per concept for the extra check."""
    NEGATIVE.clear()
    gj = gloss_dir / "glossary.jsonl"
    if not gj.exists():
        return
    amb = gloss_dir / "ambiguous.json"
    ambiguous = set()
    if amb.exists():
        for a in json.loads(amb.read_text()):
            ambiguous.add((a.get("lang"), a.get("p", "").casefold()))
    phrases: dict[str, set] = {}
    for line in gj.read_text().splitlines():
        if not line.strip():
            continue
        r = json.loads(line)
        phrases.setdefault(r["c"], set()).add(
            (r["lang"], r["p"].casefold()))
    for c, ps in phrases.items():
        kind = c.split(":", 1)[0]
        if kind not in ("source", "param", "family"):
            continue
        keep = []
        for lang, p in ps:
            words = p.split()
            if (len(words) >= 2 or (len(words) == 1
                    and len(words[0]) >= 5))                     and (lang, p) not in ambiguous:
                keep.append((lang, p))
        NEGATIVE[c] = keep


def edit_concepts(e: dict) -> set:
    """source/param/family concepts this edit legitimately mentions."""
    out = set()
    for s in e["slots"].values():
        if s["kind"] == "source":
            out.add(f"source:{s['value']}")
        elif s["kind"] == "gate":
            out.add(f"source:{s['value']}")
        elif s["kind"] == "target":
            if s["target"] == "param":
                out.add(f"param:{s['value']}")
            elif s["target"] == "toggle":
                out.add(f"family:{s['value']}")
        elif s["kind"] == "rejected":
            out.add(f"param:{s['value']}")
            out.add(f"source:{s['value']}")
    return out


def negative_check(e: dict, plain: str, spans: list) -> str | None:
    """extra:<concept> when a distinctive phrase of a concept not in
    the edit shows up outside the marked spans."""
    if not NEGATIVE:
        return None
    own = edit_concepts(e)
    own_phr = set()
    for c in own:
        for _l, p in NEGATIVE.get(c, []):
            own_phr.add(p)
    # blank the marked spans, keep positions
    buf = list(plain.casefold())
    for sp in spans:
        for i in range(sp["start"], sp["end"]):
            buf[i] = " "
    outer = "".join(buf)
    for c, phrases in NEGATIVE.items():
        if c in own:
            continue
        for lang, p in phrases:
            if lang != e["lang"] or p in own_phr:
                continue
            if re.search(r"\b" + re.escape(p) + r"\b", outer):
                return f"extra:{c}"
    return None


SIDE_WORDS = {
    "left": ["left", "lefty", "links", "linke", "linken", "linker",
             "linkes"],
    "right": ["right", "righty", "rechts", "rechte", "rechten",
              "rechter", "rechtes"],
    "both": ["both", "beide", "beiden"],
}


def _edit1(a: str, b: str) -> bool:
    """Levenshtein <= 1."""
    if abs(len(a) - len(b)) > 1:
        return False
    if len(a) == len(b):
        return sum(x != y for x, y in zip(a, b)) <= 1
    if len(a) > len(b):
        a, b = b, a
    i = j = 0
    while i < len(a) and j < len(b):
        if a[i] == b[j]:
            i += 1
        j += 1
    return i == len(a)


def side_check(e: dict, spans: list, plain: str) -> str | None:
    """Source/gate slots with a side need a side word near the mark."""
    if e["lang"] == "de":
        pass
    by_slot = {}
    for sp in spans:
        by_slot.setdefault(sp["slot"], []).append(sp)
    words = plain.casefold().split()
    for sid, s in e["slots"].items():
        if s["kind"] not in ("source", "gate"):
            continue
        side = s.get("side")
        if side not in SIDE_WORDS:
            continue                    # None / either need no word
        wanted = SIDE_WORDS[side]
        ok = False
        for sp in by_slot.get(sid, []):
            # words overlapping the span + the 4 words before it
            region = plain[:sp["end"]].casefold().split()[-7:]
            region += [w for w in plain[sp["start"]:sp["end"]]
                       .casefold().split()]
            for w in region:
                w = w.strip('.,;:!?()[]"\'')
                if w in wanted or (len(w) >= 5 and any(
                        _edit1(w, x) for x in wanted)):
                    ok = True
                    break
            if ok:
                break
        if not ok:
            return f"missing side ({sid})"
    return None


def validate_variant(e: dict, v: int, raw: str):
    """-> (ok, reason, sentence_dict)."""
    slot_ids = set(e["slots"])
    # a chord target whose route names its chord is said by the chord name
    named = {t for r in e.get("routes", []) for t in r["tgt"]
             if any(e["slots"][m]["kind"] == "chord" for m in r["mods"])}
    optional = {s for s in slot_ids if re.fullmatch(r"(i|bp)\d+", s)} | named
    required = slot_ids - optional
    marks = list(RE_MARK.finditer(raw))
    if e["intent"] == "none" and marks:
        return False, "marks on none intent", None
    plain_parts, spans, cursor = [], [], 0
    for m in marks:
        plain_parts.append(raw[cursor:m.start()])
        inner = m.group(1)
        start = sum(len(p) for p in plain_parts)
        spans.append({"start": start, "end": start + len(inner),
                      "slot": m.group(2), "text": inner})
        plain_parts.append(inner)
        cursor = m.end()
    plain_parts.append(raw[cursor:])
    plain = "".join(plain_parts)
    if "[[" in plain or "]]" in plain or "|" in plain:
        return False, "leftover mark", None
    counts = Counter(m.group(2) for m in marks)
    for sid in counts:
        if sid not in slot_ids:
            return False, f"unknown slot {sid}", None
        if sid in optional:
            if counts[sid] > 1:
                return False, f"{sid} marked twice", None
        elif re.fullmatch(r"[kpn]\d+", sid):
            pass                    # track/plugin/name may repeat
        elif counts[sid] != 1:
            return False, f"{sid} marked {counts[sid]}x", None
    for sid in sorted(required):
        if counts.get(sid, 0) == 0:
            return False, f"missing {sid}", None
    if not (3 <= len(plain) <= 400):
        return False, f"text length {len(plain)}", None
    sc = side_check(e, spans, plain)
    if sc:
        return False, sc, None
    var = e["variants"][v - 1]
    return True, None, {
        "id": f"{e['id']}.{v}", "edit_id": e["id"], "lang": e["lang"],
        "register": var["register"], "noise": var["noise"],
        "intent": e["intent"], "text": plain, "spans": spans,
        "edit": e}


def make_parse():
    def parse(job: spark.Job, text: str):
        edits = JOB_META[job.id]["edits"]
        requested = sum(len(e["variants"]) for e in edits)
        lines: dict[tuple, str] = {}
        for line in text.splitlines():
            m = RE_LINE.match(line.strip())
            if m:
                lines.setdefault(
                    (int(m.group(1)), int(m.group(2))), m.group(3))
        sentences, drops = [], []
        seen = set()
        for k, e in enumerate(edits, 1):
            for v in range(1, len(e["variants"]) + 1):
                raw = lines.get((k, v))
                if raw is None:
                    drops.append({"edit_id": e["id"], "v": v,
                                  "reason": "absent", "raw": None})
                    continue
                if (k, v) in seen:
                    drops.append({"edit_id": e["id"], "v": v,
                                  "reason": "duplicate", "raw": raw})
                    continue
                seen.add((k, v))
                ok, reason, sent = validate_variant(e, v, raw)
                if ok:
                    neg = negative_check(e, sent["text"],
                                         sent["spans"])
                    if neg:
                        drops.append({"edit_id": e["id"], "v": v,
                                      "reason": neg, "raw": raw})
                        continue
                    sentences.append(sent)
                else:
                    drops.append({"edit_id": e["id"], "v": v,
                                  "reason": reason, "raw": raw})
        for (k, v), raw in lines.items():
            if (k, v) in seen:
                continue
            eid = edits[k - 1]["id"] if k <= len(edits) else f"E{k}"
            drops.append({"edit_id": eid, "v": v,
                          "reason": "variant out of range",
                          "raw": raw})
        if len(sentences) < requested / 2:
            raise spark.ParseError(
                f"{len(sentences)}/{requested} valid variants")
        return {"sentences": sentences, "drops": drops}
    return parse


def main() -> None:
    global ONTO
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--edits-run", required=True)
    ap.add_argument("--max-usd", type=float, required=True)
    ap.add_argument("--limit-jobs", type=int, default=0)
    ap.add_argument("--workers", type=int, default=12)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--reasoning-effort", default="low")
    args = ap.parse_args()

    ONTO = yaml.safe_load((HERE / "ontology.yaml").read_text())
    cfg = yaml.safe_load((HERE / "edits.yaml").read_text())
    edits_path = (ROOT / "data" / "mapping" / "commands"
                  / args.edits_run / "edits.jsonl")
    edits = [json.loads(l) for l in edits_path.read_text().splitlines()
             if l.strip()]
    sys_prompt = (HERE / "prompts" / "commands.md").read_text()
    jobs = build_jobs(edits, cfg["edits_per_job"], sys_prompt)
    if args.limit_jobs:
        jobs = jobs[: args.limit_jobs]
    est = sum(spark._estimate_usd(j) for j in jobs)
    n_var = sum(len(e["variants"]) for j in jobs
                for e in JOB_META[j.id]["edits"])
    print(f"{len(edits)} edits -> {len(jobs)} jobs "
          f"({n_var} variants), estimate ${est:.3f}")

    out_dir = ROOT / "data" / "mapping" / "commands" / args.run
    out_dir.mkdir(parents=True, exist_ok=True)
    load_negative(ROOT / "data" / "mapping" / "glossary" /
                  "gloss-v1")
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
        workers=args.workers, temperature=0.9,
        reasoning_effort=args.reasoning_effort)

    n_sent = 0
    drop_reasons = Counter()
    by_intent, by_lang = Counter(), Counter()
    with (out_dir / "sentences.jsonl").open("w") as sf, \
            (out_dir / "drops.jsonl").open("w") as df:
        for jid, res in parsed.items():
            for s in res["sentences"]:
                sf.write(json.dumps(s, ensure_ascii=False) + "\n")
                n_sent += 1
                by_intent[s["intent"]] += 1
                by_lang[s["lang"]] += 1
            for d in res["drops"]:
                df.write(json.dumps(d, ensure_ascii=False) + "\n")
                drop_reasons[d["reason"]] += 1
    total_req = sum(
        len(e["variants"]) for j in jobs for e in JOB_META[j.id]["edits"])
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
    summary = {
        "variants_requested": total_req,
        "sentences": n_sent,
        "valid_rate": round(n_sent / total_req, 3) if total_req else 0,
        "drops_by_reason": dict(drop_reasons.most_common()),
        "by_intent": dict(by_intent.most_common()),
        "by_lang": dict(by_lang.most_common()),
        "spend_usd": round(spend, 4),
    }
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=1))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
