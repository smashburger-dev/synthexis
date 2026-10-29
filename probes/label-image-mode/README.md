# Probe: image-mode SAM 3.1 labeling

Question: how much cheaper is image-mode grounding — the SAM 3.1 image
backbone runs **once per frame** and every text prompt is grounded on the
shared backbone state — compared to video mode, where the backbone and
tracker rerun per prompt (~151 ms per frame-concept on L40S, measured on
S1)? And how well do the labels agree with the existing S1 video labels?

Two image variants (selected per job via `spec["variant"]`):

- `seq`: `set_image` once per frame, then `set_text_prompt` per prompt.
- `grouped`: `set_image_batch` over 8 frames, `forward_text` once for all
  prompts, one `forward_grounding` for all frame x prompt pairs
  (`FindStage` carries per-query `img_ids`/`text_ids`; the dummy geometric
  prompt is built with `num_prompts = B x P`).

Pilot jobs:

- **P1**: the chunk with the most `A_both_*` frames — base 5 prompts, in
  every requested variant.
- **P2**: the chunk with the most block-F frames — base 5 prompts, seq.
- **Prompt tests**: every 5th frame of `E_mug` x prompts "soda can" /
  "can" / "drink can" and of `E_pen` x "drumstick" / "wooden stick" /
  "stick" / "pen" (seq). These are free-form specs, not chunks.

Pilot output goes to `/vol/pilot/<session>/<cam>/<name>/` on the volume
and `data/labels/_pilot/<session>/<name>/` locally — never into
`data/labels/<session>/` or the `raw/` volume prefix. The comparison
(`data/labels/_pilot/<session>/compare.json`, stdout table,
`disagree_hand.jpg`, `prompts.jpg`) runs postprocess.py on the pilot dirs
and diffs the semantic PNGs against the video labels of the same chunk.

Cost guards: up-front estimate refused above `--max-usd`, re-estimate
after the first measured job, launches stop once finished GPU seconds
reach the cap.

## Commands

Dry-run (free — prints picks + estimate, spends nothing):

    uv run tools/labels/compare_modes.py data/recordings/2026-10-27-S1 --dry-run

Pilot (seq + grouped on P1, seq on P2, both prompt tests):

    uv run tools/labels/compare_modes.py data/recordings/2026-10-27-S1 --max-usd 0.75

Seq only (cheaper):

    uv run tools/labels/compare_modes.py data/recordings/2026-10-27-S1 --max-usd 0.5 --variants seq

Full-session image-mode labeling after a successful pilot:

    uv run tools/labels/run.py data/recordings/2026-10-27-S1 --mode image --max-usd <cap>
    # re-label only some concepts (cached video raws are overwritten):
    uv run tools/labels/run.py data/recordings/2026-10-27-S1 --mode image --redo-concepts mug,pen --max-usd <cap>

Worker: `tools/labels/sam_label.py` -> `ImageLabelerL40S` (L40S, timeout
1800 s, max_containers 4, same image/secrets/volumes as the video worker).
