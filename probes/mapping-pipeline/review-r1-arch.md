 Up to 12 blockers, concrete file+block, what fails in demo and minimal fix. No restatement.

**1. [Blocker] `DESIGN.md` runtime 1 parser + `ontology.yaml` sources polarity pairs**
Problem: you emit two ids for one sensor: `hand.openness/hand.fist` and `eye.open/eye.closed` with `pair:`. `edits.yaml:sources` samples both at weight 9/1.5. Parser must split lexical evidence "fist" vs "open hand" into two classes that resolver collapses to `hand.openness+invert`. Training examples halved per form, decoder can emit either id for same span, and `i1` invert doubles: `hand.fist+i1` == `hand.openness`. Demo: "fist inverted" produces double-flip, wrong direction.
Fix: delete pair ids from ontology. Keep single `hand.openness`, `eye.open`. Teach polarity via surface form in glossary, not ontology id. Resolver normalizes to one signal.

**2. [Blocker] `DESIGN.md` runtime 1 frame grammar - route brackets, order**
Problem: 1 sentence = up to 3 routes x 3 targets (`edits.yaml:routes_per_edit/targets_per_route`). No canonical order defined. Constrained decoder penalized for emitting `t2,t1` vs `t1,t2` though both correct. Exact-match accuracy and log-prob calibration break on shared scope example "fist controls reverb and delay on the pad".
Fix: define grammar order: sort routes by `(source_id, side, appearance_index)` and merge same `source+scope` into multi-target route. Train and score only in that order; enforce in constrained decode mask.

**3. [High] `DESIGN.md` runtime 1 span pointers + `ontology.yaml` modifiers + `prompts/commands.md` marks**
Problem: `range/lo1/hi1`, `amount`, `curve` marked as spans `[[20|lo1]]` but parser must output normalized value: "half"->50, "up to half"->`[0,50]`, `between 20 and 80`. `edits.yaml:range_shapes` defines shapes. 2x d256 decoder copying surface string cannot normalize. At runtime resolver gets span "half" not `0.5`.
Fix: split modifier into closed-vocab value token (quantized 0,5..100, `amount_values`, `curve_values`) + optional span pointer for provenance. Predict value token, copy span only for audit.

**4. [High] `ontology.yaml` taxonomy `safe` vs `prompts/label.md` lo/hi + `DESIGN.md` resolver safe range**
Problem: ontology hardcodes class safe e.g. `filter.resonance [0,0.8]`, `fx.delay.feedback [0,0.85]`. `label.py` asks Spark for per-parameter `lo,hi` from `values` display. Resolver spec says "apply direction d and the safe range" - ambiguous which. Class safe will still allow self-oscillation on plugins where 0.8 oscillates; per-param table missing for unknown plugins.
Fix: ship per-param safe only for known-plugin table. For unknown, train `param-sense` to predict safe-cap bucket and take `min(class_safe, predicted_cap)`.

**5. [High] `DESIGN.md` runtime 2 selective prediction + evaluation synthetic dev**
Problem: confidence = frame log-prob calibrated on dev that is held-out edits from same `edits.py`+glossary distribution. Demo inputs are Denglisch code-switch, typos, custom channels `tools/mapping/edits.yaml:tracks:custom`. Log-prob overconfident on synthetic, threshold for target `>=0.95` precision on accepted will be too low for humans -> high coverage, low precision in demo.
Fix: calibrate threshold on human data only. Split Noa's 100 50/50 calib/test or collect 50 pilot human sentences. Report precision-coverage curve on shift; never set threshold on synthetic.

**6. [Blocker] `DESIGN.md` evaluation: Noa's 100 hand-written commands**
Problem: final gate `n=100` gives binomial Wilson interval ±8-9% at 0.90. 90/100 pass could be true 80%. Spec `map.eval >=0.90 overall` can be gamed with low coverage selective prediction. Single author bias, only demo plugins.
Fix: require lower Wilson bound >=0.90 (~96/100) or expand to 200 stratified by `intents` distribution in `edits.yaml`. Evaluate joint parser+resolver route diff, not parser alone, with coverage reported.

