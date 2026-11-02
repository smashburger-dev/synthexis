# Train students: seg, hand, face (2026-11-02)

Roadmap items "students seg and hand on Modal L4; eval on S4 against `budget.quality`" and "H3", under `train.students`. The students learn straight from SAM 3.1 masks and the MediaPipe / SAM 3D Body keypoints; the proxy teacher (D9) is deferred (Noa, 2026-11-02) and runs only if S4 misses the gate. D10 to D12 as packed (Noa: yes). No Apple Vision output is a target anywhere (DPLA 3.2).

## Commands

```
uv run tools/train/preflight.py                                   # 3 students, 20 steps each, CoreML export
uv run tools/train/train.py --student hand --name <run> --epochs 20 [--seed 0] [--device mps]
uvx modal deploy tools/train/modal_students.py                    # once; shards on volume synthexis-students
uv run --with modal python tools/train/modal_students.py spawn <run> --max-usd 0.6 --est-min 20 -- --student hand --epochs 20 --seed 0
uvx modal volume get synthexis-students /runs/<run> data/train/runs/
uv run tools/train/train.py --student hand --name <run> --gate     # dev calibrates, S4 once
uv run --with coremltools tools/train/train.py --student hand --name <run> --export
```

Runs: `data/train/runs/<run>/` (metrics.jsonl, best.pt, last.pt, gate.json, export.json). Models: `data/models/synthexis-{seg,hand,face}.mlpackage`.

## Step 0: refinement before the long run

Checked over every shard (`scratchpad check_meta.py`, 60 s, metadata plus masks) and by eye on augmented batches.

### Leakage

- Sample keys: 0 overlap between train, dev and heldout; 0 segments in both train and dev; S4 only in `heldout`.
- Found: the previous frame crosses the split at segment edges. 40 train frames have a dev frame as their previous frame (and 40 dev frames a train frame). The seg sample stores that previous frame and its SAM mask, so dev pixels would enter training through the prev-mask input and the temporal loss. Fix in `train.py`: a train frame whose previous frame is a dev or heldout frame is treated as a first frame (zero prev mask, no temporal loss). No repack needed.
- `train.py` reads heldout shards only under `--gate`. Finger thresholds are calibrated on dev; the gate applies them to S4 once.

### Label edge cases

- Frames without a previous frame: 20 train, 5 dev, 3 heldout. The preflight batch builder assumed a previous frame; `train.py` feeds zeros and drops the temporal term for them (`has_prev`).
- Hands at the frame edge: 43 % of hand crops reach past the frame (12,350 of 28,939 train) and carry black zero padding. The plugin must pad the same way (contract: zero pad), so nothing to fix; the crop jitter adds more padded views.
- SAM 3D Body hands: 1,519 train crops, fingertips at conf 0, they drop out of the keypoint loss as intended.
- Crops without keypoints: 3,912 train crops have no teacher hand; 199 of them still have a finger state and train the finger head only. The other 3,713 carry no target and are skipped by the reader.
- Keypoints outside the crop: 384 train crops have a visible keypoint outside 0..1. The sigmoid cannot reach them; after augmentation any point that leaves the crop gets conf 0.
- `label_ok=false`: 18 segments per session in S1 to S3 are false on every frame (block B counts, as packed), none in S4; most other segments have some false frames at their edges.
- S3 `F_dim`: 37 frames before 641.1 s carry no label, 78 after it do, as intended. The `dim` label trains nothing in these students; it only matters for later per-condition evaluation.
- Object class: 9 % of train frames, 0.5 % of pixels. Not gated.
- Finger labels against the teacher keypoints: for index to pinky, "tip farther from the wrist than the pip joint" agrees with the scripted bit on 97 to 99 % of MediaPipe crops in every split, so the script labels hold. The thumb rule is too crude to judge (55 %).

### S2 eye masks (decided)

SAM paints the glasses lens as eye in S2. Eye share of the face crop: S1 0.37 %, S2 1.05 %, S3 0.51 %, S4 0.96 %. S4 is high too, but its contact sheet shows tight eye masks: the cap cuts the forehead out of the SAM face box, so the crop is tighter and every part of the face is larger (mouth 1.83 % in S4 against 0.9 to 1.1 % elsewhere). Noa (2026-11-02): ignore the S2 eye. `--s2-eye ignore` sets S2 eye pixels to IGNORE in the seg and face targets; S2 still trains face, mouth and everything else.

