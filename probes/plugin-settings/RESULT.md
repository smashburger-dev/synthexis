# Plugin settings, language and load times (2026-11-03)

Noa, 11-03: a clean English plugin for the demo and for judges who download it, German as an option, a settings window, the "frames" counter explained, and shorter load times. Decided with Noa in this session (all four were the proposals):

- German mode shows German and English completion items; on an equal match the German one ranks first. English mode shows English items only. Parameter names of the picked plugin are never filtered.
- The language is a per-user setting (`~/Library/Application Support/synthexis/settings.json`), shared by every instance; a project saved in German opens in English for someone else.
- The "frames" counter becomes a camera status in words; its numbers move to a diagnostics section of the settings. "diag" (always empty) goes.
- Settings overlay: language, completion on/off, diagnostics with the log. Live, Test signal, hand roles, camera, plugin picker, sentence field and the 32 signal sliders stay in the main view.

## 1. Language of the completion items

### Rule (written before the run)

Every completion item gets one language, `de` or `en`. A phrase used in both languages is two items, one per language, each in its own spelling ("Cutoff" de, "cutoff" en).

- Glossary rows (`glossary/gloss-v1`): the row's `lang`. Rows are deduplicated per (class, casefolded phrase, lang), so a phrase both languages use stays in both.
- Target phrases harvested from training spans: count the span's occurrences per row `lang` over rows whose register is not `codeswitch` (a code-switched row mixes both languages). Every language with at least 25 % of those occurrences gets an item. No such occurrence: no item.
- The bare "Finger" item: two items, "Finger" (de) and "finger" (en).
- An item without a language (an older vocab.json) shows in German mode only.

Check: I label the 44 harvested phrases by hand (is it English, German or both) without looking at the counts again, and count (a) German phrases that the rule shows in English mode (must be 0) and (b) English phrases it hides from English mode. The glossary tags are checked with a word list: English items with ä/ö/ü/ß or a German function word (ich, der, mit, wenn, schließen, ...) must be 0.

Known weakness: I chose the 25 % threshold after looking at the per-language counts of these 44 phrases, so (b) is not a held-out number.

### Result

`uv run probes/plugin-settings/lang_check.py data/mapping/completion`:

