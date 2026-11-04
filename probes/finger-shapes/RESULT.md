# Finger shapes for MIDI triggers: result

Question (Noa, FL test 2026-11-04 00:50): every single finger and every
combination of fingers has to trigger its part, as reliably as possible. In
the first FL test the middle finger alone played the index finger's part.

## Why it failed

Block A of every recording (S1 to S4) holds the same twelve masks per hand
(fist, thumb, point, L, peace, 11100, 01110, four, open, pinky, shaka,
rock): no middle finger alone, no ring finger alone. The student's finger
head learned those combinations, so a pose it never saw lands on the
nearest one it knows (point). The 1.0 trigger also counted a finger as
shown whatever the others did, so peace fired the index and the middle
part together.

## Rule (written 2026-11-04 01:00 CET, before the run)

Data: bench dumps of block A (students backend, the shipped hand model),
S1 to S3, the packer's dev segments of block A (13 + 17 + 17 dev segments
in all blocks; those of block A only), frames inside each segment after its
leading margin (0.7 s) and before its trailing one (0.3 s), on the labelled
side with a hand present. A segment marked bad is skipped.

Candidates, each finger extended when its value is above 0.5:
- C1 the student's finger head (the input of `ShapeGate` today)
- C2 the geometric `finger.<f>` (bend and reach, linear since merge 50)
- C3 C2 for index to pinky, C1 for the thumb
- C4 the mean of C1 and C2 per finger

Metrics: exact-mask accuracy (all five bits right) per labelled hand frame,
the primary; accuracy per finger; mask changes per second within held
segments (flicker).

Decision: the highest exact-mask accuracy on the dev frames; within 0.005,
the fewer mask changes per second. The winner, and only the winner, is
scored once on S4 block A (held out from both the student and the anchors).

Not measurable offline: the masks no recording has (middle alone, ring
alone, 00110 and the other unseen ones). Live check with Noa after the
build: thumb, index, middle, ring and pinky alone, then each "all but one",
3 s each; pass when the window's live finger display shows the right
fingers in at least 9 of the 10 poses.

## Results (2026-11-04 01:05 CET)

Dumps: `score.py` on the release bench (students backend, build 4739f19 +
MIDI), `ba_S1..S4.jsonl`; the bench line now carries `hand_<side>_ext` (the
student's probabilities) next to the feature values.

Dev frames of block A, S1 to S3 (2,662 labelled hand frames):

| candidate | exact mask | thumb | index | middle | ring | pinky | mask changes / s |
|---|---:|---:|---:|---:|---:|---:|---:|
| C1 student head | 0.877 | 0.937 | 0.970 | 0.990 | 0.988 | 0.957 | 2.21 |
| C2 geometry | 0.834 | 0.902 | 0.975 | 0.991 | 0.989 | 0.947 | 2.50 |
| C3 geometry, student thumb | 0.867 | 0.937 | 0.975 | 0.991 | 0.989 | 0.947 | 2.04 |
| C4 mean of both | **0.880** | 0.941 | 0.971 | 0.994 | 0.991 | 0.955 | **1.90** |

C4 and C1 are within 0.005, so the tie-breaker decides: C4 flickers less.
Weakest masks for C4: 01110 0.64, 11100 0.70, 11000 0.72, 01001 0.77.

S4, scored once for C4 (3,218 frames): exact 0.895, per finger 0.934 /
0.985 / 0.992 / 0.983 / 0.960, 1.34 changes per second. Weakest masks
11100 0.72, 01110 0.75, 01001 0.78, 01000 0.83.

Decision: C4 ships (`geom::trigger_fingers`). The thumb stays the weakest
finger in every candidate. On top: the hysteresis band (threshold +- 0.1),
60 ms dwell, a per-person calibration (`calib.rs`: fist, open, each finger
alone; each finger's threshold halfway between its folded and shown
medians) and the live finger display in the window. The live check of the
unseen poses with Noa is still open.

## Live check, 2026-11-04 (Noa's calibration log)

The live check of the unseen poses came back through Noa's first finger
calibration in FL (synthexis.log, 1793797094). Pose medians of the
student's finger head and of the geometric value, index column:

| pose | hand | student index | geometry index | student middle | geometry middle |
|---|---|---:|---:|---:|---:|
| middle alone | L | 0.98 | 0.93 | 0.05 | 0.10 |
| middle alone | R | 0.96 | 1.00 | 0.01 | 0.03 |
| ring alone | L | 0.96 | 0.88 | 0.00 | 0.14 |
| ring alone | R | 0.70 | 0.99 | 0.00 | 0.10 |

Both sources read middle alone and ring alone as the index on both hands.
The geometric value is no independent second opinion: it is computed from
the same student's keypoints, which put the index chain on the raised
finger. Apple Vision's hand pose, run on the same frames, read the index
at 0.81 and 0.94 and the middle at 0.64 and 0.27 in the middle pose.

Why: the recordings hold 910 s of labelled hand poses in 12 masks, with
0 s of middle shown and index folded and 0 s of ring shown without index
and middle. The nearest single-finger pose the student knows is the point
(96 s). Fix: record the 20 missing masks and retrain the hand student;
no threshold reaches it. The calibration now reports such poses
(`calib::Outcome::misread`) and logs whether the student's skeletons of
the two poses still differ (split half), which says whether a per-person
skeleton template could separate them before any retraining.
