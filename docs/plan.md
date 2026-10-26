# synthexis plan

Single source of truth for features and decisions: [`spec/synthexis.yaml`](../spec/synthexis.yaml). Visual board: [FigJam, 8 diagrams](https://www.figma.com/board/WJIE59w34hCSTXuq3XpKGH). This file covers judging, Meta model use, the demo, risks and the schedule.

## Judging fit (Indie track, Stage 2 rubric)

| Category | Weight | What earns the points here | Evidence we will show |
|---|---|---|---|
| Impact | 30 % | Producers keep their own plugins; synthexis adds a playable camera layer on top. Theme "Build for Your World": Noa is a producer and builds the tool he wants. | Live FL Studio session with third-party plugins, recorded MIDI and automation |
| Use of Meta AI tooling | 25 % | SAM 3.1 distilled into an on-device student that runs every frame for free; SAM 3.1 API for snapshot cut-outs; Muse Spark Open labels plugin parameters and drives a mapping agent; Muse Image renders diverse training people | Teacher vs student numbers on screen: parameters, ms per frame, $ per hour |
| Technical depth and execution | 20 % | Audio thread never waits on vision; lock-free value snapshot; One Euro filter; occlusion state machine; eval gate on a held-out session; measured latency budget | Probe RESULT files, eval report, latency histogram |
| Demo | 25 % | A music performance is watchable by itself; hands, face and headphones visibly play the track | 3-minute video, see storyboard |

Stage 1 (pass/fail) needs a theme fit and real use of Meta Model API or open weights: covered by SAM 3.1 (API and open weights) and Muse Spark Open.

Anti-project check: the guide lists "simple image analyzers". synthexis is an instrument that writes MIDI and automation into a DAW; the demo must open on music, not on detection boxes.

### Bonus prizes we can target

| Prize | Condition | Plan |
|---|---|---|
| Best Build on Meta Open Weight Models, $10K | Muse Glimmer or Muse Spark Open, own hardware or partner inference | Muse Spark Open on Baseten labels parameters and runs the mapping agent |
| Most Viral, $20K | Engagement on X until Nov 8 | Short performance clip, posted on submission day |
| Community Vote, $10K | Gallery votes | Same clip, ask Noa's audience |
| Best Agentic Use-Case, $5K | Agent autonomy and usefulness | Mapping agent edits routes from a sentence, verifies by reading parameter values back |
| Best Creative Build, $5K | Muse Image or Muse Wonder in the final experience | Not targeted; Muse Image only feeds training data |

## How each Meta model is used

| Model | Access | Job | Cost estimate |
|---|---|---|---|
| SAM 3.1 open weights | Modal GPU | Bulk teacher masks on recorded and synthetic frames | Probe A reports $ per 1000 frames |
| SAM 3.1 API (`sam-3.1`) | Meta Model API | Snapshot cut-outs in the plugin; cross-check sample of the Modal labels | $2.50 per 1000 images, $0.20 per 1000 video frames (min 50 frames per request) |
| Muse Spark Open | Baseten `meta-models/muse-spark-open-1026` | Parameter meaning labels, gesture-label check on synthetic images, mapping agent | Baseten credits if granted |
| Muse Image (`muse-image-1.0`) | Meta Model API | Synthetic people, hands, rooms, light for student training | $0.01 per image |
| Muse Glimmer 30B | Fireworks serverless (`muse-glimmer-30b`) | Second labeler: keep only labels where Glimmer and Spark Open agree; LoRA fine-tune on agreed labels | Inference from Fireworks credits if granted; SFT free on Fireworks through Nov 30 |
| Muse Code | subscription or pay-as-you-go | Optional coding agent | none planned |

Why not SAM 3.1 in real time from the plugin: 30 fps x 3600 s = 108,000 frames per hour, at $0.20 per 1000 frames that is $21.60 per hour per camera plus a network round trip per frame. The student costs $0 and runs on the laptop.

## The student models

- synthexis-seg: semantic masks [background, person, face, eye, mouth, hand, object], at most 4M parameters, CoreML FP16 on the Neural Engine. Candidates measured in probe B: LRASPP-MobileNetV3-Large and MobileNetV4-Conv-Small + LR-ASPP head.
- synthexis-hand: hand crop 128 px to 5 finger-extension probabilities + openness, at most 2.5M parameters. It must beat Apple Vision keypoint geometry on the held-out session to replace it.
- Total stays under 10M parameters.
- Labels: SAM 3.1 masks for segmentation; finger states from the recording script (each segment is one known state), so no labeler can invent them. Muse Spark Open only checks synthetic images.
- Eval gate (held-out session recorded on another day, never trained on): mIoU vs teacher >= 0.85 for person, face, hand and >= 0.70 for eye, mouth; finger-state accuracy >= 0.95; mouth-open and hand-openness signal MAE <= 0.05; seg p95 <= 3 ms and hand p95 <= 1.5 ms on the M1 Pro.

## Demo storyboard (3:00)

| Time | Shot | Proves |
|---|---|---|
| 0:00-0:12 | FL Studio project plays. Noa opens his fist, the pad's reverb blooms. Title. | It is an instrument |
| 0:12-0:35 | Plugin UI: camera canvas with masks, keypoints and live values; the pad is a third-party synth | Bridge, no own synth |
| 0:35-1:05 | Right hand finger states switch hi-hat basic, trap roll, snare fill; left hand plays chords in key; FL piano roll fills live | Gestures to MIDI, recorded |
| 1:05-1:25 | Mouth opens the filter, Powerbeats head turn sweeps a delay; Noa turns away, the value holds, glides back | Occlusion handling |
| 1:25-1:50 | Snapshot: hand cut-out pinned on the canvas; live hand touches it and triggers; a mug's contour becomes a melody | SAM 3.1 API, anchors |
| 1:50-2:20 | Under the hood: SAM 3.1 on Modal labelled N frames, Muse Image people, student size and ms per frame next to $21.60/h; "fist controls reverb on the pad" typed, route appears | Meta tooling, technical depth |
| 2:20-2:45 | Automation lanes from the take; iPhone camera from above the desk as a second source | Recording, multi-camera |
| 2:45-3:00 | What was built during the hackathon, repo link | Rules compliance |

## Risks and fallbacks

| Risk | Probe | Fallback |
|---|---|---|
| Camera inside the plugin fails in FL | P1 | Helper app owns the camera, shares values over shared memory |
| FL does not record automation of macros changed off the GUI thread | P2 | Keep the editor open while recording; record via Link to controller |
| FL script cannot set or record parameters | P3 | Macros + Link to controller only |
| Powerbeats motion not exposed to third-party apps | P4 | Camera head pose only |
| Heart rate not readable while connected as audio | P5 | Drop heart rate from the demo |
| SAM 3.1 weights access delayed (gated on Hugging Face) | A | Label through the API: 160,000 frames cost $32 |
| Student misses the gate | eval | Vision keypoints for hands and face; student only for objects and cut-outs |

Cut order if the schedule slips: AUv2, heart rate, param-sense student (keep labels + rules), cameras beyond two, contour melody, zone anchors.

## Schedule

See FigJam "synthexis 8 Build plan". Hard dates: probes Oct 27, first end-to-end plugin (camera to macro in FL) Oct 29, student v1 Oct 30, feature freeze Nov 2, demo recording Nov 3, submit Nov 4 before 21:00 ET (Nov 5 03:00 CET).

## Needs from Noa

1. Claim the $150 Meta Model API credits (form in the participant guide, needs a card on the account, set a spend limit), then create an API key.
2. Hugging Face: request access to `facebook/sam3.1`, create a read token, then run `uvx modal secret create huggingface HF_TOKEN=<token>` yourself so the token never passes through chat.
3. Baseten and Fireworks requests with the texts in [`credits.md`](credits.md).
4. Decision D7 in the spec: license (GPL-3.0 with nih-plug's VST3 export, or MIT-compatible via clap-wrapper).
