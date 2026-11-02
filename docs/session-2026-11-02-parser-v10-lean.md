# Session 2026-11-02: parser v10 lean

Follows `docs/session-2026-11-02-parser-v10.md`. Rule: `probes/real-eval/RESULT.md`, "Parser v10 lean: selection rule" (commit bee9d0c, before any candidate). Numbers: `probes/parser-errors/RESULT.md` section 14 and `probes/real-eval/RESULT.md` "Parser v10 lean".

## Task

The four-seed v10 ensemble reads the real set best (56 of 102 exact) but needs 4 x 121 MB in f16 and four encoder passes per sentence. Noa wanted that reading at the cost of one seed, within 500 to 800 MB of RAM and at most 6 $ of Modal credits for this session.

## What happened

1. Literature first: sequence-level and word-level distillation (Kim and Rush 2016; ensemble distillation as in Hinton et al. 2015) for one student, model soups for weight averaging (need a shared start), top-layer dropping (Sajjad et al., "Poor Man's BERT") for depth, ggml block quantisation because the Rust runtime uses candle.
2. Weight averaging fails outright (0.003): every seed starts its decoder from its own random weights.
3. Students: one mmBERT-small, trained on L4 or A10G with 0.5 x gold CE plus 0.5 x KL to the ensemble's mean log-softmax on frame, pointer and BIO steps. A pool of 31,369 unused synthetic sentences (validator drops and left-out rows, no dev edit id, 1,237 dropped by `leak.py`) labelled by the ensemble adds 0.8 points; 16k steps add 0.2 more. Twelve encoder layers lose 2.2 points for 7 ms.
4. Q8_0 weights change no dev mean by more than 0.001, Q4_0 costs 2 points.
5. By the rule, lean-kd-p16 in Q8_0 wins: 0.7706 against 0.7735, 69 MB of weights against 482.
6. Real set, scored once: the student gets 44 of 102 exact, the ensemble 56 (p 0.0075). The ensemble in Q8_0 gets the same 56 and 35 of 51 calibration at 277 MB.

## Result

| artefact | weights | dev, eight sets | real eval exact | right or one part wrong | calibration | latency |
|---|---|---|---|---|---|---|
| ensemble v10 s0 to s3, f16 (reference) | 482 MB | 0.7735 | 56/102 | 85/102 | 35/51 | 127.5 / 306.6 ms in torch |
| lean-kd-p16, Q8_0 (rule's pick) | 69 MB | 0.7706 | 44/102 | 78/102 | 33/51 | one run; Q8_0 to be timed in candle |
| lean-kd-p16, f16 | 121 MB | 0.7710 | 45/102 | 77/102 | 33/51 | 30.6 / 65.8 ms in torch |
| ensemble v10 s0 to s3, Q8_0 | 277 MB | 0.7735 | 56/102 | 84/102 | 35/51 | four runs; Q8_0 to be timed in candle |

(all with `conv,pronoun`; latency median / p95 per sentence on one CPU thread of the M1 Pro.)

The dev sets cannot tell a single model from the four-seed ensemble on what matters here: they rate the ensemble's lead as zero and the student as almost equal, and the real set disagrees by 12 points. The rule's pick is cheap but reads Noa's sentences like one seed. The Q8_0 ensemble keeps the four-seed reading at 57 % of the memory; its latency stays that of four runs unless candle's quantised matmul is faster than the f16 path. Which one ships is Noa's decision; picking the Q8_0 ensemble would rest on the seen set.

## Money

Modal: five student runs, about 4 GPU hours on L4 or A10G, 3 to 4.5 $ in credits (Modal does not log which GPU ran). Two spawns failed at start because another session redeployed the shared app `synthexis-parser` with an older `train_parser.py`; `SYNTHEXIS_MODAL_APP=synthexis-parser-lean` now gives this work its own deployment. L4 alone was out of capacity for 12 minutes, so the train function falls back to A10G (L40S removed). No DeepSeek, no Baseten.

## Files

- `tools/mapping/train_parser.py` (`--teachers`, `--kd`, `--kd-t`, `--init`, `--hf-layers`, comma list for `--train`), `frame_parser.py` (`hf_layers` in the config), `distill.py` (pool and ensemble labels), `lean.py` (cost, soup, ggml round trip), `modal_parser.py` (own app name, L4 then A10G, 4 h timeout).
- Data (git-ignored): `data/mapping/commands/kd-pool` (sentences, `leak_drop.json`), `data/mapping/train/ds-v10k` (ds-v10c plus `pool.jsonl`); runs `lean-kd-c`, `lean-kd-p`, `lean-kd-p12`, `lean-kd-pc`, `lean-kd-p16` (with `model.f16.pt`, `export_hf.json`), simulated Q8_0 dirs `lean-kd-p16-q8`, `lean-p-v10-s0-q8` to `lean-p-v10-s3-q8`, `lean-soup4`; dev scores `data/mapping/train/decode/lean-*.json`; predictions `data/mapping/eval/preds-g2-lean-*-convpro.jsonl`.

## Open

- Noa decides: the rule's pick (student, Q8_0, one run) or the Q8_0 four-seed ensemble (277 MB, four runs).
- Runtime (`plugin/parser`, session parser-runtime): Q8_0 by quantising the f16 encoder matrices and the embedding at load (candle `QTensor::quantize`, `GgmlDType::Q8_0`, `QMatMul`), then time it and check frames against `predict.py` on the `-q8` run dirs. As in ggml, candle's CPU kernel also quantises the activations, which the torch simulation does not, so expect a few frames to differ and compare the dev score, not only exact parity. Port `rules.py` (conv, pronoun) with `conventions.py` and `normalize.py`.
- If a student stays of interest: it was still improving at 16k steps, and a set Noa dictates fresh would show whether its real-set gap is the student or the seen set.
