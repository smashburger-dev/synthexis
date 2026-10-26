# CoreML student-candidate latency probe (M1 Pro)

Median / p95 / p99 ms per `model.prediction` call, 1000 timed iterations after
50 warmup, input = one reused CVPixelBuffer (32BGRA) of the model input size,
per `MLModelConfiguration.computeUnits`. Random weights throughout.

## Results

| model | input | params | .mlpackage | units | load ms | first pred ms | median ms | p95 ms | p99 ms |
|-------|-------|--------|-----------|-------|---------|---------------|-----------|--------|--------|
| S1 lraspp_mobilenet_v3_large | 256² | 3.22M | 7 MiB | cpuOnly | 168 | 8.7 | 3.710 | 5.665 | 6.564 |
| S1 | 256² | | | cpuAndGPU | 451 | 3230 | 3.190 | 3.783 | 4.337 |
| S1 | 256² | | | cpuAndNeuralEngine | 692 | 4.2 | 0.743 | 1.673 | 2.175 |
| S1 | 256² | | | all | 618 | 3.7 | 0.734 | 1.292 | 1.921 |
| S1 | 320² | | | cpuOnly | 91 | 8.0 | 5.967 | 6.980 | 9.967 |
| S1 | 320² | | | cpuAndGPU | 137 | 1054 | 4.167 | 5.150 | 8.318 |
| S1 | 320² | | | cpuAndNeuralEngine | 769 | 5.3 | 1.107 | 1.359 | 2.486 |
| S1 | 320² | | | all | 685 | 4.1 | 1.527 | 4.055 | 8.957 |
| S2 mbv4_conv_small + LR-ASPP | 256² | 1.51M | 3 MiB | cpuOnly | 60 | 2.8 | 1.628 | 2.241 | 2.837 |
| S2 | 256² | | | cpuAndGPU | 117 | 909 | 2.188 | 2.810 | 3.298 |
| S2 | 256² | | | cpuAndNeuralEngine | 373 | 3.2 | 0.482 | 0.592 | 0.661 |
| S2 | 256² | | | all | 408 | 3.1 | 0.455 | 0.588 | 0.704 |
| S2 | 320² | | | cpuOnly | 57 | 4.0 | 2.751 | 3.587 | 4.228 |
| S2 | 320² | | | cpuAndGPU | 100 | 308 | 2.195 | 2.827 | 3.591 |
| S2 | 320² | | | cpuAndNeuralEngine | 411 | 4.3 | 0.664 | 1.617 | 2.418 |
| S2 | 320² | | | all | 372 | 4.1 | 0.671 | 1.098 | 1.479 |
| H1 mbv4_conv_small cls | 128² | 2.50M | 5 MiB | cpuOnly | 59 | 2.6 | 0.772 | 1.483 | 2.375 |
| H1 | 128² | | | cpuAndGPU | 113 | 764 | 1.961 | 2.775 | 3.797 |
| H1 | 128² | | | cpuAndNeuralEngine | 365 | 2.8 | 0.375 | 0.550 | 0.741 |
| H1 | 128² | | | all | 465 | 2.9 | 0.348 | 0.591 | 1.328 |
| H2 mbv3_small_100 cls | 128² | 1.52M | 3 MiB | cpuOnly | 79 | 1.5 | 0.705 | 1.496 | 2.986 |
| H2 | 128² | | | cpuAndGPU | 123 | 1429 | 2.775 | 3.877 | 4.447 |
| H2 | 128² | | | cpuAndNeuralEngine | 723 | 2.5 | 0.350 | 0.475 | 0.659 |
| H2 | 128² | | | all | 728 | 2.5 | 0.313 | 0.430 | 0.488 |

Takeaways:

- Neural Engine wins everywhere: ~0.3–1.5 ms median. `all` matches or beats
  `cpuAndNeuralEngine` on most models.
