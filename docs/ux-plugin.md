# Plugin UX (proposal, 2026-10-31)

How the synthexis window is laid out, what each part does, how the main tasks flow, how to build it, and what the demo needs by the feature freeze on 11-02. Nothing here is built yet.

The main path is clicking: hand, finger or object, movement, target, from menus. It has to work every time. Typing or saying a sentence is an optional shortcut on top that writes the same rows (Noa, 11-01).

Sources: Noa's voice note of 10-31 (paraphrased below), spec nodes `ui.canvas`, `ui.panel`, `gest.hand_role`, `gest.presets`, `map.agent`, `map.fallback`, `targets`, `bridge`, the proposals in `probes/mapping-pipeline/SPEC_PROPOSAL.md`, the vocabulary in `tools/mapping/ontology.yaml`, and the editor as it stands in `plugin/synthexis/src/editor.rs` (camera preview, 32 macro bars, Live and Test toggles, two hand-role combos). Wireframes: [FigJam board](https://www.figma.com/board/WJIE59w34hCSTXuq3XpKGH?node-id=18-1009), area "Plugin UX (proposal)" from x 31000, right of all earlier content, in twelve sections: 1 Edit view, 2 Components, 3 Move row, 4 Target picker, 5 Assistant, 6 Objects, 7 Profiles, 8 Play view, 9 Flows, 10 Scope, 11 Spec and questions, 12 Architecture and build plan. Spec text for the new `ui.*` nodes is in the last section of this file.

## Noa's picture of the window

- A target picker first: a mixer track, then a plugin on it, or simply the plugin FL has in focus. The mapping assistant then only sees that plugin's parameters.
- Under the camera view a list of assigned moves. Per move a dropdown for the hand, then the finger, then the movement (whole hand, fist open, spread, pinch, pinch-drag, pinch-turn, finger), with icons: a fist, a spread hand, a finger.
- Objects you add, which the camera recognises, and which you can name so the name helps the assistant.
- Profiles 1, 2, 3 with presets per hand and per object, switchable live, saved.
- The assistant, typed or spoken, stays optional. Everything can be routed by hand.

## Facts that shape the design

- The picker is worth 10 to 14 points of target accuracy. On 238 held-out sentences the resolver picks the right parameter first in 0.676 of cases against the picked plugin only, 0.580 against the 6 probe plugins (2,449 parameters) and 0.538 against a project of 30 plugins (3,348) (`probes/open-vocab-mapping/picker.py`).
- The camera students are closed-set segmenters with one generic `object` class (`perc.seg`: background, person, face, eye, mouth, hand, object). A typed name cannot teach them a new kind of object at runtime.
- The plugin today emits brightness, motion and a test triangle (`plugin/synthexis/src/signal.rs`). Hand and face signals arrive with the students (training from 11-01) or with the Apple Vision fallback at 10 to 15 fps (`docs/plan.md`, risks). Every move below needs one of the two.
- FL gives scripts plugin and parameter names, but no units and no groups; value strings only for some plugins (SPEC_PROPOSAL, `map.param_sense`).
- Writes into a third-party plugin go through the FL script (`bridge.fl_script`, record-event path). A live write into a commercial plugin through FL is not tested yet (roadmap). Without the script, synthexis drives only its own 32 macros, which the user links in FL.
- The parser takes 9 ms per sentence on one CPU thread, so the diff can appear while the user still looks at the field.

## Main path: assign by clicking

Decided by Noa on 11-01: mapping by clicking comes first and has to work every time. The parser is not reliable enough to carry the product; it stays a shortcut.

What this means for the design:
- No model sits between a click and a route. Every menu entry comes from a fixed catalog of movements (`ui.catalog`), and every choice writes a route as plain data. The assistant produces the same edit commands; it has no write path of its own.
- The menus offer only what the camera can measure. Hands and fingers appear when the hand student runs, face parts when the segmenter delivers face, eyes and mouth, and an object only after the camera has found it and Noa has added it. Entries the perception cannot deliver are not offered, so a route on something the camera never sees cannot be made.
- Each offered entry shows live whether it is seen right now. A new row shows the raw value of its movement before anything else, so a movement the camera does not track is visible at once.

What "100 %" covers: a click puts exactly the chosen route into the plugin, it writes to the chosen target, and it is saved and restored with the project. That is ordinary software and can be tested to 100 % (see Architecture). Two things sit outside it and the UI shows both instead of hiding them: how well the students measure a fist or a finger (their gate in `budget.quality`: hand mIoU 0.85, finger states 0.95 on held-out data), and FL writes into third-party plugins, which are not tested yet.

## Layout

One window, two views. A button in the top bar switches them.

Edit view, default 1000 x 720 pt, resizable. Top bar, camera canvas and assistant line form the upper part; the panel sits below:

