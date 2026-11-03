# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Export the completion index data for the plugin's sentence field
(plugin/complete, Rust).

  uv run tools/mapping/export_completion.py [--plugin "Serum 2" ...] [--out DIR]

Writes into data/mapping/completion/ (or --out):
  vocab.json            gesture phrases (glossary kind source and shape),
                        parameter-class phrases (kind param), target phrases
                        (kind target: MIDI note, chord, pattern, CC and
                        macro, up to 3 words; MIDI generator properties and
                        the XY pad harvested from the training spans, see
                        harvest_targets), class prior, span prior (how
                        often each training span text named each class)
                        (label counts over all harvested plugins), and the
                        list of exported plugins
  plugins/<slug>.json   one picked plugin's parameters: index, name, class

Every gesture, class and target phrase carries its language (`lang`: de or
en); the plugin's language setting filters on it. A phrase both languages
use is one row per language, each in its own spelling. Harvested target
phrases take the languages of the training rows they come from
(harvest_targets).

Glossary rows marked as typos are left out: a completion inserts its label
into the sentence, so labels have to be spelled right. Labels come from the
Spark Open runs (final class of the priority runs, class of the bulk runs);
VST3 wins over AU for the same plugin id, priority over bulk.
"""

import argparse
import json
import re
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "data" / "mapping"
LABEL_RUNS = [  # earlier entries win per plugin id
    "label-vst3-prio-v1",
    "label-vst3-bulk-v1",
    "label-prio-v1",
    "label-bulk-v1",
]
# targets that are no plugin parameter; a chip puts its phrase into the
# sentence, so long phrases ("MIDI Note halten solange Hand da ist") stay out
TARGET_KINDS = {"midi.note", "midi.chord", "midi.pattern", "midi.cc", "macro"}
TARGET_WORDS = 3
# training spans: their texts name targets the glossary has no phrases for
TRAIN = "train/ds-v11c/train_all.jsonl"
HARVEST_MIN = 3  # occurrences; drops typos ("reverv")
HARVEST_SHARE = 0.8  # of a text's occurrences on one target
ONTOLOGY = Path(__file__).resolve().parent / "ontology.yaml"
# a bare "Finger": the panel picks which one (index by default); the same
# word in both languages, so one item per language
GENERIC = [{"source": "hand.finger.index", "p": "Finger", "lang": "de", "pol": 1},
           {"source": "hand.finger.index", "p": "finger", "lang": "en", "pol": 1}]
# a harvested phrase gets every language with at least this share and
# HARVEST_MIN of its occurrences in rows that are not code-switched
LANG_SHARE = 0.25
# demo plugins (probes/real-eval) plus a few common effects
DEFAULT_PLUGINS = [
    "Xfer Records/Serum 2",
    "Output/Portal",
    "Valhalla DSP, LLC/ValhallaFutureVerb",
    "FabFilter/Timeless 3",
    "Universal Audio (UADx)/UADx Galaxy Tape Echo",
    "Neural DSP/Archetype John Mayer X",
    "Cableguys/ShaperBox 3",
    "Waves/Nx Germano Studios (s)",
    "FabFilter/Pro-Q 3",
    "Valhalla DSP, LLC/ValhallaDelay",
]


def slug(s: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")


def train_targets(path: Path):
    """(span text, target, row lang, row register) for every TGT span of the training rows whose
    target token is known: TGT spans and T: tokens in the same count, else
    one per route (as probes/completion-targets/targets.py does for dev).
    The target drops values except a parameter's class and a MIDI
    generator property: T:midi.cc:74 -> midi.cc, T:midi.prop:velocity ->
    midi.prop:velocity."""
    for line in open(path, encoding="utf-8"):
        r = json.loads(line)
        tgts = [r["text"][b:e] for b, e, k in r["bio"] if k == "TGT"]
        routes, cur = [], None
        for t in r["frame"]:
            if t == "<route>":
                cur = []
            elif t == "</route>":
                routes.append(cur)
                cur = None
            elif cur is not None and isinstance(t, str):
                cur.append(t)
        ts = [t for x in routes for t in x if t.startswith("T:")]
        if len(ts) != len(tgts):
            ts = [next((t for t in x if t.startswith("T:")), None) for x in routes]
        if len(ts) != len(tgts):
            continue
        for text, t in zip(tgts, ts):
            if t:
                k = t[2:].split(":", 2)
                yield text, ":".join(k[:2]) if k[0] in ("param", "midi.prop") else k[0], r["lang"], r["register"]


def one_edit(a: str, b: str) -> bool:
    """a and b differ by one inserted, deleted or substituted char."""
    if abs(len(a) - len(b)) > 1 or a == b:
        return False
    if len(a) > len(b):
        a, b = b, a
    i = next((k for k in range(len(a)) if a[k] != b[k]), len(a))
    return a[i + (len(a) == len(b)):] == b[i + 1:]


def harvest_targets(path: Path, known: set[str]) -> list[dict]:
    """Phrases for midi.prop:<prop> and xy from the training spans: a text
    seen at least HARVEST_MIN times, at least HARVEST_SHARE of them on one
    of these targets, up to TARGET_WORDS words, not yet a glossary target
    phrase; shown in its most frequent spelling. Texts with a "." (ids) and
    texts one edit away from a more frequent one of the same target (the
    training set carries deliberate typos) stay out. `langs`: every row
    language with at least LANG_SHARE and HARVEST_MIN of the text's
    occurrences outside code-switched rows ("velocity" is said in German sentences too)."""
    by_text: dict[str, Counter] = {}
    spelling: dict[str, Counter] = {}
    by_lang: dict[str, Counter] = {}
    for text, target, lang, register in train_targets(path):
        key = " ".join(text.casefold().split())
        by_text.setdefault(key, Counter())[target] += 1
        spelling.setdefault(key, Counter())[text.strip()] += 1
        if register != "codeswitch":
            by_lang.setdefault(key, Counter())[lang] += 1
    out = []
    for key, c in sorted(by_text.items()):
        target, n = c.most_common(1)[0]
        total = sum(c.values())
        if not (target.startswith("midi.prop:") or target == "xy"):
            continue
        if total < HARVEST_MIN or n < HARVEST_SHARE * total or len(key.split()) > TARGET_WORDS or key in known:
            continue
        if "." in key:
            continue
        ln = by_lang.get(key, Counter())
        langs = sorted(lang for lang, m in ln.items() if m >= max(HARVEST_MIN, LANG_SHARE * sum(ln.values())))
        out.append({"kind": target, "key": key, "p": spelling[key].most_common(1)[0][0], "n": n, "langs": langs})
    return [h for h in out
            if not any(o["kind"] == h["kind"] and o["n"] > h["n"] and one_edit(o["key"], h["key"]) for o in out)]


def sided_sources(path: Path) -> list[str]:
    """Sources the ontology marks `side: true` (hand, eye, arm, motion):
    their completion items get the hand-side panel."""
    return re.findall(r"\{id: ([a-z_.]+), side: true", path.read_text(encoding="utf-8"))


def label_class(r: dict) -> str:
    return r["final"][0] if "final" in r else r["class"]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--plugin", action="append", help="plugin id (vendor/name); repeatable")
    ap.add_argument("--data", default=str(DATA), help="data/mapping of the main checkout")
    ap.add_argument("--out", help="default <data>/completion")
    a = ap.parse_args()
    data = Path(a.data)
    out = Path(a.out or data / "completion")
    (out / "plugins").mkdir(parents=True, exist_ok=True)
    wanted = set(a.plugin or DEFAULT_PLUGINS)

    gestures, phrases, targets, seen = [], [], [], set()
    for line in open(data / "glossary/gloss-v1/glossary.jsonl", encoding="utf-8"):
        r = json.loads(line)
        if r.get("typo"):
            continue
        kind, cid = r["c"].split(":", 1)
        # per language: a phrase both languages use stays in both
        key = (r["c"], r["p"].casefold(), r["lang"])
        if key in seen:
            continue
        seen.add(key)
        if kind == "source":
            gestures.append({"source": cid, "p": r["p"], "lang": r["lang"], "pol": r["pol"]})
        elif kind == "shape":
            gestures.append({"source": "hand.shape", "shape": cid, "p": r["p"], "lang": r["lang"], "pol": r["pol"]})
        elif kind == "param":
            phrases.append({"class": cid, "p": r["p"], "lang": r["lang"]})
        elif kind == "target" and cid in TARGET_KINDS and len(r["p"].split()) <= TARGET_WORDS:
            targets.append({"kind": cid, "p": r["p"], "lang": r["lang"]})

    # span prior: per span text, how often it named each parameter class
    # (ctl.switch for a toggle) or target in training
    span_prior: Counter = Counter()
    for text, target, _, _ in train_targets(data / TRAIN):
        cls = target[6:] if target.startswith("param:") else "ctl.switch" if target == "toggle" else target
        span_prior[(" ".join(text.casefold().split()), cls)] += 1
    gestures += [g for g in GENERIC if (g["source"], g["p"].casefold(), g["lang"]) not in
                 {(x["source"], x["p"].casefold(), x["lang"]) for x in gestures}]
    known = {t["p"].casefold() for t in targets}
    harvested = harvest_targets(data / TRAIN, known)
    targets += [{"kind": h["kind"], "p": h["p"], "lang": lang} for h in harvested for lang in h["langs"]]

    prior: Counter = Counter()
    params: dict[str, dict[int, dict]] = {}
    owner: dict[str, str] = {}  # plugin id -> label run it came from
    for run in LABEL_RUNS:
        for line in open(data / "labels" / run / "labels.jsonl", encoding="utf-8"):
            r = json.loads(line)
            pid = r["plugin_id"]
            if owner.setdefault(pid, run) != run:
                continue
            c = label_class(r)
            prior[c] += 1
            if pid in wanted:
                params.setdefault(pid, {})[r["index"]] = {"i": r["index"], "n": r["name"], "c": c}

    plugins = []
    for pid in sorted(params):
        rows = [params[pid][i] for i in sorted(params[pid])]
        f = f"{slug(pid)}.json"
        (out / "plugins" / f).write_text(json.dumps({"plugin_id": pid, "params": rows}, ensure_ascii=False))
        plugins.append({"plugin_id": pid, "name": pid.split("/", 1)[-1], "file": f, "n": len(rows)})
    missing = sorted(wanted - set(params))
    vocab = {"gestures": gestures, "param_phrases": phrases, "targets": targets, "sided": sided_sources(ONTOLOGY),
             "span_prior": [[t, c, n] for (t, c), n in sorted(span_prior.items())], "class_prior": dict(prior), "plugins": plugins}
    (out / "vocab.json").write_text(json.dumps(vocab, ensure_ascii=False))
    print(f"{len(gestures)} gesture phrases, {len(phrases)} parameter phrases, {len(targets)} target phrases, "
          f"{len(prior)} classes, "
          f"{len(plugins)} plugins -> {out}")
    if missing:
        print("no labels for:", ", ".join(missing))


if __name__ == "__main__":
    main()
