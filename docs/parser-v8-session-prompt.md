# Prompt for the parser v8 session

Open in the worktree of branch `claude/mapping-assistant-local-training-9b23fe` (or a branch from its tip).

---

You work on the synthexis mapping assistant, only on the sentence parser. Answer Noa in German, address him as Noa, code and comments in English, load `unslop` first.

Read first: `docs/session-2026-11-01-parser-v7.md` (state, learnings, files), `probes/real-eval/RESULT.md` (real-sentence scoring, sections from "Parser runs" on), `probes/parser-errors/RESULT.md` section 10 (v7 runs), `tools/mapping/modal_parser.py` (how training runs on Modal).

**Goal:** from 31 % to 40 % or more exact and from 68 % to 75 % or more "right or one part wrong" on the eval part of Noa's real sentences, without Spark at runtime. Spark reaches 63 % and 83 % with the same label spec, so the gap is the model. The demo is the deadline; stop when a lever does not pay.

**Levers, cheapest first (all hypotheses until measured):**
1. No training: average the decoder log-probs of p-v7a-s0 and p-v7a-s1 at each step (ensemble, 242 MB f16) and replace greedy by a grammar-constrained beam (width 4 to 8) in `frame_parser.greedy`. Measure on the dev sets.
2. Domain post-training of the encoder: continue masked-language-model training of mmBERT small de-en on in-domain text (all synthetic sentences of ds-v6p, the 11,846 pool sentences in `data/mapping/teach/pool.jsonl`, and fresh DeepSeek-written sentences), 1 to 2 epochs on Modal, then the v7a recipe on top. Never the real set.
3. Teacher data, done clean: a neutral teacher prompt built only from the label spec (ontology, frame language, examples from training data), with no rule written from calibration or eval errors. DeepSeek V4.1 Flash without thinking labels each pool sentence twice (two example orders); keep rows where both runs and the edit agree, or both runs agree. DeepSeek can also write fresh free sentences from edits (no slot marks) at about 0.15 $ per 1M input tokens off-peak. This tells whether v7's teacher rows failed through the calibration-shaped rules or through label noise.
4. Encoder choice with the larger budget: Noa allows 500 to 800 MB RAM for the plugin. mmBERT base did not beat small on our data, so another family (mDeBERTa-v3-base, XLM-R large) is the only size lever left; low prior.
5. Gold: Noa's check of `data/mapping/eval/gold-routes.json` can move the score without training (11 eval misses are switch against knob). Ask him once, do not wait for it.

**Selection rule (write it into RESULT.md before scoring eval):** choose on the synthetic dev sets (dev, challenge, dev_s2 to dev_s5, route_edit). Calibration selects only runs whose labels no calibration-derived rule shaped. Score eval once, at the end, for the chosen arm and report all arms. Two seeds before claiming a gain; paired test with `probes/real-eval/score.py` canon (scratch helpers for F1, error types and paired McNemar plus group bootstrap are easy to rebuild: see section "Parser v7").

**Money:** Baseten ledger 73.57 $ of 100 $; the rest is the motion.labels reserve of the main session, so no Spark spend here without Noa's explicit OK. DeepSeek: Noa topped up and approved use (Keychain `DEEPSEEK_API_KEY`, synthetic text only, it processes in China). Modal: L4 about 0.8 $ per hour, deploy then spawn (`python tools/mapping/modal_parser.py spawn ...`), upload data to the volume once.

**Rules:** Noa's 150 sentences (`data/mapping/eval/expanded.jsonl`) and anything derived from them never go into training or prompts; the leak check in `tools/mapping/teacher_pool.py` (word 6-gram, ratio 0.7) runs on every new text. Secrets never printed. Do not touch `spec/`, `plugin/`, `tools/labels/`, `data/labels/`, `data/frames/`; spec changes go to `probes/mapping-pipeline/SPEC_PROPOSAL.md`. Numbers only with a measurement. Commits on this line are approved, end messages with "New code written in this repo."; no pushes without Noa. At the end update `probes/real-eval/RESULT.md`, `probes/parser-errors/RESULT.md`, `docs/roadmap.md`, the FigJam row "Your real sentences" (board `WJIE59w34hCSTXuq3XpKGH`, sections 29:1277, 29:1307, 29:1326) and write `docs/session-<date>-parser-v8.md`.