```
+------------------------------------------------------------------------------------------+
| synthexis  Target: Mixer 3 Pad > Serum 2 v  (o) follow FL   [1][2][3][+]   Live FL  Play |
|                                                                                          |
|     L . Param                                               R . MIDI                     |
|     [fist] fist->open 0.62                                  [point] shape "point"        |
|                                   [ Can #2 ]                                             |
|                                                                                          |
| [mic] Optional: type or say it instead of clicking ...  in: Serum 2 (Pad) v  undo  redo  |
+-----------------------------------------------------------+------------------------------+
| MOVES                                                     | HANDS                        |
| Left hand                                                 | Left  Param|MIDI|Motion|Off  |
|  [fist]   Whole hand  Fist<->open > Filter 1 Freq         |       Filter hand, 2 moves   |
|           ||||||....  2.4 kHz   direct   inv  mute  ...   | Right Param|MIDI|Motion|Off  |
|  [spread] Whole hand  Spread      > Filter 1 Res          |       Chord hand, 1 move     |
|           |||.......  18 %      direct   inv  mute  ...   |                              |
|  + add move                                               | OBJECTS                      |
| Right hand                                                | [img] Can  tracked  1 move   |
|  [point]  Index       Shown       > Chord Am  (MIDI)      | + show an object             |
|  + add move                                               |                              |
+------------------------------------------------------------------------------------------+
```

Play view: the camera fills the window. A glass strip at the bottom holds the profile tabs and one chip per active move with its live value. Nothing is editable there. It is the view for performing and for the video.

Design language (decision D8, `chiasma/docs/STYLE.md`): the camera is the work and runs edge to edge under the top bar and the assistant line. Tool surfaces are dark frosted glass. Chrome text and icons are white at fixed alphas. Saturated colour appears only on things that carry a value: value bars, active toggles and the hand skeletons. Each hand gets one colour, used on its skeleton in the canvas and on the icon of its rows, so a row and its hand match at a glance.

Icons: Phosphor (MIT, crate `egui-phosphor`) has hand glyphs for fist, palm, pointing, grabbing, waving and peace; exact glyph names still to check. Pinch, spread and pinch-turn need three small glyphs drawn with the egui painter.

## Components

### 1. Top bar

Left to right: name, target picker, profile tabs, Live toggle (the existing `live` parameter), bridge status, view switch. The latency line moves here from the log. Bridge status is a dot: filled when the FL script answers, hollow when only macros work. A click on a hollow dot opens the setup steps.

### 2. Camera canvas (`ui.canvas`, amended)

- Camera image, masks (toggle), hand skeletons in the hand's colour.
- A tag per hand with side and role, "L . Param".
- The motion readout next to each hand (`motion.readout`), for example "fist->open 0.62" or "throw -> right edge 0.62".
- Objects with name tag and track id.
- Feedback from the moves list: hovering or editing a row lights the body part it reads (palm, fingertip, mouth) and shows the raw source value there. A lost source shows "hold" while `cond.occlusion` holds the value.
- Click on a hand: opens that hand's group in the moves list. In object pick mode, a click on an object mask adds it to the object list.
- A small glass menu in a corner: camera choice, mirror, masks on or off. Picture-in-picture for a second camera comes later.

### 3. Target picker (`ui.target_picker`, new)

The picker names the plugin that new moves, presets and sentences are about. It sits in the top bar as a breadcrumb: mixer track > plugin.

Opening it shows the project as a tree: each mixer track by number and name, under it the generators routed to it (channel rack) and its effect slots. A search field sits on top. Two entries sit above the tree:
- Follow FL focus: the picker follows the last plugin window Noa focused in FL, ignoring synthexis' own window. A pin icon freezes the current choice. Measured in `probes/fl-target-picker/` and built on 11-03 (see "Built so far" below).
- Whole project: the resolver searches every plugin. The assistant line then says which plugin it chose.

What the picker scopes:
- the resolver of the assistant searches only the picked plugin's parameters;
- "+ add move" offers the picked plugin's parameters first;
- presets bind to the picked plugin.

Existing rows keep their own target. Changing the picker never rewires them.

Several plugins of one kind: entries carry the track, "Serum 2 . Pad (Mixer 3)" and "Serum 2 . Bass (Mixer 5)". Two instances on one track add the slot number.

The first time a plugin is picked, synthexis reads its parameter list through the FL script and param-sense tags each parameter (`map.live_inventory`, proposal). The picker shows a short progress note while it reads. Lists get long: Serum 2 has 2,621 parameters in the AU harvest, so the parameter list needs a search and class sorting from day one.

Built so far (11-03, plain egui styling; `plugin/synthexis/src/fl.rs`, `sentence.rs`, `sense.rs`), tested in FL with Noa:
- The plugin opens the bridge's MIDI ports itself; the FL script (`plugin/fl/device_synthexis_bridge.py`) sends the project's plugins and every plugin-window focus.
- One list, as wide as its button: "Follow FL", then the project by mixer track. A track with one plugin is one row; a track with two or more is a folding group (arrow, count), Sound (generators routed to it) above FX (its slots), slot or channel number right-aligned, full name on hover. The list grows with opened groups up to 420 px, then scrolls; folding a group never closes it, picking a row does.
- Picking a row or "Pin" freezes the target; "Follow FL" or "Unpin" releases it. A pin whose slot now holds another plugin is dropped.
- Parameter names load on pick (1 to 14 ms); classes come from the labels of the 887 mapped plugins, else from param-sense in Rust (16 ms for 2,600 names), so no prompt is needed for an unmapped plugin.