**7. [High] `DESIGN.md` evaluation: challenge set ~500 from Spark different prompt**
Problem: still Spark-generated, English-biased, no real FL project context. Never measures fuzzy track/plugin matching or code-switch "Fist steuert Reverb auf Pad". Picking architecture/data size on this will select model that copies Spark phrasing.
Fix: generate challenge set from held-out plugin manufacturers and held-out human paraphrases of edits, not Spark rephrasing. Include `fl_plugins` + `custom` channel names as noise.

**8. [High] `DESIGN.md` runtime 3 resolver candidates + `tools/mapping/ontology.yaml` taxonomy + `DESIGN.md` param-sense**
Problem: `candidates = loaded params whose class matches; order: known-table, then param-sense prob; filter by track/plugin fuzzy`. If `param-sense` top-1 wrong (expected on unknown vendors), correct param is filtered out entirely before fuzzy match. Correct parse still yields wrong route/no route. No fallback.
Fix: hard filter -> soft rank. Keep params whose class in top-2 `param-sense` (or prob >0.2) before filtering. Score = `p(class)*fuzzy(track)*fuzzy(plugin)*type_fit`.

**9. [High] `DESIGN.md` runtime 3 resolver fuzzy match + `edits.yaml` scope + `ontology.yaml` tracks**
Problem: `scope` none 30% trains parser to omit track. Real project has `Synth 1`, `Pad Layer`, `my pad` vs role `pad`. Spec says "fuzzy match on channel and plugin names; prefer plugin type that fits" without threshold. "on the pad" fuzzy-matches three channels equally -> wrong route or chip explosion. `scope:none` with multiple candidate tracks should never auto-pick.
Fix: define fuzzy = normalized Levenshtein >=0.85 on lowercased alphanum, exact token match for `tracks:roles`. If `scope==none` and >1 loaded track hosts candidate class, force `map.fallback` chip.

**10. [High] `tools/mapping/edits.yaml` languages 60/40 + `DESIGN.md` model sizes from-scratch 8k SentencePiece**
Problem: from-scratch SentencePiece 8k EN+DE built on synthetic Spark German with English terms inserted. Real Denglisch interleaves mid-sentence: "Mund auf für Filter". Byte fallback fragments `Reverb` inside German, span pointers `[i,j]` off by one, auxiliary BIO head misaligned. 8M model has no capacity to recover.
Fix: build vocab from harvested `plugins.jsonl` names + glossary before synthesis, ensure code-switch edits (de intent with en plugin names). Validate tokenizer span alignment on challenge set.

**11. [Blocker] `DESIGN.md` model sizes pretrained option**
Problem: `parser, pretrained: 6 layers MiniLM H384, vocab pruned 10k, ~14M, 14/28MB, <15ms`. Noa allows ~10M. Even if it wins on challenge set you cannot ship; weight licence for redistribution untested, violates 6-day/90USD budget if you retrain.
Fix: remove pretrained option from decision. Keep as research candidate only. Cap shipped model at 10M int8, 8MB; if needed use distilled 4-layer encoder, not 6xH384.

**12. [Medium] `DESIGN.md` runtime 5 param-sense inputs + build time labels**
Problem: training uses `group, unit, value strings as optional inputs dropped at random`. FL runtime exposes only `plugin name + param name + other names` (`DESIGN.md:5`). Model learns to rely on leaked `group/unit` that never exists for unknown plugins. Vendor holdout `top-1 against Spark labels` rewards leakage.
Fix: train with strict runtime features only for unknown branch; evaluate `param-sense` on human labels for vendor holdout, report per-class F1 for `unsafe/ctl.switch/ctl.select/system` which gate safety.