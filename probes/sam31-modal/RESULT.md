# SAM 3.1 on Modal — image + video-mode timing probe

Random-init weights (facebook/sam3.1 is gated; `download_ckpt_from_hf` is
stubbed to return `None`, so the full model stack is built but never loads a
checkpoint). Timings are representative for architecture/pipeline cost; the
detection counts are 0 because random weights clear no 0.5 threshold.

sam3 commit `f1647e8d0c1d280991f4d1b7d974c2704537fe66`, torch 2.10.0+cu128,
CUDA 12.8, bf16 autocast for inference (as the video predictor does
internally). Input: 300 frames of the shared test clip
(`data/test_frames/`, 1280x720 JPEG; model resamples to 1008x1008).
No flash-attn-3 (`use_fa3=False`, `compile=False`, `warm_up=False`).

## Image mode (Sam3Processor.set_image + set_text_prompt "hand")

300 frames, 10 warmup, `torch.cuda.synchronize()` around each frame.

| GPU | median ms/frame | p95 ms | fps | detections/frame (med) | peak mem |
|-----|-----------------|--------|-----|------------------------|----------|
| H100 80GB | 59.56 | 61.49 | 16.79 | 0 | 5.10 GiB |
| L40S | 102.28 | 109.73 | 9.78 | 0 | 5.07 GiB |
| A100-80GB | 106.72 | 112.96 | 9.37 | 0 | 5.07 GiB |

Image model: 840.5M params.

## Video mode (SAM 3.1 multiplex predictor)

`start_session` on the JPEG folder, `add_prompt` text at frame 0,
`propagate_in_video` (forward) through all 300 frames; total propagation
timed (synchronized at end). Sessions use `async_loading_frames=True`
(default), so JPEG loading overlaps propagation.

| GPU | concept | total s / 300f | ms/frame | fps | tracked objs (med) |
|-----|---------|----------------|----------|-----|--------------------|
| H100 | hand (first session) | 14.05 | 46.83 | 21.36 | 0 |
| H100 | hand | 14.02 | 46.75 | 21.39 | 0 |
| H100 | face | 13.86 | 46.21 | 21.64 | 0 |
| H100 | mouth | 13.85 | 46.18 | 21.66 | 0 |
| H100 | person | 13.87 | 46.22 | 21.64 | 0 |
| H100 | **4-prompt total** | **55.61** | | | |
| L40S | hand (first session) | 34.16 | 113.86 | 8.78 | 0 |
| L40S | hand | 34.63 | 115.43 | 8.66 | 0 |
| L40S | face | 35.10 | 117.00 | 8.55 | 0 |
| L40S | mouth | 35.33 | 117.75 | 8.49 | 0 |
| L40S | person | 34.33 | 114.43 | 8.74 | 0 |
| L40S | **4-prompt total** | **139.38** | | | |
| A100-80GB | hand (first session) | 23.68 | 78.95 | 12.67 | 0 |
| A100-80GB | hand | 23.85 | 79.51 | 12.58 | 0 |
| A100-80GB | face | 23.70 | 78.99 | 12.66 | 0 |
| A100-80GB | mouth | 23.83 | 79.43 | 12.59 | 0 |
| A100-80GB | person | 23.60 | 78.67 | 12.71 | 0 |
| A100-80GB | **4-prompt total** | **94.98** | | | |

Peak GPU memory, video mode: 23.2 GiB on all three GPUs (300-frame buffer +
tracker memory). Video predictor build: 10.2 s (H100) / 11.3 s (L40S) /
12.4 s (A100).

## Cold start

- Fresh image build (first run): ~114 s total across layers (torch+cu128
  wheel install ~79 s dominates); later runs reuse the cached image.
- `modal run` start → model ready, warm image: H100 **78.8 s**
  (container spin-up + imports ~69.5 s + build 9.2 s), L40S 20.8 s, A100 75.2 s
  (container-start variance dominates; GPU queueing differs).
- Function entry → image model ready: 9.2–10.9 s; the model build itself is
  6.8–7.9 s on GPU.

## Derived cost ($/1000 frames)

