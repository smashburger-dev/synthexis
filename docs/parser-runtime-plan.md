# Parser runtime in the plugin (2026-11-01)

Goal: the plugin turns a typed sentence into a route frame offline, with the shipped parser (greedy ensemble of p-v7a-s0 and p-v7a-s1, `docs/session-2026-11-01-parser-v8.md`). This file lists what is missing between "sentence typed in the plugin" and "routes applied", and what this session builds.

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

M1 Pro, one core (CPU time equals wall time; `VECLIB_MAXIMUM_THREADS=1` changes nothing), 200 synthetic dev sentences (`ds-v6p/dev.jsonl`, first 200), release build. Reference: `uv run tools/mapping/predict.py --run p-v7a-s0,p-v7a-s1 --f16` (shipped f16 weights computed in f32). Tool: `plugin/target/release/synthexis-parse`.

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

## How to run

```bash
uv run tools/mapping/export_runtime.py --run p-v7a-s0,p-v7a-s1
```

```bash
cd plugin && ./guarded-cargo.sh test -p synthexis-parser --release -- --include-ignored
```

```bash
plugin/target/release/synthexis-parse --dir data/mapping/runtime "mouth opens the bass filter"
```

Plugin: `SYNTHEXIS_PARSER_DIR` (default `~/Library/Application Support/synthexis/parser`), `SYNTHEXIS_PARSER_RUNS` (default `p-v7a-s0,p-v7a-s1`, fallback `p-v7a-s0`), `SYNTHEXIS_PARSE="sentence"` for a headless check. `assistant::submit(text)` is the call the editor's text field will use.

## Still open for the mapping assistant

In the order the demo needs them:

1. Model files on Noa's machine: FL started from the Dock sees no env vars, so `install.sh` or a first-run step has to put the export into `~/Library/Application Support/synthexis/parser` (250 MB, not in git).
2. Text field in the editor plus the parse result shown next to it; the worker already runs off the audio and UI threads.
3. Resolver in Rust: frame + live plugin and parameter names -> parameter ids, in the order of the proposed map.resolver amend (alias, name match on the named plugin, param-sense class, chips). Needs the BIO head output for the target text (weights are exported, the head is not run yet) and the live inventory from the FL bridge (session fl-target-picker).
4. param-sense in Rust (hashed n-grams, < 1M params) for meaning-only targets.
5. Frame -> `route.rs`: build routes with modifiers (invert, range, curve, gates, ramp) from resolved frames.
6. Route diff UI: accept, edit, undo.
7. Chips fallback (map.fallback) and the confidence threshold that triggers it, calibrated on Noa's calibration sentences, not on synthetic ones.
