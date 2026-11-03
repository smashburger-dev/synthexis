# param-sense v2

ps-v2 is a 5-member ensemble with 2^16 hash buckets, folded into one
net of the ps-v1 format. Against ps-v1 config under vendor-grouped CV:
top-1 60.5 -> 64.8, macro F1 48.8 -> 55.5 (runtime-only, mean of 5
folds). On the old 19-vendor test split: top-1 72.8 -> 76.2, macro F1
51.1 -> 58.3, `unsafe` F1 35.7 -> 52.6.

Run: `data/mapping/train/runs/ps-v2/` (meta.json, weights.f16.bin,
metrics.json, known_plugins.json). CV files per lever:
`data/mapping/train/runs/ps-cv/<name>.json`.

## Selection rule (fixed before the first run)

ps-v1 picked its epoch on 6 val vendors whose top-1 (0.49) sat 24 points
below the 19 test vendors (0.73). One vendor can swing a small split
that far, so v2 selects on all vendors.

- Folds: 5 vendor-grouped folds over all 61 vendor groups ("Universal
  Audio", "(UADx)" and "Sphere" are one group). Greedy balance by scored
  rows, biggest vendor first. Fixed for every run.
- Scored rows: rank < 60 per (plugin, class) after a fixed shuffle, the
  same rows for every run whatever its training `--cap`.
- Branch: runtime-only (no value strings), because that is what the
  plugin computes today. The with-values branch is reported next to it.
- Epochs: each config trains 20 epochs per fold and is scored on the
  held-out fold after each. Its epoch count is the epoch with the best
  mean top-1 over the 5 folds.
- Win: per fold, delta = candidate minus current best at their own best
  epochs. A candidate replaces the current best if
  - mean delta top-1 >= +0.5 points, top-1 rises on at least 4 of 5
    folds, and mean delta macro F1 >= -1 point; or
  - mean delta macro F1 >= +1 point, macro F1 rises on at least 4 of 5
    folds, and mean delta top-1 >= -0.5 points.
- Noise check: the baseline config with seed 1 against seed 0 (A/A). If
  A/A passes the rule, the thresholds are too low and get raised before
  any lever counts.
- Levers go one at a time on top of the current best. A rejected lever
  is dropped.

Code: `verdict()` in `tools/mapping/train_paramsense.py`.

### Change after the A/A run, before any lever

A/A (single seeds) did not pass: mean delta top-1 -0.03, but single
folds moved by -2.1 and +1.8 points. That is close to the +0.5 bar, so
from then on every config's per-fold, per-epoch scores are the mean of
seeds 0 and 1. Thresholds unchanged.

## Folds

| fold | vendors | scored rows | ps-v2 config top-1 | macro F1 |
|---|---|---|---|---|
| 0 | Universal Audio | 20,912 | 87.7 | 46.7 |
| 1 | Arturia | 16,771 | 63.3 | 61.5 |
| 2 | Waves | 16,291 | 43.4 | 45.3 |
| 3 | 28 small vendors (Antares ... sonible) | 12,436 | 61.5 | 62.8 |
| 4 | 30 small vendors (Ample Sound ... reFX) | 12,436 | 68.2 | 61.2 |

The CV mean is lower than the old test number because the old test split
happens to hold easier vendors: the same ps-v1 config scores 60.5 under
CV and 72.8 on the old split. Waves is the hardest vendor by far.

## Levers

Runtime-only branch, mean over folds, each at its own best epoch. "d
top-1 per fold" is the paired delta against the run in "against".

| run | lever | against | ep | top-1 | macro F1 | unsafe F1 | top-1 values | d top-1 per fold | mean d top-1 | mean d macro | accept |
|---|---|---|---|---|---|---|---|---|---|---|---|
| base | ps-v1 config | - | 17 | 60.5 | 48.8 | 46.5 | 62.0 | - | - | - | - |
| aa-s1 | seed 1 vs 0 (single seed) | aa-s0 | 17 | 60.5 | 49.1 | 43.9 | 62.2 | +0.4 -0.3 -2.1 +1.8 +0.0 | -0.03 | +0.60 | no |
| b16 | 2^16 buckets | base | 17 | 61.4 | 48.2 | 37.8 | 63.0 | -0.3 -0.9 +4.0 +1.4 +0.1 | +0.86 | -0.52 | no |
| b15 | 2^15 buckets | base | 17 | 60.9 | 48.2 | 43.1 | 62.2 | +0.2 -1.0 +2.8 +0.9 -0.8 | +0.42 | -0.60 | no |
| h192 | hidden 192 | base | 17 | 60.0 | 47.9 | 41.3 | 61.6 | -0.2 -1.1 +0.3 -1.8 -0.1 | -0.55 | -0.87 | no |
| cap120 | --cap 120 | base | 20 | 60.3 | 48.4 | 40.1 | 61.5 | -0.1 +0.1 -0.3 -0.5 -0.4 | -0.24 | -0.34 | no |
| cap30 | --cap 30 | base | 18 | 60.6 | 48.9 | 44.3 | 62.2 | +0.1 +0.5 +0.0 +0.4 -0.4 | +0.12 | +0.09 | no |
| cw1 | class weight 1/freq | base | 19 | 59.7 | 48.9 | 44.7 | 61.5 | -0.3 -2.0 -2.9 +0.6 +0.5 | -0.81 | +0.10 | no |
| unsafe3 | unsafe weight x3 | base | 17 | 60.4 | 48.4 | 44.8 | 61.8 | -0.1 -0.2 -0.2 -0.1 -0.1 | -0.14 | -0.35 | no |
| vd08 | value drop 0.8 | base | 17 | 60.5 | 48.4 | 47.3 | 61.6 | -0.1 -0.3 +0.2 +0.1 -0.1 | -0.04 | -0.39 | no |
| smooth | label smoothing 0.1 | base | 13 | 60.4 | 48.5 | 39.9 | 61.9 | +0.3 +0.7 -3.7 +1.7 +0.4 | -0.14 | -0.27 | no |
| m3 | 3 members | base | 20 | 62.8 | 53.1 | 45.5 | 64.6 | +0.4 +4.0 +2.3 +2.7 +2.1 | +2.29 | +4.38 | yes |
| m5 | 5 members | m3 | 20 | 63.2 | 53.9 | 46.1 | 64.9 | +0.4 +0.3 +0.4 +0.5 +0.6 | +0.44 | +0.81 | no |
| m3d32 | 3 members, dim 32 | m3 | 20 | 62.5 | 53.3 | 48.4 | 64.3 | +0.0 -1.4 -0.8 +0.5 -0.0 | -0.34 | +0.14 | no |
| m3b16 | 3 members, 2^16 | m3 | 20 | 64.3 | 53.9 | 42.7 | 65.8 | +0.2 +0.1 +3.7 +1.9 +1.8 | +1.54 | +0.75 | yes |
| m3b17 | 3 members, 2^17 | m3b16 | 20 | 64.5 | 54.5 | 43.5 | 65.9 | +0.1 -0.1 -0.1 +1.1 -0.1 | +0.17 | +0.66 | no |
| m5b16 | 5 members, 2^16 | m3b16 | 18 | 64.8 | 55.5 | 46.1 | 66.4 | +0.2 +0.7 +0.2 +0.7 +0.4 | +0.46 | +1.62 | yes (macro) |

What the table says:

- Only the ensemble moved the numbers. A single net is capacity-saturated
  for this data: wider hidden layers and more buckets mostly add
  per-seed variance (b16 gains 4 points on Waves and loses on 3 other
  folds). Averaging independent nets removes that variance, and then
  more buckets pay off too (m3b16 rises on all 5 folds where b16 did
  not).
- The ensemble is legal in the ps-v1 format: `merge()` concatenates the
  members' embedding columns, places their hidden layers block-diagonally
  and divides the concatenated classifier by K, so the single exported
  net computes the mean of the members' logits exactly (checked to 1e-6).
- Class weighting, the `unsafe` weight and the cap trade top-1 against
  rare classes without a net gain. ps-v1's defaults (1/sqrt(freq),
  cap 60) stay.
