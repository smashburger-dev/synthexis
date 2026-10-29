# Roadmap

Status on 2026-10-29 evening. `spec/synthexis.yaml` stays the source of truth for features and decisions; this file tracks what is done and what is left, by workstream. Update it at the end of every session.

Hard dates: S3 and S4 recorded 10-30/10-31, training from 11-01 (new Modal credit), feature freeze 11-02, demo video and public repo 11-03, submission 11-04 21:00 ET (11-05 03:00 CET).

## Done

- [x] Plan, spec, judging analysis, credits (`docs/plan.md`, `spec/`)
- [x] Probes A (SAM 3.1 on Modal), B (student latency on the ANE), C (Apple Vision too slow live)
- [x] P1 camera in the FL plugin, P2 automation with the window closed (VST3, main thread), P3 FL script sets parameters, H1 camera latency
- [x] Recorder v0.2.0 and protocol v2 (99 segments, block H motions)
- [x] Label pipeline with three cost guards; image mode `grouped`, about a third of video-mode cost
- [x] S1 labelled (12,171 frames), S2 labelled (13,558 frames)
- [x] Plugin skeleton (Welle 3): camera engine, signal contract, conditioning, 32 macros, automation gate passed in FL
- [x] FL bridge for agent-driven FL tests (test-only)
- [x] CoreML export check of the student candidates (D12)

## Data and teacher (main session)

- [ ] S3: iPhone via Continuity, landscape, 20-30 degrees to the side and a bit higher, studio or demo light, no glasses, full protocol (Noa, 10-30)
- [ ] S4: other day, other shirt, demo setup, short protocol, held out (Noa)
- [ ] Label S3 and S4 in image mode (about 1 $ each, hypothesis), contact sheets checked
- [ ] Decide: ignore S2 eye class in the loss (glasses)
- [ ] Apple Vision hand keypoints offline on S1-S4 as targets for `perc.hand` (local, free)
- [ ] Spark Open freestyle timelines and segment QA (`motion.labels`, Baseten)

## Training (main session, from 11-01)

- [ ] Packer: tar shards, 384x216 frames, 128 px hand and face crops, segment-level dev split (`train.setup`)
- [ ] Mac preflight: 64 samples, 20 steps, CoreML export
- [ ] Licence check of every pretrained weight file
- [ ] Decide D9 (proxy teacher), D10 (crops), D11 (temporal prior), D12 (backbone)
- [ ] Proxy teacher, then students seg and hand on Modal L4; eval on S4 against `budget.quality`
- [ ] H3: ANE latency of the trained students, end-to-end frame benchmark

## Plugin (hill-climb line owns `plugin/`)

- [ ] Students in the plugin (CoreML from Rust), derived features (`feat.*`)
- [ ] Motion primitives, presets Pong and Orb, motifs
- [ ] Gestures and MIDI: finger states, chords, patterns, scale, transport
- [ ] Recording latency compensation (`rec.latency_comp`)
- [ ] Hardware probes P4 headphone motion, P5 heart rate, P6 MIDI out
- [ ] `setParamValue` root cause (bridge only, not blocking)
- [ ] UI in the glass language (last, after perception and routes are stable)

## Mapping assistant (can run in its own session now)

- [ ] Taxonomy final (`map.taxonomy`)
- [ ] Parameter harvest of installed plugins: names, ranges, units, value strings (`bridge.harvest`)
- [ ] Spark Open labels the harvest with taxonomy and safe range (`map.labeler`)
- [ ] Spark Open writes synthetic German and English commands with known structured edits (`map.parser` data)
- [ ] param-sense classifier and command parser trained, eval sets written by Noa (`map.eval`)
- [ ] Resolver, chip fallback, "Built with Muse Spark Open" credit

## Demo and submission

- [ ] FigJam diagrams regenerated from the spec
- [ ] Demo project in FL, 3-minute video with presets and behind the scenes (11-03)
- [ ] README with real request counts, repo public (11-03)
- [ ] Submission (11-04)