`GPU $/s × seconds/frame × 1000`. GPU prices: H100 $0.001097/s,
A100-80GB $0.000694/s, L40S $0.000542/s.

| GPU | image mode | video mode |
|-----|-----------|------------|
| H100 | $0.0653 | $0.0514 |
| L40S | $0.0554 | $0.0617 |
| A100-80GB | $0.0741 | $0.0548 |
| **Meta Model API** | **$2.50 / 1000 images** | **$0.20 / 1000 video frames** |

Cheapest self-hosted: L40S image ≈ 45× under API image price; A100 video ≈
3.6× under API video price. (Excludes container spin-up amortization; at
short batch sizes API wins on convenience, at sustained volume self-hosting
is far cheaper.)

## Commands

```sh
PROBE_GPU=H100      uvx modal run probes/sam31-modal/sam31_probe.py
PROBE_GPU=L40S      uvx modal run probes/sam31-modal/sam31_probe.py
PROBE_GPU=A100-80GB uvx modal run probes/sam31-modal/sam31_probe.py
```

## Modal spend

Approx. **$0.50** total (H100 ≈ 230 GPU-s incl. two failed-attempt
container starts ≈ $0.25; L40S 232.5 s ≈ $0.13; A100 180.5 s ≈ $0.13).
Far under the $5 cap. Optional flash-attn-3 run skipped: building it needs
a CUDA-devel image and a long source build, low marginal value vs. the
fa3=False baseline already measured.

## Fixes applied during bring-up (see raw_*_attempt*.log)

1. `Path.parents[2]` IndexError when Modal re-imports the script inside the
   container → frame-dir resolution now falls back to cwd and the remote
   mount path.
2. `import pkg_resources` fails on setuptools ≥ 81 → pinned `setuptools<81`.
3. sam3 `__init__` chain pulls training deps → installed pycocotools,
   opencv-python-headless, hydra-core, omegaconf, scipy, scikit-image,
   matplotlib, pandas, psutil.
4. Image-mode forward needs bf16 autocast (video predictor does it
   internally) → wrapped `set_image`/`set_text_prompt` in
   `torch.autocast(bf16)`.