Idea (Noa, 11-03, to work out in Figma): a frosted overlay over the whole plugin window as the picker, like FL's own overview of plugin windows with preview images (Noa did not remember how FL opens it; FL's Plugin Picker is on F8, from memory, not checked). The same structure as the list: mixer tracks fold open, previews large enough to recognise a plugin, the overlay scrolls.

### 4. Moves list (`ui.moves`, new; replaces the routes part of `ui.panel`)

This is the main path for mapping. One row is one route (`route.matrix`) with its conditioning (`cond.chain`). Rows are grouped by who moves: Left hand, Right hand, Both hands, Face and head, then one group per named object. Each group ends with "+ add move".

| Cell | Content |
|---|---|
| Icon | movement glyph in the hand's colour |
| Who | Left hand, Right hand, Either hand, Both hands, Face, Head, Body, an object by name |
| Part | Whole hand, Thumb, Index, Middle, Ring, Pinky, Finger shape |
| Movement | filtered by part, each with an icon (lists below) |
| Target | plugin > parameter, a MIDI target, or Signal N |
| Value | two thin bars, source (white) and what reaches the target (accent), plus the value in plugin units when known, else percent |
| Quick | invert, mute |
| More (...) | expands the row: range (two-handle slider), curve, smoothing, hold on loss, gate ("only while"), clutch, quantize, path, delete |

The Who, Part and Movement menus come from the catalog, filtered by what the perception delivers at the moment (see Main path). Each entry carries a dot: filled while the camera sees it, hollow while it does not. Movements per part, with the source ids from `tools/mapping/ontology.yaml`. Proposed sources (SPEC_PROPOSAL, ontology v1.1 and demo requests) are marked P, planned motion primitives M.

Whole hand:
- Fist <-> open: `hand.openness`
- Spread: `hand.spread`
- Pinch, as a value or as a gate: `hand.pinch`
- Pinch-drag up/down or left/right: `hand.y` / `hand.x` under the clutch (`cond.clutch`, P), a virtual fader
- Pinch-turn: `hand.roll` under the clutch (P), a virtual knob
- Move left-right, up-down, toward the camera: `hand.x`, `hand.y`, `hand.depth`
- Tilt sideways, tip at the wrist: `hand.roll`, `hand.pitch` (P, centred)
- Finger count: `hand.finger_count` (P)
- Speed: `hand.speed`
- Throw, swipe, circle, hold, grab, release: `motion.*` (M)

Thumb, Index, Middle, Ring, Pinky:
- Bend: `hand.finger.<f>`
- Point up/down: `hand.finger.<f>.tilt` (P), gated by that finger being shown
- Shown: a trigger while the finger is extended

Finger shape: five toggles thumb to pinky, so any mask works (P, "shapes"), or a named shape (fist, point, peace, rock and the others in the ontology). Its movement is "held", a trigger.

Both hands: distance, orb size, orb x, orb y, orb rotation, clap. Face and head: mouth open, smile, eye open per side, wink, head turn, nod, tilt. Object: x, y, size, angle, speed.

The order in the movement list follows the hand's role. Parameter puts continuous movements first, MIDI puts shapes and triggers first, Motion puts primitives first. All movements stay reachable, because roles mix per feature (`gest.hand_role`).

Target dropdown:
- Parameters of the picked plugin, sorted by param-sense class (Filter, Amp, Reverb and so on, because FL gives no groups), each with its current value. Search field on top; it matches names and classes, so "cutoff" finds Serum 2's "Filter 1 Freq".
- Learn: "move the knob you mean in the plugin". The FL script compares parameter values before and after and binds the one that changed (`map.language_learn`, P; polling third-party values through FL is not tested).
- XY pad: two parameters at once (`map.xy`, P).
- Another plugin: opens the picker for this row only.
- synthexis MIDI: note, chord, pattern, CC, generator property such as arp octaves or chord size (`midi.*`, `midi.prop`).
- Signal 1..32: a macro the user links in FL with Link to controller. This route works without the FL script.
- On/off: an effect's bypass (toggle target).

Row states:
- live: bars move;
- held: source lost, value held, "hold" tag;
- muted: dimmed;
- pending: from the assistant or a preset, outlined, with a + or - mark;
- sum: another row writes the same parameter, values add and clamp, tag "sum 2";
- missing: the target plugin is gone, red tag and a re-pick button.

Each row shows its path: "direct" through the FL script, or "S7" for Signal 7, with a one-line hint how to link it in FL.

Idea (Noa, 11-03): besides the rows, a node graph of the routings, styled like a FigJam board: hand and finger icons on the left, plugin parameters on the right, edges for the moves, each node opening into the same dropdowns as the row cells. Found in the FL test the same day: after a sentence the status said "sent, 2 pinned" and nothing showed what was mapped. Until the moves list or the graph exists, a sentence's result is invisible to Noa; the assistant line has to show the route it made.

### 5. Hands: roles and presets (`ui.hands`, new; UI of `gest.hand_role` and `gest.presets`)

A card per hand, one for face and head, one per named object. Each card has a role switch (Param, MIDI, Motion, Off), a preset dropdown and the number of moves. Off mutes the card's rows without deleting them.

