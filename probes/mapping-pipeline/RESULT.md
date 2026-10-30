# RESULT: mapping assistant build-time pipeline (2026-10-29, Muse Spark Open on Baseten)

Built with Muse Spark Open (`meta-models/muse-spark-open-1026`, Baseten Model API). All numbers below are measured on this machine (M1 Pro, macOS 27) or read from the Baseten responses; request and token counts come from `uv run tools/mapping/spark.py ledger` (every HTTP attempt is logged in `data/mapping/spark/ledger.jsonl`).

## Baseten facts (2026-10-29)

- Endpoint `https://inference.baseten.co/v1/chat/completions`. Prices from `GET /v1/models`: input 1.25 $/M, cached input 0.15 $/M, output 4.25 $/M (reasoning counts as output).
- Rate limits from response headers (probe): 100 requests/min, 2,000,000 tokens/min.
- Caching: the second identical request of the probe had 1,024 of 1,069 prompt tokens cached. Over all runs 3.26M of 5.37M prompt tokens came from the cache (61 %).
- Muse Spark Open does not enforce `json_schema` (Baseten docs); every reply is parsed and validated.
- Reasoning cannot be switched off. Output tokens per request by `reasoning_effort` (same kind of job):

| job | effort | reasoning tokens per request |
|---|---|---|
| label 80 lines | none / low | about 1,400 / 1,400 |
| commands 20 edits x 2 | not set | 14,100-17,100 |
| commands 20 edits x 2 | low | 41-540 |
| commands 20 edits x 2 | medium | 2,158-6,256 |

- Concurrency: the first client version held a lock around the HTTP call (3 requests/min). After the fix the label runs reached 56-63 requests/min with 12 workers and the adjudication run 113-115/min.

## Parameter harvest (AU)

