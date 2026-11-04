# Pong mode and presets: design v2 (2026-11-04 night)

Noa's request of 11-04 ~02:45 replaces the throw-of-the-hand plan in
`docs/pong-plan.md`: the shot is an index flick, the frame has four
assignable sides (not 16 segments), and the setup is saved as a preset.
v1 went through three adversarial critics (realtime and MIDI, gesture and
physics, product and persistence; 50 objections). v2 is the result and the
contract for the implementation. Numbers marked (h) are hypotheses.

## 1. Pieces and owners

| Piece | File |
|---|---|
| Arena physics, pure | `pong/arena.rs` |
| Flick detector, pure | `pong/flick.rs` |
| Setup (serde) | `pong/setup.rs` |
| Runtime: thread, input, dispatch, ramps, test shots, UI snapshot | `pong/mod.rs` (+ `pong/run.rs` if long) |
| Hit voices, owner counting, hit budget | `midi.rs` |
| Offs while the link is down, owner counting across instances | `fl.rs` |
| Snapshot capture/apply, preset files, built-ins, before-load undo | `preset.rs` |
| Rebinding a saved line to another plugin location | `saved.rs` |
| Wiring: Writer.pong, needs, Live, paused hands, auto channels, macros, state_gen | `writer.rs`, `route.rs`, `lib.rs`, `params.rs` |
| Pong section, side view, side editor, canvas walls, preset row | `pong_ui.rs` + `editor.rs`, `fingers.rs` (part editor split), `i18n.rs` |
| Sentence scoped to a side, field reload on state_gen | `sentence.rs`, `assign.rs` |
| Flick bench check | `engine/bench.rs`, `probes/pong-flick/` |

`editor.rs`, `fingers.rs`, `i18n.rs`, `sentence.rs`, `calib.rs`,
`engine/perception.rs`, `engine/mod.rs` only after the MIDI session's
`ui-done.md`.

## 2. Coordinates

Arena = the whole camera frame in preview coordinates (x 0..1 in the
person's view, mirrored like the preview and `engine::SKELETON`; y 0..1
from the top). Physics in units where the frame is ASPECT (16/9) wide and 1
high; speeds in frame heights per second. Hit position along a side: top
and bottom 0 at the left, 1 at the right; left and right 0 at the bottom, 1
at the top (like hand y, up = 1).

Canvas with Pong on: contain fit (whole frame, frosted wings), the four
walls drawn on it. The top bar covers the top wall's band; the top wall is
clickable in a 12 pt band under the bar and always in the side view.

## 3. The shot: pointing (default) or flick

Noa, 11-04 ~03:30, after v2: simpler is better, the projectile should fire
reliably when the index is shown. Default trigger `Point` (`pong/shot.rs`):
the hand's debounced finger mask (`engine::MASKS`, the mask the MIDI
triggers use, with the person's calibration) turns into a point (index
shown; middle, ring and pinky folded; thumb free) from a frame that was
not a point. Direction fitted through joints 5..8 of the same frame
(`engine::SKELETON`, same capture time); refused when the index is shorter
than 0.55 palm in the picture (into the lens) or a key joint is on the
frame border. Refractory 250 ms; a hand unseen for 300 ms that comes back
pointing does not shoot; opening the whole hand does not shoot. Strength
is fixed at 0.8 for a point. The flick below stays as `Trigger::Flick`.

### Flick detector (option)

Input per hand frame: 21 joints (preview coords) with capture time, and,
when present with the same capture time, the trigger finger values
(`engine::FINGER_VALUES`) and thresholds (`engine::FINGER_THR`). All time
windows use capture times of that hand's frames, never the wall clock.

Frame features (physics units, palm lengths; palm floored at 0.03 frame
heights; palm centre = mean of joints 0, 5, 9, 17):
- `gap` thumb tip to index tip; `reach` index tip from the wrist;
- `others` mean reach of middle, ring, pinky tips;
- `thumb` thumb tip relative to the palm centre, `tip` index tip relative
  to the palm centre;
- `proj` |index tip - index mcp|; `straight` proj over the chain length;
- `dir` least-squares direction through joints 5..8, oriented mcp to tip.
A frame with a non-finite joint, or joint 0, 4, 5, 8, 9 or 17 on the clamp
border (x or y within 0.002 of 0 or 1), is unusable: it resets the state.

