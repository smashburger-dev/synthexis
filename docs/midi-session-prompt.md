# Prompt for the MIDI session (gestures to live notes, patterns, piano roll)

Open in a fresh worktree. First fast-forward the branch to local `main` and check that it contains commit 13da854 and the commit that added this file (branch `claude/zen-mendel-39ae7e`); if `main` lacks them, merge `refs/heads/claude/zen-mendel-39ae7e`.

---

You work on synthexis, the camera-to-music plugin for FL Studio. Answer Noa in German, address Noa by name, write code, comments, commits and docs in English, load `unslop` first.

**Why this session.** Noa's request (11-03 night, paraphrased): find out whether hand gestures can trigger moves that play MIDI in real time; the sentence field and a menu should offer it; the plugin needs its own piano roll or step system where you write the notes each action or interaction plays, running as a sequence. The Pong preset is built on it and Noa wants it in the demo. Next to parameter control this is the important part; after it only the routing view is missing. Noa explicitly wants you to think further than this prompt.

**Deadline.** Submission 11-04 21:00 ET (11-05 03:00 CET), demo video before. Get one vertical slice working in FL first (one finger plays a pattern, one hand plays chords, recorded in FL), then widen.

**Read first:**
- `docs/midi-state-digest-2026-11-03.md`: what exists, with file:line (Noa's decisions, spec nodes, code, assistant, gaps, Pong). Check a line before relying on it.
- `docs/ux-plugin.md` (roles, hand cards, moves list, presets, profiles, fixed width, glass), `spec/synthexis.yaml` nodes `gest.*`, `midi.*`, `tgt.midi_*`, `bridge.midi_out`, `rec.midi`, `motion.primitives`, Pong and Orb (lines 199 to 224), `ui.pattern_editor`.
- `plugin/synthexis/src/lib.rs` (`process()`, MIDI config), `route.rs`, `writer.rs`, `assign.rs`, `saved.rs`, `signal.rs`, `cond.rs`, `fl.rs`, `plugin/fl/device_synthexis_bridge.py`.
- `probes/fl-midi-autolink/RESULT.md`: the bridge sets itself up; never rename the ports.
- `AGENTS.md`: before any UI work load the skills `design-glass`, `design-rust` and `build-rust`.

**Done when, in FL with Noa:**
1. Right-hand finger states play patterns (hi-hat basic, trap roll) and left-hand fingers play chords in a key, on an FL channel picked in synthexis, started on the host grid, recorded by FL into the pattern or piano roll; nobody opens MIDI or wrapper settings; no stuck note in the edge cases you tested.
2. Those patterns and chords are written in synthexis' own step grid or piano roll, per finger or action, and saved with the project.
3. A typed sentence makes such a row ("Daumen hoch spielt die Hi-Hat", "linke Hand Akkorde in A-Moll") and completion offers the user's pattern names, notes and chords.
4. `./guarded-cargo.sh test -p synthexis` and `clippy -p synthexis --release --tests` are green (parser and complete tests too if touched); spec statuses, `docs/ux-plugin.md` and `docs/roadmap.md` updated, `docs/session-<date>-midi.md` written.
5. Pong has defaults agreed with Noa and is built, or is planned precisely enough for its own session.

**Getting notes into FL (research and this Mac, 11-03; hypotheses unless marked measured):**
1. Plugin MIDI out (VST3 event bus or CLAP note port, sent in `process()`): sample-accurate and in host tempo, but FL passes it only through the wrapper's MIDI Output port to channels with the same Input port. No script API reads or sets wrapper ports (through API v45), no REC event, no VST or CLAP extension does it; a saved channel state (.fst) keeps them. Reports on VST3 MIDI out in FL disagree; CLAP may do better. Clashes with "no MIDI settings".
2. Notes on the "synthexis" CoreMIDI source. FL already enables that input with the bridge script (port 236, measured 11-03). Notes the script leaves unhandled go where controller notes go: the selected channel and channels whose MIDI input port is 236, recorded like a keyboard (documented). Handled with `channels.midiNoteOn(channel, note, velocity)`, the script can pick the channel from the inventory, but recording and timing of `midiNoteOn` are undocumented. Zero setup either way.
3. A "note" command in the SysEx JSON protocol calling `channels.midiNoteOn`: same target choice, runs in `OnSysEx` on the CoreMIDI thread under the GIL; protocol 5.
4. Idea: CoreMIDI packets carry timestamps. A schedule computed in `process()` from the host transport could leave the source with future timestamps, if FL honours them.
5. FL's Smart Disable may stop `process()` of a silent generator; nih-plug's `ProcessStatus::KeepAlive` exists.
6. FL calls MIDI 60 "C5". A sentence saying "C4" may mean 60 or 48; settle it with Noa and show the FL name in the UI.

Measure paths 1 to 3 (4 if cheap) in FL before building on one: note sounds on the target channel, latency, jitter against the grid, recorded into the pattern while FL records, note-offs never lost, no FL freeze under 30 notes per second. Write the rule with pass criteria into `probes/fl-midi-notes/RESULT.md` before the run. Reuse `probes/fl-midi-autolink/midiprobe.swift` (virtual ports, logs, SysEx) and `plugin/fl/test/fakefl.swift` with `fake_fl.py`.

**Build, suggested order (change it when a measurement says so):**
1. Triggers that do not chatter: per-finger states and a hand-shape mask with hysteresis and minimum dwell (`cond.hysteresis`, 60 ms), finger count, a velocity source (speed). Finger accuracy is 0.892 against a 0.95 gate, so show the debounced state live; Noa has to see why a note fired.
2. MIDI targets: `Target` gains note, chord, pattern, CC and generator-property variants with edge events (state enter = on, leave = off) next to continuous values; every match site learns them (`route.rs`, `writer.rs`, `saved.rs`, `assign.rs`). A lock-free queue carries events from the engine thread to the player.
3. Pattern model and player: clips of 1 to 8 bars (note, velocity, start, length); modes gate (plays while held), latch, one-shot; chords in key (key, scale, a degree per finger, voicing, octave); arp rate and octaves from spread or finger count. Host transport (tempo, beat position, playing) read per block, starts quantized to 1/16 or 1/4, per-instance voice bookkeeping. Persisted with the project (versioned JSON) and inside profiles.
4. Piano roll or step grid in the plugin, inside the fixed width and the glass language: a lane per finger or action, 16 steps per bar, 1 to 8 bars, click and drag to paint, rows locked to the scale, preview on click, live playhead, templates (hi-hat basic, trap roll, snare fill, kick, I-V-vi-IV in key).
5. Assignment UX: the hand card's MIDI role shows thumb to pinky and named shapes, each with a thumbnail of what it plays; a click opens the editor. Moves-list rows get the target entry "synthexis MIDI: note, chord, pattern, CC".
6. Sentence field: `resolve.rs` keeps NOTE, CHORD, TIME, VALUE, grid, key and scale values (dropped today); `assign.rs` turns MIDI targets into routes; preset, role and scale intents get handlers; completion learns the user's pattern names, note names, chord symbols, keys, scales and FL channel names. Known parser miss: a finger showing a pattern gets a value (group 10).
7. Pong, on top of 1 to 3: throw detection (speed peak, then release or stop; direction; border position), a projectile that bounces, a border ring split into segments that play notes in scale or fire ramps. No doc defines the defaults (segment count and size, what each wall plays, physics, projectiles at once, quantizing with the transport stopped, the zone editor): propose them to Noa.

**Edge cases, each a test or a written decision:** note-off on route remove, Discard, undo, Live off, profile switch, transport stop and loop jump, deactivate, reset and unload, hand lost (occlusion holds 600 ms), link down mid-note (paths 2 and 3: all-notes-off on reconnect), two routes on one note (count holders), several synthexis instances on one engine, bypass, buffer or sample-rate change, retrigger while held, quantizing with the transport stopped, FL's C5 naming, drum notes for FPC.

**Think beyond this prompt.** You have the whole repo, a working camera-to-FL chain and Noa's taste in `docs/ux-plugin.md`; trust your design judgement. Find the fastest way for a person to lay out what each finger or action plays: perform it into the grid with gestures or the computer keyboard, pick a template and bend it, drag a clip, or something better. Show Noa two or three sketches (ASCII, FigJam or an Artifact mock) before building the editor. Ask short questions only where the answer changes what you build, then build.

**Rules.** Write a rule before every measurement run. Commit, push and delete only with Noa's OK. FL's settings files (`~/Library/Preferences/Image-Line/reg.xml`) are Noa's system configuration: read them, never edit them. Install for FL tests: `./guarded-cargo.sh xtask bundle synthexis --release`, `codesign --force --deep -s -` on the .vst3 and .clap, copy while FL is closed (not `plugin/install.sh` from a worktree); a changed FL script goes to `~/Documents/Image-Line/FL Studio/Settings/Hardware/synthexis bridge/` while FL is closed, keeping its header lines. The workspace denies `unwrap`, `expect` and `panic`, tests included. Plugin log: `~/Library/Logs/synthexis.log`. When the branch is ready, tell the merge-main session (`local_7b3e6f19-45a1-49fc-aeb3-24a7f613356a`). Independent pieces (Pong motion primitives, for one) may run in their own sessions, after Noa agrees.
