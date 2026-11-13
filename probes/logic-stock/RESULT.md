# Logic Pro 13 own plug-ins: control names and labels

Run 2026-11-13 in a Linux cloud container, no Logic installed: Apple's user
guide read over the network, labels written by hand rules, no paid model
calls. Nothing here was run inside Logic. Tools and commands:
[`tools/mapping/logic/`](../../tools/mapping/logic/README.md).

## Logic version

Logic Pro 13 came out on 2026-11-05 (press reports; Apple's
[release notes](https://support.apple.com/109503) page lists it). Reviews
say it adds no new effects or instruments. The
[Logic Pro User Guide for Mac](https://support.apple.com/guide/logicpro/welcome/mac)
serves version 13.0 for macOS 27 (page URLs end in `/13.0/mac/27`).
`probes/logic-au` measured the AU build on Logic 12.4; this probe only
reads the 13.0 guide.

## Source

The user guide is the only list Apple publishes. Its parts "Logic Pro
Effects" and "Logic Pro Instruments" have 284 and 523 pages; 424 of them
were read (`tools/mapping/logic/pages.tsv`, the selection is in the README).
A control is a list item whose emphasized label ends in a control word;
the name is the label without it ("Threshold knob and field" ->
"Threshold").

Logic's own plug-ins are no Audio Units and Logic has no script interface
that lists parameters, so no machine-readable list exists outside Logic.
What Logic shows in automation lanes and Controller Assignments can differ
from the guide's labels: every name has status "UI label".

## Coverage

| | Plug-ins | Names | Classes other than other/select/switch/system/unsafe |
|---|---:|---:|---:|
| Effects (incl. 7 MIDI plug-ins with 57 names) | 64 | 845 | 217 |
| Instruments | 23 | 899 | 245 |
| **All** | **87** | **1,744** | **462** |

Classes: other 935, ctl.select 148, ctl.switch 106, system 91, unsafe 2
(Compressor Make Up, Gain plug-in Gain). `other` is large because the
guide names many controls of the big instruments by function (Alchemy's
source editors, Sculpture's material pad, Vintage B3's expert settings).

Effects: Adaptive Limiter, Amp Designer, Arpeggiator, AutoFilter, Bass Amp
Designer, Beat Breaker, Bitcrusher, Channel EQ, Chorus, ChromaGlow,
ChromaVerb, Clip Distortion, Compressor, DeEsser 2, Delay Designer,
Distortion, Distortion II, EVOC 20 Filterbank, EVOC 20 TrackOscillator,
Echo, EnVerb, Ensemble, Enveloper, Exciter, Expander, Flanger, Fuzz-Wah,
Gain, Limiter, Linear Phase EQ, Match EQ, Microphaser, Modifier, Modulation
Delay, Modulator, Multipressor, Note Repeater, Overdrive, Pedalboard (the
stompboxes merged), Phase Distortion, Phaser, Phat FX, Pitch Correction,
Quantec Room Simulator, Randomizer, Remix FX, Ringshifter, Rotor Cabinet,
Sample Delay, Scanner Vibrato, SilverVerb, Single Band EQ, Space Designer,
Spreader, Step FX, Stereo Delay, Stereo Spread, SubBass, Tape Delay,
Transposer, Tremolo, Velocity Processor, Vintage EQ Collection, Vocal
Transformer.

Instruments: Alchemy, Drum Synth, EFM1, ES E, ES M, ES P, ES1, ES2, EVOC 20
PolySynth, Quick Sampler, Retro Synth, Sample Alchemy, Sampler, Sculpture,
Studio Bass, Studio Horns, Studio Piano, Studio Strings, Ultrabeat, Vintage
B3 Organ, Vintage Clav, Vintage Electric Piano, Vintage Mellotron.

Same rules as Live: dry levels `other`, one class per direction, gains far
above 0 dB `unsafe`.

## Missing

- Logic's automation names: not read. A probe in Logic (open a plug-in,
  show its automation parameter list, or Controller Assignments) has to
  replace or map the "UI label" names.
- Noise Gate: its guide page has no control in the list-item form the
  extractor reads, so it has no names and no `known.tsv` entry.
- ES2 has 16 names only: most of its pages are not in that form either.
- Left out: legacy effects and instruments, metering, Scripter, Surround
  Compressor, Direction Mixer, Multichannel Gain, Mastering Assistant,
  Drum Kit Designer, Drum Machine Designer, External Instrument, Klopfgeist,
  Test Oscillator.
- Duplicate names inside one plug-in collapse to one (Pedalboard's pedals
  all have "Drive", "Tone", "Level"; Alchemy's sections share "Attack").
- Parameter order and value ranges: unknown.

## known.tsv

87 plugin lines `#logic:<plug-in name>` and 1,744 parameter lines (1,831
lines). No existing key starts with `logic:`, so nothing that resolves today
resolves differently; every existing line is byte-identical (see
`probes/ableton-stock/RESULT.md`, same check).

synthexis does not read Logic's plug-in names yet (the AU of PR 7 loads in
Logic; Learn maps a MIDI port, it reports no names). The labels wait for
the Logic path that does, under the key `"logic:" + plug-in name`.

## Commands

```bash
python3 -I tools/mapping/logic/manual_names.py tools/mapping/logic/pages.tsv /tmp/logic-pages > tools/mapping/logic/manual_names.tsv
python3 tools/mapping/logic/stock_labels.py > tools/mapping/logic/stock_labels.tsv
python3 tools/mapping/merge_known.py
uv run --with pytest --with pyyaml pytest tools/mapping/test_stock.py      # 9 passed
```