5. Pinned-commit API mismatch: `Sam3BasePredictor.start_session` always
   passes `offload_state_to_cpu` but
   `Sam3MultiplexTrackingWithInteractivity.init_state` doesn't accept it →
   shim drops the kwarg on `predictor.model.init_state` (no behavior change;
   we don't offload anyway).

## Caveats

- **Random weights**: all detection/tracking outputs are meaningless; median
  detections per frame = 0, median tracked objects = 0. Real checkpoints
  will produce objects and may shift per-frame cost (NMS, mask decode, and
  tracking scale with object count). Treat these as lower-bound pipeline
  timings at fixed architecture cost.
- Image-mode per-frame timing includes PIL JPEG decode (happens inside
  `set_image`'s transform) — representative of a decode+infer pipeline.
- Video timing covers `propagate_in_video` only; `start_session` frame
  loading is asynchronous by default and overlaps propagation.
- Cold-start wall time includes Modal container spin-up, which varies
  (20–80 s observed) and is not model work.
- No app left running; all three `modal run` apps are ephemeral and show
  `stopped` in `modal app list`.

## Logs

`raw_h100.log`, `raw_l40s.log`, `raw_a100.log` (final runs);
`raw_h100_attempt1..5.log` (failed bring-up attempts, kept for provenance).

## Real weights (H100, facebook/sam3.1)

Same setup as the random-weight runs above, but loading the real
`facebook/sam3.1` checkpoint `sam3.1_multiplex.pt` (snapshot `daa6319`)
via `HF_TOKEN` (Modal secret `huggingface`). The HF cache lives in the
Modal Volume `sam31-weights`, so the ~3.6 GB checkpoint was downloaded
once (17.7 s) and re-runs hit the volume cache (<1 s). Probe script:
`probes/sam31-modal/sam31_real.py`, app `synthexis-sam31-real`.

### Image mode (Sam3Processor, real weights)

20 frames evenly spaced over the 300-frame clip x 5 text prompts
(`person`, `face`, `eye`, `mouth`, `hand`). `set_image` once per frame,
`set_text_prompt` timed per prompt under bf16 autocast. The image model
is built from the multiplex checkpoint via `_load_checkpoint` (strips the
`detector.` prefix); the only missing keys are
`backbone.vision_backbone.convs.3.*` — the dual neck's 4th FPN conv,
which stays random-init (the sam3.1 tri neck has 3 convs).

| metric | value |
|--------|-------|
| text_prompt median | 58.18 ms |
| text_prompt p95 | 59.26 ms |
| peak GPU mem | 4.99 GiB |

Detections are real and stable across all 20 frames:

| prompt | det/frame (med) | det/frame (mean) | max score (med) | frames w/ det |
|--------|-----------------|------------------|-----------------|---------------|
| person | 1 | 1.00 | 0.949 | 20/20 |
| face | 1 | 1.00 | 0.961 | 20/20 |
| eye | 2 | 2.05 | 0.691 | 20/20 |
| mouth | 2 | 2.30 | 0.719 | 20/20 |
| hand | 2 | 2.10 | 0.895 | 20/20 |

Per-(frame, prompt) mask overlays: `data/sam31_real/overlays/*.jpg`
(100 files), contact sheet `data/sam31_real/contact_sheet.jpg`,
raw records `data/sam31_real/detections.json` (git-ignored, not
committed). Note: an identical earlier run measured 41.0/43.6 ms
median/p95 for `set_text_prompt`, so run-to-run variance is real; both
runs are inside the 40–60 ms band.

### Video mode (SAM 3.1 multiplex predictor, real weights)

5 concepts x 300 consecutive frames each, `add_prompt` text at frame 0,
forward `propagate_in_video`. Outputs yield in ~16-frame multiplex
batches (`out_obj_ids`, `out_binary_masks`, `out_probs`, `frame_stats`),
so per-yield deltas are bimodal (~0.016 ms inside a burst, ~1.05–1.29 s
at batch boundaries). The `ms/frame` columns below are the cumulative
elapsed/frames-done rate at each yield — the honest per-frame cost —
with `avg = wall/300`.

| concept | wall s / 300f | ms/frame avg | ms/frame med | ms/frame p95 | fps | tracked objs med/max |
|---------|---------------|--------------|--------------|--------------|-----|----------------------|
| person | 21.42 | 71.42 | 78.26 | 164.65 | 14.00 | 1 / 1 |
| face | 20.81 | 69.38 | 76.79 | 157.35 | 14.41 | 1 / 1 |
| eye | 29.42 | 98.06 | 95.59 | 161.71 | 10.20 | 2 / 3 |
| mouth | 22.29 | 74.29 | 80.65 | 158.15 | 13.46 | 2 / 2 |
| hand | 22.53 | 75.10 | 81.58 | 160.69 | 13.32 | 2 / 2 |
| **total** | **117.16** | | | | | |

Wall time includes `start_session` + `add_prompt` (~2–4 s). Real weights
are ~55–75% slower per frame than random weights (46 ms) — detection,
association and tracker reconditioning now do real work, and cost scales
with object count (`eye` with up to 3 objs is the slowest). Peak GPU
memory 28.18 GiB (vs 23.2 GiB random-weight). Video predictor build
11.7 s; image model build 9.6 s.

### Cost and budget

Measured `fn_gpu_s` 150.2 s on the final run -> **$0.165** at
$0.001097/s. Whole-task Modal spend across 4 runs (2 trivial bring-up
failures + 2 successful runs, incl. container spin-up overhead):
~**$0.5**, far under the $5 cap. Single-run hard cap via
`timeout=2100 s` (~$2.30 worst case). All four apps show `stopped` in
`modal app list`; the HF token was never printed (`hf_token_present
True` is the only log reference).

### Command

```sh
PROBE_GPU=H100 uvx modal run probes/sam31-modal/sam31_real.py \
    2>&1 | tee probes/sam31-modal/raw_h100_real.log
```

Logs: `raw_h100_real.log` (final instrumented run),
`raw_h100_real_attempt1..3.log` (earlier runs; 1–2 are trivial bug
failures, 3 is the first complete run without the cumulative-rate
metric).