States per hand:
1. Loaded: gap <= 0.35, reach <= 1.45, and (if finger values are there)
   index value <= its threshold. Held for >= 2 frames.
2. Candidate: the first frame after the last loaded one with gap >= 0.8,
   at most 200 ms (capture time) after it.
3. Fire on the next frame (confirmation) if it still has gap >= 0.7 and,
   measured from the last loaded frame to the candidate frame:
   - opening rate >= 5 palms/s;
   - reach gained >= 0.35;
   - others gained <= 0.3 (a fist springing open moves all fingers);
   - index tip moved >= 0.6 palm and the thumb tip < 0.35 palm against
     the palm centre (a pinch release moves both);
   - at the candidate and confirmation frames proj >= 0.55 and straight
     >= 0.8 (an index pointing into the lens is refused, no guessing);
   - index value (if present) >= its threshold;
   - >= 250 ms since this hand's last shot.
   A frame without confirmation drops the candidate.
- Lost: a capture-time gap over 200 ms between the hand's frames resets.

Shot: start = the candidate frame's index tip; dir as above; strength =
clamp((index tip travel - 0.6) / 0.8, 0, 1), travel in palms (rough; it
does not depend on the frame timing). Projectile spawned and advanced by
(now - candidate capture time) x velocity, so perception latency and the
confirmation frame do not show.

Input: until the engine edit is allowed the Pong thread polls `SKELETON`
every 2 ms and counts capture-time gaps; after `ui-done` the engine keeps a
small ring of the last 8 frames per hand (joints, confidence, finger
values, seq) and the detector reads every frame since its last seq;
frames below confidence 0.2 are skipped.

Not detectable: flicks straight at the camera (documented in the section).

## 4. Arena

As built in `pong/arena.rs`, plus: corner = both walls within 1e-5 units
at the hit point (one hit per side, one bounce); non-finite projectiles
are dropped; the step's dt is clamped to 50 ms and the clock resets after
a pause, Live on or Pong on.

Setup: speed 0.25..4 heights/s (default 1.0); `speed_from_flick` default
off (strength 0..1 -> 0.6x..1.4x); bounces 1..32 or never (default 3);
time limit 1..60 s or never (default never); max 1..64 at once (default
16), the oldest goes. No gravity.

Each projectile carries its shot's strength. A hit's depth and velocity
come from that strength, not the absolute speed, so the speed slider
changes the tempo of hits, not their loudness.

## 5. Sides

`Action` per side: Off, `Part { id }` (a part in the MIDI store),
`Param { key, index, name, how }`, `Macro { index, how }`. `How { mode:
Ramp | Position | Toggle, lo, hi, depth_from_speed, back_ms }`.
- Ramp (default): jump to lo + (hi - lo) x depth, glide back to lo over
  back_ms (400 ms). depth = strength when depth_from_speed, else 1.
  Overlapping ramps on one side take the maximum. New Param walls start
  at lo 0.5, hi 1.0 (a cutoff at rest stays audible); macros 0..1. Nothing
  is written before the first hit.
- Position: lo + (hi - lo) x pos, stays.
- Toggle: flips between lo and hi.
Ramp writes at most every 33 ms per target and only on a change of at
least 1/1024. Pong off, Live off, a preset load or deactivate write lo
once to each target with a running ramp.

Per side the first hit in a 30 ms window fires at once; later ones in the
window are dropped (counted).

Two writers on one parameter (a side and a route): last write wins,
documented.

## 6. Hit voices in the player

`Player::hits(&[Hit { part, strength, at_ns }])`, one call per Pong step,
one `notes` frame:
- returns at once while !live;
- Note: on now, off after `hit_len` beats (Setup, default 0.25, at least
  40 ms);
- Chords: one degree per hit, cycling through a progression (a counter
  per part); strum kept;
- Clip: a voice scanned a step ahead in tick_at like `active`, from its
  step 0 at the hit, to the clip's end; at most 2 running voices per
  part, a third ends the oldest;
- start: the part's own `start` (Now default for wall parts: at once;
  1/16, beat or bar quantize); mode is ignored for hits;
