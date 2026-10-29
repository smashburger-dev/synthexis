# /// script
# requires-python = ">=3.11"
# dependencies = ["pyyaml"]
# ///
"""Template keyword sentences for eligible map edits (no LLM).

Joins glossary noun phrases per keyword_template in edits.yaml:
order per `orders`, separator per route from `separators`,
50 % lowercase, exact spans for every slot.

  uv run tools/mapping/keywords.py --run <name>
      --edits-run edits-v2-pilot --glossary-run gloss-v1
"""

import argparse
import json
import random
import sys
from collections import Counter
from pathlib import Path

import yaml

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
ROOT = HERE.parents[1]

MOD_OK = {"invert", "range_lo", "range_hi"}
SEP_DEFAULT = ", "


def load_phrases(gloss_dir: Path, lang: str):
    """(concept) -> [noun, positive, no-typo phrases]."""
    out = {}
    gj = gloss_dir / "glossary.jsonl"
    if not gj.exists():
        return out
    for line in gj.read_text().splitlines():
        if not line.strip():
            continue
        r = json.loads(line)
        if (r["lang"] == lang and r.get("f") == "n"
                and r.get("pol") == 1 and not r.get("typo")):
            out.setdefault(r["c"], []).append(r["p"])
    return out


def eligible(e: dict) -> bool:
    if e["intent"] != "map" or len(e["routes"]) > 2:
        return False
    for r in e["routes"]:
        if r.get("ref"):
            return False
        for m in r["mods"]:
            if e["slots"][m]["kind"] not in MOD_OK:
                return False
        for t in r["tgt"]:
            if e["slots"][t].get("target") not in ("param", "macro"):
                return False
    return True


def slot_phrase(sid, s, e, phrases, rng):
    """(text, kind) — None when the slot can't be templated."""
    if s["kind"] == "source":
        ph = phrases.get(f"source:{s['value']}") or [s["value"]]
        text = rng.choice(ph)
        side = s.get("side")
        if side:
            side_ph = phrases.get(f"side:{side}") or [side]
            text = f"{rng.choice(side_ph)} {text}"
        return text
    if s["kind"] == "target":
        if s["target"] == "param":
            ph = phrases.get(f"param:{s['value']}") or [s["value"]]
            return rng.choice(ph)
        return str(s["value"])              # macro "Signal N"
    if s["kind"] in ("track", "plugin", "param_name"):
        return str(s["value"])
    if s["kind"] == "range_lo":
        return f"{int(round(s['value'] * 100))}%"
    if s["kind"] == "range_hi":
        return f"{int(round(s['value'] * 100))}%"
    return None


def build_sentence(e: dict, phrases: dict, rng) -> dict | None:
    """Returns a sentences.jsonl-style row or None."""
    cfg = yaml.safe_load((HERE / "edits.yaml").read_text())
    kt = cfg["keyword_template"]
    order = rng.choices(
        list(kt["orders"]), weights=list(kt["orders"].values()))[0]
    seps = kt["separators"]
    clauses = []
    spans: list[dict] = []
    for r in e["routes"]:
        parts = {}
        for sid in r["src"]:
            parts[sid] = slot_phrase(sid, e["slots"][sid], e,
                                     phrases, rng)
        for sid in r["tgt"]:
            parts[sid] = slot_phrase(sid, e["slots"][sid], e,
                                     phrases, rng)
        for sid in r["scope"]:
            parts[sid] = slot_phrase(sid, e["slots"][sid], e,
                                     phrases, rng)
        if any(v is None for v in parts.values()):
            continue
        src_txt = " ".join(parts[s] for s in r["src"])
        tgt_txt = " ".join(parts[s] for s in r["tgt"])
        trk_txt = " ".join(parts[s] for s in r["scope"])
        ordered = {"src_tgt_track": [src_txt, tgt_txt, trk_txt],
                   "track_tgt_src": [trk_txt, tgt_txt, src_txt],
                   "tgt_src_track": [tgt_txt, src_txt, trk_txt],
                   "src_track_tgt": [src_txt, trk_txt, tgt_txt]}
        words = [w for w in ordered[order] if w]
        sep = rng.choice(seps)
        clause = sep.join(words)
        # range modifier appended inside the clause
        rng_mods = [e["slots"][m] for m in r["mods"]
                    if e["slots"][m]["kind"] in ("range_lo",
                                                 "range_hi")]
        if rng_mods:
            lo = next((m for m in rng_mods
                       if m["kind"] == "range_lo"), None)
            hi = next((m for m in rng_mods
                       if m["kind"] == "range_hi"), None)
            form = rng.choice(kt["range_forms"])
            lo_pct = int(round(lo["value"] * 100)) if lo else 0
            hi_pct = int(round(hi["value"] * 100)) if hi else 100
            clause += " " + form.format(lo=lo_pct, hi=hi_pct)
        clauses.append(clause)
        # spans: find each slot phrase inside the clause text
        base = sum(len(c) + len(SEP_DEFAULT)
                   for c in clauses[:-1])
        for sid, txt in parts.items():
            if txt:
                i = clause.find(txt)
                if i >= 0:
                    spans.append({"start": base + i,
                                  "end": base + i + len(txt),
                                  "slot": sid, "text": txt})
    if not clauses:
        return None
    text = SEP_DEFAULT.join(clauses)
    if rng.random() < 0.5:
        text = text.lower()
        for sp in spans:
            sp["text"] = sp["text"].lower()
    return {"id": f"{e['id']}.kw", "edit_id": e["id"],
            "lang": e["lang"], "register": "keyword",
            "noise": "template", "intent": e["intent"],
            "text": text, "spans": spans, "edit": e}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--edits-run", required=True)
    ap.add_argument("--glossary-run", required=True)
    ap.add_argument("--seed", type=int, default=7)
    args = ap.parse_args()

    cfg = yaml.safe_load((HERE / "edits.yaml").read_text())
    rng = random.Random(args.seed)
    ed_path = (ROOT / "data" / "mapping" / "commands" /
               args.edits_run / "edits.jsonl")
    out_dir = ROOT / "data" / "mapping" / "commands" / args.run
    out_dir.mkdir(parents=True, exist_ok=True)

    gloss = ROOT / "data" / "mapping" / "glossary" / args.glossary_run
    phrases = {l: load_phrases(gloss, l) for l in ("en", "de")}

    n_in = n_kw = 0
    drops = Counter()
    with (out_dir / "sentences.jsonl").open("w") as f:
        for line in ed_path.read_text().splitlines():
            if not line.strip():
                continue
            e = json.loads(line)
            n_in += 1
            if not eligible(e):
                drops["ineligible"] += 1
                continue
            if rng.random() >= cfg["keyword_template"]["fraction"]:
                drops["skipped"] += 1
                continue
            row = build_sentence(e, phrases[e["lang"]], rng)
            if row is None:
                drops["no_phrase"] += 1
                continue
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
            n_kw += 1
    print(f"{n_in} edits -> {n_kw} keyword sentences "
          f"({dict(drops)}) -> {out_dir / 'sentences.jsonl'}")


if __name__ == "__main__":
    main()
