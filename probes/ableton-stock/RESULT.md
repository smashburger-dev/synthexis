# Ableton Live stock devices: parameter names and labels

Run 2026-11-13 in a Linux cloud container, no Live installed: sources read
over the network, labels written by hand rules, no paid model calls. Nothing
here was run inside Live. Tools and commands:
[`tools/mapping/ableton/`](../../tools/mapping/ableton/README.md).

## Live version

Live 12.4.7 is the newest stable release (2026-10-22, Ableton's Live 12
release notes); 12.4 shipped in early May 2026 with updated Erosion, Delay,
Chorus-Ensemble, Wavetable and Drum Sampler. The online Reference Manual is
the 12.4 one: it names the pre-12.4 Erosion "Erosion Legacy".

## Sources

| Source | What it gives | Version |
|---|---|---|
| Live's MIDI Remote Scripts, parameter bank tables, via [gluon/AbletonLive12_MIDIRemoteScripts](https://github.com/gluon/AbletonLive12_MIDIRemoteScripts) `e83d519` | `Device.class_name` -> `DeviceParameter.original_name`, the names a remote script looks up | bank tables compiled 2024-03-09/11 = 12.0; `Move/custom_bank_definitions.py` (Python 3.11 bytecode, holds `Erosion2` and the renamed Delay LFO names) = 12.4 |
| same, via [chiakibeats/AbletonLive12.1_MIDIRemoteScripts](https://github.com/chiakibeats/AbletonLive12.1_MIDIRemoteScripts) `2c04903` | the same tables, partly broken decompile | 12.1.11 (source timestamp 2025-04-10) |
| `PARAMETERS_BLACKLIST_FOR_CPP_SANITY_CHECK` in the same files | names Live does not check against real devices (Push UI pages); dropped | 12.0 |
| `ableton/v3/control_surface/components/device.py` | every device's on switch is found by `original_name.startswith("Device On")` | 12.0, 12.1 |
| [Live 12 Reference Manual](https://www.ableton.com/en/live-manual/12/), audio effect, instrument and MIDI effect chapters | display names (section titles) and a second source per name | 12.4 |
| [Ableton Live 12 release notes](https://www.ableton.com/en/release-notes/live-12/) (via search), [What's new in Live 12.4](https://help.ableton.com/hc/en-us/articles/26963870210076-What-s-new-in-Live-12-4) | version, devices 12.4 changed | 12.4.7 |

The Cycling '74 LOM docs list `DeviceParameter.name` and `original_name`
but no per-device names, and Ableton publishes no Push parameter list, so
the scripts' bank tables are the only list of LOM names. They are Ableton's
own code, read through third-party decompiles, never imported or run.
Live checks those names against the real devices at startup (the sanity
check above), so a name in a bank existed in that Live version.

## Coverage

| | Devices | Names | Classes other than other/select/switch/system/unsafe |
|---|---:|---:|---:|
| Audio effects (8 of them legacy: Auto Filter, Auto Pan, Chorus, Erosion, Flanger, Frequency Shifter, Phaser, Redux) | 47 | 1,020 | 430 |
| Instruments | 12 | 1,248 | 538 |
| MIDI effects | 8 | 85 | 16 |
| Racks (Audio Effect, Instrument, Drum, MIDI Effect; Macro 1-16) | 4 | 68 | 64 |
| **All** | **71** | **2,421** | **1,048** |

Classes: other 504, ctl.select 322, ctl.switch 298 (71 of them "Device
On"), system 244, unsafe 5 (Compressor Output Gain, Glue Compressor Makeup,
Utility Gain and Gain (Legacy), Looper State), the rest spread over 39
performance classes. Of the 42 + 13 + 8 devices in the manual's chapters, 59
are covered; External Audio Effect, Spectrum, Tuner and External Instrument
have no bank in any script.

Rules as in PR 6: a pure dry level is `other` (Filter Delay "Dry"). New
here: where two parameters of one device would share a class but move the
sound in opposite directions, the one whose raise darkens or thins it gets
`other` (Drift HP Freq, Echo HP Freq, Reverb LowShelf Gain and In Filter
Freq, Chorus-Ensemble HP Freq, Tension Body Low-Cut, Spectral Resonator LF
Damp, Vocoder Lower Filter Band, Saturator Color Amt Low), because the
resolver binds the first parameter of a class and the LOM order of these
devices is not known here.

## Status per name

| Status | Names | Meaning |
|---|---:|---|
| 12.4 | 544 | in the 12.4 Move banks (229) or used by the 12.4 manual in the device's own section (413); 98 both |
| 12.1 | 359 | in the 12.1.11 banks (incl. the 71 "Device On") |
| 12.0 | 1,394 | only in the 12.0 banks, device unchanged since as far as the release notes say |
| UNVERIFIED in 12.4 | 124 | device changed in 12.4 and no 12.4 source has the name: Wavetable 89, Reverb 21, Delay 10 (`Mod Freq`, `Dly < Mod`, `Filter < Mod`, `R Time`, ...; 12.4 calls the LFO ones `LFO Freq`, `LFO > Delay`, `LFO > Filter`), Drum Sampler 3 (`Flt Freq`, `Flt Reso`, `Flt Type`), Chorus-Ensemble 1 (`HPF Freq`; 12.4 has `HP Freq`) |

Display names: 59 devices have a manual section of that title and the
manual names Erosion Legacy; the other 7 legacy devices and the 4 racks are
named from memory (UNVERIFIED).

The manual's labels mostly differ from LOM names ("Decay" vs "Decay
Time", "Osc 1 Pos" vs "Position"), so 1,759 names are not in the manual
text; that alone says nothing against them.

## Missing

- Parameters outside every bank. The banks list what a controller maps,
  not every parameter. Devices only the 12.4 Move banks cover keep their
  main bank: Auto Filter (12.2 redesign, `AutoFilter2`) 17 names, Auto
  Pan-Tremolo 18, Auto Shift 13, Erosion 6. New 12.4 controls the banks do
  not show: Chorus-Ensemble Time and Taps, Delay's new LFO time modes and
  waveforms, Wavetable and Drum Sampler 12.4 changes.
- Parameter index order: unknown, so the class-tie rule above is a guess
  per device.
- `DeviceParameter.name` where it differs from `original_name`: Meld (its
  87 names are `MeldVoice_*` ids) and renamed rack macros. The Live bridge
  must send `original_name`.
- Max for Live devices (Granulator III, the M4L audio effects): their
  `class_name` is a generic M4L class, so `ableton:<class_name>` cannot tell
  them apart. Not covered.
- Live's parameter value ranges: the manual was used for names only.

To close the gaps: the planned Live remote script (`docs/host-support.md`,
step 11) can dump `class_name`, `original_name`, `name`, index, min, max
and value strings of every parameter of every device in one Set, the way
`tools/mapping/fl/device_synthexis_harvest.py` does for FL.

## known.tsv

71 plugin lines `#ableton:<class_name>` and 2,421 parameter lines (2,492
lines). No existing key starts with `ableton:`; keyed by display name, Live's
Frequency Shifter would have met FL's. Every existing line is byte-identical:
with the `ableton:` and `logic:` blocks removed the file equals `main`'s
(checked in Python and by `test_stock.py`). Git's default diff shows some
lines as removed and re-added because of its heuristic; `--minimal` shows 0
removed.

## Commands

```bash
python3 -I tools/mapping/ableton/lom_names.py /tmp/lom/gluon /tmp/lom/live121 > tools/mapping/ableton/lom_names.tsv
python3 -I tools/mapping/ableton/manual_check.py /tmp/lom/manual > tools/mapping/ableton/manual_12_4.tsv
python3 tools/mapping/ableton/stock_labels.py > tools/mapping/ableton/stock_labels.tsv
python3 tools/mapping/merge_known.py
uv run --with pytest --with pyyaml pytest tools/mapping/test_stock.py      # 9 passed
git diff --minimal --numstat origin/main -- plugin/assistant/completion/known.tsv   # 4323 added, 0 removed (Live and Logic)
```
