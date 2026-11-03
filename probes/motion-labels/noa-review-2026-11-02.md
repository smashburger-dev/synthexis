# Noa's spot-check of the S3 review page (2026-11-02)

Noa marked the S3 review page (`data/motion_labels/2026-11-01-S3/review.html`, two-pass QA and timelines from commit a5d251d). For QA rows, "right" means Spark's verdict was right and "wrong" means Spark's verdict was wrong.

## Segment QA

| Spark said | Noa: Spark right | Noa: Spark wrong |
|---|---|---|
| mismatch (16) or partial (8 of 9 judged) | 3: D_step_back, F_backlit, F_dim (F_dim only partly: the light went off midway, so frame 3 is the dim one) | 21: A_R_01100, A_R_01110, A_R_00001, A_R_11000, A_R_01111, A_L_11100, A_L_00001, A_L_01110, A_L_01111, A_both_10001_01001, B_L_open, B_L_spread, B_R_count1, B_R_count2, B_R_count4, C_wink_L, C_wink_R, H_throw_top, H_throw_bottom, H_throw_left, H_clap |
| ok (10 sampled) | 10: A_R_10001, B_L_count6, E_ball, E_demo_object, E_pen, F_hands_overlap, H_circle_cw, H_orb_throw, H_pinch_drag, H_swipe_right | 0 |

Reading: on S3, Spark's flags were right 3 times in 24; its "ok" was right 10 of 10. The flags are false alarms in the large majority, so they must not remove script labels from training. Noa's reasons for the false flags:
- Finger states: he showed them; Spark missed the thumb (A_R_11000, A_R_01111) or a finger, or called a hand "wrong" when tracking dropped for a frame.
- Hand side: Spark called the wrong anatomical hand. S3 films from 20 to 30 degrees to the side, so both hands sit left of the face in the image. The recording is not mirrored (checked by the claude session on the thumb side and the arm in A_R/A_L of all four sessions).
- Throws: one frame per throw is too few; "other hand moved" is wrong, he threw with one hand.

## Freestyle timelines, Noa's description

S3 G_free_hands (30 s), what he did:
- 0 to 1 s: three fingers up with thumb and index tips joined (OK sign)
- 1 to 1.75 s: pinches something
- 2 to 3 s: turns a knob
- 3.75 to 5 s: shoots a projectile out
- then catches something, turns it and pulls it towards himself
- 10 to 14 s: turns a knob
- from 14 s: pinches something and drags it across the image
- from about 20 s: holds an orb or a tube and throws it at the end

S3 G_free_face: recognised well (most events right).

S3 G_free_play: he played piano (Spark said DJ mixer) and air drums; from 16 s air guitar, from 23 s something like a violin.

Noa: the freestyle segments are nearly the same in all four sessions. He judged most free_hands events wrong or partly right: Spark forced his movements onto the old primitive list.

## What changed after this review

- Timeline vocabulary: new primitives `pinch_drag`, `knob_turn`, `catch_pull`, `finger_sign` (ok_sign, counts), `air_instrument` (piano, drums, guitar, violin, dj_deck), `orb` also for a tube; freestyle may use `other` with a precise name instead of forcing a primitive; the prompt lists the actions Noa typically performs.
- QA: 6 frames for block H; a lost or blurred hand is "unclear", not "wrong"; thumb warning for finger states; the hand side comes from the arm and thumb, not the image position; lights may switch off midway.
- Training: the packer drops the Spark exclusion list; only S3 D_step_back and F_backlit (and the pre-dark part of S3 F_dim) lose their labels.

## Raw verdicts

