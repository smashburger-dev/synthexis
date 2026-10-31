# /// script
# requires-python = ">=3.11"
# ///
"""How many continuous parameters report text that tells the resolver
nothing: no text, text that never changes, or a plain unitless linear
scale (-100..100, 0..1, ...). Reads the 11 harvest samples per parameter. Only parameters a host can
automate count (VST3 can_automate and not hidden or read-only, AU
writable, no per-channel MIDI proxy): 676,000 VST3 rows are JUCE
"MIDI CC n|m" proxies without flags.

  uv run probes/plugin-states/generic_text.py [--plugin "Xfer Records/Serum 2"]

Prints one JSON document (counts per class, per format, top plugins).
"""

import argparse
import json
import re
from collections import Counter, defaultdict

from run_states import HARVEST

NUM = re.compile(r"^\s*([-+]?\d+(?:\.\d+)?)\s*(%?)\s*$")
PLAIN = {(0, 1), (-1, 1), (0, 100), (-100, 100), (-50, 50), (0, 10),
         (-10, 10), (0, 127)}
# per-channel MIDI proxies (JUCE "MIDI CC 0|0", Serum 2 "CC0 Chan 1")
MIDI_PROXY = re.compile(r"^(MIDI CC \d+\|\d+|CC\d+ Chan \d+|"
                        r"Aftertouch Chan \d+|Pitch ?[Bb]end Chan \d+)$")
# names of quantities that have a real unit the plain scale hides
UNIT_WORD = re.compile(
    r"\b(rate|time|freq\w*|cutoff|range|oct\w*|semi\w*|pitch|tune|"
    r"transpose|delay|attack|decay|release|hold|bpm|length|speed|hz|ms|"
    r"repeats?|steps?|voices?)\b", re.I)


def linear(t: list[float], v: list[float]) -> bool:
    span = v[-1] - v[0]
    if span == 0:
        return False
    return all(abs(v[0] + span * x - y) <= 0.01 * abs(span) + 0.051
               for x, y in zip(t, v))


def classify(samples: list, units: str = "") -> str:
    t = [s[0] for s in samples]
    texts = [(s[1] or "").strip() for s in samples]
    if not any(texts):
        return "no_text"
    if not all(texts):
        return "partly_empty"
    if len(set(texts)) == 1:
        return "constant"
    m = [NUM.match(x) for x in texts]
    if not all(m):
        if any(not re.search(r"\d", x) for x in texts):
            return "named"
        return "numeric_with_unit"
    v = [float(x.group(1)) for x in m]
    if units.strip() and units.strip() != "%":
        return "numeric_with_unit"
    if not linear(t, v):
        return "numeric_nonlinear"
    ends = (round(v[0], 6), round(v[-1], 6))
    if ends in PLAIN or (ends[1], ends[0]) in PLAIN:
        return "plain_scale"
    return "linear_unitless_other"


def automatable(fmt: str, r: dict) -> bool:
    f = set(r.get("flags") or [])
    if MIDI_PROXY.match(r.get("name") or ""):
        return False
    if fmt == "au":
        return "writable" in f
    return "can_automate" in f and not f & {"is_hidden", "read_only"}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--plugin", default="")
    ap.add_argument("--class-name", default="")
    args = ap.parse_args()
    out = {}
    for fmt in ("vst3", "au"):
        cls = Counter()
        plain_ends = Counter()
        per_plugin = defaultdict(Counter)
        unit_named = Counter()
        total = 0
        skipped = 0
        with (HARVEST / fmt / "params.jsonl").open() as f:
            for line in f:
                r = json.loads(line)
                if args.plugin and r["plugin_id"] != args.plugin:
                    continue
                if args.class_name and r.get("class_name") not in (
                        None, args.class_name):
                    continue
                if fmt == "vst3" and r["steps"] != 0:
                    continue
                if fmt == "au" and (r.get("unit") in ("indexed", "boolean")
                                    or r.get("value_strings")):
                    continue
                if not automatable(fmt, r):
                    skipped += 1
                    continue
                total += 1
                c = classify(r["samples"], r.get("units", ""))
                if fmt == "au" and c == "no_text":
                    # AU values are in plugin units already: only a
                    # generic unit with a 0..1 range says nothing
                    generic = r.get("unit") in ("generic", "custom")
                    if generic and (r["min"], r["max"]) in ((0, 1), (0.0, 1.0)):
                        c = "no_text_generic_0_1"
                    elif generic:
                        c = "no_text_generic_unit"
                    else:
                        c = "no_text_typed_unit"
                cls[c] += 1
                per_plugin[r["plugin_id"]][c] += 1
                if c == "plain_scale":
                    m = [float(NUM.match(s[1]).group(1)) for s in r["samples"]]
                    plain_ends[f"{m[0]:g}..{m[-1]:g}"] += 1
                if c in ("plain_scale", "constant") and \
                        UNIT_WORD.search(r.get("name") or ""):
                    unit_named[c] += 1
        bad = ("no_text", "constant", "plain_scale", "no_text_generic_0_1")
        top = sorted(per_plugin.items(),
                     key=lambda kv: -sum(kv[1][b] for b in bad))[:15]
        out[fmt] = {
            "continuous_params": total,
            "not_automatable_skipped": skipped,
            "classes": dict(cls.most_common()),
            "plain_scale_ranges": dict(plain_ends.most_common()),
            "generic_with_unit_word_in_name": dict(unit_named),
            "plugins_with_generic": sum(
                1 for c in per_plugin.values() if any(c[b] for b in bad)),
            "plugins": len(per_plugin),
            "top_generic": [(p, {b: c[b] for b in bad if c[b]},
                             sum(c.values())) for p, c in top],
        }
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
