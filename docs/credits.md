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
