# FL Studio stock plugins as route targets

Run 2026-11-13 on Noa's M1 Pro, macOS 27, FL Studio 2026 (script API 45), synthexis 0.1.1 from `main` cc7bbd2 plus this branch. All FL work ran in throwaway projects (copies of FL's `Empty.flp` and of its render-test projects); no project of Noa's was opened or changed.

## Why they failed

The FL side was never the problem. The bridge reads stock plugin names, writes their parameters and FL records those writes, for effects and generators alike (sections 1 to 3). Two things in synthexis made stock plugins look broken:

1. A sentence only ever resolved on the plugin the picker showed. The parser marks "on Fruity Filter" as a PLUGIN span, but `assistant::parse` resolved every route against the picked plugin and nothing read the span. Measured in FL with the old build: Fruity Blood Overdrive picked (FL focuses the last plugin opened), "spread my right fingers to open the filter on Fruity Filter" gave `right hand spread -> Fruity Blood Overdrive / Post filter`. A sentence naming a stock plugin worked only when that plugin happened to be the one in focus.
2. No labels. `known.tsv` had no Fruity rows, so every stock parameter got a param-sense guess.

Fix for 1 (`assistant.rs`, `sentence.rs`): the reply carries the PLUGIN span's text; when it names another plugin of the project (same name in any case, else one name containing the other, synthexis itself never), the picker moves to that plugin like a click in FL would, the sentence waits for its parameters and is resolved again there. The pin is not touched; the next plugin the user clicks in FL, or a pin change, takes the picker back. Tests: `a_plugin_the_sentence_names_is_found_in_the_project`, `a_sentence_naming_another_plugin_moves_the_picker_until_fl_focus_moves`.

Fix for 2: hand labels for the 56 stock effects with up to 60 parameters (section 4).

Smaller fix: FL Slayer, WASP, WASP XT and Fruity LSD report parameter names made of control characters (`"\x13"`, `"p\x17"`) and ignore every write. `fl::is_placeholder` (and the Python twins in `export_paramsense.py`, `fl_param_counts.py`) now drop names with control characters, so the picker no longer lists them.

## 1. Names and writes, every stock plugin

`run_stock.py` loaded each stock plugin preset from FL's plugin database (`open -a "FL Studio 2026" <preset>.fst`: effects land in the next free master slot, generators in a new channel), ten per throwaway project, then for each plugin dumped its parameters and wrote its first 16 named parameters the way the bridge does (REC event for mixer slots, `plugins.setParamValue` for generators) and read them back 0.6 s later.

| | Count |
|---|---|
| Stock plugins loaded (30 skipped: notebooks, meters, visualisers, VFX MIDI tools, Wrapper, Edison, Control Surface) | 109 (64 effects, 42 generators, 3 without parameters) |
| Named parameters dumped | 16,160 |
| Plugins whose every tested write moved the value | 99 of 106 |
| Writes that moved the value | 1,155 of 1,233 |

The rest:

- Patcher, Wave Traveller, Soundgoodizer: 0 parameters through the script API, so no route can reach them.
- FL Slayer, WASP, WASP XT, Fruity LSD: control-character names, no write moves anything (dropped now, see above).
- GMS: 12 of 16 stuck, all of them the FX slot selectors and FX Activate.
- Fruity Scratcher: Mute did not toggle (13 of 14 moved).
- Fruity Love Philter: 0 of 16 in the batch; a single REC write to Master level afterwards moved it from 0.65 to 0.20 within 0.6 s. A timing effect of the batch, not a dead path.

Per-plugin rows: `runs/writes.jsonl`; logs `runs/full*.log`.

## 2. The bridge's own write handler, with FL recording

`rec_test.py` runs `_handle` of `plugin/fl/device_synthexis_bridge.py` inside FL, so the test goes through the shipped code, name check included. Record on, 3 s ramp 0 to 1 at 20 Hz on the 128 steps the route writer sends, then playback with the parameter parked at 0.5, sampled every 0.1 s:

| Plugin / parameter | Path | Write 0.2 reads | Wrong plugin name | Playback before recording | Playback after |
|---|---|---|---|---|---|
| Fruity Reeverb 2 / Wet level (Track 1 slot 0) | REC event | 0.2 | refused, value unchanged | 0.2 flat (29 samples) | 0.00 to 1.00, 29 distinct |
| 3x Osc / Osc 2 mix level (channel 0) | `setParamValue` | 0.2031 | refused, value unchanged | 0.5 flat | 0.016 to 0.984, 29 distinct |

FL records the bridge's writes on stock plugins as automation. `transport.isRecording()` read 0 while recording, as in `probes/fl-target-picker`; the test toggles the record button back blindly.

Focus: `mixer.focusEditor(0, 6)` on Fruity Love Philter gave `ui.getFocusedPluginName()` "Fruity Love Philter" and `mixer.getActiveEffectIndex()` (0, 6), which is what the bridge's focus event sends, so the picker follows a clicked stock effect.

## 3. In synthexis, end to end up to the camera

