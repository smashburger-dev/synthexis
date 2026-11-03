# Roadmap

Status on 2026-11-02 morning, after merging all session branches into `main` (`docs/merge-2026-11-02.md`). `spec/synthexis.yaml` stays the source of truth for features and decisions; this file tracks what is done and what is left, by workstream. Update it at the end of every session.

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

- [x] S3: iPhone via Continuity, landscape, 20-30 degrees to the side and a bit higher, studio or demo light, no glasses, full protocol (Noa, recorded 11-01)
- [x] S4: other shirt and cap, FaceTime camera, short protocol, held out (Noa, recorded 11-01, same day as S3)
- [x] Label S3 and S4 in image mode (S3 1.22 $, S4 0.78 $), contact sheets checked
- [x] Decide: ignore S2 eye class in the loss (glasses): ignored (Noa, 2026-11-02)
- [x] Hand keypoints offline on S1-S4 as targets for `perc.hand`: MediaPipe Hand Landmarker (Apache-2.0, 0 $) on all frames, SAM 3D Body (Modal A10G, 5.36 $) on every second SAM-hand frame, merged into `keypoints_teacher.jsonl`; Apple Vision dropped (Apple terms) ([result](../probes/hand-keypoints/RESULT.md))
- [x] SAM 3D Body hand keypoints as second teacher (Noa approved 11-02): fills fists, occluded and object-holding hands MediaPipe misses; chirality 99.7-100 %
- [x] Spark Open freestyle timelines and segment QA (`motion.labels`, Baseten, 5.63 $; [result](../probes/motion-labels/RESULT.md))

## Training (main session, from 11-01)

- [x] Packer: tar shards, 384x216 frames, 128 px hand and face crops, segment-level dev split (`train.setup`; 47,367 frames, 5.2 GB, 354 s; hand keypoints from the MediaPipe + SAM 3D Body teacher file on 38,004 of 43,753 crops, no Apple Vision data; [result](../probes/train-packer/RESULT.md))
- [x] Mac preflight: 64 samples, 20 steps, CoreML export (all 20 checks pass on MPS, both students export; [result](../probes/train-packer/RESULT.md))
- [ ] Licence check of every pretrained weight file (`probes/licence-check/RESULT.md`, 2026-11-02): 16 of 19 rows have a verdict; Apple Vision is blocked as the `perc.hand` teacher; Noa chose the best licence-clean alternative (2026-11-02). Missing: Meta Model API terms (SAM 3.1 API, Muse Image) and torchvision's weight terms for LR-ASPP MobileNetV3-Large
- [x] Decide D9 (proxy teacher), D10 (crops), D11 (temporal prior), D12 (backbone): D10 to D12 as packed (Noa yes, 11-02); D9 deferred, runs only if S4 misses and a proxy could fix the miss (it would not fix the misses found, [result](../probes/train-students/RESULT.md))
- [x] Students seg, hand and face on Modal (13.92 $ over two rounds), gated once per candidate on S4 against `budget.quality`: mouth passes (0.705); seg person/face/hand 0.77/0.80/0.80 live-like (gate 0.85), finger states 0.892 with pseudo finger labels (0.95; per finger 0.95 to 0.99, Apple Vision 0.962), face eye 0.24 (0.70). Models in `data/models/` ([result](../probes/train-students/RESULT.md))
- [x] Student hill-climb round 2 (leave-one-session-out validation, 11 levers): pseudo finger labels kept; seg levers void after a CUDA autocast bug, re-run self-prev equal to the control; no face lever helped
- [ ] Student hill-climb 3: seg without or with a stronger prev channel on clean LOSO runs, thumb and pinky labels, eye masks consistent across sessions (Apple Vision face landmarks as runtime eye source meanwhile)
- [x] H3: ANE latency of the trained students: seg 0.55 / 0.65 ms, hand 0.34 / 0.52 ms per crop, face 0.34 / 0.50 ms (median / p95); end-to-end frame bench in the perception session ([result](../probes/train-students/RESULT.md))

## Plugin (hill-climb line owns `plugin/`)