- velocity: when `vel_from_speed`, step and note velocities are scaled by
  0.55 + 0.45 x strength (accents kept), else the part's own.

Owners: `sounding` entries carry a voice id (0 = the part's own play,
route-held). An on of a (channel, pitch) that is already sounding sends an
off then the on, and adds its owner; a voice's off removes that owner and
the physical off goes out only when no owner is left. So a hit on a pitch a
finger holds re-strikes it and the finger's note keeps sounding after the
hit ends.

end_route(part): ends the route-held play only (trigger off, release_route).
end_part(part): ends every voice of the part, hits included (set_store
gone or channel changed, stop_all, Live off, all_off). A non-seamless host
jump (seek, start, stop) ends hit voices.

Hit budget: process-wide token bucket for hit note-ons, 40 per second
(burst 8) (h: the JSON path is measured at 30 notes/s; raise after a
probe). Offs always pass. A note-on over budget is not sent and counted.

`fl::Bridge::notes`: offs go out even while the link is down (ons wait in
`held` as today). Owner counting across instances: `held` counts owners
per (channel, pitch); an on while held sends off + on; an off with owners
left is not sent. So two instances on one channel and pitch never leave a
hanging note.

`Store.walls: Vec<u64>` (serde default): wall part ids, counted as used by
`unbind`, so a wall's part never goes with a finger.

## 7. Runtime, Live, instances, fingers

`Pong` (one per Writer) owns the setup as `Mutex<Arc<Setup>>` (a leaf
lock: cloned out, never held across another call) and atomics `on`,
`paused[2]`, `live`, `suspended`. Its thread holds only `Weak<Pong>`,
waits with a timeout (250 ms while off, 4 ms while on) and exits when the
upgrade fails. Ticks: read input, run detectors, step the arena, collect
hits, dispatch: `player.hits`, `fl::bridge().write`, macros through a
writer hook (latest + snapshot + submit_main, main queue).

- Writer `needs()` reads the `on` atomic: perception runs with the window
  closed while Pong is on.
- Live off: projectiles cleared, ramps to lo, no new shots; Player::hits
  returns while !live.
- deactivate: `suspended` set (no shots, arena cleared), stop_all; the
  next process() clears it.
- Pong off: arena cleared, hit voices ended, ramps to lo.
- Instances: each with Pong on shoots its own projectiles from the same
  flick; the hit budget and the bridge owner counting are process-wide.
- Paused fingers (`pause_fingers`, default on): per instance,
  `Values.paused: [bool; 2]` forces the gate closed for every Part route
  whose signal starts with `cam0.hand.<side>.` of a shooting hand
  (shapes, finger values, pinch). At the pause the writer ends those
  parts whatever their mode (Latch, Once); at the unpause routes with
  edge.on re-emit their on edge; the Live-on re-emit skips paused hands.
  Parameter routes of the shooting hand keep running.
- Auto channels: `Setup.auto_channel: Vec<u64>` lists parts that get the
  focused generator, else the first channel, when the inventory arrives
  (follow_rack); then they leave the list.
- Test shots: "Test shot" and "Burst x16" in the section fire from the
  frame centre at golden-angle directions, strength 0.7, so the stress
  test runs without the camera.

## 8. Presets and snapshots

`Snapshot { routes: Vec<saved::Saved>, midi: midi::Store, pong: Setup,
pin: Option<String> }`; `Preset { version, name, snapshot }`.
`preset::capture(writer, params) -> Snapshot`; `preset::apply(writer,
params, snapshot)` (no window needed; profiles reuse it):
1. validate (parsed and clamped before anything is removed);
2. capture the current state as the before-load snapshot (Undo load);
3. stop all notes, Pong cleared, ramps to lo;
4. `writer.clear_assigned()` (every non-test route);
5. lines renumbered above any line id in use; `saved::rebind` each line
   whose plugin moved (rewrites `key`, `bound.plugin` and `keys`) when the
   inventory is loaded, else again on the next inventory;
