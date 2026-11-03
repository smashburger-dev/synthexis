# Perception in the plugin (2026-11-02, side fix, hybrid and dev rule 2026-11-03)

The plugin's engine now runs perception on its own thread and emits the `feat.*` signals of `spec/synthexis.yaml` section 5 under the source names of `tools/mapping/ontology.yaml`. Three backends share one geometry module:

- `students`: seg student (384x216, previous mask fed back), hand boxes from the two largest hand-class components, 1.25x crops, hand student, sides from `SideTracker` (below); face box from the face, eye and mouth classes, 1.3x crop, face student. All on CPU + Neural Engine.
- `hybrid` (the `install.sh` default since 2026-11-03): `students` for seg and hands, plus Apple Vision face landmarks seeded with the seg face box, so Vision skips its own face detection.
- `vision`: Apple Vision hand pose (2 hands x 21 joints) and face landmarks in one handler call.

Runtime use of Vision is allowed (`probes/licence-check`); nothing trains on it. The trained students come from `probes/train-students` (branch `claude/train-students-1137e2`: seg-a-s0, face-a-s1, and hand-chir with the finger thresholds folded into the logits plus a handedness logit; hand-final before 2026-11-03).

On S4, the held-out session, `hybrid` emits hands after 2.9 / 4.2 ms (median / p95) and the face after 8.0 ms median, against 21 to 31 ms for `vision` offline and 40 to 58 ms median in FL. Hands land on the labelled side in 98.7% of block A frames and on the wrong side in 0.2% (Vision 99.0% / 0.0%). Several choices looked at S4 before the dev-only rule, so S4 is partly a selection set (list in the section on the dev rule). The landmarks run on their own thread beside the hands; they take about 6 ms on any device Vision offers. `students` alone is faster, but its face student does not separate open from closed eyes.

Tuning and calibration later: `TUNING.md` (constants, rules, lab scripts in `lab/`).

## Commands

```
cd plugin
./guarded-cargo.sh test -p synthexis --release            # 31 tests, incl. ids vs ontology
./install.sh                                              # build, sign, models, backend file
SYNTHEXIS_BENCH=../data/recordings/2026-11-01-S4/cam0.mov SYNTHEXIS_BENCH_SECONDS=266 \
  SYNTHEXIS_BENCH_OUT=/tmp/s4_students.jsonl target/release/synthexis
SYNTHEXIS_PERCEPTION=vision SYNTHEXIS_BENCH=... SYNTHEXIS_BENCH_OUT=/tmp/s4_vision.jsonl target/release/synthexis
python3 ../probes/plugin-perception/eval.py /tmp/s4_vision.jsonl ../data/recordings/2026-11-01-S4/segments.json
python3 ../probes/plugin-perception/fl_live.py           # FL running, bridge up, someone at the camera
```

The bench decodes the recording with AVAssetReader (BGRA, 1280x720) and runs every frame back to back on one thread. S4 is the held-out session, filmed with the same FaceTime camera at 1280x720 and 30 fps. The first 266 s cover blocks A (finger states, 30 segments) and C (face actions, 12 segments), 7,977 frames.

## Latency, offline bench (M1 Pro, macOS 27)

Per frame, median / p95 in ms. Other sessions were training and indexing on the same machine, so the numbers carry their load (1-minute load average 10 to 25 during the runs).

| backend | frame | stages |
|---|---|---|
| students (trained, hand-final), 7,977 frames, two runs | **2.79 / 4.29**, 2.96 / 4.29 | resize 0.38 / 0.49, seg 0.70 / 1.22, mask 0.37 / 0.47, hands 0.69 / 1.70 (crop 0.18 / 0.72, model 0.49 / 1.04), face 0.64 / 0.88 |
| students (trained, hand-v3-s0), 7,977 frames | 2.67 / 3.57 | resize 0.37 / 0.41, seg 0.69 / 0.83, mask 0.35 / 0.42, hands 0.65 / 1.45 (crop 0.16 / 0.64, model 0.47 / 0.96), face 0.60 / 0.76 |
| students (stand-ins), 7,977 frames | 3.57 / 4.79 | resize 0.37 / 0.45, seg 0.75 / 1.36, mask argmax + components 0.46 / 0.56, hands 1.94 / 2.51 (crop 1.21 / 1.43, model 0.73 / 1.10, both hands), face 0 (no face found) |
| vision, 7,977 frames | 21.16 / 28.74 | Vision call 21.00 / 28.56, geometry 0.02 / 0.04 |

- The trained students meet `budget.latency.perception_frame_p95_ms` (5) with hands and face: 4.29 ms p95 at load average 7 to 10, 3.57 ms with the earlier hand model at load 6. A third run under the same load gave 5.97 ms p95 with every stage slower, the unchanged seg model included (1.78 against 1.22 ms), so the margin under load is thin. The face crop and model cost 0.60 / 0.76 ms. The trained hand model is a MobileNetV3-Small and runs faster than the stand-in.
- Vision ran 31.0 / 39.4 ms in a second run on block A alone (load average 6), against 21.2 / 28.7 ms in the first. It stays above the 5 ms budget either way, and at 30 fps it uses most of the 33 ms frame interval; the bounded queue drops frames when it falls behind.
- What it took: the first build ran 6.16 / 9.41 ms. The seg input resize went from a full area average (1.6 ms) through vImage Lanczos (1.0 ms) to 2x2 samples per output pixel (0.37 ms). Hand crops stay on vImage, which beat the bilinear supersampled Rust crop (1.21 against 1.52 ms).
- Vision matches probe C (hands 18.4 / 27.8 ms there, without the face request).
- Seg and hand student per call: 0.75 ms and about 0.37 ms per crop, against the probe B budget of 3 and 1.5 ms p95.

