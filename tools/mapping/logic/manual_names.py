"""Control names of Logic Pro's own plug-ins from Apple's Logic Pro 13 User
Guide for Mac (13.0, macOS 27), the only parameter list Apple publishes:

  python3 -I tools/mapping/logic/manual_names.py <pages.tsv> <pages dir> \
      > tools/mapping/logic/manual_names.tsv

pages.tsv lists the guide pages read (kind, plug-in, page title, URL; see
README.md for how it was made from the guide's table of contents) and the
pages dir holds each page's article HTML as <line number>.html. A control
is a list item that starts with an emphasized label ending in a control
word ("Threshold knob and field", "Bass, Mids, and Treble knobs"); the
label without the control word is the name. Items that name no control
(model lists, display areas) are skipped. Only names are written, no
guide text. These are UI labels: the names Logic shows for automation and
Controller Assignments can differ (UNVERIFIED until read in Logic).
"""

import html
import re
import sys
from pathlib import Path

CONTROL = (r"(?:value )?(?:knobs?|sliders?|fields?|buttons?|switch(?:es)?|pop-up menus?|menus?|checkbox(?:es)?|"
           r"wheels?|xy pad|pads?|fader)")
TAIL = re.compile(rf"\s+(?:{CONTROL})(?:\s*(?:,|and|or|/)\s*(?:{CONTROL}))*$", re.I)
NOT_A_PARAM = re.compile(r"display|area|control point|\bline|graph|window|view|editor|meter|keyboard|browser|"
                         r"\bsave\b|\bcopy\b|\bpaste\b|\breset\b|\bload\b|\bundo\b|\bcompare\b|\bpresets?\b|"
                         r"\bsettings? (?:pop-up )?menu\b|^(?:on/off|power)$", re.I)
PLUGIN = {"Distortion effect": "Distortion", "Pitch Correction effect": "Pitch Correction",
          "Rotor Cabinet effect": "Rotor Cabinet", "Arpeggiator MIDI plug-in": "Arpeggiator",
          "Space Designer envelopes and EQ": "Space Designer", "Stompboxes": "Pedalboard"}
SKIP_PLUGINS = {"Utilities"}  # External Instrument, Klopfgeist and Test Oscillator on shared pages
# editor actions and fragments the label rule lets through
ACTIONS = {"off", "hide", "ms", "point", "record", "edit", "edit (e)", "learn", "clear", "image", "lasso", "brush",
           "erase", "file", "back", "previous/next", "detail", "draw/mask", "add zone (+)", "group (+)", "select midi",
           "zone name", "auto select", "randomize", "action", "overall", "transform", "mono", "poly", "both",
           "lin", "exp", "1/2/3", "a/b/c/d", "all/a/b/c/d", "all/elements", "main/details",
           "main/additive/spectral", "step number", "audio file analysis"}
FRAGMENT = re.compile(r"^[:x] |^\d+$|chosen| - |\(\+\)$", re.I)


def split(label: str) -> list:
    """'Bass, Mids, and Treble' -> three names; 'Attack and Release' too."""
    parts = re.split(r",\s*(?:and\s+)?|\s+and\s+", label)
    if len(parts) > 1 and all(0 < len(p.split()) <= 3 for p in parts):
        return parts
    return [label]


def main() -> None:
    pages, src = Path(sys.argv[1]), Path(sys.argv[2])
    seen = set()
    print("kind\tplugin\tsection\tname")
    for n, line in enumerate(pages.read_text(encoding="utf-8").splitlines(), 1):
        kind, plug, title, _ = line.split("\t")
        plug = PLUGIN.get(plug, plug)
        if plug in SKIP_PLUGINS:
            continue
        text = (src / f"{n}.html").read_text(encoding="utf-8")
        for m in re.finditer(r"<li>\s*<p>\s*<em>(.*?)</em>", text, re.S):
            lab = re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", "", m.group(1)))).strip().rstrip(":").strip()
            if not TAIL.search(lab):
                continue
            base = TAIL.sub("", lab).strip()
            for name in split(base):
                name = re.sub(r"\s+(?:handle|dot)(?:\s+(?:and|or))?$", "", name.strip())
                if (not name or NOT_A_PARAM.search(name) or len(name) > 40 or name.casefold() in ACTIONS
                        or FRAGMENT.search(name)):
                    continue
                key = (plug, name.casefold())
                if key in seen:
                    continue
                seen.add(key)
                print(f"{kind}\t{plug}\t{title}\t{name}")


if __name__ == "__main__":
    main()
