# Pong pointing shot on the recordings: result

Question (Noa, 11-04 night): the projectile should fire every time the
index is shown. Does `pong::shot::Point` do that on the recordings, and
how often does it fire when nobody points?

## Rule (written 2026-11-04 03:13 CET, before the run)

Data: the offline bench (`engine/bench.rs`, students backend, the shipped
models) over S2 and S3 whole, from 0 s to the end. S4 is held out: one run
after this rule and `shot.rs` are fixed, reported on its own, never tuned
on. S1 is not used. Segments marked bad are skipped.

Mask. The task assumed the bench gives the plain mask without a gate
(`geom::hand_features`: "without a gate (bench, tests) the plain mask at
0.5"). That holds for `hand_features` alone, but the students backend
(`perception.rs`, `Students::hand`) runs the hand's `ShapeGate` on
`geom::trigger_fingers` right after it, in the bench too. So the bench's
`shape` is the debounced mask the MIDI triggers see: band 0.1 around the
threshold, 60 ms dwell, gate reset on every frame without the hand, and
frames with `shape_settled == false` (the first 60 ms of a hand back in
view) do not reach `MASKS`. The thresholds are the default 0.5 per finger
(`FINGER_THR`), not a person's calibration.
- Primary (`gate`): one `shot::Point` per hand, fed each frame on which
  the hand is present and its shape settled, with the gate's mask and the
  frame's joints and capture time. This is what the Pong thread gets from
  `MASKS` and `SKELETON`.
- Variant (`plain`): a second `Point` per hand fed with the undebounced
  mask, each finger shown when its trigger value is above its threshold
  (no band, no dwell), on every frame with the hand.

Windows. A segment's window runs from the end of the segment before (at
most 1.0 s before its own start) to its own end, so the transition into a
pose, which happens in the gap and the leading margin, belongs to the pose.
Shots outside every window (long gaps) are listed, not scored.

Point poses per side: label `01000` (index alone) and `11000` (index with
the thumb; `shot::pointing` lets the thumb free).

(a) True shots. Segments: block A segments where a side's label is `01000`
(per session `A_R_01000`, `A_L_01000`, `A_both_01000_00000` (R)). Pass: at
least one shot from that side inside the window in >= 80 % of these
segments over S2 and S3 (6 segments, so >= 5). Reported as well, not
part of the pass: `11000` segments and the `B_*_count0`/`count8` segments
(index alone, 2 s).

(b) False shots. Every shot in a window counts as false unless its side's
label in that segment is a point pose. That covers block A and B poses of
the other masks, the other side of a two-hand segment, an absent side, the
B ramps, and every segment without hand labels (C face, D body, E objects,
F hard cases, H throws, swipes and pinch drags, G free hands and free
play). Minutes = the summed window length of these segments (a two-hand
segment with one pointing side counts once). Pass: false shots / minutes
<= 1.0 over S2 and S3. Each false shot listed with session, segment id,
side and time.

The same scoring for `plain`, and for S4 once.

## Run (2026-11-04 03:15 to 03:30 CET)

Bench: release build, `SYNTHEXIS_PERCEPTION=students`, whole sessions,
`SYNTHEXIS_BENCH_SHOTS=out/shots_S*.jsonl` (S2 85 s, S3 85 s, S4 55 s for
15 min of video each; about 3 ms per frame). Scored with
`python3 score.py --rec <data/recordings> out/shots_S2.jsonl out/shots_S3.jsonl`;
the full output with every false shot (direction, tip, seconds into the
segment) for both variants is in `out/score_dev.txt` and `out/score_S4.txt`.
`shot.rs` unchanged. A second bench run with more fields per shot line
(features, speed, trigger values, why a turn did not fire) gave identical
shot counts.

### S2 + S3 (dev), primary `gate`

| | result | pass |
|---|---|---|
| (a) index-alone segments with a shot | 5/6 = 83 % | PASS (>= 80 %) |
| (b) false shots | 334 in 29.3 min = 11.4/min | FAIL (<= 1/min) |

