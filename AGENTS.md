# synthexis

## Contract

- Hackathon entry, Indie track, theme "Build for Your World". Submission due 2026-11-04 21:00 ET (2026-11-05 03:00 CET). The repository becomes public before submission.
- Everything here must be new work created from 2026-10-26 on. Do not copy code from other Lifemaxxing projects; reimplement and say so in the commit.
- Product target: Apple Silicon macOS, FL Studio 2026 first (VST3 and AU), real time on an M1 with 16 GB RAM.
- Use only code, data and assets we have rights to. Never commit recordings, datasets, faces, credentials or the gated participant guide (`private/`, `data/` are ignored).

## Map

- `spec/synthexis.yaml`: single source of truth for features, signals, conditioners, targets and decisions. FigJam diagrams and task lists are generated from it.
- `docs/`: judging analysis, architecture, distillation design, credit texts.
- `probes/`: feasibility probes, one folder each, with a `RESULT.md` holding the measured numbers and the exact command.
- `private/`: gated hackathon material (git-ignored).

## Verification

- Every number in `docs/` or `spec/` cites the probe or source that measured it. Unmeasured values are marked `hypothesis`.
- Latency and throughput claims need a probe run on the real hardware (M1 Pro, macOS 27) or the real cloud GPU, with median and p95.

## Engineering

- Rust for the plugin (nih-plug, CLAP, with clap-wrapper for VST3 and AUv2). Perception never runs on the audio thread.
- Python with uv for the data engine and training on Modal.
- English for code, comments and docs. Talk to Noa in German.
