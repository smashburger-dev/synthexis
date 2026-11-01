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
- [ ] Target picker: follow the plugin FL has focused, or pick mixer track then plugin; the assistant resolves only there (resolver top-1 0.676 with the plugin picked, 0.580 with 6 plugins, 0.538 with 30: `probes/open-vocab-mapping/picker.py`); FL side in session fl-target-picker
- [ ] UX structure (session plugin-ux, `docs/ux-plugin.md`): assigned-moves panel per hand, finger and movement class with icons; objects you add and name; profiles 1..n with routes and presets per hand and object, switchable live while the song plays, saved with the project

## Mapping assistant (own session; state in `docs/session-2026-10-30-mapping-train.md`)

- [x] Taxonomy final: 44 classes plus ontology of sources, targets, intents, modifiers (`tools/mapping/ontology.yaml`; spec merge proposed in `probes/mapping-pipeline/SPEC_PROPOSAL.md`)
- [x] Parameter harvest AU (886 of 906 plugins, 122,664 params) and VST3 (405 of 549 bundles) with own tools (`tools/mapping/harvest/`)
- [ ] FL stock plugins: FL script dump (`tools/mapping/fl/README.md`, Noa, about 10 min)
- [x] Spark Open labels: 302 priority plugins double-labelled and adjudicated, 583 more single pass; VST3 labels running
- [x] Glossary EN/DE (11,333 phrases, 416 concepts)
- [x] Synthetic commands generator v3 with validators and round-trip check; stage 1 (8,000 edits) generated
- [x] Real-sentence set: Noa's 34 sentences plus 116 derived (translations, slot swaps, his request forms), grouped split 51 calibration / 99 eval, never trained (`probes/real-eval/RESULT.md`)
- [ ] Noa reviews the derived rows and the plugin context (`data/mapping/eval/review.md`); label the 34 originals, derived labels by delta
- [ ] Spec merge by the main session, proposal "demo requests": XY pad, pinch clutch, values in plugin units, step, ramp toggle, arp style, any finger mask
- [x] param-sense v1 trained (top-1 0.728, macro F1 0.511 on unseen vendors); known-plugin table exported (63,625 rows)
- [x] Parser v1 (9.30M, span-pointer frame decoder) trained on the Mac; challenge set 548 sentences; route_edit 0.628 challenge, 0.697 dev; int8 blob 9.4 MB, 9 ms median on one CPU thread
- [x] Filter ablation and learning curve: all sentences beat the round-trip filter by 11 points; +13-16 points per doubling of edits, so stage 2 goes ahead
- [x] Stage 2 commands (8,374 edits weighted to multi-route, invert, modify, German, layperson; 12.74 $) and parser v2 with sentence concatenation: route_edit 0.712 challenge, 0.768 dev; exported `p-v2-cat-s0`
- [x] Concat rate sweep (0.25 / 0.5 / 1.0, two seeds each): no rate wins outside seed noise; future runs use 0.5
- [x] Targeted stage-3 batch (3,749 sentences, 6.91 $) and parser v3: dev 0.796, challenge 0.748; exported `p-v3-c50-s0`. Runs are now chosen on dev sets only
- [x] Real-sentence eval (calibration 51, eval 99 rows): v3 and v5 both get 14 to 18 of 51 calibration and 14 to 16 of 99 eval rows exactly right, while dev route_edit is 0.83 (`probes/real-eval/RESULT.md`, section "Parser runs")
- [ ] Threshold for the chip fallback calibrated on the 51 calibration rows
- [ ] Resolver and chip fallback in the plugin, "Built with Muse Spark Open" credit
- [x] Open-vocabulary probe: target text against live parameter names beats the class-only binding on unseen names (held-out top-1 0.68 / 0.81 against 0.13 / 0.12); live writes audible in Portal, Timeless, Galaxy Tape Echo (`probes/open-vocab-mapping/RESULT.md`, spec proposal "open-vocabulary parameters")
- [ ] FL bridge write test with third-party plugins while the song plays: ShaperBox (module switch + band mix), Thermal, Portal (session fl-target-picker)
- [ ] Full state lists for stepped parameters (992 parameters in 100 plugins have more states than the 11 harvested samples) and how to switch Serum 2 arp patterns (session plugin-states)
- [x] Parser error analysis (`probes/parser-errors/RESULT.md`): ß missing from the tokenizer vocabulary; 29 % of object labels, 75 % of momentary labels and 6-8 % of invert labels are not expressed by their sentence; text-supported scoring +4.8 points en, +4.9 de
- [x] Bundled retrain A/B/C (tok-v2, projected labels, word hash embedding): no step beyond seed noise, B kept, word hash dropped (`probes/parser-errors/RESULT.md` section 7)
- [x] Stage 4 commands (8,691 sentences, 6.71 $, 59 near-copies of the real set dropped) and parser v5: dev 0.837, challenge 0.781, dev_s4 0.736; +1.1 points against v3 on projected labels (p 0.018, no slice drops); exported `p-v5-s0`, int8 9.4 MB, 10 ms median (section 8)
- [x] Stage 5 commands shaped by calibration findings (4,476 sentences, 6.77 $, 125 near-copies dropped) and parser v6: real-sentence eval exact match 15 % -> 22 % against v5 (p 0.029, both seeds), route F1 0.26 -> 0.34, dev level at 0.83; exported `p-v6-s0`, int8 9.4 MB, 10 ms median (`probes/parser-errors/RESULT.md`, section 9). Last data run of this line
- [x] Parser v7: mmBERT small (German and English, pretrained) as encoder, trained on a Modal L4: real-sentence eval exact match 22 % -> 31 % against v6 (p 0.004, both seeds), right or one part wrong 50 % -> 68 %; teacher labels from dropped sentences did not help; exported `p-v7a-s0`, f16 121 MB, 30 ms median on one CPU thread (`probes/parser-errors/RESULT.md`, section 10). The plugin needs a runtime that loads the backbone
- [x] Parser v8 (`docs/session-2026-11-01-parser-v8.md`): four levers against the 40 % / 75 % goal, chosen on the synthetic dev sets, eval scored once. The ensemble of two v7a seeds ships (f16 2 x 121 MB, 62 ms median on one CPU thread): eval 37 % exact and 74 % right or one part wrong, but a second seed pair gets 29 % and 68 %, so the gain over v7a is about 2 points and not established (p 0.11). Beam, domain MLM on the encoder, clean DeepSeek teacher rows and mDeBERTa-v3-base all fail on eval; teacher rows cost 7.6 points (p 0.004). Goal not reached
- [x] Parser runtime in the plugin (`docs/parser-runtime-plan.md`): Rust crate `plugin/parser` (candle + Accelerate, f16 weights) runs the v8 ensemble; 200 of 200 dev frames equal to Python, 42 ms median and 81 ms p95 on one core, 287 MB RSS for both runs; the plugin loads it in the background and logs frames. Next: text field, resolver in Rust, route diff, chips
- [x] Runtime switched to the v9 ensemble (p-v9-s0 + p-v9-s1): 200 of 200 frames equal to Python on ds-v9c dev and on dev_s6; 39 ms median, 68 ms p95 on dev, 55 / 89 ms on stage 6; v7a stays selectable
- [ ] Noa checks the gold routes of the 34 originals (`data/mapping/eval/gold-routes.json`, `review.md`); the real-sentence numbers move with it. Chips stay the main path for long sentences: v6 gets about a fifth of them exactly right

## Demo and submission

- [x] FigJam updated for non-experts (10-31): status notes on the 8 planning diagrams, build plan coloured by status, new area "What six days taught us" (`docs/figjam-update-2026-10-31.md`)
- [ ] FigJam: update again after Noa's sentences, the retrain and the FL test
- [ ] Demo project in FL, 3-minute video with presets and behind the scenes (11-03)
- [ ] README with real request counts, repo public (11-03)
- [ ] Submission (11-04)