A preset (Filter hand, Chord hand, Pattern hand; Pong and Orb as extras) arrives as pending rows bound to the picked plugin, the same way an assistant diff arrives. It can be checked and undone like any edit. Cards with the MIDI role add key, scale and target channel (`midi.scale`, `bridge.midi_out`).

Roles and presets belong to the profile.

### 6. Objects (`ui.objects`, new)

Each row: a thumbnail cut from the mask, the name (editable), the state (tracked, lost 3 s, not seen), the track id, and how many moves use it.

Adding an object: "Show an object" puts the canvas into pick mode. Noa holds the object up and clicks it. If the seg student marks it as `object`, it becomes a tracked entry called "Object 1" until renamed. If the student does not mark it, the click answers "not seen as an object". No silent failure.

What a name does:
- labels the object on the canvas, in the moves list and in saved profiles;
- lets the assistant address it. The resolver matches the object words of a sentence against the user's object names, the same text matching that binds unseen parameter names (runtime binding, `map.resolver`). Hypothesis, not measured: the parser tags unseen object words as object spans; it was trained on a fixed list (phone, ball, bottle, can, drumstick, mug, pen).
- shows up as a chip option when a sentence names an object vaguely ("the thing in my hand").

What a name cannot do:
- teach the camera a new kind of object. The student's weights are fixed; it sees "object" or nothing;
- tell two similar objects apart after both leave the frame. An object keeps its name while its track lives (`perc.track`, IoU and centroid matching). After a loss, the next object that appears asks "is this Can?".

Options beyond the demo, all research and unproven:
- Remember its look: store an appearance signature at naming time (colour histogram, or the pooled backbone feature under the mask) and re-identify after a loss. Same idea as `map.aliases`: stored prototypes instead of retraining.
- Pick a trained kind: only if a later student gets per-object classes (mug, can, drumstick). That costs labels and a retrain; it is not a plugin feature.
- Snapshot cut-out through the SAM 3.1 API (`snap.segment`, $0.0025 per image) as the stored look. Online, so opt-in and outside the play path.

For the demo: use objects that are in the recordings (the soda can, the drumstick). Those are what the student has seen as `object`.

### 7. Profiles (`ui.profiles`, new)

Decided by Noa on 10-31: a profile is a complete set of everything entered in the plugin, and switching is a click. Timeline and beat do not matter.

A profile holds:
- all moves with their settings;
- roles and presets per hand, face and object;
- the object list with names;
- the picked plugin;
- MIDI key, scale and channel.

Shared by all profiles: the camera choice and the learned words (`map.aliases`), because a word means the same thing in every profile.

This makes profiles for other plugins in the same project easy. Profile 1 drives Serum 2 on the Pad, profile 2 drives ShaperBox on the Bass, with the same hands and fingers.

Tabs in the top bar show numbers 1, 2, 3. A double-click renames a tab ("Pad", "Bass FX"). + adds a profile, up to 8; a new profile starts as a copy of the current one or empty.

Switching: a click on a tab, effective at once. At the switch:
- the moves of the old profile stop;
- every parameter stays where it is until a move of the new profile changes it;
- notes the old profile holds get note-offs, so nothing hangs.

Saving: profiles live in the plugin state (nih-plug `#[persist]`, JSON), so they are saved with the FL project and with synthexis presets in FL. Rows reference their target by track name, plugin name and slot. On load, a target that moved is rebound by name; a missing one shows the "missing" state.

Not planned: switching from the FL timeline, a MIDI pad or a gesture. If it is ever wanted, a host parameter "Profile" would cover automation and MIDI at once.

### 8. Assistant line (`ui.assistant`, new; UI of `map.agent` and `map.fallback`)

An optional shortcut: everything it does can also be done by clicking. Collapsed by default to a small field over the bottom edge of the canvas; it opens on a click or a key. The placeholder reads in both languages. A scope chip at the right, "in: Serum 2 (Pad)", comes from the picker; a click changes it for this sentence only.

After Enter:
- The sentence stays above the field. Every recognised term is underlined as a chip: source ("Faust"), target ("Filter"), plugin or track ("Pad"), modifiers (invert, range).
- The moves list shows the diff as pending rows: new rows with +, removed rows struck through with -, changed cells as old -> new.
- Pending rows already run, so Noa can try the move before keeping it. Discard restores the parameter values from before.
- Enter or Accept keeps the whole diff as one undo step. Esc discards it.
- A new sentence while rows are pending works on top of them ("nein, nimm Filter 2"), so a correction during listening needs no Esc first. Enter keeps the combined result as one step.
- Pending rows stay pending until Enter or Esc; nothing is kept or dropped silently. Decided by Noa on 10-31: every result has to be checked by ear before it is kept.
- Hand side: completion offers "open my fist" without saying which hand (Noa, 11-03). Gesture items need a side choice (left, right, either) when both hands can do the move, in the list or as a follow-up chip.
- Unsure terms (score below the calibrated threshold, or a small margin to the second candidate; `map.resolver`, P) open their chip with the top 3 and a search. Noa picks one, and the pick is stored as an alias (`map.aliases`, P), so the same phrase binds directly next time.
- A sentence the parser marks `none` gets "not a command". A `query` gets an answer line and highlights the rows it means.
- Voice: push-to-talk on the mic (`map.voice`, online, off by default; Noa decides). Later.
- Footer: "Built with Muse Spark Open" (`map.license`).

