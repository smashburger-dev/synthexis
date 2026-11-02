# RESULT: hand keypoints offline on S1 to S4 for `perc.hand`

Status 2026-11-02: done. Two licence-clean teachers ran on S1 to S4: MediaPipe Hand Landmarker (Apache-2.0, free, Mac CPU) on every frame, and SAM 3D Body (Meta, SAM License, Modal A10G, 5.36 $) on every second frame with a SAM hand. The packer reads one merged file, `data/keypoints/<session>/cam0/keypoints_teacher.jsonl`: per SAM hand the MediaPipe keypoints, else SAM 3D Body's, chirality from SAM 3D Body where it saw the hand.

Apple Vision was run first and dropped: Apple's DPLA 3.2 and the Xcode and Apple SDKs Agreement 2.2 C forbid training on Apple model output (`probes/licence-check/RESULT.md`). Its output, the S1 hand shards and the preflight hand model built from it were deleted. No Vision numbers appear here.

## Teacher file for the packer

`keypoints_teacher.jsonl`, one line per indexed frame: `{file, t, hands: [{sam_id, source, chirality, joints{name: [x, y]}, box}]}`. x, y are normalised to the unmirrored 1280x720 frame, origin top-left. `sam_id` is `<chunk>/<file>#<n>`, n being the index of the hand instance in that frame's `chunk.json`. The packer joins on it, not on boxes. Only `train_ok` hands (SAM-matched, no overlap with the other keypoint hand) are in the file.

| | S1 | S2 | S3 | S4 (held out) |
|---|---|---|---|---|
| teacher hands | 8,643 | 11,088 | 11,088 | 7,185 |
| from MediaPipe | 7,989 | 10,287 | 10,513 | 6,866 |
| from SAM 3D Body (MediaPipe missed the hand) | 654 | 801 | 575 | 319 |

Why MediaPipe first: in the overlays (MediaPipe and SAM 3D Body drawn on the same hand crops, S3) both agree on open hands, but SAM 3D Body's fingertips end short of the real tips (median hand 10 % smaller, centroid 18 px lower), and the tip-vs-pip heuristic matches all five A-block finger bits in 87 to 91 % of MediaPipe hands against 59 to 70 % of SAM 3D Body hands. SAM 3D Body is better where MediaPipe fails: fists (S1 `00000`: 83 % found vs 63 %), occluded hands and hands holding objects. Mixing the two puts SAM 3D Body's shorter tips into about 7 % of the targets. A packer that wants one convention can weight `source == "sam3dbody"` lower or mask their tips.

## Commands

```
uv run tools/keypoints/mediapipe_hands.py <session> --workers 8
uv run tools/keypoints/sam3d_body.py run 2026-10-27-S1 2026-10-29-S2 2026-11-01-S3 2026-11-01-S4 --max-usd 5.5 --stride 2 --s-per-frame 0.77
uv run tools/keypoints/postprocess.py [--source mediapipe|sam3dbody] <session>...
uv run tools/keypoints/compare.py <session>...
uv run tools/keypoints/merge.py <session>...
```

Per session under `data/keypoints/<session>/cam0/` (not in git): `mediapipe.jsonl`, `sam3dbody.jsonl` (raw), `keypoints_<src>.jsonl` (SAM-matched, `train_ok`), `stats_<src>.json`, `compare_mediapipe_sam3dbody.json`, `keypoints_teacher.jsonl`, `teacher_meta.json`, plus `data/keypoints/<session>/contact_sheet_<src>.jpg`. SAM 3D Body parts per 300-frame job sit in `data/keypoints/_sam3dbody_parts/` (resume cache).

## MediaPipe Hand Landmarker

Model `data/models/hand_landmarker.task` (float16, Google mediapipe-models bucket, Last-Modified 2023-04-26, sha256 `fbc2a300...cde1`, checked at start), mediapipe 1.2.0, IMAGE mode (no tracking), 2 hands. M1 Pro, 8 processes: S1 133 s, S2 113 s, S3 90 s, S4 41 s (4.7 to 8.3 ms per frame). Cost 0 $.

