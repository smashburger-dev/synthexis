# Logic Pro own plug-ins

Hand labels for the controls of Logic Pro 13's own effects and instruments,
for the Logic path of `docs/host-support.md` (step 12, "a hand-made Logic
stock list"). Logic's own plug-ins are no Audio Units, so `au_harvest`
never sees them, and Logic has no script interface that reads parameter
names. Measurements and gaps: [`probes/logic-stock/RESULT.md`](../../../probes/logic-stock/RESULT.md).

| File | What |
|---|---|
| `pages.tsv` | the 420 guide pages read: kind, plug-in, page title, URL |
| `manual_names.py` -> `manual_names.tsv` | control names per plug-in from those pages |
| `stock_labels.py` -> `stock_labels.tsv` | one param-sense class per name (Live rules of `../ableton/stock_labels.py`, Logic rules, overrides) |
| `../merge_known.py`, `../test_stock.py` | export into `known.tsv` and checks, shared with Live |

## Keys

Every plug-in is the plugin `logic:<plug-in name>` in `known.tsv`
(`#logic:chromaverb`). "Compressor", "Chorus", "Sampler" or "Limiter" are
also third-party plug-in names; the prefix keeps them apart, and no other
key starts with `logic:`.

The names are the guide's control labels without the control word
("Threshold knob and field" -> "Threshold"; "Bass, Mids, and Treble knobs"
-> three names). The names Logic shows in automation lanes and Controller
Assignments can differ; every row has status "UI label" until a probe in
Logic reads them. Whatever bridge reaches Logic has to map its names onto
these or replace them.

## Source

[Logic Pro User Guide for Mac](https://support.apple.com/guide/logicpro/welcome/mac),
version 13.0 for macOS 27 (its page URLs end in `/13.0/mac/27`), parts
"Logic Pro Effects" and "Logic Pro Instruments", read 2026-11-13.
`pages.tsv` comes from the guide's table of contents: every page under
those two parts except legacy effects and instruments, metering, imaging,
utilities, Mastering Assistant, Scripter, Surround Compressor, Drum Kit and
Drum Machine Designer, and pages that are tutorials, how-tos, overviews or
lists of models. A control is a list item that starts with an emphasized
label ending in a control word (knob, slider, field, button, switch, pop-up
menu, checkbox). Only names are kept, no guide text.

## Commands

```bash
mkdir -p /tmp/logic-pages
n=0; while IFS=$'\t' read -r _ _ _ url; do n=$((n+1)); curl -sS "$url" | python3 -c 'import sys; t=sys.stdin.read(); i=t.find("<h1"); sys.stdout.write(t[i:t.find("</article>", i)])' > /tmp/logic-pages/$n.html; done < tools/mapping/logic/pages.tsv
python3 -I tools/mapping/logic/manual_names.py tools/mapping/logic/pages.tsv /tmp/logic-pages > tools/mapping/logic/manual_names.tsv
python3 tools/mapping/logic/stock_labels.py > tools/mapping/logic/stock_labels.tsv
python3 tools/mapping/merge_known.py
uv run --with pytest --with pyyaml pytest tools/mapping/test_stock.py
```
