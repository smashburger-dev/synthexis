# RESULT: plugin states for the mapping resolver (2026-10-31)

Question: users name states by their words ("switch the warp mode to FM", "arp range from 2 to 3"). Can the resolver read every state name and the value-to-text mapping in the plugin's own units, and how can a host switch Serum 2's arp patterns, which no parameter exposes?

Short answer: state names are now complete for every stepped parameter on this Mac (VST3 and AU), plus 10,466 continuous parameters whose text moves in steps. For 47 % of the automatable continuous VST3 parameters the text is a plain unitless scale or never changes; Serum 2's arp parameters are bipolar offsets whose text never shows the musical value, so they need a per-plugin table. Serum 2 arp patterns can only be switched by MIDI notes in a trigger octave (documented, not verified here) or by loading a preset; no host parameter does it.

## 0. Correction to the starting numbers

The harvest already stores the text of every step when a VST3 parameter has at most 64 steps (`vst3_harvest.cpp` samples `steps + 1` points then, 11 points otherwise). The 992 rows with more than 11 states are 989 parameters (the harvest has 21 duplicate stepped rows, same plugin, class and id); 655 of them already had every state, 334 in 22 plugins (steps > 64) lacked state names. On AU the harvest cut `value_strings` at 64: 799 indexed parameters in 35 plugins were incomplete.

## 1. Full state lists

`uv run probes/plugin-states/run_states.py` (build first: `probes/plugin-states/build.sh`). Same isolation as the harvest: one subprocess per plugin in its own process group, killed after 90 s, 3 workers, resumable. Loading code follows `tools/mapping/harvest/vst3_harvest.cpp` and `au_harvest.swift`; the harvest files are read, never written.

What it reads:
- VST3: every parameter with `stepCount > 0`, text for every step at `k / steps` (cap 4,096 states).
- AU: every `indexed` or `boolean` parameter or one with `valueStrings`, text for every integer from min to max (string(fromValue:), else the value string).
- Hidden steps, both formats: continuous parameters whose 11 harvest samples repeat a text or name a state (VST3 1,833 ids, AU 11,827 addresses) get a dense scan of 513 points, run-length coded. At most 64 text runs: stored as states at the middle of each run, with the run's range. More: counted as continuous, not stored.
- Program lists (VST3 `IUnitInfo`) and AU factory presets, into the per-plugin run record.

Output (`data/mapping/harvest/states/`, not committed): `vst3.jsonl`, `au.jsonl` (one line per parameter: plugin_id, class_name, index, id, name, steps, source `steps|dense`, `states` as `[normalized value, text]`, `ranges` for dense), `{vst3,au}_runs.jsonl` (one line per plugin with status, time, presets), `summary.json`.

| | VST3 | AU |
|---|---|---|
| plugins run (harvest status ok) | 405 | 886 |
| failures | 0 in run 1; run 2 (with program lists): 1 SIGSEGV (Surrealistic MG-1 Plus), ok on `--retry-failed` | 0 in both runs |
| wall time, 3 workers (run 1 / run 2) | 55 s / 58 s | 187 s / 231 s |
| stepped parameters | 8,422 | 16,601 |
| plugins with stepped parameters | 403 | 730 |
| states | 107,112 | 159,483 |
| parameters with more than 11 states | 989 | 1,500 |
| harvest rows that lacked states, now complete | 333 of 334 (22 plugins) | 798 of 799 (35 plugins) |
| state texts added over the harvest | 62,780 | 57,142 |
| truncated (more than 4,096 states) | 1 (Pro-Q 3 "Analyzer External Spectrum", 65,538 steps) | 1 |
| states with empty text | 320 | 998 |
| parameters where two states share a text | 447 | 745 |
| hidden-step parameters stored (dense) | 901 in 71 plugins | 9,565 in 423 plugins |
| of those with named text | 530 in 52 plugins | 7,655 in 373 plugins |
| dense scans rejected as continuous | 1,129 | 2,397 |

Every stepped harvest parameter has a state list (checked by joining on plugin, class and id: 0 missing in both formats). Numbers from `uv run probes/plugin-states/stats.py`.

Example, Serum 2 "A Warp Mode" (69 steps): the harvest knew 11 of 70 names. Now all 70 are there, and "FM" matches 18 of them: FM, FME and FML, each with B, C, Noise, Sub, Filter 1 and Filter 2 (raw 0.478 to 0.725). "switch the warp mode to FM" is ambiguous without a source word and has to show chips. Hidden steps found in Serum 2: "LFO n Delay" has 15 note values (Off, 1/256 ... ) while it reports `steps = 0`.

