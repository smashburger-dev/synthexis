# Session 2026-11-03: fine calibration, digest and handoff

Branch `claude/peaceful-hugle-02ba54`, merges 47 and 50 (`docs/merge-2026-11-02.md`). Numbers and rules: `probes/plugin-perception/RESULT.md`, sections "Openness top and two hands on x" and "Fist curve and two hands on x, second FL test". Knobs and lab workflow: `probes/plugin-perception/TUNING.md`. No model was retrained; everything here maps signals.

## What happened

1. Noa, FL test 11-03 evening: an open hand tops out near 0.85; with two hands on x the left hand cannot cross the right one. Rule written first (session notes, `fine-calibration-rule.md`), then built: openness above 0.6 stretched to reach 1.0; with both hands routed on x each hand's x maps onto its side of the person's face centre (Noa: relative to the person, not to the frame). Merge 47, with three clippy warnings from the settings merge.
2. Noa, FL test of that build: half-open hands pull to 0.5, a fist never reads 0, the right hand reads 0.5 next to the left one. Two causes. The decile map per finger squeezed linear values 0.31 to 0.58 into 0.5 to 0.6. The x rule asked for both hands on x, and Noa had left hand openness and right hand x. Second rule written first, then built (2e96761): no decile map, openness one line from `OPEN_LO` to `OPEN_HI`; x splits as soon as the other hand has any route, in any plugin instance, with a 0.08 dead band at the centre.
3. Noa, FL test of 2e96761: the x split works and feels right. The fist end is more abrupt than before (a small curl takes 0.8 to 0.4). Noa left it as is for now.
4. The MIDI auto-setup went to its own session (merges 48 and 49, `probes/fl-midi-autolink/RESULT.md`); the MIDI playback session runs from `docs/midi-session-prompt.md`.

## Where to change what

| you want to change | where | check with |
|---|---|---|
| at which pose a value reads 0 or 1 (openness, fingers, pinch, mouth, eyes, smile) | constants in `plugin/synthexis/src/engine/geom.rs`; table in `TUNING.md` | lab script on dumps (`lab/openlab.py` for openness), then the same numbers from the Rust bench |
| how fast or smooth every route follows | `Chain` defaults in `plugin/synthexis/src/cond.rs`: `Range`, `Median3`, `OneEuro`, `Occlusion`, `ReacquireSlew` | `lab/euro_med.py`, held poses against ramps |
| what a sentence sets on one route (direction, output range, curve, smoothing, amount) | `assign::routes_in` (`plugin/synthexis/src/assign.rs`): `inv`, `lo:`, `hi:`, `curve:`, `smooth:`, `amt:`; the parser emits them (`plugin/parser/src/resolve.rs`) | `assign.rs` tests |
| what a sentence cannot do live yet | `assign::routes_in` skips `mode:`, `hold:`, `quantize:`, `delta:`, `BIP` and says so in the row's note | the MIDI session builds part of this |
| "only while ..." gates | `GATE_OPEN` 0.5 and `GATE_MIN_CONF` 0.2 in `route.rs`; gate signals from a sentence in `assign::gate_signals` | `route.rs` tests |
| several routes on one target | `route::apply_sample`: each line is the mean of its routes, the target the clamped sum of its lines | `route.rs` tests |
| a rule that depends on other routes or on the person | see the next section | `route.rs` tests with `feed()` |
| a new perception value | feature in `geom.rs` `HandFeatures` or the face code, emitted in `engine/perception.rs`, id in `signal.rs` (`perception_ids`), source in `tools/mapping/ontology.yaml` for the parser and completion; signal contract in `RESULT.md` | bench dump, `eval.py` |
| routes that exist without the user | `default_routes`, `PERCEPTION_DEFAULTS` in `route.rs`: test routes, only with "Test signal" on | `fl_live.py` |
| what is saved with the project | `saved.rs` (kept sentence rows as JSON in the plugin state) | `saved.rs` tests |

## How a rule like the x split is built

`route::fit_hand_x` is the pattern for rules that change a route because of something outside it.

