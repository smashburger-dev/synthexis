# Train packer and Mac preflight (2026-11-02)

Roadmap items "Packer" and "Mac preflight" under `train.setup`, with D9 to D12 as chosen in `spec/synthexis.yaml` (all four still `status: proposed`). Everything ran locally on the M1 Pro, no paid compute.

No Apple Vision output is in this pack. A first pack at 00:42 picked up Apple Vision hand keypoints for S1; Apple's DPLA 3.2 forbids training on Apple model output (licence-check session), so on Noa's order that pack and its preflight models were deleted, the Vision path was removed from the code, and everything below comes from the repack. Hand keypoints now come from MediaPipe Hand Landmarker (Apache-2.0, vision-keypoints session, `probes/hand-keypoints/RESULT.md` on branch `claude/vision-keypoints-ae6b72`); the hand set was repacked with them in 31 s.

## Commands

```
uv run tools/train/pack.py                    # all four sessions, sets seg,proxy,hand,face
uv run tools/train/pack.py --sets hand        # rewrite one set only, e.g. after new keypoints
uv run tools/train/preflight.py               # 64 samples, 20 steps, CoreML export
```

Output (gitignored): `data/packed/<set>/<session>/<split>-NNNNN.tar`, `data/packed/manifest.json`, `data/preflight/preflight.json`, `data/preflight/*.mlpackage`.

## Result

- Pack time: 392 s wall for 47,367 frames (S1 61 s, S2 116 s, S3 138 s, S4 77 s; 8 worker processes on a machine shared with other sessions; the first pack took 319 s).
- Disk: 5.2 GB in 108 shard files plus the manifest (seg 2.0 GB, proxy 2.6 GB, hand 0.29 GB, face 0.35 GB). Well under the 50,000-file guideline for Modal Volumes.
- Determinism: packing the S3 face set twice gives byte-identical tar files (md5), and the repack matches the first pack byte for byte. Tar members carry mtime 0.
- Preflight: passed, all 19 checks. CoreML export of both students works.

### Samples per set and split

| set | train | dev | heldout (S4) | shards | GB |
|-----|------:|----:|-------------:|-------:|---:|
| seg 384x216 + prev frame | 31,578 | 7,123 | 8,666 | 28 | 1.98 |
| proxy 768x432 | 31,578 | 7,123 | 8,666 | 28 | 2.53 |
| hand crop 128 | 28,939 | 6,873 | 7,941 | 26 | 0.29 |
| face crop 128 | 29,534 | 6,609 | 8,411 | 26 | 0.35 |

Per session and split (seg frames): S1 9,925 / 2,246, S2 11,052 / 2,389, S3 10,601 / 2,488 (train / dev), S4 8,666 heldout. Full shards hold 2,000 samples: about 86 MB for seg, 110 MB for proxy, 14 MB for hand and 16 MB for face. 99.9 % of seg samples carry a previous frame (the rest follow a dropped frame).

### Classes

Seg frames containing each class (train / dev / heldout):

| class | train | dev | heldout |
|-------|------:|----:|--------:|
| person | 31,576 | 7,122 | 8,666 |
| face | 29,544 | 6,612 | 8,419 |
| eye | 23,181 | 4,901 | 7,620 |
| mouth | 23,889 | 5,436 | 7,060 |
| hand | 23,357 | 5,252 | 6,292 |
| object | 2,796 | 1,043 | 883 |

Finger states (hand crops with a label; bits thumb, index, middle, ring, pinky):

| state | train | dev | heldout |
|-------|------:|----:|--------:|
| 00000 | 480 | 426 | 225 |
| 00001 | 147 | 72 | 45 |
| 01000 | 202 | 294 | 127 |
| 01001 | 423 | 73 | 135 |
| 01100 | 612 | 0 | 143 |
| 01110 | 140 | 54 | 70 |
| 01111 | 345 | 0 | 78 |
| 10000 | 579 | 0 | 120 |
| 10001 | 566 | 0 | 127 |
| 11000 | 504 | 68 | 132 |
| 11100 | 333 | 76 | 90 |
| 11111 | 798 | 120 | 179 |
| total | 5,129 | 1,183 | 1,471 |

About one hand crop in six carries a finger state (7,783 of 43,753): only blocks A and B have one, B counts are out (`label_ok=false`), and the exclusion list drops 12 A segments.

Face crops: 29,534 train, 6,609 dev, 8,411 heldout; eye pixels in 80 % of them, mouth pixels in 82 %. Block C labels (blink, wink_L, smile, ...) ride along in the sample JSON.

Keypoints: 35,655 of 43,753 hand crops carry 21 MediaPipe keypoints (train 23,508, dev 5,281, heldout 6,866), in crop coordinates with conf 1.0. That is every `train_ok` hand of `keypoints_mediapipe.jsonl` (S1 7,989, S2 10,287, S3 10,513, S4 6,866), joined on the SAM instance id `<chunk>/<file>#<n>`, not on boxes. The manifest names the source (`keypoint_source`); the packer reads only that file name and never globs `data/keypoints`. The manifest and all 26 hand shards have 0 hits for `vision`.

MediaPipe chirality decides the hand side where a crop has keypoints. The geometric side rule agreed with it on only 84 % of train crops (19,791 of 23,508; S3, filmed from the side, is worst). Switching to chirality raised the finger-labelled train crops from 4,142 to 5,129.

### Dev segments