| | S1 | S2 | S3 | S4 |
|---|---|---|---|---|
| frames | 16,069 | 13,558 | 13,089 | 8,801 |
| hands per frame | 0.64 | 0.82 | 0.85 | 0.84 |
| keypoint hands matched to a SAM hand | 87.9 % | 93.6 % | 95.9 % | 95.7 % |
| SAM hands with keypoints | 77.5 % | 75.1 % | 84.7 % | 86.5 % |
| one expected hand found (A/B) | 91.4 % | 95.7 % | 94.8 % | 99.7 % |
| two expected hands found (A) | 69.7 % | 76.4 % | 96.2 % | 89.3 % |
| chirality agrees with protocol | 99.25 % (2,919) | 99.33 % (2,983) | 99.58 % (3,118) | 98.98 % (1,573) |
| A hands, all five finger bits (heuristic) | 89.0 % | 86.6 % | 90.6 % | 89.0 % |
| `train_ok` hands | 7,989 | 10,287 | 10,513 | 6,866 |

Match rule: joint box grown by 15 % of its longer side per side, IoU with the SAM box at least 0.3, one-to-one. SAM boxes are normalised in S1 chunks and in pixels in S2 to S4; the tools handle both.

Chirality: all four sessions are unmirrored, S3 (iPhone, side angle) included. MediaPipe's docs say its handedness assumes a mirrored selfie image, but on our frames it already names the anatomical hand (checked against the one-hand A_R/A_L and B_R/B_L segments and the two-hand A segments, where the image-left hand is the right one). No flip.

Failure cases (contact sheets checked by eye):
- Fists: S1 finds only 63 % of `00000` hands (S2 79 %, S3 95 %, S4 98 %).
- Hands close to the camera, blurred or cut by the frame edge are often missed.
- False hands with score 0.95 to 0.97 on shoulder (S1), neck (S3), forehead and chin (S4). SAM has no hand there, so `train_ok` drops them; the score does not.
- Two skeletons on one hand: 31 to 59 two-hand frames per session; `train_ok` drops both.
- Holding objects (block E): only 44 to 71 % of SAM hands get keypoints.

## SAM 3D Body

`facebook/sam-3d-body-dinov3` at HF revision `11aaa346` (DINOv3-H+ backbone, 1,285 M parameters with MHR, input 512 px), code `facebookresearch/sam-3d-body` at `b5c765a`, DINOv3 architecture code `facebookresearch/dinov3` at `6876159a` loaded locally (the repo pulls it unpinned from GitHub main through `torch.hub`; `pretrained=False`, the weights come from the 3DB checkpoint). Inference `full`: body decoder, then the hand decoder on both hand crops. Prompt: the SAM 3.1 person box, no detector, no FOV model. The model always predicts both hands, hidden ones too, so only SAM-matched hands count.

Frames: every second frame with a SAM hand (`--stride 2`), 17,751 of the 34,901 SAM-hand frames. Consecutive frames at 15 fps are 67 ms apart and nearly identical, so the second half would roughly double the cost for little new data; Noa prefers saving the money for student training (2026-11-02). It can be filled in later: the part cache skips every frame already paid for.

| | S1 | S2 | S3 | S4 |
|---|---|---|---|---|
| frames processed | 4,523 | 5,150 | 4,932 | 3,146 |
| SAM hands with keypoints | 83.9 % | 84.1 % | 93.2 % | 89.8 % |
| chirality agrees with protocol | 99.68 % (1,541) | 100 % (1,278) | 100 % (1,513) | 100 % (646) |
| fists `00000` found (A) | 83.3 % | 86.5 % | 99.5 % | 99.1 % |
| A hands, all five finger bits (heuristic) | 66.3 % | 59.3 % | 70.0 % | 58.5 % |
| `train_ok` hands | 4,573 | 5,774 | 5,701 | 3,547 |

Agreement with MediaPipe on the same SAM hands (frames SAM 3D Body processed; error = pixel distance / hand size):

