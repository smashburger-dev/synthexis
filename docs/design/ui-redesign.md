# UI redesign spec v2 (2026-11-04)

This spec answers `docs/design/ui-redesign-brief.md` (R1-R23) and settles every
number an implementer needs. It builds on the current code in this worktree
(line numbers refer to it as of today) and on Noa's glass language
(design-glass, design-rust, build-rust). The brief's three screenshots show the
state it replaces: the "follows FL" picker row, card sections, colliding finger
labels, a part editor whose Remove wraps alone, and a Pong section with its own
preset row and a Point/Flick choice.

v2 revises v1 after three reviews (requirement checklist, feasibility against
the code, friction for beginners and fast users). Section 9 logs every
objection and what this version did with it. The main changes: the window is
1200 wide by default so the chat field gets wider than today; picking a
built-in preset never wipes unrelated work; closing a section takes two
clicks and Undo lives longer; the layout reads the real window height, so a
host that refuses a resize still gets a correct layout; the background
cycles through the portfolio's dusk hues instead of resting on the hand
colours; the finger label lines bend around the fingers; and the units are
cut so no file has two owners.

The engine stays untouched except `engine/camera.rs` (R1). Another session owns
`engine/*` and `calib.rs`; the camera change is limited to the preview size and
the preview's type, adds no egui type to the engine, and the unit that does it
reads `git diff` on that file first.

Two rules from AGENTS.md apply to the whole job. The dither background
reimplements the portfolio's maths from the formulas in this spec; no code is
copied from `noa-portfolio`, and the commit says so. Numbers marked
"hypothesis" are estimates until the bench named next to them measures them.

## 1. The window, top to bottom

One column, full width. Default width 1200 pt (v1: 1000; section 2.5 says
why); the height follows the content (R12, section 2.8).

v3 (section 2.11) replaces the size: one fixed window of 720 x 520 pt, the
camera and the panel area split by a share per state, the top pickers
opening upward over the camera with frost behind every popup. The mockups
below keep v2's widths.

1. Camera. Edge to edge, height from the window plan (2.8). The top bar (name,
   Live, key, transport, Settings) still floats over its top 30 pt and fades
   after 2.5 s, except the Live dot and the Settings gear, which stay at alpha
   0.55 so a beginner always finds them. Its frosted copy goes; it becomes a
   flat tint `rgba(30,30,30,0.62)` (section 4). All of it uses the
   proportional font; numbers that change (BPM, bar.beat) sit in fixed-width
   slots (2.2).
2. A 1 px line, white 0.20, across the full width at the camera's bottom edge.
3. Top section, always there, 84 pt: plugin picker, chat field, preset picker
   with "+", and a second row with the plugin note, the routes strip, and
   Save as with Undo (R4, R5, R9).
4. A rule (2.4), full width.
5. The open feature sections in their open order (Hands, Pong), each followed
   by a full-width rule. None when no section is open (R5).
6. Advanced, the last row, folded by default (R8).
7. 24 pt of empty bottom room (R3).

Items 4 to 7 scroll inside the window when they are taller than the space the
window plan leaves; the camera and the top section never scroll. Everything
under the camera, and the camera's side wings, sit on the dither background
(R2); sections have no fill, no border and no radius (R3). Messages about
presets ("Loaded Pong", errors) appear as a toast at the bottom left (2.9).

Every mockup below uses one left column: headers and body rows start at the
left inset; canvases (hands, side view, node view) and the groups that belong
to them are centred and marked "centred".

### Mockup A: no preset picked (1200 x 820)

```
+----------------------------------------------------------------------------------------------------+
| synthexis  (o Live)  A minor  [] 140 BPM . 1.1                                         * Settings  |
|                                                                                                    |
|                       camera, 1280x720 source, cover crop, 675 pt high                             |
|                                                                                                    |
+====================================================================================================+
| [Plugin picker      v] [When I spread my fingers, open the filter           ] [Preset picker   v] +|
| No plugins in the project                                                     Save as...           |
+----------------------------------------------------------------------------------------------------+
| > Advanced   Camera on . 30 fps                                                                    |
|                                                                                                    |
+----------------------------------------------------------------------------------------------------+
```

`+` is dim (no section open). The empty line under Advanced is the 24 pt
bottom room. The area under the camera shows the dither field.

### Mockup B: Hands picked, a finger selected (1200 x 820, panel area scrolls)

```
+----------------------------------------------------------------------------------------------------+
|:::::::::::::|              camera, 328 pt, cover crop (V_MIN 0.7)                    |::::::::::::::|
|:: wing :::::|                                                                        |:::: wing ::::|
+====================================================================================================+
| [Sampler            v] [When I show my index, play a hi-hat                 ] [Hands           v] +|
| Channel 1              > Kept routes: 2                                       Save as...     Undo  |
+----------------------------------------------------------------------------------------------------+
| Hands  [Single | All | Routes]          Key [A v] [Minor v]   Calibrate fingers  Reset  |   x     |
| -------------------------------------------------------------------------------------------------- |
|                                     [Kick]                 (Empty)          centred               |
|                                       |                       |                                    |
|   [Hi-hat basic]---------.   \ \ \ \  |                       |  / / / /   .--------[Clap]         |
|   [Snare fill]---------. |    \ \ \ \ |                       | / / / /    |                       |
|                        | '-----\:::::\|                       |/:::::/-----'                       |
|                        '--------\:::::\                       /:::::/                              |
|                                  .:.:.:.                     .:.:.:.                               |
|              Left hand  [Fist] [Open hand]         Right hand  [Fist] [Open hand]                  |
| -------------------------------------------------------------------------------------------------- |
| (o) Left hand . index                                                          Hide roll    Remove |
| [Pattern | Chord | Held note] [1 . Sampler      v] [Next 1/16 v] [While shown | On / off | Once]    |
| [1   bars] [Lanes | Piano roll] [Template    v] (Back)                [Hi-hat basic      ]  Save    |
| +-------------+----------------------------------------------------------------------------------+ |
| | Closed hat  | 1   :   :   :   |   :   :   :   |   :   :   :   |   :   :   :   |               | |
| +-------------+----------------------------------------------------------------------------------+ |
| [Velocity | Repeats]  Fill  Add row                                                                |
| Click adds a note, drag moves it, drag its right edge to lengthen it, ...                          |
+----------------------------------------------------------------------------------------------------+
| > Advanced   Camera on . 30 fps                                                                    |
+----------------------------------------------------------------------------------------------------+
```

The hands turn 45 degrees outward. A thumb's node sits above its tip; the
other fingers' nodes sit on the outer side, each joined by a bent line: a short
leg out of the tip along the finger, then horizontal into the gutter beside
the canvas, then to the node (R13). `(Back)` is a reserved slot that holds the
Back button when an undo slot exists. The `x` at the header's right end closes
the section after a second click (R5, point 6); a thin vertical rule and 16 pt
separate it from Calibrate fingers and Reset.

### Mockup C: Pong picked

```
+----------------------------------------------------------------------------------------------------+
|:::::::::::::::::|      camera, whole 16:9 frame (contain), walls drawn       |::::::::::::::::::::::|
|::: dither wing :|                                                            |::: dither wing ::::::|
+====================================================================================================+
| [Plugin picker      v] [                                                    ] [Pong            v] +|
| No plugins in the project                                                     Save as...     Undo  |
+----------------------------------------------------------------------------------------------------+
| Pong  (o On)  (Live is off)                            0 flying . 645 hits . 91 dropped  |   x     |
| -------------------------------------------------------------------------------------------------- |
|                       Shooting hand [Left | Right | Both]              Test shot     centred        |
|                            . . . . . . . . [Pong top] . . . . . . . .                              |
|                            [Signal 11]                    [Signal 12]                              |
|                            . . . . . . . [Pong bottom] . . . . . . .                               |
|                       [1.90 screens/s Speed][5 Bounces][Never Time limit][32 Max balls]            |
|                       (o Harder shot plays louder) (o Hard shot flies faster)                      |
|                                 (o Mute the shooting hand's parts)                                 |
| -------------------------------------------------------------------------------------------------- |
| Click a wall to choose what a hit does.                                                            |
+----------------------------------------------------------------------------------------------------+
| > Advanced   Camera on . 30 fps                                                                    |
+----------------------------------------------------------------------------------------------------+
```

The Point/Flick choice and the in-section preset row are gone (R21, R23). One
centred column 460 wide holds the shooting hand row, the side view, the four
compact sliders (their widths add up to exactly 460) and the toggles, which
wrap as centred lines when they do not fit. "Live is off" shows only while
Live is off; a click turns Live on. The stats sit in the header (R22).

### Mockup D: Hands + Pong via "+"

```
+----------------------------------------------------------------------------------------------------+
|:::::::::::::|              camera, 328 pt                                            |::::::::::::::|
+====================================================================================================+
| [Plugin picker      v] [                                                    ] [Hands           v] +|
| No plugins in the project                                                     Save as...  +--------+
+-------------------------------------------------------------------------------------------| Hands  |
| Hands  [Single | All | Routes]          Key [A v] [Minor v]   Calibrate fingers  Reset  | Pong   |
|   ... hands, part editor as in B ...                                                      +--------+
+----------------------------------------------------------------------------------------------------+
| Pong  (o On)                                           0 flying . 0 hits . 0 dropped     |   x     |
|   ... as in C ...                                                                                  |
+----------------------------------------------------------------------------------------------------+
| > Advanced   Camera on . 30 fps                                                                    |
+----------------------------------------------------------------------------------------------------+
```

The "+" menu is open: it is right-aligned to the "+" button because a
left-aligned popup would pass the window edge. "Hands" is greyed out (already
open), "Pong" is live. After the pick, Pong opens under Hands and the panel
area scrolls.

### Mockup E: Settings open

```
+----------------------------------------------------------------------------------------------------+
|  window dimmed: rgba(30,30,30,0.78) over everything                                                |
|                +--------------------------------------------------------------+                    |
|                | Settings                                                   x |                    |
|                | -------------------------------------------------------------|                    |
|                | Language       [English | Deutsch]                           |                    |
|                | Suggestions in the sentence field follow the language. ...   |                    |
|                | (o Suggestions while typing)                                 |                    |
|                | Tab takes the grey suggestion, the arrow keys pick ...       |                    |
|                | Cmd+Enter keeps the suggested routes, Cmd+Backspace ...      |                    |
|                | Finger calibration   Calibrated                              |                    |
|                | These settings belong to your user account and apply ...     |                    |
|                | -------------------------------------------------------------|                    |
|                | Saved with the project                                       |                    |
|                | > Signals 1-32 (Link to controller)                          |                    |
|                | > Diagnostics                                                |                    |
|                +--------------------------------------------------------------+                    |
+----------------------------------------------------------------------------------------------------+
```

Open, "Signals 1-32" shows 8 rows of 4 sliders; "Diagnostics" shows the camera
lines, "Pong burst x16" and the log (R8). The account note sits directly under
the account settings; the project's things sit under their own heading.

### Mockup F: routing suggestions overlay (R9)

```
+----------------------------------------------------------------------------------------------------+
| [Sampler            v] [                                                    ] [Hands           v] +|
| Channel 1              v Bottom wall . Suggested routes: 2  x  Keep  Discard  Save as...           |
+----------------------------------------------------------------------------------------------------+
|  +--------------------------------------------------------------------------------------------+    |
|  | Right hand openness -> Sampler Cutoff                                                      |    |
|  | <> Right hand pinch -> Sampler Resonance, 20 to 80 %                                       |    |
|  | Kept                                                                                       |    |
|  | x  Left hand spread -> Signal 11                                                           |    |
|  +--------------------------------------------------------------------------------------------+    |
| Pong  (o On)                           (covered part of the section under the overlay)             |
+----------------------------------------------------------------------------------------------------+
```

`<>` is the flip button, `x` in the overlay removes a kept route, `x` in the
strip drops the wall scope. The overlay floats over the sections; the chevron
in the strip folds it away while Keep and Discard stay reachable. When Hands is
open and shows these routes in its Routes view, the overlay stays folded and
only the strip shows (R19).

### Mockup G: Hands in the Routes view (R19)

```
|   (hands view as in B)                                                                             |
| -------------------------------------------------------------------------------------------------- |
|                                                       Serum 2                                       |
| [Left hand ]----[Spread       ]:::::::::::::::::::::::[Cutoff                 ]                     |
|                                                       Signals                                       |
| [Right hand]-+--[Openness     ]:::::::::::::::::::::::[Signal 11              ]                     |
|              +--[Pinch        ]- - - - - - - - - - - -( Resonance             )   pending          |
|              +--[Index shown  ]:::::::::::::::::::::::[Trap roll              ]                     |
```

`:::` is an edge drawn as dither dots that switch from the source colour to the
target colour in the middle half. `- - -` is a pending edge (6 pt dashes);
`( )` is a hollow pending target. Group labels ("Serum 2", "Signals") take a
row each.

## 2. Layout system

### 2.1 Tokens (glass.rs `Tok`)

| Token | New | Old | Use |
|---|---|---|---|
| `INSET` | 12 | `INSET_W` 10 | window edge to content, left and right |
| `GAP` | 8 | 6 | between controls in a row |
| `ROW_GAP` | 8 | (item spacing 6) | between rows |
| `CTRL_H` | 24 | `FIELD_H`/`TOGGLE_H` 22 | every button, toggle, segmented, dropdown, field |
| `CTRL_H_SM` | 20 | | Pong's secondary settings only (R22 "smaller") |
| `TOP_H` | 30 | | top row: both pickers, the chat field, "+" |
| `HEAD_H` | 36 | `HEADER_H` 34 | a section's header row |
| `POP_ROW_H` | 26 | 24 (`pop_row`), 26 (`pick_row`) | every popup list row |
| `SEC_BOTTOM` | 16 | 10 | space under a section's last row |
| `BOTTOM_ROOM` | 24 | 6 | under the last row of the window |
| `BODY` | 13 | 12.5 | all readable text |
| `TITLE` | 15 | 13 | section titles (STRONG alpha) |
| `SMALL` | 11 | 10.5 | in-field labels, compact values, ruler, note names, badges |
| `HEAD` | 18 | 18 | the window's name and the Settings title |
| `LABEL` | 0.56 | 0.50 | labels on the dither (contrast, 4.2) |
| `HINT` | 0.52 | 0.40 | hints on the dither (contrast, 4.2) |
| `RULE` | white 0.10 over black 0.35 | `SEP_SOFT` 0.10 | rules between and inside sections (2.4) |
| `CAM_EDGE` | 0.20 | `EDGE` | the line under the camera |

glass.rs keeps the old names as aliases of the new values (`INSET_W = INSET`,
`FIELD_H = CTRL_H`, `TOGGLE_H = CTRL_H`, `HEADER_H = HEAD_H`) so untouched call
sites compile. Spacing scale: 2, 4, 8, 12, 16, 24. No other gap appears in new
code.

### 2.2 Fonts (R10)

Four sizes and no more: 13 for everything a person reads as a control, row,
hint or chat text; 11 for secondary text inside or beside a control
(slider_field labels and the compact Pong values, roll ruler, note names, chip
badges); 15 for section titles; 18 for the window name and the Settings title.
A title is not a sibling of the controls next to it, so R10's "same size for
siblings" holds.

The reference is the plugin picker. It draws with `TextStyle::Button`
(`Tok::BODY`, editor.rs:277), so moving BODY to 13 moves the picker and every
glass control together. The hardcoded sizes go: sentence.rs 14 (field, chips,
ghost, list rows, route lines) -> 13; `pick_row` 15 -> 13; track group header
15 -> 13; Sound/FX label 13 stays; chip badge 9 -> 11; list hint 11 stays;
variant panel 12/13/11 -> 13/13/11. Hierarchy comes from alpha (STRONG 1.0
titles, VALUE 0.80 controls and values, ROW 0.56 rows, LABEL 0.56, HINT 0.52)
and the one title step.

