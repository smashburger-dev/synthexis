# Prompt for the next mapping session

Paste this into a new Devin session opened in `/Users/no8/Desktop/life/Lifemaxxing/synthexis`.

---

You work on **synthexis** (Meta hackathon, submission 4.11. 21:00 ET), only on the mapping assistant. The main session handles data, students and the plugin; the hill-climb line owns `plugin/`.

Read first: `AGENTS.md`, `docs/session-2026-10-29-mapping.md` (state and open items), `probes/mapping-pipeline/DESIGN.md` (architecture, training interfaces, review decisions), `probes/mapping-pipeline/RESULT.md` (measured numbers and commands), `data/mapping/PROGRESS.md` (file map). Load `unslop` and `codebase-design`.

**Goal of this session:** param-sense v1 and parser v1 trained and evaluated, with Noa's sentences as the judge.

1. Noa's sentences: `data/mapping/eval/calibration.txt` (50) and `data/mapping/eval/eval.txt` (100), one sentence per line, git-ignored. If they are not there, ask Noa. Never train on them or tune prompts with them. Label them yourself into edit JSON (format of `data/mapping/commands/*/sentences.jsonl` -> `edit`), show Noa the labels for a quick check.
2. Data: stage-1 sentences (`data/mapping/commands/cmd-s1/sentences.jsonl`) minus round-trip drops (`data/mapping/roundtrip/rt-s1/drop_ids.json`) plus template keyword sentences (`data/mapping/commands/kw-s1/`), plus free span-substitution augmentation from `data/mapping/glossary/gloss-v1/`. Split dev by edit id. Build a challenge set: about 300 held-out edits verbalised with `tools/mapping/prompts/challenge.md` (lead-authored, short and style-free), never trained.
3. param-sense v1 per DESIGN.md "Training interfaces": runtime-only features, hashed bags, about 0.8M params, vendor holdout 20 %, report top-1, macro F1, per-class F1 for unsafe/ctl.switch/ctl.select/system. Training data: `data/mapping/labels/{label-prio-v1,label-bulk-v1,label-vst3-prio-v1,label-vst3-bulk-v1}/labels.jsonl`. Export the known-plugin table from the priority runs.
4. Parser v1 per DESIGN.md: SentencePiece 8k (byte fallback), encoder 6 x d256 plus 2-layer pointer decoder with grammar-constrained decoding plus auxiliary BIO head, about 8M params, Mac (MPS or CPU). Metrics: exact frame match and route-edit accuracy on dev, challenge set and Noa's eval (Wilson interval); selective prediction curve; threshold from the 50 calibration sentences. Learning curve at 25/50/100 % decides whether a stage-2 commands run (Baseten) is worth it.
5. Latency and RSS of both models on the M1 Pro (median and p95), then an export format the plugin can load from Rust (weights blob plus JSON meta). Integration itself goes through the hill-climb line.

**OWNED PATHS:** `tools/mapping/**`, `data/mapping/**` (git-ignored), `probes/mapping-*/**`. Do not touch `spec/`, `plugin/`, `tools/labels/`, `data/labels/`, `data/frames/`. Spec changes go to `probes/mapping-pipeline/SPEC_PROPOSAL.md`.

**Money:** Baseten credit 100 $, spent so far in RESULT.md (`uv run tools/mapping/spark.py ledger`). Global guard `SYNTHEXIS_BASETEN_CAP` defaults to 60 $; keep at least 25 $ for the main session's `motion.labels`. Training runs on the Mac; no Modal.

**Rules:** new code only, nothing copied from other Lifemaxxing projects. No secrets in output. Credit "Built with Muse Spark Open". Numbers only with a measurement. At the end tick the mapping section of `docs/roadmap.md`, write `docs/session-<date>-mapping.md`, commit after Noa's approval.
