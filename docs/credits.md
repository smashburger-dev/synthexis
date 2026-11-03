# Credit requests

Modal: submitted by Noa on 2026-10-26.

## Baseten ($100 + 2M TPM, reviewed by use case)

Project: synthexis, an open-source audio plugin (VST3/AU) that turns camera input, headphone head motion and heart rate into automation and MIDI for the instruments producers already own, starting with FL Studio. Indie track, theme "Build for Your World".

How we use Muse Spark Open on Baseten (`meta-models/muse-spark-open-1026`):

1. Parameter meaning at scale. We harvest every parameter (name, range, unit, value strings) from the VST3/AU plugins installed on a producer's machine, tens of thousands of entries. Muse Spark Open labels each one with a 40-class taxonomy (filter cutoff, reverb mix, envelope attack, unsafe output gain and so on) plus a safe range. These labels train a classifier under 1M parameters that runs offline inside the plugin, so a gesture can be mapped to any plugin by meaning.
2. Mapping agent. A producer types "my fist controls the reverb on the pad"; Muse Spark Open edits the route graph through tool calls and verifies the result by reading parameter values back.
3. Training-data checks. Muse Spark Open looks at synthetic training images and confirms the gesture each one should show before it enters the dataset.

Volume: roughly 10 to 15M input tokens for one labeling pass, plus agent sessions and image checks. With 2M TPM a full pass finishes in minutes, so we can re-run it after every taxonomy change during the hackathon. We are aiming for the Best Build on Meta Open Weight Models prize.

## Fireworks ($25 per team, shortlist)

Project: synthexis (description as above).

Use: Muse Glimmer 30B on Fireworks serverless inference as the second labeler next to Muse Spark Open. We keep a parameter or gesture label only when both open-weight models agree, which removes most label noise before the labels train our on-device classifier. Then a LoRA fine-tune of Muse Glimmer on the agreed labels (Fireworks SFT for Glimmer is free through Nov 30), served with the credits and measured against the untuned model on held-out plugins from vendors it has not seen.

## Attributions and licence notices

Required by the licences of the models and services synthexis uses. Source and reasoning for every line: `probes/licence-check/RESULT.md`. Written 2026-11-02 after Noa's decisions recorded there. The plugin's About panel and the README carry the first line; the licence texts are in `LICENSES/` at the repository root. The perception student weights are in `plugin/models/` (notice per model: `plugin/models/README.md`). The mapping parser weights ship as a GitHub release asset that `plugin/assistant/install.sh` downloads; the archive carries `NOTICE.txt` and the Muse and mmBERT licence texts (`plugin/assistant/README.md`). Their labels came from SAM 3.1, SAM 3D Body, MediaPipe and the recording script; none was written or filtered by Muse Spark Open (the Spark QA exclusion list was dropped before training, `probes/train-packer/RESULT.md`).

### Muse Spark Open

Built with Muse Spark Open.

The parameter-meaning classifier, the mapping parser and any perception student whose labels were filtered by Muse Spark Open were trained on its outputs and are Derivatives under the Muse Model License. Muse Spark Open is Copyright (c) 2026 Meta Platforms, Inc., licensed under the Muse Model License (version Muse Spark Open, 2026-10-15) and its Usage Policy; both texts ship with those models (`LICENSES/MUSE-MODEL-LICENSE.md`). Their synthetic training sentences and labels were written by AI models, not by people.

### SAM 3.1

The segmentation, hand and face students learn from masks produced by SAM 3.1 (Meta, `facebook/sam3.1`, code `facebookresearch/sam3`), used under the SAM License (2025-11-19). A copy of the SAM License ships with the student weights (`LICENSES/SAM-LICENSE.txt`); to the extent those weights are derivative works of the SAM Materials, they are distributed under that agreement.

### mmBERT small

The mapping parser's encoder is mmBERT small (Marone et al. 2025, arXiv 2509.06888, `jhu-clsp/mmBERT-small` at `abc32620`), with its vocabulary trimmed to German and English, under the MIT License. Notice for the shipped weights: "Copyright (c) 2025 the mmBERT authors, Johns Hopkins University CLSP. Released under the MIT License." (Upstream ships no copyright line; this wording is ours.) The full MIT text ships as `LICENSES/MIT-mmBERT.txt`.

### Vision backbones

MobileNetV4-Conv-Small (`timm/mobilenetv4_conv_small.e2400_r224_in1k`, seg and face students) and MobileNetV3-Small (`timm/mobilenetv3_small_100.lamb_in1k`, hand student) come from timm (Ross Wightman, Hugging Face) under the Apache License 2.0; both were pretrained on ImageNet-1k. The Apache text ships as `LICENSES/Apache-2.0.txt`, and the student weights are modified versions of these backbones.

### MediaPipe Hand Landmarker

The hand student learns most of its 21-point teacher keypoints from MediaPipe Hand Landmarker (Google), model bundle `hand_landmarker.task` (float16, sha256 `fbc2a300…cde1`, from Google's mediapipe-models bucket), run offline with mediapipe 1.2.0. The model card ("Hand Tracking (Lite/Full) with Fairness", Oct 2021) states "LICENSED UNDER Apache License, Version 2.0"; the mediapipe library is Apache-2.0 as well. The hand student weights carry this notice next to the timm backbones; the Apache text is the same `LICENSES/Apache-2.0.txt`.

### SAM 3D Body and DINOv3

The hand student also learns from SAM 3D Body (Meta, `facebook/sam-3d-body-dinov3` at `11aaa346`, code `facebookresearch/sam-3d-body`): 2,349 of 38,004 teacher hands and the left/right label of every hand it saw. SAM 3D Body is used under the SAM License; its DINOv3 backbone code (`facebookresearch/dinov3`) under the DINOv3 License (2025-08-19); its MHR body model under Apache-2.0. Copies of the SAM License (as shipped with SAM 3D Body, which adds a patent clause: `LICENSES/SAM-LICENSE-sam-3d-body.txt`) and the DINOv3 License (`LICENSES/DINOv3-LICENSE.md`) ship with the hand student weights; to the extent those weights are derivative works of either set of materials, they are distributed under those agreements.

### DeepSeek

Part of the parser's synthetic training sentences (stage 6) and some teacher labels were written by DeepSeek V4.1 Flash through the DeepSeek Open Platform API, whose terms allow training other models on its outputs. Plain-text credit only: no DeepSeek logo, and no wording that suggests a partnership or endorsement (terms section 5).

### Apple Vision

Used at runtime as an optional hand and face fallback and as an evaluation baseline; no attribution required. Apple's developer terms forbid training another AI model on Vision output, so no synthexis model is trained on Apple Vision output (Noa, 2026-11-02).