The font family stays egui's default (Ubuntu-Light); switching to Inter is an
open question for Noa. The top bar drops the monospace font: the whole line is
proportional, and each changing number draws right-aligned in a slot as wide as
the same string written with "8" digits (`glass::num_slot`), so nothing
jitters. The Pong stats use the same slots.

The Unit 7 capture compares the chat field and the plugin picker side by side
in mockup A. If the field text reads thin next to the picker, BODY goes to 14
for every control (one token, nothing else changes).

Every stock `ui.button`/`ui.small_button` in the window becomes `glass::quiet`
(Pin/Unpin go, Keep/Discard/flip/remove become quiet), so siblings share one
size: Load and Save in the Pong row differed only because one sat in a disabled
scope next to a stock `TextEdit`; that row goes, and the new Save as row uses
the same `glass::quiet` and `glass::text_field` as the part presets.

### 2.3 Capitalisation (R10)

Sentence case for every string that stands alone on a control, list row,
title, header, placeholder or message line: "Hide roll", "Save as...", "While
shown", "Next 1/16", "Never", "Speed". Units after a number stay lowercase
("1 bars", "1.90 screens/s", "3 s"). Strings spliced into a sentence stay
lowercase: route words (`r_*`), wall names inside a line (`w_*`), notes after a
" . " (`n_*`). Plugin, preset, part and channel names keep their own case. A
test (Unit 1) checks the first letter of every key in the list of standalone
keys.

### 2.4 Separators and edge-to-edge sections (R3)

- Sections draw no frame. `glass::panel` stays only for the test harnesses.
- A rule is two 1 px lines: white 0.10 on top, black 0.35 directly under it,
  so it reads over dark and over lit dither cells.
- Between sections: a rule from x = 0 to the window's right edge, painted with
  a clip rect of the whole window (the section's own `Ui` is inset).
- Under a section header: a rule inset by `INSET` on both sides (design-glass:
  separators inside a surface stop at its inset).
- Under the camera: 1 px, white 0.20, full width.
- A section's content `Ui` has `max_rect` = window width minus `INSET` on both
  sides; the header row is `HEAD_H`; body rows start `ROW_GAP` under the header
  rule; `SEC_BOTTOM` 16 under the last body row; then the rule.

### 2.5 Top section grid (R4)

Historical for the numbers: v3 (2.11) sets the picker width to
`clamp(round(W * 0.19), 136, 220)`; the grid's structure stays.

The brief asks for a wider chat field. Today it is about 690 pt (screenshot 1:
x 287 to 977 pt in a 1000 pt window). At W 1000, two pickers of a usable width
plus "+" leave at most 558 pt, so v2 makes the window 1200 wide by default
(the window asks the host for it, 2.8; old projects grow to it on open). The
chat field then gets 718 pt. Noa can still choose to stay at 1000 (open
question); the grid below works at any width.

Content width `C = W - 2 * INSET` (1176 at W 1200). Picker width
`P = clamp(round(W * 0.17), 170, 220)` (204 at 1200). Both pickers have the
same `P` and `TOP_H`.

| Column | x from | width | row 1 (30) | row 2 (24) |
|---|---|---|---|---|
| plugin | `INSET` | `P` | plugin picker | plugin note (13, HINT, cut with "...", full text on hover) |
| chat | `INSET + P + GAP` | `C - 2*P - 2*GAP - 34` | chat field | routes strip (R9) |
| preset | chat right + `GAP` | `P` | preset picker | Save as... left, Undo right (R4) |
| plus | preset right + 4 | 30 | "+" (30 x 30) | |

At W = 1200: plugin 12..216, chat 224..942 (718 wide), preset 950..1154, plus
1158..1188, which ends on the right inset. At W = 1000: P 170, chat 584.
Padding: 12 above row 1, 6 between rows, 12 under row 2: 84 in all. Row 2 is
always reserved, so the section height never jumps.

### 2.6 Rows inside sections

- A control row is `CTRL_H` high; rows sit on a 32 pt pitch (`CTRL_H + ROW_GAP`).
- Every control row starts at the left inset. A right group (Hide roll and
  Remove, the part name and Save, the Pong stats) aligns to the right inset.
  Items in a row share one vertical centre.
- Canvases (the hands, the Pong side view, the node view) centre in the content
  width. Controls that belong to a centred canvas centre as one group under or
  above it (`glass::centered_row`); all other rows start at the left inset.
- Why the part editor's rows are left-aligned although the brief says "the
  same centring as the rows above" (R17): the rows above it, the owner row and
  the editor row, start at the left inset too, and the roll under them spans
  the full content width with its lane names on the left. Left-aligned tools
  rows therefore share one edge with everything they act on. The Unit 7
  capture of mockup B checks this against the brief; if it reads wrong there,
  the tools row moves into `centered_row` (one call).
- A slot that holds a control only sometimes (Back) is reserved at its full
  width while empty, so the row never shifts.
- Hints: 13 at HINT, at most two lines, under the thing they explain.

### 2.7 Pickers and popups

Historical for placement, row height and the popup fill: v3 (2.11) opens
the top pickers upward, wraps the plugin picker into columns, makes rows
22 high and gives every popup frost.

The plugin picker's look becomes the one picker style (`glass::picker_button`
+ `glass::picker_popup` + `glass::pick_row`, moved out of sentence.rs:941-1012
and 352-376): a button with the field fill (white 0.05), 1 px edge white 0.13,
radius 6, text 13 at VALUE (HINT when it shows a placeholder), cut with "...",
and the filled triangle at 12 pt from the right edge. The popup is as wide as
its button (at least 200), radius 10, padding 8, rows `POP_ROW_H` 26 with the
name left and a place text right (13, HINT); the selected row gets the
selection fill; a disabled row draws at OFF 0.25 and takes no click. A header
row (HINT, not clickable) can separate groups. The popup opens under its
button, left-aligned; when that would pass the window's right edge it aligns
to the button's right edge (`pivot = RIGHT_TOP`). The list grows to 420 and
scrolls beyond.

A row that stands for a file (saved presets, saved parts) shows "Delete"
(quiet_dim, 13) at its right end on hover. The first click turns it into
"Delete? Click again"; the second deletes. The same two-click arm closes a
section (R5.6). The letter `x` stays for things that leave the view and can be
brought back: closing Settings, dropping the wall scope, removing a kept route
(Undo exists in the overlay).

Users: plugin picker, preset picker, the "+" menu, the part picker (R17).
`glass::dropdown` (the ComboBox) stays for small value choices (key, channel,
start, roles).

### 2.8 Panel area and window size (R12)

Historical: FL does not honour a plug-in's resize. v3 (2.11) drops the
resize requests (`ResizeGate`, `window_plan`, `Settle`) and gives the
window one fixed size; the text below records v2.

The window plan, pure functions in editor.rs:

```
W         = actual window width; the window asks for W_PLAN = 1200
cam_full  = floor(W * 9/16)                                // 675 at W = 1200
below     = settled height of: top (84) + rule + open sections + Advanced + BOTTOM_ROOM
max_h     = min(900, screen_h - 60)  where screen_h = NSScreen.mainScreen.visibleFrame.height,
            else 820 when the screen cannot be read
want_h    = clamp(cam_full + below, MIN_H = 560, max_h)    // the wish sent to the host
H         = actual window height, read every frame from the root Ui's max_rect
cam_min   = max(320, round(0.4 * H))
cam_h     = clamp(H - below, cam_min, cam_full)
```

The layout always uses `H`, never `want_h`. If the host ignores a resize, the
camera takes what the real window leaves, the panel area scrolls, and nothing
is blank or clipped.

`below` settles: it is measured every frame, but the plan takes a new value
only two frames after the open-section list or `W` changed. Content that grows
or shrinks inside a section (a selected finger, Hide roll, Pattern to Chord, a
wrapping gate row) goes into the scroll area and never moves the camera, so a
button never slides away under the pointer. The camera height animates over
0.18 s (`cam-h`) when the plan changes.

Examples at W 1200 on a screen where `max_h` is 820: no preset 675 + 145 = 820,
camera 675. Hands open (below about 780): 820, camera 328, panel area 491 of
which 84 are the top section. Pong open (below about 560): 820, camera 328
(estimates from the row heights in section 2).

Selecting a finger, a shape or a wall scrolls the panel area so the part
editor's owner row sits at the top of the visible panel area
(`ui.scroll_to_rect(owner_row, Some(Align::Min))`), because at these heights
the editor would start below the fold.

Resize requests (`ResizeGate`): the window asks the host for `(W_PLAN, want_h)`
when either differs from the actual size by more than 1 pt and the last
request is at least 150 ms old. If 500 ms after a request the window has not
reached the asked size, the host refused: the gate logs it once and stops
asking for 10 s. The size lands in the project by itself, because
`editor_state` is persisted. The default becomes
`EguiState::from_size(1200, 820)` (params.rs), the planned no-preset size, so a
new instance does not jump on its first open.

This needs three vendor patches (Unit 0), recorded in `plugin/vendor/PATCHES.md`:

1. `nih_plug_egui/src/lib.rs:116`: `set_requested_size` becomes `pub`.
2. `nih_plug_egui/src/editor.rs:111-123`: today `requested_size.swap(None)`
   runs, then `context.request_resize()`, and only then
   `egui_state.size.store(new_size)`. During the request `EguiEditor::size()`
   (136-146) still returns the old size, so the VST3 wrapper
   (`vst3/view.rs:136`, which on macOS runs synchronously from `schedule_gui`)
   and the standalone (`standalone/wrapper.rs:485-493`) resize to the old size.
   The patch stores the new size first, keeps the old one, calls
   `request_resize()`, and restores the old size if it returns false.
   `queue.resize` gets physical pixels, `round(w * egui_ctx.pixels_per_point())`,
   so a Retina window does not lay out at half size until the next `Resized`
   event.
3. `src/wrapper/vst3/view.rs:150`: the `debug_assert_eq!` that panics in debug
   builds when the host denies a resize becomes a `nih_log!` line; the function
   returns false as before.

`vst3/context.rs:126-132` returns true whatever the host does; that is why the
gate detects refusal by the 500 ms rule. Whether FL 2026 honours a
plugin-initiated VST3 resize is not verified (hypothesis); Unit 0 builds a
probe plugin that answers it before the window work lands.

### 2.9 Toast

