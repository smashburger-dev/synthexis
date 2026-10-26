# Session 2026-10-26: plan, probes, tools

## Completed

- Repo `smashburger-dev/synthexis` (private until Nov 3), MIT license, README with hero image and diagrams.
- Plan: [`spec/synthexis.yaml`](../spec/synthexis.yaml) (39 features from Noa's idea, signal contract, decisions D1 to D8), [`docs/plan.md`](plan.md) (judging fit, Meta model use, demo storyboard, risks), [FigJam board](https://www.figma.com/board/WJIE59w34hCSTXuq3XpKGH) with 8 diagrams.
- Credit texts: [`docs/credits.md`](credits.md). Modal and Baseten requested by Noa; Meta $150 claimed.
- Secrets: Meta key in Keychain `synthexis.meta-model-api` and Modal secret `meta-model-api`; Modal secret `huggingface` (HF_TOKEN, SAM 3.1 access granted).
- Probes with RESULT files and raw logs:
  - [SAM 3.1 on Modal](../probes/sam31-modal/RESULT.md): runs; H100 46.8 ms per video frame, $0.051 per 1000 frames (random weights).
  - [Student latency and memory](../probes/coreml-student-latency/RESULT.md): seg + 2 hand crops 1.12 ms median, 2.06 ms p95, +16 MB.
  - [Apple Vision baseline](../probes/apple-vision-fps/RESULT.md): 29.6 ms per frame for hand + face + body.
  - [Camera in a Rust plugin, standalone](../probes/fl-camera-plugin/RESULT.md): works; 18.6 fps and about 100 ms capture latency at night.
- Hardware kits ready for Noa: FL camera plugin (P1, P2, P6), FL Python bridge (P3), Powerbeats motion (P4), heart rate (P5).
- Recording: protocol and readable script ([`docs/recording-protocol.md`](recording-protocol.md), [`docs/recording-script.md`](recording-script.md), 81 segments, 12.1 min), recorder app [`tools/recorder/`](../tools/recorder/README.md) verified: 355 frames in 12.1 s at 29.3 fps, segment lengths within one frame.
- Hill-climb protocol and log: [`docs/hillclimb.md`](hillclimb.md), [`docs/hillclimb.tsv`](hillclimb.tsv).

## Decisions

- Runtime perception uses only the students (synthexis-seg, synthexis-hand); Apple Vision runs offline as keypoint teacher and baseline (D3).
- SAM 3.1 bulk labels on Modal with open weights; Meta API for snapshots and a cross-check (D4).
- Mapping assistant is offline at runtime: param-sense classifier + command tagger + deterministic resolver. Unclear sentences show clickable term chips with options; never an online Spark Open fallback (Noa). Spark Open only at build time (labels, synthetic commands). Muse Model License allows distillation; "Built with Muse Spark Open" must be shown.
- MIT source (D7); design language from design-glass + chiasma STYLE.md, reimplemented (D8).
- Recorder fix: the SwiftUI view created a new AVCaptureVideoPreviewLayer on every render, which re-wired the capture session; movie output stopped (-11806) and the data output starved. Now one lazy preview layer, AVAssetWriter path, output folder from the bundle location.

## Open

- Noa: hardware kits P1 to P6, results into the RESULT files.
- Noa: recordings S1 to S3 (Oct 27), S4 held-out (Oct 28).
- SAM 3.1 real weights not yet run (`probes/sam31-modal/sam31_real.py` still to write).
- FL bridge recording path (CC 23) uses a guessed REC event id; correct it from Noa's dump output.
- Bonus prize question: "Best Build on Meta Open Weight Models" says the partner verifies the project runs on Glimmer or Spark Open. With an offline runtime only the data engine uses Spark Open. Ask Noa whether an explicit opt-in "scan new plugins with Muse Spark Open" button is acceptable.
- FigJam diagrams 1, 4 and 5 predate D3 and the offline mapping decision; regenerate from the spec.
- Baseten and Fireworks credit decisions pending.

## Next session start

Paste [`docs/next-session-prompt.md`](next-session-prompt.md) into a new Devin session opened at `/Users/no8/Desktop/life/Lifemaxxing/synthexis`.
