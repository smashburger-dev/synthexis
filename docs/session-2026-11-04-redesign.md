# Session 2026-11-04 afternoon: one shot, UI redesign

Branch `claude/midi-playback-411997`, on top of the Pong session. Nothing
committed. Noa's feedback after the first FL test of Pong is in
`docs/design/ui-redesign-brief.md` (R1 to R23); the spec that answers it is
`docs/design/ui-redesign.md` (v2, three critics, 83 objections taken, 9
rejected with a reason in its section 9).

## Shot

- One shot, no Point/Flick choice. Cause of "pointing slowly does not
  shoot": `pong/shot.rs` checked index value and stillness only on the frame
  the mask turned into a point; a slow point straightens a few frames later.
  Now each point (an unbroken run of point masks, also one starting when the
  hand comes back pointing) shoots once, on its first frame at least 300 ms
  in that is clear and still. A flick is a fast point and shoots the same
  way. `pong/flick.rs` is out of the build; the file stays until Noa says it
  can go.
- Measured (`probes/pong-shot/RESULT.md`, section Hold, rule written first):
  hold 0/100/200/300 ms gave 4.2/2.7/2.3/1.5 false shots per minute with
  5/6 index segments each; only 300 ms passed. Held-out S4: 2/3 index
  segments (before 1/3), 1.3 false per minute. Cost: 0.3 s from showing the
  point to the shot.

## Redesign (built by a workflow in 8 units, `docs/design/ui-redesign.md` section 7)

- Window: camera on top, then the top section (plugin picker, chat field,
  preset picker with "+"; under it the plugin note, the routes strip, Save
  as and Undo), then only the sections the preset picker opened, then
  Advanced and 24 pt of room. Sections run edge to edge with rules between
  them, no cards. Default width 1200 pt; the window asks the host for the
  height its content needs (`ResizeGate`), and the layout follows the real
  height if the host refuses.
- Background (`backdrop.rs`): ordered dither of slow colour fields from the
  portfolio's dusk hues meeting black, reimplemented from formulas, no code
  copied from noa-portfolio.
- Camera preview (`engine/camera.rs`): the full camera frame mirrored into
  the texture instead of 320x180; uploaded only on a new frame.
- Preset picker (`preset/top.rs`): lists Hands, Pong and saved presets;
  picking opens that section; "+" adds the other section (an open one is
  greyed out); Save as names a preset; a section closes with a second click
  on its x. Which sections are open is saved with the project; an old
  project opens the sections that have content.
- Hands (`fingers.rs`, `fingers/graph.rs`, `fingers/part_lib.rs`): hands
  turned 45 degrees outward, label nodes joined to the fingers by bent lines,
  no overlaps; a click lights the finger at once; Fist and Open hand under
  each hand, the other shapes gone from view; part editor in rows (header
  with Hide roll and Remove, kind/channel/start/gate, bars, view, template
  dropdown, part name with Save); lanes with beat and bar lines; a Routes
  view draws hands, fingers and targets as nodes. Calibration lists poses
  the camera misreads and says when no hand is in view (finger session's
  note).
- Pong (`pong_ui.rs`): shooting hand above the side view, the sliders and
  toggles under it, stats in the header, its preset row gone (top picker).
- Chat field (`sentence.rs`): wider, smaller cursor; a sentence about hands
  or fingers opens Hands; route suggestions show in a strip under the top
  section and an overlay that can fold away.
- Settings: Signals 1-32 and Diagnostics (camera lines, Burst x16, log)
  folded inside, the account settings on top.
- nih-plug (`plugin/vendor/PATCHES.md`): three patches so a plug-in
  initiated resize reports the right size. `probes/fl-resize/` checks
  whether FL honours it (Noa's run).

- Profiles (`preset/profile.rs`, ui-redesign.md 2.12): `1 2 3` left of
  Settings in the top bar; each holds the kept lines, MIDI parts and
  binds, the Pong setup, the picked plugin and the open sections, saved
  with the project; a new profile starts empty, an old project opens on
  profile 1 unchanged.

## Checked

- Workspace tests green: synthexis 388, complete 43 + 3, parser 42 + 2.
- Clippy clean in the redesign's files (older warnings in engine/*,
  lib.rs:295, signal.rs remain).
- Review: three reviewers (requirements, correctness, look), 15 findings,
  9 confirmed by a skeptic and fixed with tests.
- Standalone screenshots of every state in English and German. Not checked:
  the camera's sharpness and the preview's cost (the standalone has no
  camera permission), and anything inside FL.
- A debug standalone panics in baseview (`macos/view.rs:248`, null
  reference, rustc 1.99); build it with
  `--config 'profile.dev.package.baseview.debug-assertions=false'`. Release
  builds and FL are not affected.

## Open

- Cleanup commit: Unit 7's list (old wrappers `Sentence::ui`,
  `Fingers::ui`, `PongUi::ui`, Pong's old preset functions and 3 tests, old
  token aliases, unused i18n keys, dead-code allows). Waits for the head
  session, which edits the same files.
- Head section and preset: head session, stage 2.
- Skeleton templates from a calibration are not saved yet (gone after an FL
  restart).
- UI font: egui's Ubuntu-Light stays; Inter would read better over the
  dither (Noa's call).

## Test für Noa (FL)

1. FL schließen, installieren lassen, FL öffnen, Synthexis laden.
   Erwartung: Kamera oben scharf, darunter Plugin-Auswahl, Chatfeld,
   Preset-Auswahl mit "+", dahinter der langsam wandernde Dither-Hintergrund.
2. Preset-Auswahl: "Hände" wählen. Erwartung: der Abschnitt Hände geht auf,
   die Hände sind schräg nach außen gedreht. Einen Finger anklicken: er
   leuchtet sofort, auch ohne Kamera.
3. "+" klicken, "Pong" wählen. Erwartung: Pong erscheint unter Hände, "Hände"
   ist im Menü grau.
4. Rechte Hand: Faust, dann langsam den Zeigefinger zeigen und kurz halten.
   Erwartung: nach etwa 0,3 s ein Schuss. Dasselbe mit einem schnellen Flick.
   Notiere, wie oft Zeigen nicht schießt und wie oft es ohne Absicht schießt.
5. Ins Chatfeld "wenn ich den Zeigefinger zeige, spiel eine Hi-Hat"
   schreiben. Erwartung: Vorschläge unter dem oberen Bereich, Hände geht auf.
6. Einstellungen öffnen: Signals 1-32 und Diagnose sind eingeklappt.
7. Fenstergröße: wird das Fenster beim Öffnen von Hände höher? Wenn FL das
   ablehnt, scrollt der untere Bereich, und nichts ist abgeschnitten.
   `probes/fl-resize/README.md` hat die genaue Messung.
8. Finger kalibrieren: danach stehen falsch gelesene Posen als Zeile da
   ("Mittelfinger allein: die Kamera liest Zeigefinger").
9. Projekt speichern, neu öffnen: dieselben Abschnitte sind offen.
