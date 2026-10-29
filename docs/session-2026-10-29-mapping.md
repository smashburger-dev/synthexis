# Session 2026-10-29: mapping assistant, build-time pipeline

Entry point for the next mapping session. Measured numbers and commands: `probes/mapping-pipeline/RESULT.md`. Design and decisions: `probes/mapping-pipeline/DESIGN.md`. Spec changes for the main session: `probes/mapping-pipeline/SPEC_PROPOSAL.md`. Built with Muse Spark Open.

## Results

- Vocabulary: `tools/mapping/ontology.yaml` has 44 parameter classes, 42 sources, 12 finger states, 7 target kinds (incl. effect on/off toggles), 11 intents, route modifiers (invert, range, curve, amount, smoothing, hold, quantize, delta, gate, ramp time), references to earlier routes and self-corrections. Polarity convention: a source id names the state that reads 1 (hand.fist = 1 - hand.openness).
- Harvest with own tools, build time only: AU via Apple's AU API, 886 of 906 plugins, 122,664 parameters (first pass only 312: plugins printing to stdout broke the JSON). VST3 via the Steinberg SDK (MIT since 3.8.0), 405 of 549 bundles, 716,228 parameters, most of them MIDI CC style slots that rule labels take. Omnisphere exposes only "Param 0..511" in the AU.
- Labels (class, direction, safe range): 302 priority plugins (Noa's demo list plus his FL plugin database) labelled twice and adjudicated (A/B agreement 85.5 %, 2,398 adjudicated, 8.23 $); 583 other AU plugins single pass (2.03 $). Eye audit of 70 random priority labels: 90 % class-correct (Wilson 81-95 %). A copy-chain bug had left 665 Serum 2 and Augmented VOICES parameters unlabelled; fixed, regenerated from cache at no cost. VST3 labels run separately (`label-vst3-prio-v1`, `label-vst3-bulk-v1`).
- Glossary: 11,333 English and German phrases for 416 concepts, at least a third lay wording on request ("dreamier", "squeeze my hand"), Denglisch and typos included (1.28 $).
- Synthetic commands: generator v3 (attribute-conditioned edits with sentence frames, registers incl. layperson and codeswitch, minimal pairs, glossary wording, template keyword sentences, mark validator, side check, negative check against unmarked extra concepts, blind round-trip check). Medium reasoning: 89.4 % valid in the pilot at 0.00079 $ per valid sentence; low reasoning 73.8 %. Stage 1: 8,000 edits, see RESULT.md for the final counts.
- Outside review by Spark Open (three briefs, 0.14 $) found 40 everyday sentences the first grammar could not express; all are covered now (refs, deltas, toggles, gates, ramp times, corrections, keyword soup, codeswitch).
- 2026 papers folded in: Ventirozos et al. (arXiv 2610.35102, syntax-conditioned synthetic queries, dropped constraints), Shen and Zhang (EMNLP 2026, confusable neighbours), TOMORI (arXiv 2610.25344), plus Meta's span pointer networks (2021) as the parser formulation.
- Engineering fixes: the Baseten client held a lock around the HTTP call (3 requests/min); after the fix 56-133/min. Reasoning cannot be switched off; batches stay large (80-120 parameters, 40 sentences per request).

## Decisions

- Runtime stays offline: parser (about 8M parameters, encoder plus span-pointer frame decoder with grammar constraints, int8 about 8 MB) plus param-sense (about 0.8M) plus a shipped known-plugin table for the priority plugins. Low-confidence sentences show clickable terms instead of an edit; the threshold is calibrated on Noa's sentences, never on synthetic ones.
- UI answer to Noa: the plugin does not wrap the other plugin's knob. An accepted edit becomes a route in the synthexis route matrix with a depth ring knob; synthexis writes the target parameter through the macro link or the FL bridge. Unsure sentences show the terms as chips; clicking one lists candidates from the open project; picking one creates the route.
- Commands use medium reasoning; labels low; adjudication low.

## Open

1. Noa: 50 calibration sentences and 100 eval sentences, typed as he would (English, German or mixed), into `data/mapping/eval/calibration.txt` and `data/mapping/eval/eval.txt` (git-ignored). Never used for training or prompt tuning.
2. Noa, about 10 min: FL stock plugin dump (`tools/mapping/fl/README.md`), then `import_fl_dump.py` and `label.py --harvest fl --priority`.
3. Omnisphere: no parameter meaning in the AU; check the VST3 names in FL; otherwise MIDI CC or Omnisphere's own host-automation assignment.
4. Training (next session, Mac CPU/MPS, no cloud needed): tokenizer, parser v1, param-sense v1, challenge set, learning curve on stage-1 data to decide whether stage 2 is worth buying.
5. Plugin integration (resolver, chips, credit) with the hill-climb line that owns `plugin/`.
6. The main session merges SPEC_PROPOSAL.md into the spec.

## Cost

Baseten ledger at the end of the session: 29.43 $ over 1,939 requests (details in RESULT.md, Spend); about 70 $ of the 100 $ credit left. Every request is logged in `data/mapping/spark/ledger.jsonl`; `uv run tools/mapping/spark.py ledger` prints requests, tokens and $ per run for README and demo.