`uv run tools/mapping/harvest/run_harvest.py [--retry-failed]` (own Swift tool on Apple's AU API, one subprocess per plugin, 60 s timeout).

- 906 AU components (types aumu, aufx, aumf, augn, aumi): 886 ok, 16 no parameters, 3 timeouts, 1 error; 122,664 parameters.
- First pass had only 312 ok: Waves, UADx, Analog Lab V and others print to stdout while loading and broke the JSON on stdout. Writing the JSON to a file (`--out`) fixed it.
- Omnisphere and FX-Omnisphere expose only "Param 0..511" with empty display strings (generic host-automation slots); no meaning can be read from them. The VST3 versions are the same: "Param 0..511" (display strings are just the slot number) plus 2,048 "MIDI CC <channel>|<cc>" parameters, 2,594 in total.
- First full harvest: 574 s wall time.

## Parameter harvest (VST3)

`bash tools/mapping/harvest/build_vst3.sh && uv run tools/mapping/harvest/run_vst3.py` (own C++ host on the Steinberg VST3 SDK 3.8.1, MIT since 3.8.0; SDK cloned to the git-ignored `data/mapping/vendor/vst3sdk`). Normalized value strings at t = 0, 0.1 ... 1, the same 0..1 scale FL uses.

- 549 bundles: 405 ok, 142 error (crash at load or licensing, e.g. Analog Lab V, ARP 2600 V3, several UADx), 1 without parameters, 1 timeout; 716,228 parameters (median 2,089 per plugin, most of them MIDI CC style slots); 167 s wall time.
- AU and VST3 parameter lists of the same plugin usually differ (Pro-Q 3: AU 358, VST3 517 parameters), so VST3 is labelled on its own.

## Parameter labels

`uv run tools/mapping/label.py --run <name> [--priority] --reasoning-effort low --workers 12 --max-usd <cap>`

| run | plugins | LLM lines | requests | $ | notes |
|---|---|---|---|---|---|
| label-pilot-none / -low | 9 | 719 | 11 / 11 | 0.146 / 0.114 | class agreement between the two: 94.9 % (682/719) |
| label-prio-v1 (A, B, adjudication) | 302 | 16,509 x 2 | 260 + 258 + 222 | 3.36 + 3.23 + 1.63 | A/B agree (class, direction, safe range within 0.1) on 14,111 / 16,509 = 85.5 %; 2,398 adjudicated |
| label-bulk-v1 | 583 | 11,719 | 127 | 2.03 | single pass |
| label-vst3-prio-v1 (A, B, adjudication) | 161 | 8,103 x 2 | 127 + 127 + 120 | 1.53 + 1.49 + 0.78 | 1,270 adjudicated (A/B agree 84.3 %) |
| label-vst3-bulk-v1 | 244 | 5,325 | 57 | 0.95 | single pass |

- Priority set: Noa's demo plugins (ZENOLOGY, Serum 2, Analog Lab V and all Arturia, Valhalla, FabFilter, Kilohearts, Portal, Yak Delay, BadChannel, imagiro piano, shearwater piano, Apple) plus the 426 plugins in his FL plugin database; 302 matched an AU harvest, 144 favorites are FL stock plugins or were not harvested.
- label-prio-v1 output: 58,566 labelled parameter rows (16,509 by Spark Open, the rest copies of identical names inside one plugin or of identical plugins of one vendor, and 8,057 rule labels such as MIDI CC and generic macro slots); 5,162 placeholder parameters skipped (unused, reserved, blank). A first write left 665 parameters unlabelled (Serum 2 mod-matrix and LFO slots, Augmented VOICES) because copy chains (plugin copy, then template copy) were followed in the wrong plugin; fixed in `label.py` (`_copy_chain`, unit test) and regenerated from cached replies at no cost. After the fix every non-placeholder parameter of both AU runs has a label.
- label-bulk-v1: 58,154 labelled rows, 270 unlabelled (placeholders plus 12 others).
- VST3 runs: 233,769 (priority) and 477,650 (bulk) labelled rows, almost all rule labels for MIDI CC style slots.
- Eye audit of 70 random Spark labels from label-prio-v1 (lead, 2026-10-29): 63 clearly right, 2 borderline, 5 wrong class (Haas time as delay time, chorus delay as mod depth, a mode switch as other, tape-echo treble as EQ gain, a boolean mode as select); 1 wrong direction (an EQ bandwidth with d=+1). Class accuracy 90 % strict (Wilson 95 %: 81-95 %), 93 % counting borderline as right.

## Glossary

`uv run tools/mapping/glossary.py --run gloss-v1 --reasoning-effort low --max-usd 5`: 65 requests, 1.28 $; 11,333 phrases for 416 concepts in English and German, 22-33 per concept and language (median 27), 212 phrases shared by two or more concepts (`ambiguous.json`).

## Synthetic commands, pilots

| run | effort | valid variants | $ / valid sentence | round-trip agreement (intent / sources / targets / invert) |
|---|---|---|---|---|
| cmd-pilot (v1 prompt) | not set | 78/80 | 0.0020 | not measured |
| cmd-v3-low | low | 118/160 = 73.8 % | 0.00043 | 0.856 / 0.593 / 0.585 / 0.754 |
| cmd-v3-med | medium | 143/160 = 89.4 % | 0.00079 | 0.909 / 0.559 / 0.587 / 0.755 |

Round-trip = Spark Open reads the plain sentence without the edit (`tools/mapping/roundtrip.py`). Reading of 14 mismatches: about half are generator faults (dropped sides, toggles phrased as amounts, comparative frames on unmap, odd glossary wording), the rest are checker or reader artifacts. Fixes for stage 1: deterministic side check, frames per intent, toggle wording rule, rank-weighted glossary wording at 0.35.

## Synthetic commands, stage 1

```
uv run tools/mapping/edits.py --run edits-s1 --n 8000 --seed 7 --glossary-run gloss-v1
uv run tools/mapping/commands.py --run cmd-s1 --edits-run edits-s1 --reasoning-effort medium --workers 12 --max-usd 14
uv run tools/mapping/keywords.py --run kw-s1 --edits-run edits-s1 --glossary-run gloss-v1
uv run tools/mapping/roundtrip.py --run rt-s1 --sentences-run cmd-s1 --max-usd 3   # every inverted sentence + 5 %
```

- 8,311 edits (8,000 plus minimal-pair twins), 16,622 requested variants, 13,128 valid = 79.0 % (428 requests, 11.14 $, 0.00085 $ per valid sentence). Drops: 2,800 missing marks or missing side word, 389 unmarked extra concepts (negative check), 132 leftover marks, 126 marked twice, 47 other.
- Valid sentences by intent: map 6,641, modify 1,438, unmap 968, replace 810, role 652, preset 612, query 537, clear 446, none 384, undo 330, scale 310; English 9,201, German 3,927.
- Template keyword sentences: 398 (only map edits with at most two routes and no modifiers besides range qualify).
- Round-trip on 2,126 sentences (every inverted one plus 5 %), 88 requests, 0.91 $: agreement intent 0.933, routes 0.917, invert 0.849, sources 0.763, targets 0.656. 1,123 sentences have at least one mismatch: targets 554 (356 of them targets only), sources 326, routes 177, invert 145, intent 143. Targets-only mismatches are mostly lay wording read as a neighbouring class ("dreamier" as reverb vs delay); dropping them all would strip the lay sentences Noa asked for, so the training set drops intent, invert and source failures and keeps targets-only ones for a manual look.
- Full round-trip over all stage-1 sentences (`roundtrip.py --run rt-s1-all --sentences-run cmd-s1 --all --max-usd 10`, 538 requests, about 5.1 $): 13,103 read back; agreement intent 0.888, routes 0.860, invert 0.842, sources 0.752, targets 0.691. 6,425 sentences (49 %) match on every field; 8,003 (61 %) have no intent, invert, source or route mismatch (kept per register: casual 2,112 of 3,447, beginner 1,150 of 1,849, terse 1,164 of 1,914, layperson 944 of 1,561, jargon 898 of 1,509, codeswitch 854 of 1,408, rambling 475 of 761, polite 406 of 654). Mismatches mix generator faults and reader faults; which share helps training is an open measurement (filtered 8,003 vs all 13,128, same step budget).

## Training speed on the Mac

`uv run probes/mapping-pipeline/train_bench.py --device mps|cpu` (random data, DESIGN.md parser shape: vocab 8k, encoder 6 x d256, decoder 2 x d256 with pointer scores, BIO head; batch 64, source 48 tokens, frame 24 tokens; AdamW): 9.20M parameters, MPS 96.0 / 104.8 ms per step (median / p95), CPU 452.5 / 476.2 ms. At 40k training samples that is about 1 minute per epoch on MPS.

## Outside review

`uv run probes/mapping-pipeline/review.py --round 1 --max-usd 0.6`: three Spark Open reviews (data, architecture, coverage gaps), 0.14 $; findings and decisions in DESIGN.md.

## Spend

Ledger total at the end of the session (2026-10-29): 34.57 $ over 2,560 requests (11.62M prompt tokens of which 6.97M cached, 6.52M output tokens of which 3.92M reasoning). Largest items: commands stage 1 11.14 $, AU priority labels 8.23 $, round-trip checks 6.2 $, VST3 labels 4.76 $, AU bulk labels 2.03 $, glossary 1.28 $.

# RESULT: overnight training on the Mac (2026-10-30)

Code in `tools/mapping/` (commit 9e64d3a); data and runs in the git-ignored `data/mapping/train/`. The queue `caffeinate -dimsu zsh tools/mapping/run_overnight.sh` ran from 00:19 to 04:45, eleven steps, all rc=0. Full table with every run: `data/mapping/train/REPORT.md` (`uv run tools/mapping/report.py`). Accuracy below is route_edit (intent, routes and route-free items equal as multisets) with Wilson 95 % intervals unless marked otherwise.

## Data

```
uv run tools/mapping/challenge.py            # 300 edits, edits.py --seed 99, prompts/challenge.md
uv run tools/mapping/dataset.py --out ds-v1 --seed 13
uv run tools/mapping/tokenizer.py
```

- Challenge set: 300 new edits, 612 requested variants, 548 valid (89.5 %), 0.408 $. Not round-trip checked, never in training.
- ds-v1: train_all 20,012 sentences (6,093 edits, 7,808 of them glossary span substitutions), train_filtered 12,993 (3,843 edits, 5,397 substitutions). The filter keeps a sentence only if its full round-trip (rt-s1-all) has no mismatch outside targets; it reproduces the 8,003 kept stage-1 sentences. Dev 805 sentences from 418 edits (10 % of edit ids, twins with their original, clean sentences only, no substitutions). Frame length max 35, mean 8.9 tokens.
- Tokenizer tok-v1: SentencePiece unigram 8k, byte fallback, nmt_nfkc_cf, split digits; trained on 20,012 training sentences, 11,333 glossary phrases and 56,680 harvested plugin and parameter names. 18.6 pieces per training sentence on average, max 84 (encoder limit 96).

## param-sense v1

`uv run tools/mapping/train_paramsense.py --name ps-v1 --epochs 20` (0.80M parameters, CPU, 0.7 min). 787,756 labelled rows, capped at 60 per plugin and class: 78,846. Vendor split: train 38 vendors / 64,954 rows, val 6 / 3,020, test 19 / 10,872. Best epoch by val: 5.

| test (unseen vendors) | top-1 | macro F1 | F1 unsafe | F1 ctl.switch | F1 ctl.select | F1 system | direction acc |
|---|---|---|---|---|---|---|---|
| runtime-only features | 0.728 | 0.511 | 0.357 | 0.654 | 0.550 | 0.958 | 0.993 |
| with value strings | 0.741 | 0.512 | 0.305 | 0.761 | 0.577 | 0.948 | 0.992 |

- Val (6 vendors) gives top-1 0.485 against 0.728 on test (19 vendors): the per-vendor spread is large, so one number per split says little. unsafe is the weakest class.
- Known-plugin table: 63,625 rows from the priority runs (not rule-labelled), `runs/ps-v1/known_plugins.json`; weights f16 1.6 MB.

## Parser v1

`uv run tools/mapping/train_parser.py --name <run> --train train_all|train_filtered [--frac F] [--seed S] --steps 16000`. 9.30M parameters (encoder 6 x d256, pointer decoder 2 x d256, BIO head), batch 64, AdamW 5e-4 with warmup 600 and cosine to 5 %, weight EMA 0.999, grammar-constrained greedy decoding, best of raw and EMA weights by dev. 16,000 steps take 33 min on MPS (about 124 ms per step including six dev evaluations). A 64-sample overfit check reached 64/64 exact; no pointer span was lost in train, dev or challenge.

| run | train | samples | dev | challenge | challenge exact |
|---|---|---|---|---|---|
| p-filt-100-s0 | filtered | 12,993 | 0.647 [0.614, 0.679] | 0.516 [0.475, 0.558] | 0.509 |
| p-all-100-s0 | all | 20,012 | 0.697 [0.664, 0.728] | 0.628 [0.587, 0.667] | 0.617 |
| p-all-100-s1 | all | 20,012 | 0.684 [0.652, 0.716] | 0.635 [0.594, 0.674] | 0.630 |
| p-all-100-s2 | all | 20,012 | 0.694 [0.662, 0.725] | 0.620 [0.579, 0.660] | 0.606 |
| p-all-50-s0 | all, 50 % of edits | 9,889 | 0.554 [0.519, 0.588] | 0.494 [0.453, 0.536] | 0.491 |
| p-all-25-s0 | all, 25 % of edits | 4,915 | 0.399 [0.365, 0.433] | 0.338 [0.299, 0.378] | 0.338 |
| p-all-100-s0-long | all, 32,000 steps | 20,012 | 0.709 [0.677, 0.740] | 0.639 [0.598, 0.678] | 0.630 |

- Seeds 0/1/2 at 100 %: dev 0.692 (sd 0.007), challenge 0.628 (sd 0.007).
- Filter ablation: the unfiltered set wins by 11 points on the challenge set (0.628 vs 0.516, intervals do not overlap) and 5 points on the clean dev set. The filtered set also lies below the learning curve of the unfiltered set: log-linear between the 50 % and 100 % points, 12,993 sentences would give about 0.55 on the challenge set (interpolated, not measured), the filtered run reached 0.516. The sentences the round-trip drops help the parser more than they hurt it.
- Learning curve: each doubling of training edits adds 15.6 points (25 → 50 %) and 13.4 points (50 → 100 %) on the challenge set. No flattening yet.
- Steps: twice the steps (32,000) adds 1.1 points on the challenge set, inside 1.5 seed sd; its best dev checkpoint was step 16,000. More steps do not help at this data size.
- Per field (p-all-100-s0, dev / challenge): intent 0.983 / 0.942, sources 0.899 / 0.880, targets 0.821 / 0.829, invert 0.866 / 0.838, scope 0.929 / 0.898.
- Errors by number of routes in the gold frame (p-all-100-s0, dev / challenge): 0 routes 0.94 / 0.84, 1 route 0.73 / 0.63, 2 routes 0.35 / 0.38, 3 or more 0.07 / 0.00 (29 / 23 sentences). Of the 204 failed challenge sentences, 94 have a wrong target, 89 a wrong invert, 66 wrong sources; 40 fail on targets alone. By intent on the challenge set: map 0.53 (295), modify 0.38 (56), clear 0.59 (22); the rest 0.84-1.00. German 0.57 (185) vs English 0.66 (363); layperson 0.53 is the weakest register.
- Confidence (frame log-prob) ranks well: the most confident 50 % of challenge sentences are 0.898 correct, the most confident 30 % 0.963. Coverage at 95 % accuracy: 0.33 on the challenge set, 0.48 on dev. The threshold for the chip fallback still has to come from Noa's 50 calibration sentences.

## Export and latency

`uv run tools/mapping/export_parser.py --run p-all-100-s0` (M1 Pro, CPU, torch 2.15.0, batch 1, tokenize + encode + constrained greedy decode, 200 dev and challenge sentences after 10 warm-up runs).

| threads | median ms | p95 ms |
|---|---|---|
| 1 | 9.03 | 21.51 |
| 4 | 11.63 | 27.74 |

- Four threads are slower than one at batch 1; the plugin should run the parser on one thread.
- Weight blobs for Rust: `weights.f16.bin` 18.6 MB, `weights.int8.bin` 9.4 MB (per-row symmetric int8 for 2-D weights with f16 row scales, f16 for the rest); tensor tables with name, shape, dtype, offset and byte count in `export.json`. f32, f16 and int8 weights give the same dev and challenge accuracy to four decimals.
- RSS of the Python process: 217.5 MB after importing torch, 296.9 MB after loading model and data, 349.0 MB peak during inference. A Rust runtime will need far less; not measured.
- Label audit: the parser trained on train_all reproduces 99.97 % of the round-trip-kept and 100 % of the dropped training sentences, so the audit only shows memorisation. It needs a model that has not seen the audited half (cross-fitting); `audit_suspects.jsonl` is nearly empty.

## Stage-2 decision

Stage 2 on Baseten is worth it. Data is the bottleneck: +13 to +16 points per doubling of edits against +1 point for doubling the steps. Rules for stage 2:

- Use every validated sentence (train_all); the round-trip filter costs 11 points and is no longer needed for training, which saves its 5 $ per 13k sentences.
- Weight the edit sampler towards what fails: 2 and 3 routes per sentence, invert, modify, map with modifiers, German and layperson wording.
- Size: about as many edits again as stage 1 (8,000). At the stage-1 rate of 0.00085 $ per valid sentence that is about 11 $ (estimate from stage 1, not measured); the ledger stands at 34.98 $.
- Then the same 16,000 steps on the doubled set, plus one seed, to see whether the curve keeps its slope.
