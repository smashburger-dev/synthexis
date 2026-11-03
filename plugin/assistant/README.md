# Mapping assistant runtime

What the plugin's sentence field needs besides code: the parser ensemble and the completion data. `install.sh` puts both where the plugin reads them (`~/Library/Application Support/synthexis/parser` and `.../completion`, see `synthexis/src/assistant.rs` and `sentence.rs`).

| part | where it lives | size |
|---|---|---|
| completion data (`vocab.json`, one parameter file per profiled plugin) | tracked here in `completion/`, written by `tools/mapping/export_completion.py` | 1.2 MB |
| parameter classes: `known.tsv` (labels of the 887 mapped plugins) and `paramsense/` (param-sense ps-v2 for all other plugins) | tracked here in `completion/`, written by `tools/mapping/export_paramsense.py` | 2.5 MB + 32 MB |
| parser runs `p-v11-s0` to `p-v11-s3`, `config.json`, `tokenizer.json` | GitHub release pinned in `parser.release` | 446 MB gzip, 482 MB unpacked |

The parser weights stay out of git: GitHub refuses files over 100 MB, and every new parser would stay in the history for good. f16 weights barely compress (gzip saves 7 %, xz 10 %).

## Install

`plugin/assistant/install.sh`, from any directory. In a checkout with `data/mapping/runtime` and `data/mapping/completion` it links both dirs there. A worktree without `data/` uses the main checkout's `data/` (found through git's common dir), so every session links to the same live exports; `SYNTHEXIS_DATA` overrides it. Links that point to an existing dir are never replaced by copies when no `data/` is found. In a fresh clone it copies `completion/` and downloads the release asset. The download runs through `gh` when it is logged in (needed while the repository is private), otherwise through `curl` with resume. Then the script checks the sha256 from `parser.release`, unpacks into a temporary dir and swaps it in with two renames. A failed download or a wrong checksum leaves the installed parser as it was. A second run with the parser already in place does nothing. `SYNTHEXIS_ASSISTANT=release` forces the release path in a dev checkout.

## New parser version

Export it with `tools/mapping/export_runtime.py`, set `DEFAULT_RUNS` in `synthexis/src/assistant.rs`, then run `plugin/assistant/pack.sh assistant-parser-<version>`. The script packs those runs with `NOTICE.txt` and the licence texts, uploads the archive to a new release and rewrites `parser.release`. Commit `parser.release`. Tags are never reused, so a pinned checksum always matches its asset.

## Licences

The parser weights are a Derivative of Muse Spark Open under the Muse Model License: part of their training labels and sentences came from it. They ship with that licence and its Usage Policy (`LICENSES/MUSE-MODEL-LICENSE.md`), and the plugin and README show "Built with Muse Spark Open". The encoder is mmBERT small under MIT (`LICENSES/MIT-mmBERT.txt`). The archive carries both texts and `NOTICE.txt`. Full reasoning: `docs/credits.md`, `probes/licence-check/RESULT.md`.