| | S1 | S2 | S3 | S4 |
|---|---|---|---|---|
| hand pairs | 3,919 | 4,973 | 5,126 | 3,228 |
| same chirality | 97.7 % | 97.0 % | 95.8 % | 97.7 % |
| joint error median / p90 | 0.17 / 0.40 | 0.17 / 0.38 | 0.12 / 0.31 | 0.18 / 0.43 |
| PCK@0.1 | 25.7 % | 24.2 % | 40.7 % | 22.8 % |

Per joint (S3): wrist 0.10, MCPs 0.09 to 0.13, tips 0.12 to 0.16, so the gap is spread over the whole hand with a vertical offset, not a swapped joint name. Per block (median): closest in F (0.09 to 0.14) and H (0.10 to 0.16), furthest in E with objects (0.15 to 0.30). Chirality differs mostly in the free blocks D, E, G and H (crossed arms, throws, objects), where the body-anchored side of SAM 3D Body is the more plausible one, so the merged file takes it. MHR joint order checked: from the wrist the distance grows MCP < PIP < DIP < tip in 70 to 79 % of fingers (bent fingers break it, a reversed mapping would give near 0 %).

Contact sheet failure cases: very large hands close to the lens fail the SAM box IoU although the skeleton is right (S3 `A_both_11111_11111`), and hidden hands are predicted near the wrist or body (grey, dropped by `train_ok`).

### Efficiency on Modal (measured)

Profile on 16 S2 frames, per frame: decode 4 ms; L40S body-only 0.19 s, full 0.47 s, full without the per-image `torch.cuda.empty_cache()` 0.42 s; A10G full without it 0.62 s. The config already runs the backbone in bf16; whole-model autocast fails in MHR's sparse TorchScript op. L4 got no container within 10 minutes (twice). Per frame A10G (0.62 s x 0.000306 $/s) is about 16 % cheaper than L40S (0.42 s x 0.000542 $/s). In real jobs volume reads added 0.2 s per frame until images were read ahead in 8 threads; 4 CPU cores helped only 5 % for 17 % more and were dropped. Final: A10G, default CPU, read-ahead, no `empty_cache`, up to 8 containers (Modal granted about 4), 0.745 s per frame in the pilot job.

Guards worked as designed: the first full run stopped after its pilot job at a 10.56 $ re-estimate (cap 8.5), the second at 9.49 $ (cap 9), the third at 10.41 $ (cap 10, 4 CPUs). The stride-2 run: up-front 4.99 $, pilot re-estimate 4.53 $ (cap 5.5), 12,722 container seconds, 4.48 $ by the guard's count, 38 minutes wall.

Failed attempts before any inference: an import-time path bug (13 crash-looping starts), HF 403 before Noa had access, a missing `termcolor` from the unpinned DINOv3 hub code, and the whole-model bf16 autocast. Each was stopped by hand within a minute.

## Cost

| item | $ |
|---|---|
| Apple Vision run (dropped, output deleted) | 0 |
| MediaPipe on S1 to S4, M1 Pro CPU | 0 |
| SAM 3D Body up-front estimate for the stride-2 run | 4.99 |
| SAM 3D Body, Modal billing report `synthexis-sam3d-body` (pilots, profiles, failed starts, three guard-stopped pilot jobs, full stride-2 run) | 5.36 |

Modal November credit 30 $. Billing report 2026-11-02 15:05 CET: synthexis-parser 12.06, synthexis-sam-label 2.29, synthexis-sam3d-body 5.36, total 19.71, so 10.29 $ are left for student training.

## Hand-over

- Packer: read `keypoints_teacher.jsonl` (join on `sam_id`, write `keypoint_source: keypoints_teacher.jsonl` and the per-hand `source` into the manifest, never glob `data/keypoints`). S4 stays held out.
- Licence: credit MediaPipe (Apache-2.0) and SAM 3D Body (SAM License; DINOv3 architecture code under the DINOv3 License, MHR Apache-2.0) in `docs/credits.md` on the licence-check branch.
- FigJam board updated: Teachers section (MediaPipe, SAM 3D Body), Apple Vision marked fallback only, Data engine, Open and Budget stickies.
