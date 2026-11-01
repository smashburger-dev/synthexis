# RESULT: motion.labels, Spark Open freestyle timelines and segment QA

Status 2026-11-02: done for S1 to S4. Every session has a two-pass segment QA and two-pass freestyle timelines. S3 and S4 were extracted and SAM-labelled in image mode for this. Spend on Baseten for `motion-*` runs: 5.63 $ of the 15 $ cap. Noa's spot-check of the review pages is still open.

Tool: `uv run tools/labels/motion_labels.py <session> [--max-usd 15] [--passes 1|2] [--only qa|timeline] [--limit-jobs N] [--dry-run]`, review pages via `uv run tools/labels/motion_labels.py report <session>...`. Outputs under `data/motion_labels/<session>/` (not committed, faces): `timeline.json`, `qa.json`, `review.html`, `frames/` (the exact images sent). Runs need `SYNTHEXIS_BASETEN_CAP` above the ledger total (default 60), here 95.

## Design, as built

- Timeline: each freestyle segment (block G) in 10 s windows, frames at 2 fps, 768x432, SAM overlay (hand green fill, face blue outline so expressions stay visible, object orange), yellow trail of hand-box centres over the previous 0.5 s from the 15 fps SAM labels, white ring on the current hand centres. Reply: events with `t_start`, `t_end`, `hand` (anatomical R/L/both/none), `primitive` (motion.primitives vocabulary split into throw, swipe, push_pull, hold, pinch, circle, grab_release, stretch, orb, clap, plus face_head and other), `params`, free-text `name`, `confidence`. Times are seconds since segment start; `seg_start_s` keeps the absolute session time. Directions in timeline events are in image coordinates of the unmirrored video.
- QA: 3 frames per scripted segment at 20/50/80 % of the span inside the protocol margins, 8 segments per request. Reply per segment: match yes/no/unsure, a verdict per frame, issue text. Flags: `mismatch` (2 or 3 frames wrong in a pass that says no), `partial` (1 frame wrong), `unsure`, `ok`. With two passes a segment is flagged when either pass says no; `passes_agree` marks agreement.
- Count segments (`B_*_count*`): Noa acts out counting from one finger up to the target in every count segment, also on the way down. QA takes these frames at 35/70/98 % of the raw span and accepts fewer fingers on the way up (order index, middle, ring, pinky, thumb).
- Mirroring: recordings are unmirrored, the recorder preview is mirrored. Instructions in block H ("rechter Bildrand", "von links nach rechts", "im Uhrzeigersinn") mean what Noa saw in the preview, so left/right and cw/ccw are flipped in the frames. The QA prompt says so. Hand sides (hand_R, hand_L) are anatomical.
- Segments whose frames extraction dropped as bad are skipped (S1 `A_both_11000_11111`, S3 `E_phone`).
- Budget: `--max-usd` caps the sum of every `motion-*` run in the ledger, so reruns and sessions share one 15 $ cap; `spark.run` adds the up-front estimate, pilot re-estimate and rolling stop.

## Measured on Baseten (Muse Spark Open)

- At most 8 images per request ("Too many images: 10 (max 8)", HTTP 400, no charge). The budget probe's 20 images per request does not work.
- Tiling costs the same as single frames: a 2x2 grid of 768x432 frames (1536x864) bills about 1,790 prompt tokens, a 1x3 row (2304x432) about 1,300; no internal downscale, and Spark read every burned-in frame label correctly. A timeline window is 5 grids (20 frames), a QA request 8 rows (24 frames).
- `spark._estimate_usd` counted base64 characters as text (about 40x too high for images); it now counts each image part as `IMAGE_TOKENS_EST` tokens (450 default, 1,750 set by motion_labels for the grids). Test added.
- Per request at medium reasoning: timeline about 0.02 $, QA 0.025 to 0.03 $. Reasoning is 85 to 90 % of output.

## Cost

| item | $ |
|---|---|
| Baseten `motion-*`: limit probe, S2 pilot, first QA runs, reruns after the fixes, final two-pass QA and timelines on S1 to S4 | 5.63 |
| Modal SAM image mode, S3 (13,089 frames, 44 chunks, estimate 1.76 $) | 1.22 |
| Modal SAM image mode, S4 (8,801 frames, 29 chunks, estimate 1.17 $) | 0.78 |