## Signal quality on S4 (eval.py)

| | students (stand-ins) | students (hand-v3-s0) | students (hand-final) | vision |
|---|---:|---:|---:|---:|
| hand frames in block A seen on the labelled side | 0.547 | 0.673 | 0.674 | 0.990 |
| frames with a hand on the absent side | 0.287 | 0.316 | 0.316 | 0.000 |
| finger accuracy (extension > 0.5 vs label bit) | 0.588 | 0.919 | 0.928 | 0.962 |
| finger count mean abs error (fingers) | 1.02 | 0.16 | 0.19 | 0.16 |
| correlation, segment mean openness vs extended fingers / 5 | -0.26 | 0.886 | 0.880 | 0.987 |
| face seen in block C | 0.000 | 0.991 | 0.991 | 0.954 |

The hand model changes finger values only; presence and side come from the seg student and stay the same.

Trained students, why the side fails: in one-hand segments the person holds the hand near the image centre (palm x 0.45 to 0.70 for both sides, same as Vision sees it). Vision sees the hand in every frame, so its track keeps the side it had when the hand came up. The seg student misses the hand in some frames and sometimes finds a second hand blob (the resting hand at the frame edge, or a fragment). The track then expires or flips, and the fallback rule (hand left or right of the face centre) is a coin toss at the centre. In two-hand segments the sides are right (L at x 0.18 to 0.28, R at 0.72 to 0.75). A replay of the tracker on the bench output with longer track memory and a minimum area for the second blob reached 0.87 present and 0.12 wrong side for some settings and got worse for neighbouring ones; tuning it on S4, the held-out session, would overfit, so the code is unchanged.

Trained students, face (face student, mask calibration `CAL_MASK` from the stand-in days, not refitted): mouth neutral 0.48 / wide 0.89, smile neutral 0.0 / smile 0.37, eyes neutral 0.94 / closed 0.76 (eye_L wink_L 0.09). The mouth needs a new calibration and the eyes barely separate; train-students reports the same eye weakness (eye IoU 0.24 on S4, eyes ignored in the glasses session). Vision landmarks are the better face source today.

Vision face values, segment means (after the S4 calibration below):

- `mouth.open`: neutral 0.003, mouth_wide 1.0.
- `eye.L.open` / `eye.R.open`: neutral 0.80 / 0.80, eyes_closed 0.00 / 0.00. In a wink both eyes drop: wink_L gives L 0.001, R 0.044; wink_R gives L 0.033, R 0.006. The raw eye ratios still order the right way (wink_L: L 0.088, R 0.125; wink_R: L 0.117, R 0.096), so a per-user calibration could separate winks; the default does not.
- `head.yaw` and `head.roll`: p5 to p95 span 1.0 in their sweeps, 0.0 in neutral. `head.pitch`: 0.53 in the pitch sweep, 0.06 in neutral.
- `face.smile`: from the lift of the outer-lip corners over the lips' centre per mouth width: neutral -0.017, smile +0.022 (medians). An open mouth moves the same measure (mouth_wide -0.076, mouth_sweep up to +0.21 at p90), so smile is unreliable while the mouth is open.

Calibration (geom.rs `CAL_LANDMARKS`, vision.rs `SMILE_LIFT`), raw medians on S4 block C: eye ratio neutral 0.30, closed 0.06 -> eye 0.08..0.28; inner-lip ratio neutral 0.044, wide 0.569 -> mouth 0.06..0.5; lip corner lift -0.01..0.03. Calibrated and scored on the same session; no second session was checked.

The stand-in numbers judge 20 training steps on 64 frames of S1, not the pipeline.

## Side of a single hand (2026-11-03)

The first student runs put a single hand on the wrong side in 32% of S4 block A frames. The hand was found almost always: labelled side 0.674 plus wrong side 0.316 makes 0.99. train-students measured that the seg student covers at least 30% of the SAM hand in 96.8% of S1 to S3 dev frames. So the fault sat in the plugin's side rule. The old rule (`HandTracker::assign`) keeps a lone hand on the side of the nearest live track, else on its side of the face centre. In the one-hand segments the hand sits at palm x 0.45 to 0.70 for both sides, so after each short loss of the track the side was a coin toss.

The fix uses the forearm. The arm leaves the hand towards its own body side, also when the hand is held at the image centre. `wrist_lean` (`geom.rs`) is the wrist keypoint's x offset from the hand blob's centre per palm size. On S1 to S3 block A its sign alone gets 94, 94 and 87% of one-hand frames right; the face-relative position gets 78, 70 and 31% (S3 is filmed from the side). Other cues were weaker: the wrist against the keypoint centroid (82 to 94%), the person pixels touching the blob (56 to 66%), the cross product of the knuckle line (53 to 68%).