Risk to check early: typing into an egui text field inside FL. If FL takes the keystrokes as its own shortcuts, the field needs a fallback such as a small separate text window.

### 9. History (`ui.history`, new)

One undo stack per instance, for edits: a row change, a sentence, a preset, a profile rename. Playing is not history: live values and profile switches stay out. Undo and redo sit in the assistant line; cmd+Z and shift+cmd+Z work if FL forwards keys to the plugin window (unchecked); the sentence "undo" or "rückgängig" works too (intent `undo`). A sentence that made three routes is one step. 50 steps, not saved with the project. Undoing an edit that changed a target restores the parameter value from before (read through the FL script).

## Interactions

### First run

1. Noa inserts synthexis in the channel rack, as a generator, so its MIDI out reaches other channels (`bridge.midi_out`).
2. macOS asks for the camera once (P1). The canvas shows the camera with the hint "show both hands" until two hands are tracked; then the L and R tags appear.
3. Bridge check. If the FL script answers, the dot fills. If not, a card says: "Direct control is off. Install the synthexis controller script (2 steps). Until then, moves drive Signal 1..32, which you link in FL."
4. Target. The picker opens on Follow FL focus. Noa clicks the Serum 2 window in FL; the breadcrumb reads "Mixer 3 Pad > Serum 2" and synthexis reads its parameters.
5. The empty moves list offers three starts, in this order: "Add a move" (by clicking), "Load a preset" (Filter hand on the left hand), and the optional "Type it instead".
6. Live is on. Moves write as soon as they exist.

### Mapping by clicking (main path)

1. "+ add move" under Left hand. The row opens with Part on Whole hand and the movement list open, icons first.
2. Noa picks "Fist <-> open". The canvas lights the left palm and shows the live value; Noa opens and closes the hand to see it track.
3. The target list opens on the picked plugin's parameters, sorted by class. Noa types "cutoff" and picks "Filter 1 Freq": the search matches param-sense classes as well as names, so the word finds Serum 2's name for it. Or Noa clicks Learn and turns the cutoff knob in Serum 2.
4. The row runs: bars move, the cutoff moves in Serum 2.
5. Optional: ... opens range 20 to 80 %, curve, smoothing.

Pinch-turn as a virtual knob: Part Whole hand, Movement Pinch-turn. The clutch is on by default for this movement, so the knob moves only while pinched and stays where it is on release.

### Mapping by sentence (optional shortcut)

Picked plugin: Serum 2 on the Pad track. Noa types "Linke Faust auf öffnet den Filter, Finger spreizen macht ihn schärfer".

1. Parser and resolver run against Serum 2's parameters only.
2. Two pending rows appear under Left hand: "Whole hand . Fist <-> open -> Filter 1 Freq" and "Whole hand . Spread -> Filter 1 Res".
3. Both run at once ("währenddessen"). Noa tries them.
4. Enter keeps both as one step.

If "den Filter" matches Filter 1 Freq and Filter 2 Freq with a small margin, its chip opens with both. Noa's pick becomes the alias "den Filter" -> Filter 1 Freq for Serum 2.

### Switching profiles

Setup: profile 1 "Pad" (left fist opens Filter 1 Freq of Serum 2 on the Pad, right hand plays chords), profile 2 "Bass" (left fist drives the band mix of ShaperBox on the Bass, head tilt detunes).

Noa clicks tab 2. From now on the fist writes ShaperBox. The Serum 2 filter stays where it was. Chord notes held from profile 1 get note-offs. A click on tab 1 brings the Pad setup back.

### Several plugins of one kind

- Picker and target lists name each instance by its track: "Serum 2 . Pad (Mixer 3)", "Serum 2 . Bass (Mixer 5)".
- Rows bind to an instance, never to the plugin type.
- A sentence naming "Serum" while the scope is Whole project and two instances exist: the plugin chip opens with both, the picked one first. With a track word ("am Bass") the resolver takes that instance.
- The row menu offers "Copy to" another instance of the same plugin. Parameter names match, so the binding is exact.
- Two rows writing the same parameter add up (sum tag). Two synthexis instances writing the same parameter should warn on both; the engine is shared per process (D1), so instances could see each other's targets. Hypothesis, not built.

### Undo

Noa accepts the two-route sentence above, then types "nein, nimm die Resonanz vom Filter 2". The diff shows the resonance row with target "Filter 1 Res -> Filter 2 Res". Noa keeps it. One undo brings back Filter 1 Res. A second undo removes both routes of the first sentence and puts cutoff and resonance back to their earlier values.

## Architecture

Starting point in `plugin/synthexis/src/`: the engine fans signal samples out to subscribers (`engine/mod.rs`); `writer.rs` holds a fixed `Vec<Route>` from `route::default_routes` (brightness, motion, test triangle to Signal 1, 2 and 32), runs `cond::Chain` per route and writes macros on the main thread (P2 path). Routes are code, not data, and nothing is persisted except the editor size. The design below turns routes into data that the UI edits.

