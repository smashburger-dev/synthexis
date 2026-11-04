# synthexis

## Contract

- Hackathon entry, Indie track, theme "Build for Your World". Submission due 2026-11-04 21:00 ET (2026-11-05 03:00 CET). The repository becomes public before submission.
- Everything here must be new work created from 2026-10-26 on. Do not copy code from other Lifemaxxing projects; reimplement and say so in the commit.
- Product target: Apple Silicon macOS, FL Studio 2026 first (VST3 and AU), real time on an M1 with 16 GB RAM.
- Use only code, data and assets we have rights to. Never commit recordings, datasets, faces, credentials or the gated participant guide (`private/`, `data/` are ignored).

## Map

- `spec/synthexis.yaml`: single source of truth for features, signals, conditioners, targets and decisions. FigJam diagrams and task lists are generated from it.
- `docs/`: judging analysis, architecture, student model design, credit texts.
- `probes/`: feasibility probes, one folder each, with a `RESULT.md` holding the measured numbers and the exact command.
- `private/`: gated hackathon material (git-ignored).

## Verification

- Every number in `docs/` or `spec/` cites the probe or source that measured it. Unmeasured values are marked `hypothesis`.
- Latency and throughput claims need a probe run on the real hardware (M1 Pro, macOS 27) or the real cloud GPU, with median and p95.

## Design

- Before any UI work load the skills `design-glass`, `design-rust` and `build-rust`. The plugin UI uses Noa's glass language and the lessons from chiasma (`/Users/no8/Desktop/life/Lifemaxxing/chiasma/docs/STYLE.md`) as its foundation: camera canvas edge to edge, glass panels and floating bar over it, ring knobs for continuous values, colour only on values.
- Reimplement in this repo; do not copy chiasma code.

## Secrets

- Meta Model API key: macOS Keychain item `synthexis.meta-model-api` (read with `security find-generic-password -s synthexis.meta-model-api -w`), Modal secret `meta-model-api` (env `MODEL_API_KEY`).
- Hugging Face token: Modal secret `huggingface` (env `HF_TOKEN`). Baseten: Keychain `synthexis.baseten` and Modal secret `baseten` (env `BASETEN_API_KEY`).
- Never print, log or commit them.

## Engineering

- Rust for the plugin (nih-plug, CLAP, with clap-wrapper for VST3 and AUv2). Perception never runs on the audio thread.
- Python with uv for the data engine and training on Modal.
- English for code, comments and docs. Talk to Noa in German.

## Commands

- Recorder: `tools/recorder/build.sh` builds, bundles the protocol, signs and runs `--selftest` (exit 0 = ready). Never call swiftc through a pipe that hides its exit code. Recordings land in `~/Movies/synthexis/recordings/` (`data/recordings` is a symlink).
- Protocol: edit `tools/make_protocol.py`, run `python3 tools/make_protocol.py`, then `tools/recorder/build.sh`.
- Labels: `uv run tools/labels/run.py data/recordings/<date>-<Sx> --max-usd <cap>` (default GPU L40S). Inspect `data/labels/<session>/contact_sheet.jpg` by eye before using the labels.

## Money

- Every paid cloud run has a cap in its command and the three guards in `tools/labels/run.py` (up-front estimate, pilot re-estimate, rolling spend stop) or their equivalent. Measure on a pilot before scaling; keep GPU containers doing GPU work only.
- Budgets and spend so far: `spec/synthexis.yaml` -> `budget.money`; latest session digest in `docs/session-*.md`.