`SideTracker` gives each blob an identity track (nearest live track within a quarter frame width, expiry 0.5 s). A track sums the lean of its frames, capped at 40, starting from a prior of 0.3 times the position against the face centre. One hand takes its track's sign; two hands still split by image side. The rule was chosen on raw candidate dumps of S1 to S3 (`SYNTHEXIS_BENCH_RAW=1`) with an offline simulator; the Rust build reproduced the simulator's numbers exactly. Neighbouring settings (expiry 0.5 to 2 s, match radius 0.15 to 0.4, prior 0 to 1, minimum area 30 to 80) moved the wrong-side rate by less than one point, a lower cap (5 to 20) made S3 worse.

| block A, labelled side / wrong side | S1 | S2 | S3 | S4 (held out, scored once) |
|---|---|---|---|---|
| old rule | 0.721 / 0.201 | 0.727 / 0.206 | 0.760 / 0.232 | 0.674 / 0.316 |
| `SideTracker` | 0.919 / 0.003 | 0.924 / 0.008 | 0.981 / 0.011 | **0.971 / 0.018** |
| best possible with the seg candidates | 0.922 | 0.933 | 0.992 | |
| Vision | 0.888 / 0.000 | 0.959 / 0.000 | 0.993 / 0.000 | 0.990 / 0.000 |

On S4 with `SideTracker`: finger accuracy 0.935 (Vision 0.962), finger count error 0.17 fingers (0.16), openness against extended fingers 0.977 (0.987). The remaining wrong frames on S1 to S3 sit in one segment each. S1 to S3 are training sessions of the students, so their finger numbers are optimistic; S4 is the fair one.

Second vote, handedness (2026-11-03, later). train-students added a handedness logit to the hand student (hand-chir, > 0 = the person's right, Noa's choice). `side_vote` adds it to the lean, scaled to -1..1 at |logit| 2, so both cues weigh the same. Per frame on S1 to S3 one-hand frames the logit's sign alone is right in 98.6 to 99.1% (in-sample for the model), the lean in 87.4 to 95.4%. Weights from 0.5 to 4 and scales from 1 to 4 all gave 0.07 to 0.17% wrong side on S1 to S3; the equal vote sits in that flat region.

| block A, labelled side / wrong side | S1 | S2 | S3 | S4 (scored once) |
|---|---|---|---|---|
| lean + handedness | 0.921 / 0.001 | 0.932 / 0.001 | 0.990 / 0.002 | **0.986 / 0.003** |

On S4 with hand-chir: finger accuracy 0.925, finger count error 0.14 fingers, openness against extended fingers 0.975.

## Face: hybrid (2026-11-03)

The face student cannot be recalibrated into a usable eye signal. Its raw eye ratio is 0.50 to 0.65 neutral and 0.41 to 0.50 closed on S1 to S3, overlapping between sessions, and it finds no eyes with glasses (S2).

`hybrid` takes the face box from the seg student, crops a 256 px square around it (1.4x) and runs `VNDetectFaceLandmarksRequest` seeded with that box, so Vision skips its face detector. The landmarks go through the `face()` geometry of the `vision` backend; head angles come from the landmark geometry, because a seeded observation carries no Vision yaw or roll.

### Latency

