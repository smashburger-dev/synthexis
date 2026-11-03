# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Export the completion index data for the plugin's sentence field
(plugin/complete, Rust).

  uv run tools/mapping/export_completion.py [--plugin "Serum 2" ...] [--out DIR]

Writes into data/mapping/completion/ (or --out):
  vocab.json            gesture phrases (glossary kind source and shape),
                        parameter-class phrases (kind param), class prior
                        (label counts over all harvested plugins), and the
                        list of exported plugins
  plugins/<slug>.json   one picked plugin's parameters: index, name, class

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

    gestures, phrases, seen = [], [], set()
    for line in open(data / "glossary/gloss-v1/glossary.jsonl", encoding="utf-8"):
        r = json.loads(line)
        if r.get("typo"):
            continue
        kind, cid = r["c"].split(":", 1)
        key = (r["c"], r["p"].casefold())
        if key in seen:
            continue
        seen.add(key)
        if kind == "source":
            gestures.append({"source": cid, "p": r["p"], "lang": r["lang"], "pol": r["pol"]})
        elif kind == "shape":
            gestures.append({"source": "hand.shape", "shape": cid, "p": r["p"], "lang": r["lang"], "pol": r["pol"]})
        elif kind == "param":
            phrases.append({"class": cid, "p": r["p"], "lang": r["lang"]})

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
    vocab = {"gestures": gestures, "param_phrases": phrases, "class_prior": dict(prior), "plugins": plugins}
    (out / "vocab.json").write_text(json.dumps(vocab, ensure_ascii=False))
    print(f"{len(gestures)} gesture phrases, {len(phrases)} parameter phrases, {len(prior)} classes, "
          f"{len(plugins)} plugins -> {out}")
    if missing:
        print("no labels for:", ", ".join(missing))


if __name__ == "__main__":
    main()