- Epochs: the ensembles were still creeping up at epoch 18 to 20
  (+0.1 point per few epochs). Not pursued.

## unsafe is a label problem

`unsafe` F1 stays between 38 and 48 whatever the lever, and the `unsafe`
weight x3 makes it worse. The labels give identical names both classes:
"Output" is `unsafe` 129 times and `amp.gain` 76 times, "Output Gain"
58 to 19, "Master Volume" 9 to 23, "Volume" 11 to 16. No name-based model
can split those. A consistent label rule (for example every output or
master level is `unsafe`) would fix it at the source.

## Value strings: the plugin should stay runtime-only

ps-v2 on the old 19-vendor test split, by what the value bag contains:

| value bag | top-1 | macro F1 | unsafe F1 |
|---|---|---|---|
| empty (runtime-only, plugin today) | 76.2 | 58.3 | 52.6 |
| 5 strings sampled at 0/0.2/0.5/0.8/1 (harvest) | 78.3 | 59.8 | 55.4 |
| default value string only | 75.7 | 57.3 | 43.5 |

Under CV the sampled strings add 1.6 points top-1 (64.8 -> 66.4). But
`plugins.getParamValueString` only returns the string for the current
value. Getting five samples means setting each parameter to five
positions and back, which is audible and touches the project. With the
one string FL gives for free, the model is slightly worse than with
none, because training never saw a one-string bag. So: the plugin sends
an empty value bag. If sampling ever becomes acceptable (for example
once, offline, at plugin load in a muted channel), the 5-string bag is
worth about 2 points.

## Format

meta.json: `buckets` 65536, `dim` 240, tensors emb.weight [65536, 240],
hid.weight [480, 720], hid.bias [480], cls.weight [44, 480], cls.bias
[44], dir.weight [2, 480], dir.bias [2]. Same 44 classes, hash, fields,
casefold and word regex as ps-v1; only the shapes changed. weights.f16.bin
is 32 MB (ps-v1: 1.6 MB), 16.1M parameters. A numpy loader written from
meta.json alone (no torch) reproduces sensible classes: Cutoff ->
filter.cutoff, Attack -> env.attack, Bypass -> ctl.switch, Dry/Wet ->
fx.mix, Filter Type -> ctl.select.

If 32 MB is too much for the plugin, the m3b16 config (19 MB, CV top-1
64.3, macro F1 53.9, old split 76.1 / 58.2) is the fallback:
`--members 3 --buckets-log 16 --epochs 20`.

Reproduce:

    uv run tools/mapping/train_paramsense.py --name ps-v2 --members 5 \
      --buckets-log 16 --epochs 18 --export --cv-from m5b16