The landmarks cost about 6 ms whatever we try: on the full 1280x720 frame 6.7 ms median, on the 256 px crop 6.0 ms; revisions 1 and 2 take 7.5 ms; GPU 11.6 ms, CPU 10.7 ms, Neural Engine (Vision's default) 6.1 ms (S1 block C, 1,800 frames each). Training our own landmark model on Vision output is ruled out (`probes/licence-check`).

So the landmarks run on their own thread (`LandmarkWorker`, `perception.rs`). After the seg mask the perception thread crops the face and hands it over, runs the hand crops and hand model meanwhile, and emits hands, hand distance and object at once (`Emitter::emit_body`). The face follows when the worker returns (`emit_face`), with the same capture timestamp.

| per frame, median / p95 ms | hands and object out (`early`) | face out (`frame`) |
|---|---:|---:|
| S1 block C, 3,600 frames, load 4 to 5 | 2.22 / 3.10, 2.51 / 2.99 | 7.84 / 16.81, 8.15 / 9.20 |
| S1 block A, 1,800 frames | 3.59 / 4.06 | 8.14 / 9.21 |
| S4, 7,977 frames, load 8 to 12 | 2.91 / 4.17 | 7.98 / 16.22 |

Hands now meet the 5 ms frame budget again; the face takes about 8 ms. The p95 of the face swings between 9 and 17 ms with the machine load.

### Values: per-person calibration (`FaceAdapt`, `geom.rs`)

Fitted on S1 to S3 block C, leave-one-session-out, with a 60 s lead-in before block C so the running references start the way they do live.

- Eyes. The outline ratio (minor / major) says how open both eyes are together; Vision's landmarks move both outlines in a wink (S1 wink_L: 0.133 / 0.172, S2: 0.218 / 0.218). The pixels do not: the luma spread inside each eye box (p90 - p10) drops when the lid closes. Each eye's closure (minus spread) is taken against its own open level, the 10th percentile over 10 s, which absorbs glasses and uneven light. The difference between the two eyes, times 8, shifts the more closed eye down and the other up; the upward shift fades when the mean outline says both eyes are shut (gate 0.2). Outline range 0.08 to 0.2. Other pixel cues were weaker (share of dark pixels, darkest decile / median); a linear fit of all cues did no better than the static calibration.
- Hand on the face. A hand box that overlaps the face box shades the eye boxes. The eyes then fall back to the outline and the open levels stay frozen.
- Smile. Mouth width over eye distance, against its own 20 s median: 1.05 to 1.2 maps to 0 to 1. The old lip-corner lift moved more with an open mouth than with a smile. Mouth open stays on the inner-lip ratio (0.06 to 0.5).

| eye error (mean abs, neutral / closed / both winks), S1 / S2 / S3 | |
|---|---|
| static outline calibration (before) | 0.194 / 0.377 / 0.202 |
| `FaceAdapt` | **0.119 / 0.277 / 0.028** |

| segment means, eye L / R | S1 | S2 (glasses) | S3 |
|---|---|---|---|
| neutral | 0.94 / 0.99 | 0.76 / 0.91 | 0.95 / 0.91 |
| eyes closed | 0.03 / 0.03 | 0.23 / 0.87 | 0.00 / 0.02 |
| wink L | 0.19 / 0.91 | 0.57 / 0.86 | 0.00 / 0.98 |
| wink R | 0.91 / 0.46 | 1.00 / 0.09 | 0.96 / 0.00 |
| smile (neutral / smile / mouth wide) | 0.00 / 0.72 / 0.00 | 0.02 / 1.00 / 0.63 | 0.15 / 0.62 / 0.00 |

S2 with glasses stays the weak case: closed eyes keep a high outline ratio behind the frames.

S4. The first S4 run of `FaceAdapt` read the right eye as half shut in neutral (0.26). A hand blob overlaps the face in most of S4's block C (81% of neutral frames, 93 to 100% of the eye segments), which S1 to S3 do not have. That run led to the hand-on-face rule, so the S4 face numbers below are no longer held out.

| S4 block C | before (static) | `FaceAdapt` without hand rule | with hand rule (installed) |
|---|---|---|---|
| eye error | 0.220 | 0.204 | 0.211 |
| eye L / R neutral | 0.99 / 1.00 | 1.00 / 0.26 | 1.00 / 0.85 |
| wink L | 0.07 / 0.25 | 0.00 / 0.91 | 0.12 / 0.42 |
| wink R | 0.23 / 0.13 | 0.45 / 0.18 | 0.41 / 0.21 |
| eyes closed | 0.01 / 0.02 | 0.07 / 0.00 | 0.03 / 0.02 |
| smile: neutral / smile / mouth wide | 0.00 / 0.65 / 0.52 | 0.00 / 1.00 / 0.00 | 0.00 / 1.00 / 0.00 |

With a hand on the face, winks fall back to what the outline alone gives. Mouth open (neutral 0.005, wide 1.0), yaw and roll (span 1.0) are unchanged.

## Second hand blob, eye warm-up, dev-only rule (2026-11-03)

From here on every choice is made on S1 to S3, and S4 is scored once on the frozen config (Noa's rule). Hand choices use only the packer's dev segments (`data/packed/manifest.json`, `dev_segments`): the students trained on the rest of S1 to S3. Block A has 14 dev segments with 2,720 labelled hand frames. The face numbers use all of block C, because the face values come from Vision landmarks and the seg box only places the crop.

seg-v3l against seg-a-s0. train-students delivered seg-v3l (MobileNetV3-Large). On S4 it raised finger errors, and the cause was small hand-class specks (median 89 seg pixels, 2% of the real hand) that took the second hand slot. On dev the specks do not show up in one-hand segments, and seg-a-s0 still wins:

| dev block A, students backend, hand-chir | present | wrong side | finger accuracy | count error | present x finger |
|---|---|---|---|---|---|
| seg-a-s0 | 0.981 | 0.000 | 0.954 | 0.166 | 0.936 |
| seg-a-s0, pair gate 0.1 (installed) | 0.979 | 0.000 | 0.958 | 0.148 | **0.938** |
| seg-v3l | 0.975 | 0.0007 | 0.943 | 0.224 | 0.920 |
| seg-v3l, pair gate 0.1 | 0.965 | 0.0007 | 0.953 | 0.177 | 0.920 |

seg-a-s0 stays in `data/models/seg.mlpackage`; train-students agreed on these numbers, seg-v3l sits in `data/models/archive/`.

Pair gate (`hand_blobs`, `geom.rs`). The second hand blob is dropped when its area is under 0.1 of the first. Real second hands sit at a median 0.58 of the first on S1 to S3 (5th percentile 0.2 to 0.31). Swept over 0, 0.05, 0.1, 0.15 and 0.2 on dev; 0.1 has the most correct finger bits per labelled hand frame. The gain sits in the two-hand segments: tiny second hands lose their slot (present 0.992 -> 0.987) and carried wrong fingers (0.962 -> 0.967).

Eye warm-up (`FaceAdapt`). After a cold start, or after a hand covered the face for longer than the 10 s window, each eye's open level starts from a near-empty window, whose low decile is its widest spread so far. The noisier eye then reads as closing. On S4 this showed right after block A, where the eyes had been in outline-only mode: with seg-v3l the right eye read 0.18 in neutral. The pixel difference now ramps in over the first 8 s of the window. Swept 0 to 8 s on S1 to S3 block C, starting the bench cold at C_neutral and with a 60 s lead-in. The gain rose up to 8 s, the largest value inside the window; later segments did not change.

| eye error, S1 / S2 / S3 | cold start | 60 s lead-in |
|---|---|---|
| no warm-up | 0.118 / 0.278 / 0.027 | 0.119 / 0.277 / 0.028 |
| 8 s warm-up (installed) | **0.113 / 0.257 / 0.019** | 0.119 / 0.271 / 0.028 |

S2 neutral from a cold start: 0.74 / 0.91 -> 0.88 / 0.95.

S4, frozen config, scored once (`hybrid`, seg-a-s0, hand-chir, pair gate, warm-up):

| S4 | before (76ee3db) | now |
|---|---|---|
| block A labelled side / wrong side | 0.986 / 0.003 | **0.987 / 0.002** |
| finger accuracy | 0.925 | **0.934** |
| finger count error | 0.14 | **0.10** |
| openness against extended fingers | 0.975 | 0.974 |
| eye error, block C | 0.211 | 0.211 |
| eye L / R neutral | 1.00 / 0.85 | 1.00 / 0.88 |

Latency from that run is not comparable: the machine ran at load 9, landmarks p95 22.8 ms; hands came out after 2.91 / 4.39 ms.

Choices that looked at S4 before the dev rule, so the S4 figures in this file are partly selection numbers:

- `CAL_LANDMARKS` and `SMILE_LIFT` (the `vision` backend's face calibration): fitted and scored on S4 block C.
- `hybrid` as the default backend: picked on S4 latency and face values.
- `SideTracker`: tuned on S1 to S3, scored on S4 once; the handedness vote came after that score, so S4 was looked at twice for the side.
- `FaceAdapt` hand-on-face rule: added after the first S4 run (section above).
- seg-v3l rollback: train-students first rolled back on the S4 A/B; the dev table above confirms it.
- Pair gate and eye warm-up: both problems were first seen on S4; both values were chosen on S1 to S3.
- Hand openness range: the live note below compares it against S4.

## Response curves and smoothing (2026-11-03)

Noa's live feedback: a mouth about 70% open already read 0.95, and a half-closed fist read 0.7 to 0.8. Both signals ran from 0 to 1 over too short a stretch of the movement and jumped across the middle. All choices below were made on S1 to S3; S4 was scored once at the end.

Mouth. The old cue, the inner-lip outline's minor/major ratio, jumps when the lips part: it goes from 0.05 to 0.56 within 4 frames at the start of every slow sweep, because the mouth gets narrower as it opens. It also reads open in a smile (ratio 0.66 to 0.71) and in head turns. The new cue is the inner-lip height across the eye line divided by the eye distance (`height_across`, `geom.rs`). It rises smoothly through the sweep. It maps to 0 to 1 over 0.06 to 0.6 (`MOUTH_GAP`): closed mouths sit at 0.02 to 0.04, mouth wide at 0.52 to 0.62 (medians per session). Both `vision` and `hybrid` use it. The old ratio remains only when an eye is missing.

Hand. The four fingers' bend ran over 0.5 to 3.0 rad, but on block A of S1 to S3 folded fingers sit at 3.6 to 3.8 rad and extended ones at 0.5 to 0.7 (medians per finger, non-dev segments). A finger that curls towards the camera barely changes its bend angle in the image; its tip moves closer to the wrist instead. Each finger is now half bend, half tip-to-wrist reach per palm length, both anchored at those medians (`FINGER_BEND`, `FINGER_REACH`). The thumb keeps its formula: re-anchored, its accuracy dropped from 0.92 to 0.75 to 0.82.

Smoothing. Holding a hand pose, the openness wanders with a standard deviation of about 0.05, and single frames jump by up to 0.2 (frame-to-frame step p99), from keypoint spikes. The One Euro filter takes out most spikes; no setting removed the slow wander without lag (min cutoff 0.2 to 1 Hz, beta 1 to 30, derivative cutoff 0.2 to 1, a dead band of 0.01 to 0.03). The conditioning chain now runs a median of the last 3 samples before the filter (`cond::Median3`): mean frame step on held poses 0.0077 -> 0.0064, p99 0.075 -> 0.063, a full step reaches 90% after 99 ms instead of 66 ms.

| S1 to S3 | before | now |
|---|---:|---:|
| dev block A finger accuracy | 0.958 | **0.961** |
| hand open ramps (B_*_open), time between 0.2 and 0.8 | 0.17 | **0.30** |
| mouth sweep, time between 0.2 and 0.8 | 0.06 | **0.39** |
| mouth sweep, time above 0.9 | 0.69 | **0.22** |
| mouth in a smile (median) | 1.00 | **0.33** |
| mouth in a head turn (yaw sweep, p90) | 0.97 | **0.26** |
| mouth wide (median) | 1.00 | 0.99 |

| S4, scored once | before | now |
|---|---:|---:|
| finger accuracy | 0.934 | 0.935 |
| openness against extended fingers | 0.974 | 0.965 |
| mouth sweep, time between 0.2 and 0.8 | 0.05 | **0.30** |
| mouth sweep, time above 0.9 | 0.66 | **0.32** |
| mouth in a smile / head turn p90 | 1.00 / 0.58 | **0.38 / 0.20** |
| mouth neutral / wide | 0.00 / 1.00 | 0.00 / 0.96 |

The middle of a hand ramp is still passed quickly: people hold the ends and move through the middle. Stretching the middle further would also stretch the noise at the ends. The anchors come from student keypoints; `vision` uses the same geometry, but its keypoints were not checked against them.

## FL Studio, live (2026-11-03)

FL Studio with synthexis (VST3) on channel 1, backend `vision`, plugin window closed, Noa at the FaceTime camera. `fl_live.py` read the routed macros through the FL bridge at 20 Hz, 8 s per phase (run file `runs/1793662810-fl_live.json`).

| macro | signal | task | range |
|---|---|---|---:|
| Signal 3 | `cam0.hand.R.openness` | right hand open and close | 0.34 (0.24 to 0.58) |
| Signal 4 | `cam0.hand.L.openness` | left hand open and close | 0.00, see below |
| Signal 6 | `cam0.hand.R.y` | right hand up and down | 0.37 |
| Signal 7 | `cam0.hand.R.x` | right hand left and right | 0.56 |
| Signal 8 | `cam0.hand.R.pinch` | pinch | 1.00 |
| Signal 5 | `cam0.mouth.open` | mouth wide, closed | 0.99 |
| Signal 9 | `cam0.eye.L.open` | close the left eye | 0.94 |
| Signal 10 | `cam0.head.yaw` | head left and right | 1.00 |

- Signal 4 stayed flat because Noa missed the instruction in the terminal. A second check with only the left hand up in front of the chest moved Signal 4 over 0.004 to 0.978, while Signal 3 held still (range 0.000), so the side was right. The first phase of that check (arm held out to the side) gave nothing because Noa was not ready in time; it was not repeated.
- Automation: 10 s of right-hand open and close recorded with Live on into pattern 60, then played back with Live off. Signal 3 followed the recording over 0.015 to 0.983 (verdict `recorded`). FL records the perception signal as automation.
- Right-hand openness covered only 0.24 to 0.58 live, against 0 to 1 on S4. The live run used a slow open and close in 8 s; the calibration is from S4.

Latency in FL, from `~/Library/Logs/synthexis.log`, 5 s windows over the run (all perception on its own thread, writes on the main thread):

| | median | p95 |
|---|---:|---:|
| camera to FL parameter (`cam_to_param_ms`, slowest write per tick) | 71 to 148 ms | 157 to 212 ms |
| Vision call per frame | 39 to 58 ms | 68 to 89 ms |
| queue wait before perception | 26 to 44 ms | 60 to 83 ms |
| camera callback to signal emit | 68 to 97 ms | 114 to 153 ms |

- Vision takes about twice as long in FL as in the offline bench (21 to 31 ms median), because FL, the camera and other sessions' training share the machine. Perception processed about 20 of 30 frames per second; the bounded queue dropped the rest.
- `cam_to_param_ms` medians match the 116 to 126 ms of the camera features alone (before perception), but p95 rises to about 200 ms. Vision blows the 5 ms frame budget by an order of magnitude here. The students (2.8 to 3.0 / 4.3 ms offline) would remove most of the 40 to 60 ms Vision call and the queue wait, once their side errors are fixed.

### FL live with `hybrid` (2026-11-03, later)

Same setup and script, backend `hybrid` (seg-a-s0, hand-chir, pair gate, eye warm-up), run file `runs/1793701616-fl_live.json`. Every phase reached nearly the full range:

| macro | signal | range (min to max) |
|---|---|---:|
| Signal 3 | `cam0.hand.R.openness` | 0.96 (0.00 to 0.96) |
| Signal 4 | `cam0.hand.L.openness` | 0.95 (0.00 to 0.95) |
| Signal 6 | `cam0.hand.R.y` | 0.84 |
| Signal 7 | `cam0.hand.R.x` | 0.86 |
| Signal 8 | `cam0.hand.R.pinch` | 0.98 |
| Signal 5 | `cam0.mouth.open` | 1.00 |
| Signal 9 | `cam0.eye.L.open` | 1.00 |
| Signal 10 | `cam0.head.yaw` | 1.00 |

Automation replay of Signal 3: 0.00 to 0.97, verdict `recorded`. Right-hand openness now spans 0 to 0.96 live (Vision run: 0.24 to 0.58).

Latency, 22 windows of 5 s:

| | median | p95 |
|---|---:|---:|
| camera to FL parameter (`cam_to_param_ms`) | 88 to 130 ms | 120 to 148 ms |
| camera frame time to callback (`pts_to_cb_ms`, before perception) | 71 to 111 ms | 85 to 122 ms |
| callback to hands and body emitted (`cb_to_emit`) | 10 to 21 ms | 22 to 47 ms |
| callback to face emitted (`cb_to_face`) | 20 to 27 ms | 37 to 53 ms |
| seg student | 1.9 to 7.7 ms | 19 to 23 ms |
| Vision landmarks | 5.4 to 11 ms | 22 to 27 ms |

- Against `vision` in FL, p95 from camera to parameter drops from 157 to 212 ms to 120 to 148 ms. Perception kept up with all 30 frames per second; nothing was dropped.
- Most of the remaining delay sits before perception: the camera hands each frame to the callback 71 to 111 ms after its timestamp.
- In FL the students run slower than offline (seg p95 about 21 ms against 0.85 ms). FL shared the Neural Engine and CPU with a training session (train-students was working during the run), so the in-FL perception numbers are upper bounds.

### FL live with the new curves (2026-11-03, later)

Same script after the response curves and the median stage (run file `runs/1793703694-fl_live.json`). `fl_live.py` stores only min, max and the count of distinct values per phase, so the time in the middle band is not measured live.

| macro | signal | range now | range before | distinct values now / before |
|---|---|---:|---:|---:|
| Signal 3 | `cam0.hand.R.openness` | 0.87 (0.00 to 0.87) | 0.96 | 120 / 96 |
| Signal 4 | `cam0.hand.L.openness` | 0.92 | 0.95 | 112 / 108 |
| Signal 6 | `cam0.hand.R.y` | 0.90 | 0.84 | 146 / 132 |
| Signal 7 | `cam0.hand.R.x` | 0.84 | 0.86 | 128 / 124 |
| Signal 8 | `cam0.hand.R.pinch` | 0.84 | 0.98 | 124 / 108 |
| Signal 5 | `cam0.mouth.open` | 1.00 | 1.00 | 110 / 72 |
| Signal 9 | `cam0.eye.L.open` | 0.99 | 1.00 | 113 / 114 |
| Signal 10 | `cam0.head.yaw` | 1.00 | 1.00 | 90 / 67 |

Automation replay: 0.01 to 0.89, verdict `recorded`. The open hand now tops out at 0.87 to 0.92: the extended anchors are the median of extended fingers, so a hand reads 1 only when it is opened at least as far as the typical recorded one. Camera to parameter over 25 windows: median 55 to 145 ms (mid 58), p95 76 to 155 ms (mid 80). The camera's frame-to-callback delay swung between 49 and 134 ms; callback to hands emitted took 3 to 8 ms median.

### Fist curve and pinch after the second live test (2026-11-03, later)

Noa, live: the mouth and the pinch control feel right, but opening the fist still jumps from about 0.18 to 0.87 over a few centimetres of finger travel, and a closed pinch tops out at 0.84.

Fist. The raw cues are slower than the mapped value: over the open ramps, bend and reach take 630 and 580 ms (median) from 20% to 80% of their own range, the mapped openness 330 ms. In the ramps the linear finger value spends half of its in-range frames below 0.31 and only a tenth between 0.31 and 0.58. The four fingers now pass a decile map of those ramp frames (`FINGER_DECILES`, `finger_curve`), so the cue's fast middle is flattened and a half-open hand can rest near 0.5. The anchors moved to the 40th percentile of extended and the 60th of folded frames. Variants on S1 to S3 (anchor quantiles 0.5 to 0.9, map weight 0 to 1): firmer anchors alone made the middle faster (267 ms); the full map with median anchors gave the slowest middle (600 ms) but let fists read 0.16 at p75. The hand-blob shape from the seg mask (box or area per palm) grows steadily with the number of extended fingers but is too noisy for this (standard deviation 0.2 to 0.4 against a 0.8 span).

Pinch. Touching thumb and index tips sit 0.1 to 0.3 palm lengths apart in the H_pinch_drag segments of S2 and S3, open hands at 0.9 to 1.3. The window moved from 0.12..0.8 to 0.25..0.93, so its slope (the fine control) stays the same.

| S1 to S3 | 814c0ec | now |
|---|---:|---:|
| hand open ramps, time between 0.2 and 0.8 | 0.29 | **0.40** |
| hand open ramps, 20% to 80% (median) | 400 ms | **584 ms** |
| closed pinch reading 0.95 or more, S2 / S3 | 0.42 / 0.34 | **0.82 / 0.69** |
| dev block A finger accuracy | 0.961 | 0.955 |
| open hand median / fist p75 | 0.93 / 0.10 | 0.92 / 0.16 |

S4, scored once, block A: finger accuracy 0.929 (814c0ec: 0.935), openness against extended fingers 0.956 (0.965), side unchanged (0.987 / 0.002). The ramps were recorded at the protocol's pace, which moves fast through the middle; how a slow live opening feels is for Noa to judge.

Live in FL afterwards (`runs/1793705120-fl_live.json`): right-hand openness 0.02 to 0.97 with 144 distinct values (before 0.00 to 0.87, 120), left hand 0.00 to 0.90, pinch 0.00 to 1.00 (before up to 0.84), automation replay 0.00 to 0.94 `recorded`; mouth, eye and yaw kept their full range.

### Openness top and two hands on x (2026-11-03, evening)

Noa, FL test of the sentence routes: a fully open hand reads about 0.85 while a fist reaches 0.1, so a route on openness never reaches its top; and with both hands routed on x, each hand only uses its own half of the frame.

Openness. The finger mean of an open hand sits at 0.88 (p25) to 0.92 (median) on block A of S1 to S3; the thumb and the decile map keep it off 1.0. `HandFeatures::openness` now stretches the part above a knee at 0.6 so that 0.88 reads 1.0 (`OPEN_KNEE`, `OPEN_TOP`, geom.rs); below the knee nothing changes, and neither do the finger values or the finger count. Rule written before the run: knee fixed at 0.6, top at the p25 of open-hand frames of S1 to S3; pass if the open median is at least 0.97 and p25 at least 0.95, fist percentiles unchanged, the ramps' share between 0.2 and 0.8 drops by at most 0.05 and their 20% to 80% time by at most 15%. Hybrid backend as live, block A frames with one labelled hand.

| openness | before p10 / p25 / p50 / p75 / p90 | after |
|---|---|---|
| open hand, S1 to S3 (896 frames) | 0.81 / 0.88 / 0.92 / 0.94 / 0.96 | 0.91 / 1.00 / 1.00 / 1.00 / 1.00 |
| fist, S1 to S3 (800 frames) | 0.01 / 0.04 / 0.08 / 0.16 / 0.23 | unchanged |
| open hand, S4, scored once (180 frames) | 0.88 / 0.91 / 0.93 / 0.94 / 0.95 | 1.00 / 1.00 / 1.00 / 1.00 / 1.00 |
| fist, S4 (180 frames) | 0.03 / 0.07 / 0.16 / 0.22 / 0.34 | unchanged |
| B_*_open ramps, share between 0.2 and 0.8 | 0.41 | 0.37 |
| B_*_open ramps, 20% to 80% (median) | 584 ms | 617 ms |

Above the knee the slope is 1.43, so a held open hand's wander grows by that factor until it hits the 1.0 clamp.

Two hands on x. In block A the left hand's x has its median at 0.33 to 0.49 per session and the right hand's at 0.69 to 0.74 (x from the person's view), so each hand works on its own side. The split follows the person instead of the frame (Noa): the face centre, sent as the internal signal `cam0.body.x` (not an ontology source, so no sentence can route it), smoothed with a 1 s time constant in `route::Values`, 0.5 until a face is seen. Its median is 0.52 to 0.53 on S1, S2 and S4 and 0.41 on S3, where the person stood off the middle. Hand frames on their own side of the split: face centre 0.886 / 0.806 / 0.737 / 0.795 on S1 to S4, frame middle 0.839 / 0.788 / 0.717 / 0.778. While both `cam0.hand.L.x` and `cam0.hand.R.x` have an enabled route, the left hand's input range is centre - 0.5 .. centre and the right hand's centre .. centre + 0.5, cut at the frame edges (`fit_hand_x`, route.rs); with one hand routed it keeps the whole frame. y has no such rule.

## Signal contract

- Ids: `cam0.` + ontology source id, side after the first segment for sided sources: `cam0.hand.R.openness`, `cam0.hand.L.finger.index.tilt`, `cam0.eye.L.open`, `cam0.mouth.open`, `cam0.face.smile`, `cam0.head.yaw`, `cam0.hands.distance`, `cam0.object.x`. `S:hand.openness side:right` maps to `cam0.hand.R.openness`. The spec's example `cam0.face.mouth.open` (sig.contract) differs from the ontology's `mouth.open`; the code follows the ontology.
- 51 ids: 20 per hand (openness, fist, five finger extensions, spread, x, y, depth, pinch, speed, roll, five finger tilts, finger_count), hands.distance, 9 face (mouth.open, face.smile, eye.L/R.open, eye.L/R.closed, head.yaw/pitch/roll), 5 object (x, y, size, angle, speed; one slot, the largest object, students only).
- Every frame emits every id. A source not seen this frame repeats its last value with `present = false` and confidence 0, so `cond.occlusion` holds and glides. Confidence: mask area for student hands and faces, mean joint confidence for Vision hands, landmark confidence for Vision faces.
- Side: two hands split by image side (image-left is the person's right; camera frames are unmirrored, only the preview in the plugin window is mirrored). One hand: `students` and `hybrid` take the sign of its `SideTracker` track (wrist lean, section above); `vision` keeps the side of a live track within a quarter frame width (tracks expire after 0.5 s), else the face centre decides, else the frame centre.
- Internal: `cam0.body.x`, the face centre on x, splits two hands' x routes (route.rs); not routable.
- Not emitted yet: `hand.pitch` (no 2D estimate), `hand.shape` (needs the shapes table), `arm.*`, `body.*`, `orb.*`, `motion.*`. `hand.roll` measures rotation around the forearm from the knuckle line; its direction is unchecked.

## Open

- Camera delay: 71 to 111 ms from frame timestamp to callback, the largest share of the live latency; not looked at yet.
- Face p95 swings between 9 and 17 ms with machine load; hands stay under 5 ms.
- Winks while a hand overlaps the face fall back to the outline; the rule checks the whole face box, not the eye band.
- S2 (glasses): closed eyes read 0.23 / 0.87.
- Side errors left: 0.2% on S4.
- Routes: only assigned routes may move a macro, so FL records only those (Noa's rule). The built-in routes (camera brightness and motion on Signal 1 and 2, perception on Signal 3 to 10, test.triangle on Signal 32) are test routes and run only while the "Test signal" parameter is on; `fl_live.py` turns it on. No branch assigns routes from the UI yet: `docs/ux-plugin.md` (on main) plans profiles compiled into a route table swapped into the engine. Until that lands, the plugin moves no macro with Test signal off.