Baseten ledger total after this work: 79.20 $ of the 100 $ credit. The billing page needs Noa's login; the 2026-10-27 image test ran outside the ledger.

## Segment QA (final, two passes)

| session | segments | ok | partial | mismatch | both passes mismatch | passes agree |
|---|---|---|---|---|---|---|
| S1 | 77 | 59 | 7 | 11 | 4 | 66 |
| S2 | 96 | 65 | 16 | 15 | 9 | 84 |
| S3 | 95 | 70 | 9 | 16 | 10 | 82 |
| S4 (short) | 69 | 53 | 7 | 9 | 3 | 58 |

Mismatches both passes agree on:
- S1: A_R_01110, A_R_01111, A_L_01110, A_both_01000_00000
- S2: A_R_01001, A_L_00001, A_both_01000_00000, A_both_10001_01001, C_neutral, C_blink, H_throw_left, H_swipe_left, H_swipe_up
- S3: A_R_01110, A_R_00001, A_L_00001, C_wink_L, C_wink_R, D_step_back, F_backlit, F_dim, H_throw_bottom, H_throw_left
- S4: A_R_00001, C_wink_L, H_grab_release

Reading:
- Most A flags sit on single finger states that recur across sessions (pinky only `00001`, `01110` against `01111`). Either the hand rotation in A (the instruction asks to turn the wrist) hides the finger, or Spark confuses pinky and index. One S4 frame checked by eye was not decidable at 768x432 either.
- Winks (C_wink_L, C_wink_R) come out as both eyes closed in S2 to S4. If that holds on a look, the eye labels of those segments should not train a one-eye-closed class.
- History of fixes: the first S1 QA flagged 51 of 78 segments. That was a bug in `motion_labels.frame_at`, which looked up frames by list position while extraction had dropped the 277 frames of S1's bad segment, so every later frame was 18.5 s late. `frame_at` now searches by timestamp. The count rule took most B flags away, the mirror rule most H swipe flags.

## Timelines (two passes)

| session | windows | events pass a | matched in pass b | precision a vs b | recall a vs b |
|---|---|---|---|---|---|
| S1 | 8 | 63 | 44 of 57 | 0.70 | 0.77 |
| S2 | 8 | 52 | 34 of 47 | 0.65 | 0.72 |
| S3 | 8 | 60 | 39 of 52 | 0.65 | 0.75 |
| S4 (short) | 6 | 24 | 16 of 30 | 0.67 | 0.53 |

A match needs the same primitive and overlapping times (0.5 s slack). Two readings of the same frames agree on about 70 % of events, so treat single-pass events as candidates and the matched ones as the firmer set. 2 fps limits timing to about +-0.5 s: `motion.eval`'s 100 ms latency target cannot be judged from these labels, only event F1.

## Changes to the label pipeline from this work

- `tools/labels/extract.py`: count segments get `label_ok = false` (their target finger state holds only at the end). Rerunning extract on an extracted session re-labels the index without new jpegs. S1 label_ok 8,811 -> 8,384, S2 10,598 -> 10,244. A training run reading `frames_index.json` sees the change; `manifest.json` of S1 and S2 still states the old `frames_label_ok`.
- `tools/labels/objects.json`: S3 demo object is a spray can (checked on the E frames). S3 and S4 otherwise use the defaults (soda can, phone, drumstick, cork ball).

## Hand-over for training

- Block H motion labels (`throw_right`, `swipe_left`, `circle_cw`, ...) are in the user's view, like `hand.x = 1` in `tools/mapping/ontology.yaml`. On unmirrored frames, x must be flipped (and cw/ccw swapped) before these labels and SAM hand positions line up.
- S4 is the held-out set (`split=heldout` in its manifest), short protocol, FaceTime camera, other shirt and cap. S3 is iPhone via Continuity from the side, studio light.

## Open

1. Noa spot-checks the review pages (`data/motion_labels/<session>/review.html`, copy verdicts button), at least the flags both passes agree on and the winks.
2. Noa: did the S3 B_L count segments use the left hand? One QA run read them as the right hand; the camera angle in S3 makes image position unreliable.
