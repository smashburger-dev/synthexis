# Session 2026-10-27: SAM 3.1 real weights, recorder fix, S1 labels, motion layer

Entry point for the next session. Numbers cite their probe or manifest; unmeasured values are marked hypothesis.

## Results

- SAM 3.1 real weights on Modal ([probe A](../probes/sam31-modal/RESULT.md), section "Real weights"): HF access to `facebook/sam3.1` works (Modal secret `huggingface`). H100 video mode 69 to 98 ms per frame and concept, image mode 58.2 / 59.3 ms per text prompt (median / p95). Contact sheet checked by eye: person, face, eye, hand correct; "mouth" also fired on lips drawn on a wallpaper, hence the face filter below. Probe spend about $0.5.
- Muse Spark Open on Baseten works (key in Keychain `synthexis.baseten`, Modal secret `baseten`). Text: "Flt Freq" -> "filter". Image input works (synthetic test image answered "red circle"). Every call spends 60 to 80 reasoning tokens even with `reasoning_effort: none`, so batch 50 to 100 items per request and never set a tight `max_tokens`.
- Recorder ([tools/recorder](../tools/recorder/README.md)) rebuilt as v0.2.0 with `tools/recorder/build.sh` (compile with `-parse-as-library`, protocol bundled into the app, ad-hoc sign, `--selftest`). Recordings go to `~/Movies/synthexis/recordings/`; `data/recordings` is a symlink there.
- S1 recorded: built-in camera, room light, 18.2 min with pauses, 29.9 fps, 81 segments (protocol v1, no block H), one segment marked bad.
- Label pipeline [tools/labels](../tools/labels/): `uv run tools/labels/run.py <session_dir> --max-usd <cap> [--gpu L40S|H100]`. Extract 15 fps (segment boundaries within 0.07 s), pause skipping, SAM on the GPU only (raw COCO RLE per chunk and concept, cached per chunk and concept), local post-processing (top-k, eye/mouth >= 80 % inside face, hand area outlier filter), manifest, contact sheet.
- S1 labels (`data/labels/2026-10-27-S1/manifest.json`): 12,171 labelled frames, 8,811 with gesture labels, 3,898 pause frames skipped. Instances: person 12,170, face 11,641, hand 10,359 (477 dropped by the area filter), eye 12,340, mouth 8,711; objects phone 306, ball 291, demo_object (air freshener) 277, pen 3, mug 0 (it was a Pepsi can; prompt now "soda can", S1 rerun of that concept pending).
- GPU compare on one S1 chunk, identical detections: L40S $0.082, H100 $0.095 per 1000 frame-concepts. L40S is the default.
- Neighbour-frame IoU on S1 (1,500 sampled frames, 15 fps): person 0.94, face 0.91, hand 0.79 median between consecutive frames; hand 0.58 to 0.63 in hard cases and freestyle. Hands change a lot in 67 ms, so labelling at 10 fps would lose hand variety. Keep 15 fps.

## Cost

Modal by 2026-10-27, computed from container seconds: probe $0.5, label dev $0.2, S1 $17.88 (of which $17.51 in the first run), rework $0.75. About $5.7 of the $25 credit left until 2026-10-31; Noa allows up to $12.50 paid overage. Check against the Modal dashboard.

## Mistakes and fixes

- S1 first label run cost 2x its estimate: two thirds of H100 time went to post-processing. Fixed: GPU does only SAM, post-processing local, plus three guards in run.py (up-front cap, pilot re-estimate with abort above cap or 1.3x, rolling spend stop). All three triggered in tests.
- Recorder failures: (1) protocol decode expected a dict of dicts while the file has `version` and `finger_bits`; (2) my first fix never compiled because `| tail` hid the swiftc error; (3) the log was truncated on every launch, so an empty tail looked like success; (4) Finder launches had no Desktop permission after re-signing. Fixed by build.sh with `set -euo pipefail`, bundled protocol, ~/Movies output, append-only log, selftest.
- Recorder wrote list labels as "other", so SAM was prompted with letters. Fixed in recorder (`LabelValue.strings`) and pipeline (fallback from segment id).
- Pipeline used cam0 timestamps for every camera. Fixed in extract.py and run.py.

## Decisions (spec)

- No online button, not even opt-in (Noa). Bonus prize rests on the build-time Spark Open pipeline (`map.bonus_prize`).
- Spark Open labels freestyle segments from frame strips with SAM overlays and checks 3 frames per scripted segment (Noa consented to sending his frames to Baseten). It helps the students, never trains SAM (`motion.labels`).
- Motion layer `motion.*`: eight primitives (throw, swipe, push/pull, hold, pinch, circle, grab/release, two-hand), live readout, presets Pong (border zones, hit ramps with shapes) and Orb (throw, explode), own motifs via DTW. Basics stay core: finger states, chords, patterns, objects. Hand roles MIDI / Parameter / Motion / Off with presets chord_hand, filter_hand, pattern_hand (`gest.hand_role`, `gest.presets`).
- Protocol v2: block H with 18 motion segments; full 14.8 min, short 9.5 min with H and two freestyle segments for held-out motion data.
- Training on Modal from 2026-11-01 (Noa), setup in `train.setup` (packed shards, segment-level dev split, pretrained backbones, L4, free Mac preflight, same guards). Proposed: D9 proxy teacher (EUPE), D10 low-res frame plus 128 px face and hand crops, D11 temporal prior (MobileSAM2), D12 MobileNetV4-Small vs RepViT-M0.9 (EfficientSAM3); YOLO26 excluded (AGPL).

## Open, in order

1. S1: rerun concept `mug` with prompt "soda can" (cached empty result must be invalidated for that concept), cost under $0.2.
2. S2 at 15 fps on L40S, cap $7 (about $1 from the overage), as soon as Noa records it. S3 from 2026-11-01 on the new credit, S4 (short) on another day.
3. Spark Open freestyle timeline and segment QA (`motion.labels`): build, run on S1, Noa spot-checks.
4. Hardware probes P1 to P5 (Noa), then Welle 3: plugin skeleton with camera signal as recordable FL automation.
5. Training data packer and Mac preflight; student v1 on Modal from 2026-11-01.
6. Decide D9 to D12 with Noa; FigJam diagrams 1, 4, 5 regenerate from the spec.
