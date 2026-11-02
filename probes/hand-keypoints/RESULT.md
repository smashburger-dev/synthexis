# RESULT: hand keypoints offline on S1 to S4 for `perc.hand` (MediaPipe)

Status 2026-11-02: done with MediaPipe Hand Landmarker (Apache-2.0). Apple Vision was run first and dropped: Apple's DPLA 3.2 and the Xcode and Apple SDKs Agreement 2.2 C forbid training on Apple model output (`probes/licence-check/RESULT.md`). All Vision output was deleted, together with the S1 hand shards and the preflight hand model that `tools/train/pack.py` had built from it. No Vision numbers are used here.

Cost 0 $. Run time on the M1 Pro, 8 worker processes, IMAGE mode (no tracking): S1 133 s, S2 113 s, S3 90 s, S4 41 s, 4.7 to 8.3 ms per frame. Postprocess for all four sessions: 4 s.

## Commands

- `uv run tools/keypoints/mediapipe_hands.py <session> --workers 8` writes `data/keypoints/<session>/cam0/mediapipe.jsonl` and `mediapipe_meta.json`. Model `data/models/hand_landmarker.task` (float16, Google mediapipe-models bucket, Last-Modified 2023-04-26, sha256 `fbc2a300...cde1`, checked at start), mediapipe 1.2.0.
- `uv run tools/keypoints/postprocess.py <session>...` writes `keypoints_mediapipe.jsonl` (per frame: hands with chirality, score, 21 joints `[x, y, z]`, box, `sam_match`, `train_ok`, plus unmatched SAM hands), `stats_mediapipe.json` and `data/keypoints/<session>/contact_sheet_mediapipe.jpg`.
- File names carry the source on purpose. A packer must name its keypoint source and train only on hands with `train_ok`.

## Numbers

| | S1 | S2 | S3 | S4 (held out) |
|---|---|---|---|---|
| frames | 16,069 | 13,558 | 13,089 | 8,801 |
| hands per frame | 0.64 | 0.82 | 0.85 | 0.84 |
| keypoint hands matched to a SAM hand | 87.9 % | 93.6 % | 95.9 % | 95.7 % |
| SAM hands with keypoints | 77.5 % | 75.1 % | 84.7 % | 86.5 % |
| one expected hand found (A/B) | 91.4 % | 95.7 % | 94.8 % | 99.7 % |
| two expected hands found (A) | 69.7 % | 76.4 % | 96.2 % | 89.3 % |
| chirality agrees with protocol | 99.25 % (2,919) | 99.33 % (2,983) | 99.58 % (3,118) | 98.98 % (1,573) |
| A hands, all five finger bits by tip-vs-pip heuristic | 89.0 % | 86.6 % | 90.6 % | 89.0 % |
| `train_ok` hands | 7,989 | 10,287 | 10,513 | 6,866 |

Match rule: joint box grown by 15 % of its longer side per side, IoU with the SAM box at least 0.3, one-to-one. SAM boxes are normalised in S1 chunks and in pixels in S2 to S4; postprocess handles both.

Chirality: MediaPipe's docs say handedness assumes a mirrored selfie image, but on our unmirrored frames it already names the anatomical hand. No flip.

## Failure cases (contact sheets checked by eye)

- Fists: S1 finds only 63 % of `00000` hands (S2 79 %, S3 95 %, S4 98 %).
- Hands close to the camera, blurred or cut by the frame edge are often missed (A segments with a finger near the lens, H throws).
- False hands with score 0.95 to 0.97 on shoulder (S1), neck (S3), forehead and chin (S4); 123 C-block frames in S1, 6 to 21 in S2 to S4. SAM has no hand there, so `train_ok` drops them.
- Two skeletons on one hand: 31 to 59 two-hand frames per session with box IoU > 0.3; `train_ok` drops both.
- Holding objects (block E): only 44 to 71 % of SAM hands get keypoints.
- S3's side angle and fast H moves: mostly fine.

## Open

- Noa's choice of "the best state-of-the-art alternative": licence-check recommends SAM 3D Body (SAM License, hand decoder) on Modal. That is a paid GPU run and needs a cap; not started. MediaPipe stays the free cross-check either way.
- Packer: drop the Vision path in `tools/train/pack.py` and read `keypoints_mediapipe.jsonl` with `train_ok` explicitly.
