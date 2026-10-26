# Hill-climb protocol

Every performance and quality number in synthexis is pushed in measured steps. One change per run; keep it only if the target metric improves by at least 5 % and every gate still holds. Log every run, kept or reverted, in [`hillclimb.tsv`](hillclimb.tsv).

## Judges

The judge is always a measurement on the real artifact, never the author's opinion:

- latency and memory: `probes/coreml-student-latency/mem` (Neural Engine, 3000 frames, median / p95 / p99, phys_footprint), later the same counters inside the plugin in FL Studio
- end-to-end: camera presentation timestamp to parameter change, logged by the plugin (probe P1 method)
- quality: the eval gate on the held-out session S4

## Metrics, baselines and targets

| Metric | Baseline (measured) | Target | Stretch |
|---|---|---|---|
| Perception per frame, seg + 2 hand crops, p95 | 2.06 ms (S2_256 + H2_128, random weights) | 1.0 ms | 0.5 ms |
| Total student parameters | about 3.0M | 2M | 1M, only if the quality gate holds |
| Memory added by the models | +15.9 MB | 12 MB | 8 MB |
| Camera to parameter, p95 | about 150 to 215 ms (standalone, dim light, 18.6 fps) | 60 ms at 30 fps | 35 ms with a 60 fps camera |
| Delivered fps | 18.6 (dim light) | camera rate, 30 built-in | 60 with iPhone or external camera |
| Dropped frames in 10 min | not measured | 0 | 0 |
| Plugin CPU at 30 fps | not measured | measure first | halve it |

Gates (must hold for any kept change): seg mIoU vs teacher >= 0.85 for person, face, hand and >= 0.70 for eye, mouth; finger-state accuracy >= 0.95; mouth-open and hand-openness MAE <= 0.05; no audio dropouts in FL at 128-sample buffers.

## Levers, roughly cheapest first

Camera and pipeline
1. Lock the capture frame rate (activeVideoMinFrameDuration = maxFrameDuration = 1/30 s) so the camera cannot lengthen exposure; better light.
2. Capture in 420v (YUV) and feed CoreML from the IOSurface without a BGRA conversion or copy.
3. Batch both hand crops into one prediction; reuse output buffers (outputBackings) so no frame allocates.
4. Run hand crops only for hands that are present; skip seg on every second frame and track boxes in between.

Model
5. Input size: 192, 224, 256, 320.
6. Width multiplier and depth of the backbone; MobileNetV4 small vs. MobileNetV3 small vs. a custom tiny encoder.
7. One merged model with a shared encoder and both heads (seg + keypoints) instead of two models.
8. Weight palettization (6 and 4 bit) and 8-bit quantization with coremltools; pruning.
9. Better training to buy back quality after shrinking: distillation from SAM 3.1 mask logits, more Muse Image data, augmentation for light and blur.

Each lever is one run. Combine only what survived on its own.
