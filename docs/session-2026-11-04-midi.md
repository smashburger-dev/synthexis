# Session 2026-11-04: gestures to live MIDI

Branch `claude/midi-playback-411997` (from local main c1094be). Brief:
`docs/midi-session-prompt.md`. Nothing committed yet.

## Measured

`probes/fl-midi-notes/RESULT.md` (rule written before the run, FL 26.1.7):
notes from the "synthexis" port reach the FL channel synthexis names, with
nobody in MIDI or wrapper settings, through the bridge script's
`channels.midiNoteOn` (as 3-byte notes per slot or as JSON note commands).
16 of 16 recorded into that channel's pattern while another channel was
selected; delivery p95 0.42 ms; 600 notes in 20 s with every ping answered
(p95 0.90 ms playing); no hanging note; FL honours CoreMIDI timestamps.
The plugin's own MIDI out (needs wrapper ports by hand) is not needed.

## Built

- `midi.rs`: key and scale, chords on degrees (register kept, strum),
  parts (clip with ratchets and nudge, chords per bar, single note), modes
  (while shown, on / off, once), starts (now, 1/16, beat, bar), templates,
  Euclid fill, the store saved with the project (`midi` state), and the
  player: one thread per instance, host transport from process() through a
  lock-free sequence, its own clock while FL stops, bar-aligned clips,
  holders per route, note-offs on release, route removal, Discard, Live
  off, deactivate, host jumps (start, stop, loop, seek) and drop.
- `route.rs`: `Target::Part { id, trigger }` with `Trigger::Shape {mask,
  care}` (a finger shown or a whole shape) or `Trigger::Above`; trigger
  edges with hysteresis 0.55 / 0.45, 60 ms dwell and a 600 ms hold when the
  hand is lost; a closed gate ends the note.
- Engine: signal `cam0.hand.<side>.shape`, the debounced 5-bit mask from the
  student's finger probabilities (`ShapeGate`, 0.6 / 0.4, 60 ms); skeleton
  joints for the window (`engine::SKELETON`).
- FL script protocol 5: `notes` batches, `note`, `nmap` (+ OnMidiMsg),
  `alloff`, all-off on OnDeInit, Sampler channels in the inventory ("ch"
  rows). The plugin sends `alloff` when the link comes up.
- Window (`editor.rs`, `fingers.rs`): Noa's layout and the finger editor,
  see `docs/ux-plugin.md` section 5b. Checked in the standalone build:
  camera, skeletons, dither hands, a template on the right index, the
  channel picked from FL's inventory.
- Sentence field: a line with a MIDI target (`T:midi.pattern`,
  `T:midi.chord`, `T:midi.note`) makes its part at once and plays while
  pending; Keep binds it (the finger editor shows it), Discard drops it.
  The resolver now passes the source's shape and the NOTE / CHORD values
  (`shape:peace`, `note:g2`, `chord:Am`); a key in the frame ("in A
  minor", `I:scale`) sets the MIDI key. Completion offers the user's
  pattern names, the templates and the chords of the key.
- Note names follow FL: MIDI 60 is C5 (shown and read that way; Noa to
  confirm).

## Edge cases

Tests cover: release, two routes on one part, route removal, Live off (and
a finger held through it starting when Live comes back), host stop, once,
progression change, a finger lost for 600 ms, a source that stops sending,
a pinch that chatters around its threshold, a hand that comes back with a
stale mask, a tempo change on synthexis' own clock, an edit to a playing
part, a note-off never overtaking its note-on, strummed notes at a chord
change, a channel deleted above a sounding note (script check). Written
decisions: with the host stopped, parts keep playing on synthexis' own
clock at the last tempo. A host loop by whole bars is seamless: each part
counts its own bars (`Active::shift`), so progressions, long clips and Once
parts run on across an FL pattern loop; a seek, start or stop ends the
sounding notes and the parts re-join with their bar count and the rest of
a Once kept. Two instances on one channel and pitch share FL's note (an off
from one ends the other's). Profile switch does not exist yet; bypass is
untested.

## Review (two workflows, 11-04 night)

Five reviewers by area, two skeptics per finding: 46 findings, 45
confirmed, 1 refuted. Fixed by four agents on disjoint files plus a
verifier: 38 of the 39 outside the window code; the 39th (a note-off after
a channel above it was deleted) fixed in the script afterwards: the script
remembers each held note's channel name and ends it where that channel is
now. Gates on a MIDI sentence are refused with a note instead of dropped
silently; INV on a finger trigger folds to the opposite gesture (fist, eye
closed) or is refused. The seven window findings go to the piano-roll
rework.

## Finger states (`probes/finger-shapes/RESULT.md`)

First FL test: the middle finger alone played the index finger's part. No
recording has that pose; the student's finger head pulls unseen
combinations to the nearest trained one. Measured with a rule first: the
mean of the student's probability and the geometric extension per finger
reads the most exact masks (dev 0.880, S4 0.895) with the least flicker;
it ships, with a per-person calibration (fist, open, each finger alone,
about 20 s; thresholds per finger halfway between the person's folded and
shown medians, kept in settings.json) and a live display of the fingers the
camera counts as shown. Open: Noa's live check of the unseen poses.

## Window rework (two workflows, 11-04 night)

After Noa's first FL test of the editor: FL-style piano roll (click adds a
note with the last length, drag moves on the 1/16 grid, Option-drag moves
in ticks, the right edge resizes, right-click deletes, an event lane for
velocity and rolls), a fixed 15-row roll that scrolls inside and never
jumps on an edit, chord notes as long notes that turn the chord into a
pattern on the first edit ("Back to ..." restores it), FL-like grid lines,
every section folds and the camera takes the free height, readable type
(body 12.5) and one accent colour, the plugin picker on the chat row.
Cause of "notes cannot be moved": chord notes span the whole bar and the
move clamp pinned them to step 0. 257 tests. Installed 03:25 (FL closed).

## Morning test for Noa (11-04)

1. Fingers: open "What fingers play", press "Calibrate fingers", hold each
   pose with both hands until the bar moves on (fist, open, thumb, index,
   middle, ring, pinky alone; about 20 s). Then show the middle finger
   alone. Whatever happens, the log `~/Library/Logs/synthexis.log` now holds
   `calib: pose ...` lines with the student, the keypoint geometry and
   Apple Vision side by side: they say which source sees that pose, and
   the finger source gets switched to the one that does.
2. Piano roll: add notes above a chord (the roll must not move), drag a
   chord note in time and pitch, Option-drag it in fine steps, wheel over
   the roll scrolls the notes, not the page.

## Open

- FL test of the plugin with Noa (install below), then R2: timing of a
  recorded 1/16 clip against FL's grid, which sets the player's lead.
- Arp from spread or finger count, CC targets, contour melodies.
- Pong: `docs/pong-plan.md`, defaults waiting for Noa.

## Install for the FL test (FL closed)

```bash
cd plugin
./guarded-cargo.sh xtask bundle synthexis --release
codesign --force --deep -s - target/bundled/synthexis.vst3
codesign --force --deep -s - target/bundled/synthexis.clap
ditto target/bundled/synthexis.vst3 ~/Library/Audio/Plug-Ins/VST3/synthexis.vst3
ditto target/bundled/synthexis.clap ~/Library/Audio/Plug-Ins/CLAP/synthexis.clap
cp fl/device_synthexis_bridge.py "$HOME/Documents/Image-Line/FL Studio/Settings/Hardware/synthexis bridge/"
```
