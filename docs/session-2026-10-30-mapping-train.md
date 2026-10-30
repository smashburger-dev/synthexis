# Session 2026-10-30: mapping assistant, overnight training on the Mac

Entry point for the next mapping session. Numbers and commands: `probes/mapping-pipeline/RESULT.md` (section "overnight training on the Mac"). Full run table: `data/mapping/train/REPORT.md` (git-ignored, `uv run tools/mapping/report.py` rebuilds it). Code: `tools/mapping/` (commit 9e64d3a). First session in Claude Code instead of Devin; the Baseten key in the Keychain (`synthexis.baseten`) worked unchanged.

## Results

- One unattended queue (`caffeinate -dimsu zsh tools/mapping/run_overnight.sh`), 00:19 to 04:45, eleven steps, all rc=0, no Modal.
- Challenge set: 548 valid sentences from 300 new edits (seed 99), 0.41 $. Only Baseten spend of the night.
- param-sense v1 (0.8M, 0.7 min on CPU): top-1 0.728 and macro F1 0.511 on 19 unseen vendors from runtime-only features. unsafe F1 0.36 is the weak spot. Known-plugin table: 63,625 rows.
- Parser v1 (9.30M, 33 min per 16,000 steps on MPS): route_edit 0.697 dev, 0.628 challenge for the main run; 3 seeds give challenge 0.628 with sd 0.007. Intent is 0.94-0.98, the losses sit in multi-route sentences (2 routes 0.38, 3 or more 0.00 on the challenge set), invert and targets.
- Filter ablation: all sentences beat the round-trip-filtered ones by 11 points on the challenge set (0.628 vs 0.516). The filter removes useful sentences.
- Learning curve: +15.6 and +13.4 points per doubling of edits; doubling the steps adds only 1.1 points.
- Confidence works as a gate: the most confident 30 % of challenge sentences are 0.963 correct.
- Export: CPU median 9.0 ms, p95 21.5 ms per sentence on one thread (four threads are slower). int8 blob 9.4 MB with the same accuracy as f32. Tensor tables for Rust in `export.json`.

## Decisions

- Train on every validated sentence; the round-trip check stays a measurement tool, not a filter.
- Stage 2 on Baseten: yes, about 8,000 more edits weighted towards 2-3 routes, invert, modify, German and layperson wording, about 11 $ by the stage-1 rate. Rules and numbers in RESULT.md, "Stage-2 decision".
- The plugin runs the parser on one thread with the int8 blob.

## Open

1. Stage 2 commands run with the weighted edit sampler, then retrain at 16,000 steps (Baseten spend needs Noa's go).
2. Noa's 50 calibration and 100 eval sentences (`docs/mapping-next-session-prompt.md`); the chip-fallback threshold comes from them, not from the synthetic coverage numbers.
3. Label audit needs cross-fitting: the parser saw every audited sentence, so 99.97 % agreement only shows memorisation.
4. param-sense: val (6 vendors) and test (19 vendors) disagree by 24 points top-1; a vendor-grouped cross-validation would give a usable number. unsafe needs more examples or a rule.
5. The 2x-steps run (`p-all-100-s0-long`, 0.639 challenge) is not exported; the shipped blob is `p-all-100-s0`. The difference is inside seed noise.
6. Rust loader for the blobs in the plugin (hill-climb line owns `plugin/`).
