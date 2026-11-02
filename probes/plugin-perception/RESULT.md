# Perception in the plugin (2026-11-02)

The plugin's engine now runs perception on its own thread and emits the `feat.*` signals of `spec/synthexis.yaml` section 5 under the source names of `tools/mapping/ontology.yaml`. Two backends share one geometry module:

- `students`: seg student (384x216, previous mask fed back), hand boxes from the two largest hand-class components, 1.25x crops, hand student; face box from the face, eye and mouth classes, 1.3x crop, face student (seg eye and mouth classes while no face model exists). All on CPU + Neural Engine.
- `vision`: Apple Vision hand pose (2 hands x 21 joints) and face landmarks in one handler call. Runtime use is allowed (`probes/licence-check`); nothing trains on it.

The trained students from `probes/train-students` (branch `claude/train-students-1137e2`; seg-a-s0, face-a-s1, and hand-final with the finger thresholds folded into the logits) are installed and meet the frame budget with the face crop: 2.79 to 2.96 / 4.29 ms. They still lose a single hand held near the image centre in a third of the frames and then give it the wrong side, so `install.sh` keeps `vision` as the default; `SYNTHEXIS_PERCEPTION=students ./install.sh` opts in.

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

## Signal contract

- Ids: `cam0.` + ontology source id, side after the first segment for sided sources: `cam0.hand.R.openness`, `cam0.hand.L.finger.index.tilt`, `cam0.eye.L.open`, `cam0.mouth.open`, `cam0.face.smile`, `cam0.head.yaw`, `cam0.hands.distance`, `cam0.object.x`. `S:hand.openness side:right` maps to `cam0.hand.R.openness`. The spec's example `cam0.face.mouth.open` (sig.contract) differs from the ontology's `mouth.open`; the code follows the ontology.
- 51 ids: 20 per hand (openness, fist, five finger extensions, spread, x, y, depth, pinch, speed, roll, five finger tilts, finger_count), hands.distance, 9 face (mouth.open, face.smile, eye.L/R.open, eye.L/R.closed, head.yaw/pitch/roll), 5 object (x, y, size, angle, speed; one slot, the largest object, students only).
- Every frame emits every id. A source not seen this frame repeats its last value with `present = false` and confidence 0, so `cond.occlusion` holds and glides. Confidence: mask area for student hands and faces, mean joint confidence for Vision hands, landmark confidence for Vision faces.
- Side: two hands split by image side (image-left is the person's right; camera frames are unmirrored). One hand keeps the side of a live track within a quarter frame width (tracks expire after 0.5 s), else the face centre decides, else the frame centre.
- Not emitted yet: `hand.pitch` (no 2D estimate), `hand.shape` (needs the shapes table), `arm.*`, `body.*`, `orb.*`, `motion.*`. `hand.roll` measures rotation around the forearm from the knuckle line; its direction is unchecked.

## Open

- Students as default: needs hand recall near the centre and a side rule that survives dropouts, checked on a session other than S4. Option: Vision's face plus student hands, if a face-only Vision request fits the budget (not measured).
- hand-final folds its finger thresholds into the logits, so `finger_count` cutting the sigmoid at 0.5 is correct. The finger extension signals come from keypoint geometry, not from the logits.
- Face student: 1x3x128x128 logits over [other, eye, mouth], output taken by element count; works with the trained `face.mlpackage`.
- Default routes (Signal 3 to 10, `route.rs`) stand in until the assistant writes routes.