Programs as host-visible presets: 377 of 405 VST3 plugins return a program list, but only ValhallaShimmer names its programs (8, "BlackHole", "ConcertHall", ...). The rest are one empty or "default" program, or generic names (Serum 2: "Bank", 128 entries "Prog 1" ... "Prog 128"). AU factory presets: 724 of 886 components report some, 411 of them exactly one ("Init", "Untitled"); 309 have at least two real names (22,689 names in all), 258 of those from Waves, 27 Plugin Alliance, 8 Apple. Serum 2, Serum and Timeless 3 report none on AU.

## 2. Continuous parameters with generic text

`uv run probes/plugin-states/generic_text.py [--plugin P --class-name C]`, on the 11 harvest samples. Only parameters a host can automate count: VST3 `can_automate` and not hidden or read-only, AU writable, no per-channel MIDI proxy. That removes 688,507 of 707,821 continuous VST3 rows, 676,000 of them JUCE "MIDI CC n|m" proxies.

| class | VST3 (19,314 params, 381 plugins) | AU (82,867 params, 862 plugins) |
|---|---|---|
| plain unitless linear scale (0..1, 0..100, -100..100, -1..1, 0..10, -10..10, -50..50, 0..127) | 7,216 (37 %) | 24,700 (30 %) |
| text never changes | 1,885 (10 %) | 5,902 (7 %) |
| no text, generic unit, range 0..1 | | 4,928 |
| no text, generic unit, other range (value is a bare number) | | 18,966 |
| numeric with unit | 6,069 | 4,774 |
| numeric, nonlinear | 1,215 | 9,678 |
| linear unitless, other range | 1,354 | 8,742 |
| named text (hidden steps, see section 1) | 1,554 | 4,969 |
| partly empty / no text with typed unit | 21 | 35 / 173 |

Plain-scale ranges, VST3: 0..1 4,082, 0..100 1,357, 0..10 674, -1..1 651, -100..100 368. Generic text (plain scale or constant) on a parameter whose name promises a unit (rate, time, freq, range, octave, semitone, pitch, delay, attack, release, length, steps, voices): 2,071 VST3, 4,147 AU. 301 of 381 VST3 plugins have at least one generic parameter. Largest: Output Arcade 1,557 of 2,664, ShaperBox 3 589 of 589, Omnisphere 512 constant, Surrealistic MG-1 Plus 512, Serum 2 406 (both classes), Timeless 3 290 of 442.

Serum 2 (instrument class, VST3): 472 automatable continuous parameters; 203 plain scale (0..1 92, 0..100 72, -100..100 38), 6 constant, 192 named. 11 plain-scale parameters name a unit: Arp Rate, Range, Repeats, Retrig Rate, Velo Decay, Transpose, Wrap Transpose, Wrap Range, Clip Player Rate, Clip Player Transpose, Noise Pitch.

### Serum 2 arp parameters in pedalboard

`uv run probes/plugin-states/serum2_arp.py text state rate transpose`. pedalboard 0.9.26 hosts the VST3 and passes no tempo; at the default the arp steps every 0.125 s, which is 1/16 at 120 BPM.

Text. Arp Rate, Range, Shift, Offset, Repeats, Gate, Chance, Retrig Rate and Transpose report label "%" and range -100..100 (step 0.2). Writing 21 raw values from 0 to 1 reads every raw value back unchanged (0 of 21 snapped, per parameter) and `string_value` is the linear percent ("-50.0" at 0.25).