```
camera -> students -> geometry -> signals (sig.contract) ------------------- samples ---+
                                    |                                                    |
                         capabilities + presence                                         v
                                    |                                         engine thread:
UI menus = catalog filtered --> Command --> State --> compile --> RouteTable --swap--> cond.chain per route
assistant (optional) ---------> Command        (profiles, persisted)                     |
                                                                                          v
                                                       writers: macros (main thread) | FL bridge | MIDI out
```

Parts, each a small module:
- `catalog.rs`: the static table behind the menus (`ui.catalog`). Pure data, no model.
- Capabilities: the engine publishes which signal ids it can produce with the models it has loaded, plus `present` per entity from the latest samples. The menus are the catalog filtered by these ids. Today that list holds brightness, motion and the triangle; hands, fingers and face appear when the students load.
- `state.rs`: `State { profiles, active, camera, aliases }`, `Profile { name, routes, roles, presets, objects, picked, midi }`, `RouteSpec { id, source, target, cond, muted }`. `source` is a catalog id plus side or object id; `target` is one of `Macro(n)`, `FlParam { track, plugin, slot, param_index, param_name }`, `Midi(..)`. Persisted with nih-plug `#[persist = "state"]` as JSON with a version field. The plugin crate needs `serde` and `serde_json` as direct dependencies (the vendored nih-plug already uses both).
- `command.rs`: every edit is a `Command` (add, remove or change a route, set role, load preset, add or rename an object, add, rename or switch a profile). `apply(&mut State, Command) -> Command` returns the inverse, so undo is a stack of inverses. A pending diff is a group of applied commands not yet committed; Esc applies their inverses. The UI and the assistant both create commands; there is no second write path.
- `compile.rs`: turns the active profile into a `RouteTable` (signal ids interned to indices, one `cond::Chain` per route, targets resolved to writer slots) and publishes it with a pointer swap. The engine thread picks up the new table on its next sample; the audio thread never touches it (`sig.contract` rule).
- `route.rs`: `apply_sample` keeps its sum-and-clamp logic, generalised from `macro_index` to a target slot.
- Writers: macros through the existing main-thread path; FL parameters through `bridge.fl_script` (record-event path); MIDI notes through the plugin's MIDI out in `process()`.
- Rebinding on load: `FlParam` targets are matched by track name, plugin name, slot and parameter name; a target that cannot be found marks its row "missing" instead of writing somewhere else.

How the click path is tested to 100 %:
- Catalog: every entry's signal id pattern exists in `tools/mapping/ontology.yaml` and in the engine's list for the capability it names.
- Commands: random command sequences, then all inverses, give back the starting state exactly (property test).
- Persistence: state to JSON and back is identity, for every profile.
- Routing: a recorded signal fixture through a compiled table gives the expected target values (golden test), including sum, clamp, invert and range.
- In FL: the test bridge (`probes/fl-camera-plugin/bridge/`) adds a row by command, plays the fixture and reads the target back, for a macro and for a third-party parameter.
- In the window: a new row shows its live source value and the value it writes; "not seen" when the camera does not track the entity.

## Implementation plan

For the line that owns `plugin/`. Each step lands with its tests and leaves the plugin working.

1. `catalog.rs` and engine capabilities. Test: catalog against the ontology and the engine's signal list.
2. `state.rs` with persistence; today's `default_routes` become the routes of profile 1. Test: JSON round trip.
3. `command.rs` with history. Test: inverse property.
4. `compile.rs` and the table swap; `writer.rs` reads the table; macro targets only. Test: golden fixture.
5. Editor: moves list with the four menus, seen dots, live bars, invert, mute, range, delete; profile tabs; manual target list. Targets are Signal N until step 6.
6. FL parameter targets through the bridge, after the third-party write test in FL passes.
7. Assistant: parser and resolver output turned into commands, shown as pending rows. Optional.

For the freeze on 11-02: steps 1 to 5 are the demo core, the click path end to end on macros. Step 6 joins if the FL write test passes; step 7 can follow after the freeze. The hand menus fill as soon as the students deliver hand signals; until then the same code runs on the signals that exist.

## Scope: demo by 11-02 against later

Freeze is in two days, the UI comes last in the plugin roadmap, and the hand signals arrive with the students from 11-01. So the demo build should cover what the 2-minute playground part shows and nothing else.