### Settings that would not survive the live camera

- Prev-mask input: live, the first frame has no previous mask and later ones come from the student, not SAM. `train.py` zeroes the channel on 30 % of samples and shifts it up to 8 px on the rest. The seg gate on S4 runs in time order and feeds the student's own previous argmax (`heldout_sequential`), as the plugin will.
- Light and camera: brightness 0.6 to 1.4, contrast 0.7 to 1.3, saturation 0.6 to 1.4, white balance 0.9 to 1.1 per channel, sensor noise up to 0.03.
- Scale: seg zooms in 1.0 to 1.25 with a shift inside the frame; crops scale 0.9 to 1.1, shift up to 6 % and rotate up to 10 degrees, because live crops come from student masks, not SAM boxes.
- Horizontal flip, p 0.5, for all three: no output carries a side (the contract has no chirality output, and seg and face classes are not sided), so a flipped hand is a valid hand of the other side with the same finger state. Keypoints flip x and keep the joint order. Checked by eye on augmented batches.

### Class imbalance and loss

- Pixel share in train: background 69 %, person 21 %, hand 5.0 %, face 4.3 %, object 0.5 %, mouth 0.16 %, eye 0.10 %. Plain cross-entropy under-segments the small classes, and the gate measures IoU, so seg and face use cross-entropy + 0.5 x soft Dice over the foreground classes.
- At 384x216 an eye is about 10x5 px; D10 moves eye and mouth to the face crop. The seg student still predicts them, but the eye and mouth gate (0.70) is applied to the face student. The face student reads its LR-ASPP low branch from the stride-4 features (32x32) instead of stride 8.
- Finger states: about one hand crop in five has a label (6,091 of 28,939 train). Bit rates in train: thumb 0.47, index 0.65, middle 0.41, ring 0.25, pinky 0.45.
- Dev has 8 of 12 finger states (01100, 01111, 10000, 10001 missing). Every finger has both values in dev, so per-finger thresholds can be calibrated there, but the four missing states have no dev recall; S4 (all 12) is the first place they are measured, and dev accuracy overstates the S4 number if those states are harder.

## Epoch times

Real data loader (tar stream, shuffle buffer 1,000, JPEG/PNG decode in workers, augmentation on the device), bf16. Mac: M1 Pro, MPS, 4 workers, shared with other sessions (load average in brackets). Modal: L4 with A10G fallback (Modal does not report which ran), 4 CPU cores, 7 workers.

| student | batch | steps/epoch | Mac ms/step | Mac train s/epoch | Modal ms/step | Modal s/epoch (+ dev eval) |
|---------|------:|------------:|------------:|------------------:|--------------:|---------------------------:|
| seg | 32 | 986 | 841 at step 200 (load 12 to 15); 3,030 over 30 steps (load about 100) | about 830 (14 min) extrapolated from 200 steps; about 50 min at load 100 | 156 | 155 (+ 5) |
| hand | 64 | 394 | 269 (load 12 to 20) | 105 | 37 | 15 (+ 1) |
| face | 64 | 461 | 237 (load 12 to 20) | about 109 | 42 | 19 (+ 2) |

The seg Mac row is 200 steps, not a full epoch: a full one would have held the shared GPU for 14 to 50 minutes. The face and seg Mac runs died in their dev evaluation on MPS with "Impacting Interactivity" (macOS GPU watchdog) inside `torch.bincount`; the confusion matrix now runs on the CPU and the gate path passes on MPS. Modal is 5 to 7x faster per step than the shared Mac. Data wait on Modal: 3 % (seg) to 7 % (hand, face).

Budget: the brief assumed 18 $ of Modal credit; the Modal billing report showed 27.50 $ of 30 $ used for November at 19:40 CET. Noa allows up to 42.50 $ (2026-11-02) and chose to train everything on Modal. Price per container hour, priced at the dearer A10G: 1.386 $ (A10G 0.000306 $/s, 4 cores x 0.0000131, 12 GiB x 0.00000222).

## Training