State. Serum's VST3 state is `XferJson\0`, a u64 header length, a JSON header, u32 body length, u32 version, then zstd-compressed CBOR; the header hash is md5 of the zstd bytes. A host write lands in the global block `Arp0`, not in the slot: raw 0.9 on Arp Rate stores `Arp0.kParamRate = 80`, raw 1.0 on Arp Range `kParamTransposeRange = 100`, raw 0.1 on Arp Gate `kParamGate = -80`. The slot values live in `ArpClip0..11.plainParams` in plugin units (`kParamShape` "Down", `kParamGate` 52, `kParamRate`, ...). Rate behaves as an offset over the slot's rate (0 % plays the init slot's 1/16, see below); that the other host parameters are offsets too follows from the same storage, not from a listening test.

Rate snaps to note values. Sweep raw 0.30 to 0.62 in steps of 0.01, chord held 6 s, pitch tracked every 5 ms: only 6 step lengths occur, each a power of two.

| raw | text | step | note value at 120 BPM |
|---|---|---|---|
| 0.30 - 0.35 | -40 .. -30 | 1.0 s | 1/2 |
| 0.36 - 0.41 | -28 .. -18 | 0.5 s | 1/4 |
| 0.42 - 0.47 | -16 .. -6 | 0.25 s | 1/8 |
| 0.48 - 0.52 | -4 .. +4 | 0.125 s | 1/16 (the slot's rate) |
| 0.53 - 0.58 | +6 .. +16 | 0.0625 s | 1/32 |
| 0.59 - 0.62 | +18 .. +24 | 0.031 s | 1/64 |

One note value per 0.06 raw (12 %). From raw 0.0 to 0.2 no step change happens within 4 s. The text says "-20.0" where the arp plays quarter notes.

Shift moves in semitones: about one per 2 % (raw 0.51 = 1, 0.56 = 6, 0.62 = 12, 0.66 = 16), measured with Range +20 %. Above +34 % the first repeat drops back to 12-15 semitones (wrap or clamp, not resolved). Range counts pattern passes, about one per 12 %: with Shift +20 %, 0-8 % plays 3 pitches (one pass), 12-20 % 6, 24-32 % 9. With Shift at 0 %, Range changes nothing audible: all 11 values from -100 to +100 play the same 3 pitches. On a slot with the default range, "arp range from 2 to 3" is a step from about +12 % to +24 %, and only if Shift is not 0.

What a per-plugin table needs for such a parameter:
- kind: offset over the selected slot's value (Arp0 over ArpClipN), so the absolute result depends on the slot; the slot value is readable from the plugin state (Serum: `ArpClipN.plainParams`), not from text
- unit and grid: note value (powers of two, with Dotted and Triplets as separate slot flags), semitones, pattern passes
- bins: raw interval per step, measured as above (Rate: 0.06 raw per note value around 0.5)
- dependencies: Range needs Shift ≠ 0; note values become seconds only with the host tempo
- limits: where the offset wraps or clamps (Shift above +34 %, Range above +36 %)
A table for one parameter takes about 1 minute of rendering and pitch tracking; the 11 Serum 2 parameters above are the demo set.

## 3. Switching Serum 2 arp patterns from a host

Serum 2's ARP module holds 12 arpeggiators per arp bank; each slot has its own shape (Up, Down, Converge, Pattern, ...), rate and playback settings, and a slot is selected by clicking it or by a key (Serum 2 User Guide, manual 1.0.0, pp. 244-264). Factory content on this Mac: 3 arp banks, 12 arp patterns, 1,030 presets, 59 with the arp on; 27 of those use 12 different slots. Shapes stored in the presets (`kParamShape`): Pattern 92, UpDown 13, Down 10, Played 8, DownUp 5, ConvAndDiv 4, DownAndUp 4, RandNoDup 3, ThumbUD 3, Converge 3, Chord 3, Rand 2, UpAndDown 2, RandOnce 2, ThumbUp 2, PinkyUD 2, RandDrift 1, Up 1, default 548.

| way | possible? | source |
|---|---|---|
| a host parameter for shape, slot or arp bank | no. The harvest has 15 arp parameters (Enable, Rate, Shift, Range, Offset, Repeats, Gate, Chance, Retrig Rate, Velo Decay, Velo Target, Transpose, Wrap Transpose, Wrap Range, Wrap Phantom Note), none selects a slot or shape | harvest; pedalboard parameter list (2,621 parameters) |
| VST3 program change ("Bank", 128 programs) | no effect: writing Prog 1, 6 and 41 leaves the audio bit-identical (difference -240 dB) and the preset name " - Init - " | `serum2_arp.py bank` |
| MIDI note in the trigger octave | documented: "MIDI Input Trigger Octave: select the octave from which notes trigger arps", default the lowest octave of the MIDI note range (notes 0-11); the 12 slots sit on the ARP keyboard like keys. The state keeps the selected slot as `Arp0.kParamActiveClipID` and the octave as `kParamMidiSelectOctave` (11 and 2 of 59 arp presets set them). Not verified here: on the init state, notes 0, 3, 5, 7, 11, 12 and 24 leave `kParamActiveClipID` unset, and slot contents written through the state come back as "default" (an oscillator level written the same way applies: RMS 0.088 to 0.006), so no test state had distinct slots | manual pp. 264, 266; `serum2_arp.py slots` |
| same for CLIP slots | documented: 12 clips, same "MIDI Input Trigger Octave" option in the CLIP menu | manual p. 241 |
| MIDI learn | for knobs and sliders ("MIDI Learn" in the right-click menu, saved with the preset or session); the manual does not say whether the SHAPE menu or the slot buttons can be learned. Not tested (needs the GUI) | manual p. 26 |
| load a preset | switches the whole arp bank with the sound, unless the ARP module is locked ("Lock Module ... when changing (sound) presets"); loading an arp bank alone is a GUI menu (BANK) | manual pp. 246, 266 |
| FL script | `channels.midiNoteOn(indexGlobal, note, velocity)` can send the trigger note to Serum's channel; `plugins.nextPreset/prevPreset/getPresetCount` and `plugins.getName(..., FPN_Preset, i)` walk the plugin's own program list, which for Serum 2 is "Prog 1" .. "Prog 128" | [FL Studio API stubs: channels/notes](https://il-group.github.io/FL-Studio-API-Stubs/midi_controller_scripting/channels/notes/), [plugins](https://il-group.github.io/FL-Studio-API-Stubs/midi_controller_scripting/plugins/) |
| write the plugin state | works for oscillator values in pedalboard, not for arp slots; FL scripts have no call to set a plugin's state | `serum2_arp.py slots` |

So a gesture can switch Serum 2 arp patterns by sending a note 0-11 on Serum's channel, if the loaded preset fills the slots. FL test that settles it: load the factory preset "ARP - Opus Arpeggio" (slots 1-12: default, Down, DownAndUp, UpAndDown, default, Down, Played, ThumbUD, Rand, DownUp, UpDown, ConvAndDiv; arp key zone from note 36), hold a chord above C2, play notes 0, 1, 2 from the piano roll and from `channels.midiNoteOn`, and listen for the default shape, Down, DownAndUp.

## 4. Spec proposal

Proposed text for the spec, items in the style of `probes/mapping-pipeline/SPEC_PROPOSAL.md` (not merged there; another session edits that file).

### map.states (new)

"The inventory holds the full state list of every stepped parameter as [normalized value, text], plus hidden steps: continuous parameters whose text moves in steps (dense scan, at most 64 runs, the middle of each run written). Built offline by `probes/plugin-states/run_states.py` for installed plugins; for a plugin inserted live, the FL script reads `getParamValueString` after writing each step (stepped parameters only, `steps + 1` writes)." Measured: VST3 8,422 stepped parameters in 403 plugins, 107,112 states; AU 16,601 in 730, 159,483; hidden steps 901 (VST3) and 9,565 (AU) parameters.

### target kind `state` (new, amends cond.value and cond.step)

"A target naming a state of a stepped or hidden-step parameter ('warp mode to FM', 'filter type ladder') resolves the state word against the parameter's state texts with the same matcher as map.resolver, writes the state's normalized value, and shows chips when several states match ('FM' matches 18 Serum 2 warp states). cond.step walks the same list." Needs map.states.

### target kind `slot` (new)

"A target selecting one of a plugin's note-triggered slots (Serum 2 ARP and CLIP: 12 each, trigger octave notes 0-11 by default) sends that note on the plugin's channel instead of writing a parameter. Per plugin: slot count, trigger octave, and slot names where the state can be read (Serum 2: `ArpClipN.name` and `kParamShape` from the XferJson state); else 'slot 1' .. 'slot 12'. 'switch the arp to down' binds to the slot whose shape is Down in the loaded preset; with none, the answer is 'no Down slot in this preset' and chips for the slots there are." Needs the FL test above before the demo.

### target kind `preset` (new, not for the demo)

"A target naming a preset loads it through the plugin's program list (`plugins.getName(..., FPN_Preset)`, `nextPreset/prevPreset`)." Measured: 1 of 405 VST3 plugins names its programs (ValhallaShimmer); Serum 2's 128 programs are generic and do nothing; AU factory presets: 309 of 886 components name at least two, 258 of them Waves. Too thin to promise; keep as an answer ('presets are not reachable for this plugin').

### map.unit_table (new)

"A per-plugin table for parameters whose text does not show their unit: kind (absolute, or offset over a slot value), unit and grid (note value, semitones, passes), raw bins per step, dependencies (Serum 2 Range needs Shift ≠ 0, note values need the tempo), wrap or clamp limits. The resolver uses it for 'rate to 1/8', 'range 2 to 3' and for cond.value. Tables come from a render sweep with pitch or onset tracking, about 1 minute per parameter; the demo set is the 11 Serum 2 arp and clip parameters." Measured: Serum 2 Arp Rate raw 0.48-0.52 = 1/16, one note value per 0.06 raw; generic text on 7,216 automatable continuous VST3 parameters (37 %), 2,071 of them with a unit word in the name.

## Files

- `vst3_states.cpp`, `au_states.swift`, `build.sh`: state dumpers (binaries in `data/mapping/bin/`)
- `run_states.py`: runner with crash isolation, `--retry-failed`, `--fresh`
- `stats.py`: section 1 numbers
- `generic_text.py`: section 2 classes
- `serum2_arp.py`: Serum 2 sections (text, state, rate, range, transpose, bank, slots, presets)