| Element | Demo 11-02 | Later | Spec nodes it uses | New or amended | Depends on |
|---|---|---|---|---|---|
| Edit view layout, top bar with Live, bridge dot, view switch | must | latency in the bar | `ui.panel` | `ui.panel` amended | none |
| Camera canvas: hand tags, readout, row highlight, hold tag | must | masks toggle, PiP | `ui.canvas`, `motion.readout`, `cond.occlusion` | `ui.canvas` amended | hand signals |
| Moves list: who, part, movement, target, value bars, invert, mute, delete | must | | `route.matrix`, `cond.range` | `ui.moves` new | hand signals |
| Movements | must: fist<->open, spread, pinch, hand x/y, finger bend, finger shape held | pinch-drag/turn, tilt, finger count, primitives | `feat.hand.*`, `gest.finger_states` | proposals `cond.clutch`, `feat.hand.orientation` | students |
| Row detail | must: range | curve, smoothing, hold, gate, quantize | `cond.chain` | | |
| Targets | must: picked plugin's parameters (direct), Signal 1..32 | Learn, XY pad, On/off, MIDI targets in rows | `tgt.fl_param`, `tgt.macro` | proposals `map.language_learn`, `map.xy` | FL write into third-party plugin |
| Target picker | must: manual list, Whole project | Follow FL focus, pin | `bridge.fl_script` | `ui.target_picker` new | probe fl-target-picker |
| Hands cards: role, presets | should: role switch, Filter hand, Chord hand | Pattern hand, Pong, Orb, object cards | `gest.hand_role`, `gest.presets` | `ui.hands` new, `gest.hand_role` amended | |
| Profiles | must: tabs with numbers by click, rename, full settings per profile, saved in plugin state | export/import of one profile | `route.matrix`, `gest.hand_role` | `ui.profiles`, `plugin.state` new | |
| Assistant line: diff rows, accept, discard | should, optional shortcut | voice | `map.agent`, `map.parser`, `map.resolver` | `ui.assistant` new | resolver in the plugin (roadmap, open) |
| Chips with top 3 | should | aliases stored | `map.fallback` | proposal `map.aliases` | threshold on the 51 calibration rows |
| History | should | keyboard shortcuts | `map.agent` (undo) | `ui.history` new | |
| Objects: list, pick mode, name, chip | should | re-identification by look | `feat.object`, `perc.track` | `ui.objects` new, `perc.object_reid` new (research) | seg student with object class |
| Play view | should (for the video) | | `ui.canvas` | `ui.play_view` new | |
| First-run checks | must: bridge card | camera hint, coach marks | `bridge.fl_script` | `ui.first_run` new | |

Build order if time runs short, each step usable on its own (details in Implementation plan):
1. Moves list by clicking with targets as Signal N or direct, and the manual picker.
2. Profiles: tabs, full settings per profile, persisted state.
3. Assistant line with pending rows, accept, discard, undo.
4. Play view, objects with names, Follow FL focus.

## Playground part of the video (2 minutes, suggestion)

| Time | Shot | Shows |
|---|---|---|
| 0:00 | Noa clicks Serum 2 in FL; the breadcrumb follows (or Noa picks it from the list) | target picker |
| 0:10 | "+ add move": left fist -> Filter 1 Freq; Noa plays | mapping by hand |
| 0:30 | Noa types "Finger spreizen macht ihn schärfer"; a pending row appears; Noa tries it, Enter | mapping by sentence, diff |
| 0:50 | Right hand to MIDI, preset Chord hand; chords in key | roles and presets |
| 1:10 | Noa clicks profile 2 "Bass"; the same fist now drives ShaperBox on the bass | profiles |
| 1:30 | Noa shows the can, clicks it, names it "Can"; "Can höher, mehr Delay" | objects, if the student sees the can |
| 1:50 | Undo twice; Play view | undo, play view |

## Decisions and open questions

Decided by Noa on 10-31:
- A profile holds everything entered in the plugin; the host-visible role parameters go away.
- Profiles switch by a click on a numbered, renamable tab, at once, independent of the timeline.
- No profile switching by gesture.

- A sentence first shows its rows marked and already running, so the result can be checked by ear; Enter keeps, Esc restores.

Decided by Noa on 11-01:
- Clicking is the main mapping path and has to work every time; typing or speaking is an optional shortcut.
- Menus offer only what the camera can detect.

Nothing open from Noa's side.

## Spec proposal

For the main session to merge into `spec/synthexis.yaml`. Statements without a cited measurement are design decisions or marked hypothesis.

### ui.panel (replace)

"Window layout. Edit view, default 1000 x 720 pt, resizable: top bar over the top edge of the canvas (target picker, profile tabs, Live, bridge status, latency, view switch); camera canvas edge to edge; assistant line over the bottom edge of the canvas (ui.assistant); below, two columns: moves list (ui.moves) on the left, hand and object cards (ui.hands, ui.objects) on the right. Play view: ui.play_view. Glass language per D8: colour only on things that carry a value; each hand has one colour, shared by its skeleton and the icons of its rows."

### ui.canvas (amend)

"Per hand a tag with side and role in the hand's colour, the motion readout next to the hand, object names with track ids. Hovering or editing a move row lights the body part it reads and shows its raw value; a lost source shows 'hold' while cond.occlusion holds. A click on a hand opens its group in ui.moves; in pick mode a click on an object mask adds it to ui.objects. Corner menu: camera, mirror, masks."

### ui.target_picker (new)

"Breadcrumb mixer track > plugin in the top bar. Opens a tree of the project (mixer tracks by number and name; under each, the generators routed to it and its effect slots), a search field, and two entries: 'Follow FL focus' (the last plugin window focused in FL other than synthexis; pin freezes it) and 'Whole project'. The picked plugin scopes the resolver of ui.assistant, the parameter list of new rows and the target of presets; existing rows keep their targets. Instances are named by track ('Serum 2 . Pad (Mixer 3)'), plus slot when two share a track. Picking a plugin for the first time reads its parameters (map.live_inventory) and labels them with param-sense." Measured: resolver top-1 0.676 against the picked plugin, 0.580 against 6 plugins, 0.538 against 30 (probes/open-vocab-mapping/picker.py). How FL reports focus to a script: probes/fl-target-picker/.

### ui.moves (new)