All on Modal (Noa's choice), AdamW lr 2e-3, wd 0.01, 3 % warm-up + cosine, bf16. Hand and face: 20 epochs, batch 64. Seg: 15 epochs, batch 32. Checkpoint by dev score (seg: mean IoU person, face, hand; face: mean IoU eye, mouth; hand: 5-bit finger-state accuracy at 0.5). One change per run against a control, three seeds each for hand and face (rows in `docs/hillclimb.tsv`).

| run | change | dev score, seeds 0 / 1 / 2 | kept |
|-----|--------|----------------------------|------|
| hand-a | MobileNetV4-Conv-Small, 2.55M params | 0.798 / 0.810 / 0.840 (mean 0.816) | control |
| hand-v3 | `mobilenetv3_small_100`, 1.57M params | 0.837 / 0.816 / 0.788 (mean 0.814) | yes: a tie on quality (keypoint error 12.3 to 13.4 px against 12.6 to 14.2), inside the 2.5M budget |
| face-a | LR-ASPP, stride-4 low branch, S2 eye ignored | 0.601 / 0.601 / 0.601 | control |
| face-s1eye | also ignore S1 eye (video-mode masks, smaller) | 0.540 / 0.542 / 0.528 | no: S3 eye unchanged (0.59 against 0.60), and on S1 the model learns to paint no eye at all (IoU 0.00) |
| seg-a | 384x216 + prev mask, CE + Dice + temporal 0.1 | 0.821 (seed 0 only) | control, shipped |

### Bug found in seg-a and repaired without retraining

The first S4 gate of seg-a gave hand IoU 0.02 with a SAM prev mask and 0.79 with a zero prev mask; dev on the Mac showed the same, so it was the model, not S4. Cause: the gradient-free temporal pass on frame t-1 (prev channel all zero) ran in train mode and updated the BatchNorm running statistics, so half of all updates saw "no prev mask". In eval mode a real prev mask then pushed hand pixels to "object". With batch statistics (train mode) the same weights reached hand 0.90 with the prev mask. The per-epoch dev eval on Modal missed it: it reused the train config, whose blocked-prev set (dev and heldout frames) zeroed the prev mask of every dev frame, so its "sam" column was really "zero".

Repairs: `seg_loss` now runs the temporal pass with frozen BN statistics (`students.frozen_bn_stats`), evaluation uses its own config, and `train.py --recal-bn 300` recomputed the running statistics of seg-a over 300 augmented train batches (train data only, same prev dropout). Dev after the repair, SAM prev / zero prev: person 0.912 / 0.881, face 0.885 / 0.865, hand 0.792 / 0.722, eye 0.504 / 0.471, mouth 0.685 / 0.633, object 0.586 / 0.425. The pre-repair S4 numbers above came from a broken model and are not a candidate result; the gate below is the repaired model, gated once.

## S4 gate (held out, `budget.quality`)

| student | measure | S4 | gate | pass |
|---------|---------|---:|-----:|------|
| seg-a-s0, own previous prediction as prev mask, frames in time order (as the plugin runs) | person IoU | 0.766 | 0.85 | no |
| | face IoU | 0.798 | 0.85 | no |
| | hand IoU | 0.797 | 0.85 | no |
| | eye / mouth IoU (384x216, reported only) | 0.137 / 0.659 | | |
| same, SAM prev mask | person / face / hand | 0.851 / 0.833 / 0.844 | | |
| same, zero prev mask | person / face / hand | 0.780 / 0.794 / 0.791 | | |
| face-a-s1 (SAM face box crops) | eye IoU | 0.243 | 0.70 | no |
| | mouth IoU | 0.705 | 0.70 | yes |
| hand-v3-s0 (SAM hand box crops) | 5-bit finger-state accuracy, dev-calibrated thresholds | 0.885 | 0.95 | no |
| | same, thresholds 0.5 | 0.864 | | |
| | per finger thumb .. pinky | 0.940 / 0.990 / 0.988 / 0.982 / 0.952 | | |
| | keypoint error, mean / median px at 128 | 9.9 / 7.3 | | |

Seg per-class S4 (own prev): background 0.952, person 0.766, face 0.798, eye 0.137, mouth 0.659, hand 0.797, object 0.594.

Finger-state recall on S4 (calibrated): 00000 0.996, 00001 0.707, 01000 0.780, 01001 0.719, 01100 0.979, 01110 0.900, 01111 0.756, 10000 0.921, 10001 0.992, 11000 0.955, 11100 0.700, 11111 0.950. Dev-calibrated thresholds thumb .. pinky: 0.90, 0.71, 0.91, 0.56, 0.87 (sigmoid of the logit). Dev accuracy with them 0.893, S4 0.885, so the four states missing from dev did not hurt much: 01100 0.98, 10000 0.92, 10001 0.99, 01111 0.76.

What the misses mean:
- Fingers: index to pinky are right on 95 to 99 % of S4 crops; the thumb is the weak bit. The dev errors are thumb-tucked states read as thumb-out (01000 as 11000, 00001 as 10001); by eye the thumb lies against the hand and looks half extended, so part of this is label ambiguity in the script, not a model bug. The perception session measured Apple Vision on S4 block A at 0.962 per-finger accuracy (`probes/plugin-perception/RESULT.md`, branch claude/affectionate-mcnulty-55c78e, its own crop pipeline); this student's per-finger mean on S4 is 0.970 with SAM crops.
- Eye: on S4 the face student paints 0.34x the SAM eye area (S3 dev 1.06x, S1 1.40x), so it misses the eyes it should see. SAM's S4 style is not the cause: eye to mouth area is 0.49 in S4 and 0.55 in S3. Crop scale is not the cause either: zooming dev crops 1.15x and 1.3x leaves eye IoU at 0.48 and 0.47 (0.47 unzoomed). What remains is appearance (cap shadow, light of another day). On S2 (glasses, eye ignored in training) the student paints no eye at all, so with glasses live the eye signals will be missing.
- Seg: the prev mask helps only when it is right. With the student's own previous mask S4 drops 5 to 8 points below the SAM prev mask and lands at the zero-prev level; live, the prev channel currently buys nothing.

## Export and ANE latency (H3)

CoreML FP16 through `preflight.export` (traced, mlprogram, macOS 15 target), compared with PyTorch on a dev sample, then `probes/coreml-student-latency/bench2.swift` (50 warm-up + 3 x 1,000 predictions, cpuAndNeuralEngine; the machine carried other sessions, load average 5 to 9).

| model | params | PyTorch vs CoreML | ANE median / p95 / p99 ms | budget p95 |
|-------|-------:|-------------------|---------------------------|-----------:|
| seg.mlpackage (seg-a-s0, epoch 14, BN recalibrated) | 1.51M | argmax agreement 0.99995 | 0.547 / 0.649 / 0.719 | 3 |
| hand.mlpackage (hand-v3-s0, epoch 16) | 1.57M | max abs diff keypoints 0.0054, finger logits 0.073 | 0.342 / 0.523 / 0.757 | 1.5 per crop |
| face.mlpackage (face-a-s1, epoch 19) | 1.51M | argmax agreement 0.9991 | 0.339 / 0.500 / 0.743 | none set (hand budget used) |

All ops except three (const, cast, mul at the input) run on the ANE. Seg + 2 hand crops + face is about 1.6 ms median, 2.2 ms p95 of model time, inside `perception_frame_p95_ms` 5 before crops and geometry. The I/O contract is unchanged: names `image` and `prev_mask`, shapes as in the stand-ins; output names are generated (`var_223` etc.), and the plugin picks outputs by element count (`engine/coreml.rs output_with`). `data/models/students.json` names run, epoch, gate result and the calibrated finger thresholds.

## Cost

Modal billing report for app `synthexis-students`, 2026-11-02: 3.39 $ (13 training runs at 0.13 to 0.94 $ each by container seconds, 2.73 $ in sum; two pairs of failed or cancelled starts; image builds). November total before this session 27.50 $; Noa's limit 42.50 $. The Mac preflight, timing epochs, gates, BN repair and exports cost nothing.

## Not done, next

- Seg retrain with the BN fix from the start (about 0.95 $, 40 min on Modal) and a prev mask trained on noisier masks (morphological noise, the student's own prediction), so the prev channel helps live; or drop the channel if it still does not.
- Thumb: label check of the A-block thumb-tucked segments with Noa, then a finger-only fine-tune or more weight on the thumb bit.
- Eye: the face student misses S4 eyes; Apple Vision face landmarks (runtime use allowed) can carry the eye signals until a fix. D9 proxy stays deferred: it learns from the same SAM masks and would not see the S4 look either.
- Student-derived crops: the gate uses SAM boxes; the plugin cuts crops from the seg student's masks. The perception session's bench on S4 measures that path.