- It runs at the top of `apply_sample`, before conditioning, and only rewrites `cond.range.in_min` and `in_max` of the matching routes. The route itself, its sentence and its saved form stay as the user made them.
- Its inputs: the own route table (`hands_used`), the other plugin instances (`Values::hands_used`, filled in `writer.rs` from the process-wide table `HANDS_USED`; each `Writer` removes its entry on drop), and an internal signal.
- Internal signal: `SIG_BODY_X` (`cam0.body.x`, the face centre on x) is a constant in `signal.rs` but not in `perception_ids`, so the parser, completion and UI never offer it. `perception.rs` `emit_body` sends it; `apply_sample` absorbs it into `Values::centre` (smoothed, `CENTRE_TAU_S` 1 s; samples below `GATE_MIN_CONF` are ignored) and returns no target change.
- Test routes do not count as use. Tests sit in `route.rs` (`two_hands_on_x_*`, `any_route_on_the_other_hand_splits_x`, ...) and `writer.rs` (`instances_share_the_hands_their_routes_use`).

A new rule of this kind: a function next to `fit_hand_x`, called at the same place, its constants `pub` with a comment saying where the number comes from, a row in `TUNING.md`, and tests for the case Noa reported.

## How to measure a change

1. Write the rule before the run: candidates, data (S1 to S3, block and labels), gates, the tie-breaker. Put it in `RESULT.md` or the shared session notes.
2. Dumps: `probes/plugin-perception/lab/dump.sh`, or one bench call: `SYNTHEXIS_BENCH=<cam0.mov> SYNTHEXIS_PERCEPTION=hybrid SYNTHEXIS_BENCH_START=<s> SYNTHEXIS_BENCH_SECONDS=<absolute end s> SYNTHEXIS_BENCH_RAW=1 SYNTHEXIS_BENCH_OUT=<file> target/release/synthexis`. Recordings are in `data/recordings` (git-ignored, Noa's Mac only). Don't run cargo while several benches run: `guarded-cargo.sh` stops above load 40.
3. Score candidates in Python (`lab/`), change Rust, check the same numbers from the Rust bench, score S4 once.
4. Tests: `./guarded-cargo.sh test -p synthexis --release`, clippy `-p synthexis --release --tests`.
5. Install with FL closed. From the main checkout: `plugin/install.sh`. From a worktree: `./guarded-cargo.sh xtask bundle synthexis --release`, `codesign --force --deep -s -` on `target/bundled/synthexis.vst3` and `.clap`, copy both into `~/Library/Audio/Plug-Ins/VST3` and `CLAP`. Build from a tree that contains main: an install from an older branch silently rolls back other sessions' work (it happened on 11-03 at 23:21).
6. FL test with Noa. Offline numbers choose between candidates; Noa's feel decides whether a change stays.

## Open

- Fist end abrupt live. The line passes the middle as fast as the keypoint cues move; the decile map hid that behind its plateau at 0.5. The protocol ramps are fast open-and-hold moves, so no recording shows a slow opening. Next: record about 30 s of slow opening and closing, one hand, palm and knuckles towards the camera, then choose between the line, a softer S-curve and a blend with `lab/openlab.py` on that recording.
- Knuckles towards the camera: not in any recording. The 2D bend of fingers curled towards the camera barely changes, the reach term carries the change alone.
- Finger values are linear since 2e96761, so a finger reads 0.5 at its linear midpoint (before: at linear 0.31). Dev finger accuracy (fingers 1 to 4 above 0.5) holds at 0.965. Gates and future note triggers on fingers need hysteresis (`cond.hysteresis` is planned, not built).
- The x split covers `hand.L.x` and `hand.R.x` only; y and depth keep the whole frame (Noa: y needs no rule).

## Next session

The MIDI session is set up: `docs/midi-session-prompt.md` with `docs/midi-state-digest-2026-11-03.md` (read at 13da854, before merge 50). What it needs from here: finger values are linear since merge 50 (last bullet above), the x rule applies to x routes only, and any bundle it installs has to be built from a tree with main merged. A calibration session after the demo starts from the slow live recording above.
