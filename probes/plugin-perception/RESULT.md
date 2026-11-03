# Perception in the plugin (2026-11-02, side fix and hybrid 2026-11-03)

The plugin's engine now runs perception on its own thread and emits the `feat.*` signals of `spec/synthexis.yaml` section 5 under the source names of `tools/mapping/ontology.yaml`. Three backends share one geometry module:

- `students`: seg student (384x216, previous mask fed back), hand boxes from the two largest hand-class components, 1.25x crops, hand student, sides from `SideTracker` (below); face box from the face, eye and mouth classes, 1.3x crop, face student. All on CPU + Neural Engine.
- `hybrid` (the `install.sh` default since 2026-11-03): `students` for seg and hands, plus Apple Vision face landmarks seeded with the seg face box, so Vision skips its own face detection.
- `vision`: Apple Vision hand pose (2 hands x 21 joints) and face landmarks in one handler call.

Runtime use of Vision is allowed (`probes/licence-check`); nothing trains on it. The trained students come from `probes/train-students` (branch `claude/train-students-1137e2`: seg-a-s0, face-a-s1, and hand-final with the finger thresholds folded into the logits).

On S4, the held-out session, `hybrid` takes 9.7 / 11.3 ms per frame against 21 to 31 / 29 to 39 ms for `vision` offline and 40 to 58 ms median in FL. Hands land on the labelled side in 97.1% of block A frames (Vision 99.0%), the face values separate as well as Vision's or better. It misses the 5 ms frame budget: the landmarks alone take 6.7 / 7.8 ms. `students` meets the budget, but its face student does not separate open from closed eyes.

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

## Face: hybrid (2026-11-03)

The face student cannot be recalibrated into a usable eye signal. Its raw eye ratio is 0.50 to 0.65 neutral and 0.41 to 0.50 closed on S1 to S3, overlapping between sessions, and it finds no eyes with glasses (S2). The mouth separates (0.53 to 0.58 neutral, 0.68 to 0.90 wide) but not with one calibration for all sessions.

`hybrid` seeds `VNDetectFaceLandmarksRequest` with a `VNFaceObservation` built from the seg face blob's box, so Vision computes landmarks without its face detector. The landmarks go through the same `face()` geometry and `CAL_LANDMARKS` as the `vision` backend; head angles come from the landmark geometry, because a seeded observation carries no Vision yaw or roll.

| S4 block C (segment means; p5 to p95 span for angles) | `hybrid` | `vision` | face student |
|---|---|---|---|
| mouth.open neutral / wide | 0.003 / 0.999 | 0.003 / 1.0 | 0.48 / 0.89 |
| eye.L.open neutral / closed | 0.994 / 0.018 | 0.80 / 0.00 | 0.94 / 0.76 |
| eye.L / eye.R in wink_L | 0.097 / 0.284 | 0.001 / 0.044 | |
| eye.L / eye.R in wink_R | 0.241 / 0.138 | 0.033 / 0.006 | |
| face.smile neutral / smile | 0.0 / 0.67 | -0.017 raw / +0.022 raw | 0.0 / 0.37 |
| head.yaw span, yaw sweep / neutral | 1.0 / 0.18 | 1.0 / 0.0 | 0.59 / 0.19 |
| head.pitch span, pitch sweep / neutral | 0.77 / 0.23 | 0.53 / 0.06 | 0.26 / 0.38 |
| head.roll span, roll sweep / neutral | 1.0 / 0.04 | 1.0 / 0.0 | 0.31 / 0.09 |

`CAL_LANDMARKS` and `SMILE_LIFT` were calibrated on S4 with the `vision` backend, so these face numbers are in-sample for the calibration. In a wink the closed eye drops further than the open one but both fall, as with `vision`.

Per frame on S4 (7,977 frames, load average 5 to 8): `hybrid` 9.66 / 11.30 ms, of which landmarks 6.71 / 7.79, seg 1.29 / 1.51, hands 0.90 / 1.98. `students` with `SideTracker` 3.22 / 5.76 ms at load 6 to 10. Running the landmarks on a second thread beside the hand crops would save about 1 ms; it is not done.

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

## Signal contract

- Ids: `cam0.` + ontology source id, side after the first segment for sided sources: `cam0.hand.R.openness`, `cam0.hand.L.finger.index.tilt`, `cam0.eye.L.open`, `cam0.mouth.open`, `cam0.face.smile`, `cam0.head.yaw`, `cam0.hands.distance`, `cam0.object.x`. `S:hand.openness side:right` maps to `cam0.hand.R.openness`. The spec's example `cam0.face.mouth.open` (sig.contract) differs from the ontology's `mouth.open`; the code follows the ontology.
- 51 ids: 20 per hand (openness, fist, five finger extensions, spread, x, y, depth, pinch, speed, roll, five finger tilts, finger_count), hands.distance, 9 face (mouth.open, face.smile, eye.L/R.open, eye.L/R.closed, head.yaw/pitch/roll), 5 object (x, y, size, angle, speed; one slot, the largest object, students only).
- Every frame emits every id. A source not seen this frame repeats its last value with `present = false` and confidence 0, so `cond.occlusion` holds and glides. Confidence: mask area for student hands and faces, mean joint confidence for Vision hands, landmark confidence for Vision faces.
- Side: two hands split by image side (image-left is the person's right; camera frames are unmirrored, only the preview in the plugin window is mirrored). One hand: `students` and `hybrid` take the sign of its `SideTracker` track (wrist lean, section above); `vision` keeps the side of a live track within a quarter frame width (tracks expire after 0.5 s), else the face centre decides, else the frame centre.
- Not emitted yet: `hand.pitch` (no 2D estimate), `hand.shape` (needs the shapes table), `arm.*`, `body.*`, `orb.*`, `motion.*`. `hand.roll` measures rotation around the forearm from the knuckle line; its direction is unchecked.

## Open

- FL live run with `hybrid`: camera to parameter latency and the same macro phases as above (needs FL closed for the install).
- 5 ms budget: `hybrid` takes 11.3 ms p95 because of the face landmarks. Either the spec budget moves for the face, or the face student needs eye training that works with glasses.
- Side errors left: 1.8% on S4, concentrated in single segments; a hand that crosses to the other side of the body still trusts its track's history for up to 40 frames of evidence.
- Default routes (Signal 3 to 10, `route.rs`) stand in until the assistant writes routes. With Live on they all move, and FL records every moving macro as automation; Noa's rule is that recording one parameter records only that one.
