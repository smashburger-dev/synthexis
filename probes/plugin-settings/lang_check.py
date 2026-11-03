# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Checks the language tags of a completion export (RESULT.md, section 1).

  uv run probes/plugin-settings/lang_check.py <completion dir> [<glossary.jsonl>]

(a) harvested target phrases: German ones the export shows in English mode
    (leaks) and English ones it hides from English mode (misses), against
    the hand labels in GOLD;
(b) every English item: no ä/ö/ü/ß and no German function word.
"""

import json
import re
import sys
from collections import Counter
from pathlib import Path

# Hand labels of the 44 harvested phrases (written before the run): is the
# phrase English, German or used as-is in both.
GOLD = {
    "akkord-voicing": "de", "akkord-weite": "de", "akkorde grösser": "de", "akkorde weiter": "de",
    "akkordgrösse": "de", "akkordlage": "de", "anschlagstärke": "de", "arp go faster": "en",
    "arp octave range": "en", "arp octaves": "en", "arp oktavbereich": "de", "arp range": "en",
    "arp rate": "both", "arp schneller": "de", "arp speed": "en", "arp style": "en",
    "arp tempo": "both", "arp-geschwindigkeit": "de", "arp-oktaven": "de", "arp-oktavlage": "de",
    "arp-reihenfolge": "de", "arp-stil": "de", "arpeggiator octaves": "en", "arpeggiator rate": "en",
    "arpeggios": "both", "chord notes": "en", "chord size": "en", "chord spread": "en",
    "chord-grösse": "de", "chord-voicing": "de", "chordgrösse": "de", "chords bigger": "en",
    "gate length": "en", "note length": "en", "notenlänge": "de", "octaves": "en", "oktaven": "de",
    "oktaven im arp": "de", "pad": "both", "softer and harder": "en", "two-axis pad": "en",
    "velocity": "both", "voicing": "both", "xy pad": "both",
}
GERMAN = set("""ich mich mein meine meinen meiner der das den dem des und oder mit wenn dann
auf zum zur nach hoch runter schließen öffnen machen spreizen ballen kneifen augen mund
kopf links rechts beide spielen akkord noten lauter leiser schneller langsamer zusammen
ein eine einen nicht ganz sehr mehr weniger drehen heben senken""".split())


def norm(p: str) -> str:
    return " ".join(p.casefold().split())


def main() -> None:
    v = json.loads((Path(sys.argv[1]) / "vocab.json").read_text(encoding="utf-8"))
    gloss = Path(sys.argv[2] if len(sys.argv) > 2 else
                 Path(__file__).resolve().parents[2] / "data/mapping/glossary/gloss-v1/glossary.jsonl")
    glossary_targets = {norm(json.loads(l)["p"]) for l in open(gloss, encoding="utf-8")
                        if json.loads(l)["c"].startswith("target:")}
    harvested: dict[str, set] = {}
    for t in v["targets"]:
        if (t["kind"].startswith("midi.prop") or t["kind"] == "xy") and norm(t["p"]) not in glossary_targets:
            harvested.setdefault(norm(t["p"]), set()).add(t["lang"])
    leaks = sorted(k for k, ls in harvested.items() if "en" in ls and GOLD.get(k) == "de")
    misses = sorted(k for k, ls in harvested.items() if "en" not in ls and GOLD.get(k) in ("en", "both"))
    unlabelled = sorted(set(harvested) - set(GOLD))
    print(f"harvested {len(harvested)}, unlabelled {unlabelled}")
    print(f"(a) leaks into English mode: {len(leaks)} {leaks}")
    print(f"(a) English phrases hidden from English mode: {len(misses)} {misses}")
    bad = [(k, t["p"]) for k in ("gestures", "param_phrases", "targets") for t in v[k]
           if t.get("lang") == "en" and (re.search("[äöüß]", t["p"].casefold())
                                         or GERMAN & set(re.findall(r"[\wäöüß]+", t["p"].casefold())))]
    print(f"(b) English items with German marks: {len(bad)} {bad[:20]}")
    print("items per language:", dict(Counter(t.get("lang") for k in ("gestures", "param_phrases", "targets")
                                               for t in v[k])))


if __name__ == "__main__":
    main()