```json
{"tl-G_free_hands-w0/e0":"wrong","tl-G_free_hands-w0/e1":"wrong","tl-G_free_hands-w0/e2":"wrong","tl-G_free_hands-w0/e3":"partly","tl-G_free_hands-w0/e4":"partly","tl-G_free_hands-w0/e6":"partly","tl-G_free_hands-w0/e7":"wrong","tl-G_free_hands-w0/e8":"partly","tl-G_free_hands-w0/missed":"many","tl-G_free_hands-w1/e0":"partly","tl-G_free_hands-w1/e1":"right","tl-G_free_hands-w1/e2":"wrong","tl-G_free_hands-w1/e3":"wrong","tl-G_free_hands-w1/e4":"wrong","tl-G_free_hands-w1/e5":"right","tl-G_free_hands-w1/e6":"partly","tl-G_free_hands-w1/e7":"right","tl-G_free_hands-w1/e8":"partly","tl-G_free_hands-w1/e9":"right","tl-G_free_hands-w1/missed":"some","tl-G_free_hands-w2/e0":"partly","tl-G_free_hands-w2/e1":"wrong","tl-G_free_hands-w2/e2":"wrong","tl-G_free_hands-w2/e3":"partly","tl-G_free_hands-w2/e4":"partly","tl-G_free_hands-w2/e5":"wrong","tl-G_free_hands-w2/e6":"right","tl-G_free_hands-w2/e7":"wrong","tl-G_free_hands-w2/e8":"partly","tl-G_free_hands-w2/missed":"some","tl-G_free_face-w0/e0":"right","tl-G_free_face-w0/e1":"right","tl-G_free_face-w0/e2":"right","tl-G_free_face-w0/e3":"partly","tl-G_free_face-w0/e4":"right","tl-G_free_face-w0/e5":"right","tl-G_free_face-w0/missed":"none","tl-G_free_face-w1/e0":"right","tl-G_free_face-w1/e1":"right","tl-G_free_face-w1/e2":"right","tl-G_free_face-w1/e3":"right","tl-G_free_face-w1/e4":"right","tl-G_free_face-w1/e5":"partly","tl-G_free_face-w1/missed":"none","tl-G_free_play-w0/e0":"wrong","tl-G_free_play-w0/e1":"right","tl-G_free_play-w0/e2":"right","tl-G_free_play-w0/missed":"none","tl-G_free_play-w1/e0":"right","tl-G_free_play-w1/e1":"wrong","tl-G_free_play-w1/e2":"wrong","tl-G_free_play-w1/e3":"wrong","tl-G_free_play-w1/e4":"right","tl-G_free_play-w1/missed":"some","tl-G_free_play-w2/e0":"wrong","tl-G_free_play-w2/e1":"partly","tl-G_free_play-w2/e2":"right","tl-G_free_play-w2/e3":"partly","tl-G_free_play-w2/e4":"partly","tl-G_free_play-w2/e5":"partly","tl-G_free_play-w2/e6":"partly","tl-G_free_play-w2/e7":"partly","tl-G_free_play-w2/e8":"partly","tl-G_free_play-w2/e9":"partly","tl-G_free_play-w2/e10":"partly","tl-G_free_play-w2/e11":"partly","tl-G_free_play-w2/missed":"some","qa/A_R_01100":"wrong","qa/A_R_01110":"wrong","qa/A_R_00001":"wrong","qa/A_L_11100":"wrong","qa/A_L_00001":"wrong","qa/A_both_10001_01001":"wrong","qa/B_L_open":"wrong","qa/B_L_spread":"wrong","qa/C_wink_L":"wrong","qa/C_wink_R":"wrong","qa/D_step_back":"right","qa/F_backlit":"right","qa/F_dim":"right","qa/H_throw_top":"wrong","qa/H_throw_bottom":"wrong","qa/H_throw_left":"wrong","qa/A_R_11000":"wrong","qa/A_R_01111":"wrong","qa/A_L_01110":"wrong","qa/A_L_01111":"wrong","qa/B_R_count1":"wrong","qa/B_R_count2":"wrong","qa/B_R_count4":"wrong","qa/H_clap":"wrong","qa/A_R_10001":"right","qa/B_L_count6":"right","qa/E_ball":"right","qa/E_demo_object":"right","qa/E_pen":"right","qa/F_hands_overlap":"right","qa/H_circle_cw":"right","qa/H_orb_throw":"right","qa/H_pinch_drag":"right","qa/H_swipe_right":"right"}
```