Messages about presets and sections go into one toast: a glass pill
(`pop_fill`, radius 10, 0.7 pt edge white 0.20, text 13 VALUE, padding 8 x 6)
at 12 pt from the window's left and bottom edges, over the panel area,
`Order::Foreground`. Info messages ("Loaded Pong", "Saved Live set", "Closed
Hands", "Reloaded Pong", "Deleted X") fade out after 4 s; errors stay until
clicked. A new message replaces the old one. The toast never takes a click
except to dismiss an error.

### 2.10 Keyboard

While the chat field has focus: Cmd+Enter keeps the pending routes,
Cmd+Backspace discards them (the field's own Enter and Backspace keep their
meaning). Settings lists both next to the existing Tab note. There is no
window-wide Cmd+Z: FL uses it for its own undo.

### 2.11 v3 sizing (2026-11-04, after Noa's FL test)

Noa's verdict on v2 in FL: the window is far too big, the top section sits
too low when the window opens, and the plugin picker is cut off in a real
project. v3 answers with these numbers (editor.rs, glass.rs, sentence.rs).

Window
- One fixed size, 720 x 520 pt (`WIN_W`, `WIN_H`; v2 1200 x 820, 40 % less
  wide, 37 % less high). The window never asks the host for a size. The
  size is not saved with the project, so a project saved with v2 opens at
  720 x 520 too. The layout reads the real size every frame (egui's rect,
  cut to the host's NSView) and fits whatever FL gives it.
- Top grid at 720: pickers `clamp(round(W * 0.19), 136, 220)` = 137 wide,
  the chat field 372 (`C - 2P - 2 GAP - 34`), "+" 30, all inside the 12 pt
  insets: plugin 12..149, chat 157..529, preset 537..674, "+" 678..708.

Camera and panel area
- The panel area (top section, its rule, the open sections, Advanced, the
  bottom room) takes a share of the real height; the camera gets the rest.

| State | Panel share | At 720 x 520: camera / panel | v2 at 1200 x 820 |
|---|---|---|---|
| no section open | 30 % | 364 / 156 | 675 / 145 (18 %) |
| any section open (Hands, Pong, Head, several) | 72 % | 145 / 375 | 328 / 492 (60 %) |

- 72 % is v2's 60 % plus 20 %; with no section the top section sits at
  70 % of the height instead of 82 %. The panel never gets less than 146
  (top 84, rule 2, Advanced 36, bottom room 24); the camera never less
  than 120 while the window has room for both, and never more than the
  full 16:9 frame. The camera animates between the two (0.18 s) when the
  first section opens or the last closes; content inside a section never
  moves it.
- Everything under the top section scrolls (sections, part editor, Pong's
  sliders and wall editor, Head, Advanced); the top section stays, so its
  pickers are always there. At 145 pt the camera shows wings (cover crop
  at 70 % of the frame's height); Pong keeps the whole frame (contain).
- The Hands canvases move together in a narrow window (centres at
  `clamp(w/2 - 120 - 120, 122, 150)` from the middle), so an outer node
  keeps room for "Hi-hat basic".

Popups
- Placement (`glass::place`, pure): left-aligned to the anchor, shifted
  left at the right edge, never wider than the window, 4 pt from every
  window edge, 2 pt from the anchor. `Up` opens above unless less than two
  rows fit there and below has more room; `Auto` opens below when the
  content fits there or below has more room. What does not fit scrolls.
- Up: the plugin picker, the preset picker and "+" (over the camera).
  Auto: every dropdown (key, scale, channel, start, roles, Pong targets),
  the template picker, the older shapes, the lane menus.
- Rows are 22 high (`POP_ROW_H`, v2 26), popup padding 8. At 720 x 520 with
  a section open the room above the plugin picker is 151 pt: padding 16 plus
  six rows of 22.
- Plugin picker: "Follow FL's focus" first, then one group per mixer track
  (Master first; its sounds above its FX in slot order), each headed by
  its track label. Columns 184 wide, 8 apart, at most 6 rows (a header and
  five plugins), fewer when the room above is smaller. A track goes into
  the current column when it fits, else into a new one; a track longer
  than a column fills the current one and carries on in the next under a
  repeated header (`glass::columns`, pure). A 1 pt vertical line sits in
  every gap between columns, 3 pt short of the column's top and bottom:
  white 0.20 (`EDGE`) where a new track starts, white 0.10 (`SEP_SOFT`)
  where a track carries on (`glass::col_seps`, pure). Wider than the
  window, the columns scroll sideways inside the popup only (its own
  scroll area; the page behind stays): the mouse wheel and trackpad, and
  Left/Right while the popup is open, one column (192 pt) per press,
  stopping at either end (`glass::col_step`, pure; the keys reach nothing
  else then). A chevron in the padding marks the side that hides
  columns. Row: plugin name left, "Sound" or "FX n" right, the full label
  on hover.
- Frost (`glass::frosted`, every popup, the suggestion list, the variant
  panel and the routes overlay): no fill of its own; behind it a strong
  tint rgba(18,18,20,0.84) over the whole popup; where it lies on the
  camera picture, the camera block-averaged to 40 x 23 (32 px blocks,
  every 4th pixel, linear filtering) at the same place, under a dark tint
  rgba(18,18,20,0.65) (VALUE text keeps 4.5:1 over a blurred white wall);
  the 0.7 pt edge on top. The blur is made only while a popup is open and
  the frame changed. The camera's status line hides while a popup is open,
  so it shows through no glass; the top bar (Live, key, tempo, Settings)
  hides while a popup covered it the pass before (`glass::popup_cover`).
- Settings (v3 follow-up): 62 % of the window wide, 320 to 460 (446 at
  720), at most 78 % of the window high (405 at 520), then it scrolls.
  Padding 12, rows 4 apart, title 15, hints 11, the language label and
  its switch on one row, Signals and Diagnostics as 24 pt disclosures
  with 13 pt titles. A signal slider leaves its quarter of the row 52 pt
  for the value; the log is 120 high.

Captures: `SYNTHEXIS_UI_SHOT` p (Hands, a ten-track project and the plugin
picker open), q (the same, no section), r (preset picker open), z and y
(every section open, a finger or a wall picked, Advanced open), t (z with
the finger part's template picker open), s (Settings with Signals and
Diagnostics open); `SYNTHEXIS_UI_SCROLL` = points or "end",
`SYNTHEXIS_UI_SCROLL_X` = the plugin picker's sideways offset (past the
end shows the last columns). p, q, r and d show a stand-in camera
picture so the frost's blur shows.

### 2.12 Profiles (2026-11-04)

Noa: "1, 2, 3 in the top bar, left of Settings; each profile stores
everything that is going on right now, so I can switch between them."

- Control: one segmented control `1 2 3` directly left of the Settings
  gear, 24 high, 13 pt numbers, 26 pt segments (`glass::segmented_narrow`,
  the `segmented` look: field edge r6, the active one in the selection
  fill at STRONG, the others LABEL, hover veil). It stays at 0.55 after
  the bar fades, like Live and the gear. Hover texts: the active one
  "Profile 2: what is set up now"; another "Profile 1. Click to switch;
  what is set up now stays in profile 2."; a never visited one "Profile
  3: empty. Click to set it up fresh; ...". No keyboard shortcut.
- A profile is a preset snapshot (kept lines, MIDI parts, binds and key,
  the Pong setup, the picked plugin) plus the view (open and folded
  sections, the picked preset). `preset/profile.rs`.
- Switching from A to B: the chat field's pending lines go, A's state
  and view are captured into A, B's are applied through `preset::apply`
  (A's notes stop and Pong's ramps go to rest, as on a preset load).
  Undo, the picked preset file and the edited marker belong to A and go.
  Toast "Profile 2", or "Profile 2: empty, set it up fresh".
- A never visited profile starts empty: no kept lines, no parts or binds,
  Pong off, no section open. The picked plugin and the MIDI key stay.
- Saved with the project in params `profiles`
  (`{"version":1,"active":1,"slots":[{..},null,null]}`); the active
  profile's slot stays null because the live state holds it. Older
  projects have no `profiles` and open on profile 1 with their setup
  unchanged. FL's undo does not see a switch.
- Capture: `SYNTHEXIS_UI_SHOT=profiles` (profile 1 holds Hands and Head,
  profile 2 is active with the built-in Pong).

## 3. Camera preview (R1)

- `engine/camera.rs`: `PREVIEW_W/H` (39-40) go. `preview_bgra` (189-198) becomes
  `preview_rgba(base, stride, w, h) -> Preview` at the source size (1280x720
  from `AVCaptureSessionPreset1280x720`, 410-411): mirrored, BGRA to RGBA,
  alpha 255, no scaling, one allocation per frame as today.
  `pub struct Preview { pub w: usize, pub h: usize, pub rgba: Vec<u8> }`, no
  egui type in the engine. A safe inner
  `fn mirror_to_rgba(src: &[u8], stride: usize, w: usize, h: usize) -> Preview`
  does the work and is what the test calls. `EngineShared.preview`
  (camera.rs:54 struct; 61, 80, 452) becomes `Mutex<Option<Arc<Preview>>>`. It
  is still built only while `OPEN_EDITORS > 0` (258-261). Perception, motion,
  brightness and the capture preset are untouched.
- `editor.rs`: `PREVIEW` (38) goes; the size comes from the frame. The editor
  keeps the last `Arc<Preview>` and converts and uploads only when
  `!Arc::ptr_eq(&frame, &last)`: one copy into a `ColorImage`
  (`ColorImage::from_rgba_unmultiplied`), then `tex.set` on the first upload
  or a size change and `tex.set_partial([0, 0], ...)` after. When the build
  closure clears the textures on a reopen (editor.rs:981-987), it clears
  `last` too, so the new texture uploads the current frame at once even while
  the camera has stalled.
- Sharpness limit: the source has 1280 px; a 1200 pt window on Retina has
  2400 physical px, a 1.9x upscale (today's 320 px preview: 7.5x). Capturing
  1920x1080 would change what perception receives, which belongs to the other
  session, so the 1280 source stays and the limit is accepted.
- Cost: the full-size swizzle and mirror measured 0.43 ms per frame and nearest
  320x180 0.06 ms in the v1 study bench (M1, rustc -O, scratchpad, not in the
  repo); the extra copy into `ColorImage` 0.2 to 0.4 ms, 30 uploads of 3.7 MB
  per second at 0.5 to 1.5 ms each (hypothesis). Unit 7 measures all three in
  the standalone build and writes the numbers into its commit.
- `ASPECT` stays 16:9; `crop` and `contain` stay; the wings show the dither
  field instead of the frost.

## 4. Dither background (R2)

New file `backdrop.rs`. It replaces `frosted()`, `FROST`, `UiState.frost`, the
frost uploads (1006-1008), the background (1011-1015), the frost swap while
Settings is open (1016-1019), the wings in `paint_cam` (671-675) and the top
bar's frosted copy (1083-1086). The ideas come from Noa's portfolio: the field
maths from `noa-portfolio/src/scripts/c/dome-gradient.ts`, `oklab.ts`,
`dome-pixel-text.ts`, `DomeLoader.astro`; the colours from `dawn-scape.ts` and
`sky-palette.ts` (4.2). This is a reimplementation from the formulas below.

### 4.1 Grid, timing, texture

- Cell `CELL = 3.0` pt. `cols = ceil(W / 3)`, `rows = ceil(H / 3)` for the whole
  window. Every cell is computed, also under the camera image (the camera
  paints over it). This keeps the texture valid when the camera height
  animates or a wing appears.
- Coordinates per cell centre: `px, py` in window points; `S = 0.45 * W` (540
  at W 1200), fixed by the width only; `q = (px / S, py / S)`.
- Time `t` = seconds since the `Backdrop` was created (it lives in `UiState`, so
  reopening the window does not restart the motion).
- Refresh: the grid is split into 8 horizontal bands; each editor frame renders
  one band at the current `t` and uploads it with `set_partial` (at 60 fps a
  full refresh every 0.13 s; the fastest motion is 5 pt/s, so a band is at
  most 0.7 pt behind its neighbour). A change of `W` or `H` re-renders all bands
  and calls `tex.set`.
- Texture: `ColorImage` of `cols x rows`, opaque `Color32::from_rgb`,
  `TextureOptions::NEAREST`, painted first in the frame over the rect
  `(0, 0) .. (cols * 3, rows * 3)`, clipped to the window.

### 4.2 Palette

Noa named the portfolio's gradients as the reference. `dawn-scape.ts` walks a
day in four phases (dawn, noon, sunset, night) and blends neighbouring phases
with a smoothstep (`phaseColor`, dawn-scape.ts:97-101). Its zenith stops
(`skyDeckZenith`, sky-deck-glsl.ts:50-57) are a dawn navy (0.078, 0.141,
0.239), a noon blue, a sunset aubergine (0.141, 0.114, 0.239) and a night
(0.008, 0.031, 0.090); the warm horizon light is `uOrange` `#F07828` mixed
toward `uCream` `#EEE3C8`; the dark base is `sky-palette.ts` `uBase` `#050b1a`.
The background takes that walk, dark navy through aubergine into a warm ember,
and runs it slowly as a hue path. It leaves out the two hue bands the hands
own (36..86 deg orange, `HAND[0]` #f0a35e; 197..247 deg light blue, `HAND[1]`
#62c6e6), so the background never looks like a hand and the hand nodes keep
their meaning.

All colour maths in OKLab (Ottosson's matrices). Base `#0a0a0c` (L 0.145).
Each field has a ramp of four stops at its current hue, interpolated linearly
in OKLab into a 256-entry LUT per field, rebuilt once per refresh cycle (the
hue moves). Chroma at each stop is folded into the sRGB gamut by a binary
search on chroma (30 steps) at fixed L and hue. Anchor chroma C = 0.11 for all
fields.

| h | L | chroma |
|---|---|---|
| 0.00 | 0.145 | base, neutral |
| 0.35 | 0.19 | 0.50 x C |
| 0.70 | 0.245 | 0.75 x C |
| 1.00 | 0.30 | 0.90 x C |

Hue path: `hue(u) = 258 + 120 u` deg for `u` in 0..1, taken modulo 360 (258
navy, 285 iris, 312 aubergine, 340 plum, 18 ember). Reference colours on it
(scratchpad `pal.py`):

| hue | h 0.35 | h 0.70 | h 1.00 |
|---|---|---|---|
| 258 navy | `#04132c` | `#031f47` | `#072c5e` |
| 285 iris | `#120f2b` | `#1e1845` | `#2b235c` |
| 312 aubergine | `#1c0b25` | `#2d113c` | `#3e1b51` |
| 340 plum | `#23081b` | `#380c2c` | `#4c143d` |
| 18 ember | `#28070a` | `#3f0911` | `#55101a` |

The peak L is 0.30 (v1: 0.34), so text stays readable without a veil.
Contrast of white over the brightest cell of any hue, blended in gamma space as
egui does: alpha 0.52 (HINT) at least 4.6:1, alpha 0.56 (ROW, LABEL) at least
5.2:1, alpha 0.80 (VALUE) at least 9.2:1.

### 4.3 Fields and motion

Per field i, with `T = 2 pi t`, `He` the window height eased toward the actual
height with a 4 s time constant (`He += (H - He) * (1 - exp(-dt / 4))`):

```
c_i   = (home_x_i * W, home_y_i * He) / S + amp_i * (sin(T/Px_i + phx_i), sin(T/Py_i + phy_i))
r_i   = r0_i * (1 + 0.12 * sin(T/Pr_i))
g_i   = g0_i * (1 + 0.08 * sin(T/Pg_i))
u_i   = 0.5 - 0.5 * cos(T/Ph_i + phh_i)          // position on the hue path
hue_i = 258 + 120 * u_i  (mod 360)
```

| Field | home (x/W, y/H) | amp (S units) | Px / Py (s) | r0 | Pr (s) | g0 | Pg (s) | Ph (s) | phh |
|---|---|---|---|---|---|---|---|---|---|
| A | (0.18, 0.30) | (0.22, 0.10) | 97 / 131 | 0.50 | 29 | 0.85 | 23 | 173 | 0.0 |
| B | (0.82, 0.62) | (0.22, 0.10) | 113 / 89 | 0.50 | 37 | 0.85 | 31 | 229 | 2.1 |
| C | (0.50, 1.02) | (0.30, 0.06) | 157 / 211 | 0.60 | 43 | 0.70 | 41 | 283 | 4.2 |

Phases `phx, phy` are fixed per field: (0.0, 1.3), (2.1, 0.4), (4.2, 2.9).
Field A sits high, in the camera band, so it lights the camera's wings when
the camera is short (Hands, Pong). Homes are window fractions, and `S` depends
on the width only, so opening a section neither zooms nor jumps the fields;
when the window height changes, the homes glide there over about 4 s.

Speeds: the fastest centre moves `2 pi * 0.22 / 89 = 0.016` S per second, about
8 pt/s at S = 540 (v1 said 5 pt/s at S 350), one cell in 0.4 s. Each field
sweeps the whole 120 deg hue path in half its period (86 to 142 s); its hue
moves at most `120 * pi / 173 = 2.2` deg/s. Nothing pulses faster than a 23 s
period.

### 4.4 Flow

The portfolio's domain-warped fbm (`flowAt`, `fbm`, `gnoise`, `pcg2d` in
`dome-gradient.ts:185-231`), rewritten in Rust:

- `pcg2d(v: [u32; 2])`: `v = v * 1664525 + 1013904223`; `v.x += v.y * 1664525`;
  `v.y += v.x * 1664525`; `v ^= v >> 16`; repeat the two adds; `v ^= v >> 16`.
  All with `wrapping_mul`/`wrapping_add`; integer cell coordinates go in as
  `i as u32`.
- `grad(c) = pcg2d(c) * (2 / 4294967295) - 1`; `gnoise` is gradient noise with
  the quintic fade `f^3 (f (6f - 15) + 10)`, times 1.414.
- `fbm(p)`: 4 octaves, amplitude 0.5 halving, `p = R p * 2 + 17.3` with
  `R p = (0.8 px - 0.6 py, 0.6 px + 0.8 py)`, divided by 0.9375.
- `flow(q, t)`: angle 0, so `s = q`; `s.x /= 1 + 7 * 0.5` (streak 0.5);
  `p = s * 1.3` (scale); `tf = t * 0.03` (speed; the site uses 0.045);
  `a = (fbm(p + tf (0.7, 0.2)), fbm(p + (5.2, 1.3) - tf (0.3, 0.6)))`;
  `b = (fbm(p + 0.6 a + (1.7, 9.2) + tf (-0.5, 0.4)), fbm(p + 0.6 a + (8.3, 2.8) + tf (0.2, -0.7)))`;
  `flow = clamp(fbm(p + 0.6 b) * 1.8, -1, 1)`, `wv = b`.
- It is evaluated on a coarse grid with one node every 8 cells, read
  bilinearly per cell. A band refresh computes only the coarse rows the band
  touches plus one on each side (about 240 nodes per band at 1200 x 820, not
  the whole grid).

### 4.5 Per cell

```
qs   = q + 0.10 * wv
w_i  = g_i * exp(-|qs - c_i|^2 / r_i^2)
u    = 1 - prod(1 - w_i)
h    = clamp(-0.05 + u + 0.18 * flow * smoothstep(0.03, 0.3, u), 0, 1)

B(x, y) = (BAYER8[(y & 7) * 8 + (x & 7)] + 0.5) / 64      // standard 8x8 Bayer, 0..63
thr  = B(x, y) + (hash(x, y) - 0.5) * 0.06

e    = min(h / 0.42, 1)
on   = clamp((e - thr) / 0.14, 0, 1)
if e < 1:
    (klo, khi) = (0.60, 0.25) inside the text area, (0.20, 0.55) elsewhere
    lo = klo * h
    hi = lerp(h, 0.42, khi)
    h  = lo + (hi - lo) * on

p_i  = w_i^2 / sum(w^2)          (equal weights when sum(w^2) < 1e-6)
k    = first i with cumsum(p) > B(x + 4, y + 2)
lab  = lerp(LUT_k[h], sum_i p_i * LUT_i[h], 0.35)
rgb  = srgb_u8(clamp(oklab_to_linear(lab), 0, 1))     // 4096-entry linear -> sRGB LUT
```

The text area is the rect from the bottom of the camera to the top of the
bottom room, inset by `INSET`; the window passes it in each frame. There the
falloff dither is gentle (neighbouring cells differ by at most about 0.06 L at
h 0.3), so 13 pt light glyphs never sit on a hard moving checker. In the camera
wings and the bottom room the full-strength dither shows, as v1 had it
everywhere.

What this gives: below h 0.42 (the falloff into the base) cells switch between
a dim and a lifted level in the ordered-dither pattern; above it the field is
smooth. It is softer than the portfolio's hard edges: unlit cells keep part of
h instead of black, lit cells lift only part of the way, and `on` ramps over
0.14 instead of switching. Where two fields meet, cells pick one field's colour
by a second Bayer phase, so the hues alternate in a dither instead of mixing.
`hash` is the integer hash already in fingers.rs:144-150 (copied into
backdrop.rs). No grain, nothing that crawls.

### 4.6 Cost

Hypothesis from the study: 40 to 60 ns per cell in release; at 1200 x 820
(400 x 274 = 110k cells, 8 bands) about 0.55 to 0.8 ms per band including its
flow rows. Gate: the release bench in Unit 6 must show one band at most 0.8 ms
and a full render at most 6 ms at 1200 x 900 on the M1 Pro, else CELL goes to
4. Editor thread only, never the audio thread.

## 5. Each requirement

### R1. Sharp camera preview

Section 3. The preview is the camera's 1280x720 frame, mirrored, uploaded only
when a new frame arrived, filtered linearly by the GPU. The engine's analysis
input is unchanged.

### R2. Dither background

Section 4. The frosted copy is gone everywhere: under the panels, in the wings,
in the top bar and while Settings is open. The fields drift and cycle through
the portfolio's dusk hues over minutes.

### R3. Edge-to-edge sections, separators, bottom room

Section 2.4. `glass::panel` and `glass::section_header`'s rounded cards are not
used by the window any more. Feature sections have no fold chevron (the preset
picker decides what shows); Advanced keeps its fold. Bottom room is 24 pt.

### R4. Top section

- Plugin picker (left column, `P` x 30): label is the pinned plugin's name
  (VALUE); while following FL with a focused plugin, that plugin's name (ROW
  alpha, hover "Following FL's focus"); otherwise "Plugin picker" (HINT). The
  list's top row reads "Follow FL's focus"; picking a plugin pins it, as today.
  The Pin and Unpin buttons go (picking is pinning).
- Chat field (centre column, 30 high): `TextEdit::singleline` with
  `.min_size(vec2(w, 30))`, `.margin(Margin::symmetric(10, 7))`,
  `.vertical_align(Center)`, font 13 in the field and in the chip layouter. The
  text cursor is 1 pt wide in `ACCENT_HI` (set in `glass::style` through
  `visuals.text_cursor`), blinking 0.5 s on, 0.5 s off; it spans the 13 pt line,
  shorter than today's 2 pt stroke on a 14 pt line.
- Preset picker (right column, same `P` x 30, same style): lists Hands, Pong and
  the saved presets (R5). Placeholder "Preset picker". When the picked preset
  is a saved one and the state differs from what was loaded (checked once per
  second), the label reads "Live set, edited" at ROW alpha.
- Row 2 under the preset picker, a fixed layout in every state:

  | State | left | right |
  |---|---|---|
  | idle | Save as... | (empty) |
  | an Undo exists (R5.4) | Save as... | Undo (hover: "Undo: back to before loading Pong") |
  | saving | name field | Save or Overwrite |

  Save as... is always visible outside saving. A click turns the row into
  `[name field][button]`: `glass::text_field` 24 high, width
  `P - button width - GAP` (the button measured from its text); the field
  starts with the picked saved preset's name, or empty; hint "Preset name";
  the button reads "Overwrite" when the name is a saved preset, else "Save".
  Enter saves, Esc or a click elsewhere cancels; a built-in name is refused
  with the existing message (toast, error). After saving, the picker shows the
  new name and a toast says "Saved X". A test measures row 2 in English and
  German in all three states at P 170 and P 204. German `undo` is "Zurück" so
  "Speichern als..." and "Zurück" fit into 170 together.

### R5. The preset picker decides the sections; "+"

The model, implemented in `preset.rs` and `preset/top.rs`:

1. Sections: `enum Section { Hands, Pong }`. The open list (ordered) and the
   picked preset name live in a new persisted param `view` (JSON
   `{"version":1,"sections":["hands"],"preset":"Hands"}`), so FL keeps them with
   the project.
2. The picker lists the built-ins first, place text "Built-in": "Hands", "Pong";
   then a header row "Saved presets" and the user's presets by name, each with
   the two-click Delete of 2.7.
3. Picking:

   | Pick | What happens |
   |---|---|
   | a saved preset | the field's pending lines are discarded, `preset::apply(writer, &snapshot)` replaces the whole state as Load does today (pong_ui.rs:237), the open list becomes `preset::opens(&snapshot)`; toast "Loaded X" |
   | built-in X, not open | X becomes the only open section. Other open sections close as in point 6; if one of them has content, the picked row first turns into "Pong goes. Click again" and the pick needs a second click. Hands brings no content of its own. Pong merges the built-in's Pong part (`preset::with_section`) when the Pong part is empty, else keeps the current one; toast "Opened Hands" or "Loaded Pong" |
   | built-in X, open with others | the others close as above; X keeps its content |
   | the built-in shown as the only open section | Pong: its Pong part is replaced by the built-in's (walls, setup), toast "Reloaded Pong". Hands: nothing to reload, no change |
   | the saved preset already shown | loads it again (back to the saved version), toast "Reloaded X" |

   Every pick that changes state keeps the state and open list from before for
   Undo. A built-in pick keeps the current plugin pin and the key
   (`midi.key`); a saved preset restores its saved pin and key, as today.
   Kept lines whose source is neither a hand nor Pong (face routes) never
   change on a built-in pick.
4. Undo (row 2, right): restores the snapshot and the open list from before
   the last pick, "+" or close, through today's `undo_load` path. It stays
   until the next pick, "+", close or save, or until the state has changed by
   the user for at least 10 s: once per second the current `capture()` is
   compared with the one taken right after the action; when they differ, a 10 s
   timer starts, and when it runs out, Undo goes. `Snapshot` holds only
   configuration (parts, binds, kept lines, Pong setup, key, pin), so Pong
   play, playing parts and camera values never count as a change. A test plays
   Pong for 2 s after a load and checks that Undo is still there.
5. "+" (30 x 30, right of the preset picker): a popup with the rows "Hands" and
   "Pong"; a section already open is greyed out (OFF 0.25, hover "Already
   open"). Picking adds the section after the open ones:
   - Hands: opens, nothing else changes.
   - Pong: when the Pong part of the state is empty (off, all walls Off), the
     built-in's Pong part merges in through `preset::with_section` (its two wall
     parts get fresh ids in the current store; finger parts, binds and kept
     lines stay) and `apply` runs, with Undo; when Pong already has content, it
     only opens.
   - "+" draws at HINT alpha while no section is open and at VALUE alpha with
     the selection edge once one is. It stays clickable in both states, so a
     fast user can add Pong without picking a preset first.
6. Closing: the `x` at a feature section's header right end. The first click
   arms it: the header's right group shows "Close Hands? 3 parts go" (or
   "Close Pong? Walls and Pong settings go") with the `x` lit; the arm lasts
   4 s or until the pointer leaves the header. The second click closes. Closing
   removes what the section plays (`preset::without_section`: Hands drops the
   finger and shape binds, their parts unless a wall also plays them
   (fingers.rs:1823-1830 has that check), and the kept lines whose source is a
   hand; Pong goes back to `Setup::default()` and its wall parts go), runs
   `apply`, offers Undo, and toasts "Closed Hands". A hidden section never
   keeps playing.
7. Content always shows: the open list is never smaller than what the state
   plays. `preset::opens(s)` returns `s.sections` followed by every section of
   `sections_of(s)` not in it. The window applies the same union whenever the
   writer's `state_gen` changes, and appends Hands when a reply makes a hand
   route or a finger part (R7), and appends Pong when a wall is picked from
   the All view (`take_wall_pick`).
8. Saving writes `sections = view.sections` into the snapshot.
9. Old projects have an empty `view`. The window then opens
   `preset::sections_of(&capture(writer))`: Hands when a bind's signal starts
   with `cam0.hand.` or a kept line's route source starts with `hand.`; Pong
   when `setup.on` or a wall is not Off. Old preset files without `sections`
   open by the same rule, so nothing visible is lost.
10. Who writes `view`: every action that changes it (`Presets` after `apply`,
    the window's R7 and wall-pick appends, close) writes it to the param in the
    same call. The window re-reads `view` when the writer's `state_gen`
    changes (a project load, `initialize` at lib.rs:227, or an `apply`), after
    any own write of that frame, then applies the union of point 7.

A user preset whose name folds to "Hands" (made before this change) keeps its
file. Today `files()` (preset.rs:333-336) drops it, `load_in` (351-355) returns
the built-in first and `delete_in` (376-378) refuses the name. v2 changes the
listing: `list()` returns `(name, file, builtin)` entries, `files()` keeps such
twins and tags them, the picker shows the twin as "Hands (saved)", and
`load_file(path)` and `delete_file(path)` work by file. Saving under a built-in
name stays refused.

### R6. "What fingers play" becomes "Hands"

`f_title` "Hands" / "Hände". The notes that name the section (`n_no_channel`,
`midi_kept`) say "Hands" too.

### R7. A sentence about fingers or hands opens Hands

`Sentence::take_open_hands()` returns true once after a reply resolved with at
least one route whose source starts with `hand.` or which made a finger part
(`assign::midi_routes` with a finger trigger), while no Pong side is the scope.
The window then appends Hands to the open list when Hands is not open, whatever
else is open (Pong open stays open). It does not load anything and does not
set a preset name; with nothing picked the picker keeps its placeholder and
"+" lights up because a section is open. The Undo of row 2 does not cover this
(nothing was removed).

### R8. Signals and the log into Settings; Advanced last

Settings, top to bottom (panel 560 wide, centred, scrolls when taller than the
window minus 48):

1. "Settings" (18) and the close button `x`.
2. Language, its note; Suggestions while typing, its note; the keyboard note
   (Tab, Cmd+Enter, Cmd+Backspace).
3. "Finger calibration" with "Calibrated" or "Not calibrated" (status only;
   Calibrate and Reset sit in the Hands header, R20).
4. The account note ("These settings belong to your user account ...").
5. Rule, then "Saved with the project" (LABEL).
6. "Signals 1-32 (Link to controller)" as a disclosure, closed by default;
   open, 8 rows of 4 `ParamSlider`s, each `(560 - 32 - 3 * 8) / 4` wide.
7. "Diagnostics" as a disclosure, closed by default, no summary; open: the
   three diagnostic lines, "Pong burst x16" (disabled unless Pong and Live are
   on), the log label and the last 40 log lines in a 180 pt `ScrollArea` stuck
   to the bottom.

`settings_overlay` gets `params`, `setter` and `writer`. The overlay's scrim is
`Tok::WINDOW` at alpha 200 over the sharp camera (no frost swap). Advanced
stays the last row of the window with its current content (Test signal, the
two hand role dropdowns, the camera status line as its summary).

### R9. Routing suggestions as a collapsible overlay

- The scope chip and its `x` leave the picker row (sentence.rs:1873-1891). The
  plugin note and the routes leave the field's frame.
- Routes strip (row 2 of the chat column): a chevron, then a summary built
  from what exists, in this order and joined by " . ": the scope ("Bottom
  wall", with its `x` that drops the scope), then the first of "Suggested
  routes: N" (pending lines), the assistant's status ("Reading the sentence"),
  "Kept routes: N". With pending lines, Keep and Discard follow the summary in
  the strip, so they work while the overlay is folded. Nothing to show: the
  strip is empty.
- Overlay: `egui::Area` at `Order::Foreground`, from x = `INSET` to
  `W - INSET`, top 4 pt under the top section, at most 35 % of the panel area
  high, scrolling inside. Glass popup look: `pop_fill`, radius 10, 0.7 pt edge
  white 0.20, padding 12. Rows (13): the reply lines, the pending lines with the
  flip button, the wall edits, a "Kept" header (LABEL) and the kept lines with
  `x`. It covers the sections under it; a click on it never falls through
  (interactive Area hit-test).
- Folding: the chevron toggles it. It opens by itself when a new reply or a new
  pending batch arrives, unless the window says the Hands Routes view shows
  that batch (`quiet`, R19); then it stays folded and the strip carries Keep
  and Discard. It folds on Keep, Discard, Esc on an empty field, or a click
  outside it. The outside test leaves out the strip's rect, so the chevron's
  own click does not fold it again. A click outside that folds the overlay is
  consumed and does not reach the widget beneath. It is not drawn while
  Settings is open or while the completion list is open. The fold state lives
  in `Sentence` (not persisted); `Sentence::unfold_routes()` opens it from
  outside (R19).

### R10. Fonts, capitalisation, sibling sizes

Sections 2.2 and 2.3. Load/Save in Pong: that row goes (R23); the presets'
Save as row and the part name row use `glass::quiet` and `glass::text_field`,
both 13 and 24 high. Key in the Hands header shows the root before the mode:
`Key [A v] [Minor v]`.

### R11. One layout logic; fewer buttons; rare things in Settings

The grid and rules of section 2. Removed from the window: Pin/Unpin, "follows
FL", the scope chip in the picker row, Pong's preset row (to the top picker),
Point/Flick and its hint, Burst x16 (to Settings, Diagnostics), the shape
popover with 12 shapes, the five template chips and Empty and the part Load
button (into one part picker), the L and R letters (replaced by the hand names
over the shape chips), the Signals section (to Settings), the fold chevrons of
feature sections, the frost. A test (Unit 7) fails when `ui.button(` or
`small_button(` appears in editor.rs, preset/top.rs, sentence.rs, fingers.rs or
pong_ui.rs outside `#[cfg(test)]`.

### R12. The window sizes itself

Section 2.8. If FL refuses plugin-initiated resizes (Unit 0 answers this),
R12 is met as far as the host allows: the window keeps the size FL gives it,
the camera takes what the open sections leave (at least `cam_min`), the panel
area scrolls, and no area stays blank.

### R13. Rotated hands, label nodes with lines, no overlaps

- Art: `build_art(angle)` rasterises into a `CANVAS` = 160 x 160 dot canvas
  (was 120 x 104), `DOT` 1.5, so 240 x 240 pt per hand. Hand space keeps its
  own constants `HAND_W` 120 and `HAND_H` 104 (today's `ART_W`/`ART_H`), which
  the palm and finger SDFs and the wrist dissolve (fingers.rs:197-202) keep
  using; only the raster loop and the texture use `CANVAS`. For each canvas
  dot `p`, the hand-space point is `R(-45 deg) (p - (80, 92)) + (61, 62)` (the
  palm centre sits at canvas (80, 92), a little low, because the fingers reach
  further up than the wrist down); the SDFs run there as today. The right hand
  turns clockwise on screen (fingers to the upper right, thumb straight up on
  the inner side); the left hand is its mirror (`dot_at` mirroring
  unchanged). Bayer dots stay screen-aligned and sharp because the rotation
  happens before rasterising. `Art.tips` go through the forward map
  `canvas = R(+45 deg) (hand - (61, 62)) + (80, 92)`. With today's tips the
  right hand's tips land at about thumb (68, 47), index (100, 46), middle
  (116, 53), ring (122, 69), pinky (122, 87) in canvas dots.
- Placement: canvas centres at `cx - 150` and `cx + 150` (`cx` = content
  centre), so the two canvases span `cx - 270 .. cx - 30` and
  `cx + 30 .. cx + 270`. Under each canvas, centred, a row: the hand name (13,
  hand colour at 0.8) and the shape chips (R16). Hands view height:
  240 + 8 + 24 = 272.
- Label nodes: one per bound finger, plus one for the selected finger when it
  has no part (text "Empty", HINT). The node is a chip 22 high, radius 6, text
  13, fill black 0.60, 1 px edge in the hand colour; selected: fill hand colour
  at 0.25, text STRONG and a 2 pt white dot at the finger's tip. Text: the
  part's `summary()` cut to 18 characters with "..." (today it cuts at 10
  without a mark, "i-hat bas").
- Where nodes go: the thumb's node sits above its tip, centred on the tip's x,
  its top 8 pt under the canvas top (the art has nothing above the tips there).
  The other four go to the outer side: right-hand nodes left-align at
  `canvas.right + 16`, left-hand nodes right-align at `canvas.left - 16`; a
  node never passes the content inset (narrow windows cut the text). They are
  ordered by the tip's angle around the palm centre (index on top, pinky at
  the bottom), not by the tip's y.
- Lines: 1 px in the hand colour at 0.55, painted before the art so the dots
  cover them. Thumb: straight up from the tip to the node's bottom centre.
  Outer fingers, three segments: a 12 pt leg from the tip along the finger's
  axis, then horizontal to the gutter at `canvas.right + 8` (`left - 8`), then
  straight to the node's near edge at its vertical centre. The last segment
  lies in the gutter outside the art. A 2 pt dot marks the tip.
- No overlaps: `fn spread(ys: &mut [f32], gap: f32, lo: f32, hi: f32)` (pure)
  places the four outer nodes in their angle order: a forward pass pushes each
  down to at least `gap = 26` under the previous; if the last passes `hi`, all
  shift up by the excess and a backward pass enforces the gap upward; finally
  clamp to `lo`. Four nodes need 4 * 26 = 104 pt of the 240 pt column, so it
  always fits.
- The nodes stay clickable (select the finger), as the chips are today.

### R14. A click highlights the finger at once

The colour rule in `Fingers::hands` (1966-1999) becomes a pure function
`finger_colour(side, f, sel, bound, playing, seen, target) -> (lit, unlit)`:

1. Calibration target: as today.
2. Playing: white, as today.
3. Selected: the hand colour at full for lit dots, and the hand colour at 0.45
   for the region's unlit dots, so the whole finger reads as a filled shape,
   plus the white tip dot of R13. The camera's mask never dims a selected
   finger.
4. Bound, not selected: hand colour; the camera mask dims it as today (seen
   1.0, folded 0.28, no camera 0.75).
5. Neither: white 0.92 with the same camera dimming.

The hands view allocates its rect and reads the click before it builds the
texture (today the texture is built at fingers.rs:2005-2016 and the click is
read at 2044), so the selection shows in the frame of the click.

### R15. More realistic dither hands (fine tuning)

In `build_art`, still the same regions and click targets:

- Each finger chain gets three segments (proximal, middle, distal at 0.49 /
  0.29 / 0.22 of its length) and tapers from radius r at the base to 0.85 r at
  the tip (capsule distance minus the interpolated radius).
- Lengths in hand-space dots, knuckle to tip: index 34, middle 38, ring 35,
  pinky 27; the thumb keeps two segments, 30 long. The ring finger becomes a
  little longer than the index, as on most hands (today ring 32, index 33).
- Joints: an unlit seam 0.8 dots wide across the finger at the two joints
  (where the chain parameter passes 0.49 and 0.78), with the edge dots kept lit,
  like the existing seam where a finger meets the palm.
- Everything else (palm SDF, Bayer shading, dissolving wrist) stays.

### R16. Shapes

The "Shapes" label, its chips row and the "+ shape" popover with 12 shapes go
(fingers.rs:2053-2095). Under each hand: "Fist" and "Open hand" chips
(`glass::chip`, hand colour when bound, selection look when selected), hover
"Plays while the whole hand shows this shape". A click selects
`Slot::shape(side, 0b00000)` or `Slot::shape(side, 0b11111)` and opens the part
editor like a finger.

Shapes other than fist and open that an old project already binds stay
reachable, so nothing bound disappears: after the two chips, one chip
"+N older shapes" opens a small picker list of them. Their names there
describe the fingers, built from the 5-bit mask: the extended fingers joined
("Thumb and pinky" for Shaka, "Thumb and index" for L, "Index, middle and
ring"); the stored keys do not change. They cannot be added any more. The chip
row keeps its 24 pt height, so the hands view stays 272 high. `SHAPES` stays in
fingers.rs because the sentence field and the calibration still use the names.

When nothing is selected, the part editor's place shows one hint: "Click a
finger or a hand shape to give it a part." (13, HINT).

### R17. Part editor rows

Exact order, every row starting at the left inset, right groups at the right
inset, all on the 32 pt pitch (2.6 says why left-aligned):

1. Owner row: `(o) Left hand . index` (13, hand colour; filled dot while
   playing). Right group: `Hide roll` (or `Show roll`), `Remove`
   (`glass::quiet_dim`), Remove right of Hide roll with `GAP` between.
2. Editor row: `[Pattern | Chord | Held note]`, channel dropdown 180,
   then for a finger the start dropdown 124 and `[While shown | On / off | Once]`,
   for a wall the label "A hit plays" and the start dropdown, as today. One
   `horizontal` row; where it does not fit, the gate-mode control moves to its
   own row under it, so no control ends up alone at the start of a line the
   way Remove did.
3. Tools row, by kind. Left group:
   - Pattern: `[1   bars]` (slider_field 88), `[Lanes | Piano roll]`, the part
     picker (168 wide), the reserved Back slot.
   - Chord: the chord tools as today (notes, octave, strum, velocity fields and
     the chord row), then the part picker.
   - Held note: the velocity field (104) and the Back slot.
   Right group (Pattern and Chord): part name field (`glass::text_field` 150,
   shows `part.name`, edits rename the part), then `Save` (`Overwrite` when a
   saved part has that name).
4. The roll (lanes or piano roll), unchanged in size rules. It takes the mouse
   wheel only when its content overflows its own height; otherwise the wheel
   scrolls the panel area.
5. Footer: `[Velocity | Repeats]`, Fill, Add row. "Repeats" replaces "Rolls"
   (the ratchet lane), so "roll" means only the grid ("Hide roll") and the view
   ("Piano roll").
6. Hints, 13 at HINT.

Part picker (picker style, 2.7): the button always reads "Template". Rows: the
built-in templates first with place text "Built-in" (Hi-hat basic, Trap roll,
Snare fill, Kick, Clap, Empty for a pattern: `midi::TEMPLATES` first five plus
Empty, which clears the steps; I-V-vi-IV for a chord), then a header row
"Saved parts" and the user's parts with the two-click Delete; "No saved parts
yet" (HINT) when there are none. Picking a template runs today's template code
(fingers.rs:2283-2302) with the undo slot; picking a saved part replaces play,
mode, start and name with the undo slot (`remember`) and fits drum notes to the
channel (`midi::fit_to_channel`).

Part presets, new file `fingers/part_lib.rs`: one JSON file per name in
`~/Library/Application Support/synthexis/parts/` (`$SYNTHEXIS_PARTS` for
tests), `{"version":1,"name":..,"play":..,"mode":..,"start":..}`, written
through a tmp file and a rename like presets. The slug and atomic write are
reimplemented there (about 30 lines) rather than shared with preset.rs, so the
two units do not touch one file.

### R18. Lanes grid

In `paint_roll` (fingers.rs:1430-1470), lanes mode only (the piano roll keeps
today's values):

| Line | Today | New |
|---|---|---|
| bar lines (every 16 steps) | white 0.20 | white 0.32 |
| beat lines (every 4 steps) | 0.09, 0.06 when `cell_w` < 5 | 0.16 always |
| step lines | 0.04 only when `cell_w` >= 8 | 0.07 when `cell_w` >= 6 |
| lane separators | 0.06 | 0.10 |
| alternate bar shading | 0.025 | 0.035 |
| ruler beat ticks | 3 pt at 0.20 | 4 pt at 0.28, bar ticks full ruler height at 0.32 |

A pure `fn grid_alpha(step: u16, cell_w: f32, lanes: bool) -> Option<f32>`
carries the table and is what the test checks.

### R19. Routing as a node view

- Where: the Hands header's segmented control becomes
  `[Single | All | Routes]` with hovers "One finger's part", "All parts at
  once", "Where the hands go". In Routes the area under the hands view (where
  the part editor sits in Single) shows the node view.
- When: the view switches to Routes by itself on a rising edge only: when
  `Routing.batch` changes to a new non-zero id while `Routing.focus` is true (a
  pending line has a parameter or Signal target from a hand signal, or the
  field holds a bound parameter chip). `Fingers` remembers the view it left.
  When `focus` goes false (Keep, Discard, chip removed), it returns to that
  view, unless the user picked a view by hand in between; a manual pick of
  Routes stays. Clicking a finger switches to Single as today, and the
  auto-switch does not fire again for the same batch. With Hands closed, R7
  appends Hands first.
- While Routes shows a pending batch, the window tells `Sentence` to keep the
  overlay folded for it (`quiet`, R9): the node view is the view of that
  batch, and Keep and Discard stay in the strip.
- Data (no new engine state): `Sentence::routing()` gives one `EdgeIn` per
  (signal, target) of the pending and kept lines whose signal starts with
  `cam0.hand.`, the target label from the line's resolved route (`Bound.name`
  for parameters, "Signal N" for macros) and the plugin from the line's
  `fl::Key`, plus `focus` and `batch`. Fingers adds one `EdgeIn` per MIDI bind
  on a hand signal (finger shown or shape -> the part's name).
- Layout (`fingers/graph.rs` `layout`, pure): three columns. Column 1 at x 0:
  hand nodes "Left hand" / "Right hand" (only the hands used), 112 wide.
  Column 2 at x 136: one source node per (hand, signal), 120 wide, labels from
  `graph::signal_label` ("Spread", "Openness", "Pinch", "Left-right",
  "Up-down", "Distance", "Turn", "Hand speed", "Fist", "Finger count", "Index
  tilt", "Index shown" for a finger bind, "Fist" / "Open hand" for a shape
  bind), left hand first. Column 3 at x `0.55 C`: targets in order of first
  use, grouped by plugin under a group label (13, the plugin's colour, no box)
  that takes a row of its own; Signal targets and parts form groups "Signals"
  and "Parts". Target width `min(240, 0.45 C - 16)`. Row pitch 30, node height
  24, radius 6. A hand node's y is the mean of its sources. At most 16 targets;
  then a "+N more" node. The view's height is `rows * 30 + 16`, group label
  rows included.
- Look: nodes like the finger label nodes (fill black 0.55, 1 px edge in their
  colour, text 13 VALUE). Colours: left hand `HAND[0]`, right hand `HAND[1]`,
  each plugin its own colour from `graph::plugin_colour(name)` (OKLCH L 0.78,
  C 0.10, hue from a hash of the name, moved out of 36..86 and 197..247 deg so
  no plugin looks like a hand), Signals white 0.80, parts in their hand's
  colour.
- Edges: hand to source a straight 1 px line in the hand colour. Source to
  target a cubic Bezier with horizontal tangents (control points at half the
  x distance), drawn as 1.5 pt square dots every 2 pt of arc length, pixel
  aligned like the hand art. Dot i takes the target colour when
  `smoothstep(0.25, 0.75, t_i) > (BAYER4[i % 16] + 0.5) / 16`, else the source
  colour: the colours meet in an ordered dither in the middle half.
- Dither where colours meet at the nodes: where an edge enters a node, an
  8 pt wide band of the node's fill, at the entry side, dithers from the edge's
  colour at 0.35 into the node fill (same BAYER4 rule over the band's x). A
  target fed by both hands splits its fill: the top half dithers in from
  `HAND[0]`, the bottom half from `HAND[1]`.
- Pending edges: 6 pt dash runs of dots with 4 pt gaps, at 0.6 alpha; a pending
  target node is hollow (no fill, edge at 0.6).
- Empty: "No routes from the hands yet. Type one in the field above." (HINT).
- Read-only: removing a route stays in the overlay (R9). A click on a target
  node sets a flag that `Fingers::take_unfold()` returns once; the window then
  calls `Sentence::unfold_routes()`.

### R20. Calibrate fingers stays

"Calibrate fingers" stays in the Hands header's right group (`glass::quiet`),
and "Reset" (`glass::quiet_dim`) stays right of it, shown only while a
calibration exists, so a bad run is undone where it was started. Then 16 pt, a
1 px vertical rule (white 0.10, `CTRL_H` high), and the section's `x`. Its
status line above the hands stays. Settings shows only the status (R8). The
calibration itself is another session's (`calib.rs`).

### R21. No Point/Flick choice

`PongUi::controls` drops the Shot label, the segmented control and the hint
(pong_ui.rs:312-317). `Setup.trigger` stays in the struct and in serde, so old
presets load and save unchanged; the UI never writes it. Both values already
shoot the same way: `pong/setup.rs:94` says Flick "shoots as a point now", so
there is one shot whatever an old preset stores. The keys `p_shot`, `p_point`,
`p_flick`, `p_point_hint`, `p_flick_hint` stay unused (section 6).

### R22. Pong rearranged

Header row (36): "Pong" (title), then the toggle "On"; while Live is off, a
chip "Live is off" (glass::chip, LIVE colour edge) whose click turns Live on.
Right group: the stats "0 flying . 645 hits . 91 dropped" (13, ROW, numbers in
`num_slot`s; HINT while Pong is off), then the rule and the section's `x`.

Body, one centred column `PONG_COL` = 460 wide, then full-width rows:

1. Shooting hand label and `[Left | Right | Both]` at the column's left;
   `Test shot` at its right (disabled at alpha 0.40, not 0.25, with hover "Turn
   Pong and Live on to test").
2. The side view (unchanged, `VIEW_W` 360, 16:9), centred in the column.
3. Compact row (`CTRL_H_SM` 20, labels and values 11): Speed 136, Bounces 99,
   Time limit 106, Max balls 95, with `GAP` 8: exactly 460.
4. Compact toggles: "Harder shot plays louder", "Hard shot flies faster",
   "Mute the shooting hand's parts", 20 high, text 11, centred in the column,
   wrapping into centred lines when they do not fit (German does not).
5. Inset rule, then the side editor (full width, the part editor rules of R17
   for MIDI walls) or the hint "Click a wall to choose what a hit does."

### R23. Pong's preset row moves to the top picker

`presets_row`, `load`, `undo_load`, `save`, `delete` and their fields stop being
drawn by Pong; the same behaviour lives in `preset/top.rs` for the preset
picker (R5), with its own tests. The section keeps only Pong. Pong's old
functions stay reachable from the old `PongUi::ui` wrapper until the cleanup
(section 7).

## 6. i18n

All changes go into `i18n.rs` in English and German. New keys:

| Key | English | German |
|---|---|---|
| `plugin_picker` | Plugin picker | Plugin-Auswahl |
| `picker_following` | Following FL's focus | Folgt dem FL-Fokus |
| `preset_picker` | Preset picker | Preset-Auswahl |
| `preset_hands` | Hands | Hände |
| `preset_pong` | Pong | Pong |
| `preset_builtin` | Built-in | Eingebaut |
| `preset_saved` | Saved presets | Gespeicherte Presets |
| `preset_saved_twin` | {} (saved) | {} (gespeichert) |
| `preset_edited` | {}, edited | {}, geändert |
| `save_as` | Save as... | Speichern als... |
| `save` | Save | Speichern |
| `overwrite` | Overwrite | Überschreiben |
| `undo` | Undo | Zurück |
| `undo_hover` | Undo: back to before {} | Zurück: Stand vor {} |
| `delete` | Delete | Löschen |
| `delete_arm` | Delete? Click again | Löschen? Nochmal klicken |
| `add_section` | Add a section | Bereich hinzufügen |
| `section_open` | Already open | Schon offen |
| `close_section` | Close {}. What it plays is removed; Undo brings it back. | {} schließen. Was er spielt, wird entfernt; Zurück holt es wieder. |
| `close_arm_hands` | Close Hands? {} parts go | Hände schließen? {} Parts werden entfernt |
| `close_arm_pong` | Close Pong? Walls and Pong settings go | Pong schließen? Wände und Pong-Einstellungen werden entfernt |
| `replace_arm` | {} goes. Click again | {} wird entfernt. Nochmal klicken |
| `section_closed` | Closed {} | {} geschlossen |
| `section_opened` | Opened {} | {} geöffnet |
| `reloaded` | Reloaded {} | {} neu geladen |
| `ov_pending` | Suggested routes: {} | Vorgeschlagene Routen: {} |
| `ov_kept` | Kept routes: {} | Behaltene Routen: {} |
| `ov_reading` | Reading the sentence | Satz wird gelesen |
| `ov_show` | Show the routes | Routen zeigen |
| `ov_hide` | Hide the routes | Routen ausblenden |
| `ov_kept_head` | Kept | Behalten |
| `keys_note` | Cmd+Enter keeps the suggested routes, Cmd+Backspace discards them. | Cmd+Enter behält die vorgeschlagenen Routen, Cmd+Rückschritt verwirft sie. |
| `f_routes` | Routes | Routen |
| `f_single_hover` | One finger's part | Der Part eines Fingers |
| `f_all_hover` | All parts at once | Alle Parts auf einmal |
| `f_routes_hover` | Where the hands go | Wohin die Hände gehen |
| `f_routes_empty` | No routes from the hands yet. Type one in the field above. | Noch keine Routen von den Händen. Schreib oben eine. |
| `f_fist` | Fist | Faust |
| `f_open` | Open hand | Offene Hand |
| `f_shape_hover` | Plays while the whole hand shows this shape | Spielt, solange die ganze Hand diese Form zeigt |
| `f_older_shapes` | +{} older shapes | +{} ältere Formen |
| `f_and` | and | und |
| `f_pick_part` | Click a finger or a hand shape to give it a part. | Klick einen Finger oder eine Handform an, um ihm einen Part zu geben. |
| `f_template` | Template | Vorlage |
| `f_saved_parts` | Saved parts | Gespeicherte Parts |
| `f_part_name` | Part name | Part-Name |
| `f_no_parts` | No saved parts yet | Noch keine gespeicherten Parts |
| `f_repeats` | Repeats | Wiederholungen |
| `g_openness` | Openness | Öffnung |
| `g_spread` | Spread | Spreizung |
| `g_x` | Left-right | Links-rechts |
| `g_y` | Up-down | Hoch-runter |
| `g_depth` | Distance | Abstand |
| `g_pinch` | Pinch | Pinch |
| `g_speed` | Hand speed | Handtempo |
| `g_roll` | Turn | Drehung |
| `g_tilt` | {} tilt | {} Neigung |
| `g_count` | Finger count | Fingerzahl |
| `g_shown` | {} shown | {} gestreckt |
| `g_more` | +{} more | +{} weitere |
| `g_signals` | Signals | Signale |
| `g_parts` | Parts | Parts |
| `set_calibration` | Finger calibration | Fingerkalibrierung |
| `set_calibrated` | Calibrated | Kalibriert |
| `set_not_calibrated` | Not calibrated | Nicht kalibriert |
| `set_project` | Saved with the project | Wird mit dem Projekt gespeichert |
| `p_on` | On | An |
| `p_live_off` | Live is off | Live ist aus |
| `p_test_off` | Turn Pong and Live on to test | Pong und Live einschalten zum Testen |

Changed texts (key: English / German):

- `f_title`: Hands / Hände. `n_no_channel`: pick its FL channel under Hands /
  FL-Kanal unter Hände wählen. `midi_kept`: kept: see Hands / behalten: siehe
  Hände. `f_pick`: Click a finger or a hand shape to give it a part. / Klick
  einen Finger oder eine Handform an, um ihm einen Part zu geben.
- `follow_fl`: Follow FL's focus / FL-Fokus folgen. `placeholder`: When I spread
  my fingers, open the filter / Wenn ich die Finger spreize, Filter auf.
- `flip_hover`: Flip the direction / Richtung umdrehen. `remove_hover`: Remove
  this route / Route entfernen. `w_drop_scope`: Stop assigning this wall / Wand
  nicht mehr zuweisen.
- `fl_no_plugins`: No plugins in the project / Keine Plugins im Projekt.
  `fl_click_plugin`: Click a plugin window in FL / Klicke in FL ein
  Plugin-Fenster an. `fl_waiting`: Waiting for FL / Warte auf FL. `loading`:
  Loading suggestions / Lade Vorschläge. `loader_failed`: Suggestions not
  started: {} / Vorschläge nicht gestartet: {}. `no_data`: No suggestion data
  ({}) / Keine Vorschlagsdaten ({}). `reading_classes`: Reading parameter
  classes / Lese Parameterklassen.
- `f_no_channel`: Pick a channel / Kanal wählen. `f_while_held`: While shown /
  Solange gezeigt. `f_toggle`: On / off / An / aus. `f_once`: Once / Einmal.
  `f_start_now`: At once / Sofort. `f_start_16`: Next 1/16 / Nächstes 1/16.
  `f_start_beat`: Next beat / Nächster Beat. `f_start_bar`: Next bar / Nächster
  Takt. `f_size`: Notes / Töne. `f_octave`: Octave / Oktave. `f_strum`: Strum /
  Strum. `f_vel`: Velocity / Anschlag. `f_add_row`: Add row / Zeile hinzufügen.
  `fw_hit_plays`: A hit plays / Treffer spielt.
- `p_speed`: Speed / Tempo. `p_speed_unit` (the value): {} screens/s / {}
  Bildhöhen/s. `p_bounces`: Bounces / Abpraller. `p_time`: Time limit /
  Zeitlimit. `p_max`: Max balls / Bälle max. `p_never`: Never / Nie. `p_vel`:
  Harder shot plays louder / Härterer Schuss spielt lauter. `p_faster`: Hard
  shot flies faster / Harter Schuss fliegt schneller. `p_pause`: Mute the
  shooting hand's parts / Parts der Schusshand stumm. `p_burst`: Pong burst x16
  / Pong-Salve x16 (with the real multiplication sign as today). `p_target`:
  Pick a target / Ziel wählen. `p_rest`: Rest / Ruhe. `p_peak`: Peak / Spitze.
  `p_depth`: Depth from the shot / Tiefe aus dem Schuss. `p_glide`: Glide back
  / Zurückgleiten. `p_link`: Link Signal {} in FL / Signal {} in FL verknüpfen.
  `p_part_gone`: Part gone / Part fehlt. `p_loading`: Loading the parameters of
  {} / Parameter von {} werden geladen. `p_no_plugin`: No plugin picked: Signal
  1..32 only / Kein Plugin gewählt: nur Signal 1..32.
- `p_name_hint`: Preset name / Preset-Name. `p_loaded`: Loaded {} / {} geladen.
  `p_saved`: Saved {} / {} gespeichert. `p_deleted`: Deleted {} / {} gelöscht.
  `p_load_undone`: Back to the state before / Stand von vorher ist zurück.
  `p_need_name`: Type a name first / Erst einen Namen eingeben.

If the speed value's text is built in pong_ui.rs from a number and a unit
literal, Unit 4 uses `p_speed_unit` instead; if a key above does not exist
under that name, Unit 1 adds it.

Keys the new window no longer uses: `follows_fl`, `follows_fl_after`, `pin`,
`unpin`, `assigned`, `f_shapes`, `f_add_shape`, `f_bound`, `f_part_load`,
`f_routing`, `f_rolls`, `p_summary_on`, `p_summary_off`, `p_pick_preset`,
`p_builtin`, `p_load`, `p_undo_load`, `p_shot`, `p_point`, `p_flick`,
`p_point_hint`, `p_flick_hint`. Unit 1 does not change their texts (a test in
sentence.rs, `h.click("follows FL")` at 2768, still matches one) and does not
remove them; the cleanup after Unit 7 removes them (section 7).

## 7. Implementation units

Eight units. Every file belongs to exactly one unit. Every unit leaves
`cargo test` green and runs cargo only through `plugin/guarded-cargo.sh`, one
cargo process at a time. Paths are relative to `plugin/synthexis/src/` unless
they start with `plugin/` or `probes/`.

Order: Unit 0 and Unit 1 first, in parallel. Then Units 2 to 6 in parallel
(disjoint files), all after Unit 1. Unit 7 last, after all others.

How the seams work without shared files:

- New modules sit where their owner can declare them. Unit 3 declares
  `pub mod graph;` and `mod part_lib;` in fingers.rs (files
  `fingers/graph.rs`, `fingers/part_lib.rs`); Unit 5 declares `pub mod top;` in
  preset.rs (`preset/top.rs`). Unit 1 declares `mod glass; mod edges; mod
  backdrop;` in lib.rs and creates `backdrop.rs` as an empty file with one
  `//!` line so the crate compiles; from then on that file belongs to Unit 6
  only.
- glass: Unit 1 creates `glass.rs` as a copy of `mod glass` (editor.rs:78-606)
  with the v2 changes. editor.rs keeps its own copy until Unit 7 deletes it and
  writes `pub(crate) use crate::glass;` (not `pub use`, which is error E0365
  for a crate-private module), so every `crate::editor::glass` path then
  resolves to the new file. Units 2 to 6 import `crate::glass` in all code they
  touch and own any test change the new tokens cause in their files. The old
  window keeps the old look until Unit 7, so Unit 1 breaks no test elsewhere.
- Shared types the chat field and the hands view both need (`EdgeIn`,
  `Routing`, `NodeTarget`) sit in `edges.rs` (Unit 1), never changed after.
- Old entry points: `Sentence::ui`, `Fingers::ui` and `PongUi::ui` stay as thin
  wrappers that draw the new parts in the old arrangement, because the old
  editor calls them until Unit 7. After Unit 7 they are dead; each carries
  `#[allow(dead_code)] // old window; removed in the ui-redesign cleanup`.
- Cleanup after Unit 7 (not a unit here, because its files belong to Units 1 to
  4): one commit removes the three wrappers, Pong's old preset functions and
  their three tests, the old token names and the unused i18n keys of section
  6. Unit 7's report lists exactly what that commit must remove.

### Unit 0: FL resize probe and vendor patches

Files: `plugin/vendor/nih-plug/nih_plug_egui/src/lib.rs`,
`plugin/vendor/nih-plug/nih_plug_egui/src/editor.rs`,
`plugin/vendor/nih-plug/src/wrapper/vst3/view.rs`, `plugin/vendor/PATCHES.md`,
`probes/fl-resize/` (new).

Work: the three vendor patches of 2.8. A probe plugin in `probes/fl-resize/`
modelled on `probes/fl-camera-plugin/` (same vendored nih-plug by path): an
egui window with buttons that ask for 720, 820 and 900 pt height at 1200 width
through `set_requested_size`, and a label showing the real
`ctx.screen_rect()` size, `pixels_per_point` and the 500 ms verdict (reached or
refused). Build the VST3 bundle and the standalone the way the repo already
bundles plugins (read `plugin/xtask` and `probes/fl-camera-plugin/` first); run
the standalone and record that it resizes and stays resized. Write
`probes/fl-resize/README.md` with the steps for Noa's FL run and a result
table to fill in. Copy nothing into the user's plug-in folders unless
`probes/fl-camera-plugin/` documents that as its normal step.

Tests: a unit test in nih_plug_egui (or the probe) that `EguiEditor::size()`
returns the new size while the request runs; the standalone check above.

### Unit 1: seam (glass, edges, strings)

Files: `glass.rs` (new), `edges.rs` (new), `lib.rs`, `i18n.rs`.

Work:
- `glass.rs`: copy `mod glass` from editor.rs:78-606 with its tests, then
  change it: tokens of 2.1 (old names alias new values), the ramp of 2.2;
  `style` sets Body, Button 13, Small 11, Heading 18, a `TITLE` 15 text style,
  `interact_size.y = CTRL_H`, `button_padding (8, 4)`,
  `item_spacing (GAP, ROW_GAP)`, the text cursor (1 pt `ACCENT_HI`, 0.5 s on,
  0.5 s off). Monospace stays for code-like text only.
- New widgets (contract below): `picker_button`, `picker_popup` (right-aligns
  when it would overflow), `pick_row` (13 pt, 26 high, disabled state,
  optional two-click Delete), `pick_header`, `text_field`, `section_head`
  (with the two-click close of R5.6), `section_rule` (the two-line rule),
  `disclosure`, `plus_button`, `centered_row` (calls `ctx.request_discard()`
  when its stored width is missing or changed, so the row never draws
  left-aligned for a frame), `num_slot`, `toast`, and a compact variant of
  `slider_field` and `toggle` at `CTRL_H_SM` with 11 pt text.
- `edges.rs`: the types below.
- `lib.rs`: `mod glass; mod edges; mod backdrop;` and the one-line
  `backdrop.rs`.
- `i18n.rs`: every new key and changed text of section 6, except the keys
  listed there as unused.

Contract:

```rust
// glass.rs
pub fn style(ctx: &egui::Context);
pub fn picker_button(ui: &mut Ui, id: Id, size: Vec2, text: &str, placeholder: bool, hover: &str) -> Response;
pub fn picker_popup<R>(ui: &Ui, id: Id, button: &Response, max_h: f32, add: impl FnOnce(&mut Ui) -> R) -> Option<R>;
pub struct PickRow { pub clicked: bool, pub delete: bool }
/// `delete`: Some(labels) shows Delete on hover with the two-click arm; `delete` is true on the second click.
pub fn pick_row(ui: &mut Ui, id: Id, selected: bool, enabled: bool, name: &str, place: &str, hover: &str, delete: Option<(&str, &str)>) -> PickRow;
pub fn pick_header(ui: &mut Ui, text: &str);
pub fn text_field(ui: &mut Ui, id: Id, text: &mut String, width: f32, hint: &str) -> Response;
/// Header row HEAD_H: title (15) at the left; when `close` is Some((hover, armed_text)), the rule and
/// an x at the right inset with the two-click arm (4 s, or until the pointer leaves the header).
/// Draws the inset rule under the row. Returns the free slot between title and close group,
/// and true on the confirming second click.
pub fn section_head(ui: &mut Ui, id: Id, title: &str, close: Option<(&str, &str)>) -> (Rect, bool);
/// Full-width two-line rule at y over `x` (window x range).
pub fn section_rule(painter: &egui::Painter, x: egui::Rangef, y: f32);
pub fn disclosure(ui: &mut Ui, id: Id, open: &mut bool, title: &str, summary: Option<&str>) -> Response;
pub fn plus_button(ui: &mut Ui, lit: bool, hover: &str) -> Response;
/// Lays `add` out centred in `width` using the width it took last frame; discards the first frame.
pub fn centered_row<R>(ui: &mut Ui, id: Id, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R;
/// Right-aligned text in a slot as wide as `text` with every digit replaced by '8'.
pub fn num_slot(ui: &mut Ui, text: &str, size: f32, color: Color32) -> Response;
/// The toast of 2.9 at the bottom left of `window`; returns true when an error toast was clicked.
pub fn toast(ctx: &egui::Context, window: Rect, text: &str, error: bool, age: f32) -> bool;

// edges.rs
#[derive(Clone, Debug, PartialEq)]
pub enum NodeTarget { Param { plugin: String, name: String }, Macro(usize), Part { id: u64, name: String } }
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeIn { pub signal: String, pub target: NodeTarget, pub pending: bool }
#[derive(Clone, Debug, Default, PartialEq)]
/// `batch`: id of the current pending batch, 0 when none; changes when a new batch arrives.
pub struct Routing { pub edges: Vec<EdgeIn>, pub focus: bool, pub batch: u64 }
```

Tests: `glass_style_sets_tokens` (13 / 11 / 15 / 18, cursor 1 pt, interact
height 24); `picker_button_has_the_given_size_and_13_pt_text`;
`picker_popup_right_aligns_at_the_window_edge`;
`pick_row_is_26_high_and_a_disabled_row_ignores_clicks`;
`pick_row_delete_needs_two_clicks`;
`section_head_returns_the_slot_and_closes_on_the_second_click`;
`centered_row_never_draws_left_aligned`; `num_slot_width_ignores_the_digits`;
`compact_controls_are_20_high`; `ui_labels_start_uppercase` (over the list of
standalone keys); the copied glass tests and the i18n tests.

### Unit 2: chat field, plugin picker, routes strip and overlay

Files: `sentence.rs`.

Work: split `Sentence::ui` (1748) into the parts below; the picker uses
`glass::picker_button`/`picker_popup`/`pick_row` (its own `pick_row` goes);
labels of R4; no Pin/Unpin; field per R4; scope out of the picker row; the
routes strip and overlay per R9 (combined summary, 35 % height, `quiet`, strip
rect excluded from the outside test, the folding click consumed,
`unfold_routes`); Cmd+Enter and Cmd+Backspace (2.10); R7 and R19 accessors
with `batch`; all stock buttons become `glass::quiet`. The old `ui` stays as a
wrapper that draws the new parts in the old arrangement (#[allow(dead_code)]
after Unit 7).

Contract:

```rust
impl Sentence {
    /// Polls (loader, state, FL, MIDI, reply); once per frame before the parts.
    pub fn tick(&mut self, ctx: &egui::Context, pin: &RwLock<String>, lang: Lang);
    pub fn picker_ui(&mut self, ui: &mut Ui, size: Vec2, pin: &RwLock<String>);
    /// The field with its ghost text, chip badges, completion list and variant panel.
    pub fn field_ui(&mut self, ui: &mut Ui, size: Vec2, enabled: bool);
    pub fn note_ui(&self, ui: &mut Ui, width: f32);
    /// Returns the strip's rect (the overlay leaves it out of its outside-click test).
    pub fn routes_strip(&mut self, ui: &mut Ui, width: f32) -> Rect;
    /// `area`: x range, top and max height of the overlay; not drawn when `hidden`;
    /// `quiet`: a new batch does not unfold it (the Hands Routes view shows it).
    pub fn routes_overlay(&mut self, ctx: &egui::Context, area: Rect, strip: Rect, hidden: bool, quiet: bool);
    pub fn unfold_routes(&mut self);
    pub fn take_open_hands(&mut self) -> bool;
    pub fn routing(&self) -> crate::edges::Routing;
}
```

Tests: update `the_picker_sits_left_of_the_field_on_its_row` (picker width
equals `size.x`, field height 30, field right of picker);
`picker_follows_fl_and_pins` and `picker_groups_stay_open_and_rows_pin` (no Pin
button; the "Follow FL's focus" row unpins; the test at 2768 no longer clicks
"follows FL"); new
`the_picker_shows_plugin_picker_then_the_followed_then_the_pinned_name`;
`routes_run_pending_then_keep_flip_undo` (Keep and Discard in the strip, flip in
the overlay); `the_scope_chip_and_esc` (scope in the strip as "Bottom wall"
together with "Suggested routes: N", its `x` drops it); new
`the_overlay_opens_on_a_new_batch_and_folds_on_keep`; new
`a_quiet_overlay_stays_folded_and_unfold_opens_it`; new
`the_chevron_click_does_not_fold_it_again`; new
`a_sentence_about_fingers_asks_for_hands_once` (true once; false for a face
route; false while a wall is the scope; true with Pong open); new
`routing_lists_pending_hand_param_edges_and_sets_focus_and_batch`; new
`cmd_enter_keeps_and_cmd_backspace_discards`; the completion and variant panel
tests keep passing with 13 pt text.

### Unit 3: Hands

Files: `fingers.rs`, `fingers/graph.rs` (new), `fingers/part_lib.rs` (new).

Work: R6, R10 key order, R13 to R20 (not R7, R8, R9): rotated and refined art
with separate hand-space and canvas constants (R13, R15); label nodes, thumb
node above, bent lines, angle order and `spread` (R13); `finger_colour`, the
white tip dot and click-before-texture (R14); Fist/Open chips, "+N older
shapes" with descriptive names, the empty hint (R16); part editor rows, the
reserved Back slot, the part picker with templates and saved parts, the name
field with Save, roll wheel rule, "Repeats" (R17); scroll to the owner row on
select (2.8); lanes grid (R18); Routes view with `graph::layout`/`paint`,
rising-edge switch and return, node dither bands, pending dashes, unfold flag
(R19); header with Calibrate, Reset when calibrated, key root before mode
(R20). The old `ui` stays as a wrapper with `Routing::default()`.

Contract:

```rust
impl Fingers {
    /// Single | All | Routes on the left; key, Calibrate fingers, Reset on the right of `slot`.
    pub fn header(&mut self, ui: &mut Ui, slot: Rect, writer: &Writer, t: &Text);
    /// The hands view and, under it, the part editor (Single), all parts (All) or the node view (Routes).
    pub fn section(&mut self, ui: &mut Ui, writer: &Writer, t: &Text, de: bool, routing: &crate::edges::Routing);
    /// True while Routes shows the batch in `routing` (the window passes it to the overlay as `quiet`).
    pub fn shows_routes(&self) -> bool;
    /// True once after a click on a target node.
    pub fn take_unfold(&mut self) -> bool;
}
// fingers/graph.rs
pub use crate::edges::*;
pub struct Graph { /* nodes with rects and colours, edges with dot lists */ }
pub fn layout(edges: &[EdgeIn], width: f32, t: &Text) -> Graph;
pub fn paint(ui: &mut Ui, g: &Graph, origin: Pos2);
pub fn plugin_colour(name: &str) -> Color32;
pub fn signal_label(signal: &str, t: &Text) -> String;
// fingers/part_lib.rs
pub struct PartPreset { pub version: u32, pub name: String, pub play: Play, pub mode: Mode, pub start: Start }
pub fn list() -> Vec<String>;
pub fn load(name: &str) -> Result<PartPreset, String>;
pub fn save(name: &str, part: &Part) -> Result<(), String>;
pub fn delete(name: &str) -> Result<(), String>;
```

`wall_editor`, `wall_switched`, `take_wall_pick`, `bound`, `calibrating`,
`reset_textures` keep their signatures (Pong and the window use them).

Tests: rewrite `the_art_has_five_clickable_fingers_and_a_dissolving_wrist` for
the 160 x 160 canvas (all lit dots in bounds, each finger region at least 150
dots, tips inside their region, the right hand's middle tip right of and above
the palm centre, the left hand mirrored, the wrist trail as long as today);
new `each_long_finger_has_two_joint_seams`; new `labels_never_overlap`
(`spread` on four tips within 10 pt; gaps at least 26; inside `lo..hi`); new
`no_label_line_crosses_another_finger` (for both hands with all five fingers
bound, no line segment passes a lit dot of another finger's region); new
`nodes_follow_the_finger_angle_and_the_thumb_sits_above`; new
`a_selected_finger_stays_lit_when_the_camera_sees_it_folded`
(`finger_colour`); new `a_click_selects_in_the_same_frame`; new
`shapes_offer_fist_and_open_and_keep_old_bound_ones_behind_one_chip` (names
"Thumb and pinky", "Thumb and index"); new `part_editor_rows_in_order` (Win
harness: owner label above the kind segmented above the bars field; Hide roll
and Remove on the owner's row, Remove right of Hide roll; the bars field does
not move when Back appears); new
`the_part_picker_lists_templates_then_saved_parts_and_empty_clears`; new
`part_presets_save_list_load_delete` (`SYNTHEXIS_PARTS` temp dir); new
`lanes_grid_alphas` (`grid_alpha`); new
`routing_layout_has_no_overlaps_and_hand_colours`; new
`a_dither_edge_switches_colour_in_the_middle_half`; new
`routes_view_switches_once_per_batch_and_returns`; new
`selecting_scrolls_the_owner_row_to_the_top`; new
`key_shows_root_before_mode`; the existing Win tests (`f_roll_hide`,
`f_remove`, the wall editor tests) keep passing.

### Unit 4: Pong section

Files: `pong_ui.rs`.

Work: R21, R22: header slot with "On", the "Live is off" chip, stats in
`num_slot`s (HINT while off); the centred 460 column; compact sliders and
toggles; new labels (Max balls, Harder shot plays louder, Mute the shooting
hand's parts, screens/s); body without the presets row. The old `ui` wrapper
draws `header` in a row first, then `body`, so the window keeps its Pong on
toggle until Unit 7. `presets_row`, `load`, `undo_load`, `save`, `delete` and
their three tests stay, reachable only from the old wrapper, until the
cleanup.

Contract:

```rust
pub struct PongHead { pub live_on: bool }
impl PongUi {
    /// "On" toggle and, while `live` is false, the Live is off chip after the title; stats on the
    /// right of `slot`. `live_on` is true when the chip was clicked.
    pub fn header(&mut self, ui: &mut Ui, slot: Rect, writer: &Writer, t: &Text, live: bool) -> PongHead;
    pub fn body(&mut self, ui: &mut Ui, writer: &Writer, fingers: &mut Fingers, t: &Text, de: bool);
}
```

`frame`, `sync_scope`, `canvas`, `select` keep their signatures.

Tests: new `there_is_no_shot_choice` (no `EN.p_point`/`EN.p_flick` text
drawn); new `the_shooting_hand_is_above_and_the_settings_under_the_side_view`;
new `the_body_is_one_460_column` (hand row, side view, slider row and toggles
centred on one x, slider widths adding up to 460); new
`compact_controls_are_20_high`; new `the_stats_sit_in_the_header_slot_and_do_not_jitter`
(slot width equal for "1 flying" and "9 flying"); new
`the_live_chip_shows_only_while_live_is_off`; new
`test_shot_reads_disabled_not_missing` (alpha at least 0.40); the side view,
canvas band and chip tests unchanged.

### Unit 5: presets model, view state, preset column

Files: `preset.rs`, `preset/top.rs` (new), `params.rs`.

Work: R5 model (`Section`, `sections` in `Snapshot`, built-ins that never wipe
unrelated work, pin and key kept for built-ins, `sections_of`, `opens` as a
union, `with_section`, `without_section` keeping parts a wall also plays,
twins with `list()` entries, `load_file`, `delete_file`); the `view` param and
the default size `EguiState::from_size(1200, 820)`; the preset column: picker
with the pick table of R5.3 and its two-click replace, "+", row 2 in its three
states, Undo with the lifetime of R5.4, close with Undo, the toast messages,
the edited marker.

Contract:

```rust
// preset.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section { Hands, Pong }
// Snapshot gains: #[serde(default)] pub sections: Vec<Section>
pub struct Entry { pub name: String, pub file: Option<PathBuf>, pub builtin: bool, pub twin: bool }
pub fn list() -> Vec<Entry>;                             // built-ins first, then saved (twins tagged)
pub fn load_file(path: &Path) -> Result<Preset, String>;
pub fn delete_file(path: &Path) -> Result<(), String>;
pub fn builtin(name: &str) -> Option<Preset>;            // "Hands" (empty), "Pong"
pub fn sections_of(s: &Snapshot) -> Vec<Section>;        // by content
pub fn opens(s: &Snapshot) -> Vec<Section>;              // s.sections, then sections_of not in it
pub fn has_content(s: &Snapshot, sec: Section) -> bool;
pub fn with_section(now: Snapshot, s: Section) -> Snapshot;
pub fn without_section(now: Snapshot, s: Section) -> Snapshot;
// params.rs
#[persist = "view"] pub view: Arc<RwLock<String>>,
// preset/top.rs
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct View { pub sections: Vec<Section>, pub preset: Option<String> }
impl View { pub fn read(params: &SynthexisParams) -> View; pub fn write(&self, params: &SynthexisParams); }
pub struct Ctx<'a> { pub writer: &'a Writer, pub params: &'a SynthexisParams, pub t: &'a Text, pub discard: &'a mut dyn FnMut() }
#[derive(Default)] pub struct Presets { /* list, saving, toast, before, before_view, after, changed_at, armed */ }
impl Presets {
    pub fn picker(&mut self, ui: &mut Ui, size: Vec2, view: &mut View, cx: &mut Ctx);
    pub fn plus(&mut self, ui: &mut Ui, view: &mut View, cx: &mut Ctx);
    pub fn row2(&mut self, ui: &mut Ui, width: f32, view: &mut View, cx: &mut Ctx);
    pub fn close(&mut self, view: &mut View, s: Section, cx: &mut Ctx);
    /// Text for the close arm ("Close Hands? 3 parts go").
    pub fn close_text(&self, s: Section, cx: &Ctx) -> String;
    /// Once a second: drops Undo per R5.4 and refreshes the edited marker.
    pub fn tick(&mut self, now: Instant, cx: &Ctx);
    pub fn toast(&mut self, ctx: &egui::Context, window: Rect);
}
```

Every `Presets` method that changes `view` writes it to `params` in the same
call, right after `apply` (R5.10).

Tests (preset.rs): `builtins_are_hands_then_pong`;
`a_builtin_keeps_the_pin_and_the_key`;
`old_presets_open_the_sections_with_content` (JSON without `sections`);
`opens_is_never_smaller_than_the_content`;
`with_section_pong_keeps_finger_parts_and_binds`;
`without_section_hands_drops_only_hand_binds_parts_and_lines_and_keeps_wall_parts`;
`a_flick_preset_keeps_its_trigger` (serde round trip);
`a_saved_hands_twin_is_listed_loaded_and_deleted_by_file`.
Tests (preset/top.rs): `view_round_trips_and_an_empty_field_derives_from_content`;
`the_plus_menu_greys_open_sections`; `save_as_writes_the_open_sections`;
`save_as_is_visible_in_every_state`; `row2_fits_in_english_and_german_at_170_and_204`;
`the_builtin_name_is_refused`; `save_then_the_picker_lists_the_new_name`;
`a_failed_load_keeps_the_pending_lines`;
`picking_hands_keeps_pong_parts_until_the_second_click`;
`picking_pong_merges_only_into_an_empty_pong`; `repicking_pong_reloads_it`;
`close_needs_two_clicks_then_undo_restores_state_and_sections`;
`undo_survives_two_seconds_of_pong_play`;
`undo_goes_10_s_after_a_user_edit`; `a_saved_preset_reads_edited_after_a_change`.

### Unit 6: dither background

Files: `backdrop.rs` (Unit 1 creates it empty).

Work: section 4 as written: palette and hue path, fields in window
coordinates with the eased height, flow per band rows, the per-cell rule with
the gentle text-area dither, 8 bands, the texture. No egui state outside the
`Backdrop` struct.

Contract:

```rust
pub const CELL: f32 = 3.0;
#[derive(Default)] pub struct Backdrop { /* start, tex, buf, flow rows, LUTs, next band, eased height */ }
impl Backdrop {
    pub fn reset_texture(&mut self);
    /// Renders one band and paints the whole texture over `window`; `text_area` gets the gentle dither.
    pub fn paint(&mut self, ctx: &egui::Context, painter: &egui::Painter, window: Rect, text_area: Rect);
}
/// Pure: cells of `rows` at time `t` for a window of `w` x `h` pt (eased height `he`).
pub fn render_rows(w: f32, h: f32, he: f32, rows: std::ops::Range<usize>, text_area: [usize; 4], t: f64, out: &mut [Color32]);
pub fn field_hue(i: usize, t: f64) -> f32;
```

Tests: `render_is_deterministic`; `far_from_the_fields_is_the_base_colour`
(within 1 level of `#0a0a0c`); `the_core_is_smooth` (cells with h >= 0.42 have
no dither step); `the_seam_dithers_instead_of_greying` (an 8x8 tile between two
fields of different hue holds both hues); `the_text_area_dither_is_gentle`
(neighbouring cells in the text area differ by at most 0.07 L);
`text_contrast_at_the_brightest_cell` (over every hue: alpha 0.52 at least
4.5:1, 0.56 at least 5:1); `hues_sweep_and_never_rest_on_a_hand_colour` (each
field's hue covers at least 60 deg in any 300 s window and never enters
36..86 or 197..247 deg); `no_field_moves_faster_than_0_02_S_per_second`;
`opening_a_section_does_not_move_the_fields` (same W, H change: field centres
move less than 1 pt in the first frame); `#[ignore]` release bench
`backdrop_bench_1200x900` (one band at most 0.8 ms, full at most 6 ms).

### Unit 7: window integration

Files: `editor.rs`, `engine/camera.rs` (preview only; read `git diff` on it
first, another session edits `engine/*`), `plugin/synthexis/Cargo.toml` (only
if an objc2-foundation feature for `NSRect` is missing).

Work: the composition of section 1 (camera, edge line, top grid of 2.5 with
`Sentence::picker_ui`/`field_ui`/`note_ui`/`routes_strip` and
`Presets::picker`/`plus`/`row2`, the overlay with `quiet` from
`Fingers::shows_routes()`, sections with `glass::section_head` and the close
arm, `Fingers::header`/`section`, `PongUi::header`/`body`, rules, Advanced,
bottom room, the toast); the scrolling panel area; `View` read and write and
the union of R5.7 on `state_gen`; R7 append; wall pick append; unfold wiring
(`take_unfold` -> `unfold_routes`); the Pong chip turning Live on; Settings of
R8; the top bar fade with the gear and Live dot at 0.55 and the proportional
number slots; the window plan, `max_h` from `NSScreen` and `ResizeGate` of 2.8;
the preview of section 3; `Backdrop::paint` first in the frame with the text
area; delete editor.rs's `mod glass` copy and its tests and write
`pub(crate) use crate::glass;`; removal of `frosted`, `FROST`, `PREVIEW`,
`Folds`, `sections()`.

Contract it relies on: everything above. It provides no new API to other
units.

```rust
// editor.rs, pure and tested
fn window_plan(width: f32, below: f32, max_h: f32) -> f32;           // want_h
fn cam_height(width: f32, actual_h: f32, below: f32) -> f32;          // from the real height
struct Settle { /* open list, width, frames since change, below */ }
struct ResizeGate { last: Option<(Instant, Vec2)>, refused_until: Option<Instant> }
impl ResizeGate { fn ask(&mut self, now: Instant, actual: Vec2, want: Vec2) -> bool; }
fn top_columns(width: f32) -> [Rect; 4];   // plugin, chat, preset, plus (row 1)
fn needs_upload(last: Option<&Arc<Preview>>, now: &Arc<Preview>) -> bool;
fn screen_max_h() -> f32;                  // NSScreen visibleFrame - 60, at most 900, else 820
```

Tests: `window_plan_cases` (no section asks 820 at W 1200 with max 820;
Hands-sized below asks max_h; a smaller below asks less); `camera_follows_the_real_height`
(with a refused resize, actual 720: camera at least 320, no blank band);
`the_plan_ignores_content_changes_inside_a_section` (below grows by 100 with
the same open list: no new ask, camera unchanged);
`resize_requests_wait_150_ms_and_back_off_after_500_ms_unanswered`;
`top_columns_share_one_picker_width_and_8_pt_gaps` (no overlaps, plus inside
the right inset, P clamped at 170 and 220, chat at least 700 at W 1200);
`a_frame_uploads_once`; `a_reopen_uploads_the_current_frame_again`;
`mirror_to_rgba_flips_and_swizzles` (camera.rs, 4x2 synthetic BGRA buffer with
padding in the stride); `content_always_opens_its_section` (a reply with a
hand route while Pong is open appends Hands; a wall pick appends Pong); the
Settings tests (`overlay_starts_english_and_switches_to_german`,
`completion_switch_is_saved`, `esc_close_button_and_a_click_outside_close_it`)
with the new signature, plus `signals_and_log_start_folded` and
`the_account_note_sits_under_the_account_settings`;
`the_top_bar_keeps_gear_and_live_visible_after_the_fade`;
`no_stock_buttons_in_the_window` (source scan of R11); `crop_and_joint_mapping`,
`the_contain_fit_keeps_the_whole_frame_for_wide_and_tall_rects` updated to the
window plan; `frosted_preview_averages_blocks`,
`section_header_click_folds_and_reopens` and the old `camera_height_rule`
deleted with what they tested.

Check on the real artifact (build-rust section 5): build the standalone
(`plugin/synthexis/src/main.rs`), open the window, capture only that window
(`screencapture -l <window id>`, never the full screen) in the five states of
mockups A to E, and describe each region against section 1 before calling it
done: camera sharpness (edge energy in the camera rect above the old 320x180
upscale), the dither visible and moving (pixel variance in the panel area over
two captures 2 s apart, non-zero and small), the wings lit in B and C, the
rules, the top grid rects within 0.5 pt of `top_columns`, no clipped or
wrapped labels in English and German, the chat field's text next to the
plugin picker's (2.2), the part editor's left-aligned rows (2.6). Then the same
states in FL Studio with Unit 0's verdict at hand. The report lists what the
cleanup commit removes.

## 8. Requirement to unit map

| R | What changes | Unit |
|---|---|---|
| R1 | full-size mirrored preview, upload on new frames only, reset on reopen | 7 |
| R2 | `backdrop.rs` dither fields on the dusk hue path; frost removed | 6, 7 |
| R3 | no cards; two-line full-width rules; 24 pt bottom room | 1, 7 |
| R4 | top grid at 1200; picker labels; taller field, thin cursor; preset picker; Save as row | 1, 2, 5, 7 |
| R5 | `Section`, non-destructive built-ins, "+", two-click close with Undo, `view` param, content always shows | 5, 7 |
| R6 | `f_title` Hands | 1 |
| R7 | `take_open_hands`, the window appends Hands whatever is open | 2, 7 |
| R8 | Signals, diagnostics and log in Settings; account note placed; Advanced last | 7 |
| R9 | routes strip and foreground overlay, foldable, quiet while Routes shows it | 2, 7 |
| R10 | 11 / 13 / 15 / 18 ramp, sentence case, quiet buttons, number slots, key order | 1, 2, 3, 4, 7 |
| R11 | grid of section 2, cuts listed in R11, source-scan test | all |
| R12 | window plan from the real height, resize requests, vendor patches, FL probe | 0, 7 |
| R13 | rotated art, nodes by angle, bent lines, `spread` | 3 |
| R14 | `finger_colour`, tip dot, click before texture | 3 |
| R15 | three phalanges, taper, joint seams, ring length | 3 |
| R16 | Fist and Open hand chips; older shapes behind one chip with descriptive names | 3 |
| R17 | three rows, Remove by Hide roll, part picker, name and Save | 3 |
| R18 | `grid_alpha` values for lanes | 3 |
| R19 | Routes view, `fingers/graph.rs`, dithered edges and nodes, rising-edge switch | 1, 2, 3, 7 |
| R20 | Calibrate and Reset in the header; status in Settings | 3, 7 |
| R21 | Point/Flick UI gone, `trigger` kept in serde, one shot already at runtime | 4 |
| R22 | hand above, compact settings under the side view in one column, stats in the header | 4 |
| R23 | presets row out of Pong, into `preset/top.rs` | 4, 5, 7 |

## 9. Critique log

Three reviews read v1: a requirement checklist (C1-C25), a feasibility check
against the code (F1-F24) and a friction review (Fr1-Fr36). Duplicates are
answered once and cross-referenced.

| # | Objection (short) | Answer |
|---|---|---|
| C1, F3, Fr1 | chat field narrower than today | accepted: default W 1200, P = clamp(W*0.17, 170, 220), chat 718 at 1200, test >= 700 (2.5). "+" stays in row 1 next to the picker, as the brief says; Noa may keep 1000 (open question) |
| C2, Fr2 | built-in Hands wipes Pong and routes | accepted: built-ins never clear unrelated work; closing other sections with content needs a second click; only saved presets replace the state (R5.3). C2's "set sections to exactly X" alone would leave hidden content playing, so others close through point 6 |
| C3, C17 | background static, reuses hand colours; dawn-scape not used | accepted: hue path from dawn-scape and sky-palette, 258 to 18 deg, 173 to 283 s periods, never in the hand bands; test of the sweep (4.2, 4.3) |
| C4, Fr3 | one-click `x`, short Undo, five meanings of `x` | accepted: two-click close, 16 pt and a rule before `x`, Delete text for files (2.7, R5.6, R20) |
| C5, Fr5, F18 | finger part made while Pong open plays hidden | accepted: R7 appends Hands whatever is open; `opens` is a union; wall pick appends Pong (R5.7) |
| C6, Fr14, F22 | row 2 states undefined and too wide | accepted: fixed table, Save as always visible, messages in a toast, German `undo` "Zurück", width test (R4, 2.9) |
| C7, F11, Fr8 | resize unverified; 900 too tall | accepted: Unit 0 probe first, fallback written as R12's answer, max_h from NSScreen else 820 (2.8, R12) |
| C8, F5, Fr11 | label lines cross fingers | accepted: thumb node above, others by angle, bent lines behind the art, crossing test (R13) |
| C9, Fr15 | template dropdown repeats the name; two pickers | accepted: one part picker reading "Template", saved parts in it, Load button gone (R17) |
| C10 | nodes flat, no dither where colours meet | accepted: 8 pt dither band at entries, two-hand split (R19) |
| C11, F6, Fr17 | auto Routes flips back every frame and never returns | accepted: rising edge on `batch`, return to the previous view (R19) |
| C12 | Flick presets keep a second shot | rejected: `pong/setup.rs:94` already shoots Flick as a point, so one shot exists; normalising would touch the Pong runtime, which no unit owns. `trigger` stays as is (R21) |
| C13, Fr32 | Reset hidden in Settings | accepted: Reset stays next to Calibrate, shown only when calibrated; Settings shows status. Fr32's extra Calibrate in Settings rejected (a duplicate, R11); its camera-status point accepted (Diagnostics has no summary); the role dropdowns already carry the hand name (editor.rs:757-762), no change |
| C14 | Signals under the account note | accepted: note under account settings, "Saved with the project" heading (R8) |
| C15, F7, Fr16 | overlay covers the Routes view | accepted: 35 % height; `quiet` keeps it folded while Routes shows the batch (R9, R19) |
| C16 | re-picking the shown preset does nothing | accepted: re-pick reloads, with Undo and toast (R5.3) |
| C18 | 1280 source upscaled on Retina | accepted in part: the limit is written down (1.9x at 1200). A 1920x1080 capture is rejected: it changes what perception receives, the other session's area (3) |
| C19 | compact controls not smaller | accepted: `CTRL_H_SM` 20, 11 pt, widths about 15 % less (R22) |
| C20 | key mode before root | accepted: root before mode (R10, Unit 3) |
| C21, Fr27 | mixed fonts and jitter in the top bar and stats | accepted: proportional font, `num_slot` (2.2) |
| C22, Fr34 | legacy shapes keep cryptic names and can break the row | accepted: one "+N older shapes" chip, names from the finger mask (R16) |
| C23 | brief says centring, spec left-aligns | accepted: rationale in 2.6, capture check, reserved Back slot |
| C24 | camera shrinks to 300 with Hands | accepted: `cam_min = max(320, 0.4 H)` (2.8) |
| C25 | 13 pt may look thin in the taller field | accepted as a check: capture comparison, BODY 14 if needed (2.2) |
| F1 | host gets the old size during the resize | accepted: third vendor patch order fixed (2.8, Unit 0) |
| F2 | refusal never detected; layout from want_h | accepted: layout from the real height, 500 ms rule, debug_assert to log, default 1200 x 820 (2.8) |
| F4 | `Fingers::ui` defined twice | accepted: new name `section` (Unit 3) |
| F8, Fr13 | fields jump when a section opens; seams; skip rect | accepted: S from W only, homes in window coordinates with eased height, nothing skipped, 8 bands (4.1, 4.3) |
| F9 | saved "Hands" twin disappears | accepted: `list()` entries, `load_file`, `delete_file`, twins kept (R5) |
| F10 | Unit 1's style change breaks other units' tests | accepted: glass.rs is a copy; the window switches in Unit 7; units own their test changes (7) |
| F12 | stale texture on reopen; egui type in the engine | accepted: `last` reset with the texture; `Preview` without egui, one copy in the editor (3) |
| F13 | `pub use crate::glass` is E0365 | accepted: `pub(crate) use` (7) |
| F14 | chevron click folds the overlay again | accepted: strip rect left out of the outside test (R9) |
| F15 | no path for "click a target node unfolds" | accepted: `take_unfold`, `unfold_routes` (R19) |
| F16 | who writes `view` | accepted: every changer writes in the same call; re-read on `state_gen` after own writes (R5.10) |
| F17 | built-in resets the key; wall parts lost on close | accepted: key kept like the pin; `without_section` keeps parts a wall plays (R5.3, R5.6) |
| F19 | Pong on toggle lost between Units 4 and 7 | accepted: old wrapper draws `header` first (Unit 4) |
| F20 | whole flow grid per band too slow | accepted: only the band's coarse rows plus one (4.4) |
| F21 | `centered_row` jumps on first frames | accepted: `request_discard` (Unit 1) |
| F23 | tips need the forward map; ART_H reused | accepted: forward map stated, separate hand-space and canvas constants (R13) |
| F24 | selection one frame late | accepted: click read before the texture is built (R14) |
| Fr4 | Undo lifetime undefined | accepted: until the next preset action, or 10 s after a user change; Pong play does not count; test (R5.4) |
| Fr6 | black wings at a short camera | accepted: field A sits in the camera band, nothing skipped, mockups B and D show wings (4.3, 1) |
| Fr7 | camera moves with in-section content | accepted: `below` settles on section-list or width changes only (2.8) |
| Fr9 | part editor below the fold | accepted: scroll to the owner row on select (2.8) |
| Fr10 | empty Hands gives no prompt | accepted: hint where the editor goes (R16) |
| Fr12 | text over a moving high-contrast checker; HINT below 4.5:1 | accepted in part: gentle dither in the text area (0.60 / 0.25), peak L 0.30, HINT 0.52 with a 4.5:1 test. The switch from Ubuntu-Light to a regular weight is not made here: it needs a bundled font and changes the whole look, so it is an open question for Noa |
| Fr18 | Settings and Live vanish with the fade | accepted: gear and Live dot stay at 0.55; "Live is off" chip in Pong (1, R22) |
| Fr19 | "At once" twice, "velocity" ambiguous | accepted: Max balls, Harder shot plays louder, Mute the shooting hand's parts, screens/s (6) |
| Fr20 | four left edges in Pong | accepted: one 460 column, sliders sum to it (R22) |
| Fr21 | weak hierarchy | accepted: titles 15, two-line rule (2.1, 2.4) |
| Fr22 | picker name stale; no one-click save | accepted in part: "edited" marker for saved presets; Save as prefilled with the picked name so Enter overwrites. A third button in row 2 is rejected: it does not fit P in German |
| Fr23 | follow and pinned differ only by alpha | rejected: Noa asked to remove the "follows FL" text (R4); a note or glyph brings it back. The hover says "Following FL's focus" |
| Fr24 | scope disappears behind the pending summary | accepted: combined summary (R9) |
| Fr25 | "roll" means three things | accepted: "Rolls" becomes "Repeats"; "Hide roll" stays as Noa named it (R17) |
| Fr26 | "Routing" vs "routes"; no hovers | accepted: "Routes" with hovers (R19) |
| Fr28 | selected and bound look alike | accepted: white tip dot and selected node; moving field homes is moot since the fields no longer use hand hues (R13, R14) |
| Fr29 | mockups indent rows differently | accepted: mockups redrawn on one column (1) |
| Fr30 | "+" popup overflows | accepted: right-aligned when it would overflow (2.7) |
| Fr31 | node sizes and pending look underspecified | accepted: widths, group rows, dashes and hollow targets (R19) |
| Fr33 | German strings | accepted: all four (6) |
| Fr35 | wheel over the roll | accepted: roll takes the wheel only on overflow (R17) |
| Fr36 | no keyboard path | accepted in part: Cmd+Enter and Cmd+Backspace in the field (2.10). A window-wide Cmd+Z is rejected: FL uses it for its own undo |
