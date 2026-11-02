# Parser runtime in the plugin (2026-11-01)

Goal: the plugin turns a typed sentence into a route frame offline, with the shipped parser. The runtime was built and chosen on the v8 ensemble (p-v7a-s0 + p-v7a-s1, `docs/session-2026-11-01-parser-v8.md`); on 2026-11-01 evening it moved to the v9 ensemble, and since 2026-11-02 the plugin runs the v11 four-seed ensemble with Q8_0 encoders and the conv,pronoun rules (Noa's pick), see "Parser v11 in Q8_0". This file lists what is missing between "sentence typed in the plugin" and "routes applied", and what this session builds.

Sources read: `spec/synthexis.yaml` (map.parser, map.resolver, map.fallback, map.agent, map.param_sense), `probes/mapping-pipeline/SPEC_PROPOSAL.md` (map.parser.model amend, map.resolver amend, map.aliases, map.live_inventory), `probes/mapping-pipeline/DESIGN.md` ("Training interfaces"), `docs/roadmap.md` (mapping assistant), `plugin/synthexis/src/` (no parser, resolver or text input exists yet).

## Chain and gaps

| Step | What it does | State before this session | This session |
|---|---|---|---|
| Text input | text field in the editor, Enter sends the sentence to a worker thread | missing (`editor.rs` has no text field) | minimal entry only: `assistant::submit`, `SYNTHEXIS_PARSE` env, frame logged |
| Tokenizer | HF `tokenizer.json` of mmBERT small de-en, byte offsets per piece | Python only (`frame_parser.HFTok`) | built: `tokenizers` crate reads the same file |
| Encoder | ModernBERT, 22 layers, width 384, local/global attention, RoPE | Python only (transformers) | built in Rust (`plugin/parser/src/encoder.rs`), f16 weights |
| Frame decoder + grammar | 2-layer decoder, pointer head, grammar mask, greedy over the ensemble | Python only (`Parser.decode`, `Grammar`, `beam` k=1) | built in Rust (`decoder.rs`, `grammar.rs`), 200/200 frames equal |
| Weight export | f16 state dicts to a format Rust reads | `model.f16.pt` (pickle) | built: `tools/mapping/export_runtime.py` writes safetensors + `parser.json` |
| Load once | background load at plugin init, never on the audio thread | missing | built: `plugin/synthexis/src/assistant.rs`, one worker per process, started in `initialize()` |
| Confidence | frame log-prob, threshold below which the plugin shows chips instead of a diff | log-prob exists; threshold not calibrated on human sentences | log-prob returned; threshold stays open |
| BIO head output | target text for the resolver = first to last TGT piece | Python only | open (head weights are exported, not run) |
| Resolver | frame + live parameter list -> concrete plugin parameters: alias > name match on the named plugin > param-sense class > chips | Python probe (`probes/open-vocab-mapping/resolve.py`); spec amend proposed | open |
| param-sense | parameter name -> taxonomy class, hashed n-grams, < 1M params | trained in Python, known-plugin table exported | open (Rust port, small) |
| Live inventory | plugin and parameter names of the project from the FL bridge | target picker in session fl-target-picker | open |
| Route diff UI | show proposed routes in the route matrix, accept, edit, undo | missing | open |
| Chips fallback | sentence with recognised terms as chips, click lists options | missing (glossary exists) | open |
| Frame -> `route.rs` | turn a resolved frame into `Route` structs with modifiers (invert, range, curve, gate) | `route.rs` has routes, no builder from frames | open |

## Runtime choice

Constraints: CPU in a DAW plugin on macOS arm64, weights loaded once in the background, about 250 MB for two f16 models, target under 150 ms per sentence.

Chosen: candle 0.11 (pure Rust) with Apple Accelerate for the f32 matrix products, crate `plugin/parser` (`synthexis-parser`). Weights stay f16 in RAM and each encoder matrix is widened to f32 per call with half's SIMD conversion; the decoder (5M weights, up to 48 steps) is kept in f32. Reasons, all measured below: exact frames against Python, faster than ONNX Runtime on the encoder, no 33 MB libonnxruntime.dylib to bundle and sign, and f16 storage without f16 arithmetic.

## Measurements

Runtime choice, measured on the v7a ensemble. M1 Pro, one core (CPU time equals wall time; `VECLIB_MAXIMUM_THREADS=1` changes nothing), 200 synthetic dev sentences (`ds-v6p/dev.jsonl`, first 200), release build. Reference: `uv run tools/mapping/predict.py --run p-v7a-s0,p-v7a-s1 --f16` (shipped f16 weights computed in f32). Tool: `plugin/target/release/synthexis-parse`.

| Variant | Frames equal | Max conf diff | Median | p95 | RSS after load |
|---|---|---|---|---|---|
| candle, gemm crate, f16 arithmetic | 199/200 | 0.044 | 99 ms | 191 ms | 257 MB |
| candle, gemm crate, f32 weights | 200/200 | 0.000 | 129 ms | 244 ms | 536 MB |
| candle + Accelerate, f16 stored, scalar widening | 200/200 | 0.000 | 63 ms | 118 ms | 291 MB |
| + SIMD widening (half) | 200/200 | 0.000 | 48 ms | 101 ms | 292 MB |
| + GELU with polynomial erf (shipped) | 200/200 | 0.000 | 42 ms | 81 ms | 287 MB |
| shipped, f32 weights (`--f32`) | 200/200 | | 37 ms | 74 ms | 551 MB |
| shipped, single run p-v7a-s0 (fallback) | 200/200 vs Python s0 | | 20 ms | 38 ms | 162 MB |

- Shipped variant: load 95 to 230 ms, peak memory footprint 323 MB for the whole process (`/usr/bin/time -l`). Against the Python f32 weights (`model.pt`) the frames are also 200/200 equal, confidence within 0.004.
- Of a parse, the two encoders take 31 ms median (15.7 ms each, p95 26.5), the decode loop 10 ms (p95 29).
- The first build read each safetensors file into one buffer; macOS malloc kept the freed 120 MB as "Malloc Large (empty)" and the footprint was 423 MB. Reading tensor by tensor into their own buffers fixed it.
- The gemm crate's f16 kernels use NEON fp16 FMAs (f16 accumulation), which moved one frame; it also repacks every weight matrix on every call.
- ONNX Runtime 1.31.1 through Python (same C library as the `ort` crate), encoder of p-v7a-s0 only, one intra-op thread (`probes/parser-runtime/ort_probe.py`): f32 model 22.6 ms median, p95 66.2, max abs diff to torch 3e-5; f16 model 24.1 ms median, p95 66.4, max abs diff 0.027. candle + Accelerate: 15.7 ms median, p95 26.5, exact. The f16 ONNX export also needed RoPE and mask casts kept in f32 to load.
- In the plugin (standalone, camera engine running): load 228 ms, a two-route German sentence in 106 ms, frame in `~/Library/Logs/synthexis.log`.

### Parser v9 in the runtime

Same recipe as v7a (backbone, decoder, grammar), trained on ds-v9c; the frame vocab grows from 340 to 342 tokens (`shape:m00111`, `shape:m10101`), which the grammar handles without code changes. Reference `predict.py --run p-v9-s0,p-v9-s1 --f16`, first 200 rows of each set, same machine and tool.

| Set | Runs | Frames equal | Max conf diff | Median | p95 | Max | Encoders median / p95 | Decode median / p95 |
|---|---|---|---|---|---|---|---|---|
| ds-v9c dev | p-v9-s0 + s1 | 200/200 | 0.000 | 39.0 ms | 67.8 ms | 87.9 ms | 29.9 / 40.6 ms | 8.9 / 27.1 ms |
| ds-v9c dev_s6 | p-v9-s0 + s1 | 200/200 | 0.000 | 54.7 ms | 89.0 ms | 159.9 ms | 38.6 / 51.6 ms | 17.6 / 41.3 ms |
| ds-v9c dev | p-v9-s0 (fallback) | 200/200 | 0.001 | 18.8 ms | 34.3 ms | 85.4 ms | 15.1 / 24.7 ms | 3.8 / 10.7 ms |

- RSS after load 293 MB for the ensemble (164 MB for s0), peak footprint 324 MB, load 87 to 106 ms (219 ms in the plugin).
- The stage-6 sentences are longer and carry more routes, so both encoder and decode take longer; one sentence of 200 crossed 150 ms.
- In the plugin: `p-v9-s0,p-v9-s1 loaded in 219 ms`, a two-route English sentence in 78 ms.
- Superseded on 2026-11-02; the v7a and v9 exports and their parity tests are deleted.

### Parser v11 in Q8_0 (2026-11-02)

Noa chose the v11 four-seed ensemble (p-v11-s0 to s3) in Q8_0 after the v10 lean session (`docs/session-2026-11-02-parser-v10-lean.md` on claude/parser-v10-lean-76d6f3; v11 on claude/parser-v11-de5d19): 66 of 102 real sentences exact in f16 and in Q8_0. Runtime changes:

- Q8_0 at load: every 2-d `hf.*` tensor (encoder matrices and embedding, 54.8M weights per run) is quantised with candle's `QTensor::quantize` while the safetensors file is read, so a run's f16 copy never sits next to its quantised one. Per call a matrix is dequantised to f32 and multiplied with Accelerate's sgemm, as the f16 path widens. The decoder stays in f32 (5M weights per run, 21 MB).
- Decode-time rules after decoding, ported from `tools/mapping/rules.py` (`plugin/parser/src/rules.rs`): `conv` (pinch next to a movement is a hold, fingers shown together become a finger state, fist folds into inverted openness, closed eyes into inverted open eyes), `conv10` (conv without the fold, as v10 was scored), `pinch`, `pronoun`.
- `export_runtime.py --rules conv,pronoun --q8` writes both settings into each run's `parser.json`; the runtime follows them unless `Options` overrides.

Parity on all 805 rows of ds-v11c dev against `predict.py --rules conv,pronoun` (branch claude/parser-v11-de5d19):

| Runtime | Reference | Frames equal | Max conf diff | route_edit dev (both) |
|---|---|---|---|---|
| v11 four seeds, Q8_0 (shipped) | torch on `p-v11-sN-q8` (candle's Q8_0 rounding simulated) | 805/805 | 0.003 | 0.8820 |
| v11 four seeds, f16 | torch on `p-v11-sN` (f32) | 805/805 | 0.005 | 0.8820 |
| v10 four seeds, f16, conv10,pronoun | torch on `p-v10-sN` | 805/805 (ds-v10c) | 0.002 | 0.8882 |
| student lean-kd-p16, f16, conv10,pronoun | torch on `lean-kd-p16` | 805/805 (ds-v10c) | 0.004 | 0.8870 |

Dequantising instead of candle's quantised matmul is why Q8_0 matches exactly: that kernel also quantises the activations.

Latency and memory, four v11 runs, one thread, first 200 rows of ds-v11c dev, measured back to back:

| Weights | Median | p95 | Max | RSS after parsing | Peak footprint |
|---|---|---|---|---|---|
| Q8_0, dequantised per call (shipped) | 75.1 ms | 134.9 ms | 179.9 ms | 343 MB | 399 to 402 MB |
| f16, widened per call | 78.4 ms | 152.2 ms | 223.4 ms | 539 MB | 578 MB |
| Q8_0, candle's quantised matmul (dropped) | 68.9 ms | 177.9 ms | 490.6 ms | 534 MB | 570 MB |

- The machine was loaded by other sessions while these ran (swap full, load above 90 at times, two runs killed by the system); the v9 control ran 9 to 18 % slower than on 2026-11-01. The numbers are an upper bound; one quiet re-measurement is open.
- On all 805 rows under that load Q8_0 took 127 ms median and 226 ms p95, so long sentences can exceed 150 ms on a busy Mac.
- The first Q8_0 dequantisation pushed element by element and took 113 ms median; writing fixed 32-weight blocks lets LLVM vectorise it.
- candle's quantised matmul (it quantises the activations too) was fastest in the median for one run but had stalls up to 1.8 s for four runs and more memory; it is gone from the code.
- The v10 lean session's 277 MB of weights keeps the decoder in f16; the runtime keeps it in f32 (+42 MB for four runs) because the decoder runs up to 48 steps per sentence.

## How to run

```bash
uv run tools/mapping/export_runtime.py --run p-v11-s0,p-v11-s1,p-v11-s2,p-v11-s3 --rules conv,pronoun --q8
```

```bash
cd plugin && ./guarded-cargo.sh test -p synthexis-parser --release -- --include-ignored
```

```bash
plugin/target/release/synthexis-parse --dir data/mapping/runtime "mouth opens the bass filter"
```

Plugin: `SYNTHEXIS_PARSER_DIR` (default `~/Library/Application Support/synthexis/parser`), `SYNTHEXIS_PARSER_RUNS` (default `p-v11-s0,p-v11-s1,p-v11-s2,p-v11-s3`; weights and rules come from each run's `parser.json`), `SYNTHEXIS_PARSE="sentence"` for a headless check. `assistant::submit(text)` is the call the editor's text field will use.

## Still open for the mapping assistant

In the order the demo needs them:

1. Model files on Noa's machine: FL started from the Dock sees no env vars, so `install.sh` or a first-run step has to put the export into `~/Library/Application Support/synthexis/parser` (460 MB of f16 safetensors for four runs, quantised at load; not in git).
2. Text field in the editor plus the parse result shown next to it; the worker already runs off the audio and UI threads.
3. Resolver in Rust: frame + live plugin and parameter names -> parameter ids, in the order of the proposed map.resolver amend (alias, name match on the named plugin, param-sense class, chips). Needs the BIO head output for the target text (weights are exported, the head is not run yet) and the live inventory from the FL bridge (session fl-target-picker).
4. param-sense in Rust (hashed n-grams, < 1M params) for meaning-only targets.
5. Frame -> `route.rs`: build routes with modifiers (invert, range, curve, gates, ramp) from resolved frames.
6. Route diff UI: accept, edit, undo.
7. Chips fallback (map.fallback) and the confidence threshold that triggers it, calibrated on Noa's calibration sentences, not on synthetic ones.
