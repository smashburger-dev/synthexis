# RESULT: image-mode SAM 3.1 labeling (2026-10-29, L40S)

Decision: label with image mode, variant `grouped` (`run.py --mode image`). It costs a third to a sixth of video mode. For person, face and hand it agrees with video mode, and it finds more hands under motion blur. Eyes behind glasses are unreliable in both modes (see S2 below).

## Cost

| run | ms per frame-concept | $ per 1000 frame-concepts |
|---|---|---|
| video mode, S1 fresh L40S calls (median of 7) | | 0.104 |
| video mode, S2 reverse check (2 chunks, 5 prompts) | 160.3 | 0.0869 |
| image seq, S1 chunk_0009 / chunk_0034 | 81.8 / 82.5 | 0.0443 / 0.0447 |
| image grouped, S1 chunk_0009 (warm container, frames already read once) | 24.6 | 0.0133 |
| image grouped, S2 full run (45 chunks, median) | 72.5 | 0.040 |

S2 full run: 5,039 GPU-seconds, of which 780 s were grounding, about 4,260 s went to backbone plus reading JPEGs one by one from the volume (the pilot's 24.6 ms had the files cached). Fix in `sam_label.py`: the grouped worker now reads all chunk files with 32 threads before inference and reports `io_s`. On a 1-prompt redo job, ms per frame-concept went from 133.9 (before) to 78.9 (after), n=2 chunks each. Full 5-prompt effect not yet measured; the next session (S3) shows it.

Spend: pilot $0.171, S1 mug/pen redo about $0.07, S2 $2.731 + demo_object redo $0.026, reverse check $0.261 (method GPU seconds at $0.000542/s; container start not included).

## Agreement

Forward check on S1 (reference = existing video labels, 300 frames per chunk), IoU median / p10:

| class | chunk_0009 (A_both) | chunk_0034 (block F) |
|---|---|---|
| person | 0.970 / 0.957 | 0.938 / 0.650 |
| face | 0.949 / 0.925 | 0.918 / 0.845 |
| hand | 0.979 / 0.949 | 0.953 / 0.384 |
| eye | 0.363 / 0.283 | 0.417 / 0.000 |
| mouth | 0.780 / 0.000 | 0.000 / 0.000 |

grouped vs seq on chunk_0009: IoU 0.998 to 1.000 for every class, so batching changes nothing.

Reverse check on S2 (reference = image labels, candidate = fresh video run), IoU median / p10, frames with detection video / image:

| class | chunk_0020 (C, glasses) | chunk_0034 (H, motion) |
|---|---|---|
| person | 0.960 / 0.938 | 0.984 / 0.916 |
| face | 0.884 / 0.820 | 0.909 / 0.850 |
| hand | no hands | 0.845 / 0.000; frames 132 / 256 |
| eye | 0.000 / 0.000; frames 239 / 188 | 0.230 / 0.000; frames 41 / 43 |
| mouth | 0.832 / 0.352; frames 295 / 278 | 0.646 / 0.000; frames 219 / 210 |

Read by eye (`data/labels/_pilot/*/disagree_hand.jpg`, `review_face_*.jpg`, 10 S1 face crops, 12 S1 hand frames, 15 S2 face crops):
- Low hand IoU is almost always video mode losing a blurred or overlapping hand that image mode finds (S1 F block, S2 block H: 256 vs 132 frames).
- S1 eyes and mouth: video mode often drops one eye or the mouth; image mode labels both. Image mode missed a closed, pursed mouth in 2 of 10 crops.
- S2 with glasses: both modes err. Image mode sometimes paints the whole lens as eye or misses an eye; video mode sometimes paints the lens rim. Eye mask area relative to face area: S1 median 0.99 %, p99 3.91 %; S2 median 2.20 %, p99 5.13 %. Glasses inflate eye masks about 2.2x.

Not verified: no ground truth; false positives not counted systematically.

## Other fixes from this run

- Face filter: eye/mouth must be >= 80 % inside the face boxes (each face mask's bounding box +10 % per side) instead of inside the face pixels. With glasses SAM's face mask leaves lens parts out, and the old rule removed 10,438 correct S2 instances (S2 eye frames 4,063 -> 9,133). S1 counts are unchanged under the new rule.
- Prompts: pen -> "drumstick" (S1 E_pen 48/60 frames, "pen" 0/60; S1 pen frames 3 -> 231 after redo); mug stays "soda can" (45/61, "can" 46/61); S2 demo object is a dark bottle, prompt "bottle" (0 -> 268 frames).
- run.py pilot guard: the 1.3x rule now needs more than $0.10 absolute overrun, so cent-sized redo jobs do not abort on noise.

## Commands

```
uv run tools/labels/compare_modes.py data/recordings/2026-10-27-S1 --max-usd 0.75 --variants seq,grouped
uv run tools/labels/compare_modes.py data/recordings/2026-10-27-S1 --analyze-only
uv run tools/labels/compare_modes.py data/recordings/2026-10-29-S2 --reverse --max-usd 0.40
uv run tools/labels/run.py data/recordings/2026-10-29-S2 --mode image --max-usd 3
uv run tools/labels/run.py data/recordings/2026-10-29-S2 --mode image --redo-concepts demo_object --max-usd 0.30
```
