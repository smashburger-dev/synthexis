# synthexis

Camera, headphones and hands as a playable control layer for the plugins producers already own.

synthexis is an audio plugin (VST3, AU, CLAP; Apple Silicon first) that watches one or more cameras, reads head motion and heart rate from Powerbeats Pro 2, and turns hands, face, body and objects into automation and MIDI inside the DAW. It does not ship its own synth. It bridges into the instruments and effects already in the project, starting with FL Studio.

Perception runs locally in real time on a student model distilled from Meta's SAM 3.1. Meta's open-weight Muse Spark Open labels plugin parameters so gestures can be mapped to any instrument by meaning.

Status: planning. Built during the Meta Global AI Developer Hackathon (Oct 26 to Nov 4, 2026). Everything in this repository was created during the event.

- Plan and specification: [`spec/`](spec/) and [`docs/`](docs/)
- Feasibility probes: [`probes/`](probes/)
