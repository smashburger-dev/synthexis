# Ableton Live stock devices

Hand labels for the parameters of Live's own devices, keyed by the names a
remote script reads, so the planned Live bridge (`docs/host-support.md`,
step 11) gets classes for stock devices without param-sense. Measurements
and gaps: [`probes/ableton-stock/RESULT.md`](../../../probes/ableton-stock/RESULT.md).

| File | What |
|---|---|
| `lom_names.py` -> `lom_names.tsv` | parameter names per `Device.class_name`, from the parameter banks in Live's MIDI Remote Scripts |
| `manual_check.py` -> `manual_12_4.tsv` | which of those names the Live 12.4 Reference Manual uses in the device's own section |
| `stock_labels.py` -> `stock_labels.tsv` | one param-sense class per name (rules per device family, then overrides), status per name |
| `../merge_known.py` | writes the labels into `plugin/assistant/completion/known.tsv` without the label runs |
| `../test_stock.py` | checks (reproducible TSVs, classes in the ontology, no key collision, other plugins unchanged) |

## Keys

A remote script sees a stock device as `Device.class_name` (`Reverb`,
`Eq8`, `InstrumentVector`) and its parameters by
`DeviceParameter.original_name`, which the user cannot rename. Live's own
scripts look parameters up by `original_name` (`device_parameter_bank.py`).
`Device.name` is no key: a loaded preset renames the device ("Large Hall"),
and for Meld `name` differs from `original_name` (`MeldVoice_EngineA_...`).

`known.tsv` keys plugins by casefolded name only (`sense.rs`,
`Classes::of`). Live's display names would meet AU/VST3 plugins:
"Frequency Shifter" is already FL's, and "Reverb", "Delay", "Compressor",
"EQ Eight" or "Auto Filter" are ordinary plugin names. So every device is
the plugin `ableton:<class_name>` (`#ableton:reverb`). No other key starts
with `ableton:`, so nothing that resolves today resolves differently. The
Live bridge has to ask `sense.rs` for `"ableton:" + class_name` with
`original_name` as the parameter name.

## Sources

- Live's MIDI Remote Scripts ship as bytecode inside Live. Decompiled
  mirrors on GitHub, only parsed with `ast`, never imported or run:
  - [gluon/AbletonLive12_MIDIRemoteScripts](https://github.com/gluon/AbletonLive12_MIDIRemoteScripts)
    at `e83d5192f321b24eb9daab843ac49a2d95d862b1`. The README says Live 12.4,
    but the bank tables (`ableton/v2|v3/control_surface/default_bank_definitions.py`,
    `Push2/custom_bank_definitions.py`, `_Generic/Devices.py`) carry
    "Compiled at: 2024-03-09/11", i.e. Live 12.0. Only the pycdc files
    from Python 3.11 bytecode (`Move/custom_bank_definitions.py`) are newer:
    they hold `Erosion2` and the renamed Delay LFO parameters, both 12.4
    changes, so they count as 12.4.
  - [chiakibeats/AbletonLive12.1_MIDIRemoteScripts](https://github.com/chiakibeats/AbletonLive12.1_MIDIRemoteScripts)
    at `2c04903d6eb1b26ebd065a8308abbf14f95c39c5`, Live 12.1.11 (source
    timestamp 2025-04-10), partly broken decompiles; read line by line.
- Live checks bank names against the real devices at startup, except the
  pseudo-parameters in `PARAMETERS_BLACKLIST_FOR_CPP_SANITY_CHECK`
  ("Osc Select" and other Push pages); those are dropped.
- "Device On": Live's v3 device component finds every device's on switch by
  `original_name.startswith("Device On")`, so each device gets that name.
- [Live 12 Reference Manual](https://www.ableton.com/en/live-manual/12/),
  chapters "Live Audio Effect Reference", "Live Instrument Reference",
  "Live MIDI Effect Reference", read 2026-11-13. It is the 12.4 manual (it
  names the pre-12.4 Erosion "Erosion Legacy"). Used for display names and
  as a second source per name; no manual text is kept.

Only parameter names and short class labels are in this repo. No code from
the mirrors is copied.

## Commands

```bash
git clone https://github.com/gluon/AbletonLive12_MIDIRemoteScripts /tmp/lom/gluon
git -C /tmp/lom/gluon checkout e83d5192f321b24eb9daab843ac49a2d95d862b1
git clone https://github.com/chiakibeats/AbletonLive12.1_MIDIRemoteScripts /tmp/lom/live121
git -C /tmp/lom/live121 checkout 2c04903d6eb1b26ebd065a8308abbf14f95c39c5
python3 -I tools/mapping/ableton/lom_names.py /tmp/lom/gluon /tmp/lom/live121 > tools/mapping/ableton/lom_names.tsv
mkdir -p /tmp/lom/manual
for c in live-audio-effect-reference live-instrument-reference live-midi-effect-reference; do curl -sS -o /tmp/lom/manual/$c.html https://www.ableton.com/en/live-manual/12/$c/; done
python3 -I tools/mapping/ableton/manual_check.py /tmp/lom/manual > tools/mapping/ableton/manual_12_4.tsv
python3 tools/mapping/ableton/stock_labels.py > tools/mapping/ableton/stock_labels.tsv
python3 tools/mapping/merge_known.py
uv run --with pytest --with pyyaml pytest tools/mapping/test_stock.py
```

`uv run tools/mapping/export_paramsense.py` (needs `data/`) reads the same
table through `tools/mapping/stock.py` and writes the same `known.tsv`.
