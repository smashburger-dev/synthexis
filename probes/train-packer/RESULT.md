# Train packer and Mac preflight (2026-11-02)

Roadmap items "Packer" and "Mac preflight" under `train.setup`, with D9 to D12 as chosen in `spec/synthexis.yaml` (all four still `status: proposed`). Everything ran locally on the M1 Pro, no paid compute.

No Apple Vision output is in this pack. A first pack at 00:42 picked up Apple Vision hand keypoints for S1; Apple's DPLA 3.2 forbids training on Apple model output (licence-check session), so on Noa's order that pack and its preflight models were deleted, the keypoint path was removed from the code, and everything below comes from the repack.

## Commands

```
uv run tools/train/pack.py                    # all four sessions, sets seg,proxy,hand,face
uv run tools/train/pack.py --sets hand        # rewrite one set only
uv run tools/train/preflight.py               # 64 samples, 20 steps, CoreML export
```

Output (gitignored): `data/packed/<set>/<session>/<split>-NNNNN.tar`, `data/packed/manifest.json`, `data/preflight/preflight.json`, `data/preflight/*.mlpackage`.

## Result

- Pack time: 392 s wall for 47,367 frames (S1 61 s, S2 116 s, S3 138 s, S4 77 s; 8 worker processes on a machine shared with other sessions; the first pack took 319 s).
- Disk: 5.2 GB in 108 shard files plus the manifest (seg 2.0 GB, proxy 2.6 GB, hand 0.28 GB, face 0.35 GB). Well under the 50,000-file guideline for Modal Volumes.
- Determinism: packing the S3 face set twice gives byte-identical tar files (md5), and the repack matches the first pack byte for byte. Tar members carry mtime 0.
- Preflight: passed, all 19 checks. CoreML export of both students works.

### Samples per set and split

| set | train | dev | heldout (S4) | shards | GB |
|-----|------:|----:|-------------:|-------:|---:|
| seg 384x216 + prev frame | 31,578 | 7,123 | 8,666 | 28 | 1.98 |
| proxy 768x432 | 31,578 | 7,123 | 8,666 | 28 | 2.53 |
| hand crop 128 | 28,939 | 6,873 | 7,941 | 26 | 0.28 |
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
| 00000 | 367 | 427 | 186 |
| 00001 | 94 | 44 | 34 |
| 01000 | 175 | 196 | 79 |
| 01001 | 333 | 74 | 104 |
| 01100 | 505 | 0 | 109 |
| 01110 | 59 | 52 | 40 |
| 01111 | 240 | 0 | 28 |
| 10000 | 523 | 0 | 112 |
| 10001 | 477 | 0 | 110 |
| 11000 | 427 | 46 | 125 |
| 11100 | 255 | 58 | 55 |
| 11111 | 687 | 120 | 154 |
| total | 4,142 | 1,017 | 1,136 |

About one hand crop in seven carries a finger state (6,295 of 43,753): only blocks A and B have one, B counts are out (`label_ok=false`), and the exclusion list drops 12 A segments.

Face crops: 29,534 train, 6,609 dev, 8,411 heldout; eye pixels in 80 % of them, mouth pixels in 82 %. Block C labels (blink, wink_L, smile, ...) ride along in the sample JSON.

Keypoints: none. The hand samples carry crop, side, SAM box and finger state only. The manifest and all 26 hand shards were searched for `keypoint` and `chirality`: 0 hits.

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
| seg, MobileNetV4-Conv-Small + LR-ASPP, 384x216, prev-mask channel | 1,509,454 | 2.570 -> 0.214 (CE 2.554 -> 0.202) | 337 | argmax agreement 0.9997 |
| hand, MobileNetV4-Conv-Small, 128 px -> 21x2 keypoints + 5 logits | 2,553,231 | 0.839 -> 0.031 (finger BCE only, keypoint loss masked) | 142 | max abs diff 0.0064 keypoints, 0.0196 logits |

