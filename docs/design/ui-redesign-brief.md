# UI redesign: Noa's requests of 2026-11-04

Noa's feedback after the first FL test of Pong and the finger editor,
sorted into numbered requirements. Every design and every review checks
each number. Screenshots Noa sent (read them): the main window
(`/private/tmp/claude-501/-Users-no8-Desktop-life-Lifemaxxing-synthexis--claude-worktrees-parser-v11-de5d19/fcc8579e-206b-4500-8c3e-e3363e4b6a50/images/1.jpg`),
the finger section (`.../images/2.jpg`), the Pong section (`.../images/3.jpg`).

His test for everything: the least experienced user, the fastest user and
the user who understands everything all get through with no friction, and
the window looks good in a demo. "No mistakes."

## Camera and background

- R1. The camera preview in the window is too low-res (today 320x180,
  `engine/camera.rs` PREVIEW_W/H). Show it sharp at the window's size.
  The engine keeps its own analysis resolution; this is only the preview.
- R2. Below the camera the frosted, mirrored low-res copy of the camera
  (the "pass-through" glass behind the panels) looks bad. Replace it with a
  dither background: dark base, slow pulsing colour fields whose edges meet
  the black in an ordered-dither falloff. Colours change and drift
  position, slowly and elegantly, never fast. Softer than the hard edges on
  Noa's portfolio, but with its gradients as the reference: noakatana.com
  (deployed) and the source in
  `/Users/no8/Desktop/life/Lifemaxxing/noa-portfolio/src/scripts/`
  (`dawn-scape.ts`, `gl-surface.ts`, `c/oklab.ts`, `c/dome-*.ts`). Port the
  idea to Rust/egui (CPU texture or mesh), cheap enough to run every frame.

## Structure

- R3. Sections are not rounded cards. Each runs edge to edge; a thin
  separator line divides them. Bottom gets a little more breathing room.
- R4. Top section (always there): left the plugin picker (called "Plugin
  picker" or the picked plugin's name, not "follows FL"), centre the
  sentence/chat box (wider, a bit taller, the blinking text cursor
  smaller), right the preset picker (same design and size as the plugin
  picker; it lists Hands, Pong and saved presets). Under the preset picker a
  Save button that opens a "Save as" field/dialog.
- R5. The preset picker decides which feature section shows below: picking
  Hands shows the Hands section, picking Pong shows the Pong section. With
  nothing picked no feature section shows (only camera, top section,
  Advanced). Next to the preset picker a "+" adds another section
  (instance): its menu offers Hands and Pong, with an already-open one
  greyed out. The "+" is dim while no preset is picked and highlighted once
  one is.
- R6. Rename "What fingers play" to "Hands".
- R7. Typing a sentence about fingers or hands into the chat box (with no
  preset picked) opens the Hands section automatically.
- R8. Signals 1-32 move into Settings. In Settings the log is collapsed
  behind a disclosure; the Signals sit there too. Advanced stays as the
  last row.
- R9. Routing suggestions (today a row appears next to the plugin picker,
  e.g. "Bottom wall") show BELOW the top section as an overlay that may
  cover the sections under it, and the overlay can be collapsed.
- R10. Fonts: the plugin picker's text size is the reference; the other
  controls move up to it. Consistent capitalisation everywhere. Same size
  for siblings (today Load and Save differ in the Pong section).
- R11. Spacing, centring and alignment are checked everywhere and follow
  one layout logic; cut buttons that are not needed; move rarely used
  controls into Settings.
- R12. The window sizes itself sensibly: the panel area grows with the
  open section instead of a fixed big block.

## Hands section

- R13. The two hands turn outward by about 45 degrees (left hand tilted
  left, right hand tilted right) so the space left and right of them holds
  the labels; each label is a small node joined by a line to its finger.
  Labels never overlap (today index and ring labels on the left hand
  collide).
- R14. Clicking a finger highlights it at once, independent of the camera
  (today it lights only when the camera sees it, and the camera misreads).
- R15. The dither hands get a little more realistic (finger proportions,
  joints); only fine tuning.
- R16. "Shapes" (fist, open, shaka, L, ...) look like training leftovers;
  Noa does not know what Shaka or L mean. Hide or rework: at most a compact,
  self-explaining place; fist and open are the useful ones.
- R17. The part editor (pattern/chord/held note, channel, start, gate
  mode, roll) needs segmentation: a header row, then the editor row. Remove
  sits next to Hide roll, not alone on the left. Bars, Lanes/Piano roll and
  the templates follow the same centring as the rows above. Templates
  (Hi-hat basic, Trap roll, Snare fill, Kick, Clap, Empty) go into a
  dropdown styled like the plugin picker. On the free right side: preset
  name, Save, Load for the part.
- R18. Lanes view: more grid lines (beat and bar separators), it is hard to
  read today. The piano roll is good, keep it.
- R19. When a parameter is the target (picked, or written in the chat), the
  Hands section shows routing as a node view: minimal, elegant, like a
  calm FigJam board; nodes with dither where colours meet; left hand
  orange, right hand light blue, each plugin its own colour. Hands to
  fingers to targets.
- R20. Calibrate fingers stays (Noa liked finding it), its result is a
  separate bug handled by another session.

## Pong section

- R21. Drop the Point/Flick choice: one built-in shot (the best one; the
  shot logic is fixed separately in `pong/shot.rs`).
- R22. Shooting hand stays above the side view; speed, bounces, time
  limit, at once, velocity from the shot, faster with a harder shot, pause
  fingers go UNDER the side view, smaller. Stats (flying, hits, dropped)
  sit in one tidy row, not a line of their own.
- R23. Pong's own preset row (load/save) moves to the top section's preset
  picker; inside the section keep only what is Pong.

## Later (not in this redesign)

- Head section (mouth, eyes, turn, tilt; contours in dither, clickable
  eyes/mouth/head) and a "Head" preset: its own session after this
  redesign lands.
