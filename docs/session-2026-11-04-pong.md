# Session 2026-11-04: Pong mode and presets

Branch `claude/midi-playback-411997` (on top of the MIDI session's
uncommitted work). Nothing committed. Design and its critique:
`docs/design/pong-presets.md` (v2). Installed into FL at 12:38 (VST3, CLAP,
bridge script), FL was closed.

## Built

- Shot (`pong/shot.rs`): pointing, Noa's choice of 11-04 night. The
  debounced finger mask turns into a point (index shown; middle, ring and
  pinky folded; thumb free), the index's trigger value at least 0.9, the
  others at most 0.3, the hand still (speed at most 0.25). The projectile
  starts at the index tip and flies along the index (a line fitted through
  its four joints); an index pointing into the lens is too short in the
  picture and does not shoot. 250 ms between shots per hand; a hand coming
  back already pointing does not shoot. The flick (index sprung off the
  thumb, `pong/flick.rs`) is the other choice in the section.
- Arena (`pong/arena.rs`): the whole camera frame, mirrored like the
  preview, 16:9. Reflection at the incoming angle, continuous collision
  (no tunnelling at any speed), a corner hits both sides as one bounce.
  Speed 0.25 to 4 frame heights per second, bounces 1 to 32 or never, a
  time limit, at most 1 to 64 at once (the oldest goes). The shot starts
  where it would be now (perception latency made up).
- Sides: each one Off, a MIDI part (note, chord, pattern; once per hit,
  at once or on 1/16, beat or bar), a plugin parameter through the bridge
  `rec` path, or a Signal macro. Parameter hits: ramp (jump by the shot's
  strength, glide back to rest; default), position (where on the side it
  hit; left and right count from the bottom), toggle. New parameter sides
  rest at 50 % so a cutoff stays audible.
- Runtime (`pong/mod.rs`, `pong/run.rs`): its own thread per instance,
  independent of the window (FL records with the window closed); the
  writer keeps the camera and perception running while Pong is on. Live
  off and deactivate clear the projectiles and set ramps back to rest;
  Pong off too.
- No stuck notes, no flooded script: hit voices with their own note-off
  (`midi.rs`), owners counted per note so a hit on a note a finger holds
  re-strikes it and the finger keeps it; across instances the bridge
  counts owners too (`fl.rs`), and offs go out even while the link is
  down. A process-wide budget of 40 hit note-ons per second (burst 8);
  per side the first hit in 30 ms fires; ramps write at most every 33 ms
  per target. A clip played by hits runs at most twice at once per part.
- Fingers of the shooting hand pause while Pong is on (default; a point
  folds and shows the index). Parts the other hand holds keep playing.
- Window (`pong_ui.rs`, `editor.rs`, `fingers.rs`): section "Pong" with
  presets, settings, Test shot and Burst x16, the side view (dotted 16:9
  outline, clickable sides with chips), the side editor (the finger
  editor's part editor for MIDI sides); on the canvas the whole frame,
  the walls, projectiles with trails, clickable walls. `docs/ux-plugin.md`
  section 5c.
- Sentence field: with a side selected a sentence without a source
  assigns that side, as a pending row with Keep and Discard. Also fixed:
  the field now reloads after a project load (the editor is built before
  FL sets the state, so kept lines were missing and the next Keep
  overwrote them).
- Presets (`preset.rs`): snapshots of sentence routes, MIDI parts and
  binds, the Pong setup and the picked plugin; save, load (with Undo
  load), delete (into `presets/deleted/`). Files in
  `~/Library/Application Support/synthexis/presets/`. A load lands in the
  project state too. Plugins that moved are found again by name, also
  when FL's inventory arrives after the load.
- Built-in "Pong" (stress test): right hand points, speed 1, bounces never,
  32 at once; left wall ramps Signal 11, right wall Signal 12 (link them
  in FL, or click a side and pick a plugin parameter); top plays "Pong top"
  (C6, FL naming), bottom "Pong bottom" (I-V-vi-IV, one chord per hit), on
  the generator FL has in focus once the channel list is there.

## Decisions (with the reason)

- Pointing instead of the flick: Noa, "fires every time the index is
  shown, simpler". The flick stays for later tuning.
- Hits play at once by default; a wall part's start (1/16, beat, bar)
  quantizes. At once feels like a hit; quantize is one click per side.
- Ramp as the default parameter mode: a hit is an event, a ramp makes it
  audible and returns; position is there for "where it hit".
- Two writers on one parameter (a side and a route): last write wins.
- Several synthexis instances with Pong on each shoot their own
  projectiles from the same point; budget and note owners are shared.
- Not built: the shot as a movement for routes and completion, loading a
  preset by sentence, gravity, position-to-pitch, profiles (tabs) on top of
  presets. FL is not told the project changed after a preset load (FL may
  not ask to save).

## Measured

`probes/pong-shot/RESULT.md` (rule written before the run): over S2 and
S3 the point shoots in 5 of 6 index-alone segments; false shots in all
other segments 11.4 per minute unfiltered, 2.0 per minute with the
shipped filters (S4 held out: 1.4). The rule asked for at most 1; the
recordings have no labels for deliberate pointing, many of the rest are in
free play and throws. Noa's live test decides.

## Review

Design: three critics, 50 objections, design v2. Code: three reviewers by
area, 12 confirmed findings (4 major), each fixed with a regression test.
289 tests green, clippy clean in tonight's files.

## Test für Noa (FL)

1. Synthexis öffnen, Abschnitt "Pong" aufklappen, Preset "Pong" laden.
   Erwartung: Pong an, auf dem Kamerabild das ganze Bild mit vier Wänden.
2. "Burst x16" klicken (ohne Kamera). Hören: oben eine Note (C6), unten
   wechselnde Akkorde auf dem Kanal im Fokus; nichts hängt, wenn du Live
   ausschaltest oder Pong aus.
3. Rechte Hand: Faust, dann nur den Zeigefinger zeigen, die Hand ruhig
   halten. Erwartung: ein Schuss aus der Fingerspitze in Zeigerichtung.
   Öffnen der ganzen Hand oder Zeigen in die Kamera schießt nicht.
   Notiere, wie oft ein Zeigen nicht schießt und wie oft es ohne Absicht
   schießt.
4. Eine Seite anklicken (im Seitenbild oder am Bildrand), "Parameter"
   wählen, z. B. Cutoff eines Plugins: jeder Treffer öffnet kurz und
   gleitet zurück. Oder mit gewählter Seite in das Satzfeld schreiben:
   "cutoff von 30 bis 80 Prozent", Keep.
5. Mit FL-Aufnahme und geschlossenem Fenster schießen: Noten landen im
   Pattern, Parameter als Automation.
6. Eigenes Preset speichern, Projekt speichern und neu öffnen: Pong und
   die Satzzeilen sind wieder da.