Seg ms/step includes the second, gradient-free forward pass on the previous frame for the temporal loss. Packages: 2.9 MB seg, 4.9 MB hand. torch 2.15.0, timm `mobilenetv4_conv_small.e2400_r224_in1k`, coremltools 9.0, target macOS 15. The 64 seg samples are the first 64 of `seg/S1/train-00000.tar` with a previous frame; the 64 hand samples are the first 64 with a finger state. ms/step ran next to other sessions' jobs; the first, quieter run measured 281 and 105.

## Decisions where the spec is open

- Split: per session and block, the segments whose `sha1(session/segment_id)` sorts lowest go to dev, `max(1, round(0.15 n))` of a block with n >= 3 segments. Frames outside any segment take the split of the nearest segment in time. S4 is all `heldout` (its manifest says `split=heldout`).
- Sample order inside one (set, session, split) is shuffled with seed 20261102 before writing, so every shard spans the whole session.
- Masks are downscaled nearest-neighbour. Frames are JPEG quality 90.
- D9: the proxy gets its own set at 768x432 (twice the student size) so the paid run needs no repack. Proxy soft labels for the students do not exist yet; they can live in a sidecar shard set joined by sample key (`<session>_<frame idx>`).
- D10: crops are squares of 1.25x (hands) or 1.3x (face) the longer SAM box side, centred on the box, zero padded past the frame edge, resized to 128. Hands need a box side of 32 px or more, faces 48 px. At most two hands per frame (the SAM post-processing already keeps the top two). Face crop masks: 0 other, 1 eye, 2 mouth.
- D11: each seg sample stores the previous extracted frame and its SAM mask (66 ms earlier at 15 fps). The prev-mask input is one channel holding class id / 6, as in the latency probe. The temporal loss is the L1 distance between class probabilities of frame t and t-1 on pixels where both SAM masks agree, weight 0.1 (placeholder for the pilot).
- D12: MobileNetV4-Conv-Small with timm ImageNet weights for both students; the 4-channel seg stem comes from timm's channel adaptation of the pretrained first conv.
- Hand side: with two hands, the image-left hand is the person's right (recordings are unmirrored). With one hand, it is left or right of the face centre (fallback person box, then frame centre), shifted by half the face width toward the side the script expects when it expects exactly one hand. A finger state is attached only when `label_ok` is true, the segment is not excluded and the assigned side carries a 5-bit label.
- Exclusions: the conservative list from the Spark QA (both passes flag a mismatch, 26 segments over S1 to S4) drops the per-segment labels; frames and SAM masks stay.
- Block H: `motion_user` keeps the scripted label (user's mirrored view), `motion_rec` flips x to the recorded frame (left and right swap, cw and ccw swap, `throw_any_L` stays because L names the hand).
- Box units: S1 video-mode boxes are normalised to 0..1, image mode (S2 to S4 and the S1 mug/pen redo) uses pixels; the packer decides per instance.

## Open points

- Dev covers 8 of 12 finger states (01100, 01111, 10000 and 10001 are missing) because the split is stratified over blocks, not states. S4 has all 12. For calibrating finger thresholds on dev, block A would need stratification by finger state.
- The hand student has 2.55M params, slightly above the `perc.hand` budget of 2.5M. MobileNetV3-Small (1.52M, probe B) stays an option.
- The hand student's 21-keypoint output has no teacher. Apple Vision is ruled out; until a licence-clean keypoint source is chosen, the keypoint loss stays masked and only the finger logits train. `spec/synthexis.yaml` (`perc.hand`, `perc.vision.hand`) and the roadmap still name Vision as the teacher.
- S2 eye class (glasses) is still undecided; samples carry their session so the trainer can mask the eye loss for S2.
- D9 labels "every recorded frame at 30 fps"; the extracted frames are 15 fps, so the proxy labelling pass needs a 30 fps extraction from `cam0.mov`.
- Face student (eye and mouth on the face crop) is packed but not part of this preflight.