- [ ] Students in the plugin (CoreML from Rust), derived features (`feat.*`)
- [ ] Motion primitives, presets Pong and Orb, motifs
- [ ] Gestures and MIDI: finger states, chords, patterns, scale, transport
- [ ] Recording latency compensation (`rec.latency_comp`)
- [ ] Hardware probes P4 headphone motion, P5 heart rate, P6 MIDI out
- [ ] `setParamValue` root cause (bridge only, not blocking)
- [ ] UI in the glass language (last, after perception and routes are stable)
- [ ] Target picker: follow the plugin FL has focused, or pick mixer track then plugin; the assistant resolves only there (resolver top-1 0.676 with the plugin picked, 0.580 with 6 plugins, 0.538 with 30: `probes/open-vocab-mapping/picker.py`); FL side measured (`probes/fl-target-picker/RESULT.md`): track-then-slot works (inventory 0.4 ms, names 7-20 ms per plugin), follow-FL mode waits for a focus test with FL in front
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
- [x] FL bridge write test with third-party plugins while the song plays: REC writes reach Portal and ShaperBox in mixer slots, GUIs follow, Portal audible, FL records them as automation; ShaperBox keeps its curve amounts internal, Thermal does not load (`probes/fl-target-picker/RESULT.md`)
- [x] Full state lists for stepped parameters (session plugin-states, `probes/plugin-states/RESULT.md`): VST3 8,422 parameters in 403 plugins (107,112 states), AU 16,601 in 730 (159,483), plus dense scans of continuous parameters with stepped text. Formerly "992 parameters in 100 plugins have more states than the 11 harvested samples": the harvest already kept every step up to 64 steps; 334 VST3 and 799 AU parameters were truly incomplete, all but one per format complete now
- [ ] Serum 2 arp patterns: no parameter switches them; slots switch through `channels.midiNoteOn` with trigger note 0-11 on Serum's channel (decided 11-01, session plugin-states); the FL test has to pass before the demo
- [x] Parser error analysis (`probes/parser-errors/RESULT.md`): ß missing from the tokenizer vocabulary; 29 % of object labels, 75 % of momentary labels and 6-8 % of invert labels are not expressed by their sentence; text-supported scoring +4.8 points en, +4.9 de
- [x] Bundled retrain A/B/C (tok-v2, projected labels, word hash embedding): no step beyond seed noise, B kept, word hash dropped (`probes/parser-errors/RESULT.md` section 7)
- [x] Stage 4 commands (8,691 sentences, 6.71 $, 59 near-copies of the real set dropped) and parser v5: dev 0.837, challenge 0.781, dev_s4 0.736; +1.1 points against v3 on projected labels (p 0.018, no slice drops); exported `p-v5-s0`, int8 9.4 MB, 10 ms median (section 8)
- [x] Stage 5 commands shaped by calibration findings (4,476 sentences, 6.77 $, 125 near-copies dropped) and parser v6: real-sentence eval exact match 15 % -> 22 % against v5 (p 0.029, both seeds), route F1 0.26 -> 0.34, dev level at 0.83; exported `p-v6-s0`, int8 9.4 MB, 10 ms median (`probes/parser-errors/RESULT.md`, section 9). Last data run of this line
- [x] Parser v7: mmBERT small (German and English, pretrained) as encoder, trained on a Modal L4: real-sentence eval exact match 22 % -> 31 % against v6 (p 0.004, both seeds), right or one part wrong 50 % -> 68 %; teacher labels from dropped sentences did not help; exported `p-v7a-s0`, f16 121 MB, 30 ms median on one CPU thread (`probes/parser-errors/RESULT.md`, section 10). The plugin needs a runtime that loads the backbone
- [x] Parser v8 (`docs/session-2026-11-01-parser-v8.md`): four levers against the 40 % / 75 % goal, chosen on the synthetic dev sets, eval scored once. The ensemble of two v7a seeds ships (f16 2 x 121 MB, 62 ms median on one CPU thread): eval 37 % exact and 74 % right or one part wrong, but a second seed pair gets 29 % and 68 %, so the gain over v7a is about 2 points and not established (p 0.11). Beam, domain MLM on the encoder, clean DeepSeek teacher rows and mDeBERTa-v3-base all fail on eval; teacher rows cost 7.6 points (p 0.004). Goal not reached
- [x] Parser v9 (`docs/session-2026-11-01-parser-v9.md`): Noa's label conventions (pinch next to a movement is a hold, opposite directions are separate routes, fingers shown together are one finger state, switching words mean a switch) and stage 6 written by DeepSeek (4,785 sentences, 0.87 $). The v9 ensemble ships (f16 2 x 121 MB): 42 of 102 exact on the changed real set (v7a ensemble 37), route F1 0.537 against 0.425 (gain holds over two seeds), right or one part wrong 74 %. The set was seen while writing the conventions; the exact gain is not established (p 0.18)
- [x] Parser runtime in the plugin (`docs/parser-runtime-plan.md`): Rust crate `plugin/parser` (candle + Accelerate, f16 weights) runs the v8 ensemble; 200 of 200 dev frames equal to Python, 42 ms median and 81 ms p95 on one core, 287 MB RSS for both runs; the plugin loads it in the background and logs frames. Next: text field, resolver in Rust, route diff, chips
- [x] Runtime switched to the v9 ensemble (p-v9-s0 + p-v9-s1): 200 of 200 frames equal to Python on ds-v9c dev and on dev_s6; 39 ms median, 68 ms p95 on dev, 55 / 89 ms on stage 6; v7a stays selectable
- [x] Parser v10 (`docs/session-2026-11-02-parser-v10.md`): stage 7 says hand movements in their low direction and tells each target whether it rises or falls (4,638 sentences by DeepSeek, 0.87 $), plus Noa's conventions applied to the prediction. The v10 ensemble ships (f16 2 x 121 MB): 49 of 102 exact on the real set (v9 43, both after a gold fix for two derived rows), right or one part wrong 77 % (v9 74 %), eval gain over two seeds +12 points (p 0.003); calibration falls from 30 to 25 of 51, and "B warp" lost its target. The four-seed ensemble reads the real set best (56 of 102, 82 %; with the pronoun rule 85 of 102 and calibration 35 of 51) but shows no dev gain; making it cheap is a separate session. Next data levers (hand as subject, parameter name as the only target, one encoding for the fist): `docs/parser-v10-plan.md`.
- [x] Parser v11 (`docs/session-2026-11-02-parser-v11.md`): four data gaps closed: a side check that accepts the hand as subject, one label for closing the fist, a parameter name as the only target, direction hints on every fan-out target; stage 8 by DeepSeek (4,898 sentences, 0.77 $). By the dev rule the four-seed ensemble ships (f16 4 x 121 MB): 66 of 102 exact on the real set (v10 four-seed 56), 89 right or one part wrong (85), calibration 32 of 51 (35); over four seeds the eval gain is +15 points (p < 0.0001). New misses: MIDI patterns get a value (group 10), a later movement takes the side of the hand named before (calibration 20)
- [x] Parser v10 lean (`docs/session-2026-11-02-parser-v10-lean.md`): the four-seed ensemble distilled into one mmBERT-small student (ensemble KL plus 31,369 unused synthetic sentences labelled by the ensemble, 16k steps) and ggml Q8_0 weights. By the dev rule the student in Q8_0 replaces the ensemble (0.7706 against 0.7735, 69 MB against 482 MB), but on the real set it gets 44 of 102 exact against 56 (p 0.0075), like one seed. The four-seed ensemble in Q8_0 keeps 56 of 102 and 35 of 51 calibration at 277 MB; on v11 Q8_0 keeps 66 of 102 (34 of 51 calibration, one row less). Weight averaging fails (separate decoder starts), 12 encoder layers cost 2.2 points for 7 ms, Q4_0 costs 2 points. Noa chose the v11 four-seed ensemble in Q8_0 (66 of 102 exact, 277 MB of weights); in the Rust runtime the four v11 seeds take 101 ms median and 176 ms p95 per sentence on one core in f16, Q8_0 still to be built and timed, conv and pronoun rules still to be ported
- [x] Runtime on the v11 four-seed ensemble in Q8_0 (Noa's pick, 2026-11-02): encoders quantised at load, conv and pronoun rules ported to Rust; 805 of 805 dev frames equal to Python, 75 ms median and 135 ms p95 on 200 sentences, 343 MB RSS, 400 MB peak. v7a and v9 exports deleted
- [x] Noa reviewed the gold routes as diagrams (2026-11-01); 19 and 18 got alternatives, 28 was replaced by two opposite-direction sentences, 29 and 30 point to ShaperBox 3. Formerly: Noa checks the gold routes of the 34 originals (`data/mapping/eval/gold-routes.json`, `review.md`); the real-sentence numbers move with it. Chips stay the main path for long sentences: v6 gets about a fifth of them exactly right

## Demo and submission

- [x] FigJam updated for non-experts (10-31): status notes on the 8 planning diagrams, build plan coloured by status, new area "What six days taught us" (`docs/figjam-update-2026-10-31.md`)
- [ ] FigJam: update again after Noa's sentences, the retrain and the FL test
- [ ] Demo project in FL, 3-minute video with presets and behind the scenes (11-03)
- [ ] README with real request counts, repo public (11-03)
- [ ] Submission (11-04)