| session | dev segments |
|---------|--------------|
| S1 (13) | A_L_01000, A_L_11100, A_R_00000, A_both_00000_00000, B_L_count0, B_L_count4, B_L_open, C_neutral, C_roll_sweep, D_arms_sweep, E_ball, F_hand_over_face, G_free_face |
| S2 (17) | A_L_01000, A_R_00001, A_R_01110, A_R_11000, A_both_11111_11111, B_L_count2, B_R_count2, B_R_count3, C_mouth_wide, C_wink_R, D_step_back, E_pen, F_hands_overlap, G_free_play, H_circle_ccw, H_circle_cw, H_throw_bottom |
| S3 (17) | A_L_00001, A_L_01000, A_R_01001, A_both_00000_00000, A_both_01000_00000, B_L_count1, B_L_count3, B_L_count6, C_mouth_wide, C_yaw_sweep, D_lean_sweep, E_ball, F_hand_over_face, G_free_hands, H_orb_hold, H_swipe_down, H_throw_left |
| S4 | none, all heldout |

### Preflight (MPS, bf16 autocast, batch 16, AdamW lr 2e-3 + cosine, 20 steps)

| student | params | loss step 1 -> 20 | ms/step | CoreML FP16 vs PyTorch |
|---------|-------:|-------------------|--------:|------------------------|
| seg, MobileNetV4-Conv-Small + LR-ASPP, 384x216, prev-mask channel | 1,509,454 | 2.570 -> 0.214 (CE 2.554 -> 0.202) | 268 | argmax agreement 0.9997 |
| hand, MobileNetV4-Conv-Small, 128 px -> 21x2 keypoints + 5 logits | 2,553,231 | 1.391 -> 0.303 (finger BCE 0.851 -> 0.034, keypoint L1 0.540 -> 0.269) | 103 | max abs diff 0.0049 keypoints, 0.0314 logits |

Seg ms/step includes the second, gradient-free forward pass on the previous frame for the temporal loss. Packages: 2.9 MB seg, 4.9 MB hand. torch 2.15.0, timm `mobilenetv4_conv_small.e2400_r224_in1k`, coremltools 9.0, target macOS 15. The 64 seg samples are the first 64 of `seg/S1/train-00000.tar` with a previous frame; the 64 hand samples are the first 64 with both a finger state and keypoints; all keypoints lie inside the crop. ms/step varies with other sessions' load on the machine (seg 268 to 337, hand 103 to 142 over three runs).

## Decisions where the spec is open

- Split: per session and block, the segments whose `sha1(session/segment_id)` sorts lowest go to dev, `max(1, round(0.15 n))` of a block with n >= 3 segments. Frames outside any segment take the split of the nearest segment in time. S4 is all `heldout` (its manifest says `split=heldout`).
- Sample order inside one (set, session, split) is shuffled with seed 20261102 before writing, so every shard spans the whole session.
- Masks are downscaled nearest-neighbour. Frames are JPEG quality 90.
- D9: the proxy gets its own set at 768x432 (twice the student size) so the paid run needs no repack. Proxy soft labels for the students do not exist yet; they can live in a sidecar shard set joined by sample key (`<session>_<frame idx>`).
- D10: crops are squares of 1.25x (hands) or 1.3x (face) the longer SAM box side, centred on the box, zero padded past the frame edge, resized to 128. Hands need a box side of 32 px or more, faces 48 px. At most two hands per frame (the SAM post-processing already keeps the top two). Face crop masks: 0 other, 1 eye, 2 mouth.
- D11: each seg sample stores the previous extracted frame and its SAM mask (66 ms earlier at 15 fps). The prev-mask input is one channel holding class id / 6, as in the latency probe. The temporal loss is the L1 distance between class probabilities of frame t and t-1 on pixels where both SAM masks agree, weight 0.1 (placeholder for the pilot).
- D12: MobileNetV4-Conv-Small with timm ImageNet weights for both students; the 4-channel seg stem comes from timm's channel adaptation of the pretrained first conv.
- Hand side: MediaPipe chirality (anatomical) where the crop has keypoints (`side_source: kp_chirality`). Otherwise, with two hands, the image-left hand is the person's right (recordings are unmirrored); with one hand, it is left or right of the face centre (fallback person box, then frame centre), shifted by half the face width toward the side the script expects when it expects exactly one hand. A finger state is attached only when `label_ok` is true, the segment is not excluded and the assigned side carries a 5-bit label.
- Exclusions: the conservative list from the Spark QA (both passes flag a mismatch, 26 segments over S1 to S4) drops the per-segment labels; frames and SAM masks stay.
- Block H: `motion_user` keeps the scripted label (user's mirrored view), `motion_rec` flips x to the recorded frame (left and right swap, cw and ccw swap, `throw_any_L` stays because L names the hand).
- Box units: S1 video-mode boxes are normalised to 0..1, image mode (S2 to S4 and the S1 mug/pen redo) uses pixels; the packer decides per instance.

## Open points

- Dev covers 8 of 12 finger states (01100, 01111, 10000 and 10001 are missing) because the split is stratified over blocks, not states. S4 has all 12. For calibrating finger thresholds on dev, block A would need stratification by finger state.
- The hand student has 2.55M params, slightly above the `perc.hand` budget of 2.5M. MobileNetV3-Small (1.52M, probe B) stays an option.
- Keypoint teacher quality (from the vision-keypoints session): fists in S1 are found only 63 % of the time, and close-to-lens and edge hands are often missed, so 18 % of hand crops have no keypoints and drop out of the keypoint loss. If Noa gets access to SAM 3D Body (gated HF repo), its `keypoints_sam3dbody.jsonl` has the same schema; switching means changing the three `KEYPOINT_*` constants in `pack.py` and running `pack.py --sets hand`.
- S2 eye class (glasses) is still undecided; samples carry their session so the trainer can mask the eye loss for S2.
- D9 labels "every recorded frame at 30 fps"; the extracted frames are 15 fps, so the proxy labelling pass needs a 30 fps extraction from `cam0.mov`.
- Face student (eye and mouth on the face crop) is packed but not part of this preflight.
