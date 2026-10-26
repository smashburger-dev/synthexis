# Apple Vision per-frame latency probe (M1 Pro)

300 frames from `data/test_clip.mp4` read via `AVAssetReader` (32BGRA), scaled
to 1280x720 with CoreImage, each `VNImageRequestHandler.perform` timed with
`ContinuousClock`. Stateless detectors → plain `VNImageRequestHandler` per
frame (no `VNSequenceRequestHandler` — none of these are tracking requests);
request objects created once and reused across frames.

## Results

| request | median ms | p95 ms |
|---------|-----------|--------|
| VNDetectHumanHandPoseRequest (maximumHandCount=2) | 18.4 | 27.8 |
| VNDetectFaceLandmarksRequest | 14.5 | 24.6 |
| VNDetectHumanBodyPoseRequest | 15.4 | 20.4 |
| VNGeneratePersonSegmentationRequest (.fast) | 8.6 | 24.4 |
| VNGeneratePersonSegmentationRequest (.balanced) | 25.3 | 54.1 |
| hand + face + body in one perform call | 29.6 | 46.6 |

Sanity (fraction of 300 frames with ≥1 detection): hands 100%, face 100%,
body 9.7%.

Takeaways:

- A combined perform of hand+face+body costs 29.6 ms vs ~48 ms summed solo —
  ~40% saving from one batched call.
- Vision hand+face at 1280x720 ≈ 33 ms/frame solo; adding body to the same
  call stays under a 30 fps budget (~34 ms). Person segmentation is extra.
- `.balanced` person segmentation alone nearly exhausts a 30 fps frame
  budget; `.fast` is usable.

## Test clip

- `data/test_clip.mp4`: Pexels video 8135472, "Woman Doing Hand Gestures
  While Looking at Camera", ANTÓNIO SHKRABA production.
- URL: https://www.pexels.com/video/woman-doing-hand-gestures-while-looking-at-camera-8135472/
- License: Pexels License (free to use, no attribution required).
- 1920x1080 @ 25 fps, 13.5 s; first 300 frames used.
- Frames also extracted to `data/test_frames/` (1280x720 JPEG) for the
  Modal probe.

## Environment / commands

- Apple M1 Pro, 16 GB, macOS 27.0.1, Swift 6.4, `xcrun swiftc -O -o vision_probe main.swift`
- `./vision_probe ../../data/test_clip.mp4 300`
- Machine otherwise idle during the run.
- Logs: `raw_vision.log` (final run). `raw_vision_run2_superseded.log` kept
  for provenance — it used a fresh set of request objects but the same
  handler, so the combo call returned Vision's per-handler cached results
  (0.6 ms) and does not reflect real work. The final run performs the combo
  on a fresh `VNImageRequestHandler`.

## Caveats

- The clip is a close-up: torso fills the lower frame, so the body-pose
  detector only finds a body in 9.7% of frames. Hand and face coverage is
  100%. Body numbers reflect the detector's search cost, not tracking
  quality — a full-body clip would be needed for body-detection recall
  numbers.
- Per-request timings include Vision's per-handler detector setup on first
  frames; medians are post-warmup behavior.