(a) per segment: S2 A_R_01000 +0.25 s, S2 A_L_01000 -0.16 s, S2
A_both_01000_00000 only at +3.53 s, S3 A_R_01000 -0.03 s, S3 A_L_01000
+0.17 s, S3 A_both_01000_00000 none. Info: 11000 segments 4/6, B count
index segments 7/8.

The S3 miss: the right hand was lost, came back already as 11000 (the
"back in view while pointing does not shoot" rule) and then switched
between 11000 and 01000. Both count as a point, so the index segment never
saw a turn into a point. S2's A_both shot at +3.53 s came from a flicker,
not the pose change.

(b) per block: A 10, B 33, D 16, E 38, F 37, G 24, H 176. Throws are the
bulk (8 s segments with 8 to 18 shots each). Not checked on video: a
fist opening fast probably passes through an index-first mask for longer
than the gate's 60 ms dwell; 213 of the 334 came straight from a fist mask.
B count segments: 2 s poses with label lag. Three shots fell between
segments (S2 L 629.3, R 638.9, R 639.3) and are not scored.

### Variant `plain` (no band, no dwell), S2 + S3

(a) 5/6 (same miss), 11000 6/6, B counts 8/8. (b) 799 false = 27.2/min,
FAIL. The gate removes 58 % of the plain mask's false shots and loses no
index segment.

### S4 (held out, run once with the rule above)

| | gate | plain |
|---|---|---|
| (a) index-alone segments | 1/3 = 33 %, FAIL | 2/3 = 67 %, FAIL |
| (b) false shots, 9.5 min | 136 = 14.3/min, FAIL | 360 = 37.9/min, FAIL |

gate (b) per block: A 7, C 27, E 20, F 10, G 22, H 50; 22 shots between
segments, not scored. Both gate misses come from the lost rule: A_R_01000
(hand gone after the fist at 9.5 s, back pointing at 10.63 s, proj 1.14)
and A_both_01000_00000 (R back as 11000 at 123.07 s, then no turn).

### Why it fails

1. A finger mask turning into a point is not a shot by itself: fast hand
   motion (throws, circles, fast moves, objects in the hand) produces
   point masks for 0.1 to 0.3 s. Of the 334 dev false shots the point
   held under 0.3 s in 191 and under 0.5 s in 272. The true shots held
   for seconds (20 of 23 at least 1 s).
2. The lost rule costs true shots whenever the hand drops out of view
   between poses, which happens in every session (S3 once, S4 twice).

### What to change (suggestions, measured on S2 + S3; not in shot.rs)

Shot-time numbers, dev true shots vs false shots (p10/p50/p90): hand speed
0.08/0.19/0.45 vs 0.13/0.33/0.67; index trigger value 0.90/0.98/1.00 vs
0.58/0.89/1.00; max of middle, ring, pinky 0.00/0.04/0.14 (middle) vs
0.01/0.10/0.38. Geometry (straight, reach, proj) overlaps too much to help.

Filters on the existing gate shots, dev (6 index segments, 29.3 min):

| filter at the shot frame | index segments | false/min |
|---|---|---|
| none | 5/6 | 11.4 |
| hold the point >= 300 ms | 5/6 | 4.9 |
| hold >= 500 ms | 5/6 | 2.1 |
| speed <= 0.25 | 5/6 | 3.9 |
| speed <= 0.25, index value >= 0.9, middle/ring/pinky <= 0.3 | 5/6 | 2.0 |
| the same + hold >= 300 ms | 5/6 | 1.06 |

The last two applied once to S4, after choosing them on dev: 1.4/min
and 0.5/min false, still 1/3 index segments (the lost rule).

Proposal, in order of effect:
1. Fire on a point that holds: the mask has to stay a point for 150 to
   300 ms (capture time) and the hand has to be slow (speed <= 0.25)
   during it; spawn the projectile at the start of the hold with the
   hold's latency added like the flick's confirmation frame. Costs 0.15
   to 0.3 s per shot.
