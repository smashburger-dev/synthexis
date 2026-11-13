"""Which LOM parameter names the Live 12.4 Reference Manual uses for the
same device, as a second, primary source next to the remote scripts:

  python3 -I tools/mapping/ableton/manual_check.py <dir with the manual pages> \
      > tools/mapping/ableton/manual_12_4.tsv

The directory holds the HTML of three chapters of
https://www.ableton.com/en/live-manual/12/ (live-audio-effect-reference,
live-instrument-reference, live-midi-effect-reference), saved as
<chapter>.html. A device's section runs from its numbered heading ("29.32
Reverb") to the next one; a name counts as found when it appears there as a
whole phrase, case ignored. The manual's labels often differ from the LOM
names ("Decay" for "Decay Time"), so "no" does not mean a name is wrong.
Devices without a section (legacy devices, racks) are left out. Only names
and yes/no are written; no manual text is kept.
"""

import html
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from stock_labels import DEVICES  # noqa: E402

CHAPTERS = ["live-audio-effect-reference", "live-instrument-reference", "live-midi-effect-reference"]


def sections(text: str) -> dict:
    out = {}
    heads = list(re.finditer(r"<h2[^>]*>(.*?)</h2>", text, re.S))
    for h, nxt in zip(heads, heads[1:] + [None]):
        title = html.unescape(re.sub(r"<[^>]+>", "", h.group(1))).strip()
        m = re.match(r"\d+\.\d+ (.+)$", title)
        if not m:
            continue
        body = text[h.end(): nxt.start() if nxt else len(text)]
        body = re.sub(r"<script.*?</script>|<style.*?</style>|<[^>]+>", " ", body, flags=re.S)
        out[m.group(1)] = re.sub(r"\s+", " ", html.unescape(body)).casefold()
    return out


def main() -> None:
    src = Path(sys.argv[1])
    secs = {}
    for ch in CHAPTERS:
        secs.update(sections((src / f"{ch}.html").read_text(encoding="utf-8")))
    here = Path(__file__).resolve().parent
    print("class_name\tname\tin_manual")
    for line in (here / "lom_names.tsv").read_text(encoding="utf-8").splitlines()[1:]:
        cls, name, _ = line.split("\t")
        body = secs.get(DEVICES.get(cls, ("",))[0])
        if body is None:
            continue
        pat = r"(?<![\w])" + re.escape(name.strip().casefold()) + r"(?![\w])"
        print(f"{cls}\t{name}\t{'yes' if re.search(pat, body) else 'no'}")


if __name__ == "__main__":
    main()