6. MIDI store: `rebind_channels` when the inventory is loaded (else the
   writer's fl_gen resets so the next inventory rebinds), `set_midi`;
7. Pong setup set (params `pong` written);
8. `writer.bump_state()`: the sentence field and the finger editor
   compare `state_gen` each frame and reload (kept lines from params,
   pending, undo stacks, MIDI undo, selected wall cleared). initialize()
   bumps it too, which also fixes the field not showing kept lines after
   a project reopen.
Pending sentence lines: the window's Load first runs the field's Discard.

Files: `~/Library/Application Support/synthexis/presets/` (`$SYNTHEXIS_PRESETS`
overrides), one JSON each; the name lives in the JSON, the file name is a
slug (slug-2 ... on collision with another name); names compare trimmed
and case-folded; writes through a tmp file with pid and counter and a
rename; Delete moves the file to the Trash (else into `presets/deleted/`).
Built-ins live in code, listed first, cannot be overwritten or deleted.

Built-in "Pong" (stress test): Pong on, right hand, speed 1.0, bounces
never, max 32; left wall Ramp on Signal 11, right wall Ramp on Signal 12
(link them in FL, the chip says so; one click retargets to the picked
plugin's parameters); top a Note part "Pong top" (C6, FL naming), bottom a
Chords part "Pong bottom" (I-V-vi-IV, one chord per hit); both parts in
`auto_channel`; drum channels (fit_to_channel's test) get hat 42 and kick
36 notes instead.

## 9. Not tonight

The flick as a bindable movement in routes and completion; loading a
preset by sentence; gravity; position-to-pitch on MIDI walls; probing
the JSON notes path above 30 notes/s; marking the FL project dirty after a
preset load (nih-plug has no call for it; a param touch could).

## 10. Tests and measurement

Units: arena, flick (synthetic hands: fire, two-frame fire, slow opening,
pinch release, fist opening, OK sign, refused into-lens, outliers without
confirmation, refractory, lost hand, clamp border, NaN), setup serde,
player hits (owners, budget, clips bounded, Live off, set_store, seek),
bridge offs while down and owner counting, presets (round trip, slug
collisions, built-ins, rebind keeps the route count, apply clears and
restores, before-load undo), writer (needs, paused hands for each mode,
unpause re-emit, auto channel), Pong runtime (thread exits on drop, Live
off, deactivate, test burst of 64 x 60 s with balanced notes and the
budget held).

Recordings (no flick in them): rule before the run in
`probes/pong-flick/RESULT.md`; false shots over the pinch-drag and
free-play segments of S2 and S3 at most 1 per minute, each listed with its
time for a look. For tuning, Noa records one clip with 30 flicks in four
directions, 30 pinch releases, 30 fast fist openings and 20 OK-sign
releases; the rule per class: flicks fire >= 90 % with direction error
<= 20 degrees, each other class <= 1 in 20.

## 11. UI

Pong section (folding, under "What fingers play"): on, shooting hand (L,
R, both), speed, bounces (1..32, never), time limit, max, velocity from
the flick, pause the shooting hand's fingers, Test shot, Burst x16, a line
with live counts (projectiles, hits/s, dropped). The side view: the 16:9
outline in the dither style of the hands, four sides as clickable strokes,
each with a chip naming its action. The selected side's editor: Off | MIDI
| Parameter. MIDI: the finger editor's part editor (split from `single`,
owner Wall: no mode picker, start = quantize, Remove sets the wall Off).
Parameter: the picked plugin's parameters plus Signal 1..32, Ramp /
Position / Toggle, rest and peak, depth from the flick, glide. Canvas:
the walls (selected bright, a hit flashes in the hand colour), projectiles
as white dots with a short trail, clicks on the walls.

Presets row (top of the Pong section, later in the top bar with
profiles): a dropdown of presets (built-ins first), Load, a name field and
Save (Overwrite when the name exists), Delete (two clicks), Undo load
while the before-load snapshot exists.

Sentence scoped to a side: a chip "-> top wall" in the field while a side
is selected; a sourceless sentence assigns the side
(`assign::wall_action`: first target, lo:/hi: to rest and peak,
mode:toggle to Toggle, a MIDI target makes an unbound part) as a pending
row "top wall -> Cutoff" with Keep and Discard (Discard restores the old
action and drops the new part); the scope ends after a Keep; a sentence
with a source is a normal route; clear/unmap while scoped set the side
Off; Esc drops the scope only when the list and panel are closed and the
field is empty.