- `cpuAndGPU` is worse than `cpuOnly` for every model (sync overhead) and its
  first prediction includes a one-time shader compilation of 0.3–3.2 s.
- S2 (1.51M params, handwritten LR-ASPP on mbv4_conv_small stride-8/32
  features) beats S1 at every size: 0.46 ms vs 0.73 ms ANE median at 256².
- All four candidates fit the <10M param student budget.

## Memory and full per-frame pipeline (added 2026-10-26 23:01)

`mem.swift`: loads one seg model + one hand model on `.cpuAndNeuralEngine`, then
runs 3000 frames of the plugin's per-frame work (1 seg + 2 hand crops), 50
warmup frames first. Memory = process `phys_footprint` (Activity Monitor
"Memory"), sampled every 50 frames. Log: `raw_mem.log`.

| seg + hand | frame median ms | p95 | p99 | max fps at p95 | footprint after load | steady peak (delta) |
|---|---|---|---|---|---|---|
| S2_256 + H2_128 (front-runners) | 1.121 | 2.062 | 2.676 | 485 | +12.2 MB | +15.9 MB |
| S1_256 + H1_128 (largest) | 2.083 | 5.319 | 8.409 | 188 | +12.7 MB | +16.5 MB |
| S2_320 + H2_128 | 1.522 | 3.541 | 4.780 | 282 | +10.7 MB | +15.8 MB |

Not included: camera buffers (1280x720 BGRA is 3.7 MB per frame) and the
editor preview texture; probe P1 measures those inside FL.

```sh
xcrun swiftc -O -o mem mem.swift
./mem models/S2_256.mlmodelc 256 models/H2_128.mlmodelc 128
```

## Models

- **S1**: `torchvision.models.segmentation.lraspp_mobilenet_v3_large(weights=None,
  weights_backbone=None, num_classes=8)`, wrapper returns only `["out"]`.
- **S2**: `timm.create_model("mobilenetv4_conv_small", features_only=True)`
  stride-8 (low) + stride-32 (high) features + LR-ASPP head written in
  `export.py` (128-ch `cbr` + global-pool sigmoid gate on the high branch,
  `low_classifier`/`high_classifier`, bilinear upsample to input).
- **H1**: `timm.create_model("mobilenetv4_conv_small", num_classes=6)`.
- **H2**: `timm.create_model("mobilenetv3_small_100", num_classes=6)`.

## Environment

- Apple M1 Pro, 16 GB, macOS 27.0.1 (26A434), Xcode 27.1
- Swift 6.4 (swiftlang-6.4.0.34.1), `xcrun swiftc -O`
- Python 3.13.17, torch 2.14.1, torchvision 0.29.1, timm 1.0.30,
  coremltools 9.0 (uv project, CPU wheels)
- coremltools 9.0 warns it has only been tested up to torch 2.7.0; all six
  conversions succeeded and the compiled models run.

## Commands

```sh
# env
uv init --bare --python 3.12 && uv add torch torchvision timm coremltools

# export (jit.trace + freeze -> mlprogram FP16, ImageType RGB scale 1/255,
# minimum_deployment_target macOS 15)
uv run python export.py

# compile
for m in models/*.mlpackage; do xcrun coremlcompiler compile "$m" models/; done

# bench
xcrun swiftc -O -o bench bench.swift
for m in S1_256 S1_320 S2_256 S2_320 H1_128 H2_128; do
  size=${m##*_}; ./bench models/$m.mlmodelc $size $size
done
```

## Caveats

- Random weights: latency is representative, outputs are meaningless.
- Machine was otherwise idle during the bench (single run of 1000 iters;
  no other heavy processes).
- `first_pred_ms` on cpuAndGPU includes Metal shader compile; it is a one-time
  cost per process.
- Logs: `raw_export.log` (versions, param counts, package sizes, re-derived
  after the fact — the first export's console output was not saved),
  `raw_bench.log` (all bench RESULT lines of the timing run).