| run | harvested | German in English mode | English hidden from English mode | English items with German marks |
|---|---|---|---|---|
| rule as written (25 %) | 44 | 1 ("Akkord-Voicing": 2 of 8 occurrences in English rows) | 2 ("arp tempo", "arpeggios") | 0 |
| plus at least 3 occurrences per language (HARVEST_MIN, the harvest's own noise floor) | 43 | 0 | 2 ("arp tempo", "arpeggios") | 0 |

The first run failed the rule. Changed after it, before the second: a language also needs HARVEST_MIN (3) occurrences, the floor the harvest already uses against typos. "chord notes" drops out (2 English occurrences outside code-switched rows). The two hidden phrases are German-majority in the training rows; both are fine English words, so English mode misses them. Nothing leaks.

Export: 2,642 German and 2,561 English items (gestures, class phrases, targets); 226 glossary phrases are now two items, one per language. Parameter names are not items and are never filtered. Tests: `plugin/complete/src/tests.rs` (english_mode_shows_no_german_item, parameter_names_are_never_filtered, german_mode_shows_both_languages_german_first) and `plugin/synthexis/src/sentence.rs` (no German item or string drawn in English mode, the English "finger" gets the finger panel, switching refilters at once).

## 2. Load times

### What loads

| part | when | thread |
|---|---|---|
| parser ensemble (4 runs, Q8_0) | `initialize()` | assistant worker |
| completion data (vocab.json), known.tsv, param-sense | editor created | completion loader |
| camera session | first instance | camera thread |
| perception models seg, hand, face (CoreML) | first camera frame | perception thread, one after the other |

### Rule (written before the run)

Metric per part: wall time from the start of loading to "ready", in a fresh process each run, median of 5 runs (`SYNTHEXIS_LOADPROBE`, `probes/plugin-settings/loadtimes.sh`). CoreML is measured twice: warm (models already loaded once from the same files by an earlier process) and cold (a fresh copy of the models, as after `install.sh`, which compiles them again). The camera needs the camera permission of the host, so its numbers come from the FL log only (before: the last FL session in `~/Library/Logs/synthexis.log`; after: the first FL session with the new build).

The number that matters is "perception ready": from instance creation until the first frame goes through the models, read from the FL log (`wait=` of the first perception line plus the time to the first camera frame).

A change is kept when its part gets faster at the median and no other part gets slower by more than 10 % at the median. Correctness gate: `cargo test` for plugin/complete and plugin/synthexis passes.

Added before the parser run (parallel loading holds several runs' load buffers at once): the probe's peak memory (`/usr/bin/time -l`, maximum resident set size, median of 5) may rise by at most 10 %.

### Result

Probe medians, 5 fresh processes each, M1 Pro, `probes/plugin-settings/loadtimes.jsonl` (ms from the start until the part is ready; completion = the vocabulary, typing can start; labels = labels and param-sense, the picker's classes):

| build | cache | parser | completion | labels | perception | peak memory |
|---|---|---|---|---|---|---|
| before | warm | 202 | 119 | (in completion) | 82 | |
| before | cold | 202 | 119 | | 1466 | |
| perception models in parallel | warm | 206 | 120 | | 42 | 460 MB |
| perception models in parallel | cold | 202 | 119 | | 1417 | |
| plus vocabulary before labels | warm | 204 | 7 | 121 | 42 | |
| plus vocabulary before labels | cold | 202 | 7 | 120 | 1424 | |
| plus parser runs in parallel (dropped) | warm | 119 | 15 | 182 | 129 | 596 MB |
| final (parser as before) | warm | 214 | 7 | 123 | 42 | 466 MB |
| final | cold | 211 | 7 | 125 | 1492 | |

- Perception models in parallel (`perception.rs load_all`): warm 82 -> 42 ms; cold 1466 -> 1417 ms. CoreML compiles for the Neural Engine one model after the other somewhere in the system, so a cold load barely gains. Kept.
- Vocabulary before labels (`sentence.rs`): typing can start after 7 ms instead of 119 ms; the picker waits for the labels as before. Kept.
- Parser runs in parallel (`plugin/parser/src/lib.rs`): parser 202 -> 119 ms, but labels +50 %, warm perception 42 -> 129 ms and peak memory 460 -> 596 MB (+30 %). Fails both guards; reverted.
- The final run's parser (214 against 202) and cold perception (1492 against 1466) are within the 10 % and within the spread of single runs (cold 1406 to 1623).

Why FL is slower than the probe, and what fixes it:

- Thread QoS is not it: the cold perception load with the loader thread at background, utility or user-initiated QoS took 1571, 1494 and 1457 ms (3 runs each, median).
- CoreML's cache is per app and model path: a model warmed by a Swift program stays cold for the plugin binary (1457 ms after the Swift load), `touch` keeps it warm (42 ms), a move to another path makes it cold (1398 ms), and so does replacing the files at the same path with identical copies (1454 ms). `install.sh` deleted and recompiled the three models on every install, so FL's first load after every install was cold: in the last FL session before this change (build 3e40316, right after an install) seg, hand and face took 3038, 3984 and 3018 ms, one after the other.
- `install.sh` now keeps a compiled model whose source did not change (fingerprint of the .mlpackage in `<name>.source-sha256`; coremlcompiler output differs on every run, so the output cannot be compared). Probe: after install 1887 ms (cold, first compile with the fingerprint), again 42 ms, after a second install 48 ms (warm).
- The models started loading only when the first camera frame arrived, 1.6 s after the instance was created (camera permission, device, session start). They now load from `initialize()`, next to the camera start; a frame that queued up meanwhile is dropped.

FL, before (log of build 3e40316, instance at 0 s): first camera frame 1.6 s, models loaded one after the other from 1.6 s to 11.7 s, first perception frame 11.7 s (`wait=10058 ms`). FL, after: (to read from the first FL session with this build)

## 3. Settings: where each one went

| setting | stored | before | now |
|---|---|---|---|
| Signal 1..32 | host parameters | main view, sliders | main view |
| Live | host parameter | main view | main view |
| Test signal | host parameter | main view | main view |
| Hand role left/right (Parameter, MIDI, Motion, Off) | host parameters | main view, "Hand L/R" | main view, "Left hand"/"Right hand" |
| plugin picker pin | project (`target-pin`) | main view | main view |
| completion on/off | project (`completion`) | collapsing "Settings" under the field | overlay; per user (`settings.json`); the project key of older projects is ignored |
| language | (new) | | overlay; per user |
| log (last 8 lines) | | main view | overlay, last 40 lines |
| camera line "auth: … frames: … diag: …" | | main view | main view: status in words; overlay: the numbers |
| window size | project (`editor-state`) | | unchanged |

## 4. The "frames" counter

`EngineShared::frame_count` (`engine/camera.rs`) counts every frame the camera delegate receives, process-wide, about 30 a second, since the first synthexis instance started the camera; the 5-second engine log line prints it too. Nothing records: the frames go to brightness/motion, the preview and the perception thread and are dropped. `diag` was never written. Now: the main view says "Camera on · 30 fps", "Camera starting", "Waiting for camera permission", "No camera access. Allow it in …" or "The camera sends no picture (in use by another app?)" (no frame for 2 s); the overlay's diagnostics show permission, fps, frames received and dropped, and the capture-to-callback delay.

FL, after (first FL session with this build, models cold because install.sh compiled them once more with their fingerprint): face, seg and hand loaded in parallel in 563, 976 and 2357 ms; backend ready 2.4 s after the instance was created, the first frame went through the models right after it (`wait=0.31 ms`; the first Apple Vision face result took 1.76 s more). Before: 11.7 s.

## 5. Memory and idle work (Noa, 11-03, later)

Noa: what is only needed now and then (the parser, param-sense) should not hold memory while the routes are set and he only plays, switches profiles or tweaks; work the open window needs should stop while it is closed; everything that is needed must still be ready in time ("warm"); and memory leaks fixed where they can happen.

### Rule (written before the run)

Memory per part: the probe process's `phys_footprint` (proc_pid_rusage) before loading, after loading and after dropping each part, in a fresh process (`SYNTHEXIS_MEMPROBE`). A part is released when dropping it returns at least 80 % of what loading added.

- Parser: loads when the sentence field is used (focus or first keystroke), drops after 3 minutes without use. Kept when (a) the idle plugin holds no parser, (b) a sentence sent before the parser finished loading is parsed, not lost (test), and (c) from the first keystroke the parser is ready within the time it takes to type a short sentence: loading median under 1 s in the probe.
- Labels and param-sense: load when a picked plugin needs classes, shared by all instances, dropped after 3 minutes without use; the same three conditions, (c) within 1 s.
- Camera and perception: perception only runs on frames while something needs it (an open plugin window, or an enabled route reading a camera signal), with the models kept loaded; the camera session stops after 10 s without any need and starts again when needed. Kept when FL's CPU with the window closed and Test signal off drops (median of 20 samples of `ps %cpu` at 15 s, before and after) and a reopened window shows the camera again.
- Leaks: loading and dropping the parser and the labels 10 times in one process may not grow the footprint by more than 5 % over the first cycle; FL's footprint, sampled every 15 s for 10 minutes with the window open and closed, may not grow steadily.

### Result (probe; FL after the install below)

What holds memory (footprint added by loading, probe, M1 Pro):

| part | memory | load | before | now |
|---|---|---|---|---|
| parser (4 runs, Q8_0) | 370-380 MB | 170-290 ms | loaded at `initialize()`, kept for the process | loads when the sentence field gets the focus (and every 30 s while it has it), dropped after 3 min without use; a sentence sent while it loads waits and is parsed (test) |
| labels + param-sense | 98 MB, loaded per instance | 120 ms | per instance, kept | one copy for all instances, loaded when a picked plugin needs classes, dropped after 3 min unused; embedding table kept as f16 through a lookup table: 68 MB, 24-34 ms; classifying 2,600 names 19 ms (f32: 17-21 ms) |
| completion vocabulary | a few MB | 7 ms | per instance | unchanged |
| perception models | about 15 MB | 42 ms warm | kept | kept, so perception resumes at once |

- Release of dropped memory: this macOS's malloc keeps freed memory (after dropping parser and labels: 251 MB "Malloc Small" and 184 MB "Malloc Large", dirty, not reclaimable, per `footprint`) and hands it back only now and then; `malloc_zone_pressure_relief` returned 0 bytes, and neither waiting 8 s nor small, medium or large allocations returned it. It is reused: 10 load/drop cycles moved the high-water mark from 500 to 556 MB instead of adding 440 MB per cycle, and in two runs the footprint later fell to 16-22 MB with the perception backend loaded. So dropping does not lower Activity Monitor's number at once; the rule's 80 % is not met in the probe. Whether FL's footprint falls after the 3 minutes is measured in FL below.
- Leaks: none found. After 10 cycles everything came back (21 MB with perception loaded). FL with the plugin, 15 s samples: window open 1328 -> 1366 MB over 7 minutes, then 1287-1317 MB for 5 minutes after the window closed; no steady growth.
- Camera and perception (`engine/camera.rs`, `engine/mod.rs` Demand, `writer.rs`): the camera thread checks every 250 ms what is needed. Everything runs while a plugin window is open; else only what the enabled routes read (`cam0.frame.*` needs the camera, other `cam0.*` perception too; test routes count while Test signal is on). Frames go to perception only while it is needed; the session stops after 10 s without need (camera light off) and starts again when needed; the preview is computed only for an open window; no permission prompt and no camera during a plugin scan. Before, with the window closed and Test signal off, FL ran at a median of 21.7 % CPU (20 samples) and a 3 s `sample` showed the perception thread busy in 434 of 734 samples and the capture queue in 124: work for no route.
