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
- Omnisphere and FX-Omnisphere expose only "Param 0..511" with empty display strings (generic host-automation slots); no meaning can be read from them.
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

## Outside review

`uv run probes/mapping-pipeline/review.py --round 1 --max-usd 0.6`: three Spark Open reviews (data, architecture, coverage gaps), 0.14 $; findings and decisions in DESIGN.md.

## Spend

Ledger total at the end of the session (2026-10-29): 29.43 $ over 1,939 requests (10.23M prompt tokens of which 6.00M cached, 5.47M output tokens of which 3.36M reasoning). Largest items: commands stage 1 11.14 $, AU priority labels 8.23 $, VST3 labels 4.76 $, AU bulk labels 2.03 $, glossary 1.28 $.
