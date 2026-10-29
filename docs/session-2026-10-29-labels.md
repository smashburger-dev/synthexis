# Session 2026-10-29: image-mode labelling, S2 labelled

Entry point for the next session. Numbers cite their probe; unmeasured values are marked hypothesis.

## Results

- Image-mode SAM 3.1 labelling ([probes/label-image-mode/RESULT.md](../probes/label-image-mode/RESULT.md)). Variant `grouped` (backbone once per 8-frame batch, all prompts in one grounding call) is now the default: `run.py --mode image`. Batching changes nothing (grouped vs seq IoU 0.998-1.000). Against video mode: person/face/hand agree (IoU median 0.88-0.98), image mode finds more blurred hands (S2 block H: 256 vs 132 frames), and on S1 it labels eyes and mouth that video mode dropped. Video mode is not ground truth; read by eye on 37 crops and frames.
- Cost per 1000 frame-concepts (L40S): video 0.087-0.104 $, image grouped 0.040 $ in the S2 run, 0.0133 $ in the warm pilot. The gap: S2 spent about 4,260 of 5,039 GPU-seconds outside grounding, mostly reading JPEGs one by one from the volume. The worker now reads all chunk files with 32 threads first (1-prompt job: 133.9 -> 78.9 ms per frame-concept, n=2). The 5-prompt effect on S3 is a hypothesis: about 40 s instead of 112 s per chunk, so S3 about 1 $ instead of 2.7 $.
- S2 recorded by Noa (built-in camera, daylight, reading glasses, black T-shirt, 15.1 min, 29.93 fps, 99 segments, none bad, segment boundaries within +-0.06 s of the extracted frames) and labelled: 13,558 frames; frames with detection person 13,439, face 12,250, hand 10,300, mouth 10,237, eye 9,133, can 289, phone 363, drumstick 284, ball 265, bottle 268. Contact sheet checked by eye.
- S1 objects redone in image mode: mug ("soda can") 0 -> 231 frames, pen 3 -> 231 frames (Noa used a wooden drumstick; prompt "drumstick", 48/60 in the prompt test vs "pen" 0/60).
- Face filter fixed: eye/mouth must lie >= 80 % inside the face boxes (bounding box +10 % per side) instead of inside the face pixels; with glasses SAM's face mask leaves out lens parts and the old rule dropped 10,438 correct S2 instances. S1 counts unchanged.
- Spec updated from the hill-climb handoff: P1-P3 and H1 done, D1 main-thread edits, D12 without EfficientViT-B0, `budget.latency` camera numbers, `rec.latency_comp`, `setParamValue` bug under `bridge.fl_script`, iPhone 30 fps note, image-mode teacher, label status.

## Open

1. S2 eye masks are about 2.2x larger relative to the face than S1 (median 2.20 % vs 0.99 % of face area; glasses, lens painted as eye, both modes err). Proposal: ignore the eye class in the loss for S2 frames. Decide before the first training run.
2. Effect of the parallel frame read on a full 5-prompt chunk: read `io_s` and ms per frame-concept in the S3 run.
3. Disk: 12 GiB free (99 %). S3 needs about 3.5 GB, S4 about 2.5 GB.
4. FigJam diagrams for P2/P3 and the automation path are not regenerated yet (handoff task 2).

## Cost

Method GPU seconds at 0.000542 $/s: pilot 0.17, S1 redo about 0.07, S2 2.76, S2 reverse check 0.26; about 3.26 $ plus container start. Modal billing report showed October at 24.996 $ of the 25 $ credit before this session, so everything until 2026-10-31 is Noa's 12.50 $ overage; Noa allows it for S3 and S4. 30 $ credit from 2026-11-01 for training.

## Commands

- Label a session: `uv run tools/labels/run.py data/recordings/<date>-<Sx> --mode image --max-usd <cap>`; redo concepts: `--redo-concepts a,b`; per-session object prompts in `tools/labels/objects.json` -> `sessions`.
- Compare modes: `uv run tools/labels/compare_modes.py <session_dir> [--reverse] [--analyze-only] [--dry-run]`.
