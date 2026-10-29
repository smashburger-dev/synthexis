# Prompt for the overnight mapping training session

Paste this into a new Devin session opened in `/Users/no8/Desktop/life/Lifemaxxing/synthexis`. Runs unattended; Noa is not available until morning. Keep the Mac on power and awake (`caffeinate -dims` around the training commands).

---

You work on **synthexis** (Meta hackathon, submission 4.11. 21:00 ET), only on the mapping assistant: train param-sense v1 and parser v1 overnight and leave measured results for the morning. Noa's own sentences come later in a separate session (`docs/mapping-next-session-prompt.md`); do not wait for them.

Read first: `AGENTS.md`, `docs/session-2026-10-29-mapping.md`, `probes/mapping-pipeline/DESIGN.md` (section "Training interfaces" is the settled spec), `probes/mapping-pipeline/RESULT.md`, `data/mapping/PROGRESS.md`. Load `unslop` and `codebase-design`.

**Where to train:** on this Mac. Measured on the M1 Pro (`probes/mapping-pipeline/train_bench.py`): a 9.2M-parameter parser of the DESIGN.md shape takes 96 ms per optimizer step on MPS at batch 64 (CPU 452 ms), so about 1 minute per epoch on 40k samples and about 30 minutes per 30-epoch run. param-sense (about 0.8M) trains in minutes on CPU. Modal is not needed: until 2026-10-31 every Modal second is Noa's own money, and the 30 $ credit from 11-01 is reserved for the vision students.

**Plan (in this order, one change per run, every run logged with its numbers):**

1. Data. Training set = `data/mapping/commands/cmd-s1/sentences.jsonl` minus sentences that fail intent, invert or sources in the full round-trip (`data/mapping/roundtrip/rt-s1-all/`; keep targets-only mismatches, they are mostly lay wording), plus `data/mapping/commands/kw-s1/` keyword sentences, plus span-substitution augmentation from `data/mapping/glossary/gloss-v1/` (same concept and polarity, re-validated). Dev = 10 % of edit ids (twins stay with their original). Write `tools/mapping/dataset.py` (deterministic, seeded) and print counts.
2. Challenge set: 300 new edits (`edits.py --seed 99`, never in training) verbalised with `tools/mapping/prompts/challenge.md` through a small `challenge.py` (same validator as commands.py, `--reasoning-effort medium`, `--max-usd 1.5`). Lead-authored prompt; do not reword it.
3. param-sense v1 per DESIGN.md on `data/mapping/labels/*/labels.jsonl` (runtime-only features, hashed bags, vendor holdout 20 %). Report top-1, macro F1, per-class F1 for unsafe, ctl.switch, ctl.select, system; export the known-plugin table from the priority runs.
4. Tokenizer: SentencePiece unigram 8k, byte fallback, trained on training sentences + glossary + harvested plugin and parameter names (never on dev, challenge or Noa's files).
5. Parser v1 per DESIGN.md (encoder 6 x d256, 2-layer pointer decoder, grammar-constrained decoding, auxiliary BIO head). First a 64-sample overfit check, then the full run. Metrics on dev and challenge: exact frame match, route-edit accuracy, per-field accuracy (intent, sources, targets, invert, scope pointers), with Wilson intervals; confidence curve (accuracy vs coverage).
6. Filter ablation: filtered set (8,003 sentences without intent, invert, source or route mismatch in rt-s1-all) vs all 13,128, same step budget, scored on dev built from clean sentences and on the challenge set. Learning curve: 25 / 50 / 100 % of the better set, same step budget; 3 seeds for the 100 % point. This decides whether a stage-2 commands run on Baseten is worth it; write the decision with numbers.
7. Latency and RSS on the M1 Pro (median and p95 over 200 sentences, CPU inference), int8 or fp16 export, and a weights blob plus JSON meta format the plugin can load from Rust.

**OWNED PATHS:** `tools/mapping/**`, `data/mapping/**` (git-ignored), `probes/mapping-*/**`. Do not touch `spec/`, `plugin/`, `tools/labels/`, `data/labels/`, `data/frames/`. Spec changes go to `probes/mapping-pipeline/SPEC_PROPOSAL.md`.

**Money:** only the challenge set calls Baseten (cap 1.5 $). Ledger: `uv run tools/mapping/spark.py ledger`.

**Rules:** new code only. No secrets in output. Numbers only with a measurement. In the morning: results into `probes/mapping-pipeline/RESULT.md`, digest `docs/session-<date>-mapping-train.md`, mapping section of `docs/roadmap.md`; commit (Noa approved commits for the mapping line).
