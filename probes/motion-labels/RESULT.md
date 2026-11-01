# RESULT: motion.labels, Spark Open freestyle timelines and segment QA

Status 2026-11-01: tool built, segment QA run on S1 and S2 (second run after a frame-lookup fix and a count rule), timeline pilot on S2 (3 of 8 windows). S3 is recorded but not yet extracted or SAM-labelled; the timelines run on S1 to S4 in one go once S3 and S4 are labelled.

Tool: `uv run tools/labels/motion_labels.py <session> [--max-usd 15] [--passes 1|2] [--only qa|timeline] [--limit-jobs N] [--dry-run]`, review page via `uv run tools/labels/motion_labels.py report <session>...`. Outputs under `data/motion_labels/<session>/` (not committed, faces): `timeline.json`, `qa.json`, `review.html`, `frames/` (the exact images sent). Runs need `SYNTHEXIS_BASETEN_CAP` above the ledger total (73.57 $ before this work; default 60), here 95.

## Design, as built

- Timeline: each freestyle segment (block G) in 10 s windows, frames at 2 fps, 768x432, SAM overlay (hand green fill, face blue outline so expressions stay visible, object orange), yellow trail of hand-box centres over the previous 0.5 s from the 15 fps SAM labels, white ring on the current hand centres. Reply: events with `t_start`, `t_end`, `hand` (anatomical R/L/both/none), `primitive` (motion.primitives vocabulary split into throw, swipe, push_pull, hold, pinch, circle, grab_release, stretch, orb, clap, plus face_head and other), `params`, free-text `name`, `confidence`. Times are seconds since segment start; `seg_start_s` keeps the absolute session time.
- QA: 3 frames per scripted segment at 20/50/80 % of the span inside the protocol margins, 8 segments per request. Count segments (`B_*_count*`) use 35/70/98 % of the raw span instead, because Noa acts out counting from one finger up to the target in every count segment (also on the way down), so the target appears late; the prompt accepts frames with fewer fingers on the way up in the order index, middle, ring, pinky, thumb. Segments whose frames were dropped as bad during extraction are skipped (S1: `A_both_11000_11111`). Reply per segment: match yes/no/unsure, a verdict per frame, issue text. Flags: `mismatch` (2 or 3 frames wrong), `partial` (1 frame wrong), `unsure`, `ok`. The prompt names the per-session object stand-ins from `tools/labels/objects.json` (mug = soda can).
- Budget: `--max-usd` caps the sum of every `motion-*` run in the ledger, so reruns and later sessions share one 15 $ cap; `spark.run` adds the up-front estimate, pilot re-estimate and rolling stop.
- Recordings are not mirrored (checked on A_L frames: left arm with watch enters from image right; recorder mirrors the preview only). The prompt says so and asks for anatomical sides and image-coordinate directions.

## Measured on Baseten (Muse Spark Open)

- At most 8 images per request ("Too many images: 10 (max 8)", HTTP 400, no charge). The budget probe's 20 images per request does not work.
- Tiling costs the same as single frames: a 2x2 grid of 768x432 frames (1536x864) bills about 1,790 prompt tokens, a 1x3 row (2304x432) about 1,300; no internal downscale, and Spark read every burned-in frame label correctly. So a timeline window is 5 grids (20 frames), a QA request 8 rows (24 frames).
- `spark._estimate_usd` counted base64 characters as text (about 40x too high for images); it now counts each image part as `IMAGE_TOKENS_EST` tokens (450 default, 1,750 set by motion_labels for the grids). Test added.

## Cost so far

| run | requests | $ |
|---|---|---|
| motion-limit-probe (image limit, tile tokens) | 6 | 0.03 |
| S2 timeline pilot (3 windows) | 3 | 0.07 |
| S2 QA (pilot 3 + full 12; the pilot used the earlier prompt) | 15 | 0.44 |
| S1 QA | 10 | 0.25 |
| S1 QA rerun (fixed frames, count rule) | 10 | 0.26 |
| S2 QA rerun (count rule) | 12 | 0.33 |
| total motion-* | 56 | 1.37 |

Per request at medium reasoning: timeline 0.022 $, QA 0.025 to 0.029 $ (1,800 to 4,600 output tokens; reasoning is 85 to 90 % of output). Projection for S1 to S4: timelines 30 windows x 0.022 = 0.66 $ per pass, QA 42 requests x 0.028 = 1.2 $. Two timeline passes plus one QA pass about 2.5 $, far below the 15 $ cap. Ledger total after the reruns: about 74.9 $.

## Segment QA findings (second run, not yet spot-checked)

| session | ok | partial | mismatch | first run mismatch |
|---|---|---|---|---|
| S1 (77 segments, 1 skipped) | 65 | 5 | 7 | 51 |
| S2 (96 segments) | 70 | 13 | 13 | 21 |

- The first run's S1 result was a bug in `motion_labels.frame_at`, not in the recording. It looked up frames by list position (`t * fps`), but extraction dropped the 277 frames of the bad segment `A_both_11000_11111` (170.7 to 189.3 s), so every frame after it was 18.5 s late. The "15 to 18 s ahead" reading in the first version of this file came from the same lookup. `frame_at` now searches by timestamp; a contact strip of D and E checked against `segments.json` shows the soda can in E_mug, the phone in E_phone, the drumstick in E_pen and the ball in E_ball. `segments.json` of S1 is correct.
- Count segments: in the first run most `B_*_count*` flags came from the counting animation, not from errors. With the count rule, B drops to 2 mismatches in S1 and 1 in S2.
- Remaining flags are mostly single A frames with a neighbouring finger state (thumb or pinky), which fits a hand still forming the pose. Real deviations to look at in S2: C_neutral (pursed lips), C_wink_L (both eyes squinted), C_turned_mouth (pursed instead of open and close), D_lean_sweep (turn instead of lean), H_swipe_right and H_swipe_left (direction or fist), H_pinch_drag.
- Training labels: extraction marks frames inside the margins of a count segment as `label_ok` with the target finger state (S1: 427 frames, S2: 354). While Noa counts up, the hand shows fewer fingers, so those labels are wrong for the first part of each count segment. S3 was recorded the same way. Not fixed here (label pipeline, `tools/labels/extract.py`).

## Timeline pilot (S2, 3 windows, 24 events)

G_free_hands 0-10 s: 10 events (throw, hold, push_pull, grab_release, swipe, orb, other) with sides and directions; G_free_face 0-10 s: 8 events, mostly face_head with concrete descriptions (O mouth, grin, head back); G_free_play 10-20 s: 6 events, all `other` or face_head (air keyboard, knob twisting, fist pumping), which fits the instruction. 2 fps limits timing to about +-0.5 s, so `motion.eval`'s 100 ms latency target cannot be judged from these labels, only event F1.

## Open

1. Noa spot-checks `data/motion_labels/2026-10-29-S2/review.html` and `2026-10-27-S1/review.html` (copy verdicts button); the pages show the second run.
2. Noa: real balance on the Baseten billing page (needs his login).
3. After S3 and S4 are recorded and SAM-labelled: timelines (two passes, agreement in `timeline.json`) on S1 to S4, QA on S3 and S4.
4. Count-segment finger labels in extraction: keep only the end of each count segment, or drop count frames from finger-state training.
