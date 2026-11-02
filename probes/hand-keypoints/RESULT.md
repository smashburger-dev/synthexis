# RESULT: hand keypoints offline on S1 to S4 for `perc.hand` (MediaPipe)

Status 2026-11-02: done with MediaPipe Hand Landmarker (Apache-2.0). Apple Vision was run first and dropped: Apple's DPLA 3.2 and the Xcode and Apple SDKs Agreement 2.2 C forbid training on Apple model output (`probes/licence-check/RESULT.md`). All Vision output was deleted, together with the S1 hand shards and the preflight hand model that `tools/train/pack.py` had built from it. No Vision numbers are used here.

Cost 0 $. Run time on the M1 Pro, 8 worker processes, IMAGE mode (no tracking): S1 133 s, S2 113 s, S3 90 s, S4 41 s, 4.7 to 8.3 ms per frame. Postprocess for all four sessions: 4 s.

## Commands

- `uv run tools/keypoints/mediapipe_hands.py <session> --workers 8` writes `data/keypoints/<session>/cam0/mediapipe.jsonl` and `mediapipe_meta.json`. Model `data/models/hand_landmarker.task` (float16, Google mediapipe-models bucket, Last-Modified 2023-04-26, sha256 `fbc2a300...cde1`, checked at start), mediapipe 1.2.0.
- `uv run tools/keypoints/postprocess.py [--source mediapipe|sam3dbody] <session>...` writes `keypoints_mediapipe.jsonl` (per frame: hands with chirality, score, 21 joints `[x, y, z]`, box, `sam_match`, `train_ok`, plus unmatched SAM hands), `stats_mediapipe.json` and `data/keypoints/<session>/contact_sheet_mediapipe.jpg`.
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

## SAM 3D Body (approved by Noa, blocked on access)

Noa approved SAM 3D Body (Meta, SAM License, DINOv3-H+ 840M, hand decoder, FreiHAND PA-MPJPE 5.5 mm per its README) as the stronger teacher on 2026-11-02. `tools/keypoints/sam3d_body.py` is ready:

- Modal L40S, code `facebookresearch/sam-3d-body` at `b5c765a`, weights `facebook/sam-3d-body-dinov3` at `11aaa346`, inference `full` (body decoder, then the hand decoder on both hand crops), prompted with the SAM 3.1 person box. No detector, no FOV model.
- Frames: the 37,207 of 51,517 frames where SAM 3.1 or MediaPipe saw a hand.
- Output `sam3dbody.jsonl` in the MediaPipe schema. The body model always predicts both hands, also hidden ones, so `postprocess.py --source sam3dbody` counts only SAM-matched hands as visible.
- Guards: up-front estimate refused above `--max-usd`, a pilot job re-estimates from measured s/frame before the rest is spawned, and a rolling stop counts finished plus in-flight container seconds, load time included.
- `pilot` runs the same 24 S2 frames in fp32 and bf16 and reports s/frame, the bf16 joint drift in px and the full-run estimate.

Blocker: both HF repos (`-dinov3`, `-vith`) are gated with manual approval, and the token in the Modal secret `huggingface` gets 403 "not in the authorized list". Noa has to request access at huggingface.co/facebook/sam-3d-body-dinov3 with the account behind that token. Then:

```
uv run tools/keypoints/sam3d_body.py pilot --max-usd 0.5
uv run tools/keypoints/sam3d_body.py run 2026-10-27-S1 2026-10-29-S2 2026-11-01-S3 2026-11-01-S4 --max-usd 6 --s-per-frame <pilot value> [--bf16]
uv run tools/keypoints/postprocess.py --source sam3dbody 2026-10-27-S1 2026-10-29-S2 2026-11-01-S3 2026-11-01-S4
```

Two pilot attempts on 2026-11-02 failed before any inference: first an import-time path bug (module-level `Path(__file__).parents[2]` inside the container, 13 crash-looping starts, stopped by hand), then the 403. Modal billed 0.005 $ for both.

## Cost

| item | $ |
|---|---|
| Apple Vision run (dropped, output deleted) | 0 |
| MediaPipe on S1 to S4, M1 Pro CPU | 0 |
| SAM 3D Body pilot attempts (Modal billing report, `synthexis-sam3d-body`) | 0.005 |
| SAM 3D Body full run, estimate (hypothesis until the pilot: 0.30 s per frame on L40S, 37,207 frames, 4 container loads of 120 s, x1.15 overhead) | about 7.3; with `--stride 2` about 3.7 |

Modal November credit: 30 $. Used by 2026-11-02 06:40 CET: parser 9.48, SAM labels 2.29 (S3, S4), SAM 3D Body 0.005, total 11.78, so about 18.2 $ left for student training and SAM 3D Body. Suggested SAM 3D Body cap 6 $, which keeps about 12 $ for training. If the pilot confirms about 0.3 s per frame, run with `--stride 2` (every second frame; 15 fps neighbours are near duplicates) or with `--bf16` if its drift is small.

## Hand-over

- Packer (`train-packer` session, told 2026-11-02): join `keypoints_mediapipe.jsonl` to its hand samples by `sam_match.sam_id` (chunk, file, instance index), use only `train_ok` hands, write the keypoint source into the manifest, never glob `data/keypoints`.
- Licence notice for MediaPipe (Apache-2.0) belongs in the attributions section the licence-check branch adds to `docs/credits.md`; that session was asked to add it.
- FigJam board updated 2026-11-02: Teachers section has "MediaPipe hand keypoints" (wired from the scripted sessions to "Masks and labels") and "SAM 3D Body hands: waiting for Meta access" (dashed); the Apple Vision box in live perception and the two stickies that called Vision a teacher now say fallback only; new stickies in Data engine and Open; Modal budget sticky updated.
