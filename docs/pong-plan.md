# Pong preset: plan (2026-11-04)

Status: historical. Replaced on 11-04 night by Noa's request (pointing
shot, four assignable sides, presets) and the build: design
`docs/design/pong-presets.md`, session `docs/session-2026-11-04-pong.md`.
What still holds from here: the throw as a movement for routes and
completion (not built), the visual style. The table below was the
proposal of 11-04 morning. Built on the MIDI
layer of 11-04 (`plugin/synthexis/src/midi.rs`: parts, player in host time,
notes into FL through the bridge script, `probes/fl-midi-notes/RESULT.md`).

## What Noa asked for (11-03 and 11-04, paraphrased)

- A throw of the hand (the move recorded in block H from S2 on) shoots a
  projectile out of the hand into the camera picture.
- The projectile bounces off the frame border like Pong or the old DVD logo.
- A slider sets when it disappears, up to "never"; its speed is settable.
- The border is split into zones; a hit on a zone plays MIDI or moves a
  parameter. The canvas itself becomes something the sentence field and the
  menus can target.
- The throw is a movement like any other: completion offers it ("Wurf",
  "throw"), the sentence field and the hand map can bind it.

## Defaults to agree on

| Question (spec preset.pong) | Proposal |
|---|---|
| Segments | 16, four per side, each a quarter of its side; numbered clockwise from the top-left corner |
| What each wall plays | notes of the key's scale: top = degrees 1 to 4 in octave 5, right = 5 to 8, bottom = 1 to 4 in octave 4, left = 5 to 8 in octave 4, so up is higher and the walls sound like a scale run around the frame |
| Velocity | the projectile's speed at the hit, 60 to 127 |
| Note length | 1/8, played as a one-shot part |
| Hit on a pattern segment | starts the pattern once from the next bar (Mode::Once, Start::Bar); a second hit while it plays does nothing |
| Hit on a parameter segment | a one-shot ramp up and back over 1 beat, depth from the speed (spec preset.pong ramps); later than notes |
| Lifetime slider | 1 to 32 bounces, then "forever" at the right end; default 8 bounces |
| Speed | throw speed x a speed slider (0.25x to 4x, default 1x); no gravity by default, gravity slider 0 to 1 |
| Projectiles at once | 4; a fifth throw removes the oldest |
| Quantizing hits | to the next 1/16 of the host grid while FL plays; on synthexis' own clock at the project tempo while FL stops (the player does this already for patterns) |
| Visual | a white dither dot (the hand art's style) with a short dissolving trail, the hit segment lights in the hand colour |
| Zone editor | the border ring drawn over the camera; a click on a segment opens the same single view as a finger (note, chord, pattern, channel); the Pong section has the lifetime, speed and gravity sliders |

## Throw detection (motion.primitives.throw)

From the palm track the engine already has (`engine/geom.rs` HandTracker,
smoothed palm speed 0..1): a throw is a speed peak above v_min = 0.55
followed within 150 ms by the speed falling below 0.25 or the hand opening
(openness up by 0.3). Direction: the palm's motion over the last 100 ms;
start: the palm position at the peak; speed: the peak. Emitted as an event
signal `cam0.motion.R.throw` (ontology `motion.throw`, sided, event) with
speed, direction and the border position the ray hits. Threshold check
before building: the block H recordings with their Spark Open timelines
(`probes/motion-labels/RESULT.md`), pass when at least 9 of 10 labelled
throws fire and at most 1 false throw per minute of free play.

## Build order

1. Throw detector in the engine and the event signal (unit tests on
   synthetic tracks, then the block H check above).
2. Projectile simulation on the player thread (positions in frame
   coordinates, bounces, lifetime), hits as trigger events into the player
   as one-shot parts.
3. Drawing in the camera canvas (projectiles, ring, lit segments).
4. The Pong section: segment list or ring editor, sliders; preset "Pong"
   fills the 16 segments with the defaults above.
5. Sentence field: `I:preset preset:pong` loads it as pending rows; the
   completion entry "throw" / "Wurf" comes from the ontology's motion.throw.

Rough size: 1 and 2 are each about an hour and a half, 3 and 4 together
about two hours, 5 half an hour.