"The main mapping path; it must work without the assistant. Assigned-moves list, one row per route, grouped by who moves (left hand, right hand, both hands, face and head, each named object), each group ending in '+ add move'. Row cells: icon, who, part (whole hand, thumb .. pinky, finger shape as a 5-bit mask or a named shape), movement filtered by part and ordered by the hand's role, target (parameter of the picked plugin sorted by param-sense class, learn, XY pad, another plugin, synthexis MIDI target, Signal 1..32, on/off), live value as two bars (source, output) with the value in plugin units when a value string is known, invert, mute, and a detail area for range, curve, smoothing, hold, gate, clutch, quantize and delete. Row states: live, held, muted, pending (+ / -), sum (several rows on one parameter), missing (target gone). Each row shows its write path: direct (bridge.fl_script) or Signal N (tgt.macro, linked in FL)."

### ui.catalog (new)

"Fixed table of every selectable movement: who, part, movement, icon, signal id pattern per sig.contract (e.g. cam{n}.hand.{side}.openness), kind (continuous, centred, trigger, event) and the perception capability it needs (hand keypoints, finger states, face parts, object track, motion primitives). The ui.moves menus show catalog entries whose signals the engine can deliver now, each with a live seen / not seen dot; objects appear only after they are added in ui.objects. A test fails when the catalog and tools/mapping/ontology.yaml disagree."

### ui.hands (new)

"One card per hand, one for face and head, one per named object: role (parameter, MIDI, motion, off; off mutes the card's rows), preset, number of moves; MIDI cards add key, scale and channel. A preset arrives as pending rows bound to the picked plugin, accepted or discarded like an assistant diff."

### gest.hand_role (amend)

"Roles are part of the profile (plugin.state). The skeleton's host-visible parameters hand_role_l and hand_role_r go away: they would show one profile's roles in FL's automation list and fight the profile switch."

### ui.objects (new)

"Object list: thumbnail from the mask, editable name, state (tracked, lost, not seen), track id, moves using it. 'Show an object' enters pick mode; a click on a mask the seg student marks as object adds a tracked entry, a click elsewhere answers 'not seen as an object'. A name labels the object on the canvas, in rows and in profiles, and is matched as text by map.resolver against the object words of a sentence. A name never changes perception: perc.seg is closed-set with one object class. After a track is lost, the next object asks whether it is the named one." Hypothesis: the parser tags unseen object words as object spans (trained on a fixed list of 7 names).

### perc.object_reid (new, research)

"Store an appearance signature when an object is named (colour histogram or pooled backbone feature under the mask) and re-identify the object after it leaves and returns. Prototype matching, no retraining." Not measured.

### ui.profiles (new)

"Profiles 1..8 (3 shown by default) as numbered tabs in the top bar, renamable. A profile is a full set of the plugin's settings: moves with conditioning, roles and presets per hand, face and object, the object list with names, the picked plugin, MIDI key, scale and channel. Shared by all profiles: camera choice and map.aliases. A new profile starts as a copy of the current one or empty. Switching is a click and takes effect at once: the old profile's moves stop, parameters keep their values until a move of the new profile changes them, and notes held by the old profile get note-offs. No switching from the host timeline, MIDI or gestures (Noa 2026-10-31)."

### plugin.state (new)

"Persisted with the project and with synthexis presets (nih-plug #[persist], JSON): profiles (each a full set of the plugin's settings: moves, roles, presets, objects with names, picked plugin, MIDI key, scale and channel), aliases and the camera choice. Rows reference targets by track name, plugin name and slot; on load a moved target is rebound by name, a missing one shows the row state 'missing'."

### ui.assistant (new)

"Optional shortcut for mapping; it writes the same ui.history commands as ui.moves and has no write path of its own. Collapsed by default over the bottom edge of the canvas; scope chip from ui.target_picker, changeable per sentence. On Enter, map.parser and map.resolver produce a diff shown in ui.moves as pending rows (+ new, - removed, old -> new for changed cells). Pending rows run at once so the move can be checked by ear; Accept keeps the diff as one history step, Discard restores the parameter values from before. A sentence typed while rows are pending is applied on top of them; pending rows stay until Accept or Discard (Noa 2026-10-31). Recognised terms are chips (map.fallback); a term below the resolver threshold or with a small margin opens its top 3 and a search, and the pick is stored in map.aliases. 'none' answers 'not a command', 'query' answers in a line and highlights rows. Mic for map.voice, off by default. Footer credit 'Built with Muse Spark Open' (map.license)." Check: keyboard input into an egui text field inside FL.

### ui.history (new)

"One undo stack per instance for edits (row edits, sentences, presets, profile renames), 50 steps, not persisted; live values and profile switches are not history. Undo and redo buttons, cmd+Z if FL forwards keys, and the undo intent. Undoing a target change restores the parameter value read before the edit."

### ui.play_view (new)

"Camera fills the window; a glass strip holds the profile tabs and one chip per active move with its live value. No editing."

### ui.first_run (new)

"Camera permission and a 'show both hands' hint until two hands are tracked; bridge check with a setup card when the FL script does not answer (moves then target Signal 1..32); target picker opened on 'Follow FL focus'; an empty moves list offering 'Load a preset', 'Say what you want' and 'Add a move by hand'."