Release build of this branch in FL, project from FL's "Fruity Reeverb 2" render test plus Fruity Filter, Delay 3, Limiter, Parametric EQ 2 and Blood Overdrive in master slots 0 to 4, picker following FL (last focused: Blood Overdrive). Each sentence typed into synthexis:

| Sentence | Picker moved to | Route |
|---|---|---|
| spread my right fingers to open the filter on Fruity Filter | Fruity Filter | right hand spread -> Fruity Filter / Cutoff frequency |
| tilt my head left to add more delay on Fruity Delay 3 | Fruity Delay 3 | head roll -> Fruity Delay 3 / Output wet |
| raise my left hand to push the drive on Fruity Blood Overdrive | (already there) | left hand y -> Fruity Blood Overdrive / Pre amp |
| close my right fist to squash it harder on Fruity Limiter | Fruity Limiter | right hand openness inverted -> Fruity Limiter / Limiter ceiling |
| open my mouth to boost the bass on Fruity Parametric EQ 2 | Fruity parametric EQ 2 | mouth open -> Fruity parametric EQ 2 / Band 1 level |
| open my right fist to open the reverb on Fruity Reeverb 2 | Fruity Reeverb 2 | right hand openness -> Fruity Reeverb 2 / Wet level |

The log shows each plugin's classes as "N labelled, 0 by param-sense". Without a side ("open my fist ...") synthexis asks "which hand? say left or right", as for any plugin. In an earlier run the Reeverb 2 route was kept ("Fruity Reeverb 2 · Wet level" on the right hand).

Not tested: a real hand moving the kept route. Nobody sat in front of the camera during the run, synthexis has no camera choice, and the test signal drives only the Signal macros. Every link after the camera is measured above (route bound, bridge handler writes and is recorded); the openness signal itself is the one Noa used on 11-03 for Topos. To close it: insert synthexis and Fruity Reeverb 2, say "open my right fist to open the reverb on Fruity Reeverb 2", Keep, press record in FL, open and close the right hand.

Seen in passing: with six suggestions at once the list cuts off the sixth line and the Keep / Discard row (window 720 x 520); not stock-specific, not changed here.

## 4. Labels

`tools/mapping/fl/stock_labels.py` holds hand labels for the 56 stock effects with up to 60 parameters (680 parameters, Fruity Reeverb 2, Delay 3, Filter, Limiter, Parametric EQ 2, LuxeVerb, Maximus, Vintage Chorus and Phaser, Transient Processor, Gross Beat, Vocodex and the rest); `export_paramsense.py` reads `stock_labels.tsv` into `known.tsv` (887 -> 943 plugins, 731 lines added, nothing else changed). A pure dry level gets `other`, not the mix class: on a class tie the resolver binds the first parameter, and Dry level (10) comes before Wet level (12) on Reeverb 2, so "open the reverb" would have raised the dry signal. Synths and the big racks (Sytrus, Harmor, Love Philter, Delay Bank, Distructor, Hardcore, Pitcher, ...) keep param-sense.

Why by hand: the planned paid run (`label.py --harvest fl --priority`, dry-run estimate 2.26 USD plus adjudication, run with `--max-usd 5`) got HTTP 400 "Model version ... is deactivated" from Baseten for `meta-models/muse-spark-open-1026` on every job, 0.00 USD spent; the same key gets 403 for every other Baseten model. Re-activating the deployment is Noa's call in the Baseten console. A DeepSeek backend for `spark.py` (`SYNTHEXIS_SPARK_BACKEND=deepseek`, same guards, peak prices) answered a probe (0.00006 USD) but this session was not cleared to use that key for the labelling run, so the run did not happen. With either backend available:

```bash
SYNTHEXIS_BASETEN_CAP=87 uv run tools/mapping/label.py --run label-fl-prio-v1 --harvest fl --priority --max-usd 5
```

then add the run to `LABEL_RUNS` in `export_paramsense.py`; label runs win over the hand table per plugin.

## Commands

```bash
# dev channel (probe only: the dev script runs any code it is sent)
cp probes/fl-stock-plugins/device_synthexis_dev.py "$HOME/Documents/Image-Line/FL Studio/Settings/Hardware/sxdev/"
uv run probes/fl-stock-plugins/fldev.py serve &        # before FL starts
uv run --with python-rtmidi==1.5.8 python probes/fl-stock-plugins/run_stock.py
uv run --with python-rtmidi==1.5.8 python probes/fl-stock-plugins/rec_test.py
uv run --with python-rtmidi==1.5.8 python probes/fl-stock-plugins/rec_test.py --ch 0 --plugin "3x Osc" --param 6
uv run tools/mapping/fl/import_fl_dump.py ~/Documents/synthexis-harvest/20261113-stock-merged.json
python3 tools/mapping/fl/stock_labels.py ~/Documents/synthexis-harvest/20261113-stock-merged.json > tools/mapping/fl/stock_labels.tsv
uv run tools/mapping/export_paramsense.py
cd plugin && ./guarded-cargo.sh test -p synthexis      # 471 passed, 1 ignored
```

The dump itself (`~/Documents/synthexis-harvest/20261113-stock-merged.json`, 109 plugins) and `data/mapping/harvest/fl/` stay out of git like the other harvests.