2. Stricter values for the shot than for MIDI: index >= 0.9, middle,
   ring and pinky <= 0.3 (the gate's band is 0.6 and 0.4).
3. Replace the lost rule by the hold: a hand that comes back pointing
   shoots once its point held. On dev, lost turns that pass the filters
   of (2) add 2 false shots and no true one; on S4 they add the missed
   A_R_01000.
4. Count 11000 to 01000 as no new shot, as now; the protocol's gun
   segment before A_both_01000_00000 is why that segment misses twice.
   Noa's clip with deliberate points (design 10) decides whether the gun
   should count as a point at all.

Even with (1) and (2) the dev false rate stays at about 1/min, mostly in
B count lags, F_fast_moves and throws. A new recording with labelled
deliberate points is the honest test; these recordings have no pointing
intent, so part of the "false" shots in G and H are real points.

### False shots, `gate` (side and time in s)

S2 + S3:
- S2 A_R_11100: R 37.2
- S2 A_L_01100: L 113.8, L 117.7
- S2 A_L_11100: L 121.0
- S2 A_L_10001: L 155.2
- S2 B_R_open: L 211.9, R 214.2, R 226.6
- S2 B_R_count1: R 250.1
- S2 B_R_count2: R 253.1
- S2 B_R_count3: R 256.1
- S2 B_R_count4: R 259.5
- S2 B_R_count5: R 262.1
- S2 B_R_count6: R 265.2
- S2 B_R_count7: R 268.2
- S2 B_L_open: L 284.8
- S2 B_L_count1: L 311.7
- S2 B_L_count2: L 314.4
- S2 B_L_count3: L 317.7
- S2 B_L_count4: L 320.8
- S2 B_L_count5: L 323.4
- S2 B_L_count7: L 329.5
- S2 D_lean_sweep: R 462.5
- S2 D_step_back: R 475.6, R 477.4, L 484.1, R 484.4, L 484.4, R 485.9, L 486.0
- S2 E_mug: R 491.0, R 491.8, R 493.4, R 496.7, R 502.1, R 507.9, R 508.2
- S2 E_phone: R 509.1, L 520.9
- S2 E_pen: L 533.5, L 534.7
- S2 E_ball: R 557.3
- S2 E_demo_object: R 578.2, L 582.0, R 592.6
- S2 F_hand_over_face: R 593.1, R 601.5
- S2 F_fast_moves: R 611.3, L 612.5, L 613.9, R 614.3, L 615.2, R 615.4, L 616.3, R 616.3, R 617.0, L 617.0, L 618.4, L 619.5, R 619.5
- S2 F_hand_edge: R 621.3
- S2 F_dim: R 655.3, L 657.0, R 657.2
- S2 H_throw_top: R 660.2, R 661.0, R 661.7, R 662.3, R 662.9, R 663.9, R 664.8, R 665.4
- S2 H_throw_right: R 667.1, R 670.0, R 670.5, R 671.7, R 672.2, R 672.9, R 673.6, R 674.8, R 675.3
- S2 H_throw_bottom: R 677.9, R 680.5, R 681.2, R 681.9, R 682.4, R 682.7, R 683.4, R 683.9, L 684.3
- S2 H_throw_left: R 685.4, L 687.7, R 687.8, L 688.3, L 688.9, L 689.5, L 690.0, L 690.5, L 690.9, L 691.4, R 691.8, R 693.9
- S2 H_throw_any_L: R 694.2, L 696.4, L 696.9, L 697.5, L 698.6, L 698.9, L 699.4, L 700.3, L 700.7, L 701.2, L 701.7, L 702.1, L 702.8
- S2 H_swipe_right: R 709.8, R 710.1, R 711.7
- S2 H_swipe_left: R 712.4, R 720.3
- S2 H_swipe_up: R 724.6, R 725.4
- S2 H_circle_cw: R 741.7, R 742.5, R 743.3, R 744.4, R 745.2, R 746.6, R 747.8
- S2 H_circle_ccw: R 748.7, R 750.0, R 751.4, R 752.0, R 752.9, R 754.2, R 755.3, R 755.7, R 756.3
- S2 H_push_pull: R 757.5
- S2 H_pinch_drag: R 773.9
- S2 H_grab_release: R 777.5, R 779.0, R 779.9, R 781.0, R 781.7, R 783.0, R 783.7
- S2 H_orb_hold: R 784.7, R 791.1
- S2 H_orb_throw: L 796.4, L 798.5, L 801.7
- S2 H_stretch: L 808.3, L 810.2
- S2 H_clap: R 811.7
- S2 G_free_hands: R 823.0, R 823.6, L 840.8, R 841.6, L 842.0, R 844.8, L 849.6
- S2 G_free_play: R 879.5, L 896.6
- S3 A_R_01100: R 29.1
- S3 A_R_11100: R 36.5
- S3 A_R_01001: R 79.9, R 80.6, R 81.3
- S3 B_R_count1: R 249.6
- S3 B_R_count2: R 252.7
- S3 B_R_count3: R 255.6
- S3 B_R_count4: R 259.1
- S3 B_R_count5: R 262.0
- S3 B_R_count6: R 264.9
- S3 B_R_count7: R 268.4
- S3 B_L_open: R 272.4
- S3 B_L_count1: L 311.0
- S3 B_L_count2: L 313.9
- S3 B_L_count3: L 316.9
- S3 B_L_count4: L 320.3
- S3 B_L_count5: L 323.1
- S3 B_L_count6: L 325.9
- S3 B_L_count7: L 328.3, L 328.9
- S3 D_arms_sweep: L 458.9
- S3 D_step_back: L 477.8, L 478.6, R 478.6, L 478.9, L 479.5, R 479.6, R 480.2
- S3 E_mug: L 490.7, L 491.3, L 492.5, R 492.9, L 493.4, L 501.0
- S3 E_pen: L 531.9, L 535.1, R 537.4, L 537.5, L 538.3
- S3 E_ball: R 560.3, R 560.8, R 561.5, R 569.9, L 571.0
- S3 E_demo_object: L 576.3, L 581.3, L 582.9, R 583.5, L 589.5, L 590.8, L 591.2
- S3 F_hand_over_face: L 592.5, R 592.8
- S3 F_fast_moves: R 613.4, L 613.5, L 613.8, R 614.0, R 614.9, L 615.3, L 616.1, R 616.1, R 617.8, L 618.5, R 618.5, L 619.0
- S3 F_backlit: L 636.8, L 637.3
- S3 F_dim: R 638.5, R 646.4
- S3 H_throw_top: R 646.7, L 646.9, L 652.2, L 652.6, L 653.7, L 654.1, L 654.7, L 655.3
- S3 H_throw_right: L 655.9, L 656.2, L 658.1, L 659.1, L 660.0, L 660.5, L 660.8, L 661.3, L 661.8, L 662.1, R 662.9, R 663.3, R 663.7, R 664.1, R 664.5
- S3 H_throw_bottom: R 667.1, L 667.4, R 667.8, L 668.2, R 668.2, L 669.3, R 669.6, L 669.8, R 670.4, R 670.8, L 671.1, R 671.4, R 671.9, L 671.9, R 672.6, L 672.9, R 673.1, R 673.4
- S3 H_throw_left: R 673.8, L 675.5, R 675.5, R 676.8, R 677.2, L 677.9, R 678.1, R 679.8, R 680.3, L 680.9
- S3 H_throw_any_L: L 686.1, L 686.9, L 689.4, L 690.1, L 690.8, L 691.2
- S3 H_circle_cw: R 730.4, R 730.9, R 731.4, R 735.0, R 735.6
- S3 H_circle_ccw: R 737.2, R 739.1, R 739.8, R 740.6, R 741.2, R 741.8, R 742.5, R 743.6, R 744.5, L 744.8, R 745.3
- S3 H_pinch_drag: R 756.2, R 761.0, R 762.9
- S3 H_grab_release: R 765.7, R 766.7, R 768.2, R 769.4, R 770.1, R 771.8, R 772.6
- S3 H_orb_hold: R 773.4, R 775.4
- S3 G_free_hands: L 811.5, L 812.1, L 814.5, R 817.9, R 818.4, R 820.2, L 823.2, L 823.5, L 827.2, L 827.6, L 829.0, L 832.4, L 834.3
- S3 G_free_play: R 866.7, L 880.0

S4:
- S4 A_R_01001: R 59.1
- S4 A_L_00001: L 107.1
- S4 A_L_10001: L 114.2
- S4 A_both_11000_11111: L 121.6
- S4 A_both_10001_01001: L 149.0, L 150.2, L 150.5
- S4 C_neutral: R 157.0
- S4 C_mouth_sweep: R 160.0, R 160.8, R 161.7, R 163.0, R 165.9, R 171.3
- S4 C_mouth_wide: R 171.6, L 174.3, L 176.3, L 177.0, L 177.9
- S4 C_wink_L: L 188.5, L 189.1, L 189.6, L 190.3, L 191.2
- S4 C_wink_R: L 193.2, R 195.1, R 196.0, R 197.7, R 198.3
- S4 C_eyes_closed: R 205.8
- S4 C_smile: R 211.9, R 212.7
- S4 C_yaw_sweep: R 213.7, R 214.3
- S4 E_mug: R 268.4, R 270.0, R 270.7, L 277.4, R 282.2, R 282.6, R 284.5
- S4 E_phone: L 288.8, R 299.0, R 300.4, L 305.2
- S4 E_pen: L 308.0, R 311.3, R 312.1, R 313.5, L 314.9, L 316.9, R 323.5, R 324.7, R 328.0
- S4 F_hand_over_face: R 328.9, L 329.7
- S4 F_fast_moves: R 347.8, L 348.6
- S4 F_hand_edge: L 357.7, R 358.1, L 363.2
- S4 F_backlit: R 380.7, L 380.7
- S4 F_dim: L 387.3
- S4 H_throw_top: R 400.2, R 400.9, R 401.6, R 402.4, R 403.1, R 403.6, R 404.1, R 404.6
- S4 H_throw_right: R 405.1, R 406.7, R 407.4, R 407.8, R 408.4, R 408.9, R 409.4, R 410.5, R 411.0, R 411.5
- S4 H_throw_bottom: R 412.1, L 412.5, R 412.6, R 413.8, L 415.1, L 415.7, L 416.2, L 417.0, L 417.9, L 419.0
- S4 H_throw_left: R 421.0, R 421.5, R 422.1, R 423.3, R 424.3, R 425.5
- S4 H_throw_any_L: R 426.2, R 426.6, L 428.2, L 431.5, L 432.4
- S4 H_swipe_right: L 433.7
- S4 H_swipe_left: R 446.4
- S4 H_pinch_drag: R 484.8, R 485.7
- S4 H_grab_release: R 490.2, R 491.5, R 492.5, R 494.6, R 496.3
- S4 H_stretch: R 515.8
- S4 H_clap: L 517.8
- S4 G_free_hands: L 528.0, L 529.3, L 533.1, L 535.8, L 536.4, R 540.1, L 543.6, L 545.0, R 546.4, L 546.6, R 547.4, R 547.8, R 552.0, L 554.2
- S4 G_free_play: R 555.6, L 555.7, L 560.0, L 561.3, L 562.5, L 563.1, L 577.9, R 581.4

## Follow-up (11-04 night): filters shipped

Chosen on S2 and S3 from the shot lines above (vals and speed of the
frame a shot fired on; `scratchpad` analysis with `score.py`'s windows),
gate masks:

| Variant | index-alone segments | false shots per minute |
|---|---|---|
| none (as measured above) | 5/6 | 11.38 |
| index value >= 0.9, middle/ring/pinky <= 0.3 | 5/6 | 4.67 |
| + hand speed <= 0.25 | 5/6 | 1.98 |
| + pose held 150 ms | 5/6 | 1.74 |
| + pose held 200 ms | 5/6 | 1.43 |
| + pose held 300 ms | 5/6 | 1.16 |

Shipped: the first two filters without a hold (`shot::POINT_INDEX`,
`POINT_REST`, `POINT_SPEED`; `engine::HAND_SPEED` carries the speed of
each frame). A hold buys 0.2 to 0.8 false shots per minute for 150 to 300
ms of latency on every shot; not worth it. S4 with the same filter (from
the measure agent's run): 1.4 per minute. The rule (b) of at most 1 per
minute still fails on paper, but the recordings have no labels for
deliberate pointing, and the remaining shots sit in free play, throws and
object handling where Noa may well point. Next check: Noa's live test, and
a clip with deliberate points, pinch releases and fist openings.

## Hold (2026-11-04 afternoon)

Noa in FL: a flick shoots well, pointing slowly does not. Cause: `Point`
checked the clear-and-still filter only on the one frame where the mask
turned into a point; a slow point reaches index >= 0.9 and a still hand a
few frames later and never got another chance. A flick is straight at
once, so it passed. New rule in `shot.rs`: each point (an unbroken run of
point masks, also one starting when the hand comes back pointing) shoots
once, on its first frame that is at least HOLD_NS into the run, clear and
still (POINT_INDEX, POINT_REST, POINT_SPEED unchanged) and has a
direction. The flick choice is gone from the window.

### Rule (written 2026-11-04 14:17 CET, before the run)

Variants: HOLD_NS = 0, 100, 200, 300 ms. Data, scoring, windows and
segments exactly as in the first rule (`gate` primary; S2 + S3 dev; S4
held out, run once with the chosen value only). The bench runs on a copy
of the source taken at 14:17 (another session is changing perception in
the working tree), with the hold read from an env var in the copy only.

A variant passes on dev when (a) at least 5 of 6 index-alone segments
shoot and (b) false shots are at most 2.0 per minute (the shipped rate;
the new rule must not fire more often by accident). Among passing
variants take the smallest hold (Noa wants every point to shoot, latency
counts), unless a longer hold up to 200 ms cuts false shots by at least
40 % with the same (a); then that one. If none passes, take the variant
with the best (a) and report the false rate as failed. S4 is reported
for the chosen value: pass when at least 2 of 3 index segments shoot and
false shots stay at most 2.0 per minute.

### Run (14:19 to 14:35 CET)

Copy built at 14:19, release, same bench and scoring. Full output in
`out/hold/score_h*.txt`.

| hold | index-alone (a) | 11000 | B counts | false/min (b) | pass |
|---|---|---|---|---|---|
| shipped (edge only) | 5/6 | 4/6 | 7/8 | 2.0 | |
| 0 ms | 5/6 | 6/6 | 8/8 | 4.16 | no |
| 100 ms | 5/6 | 6/6 | 8/8 | 2.73 | no |
| 200 ms | 5/6 | 6/6 | 8/8 | 2.25 | no |
| 300 ms | 5/6 | 6/6 | 8/8 | 1.47 | yes |

Only 300 ms passes, so `HOLD_NS` = 300 ms. False shots per block at
300 ms: B 17 (count poses with label lag), H 17 (throws), F 5, E 2, D 1,
G 1. The miss is the same as before: S2/S3 A_both_01000_00000 fires only
3.8 s in, after the gun pose before it.

S4 held out, 300 ms, run once: index-alone 2/3 (shipped 1/3; A_R_01000
now shoots after the hand came back pointing), false 12 in 9.5 min =
1.26/min. Passes the S4 rule (>= 2/3, <= 2.0/min). score.py still prints
FAIL against the first rule's thresholds (80 %, 1/min).

Cost: a point shoots 0.3 s after the mask shows it. The projectile starts
at the index tip of that frame.

### Noa's choice (11-04 evening, FL)

300 ms felt too slow in FL; Noa set the hold to 100 ms and accepts the
higher false rate (2.7 per minute on dev instead of 1.5). `HOLD_NS` =
100 ms.
