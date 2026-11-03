# Tuning and calibration handoff (2026-11-03)

How to change how perception values feel in the plugin without breaking what was measured. Numbers and the reasons behind each choice are in `RESULT.md`; this file says where the knobs are and how to turn them safely.

## Rules

- Choose on S1 to S3, score S4 once per frozen config. Hand choices use only the packer's dev segments (`data/packed/manifest.json`, `dev_segments`; the students trained on the rest). Face choices may use all of block C, because the face values come from Apple Vision landmarks.
- Add every choice that looked at S4 to the list in `RESULT.md` (section on the dev-only rule).
- Latency claims need a run on the Mac with median and p95, and the machine load next to them.
- Live feel is Noa's call. Offline numbers decide between candidates; `fl_live.py` and Noa's feedback decide whether a change ships.

## Where the knobs are

| what it changes | constant | file | chosen from |
|---|---|---|---|
| finger values and hand openness, index to pinky: anchors | `FINGER_BEND`, `FINGER_REACH` | `plugin/synthexis/src/engine/geom.rs` | block A, 40th percentile of extended and 60th of folded frames, non-dev segments |
| where openness reads 0 and 1 | `OPEN_LO`, `OPEN_HI` (`HandFeatures::openness`) | `geom.rs` | block A, p75 of fist and p25 of open-hand frames; one slope in between (the decile map of the B ramps was dropped 11-03) |
| where a hand's x splits from the other | `HAND_X_REACH`, `HAND_X_GAP`, `CENTRE_TAU_S` (`fit_hand_x`) | `plugin/synthexis/src/route.rs` | face centre (`cam0.body.x`); half a frame out, 0.08 dead band at the centre; on while the other hand has any route |
| thumb | formula in `hand_features` | `geom.rs` | unchanged since the stand-ins; re-anchoring it cost accuracy |
| pinch | `lin(d / palm, 0.25, 0.93)` in `hand_features` | `geom.rs` | H_pinch_drag (S2, S3): touching tips 0.1 to 0.3 palm apart |
| mouth open | `MOUTH_GAP` (0.06, 0.6) | `geom.rs`, used in `vision.rs` `face()` | block C: closed 0.02 to 0.04, wide 0.52 to 0.62 |
| eyes | `EYE_GEO`, `EYE_PX_GAIN`, `EYE_PX_GATE`, `EYE_PX_WARM` (`FaceAdapt`) | `geom.rs` | block C, leave one session out; warm-up from cold starts |
| smile | `SMILE_WIDTH` | `geom.rs` | block C |
| second hand blob | `HAND_PAIR_MIN` (0.1) | `geom.rs` | dev block A, present x finger |
| side of a single hand | `SideTracker`, `side_vote` | `geom.rs` | block A raw dumps, S1 to S3 |
| smoothing for every route | `Median3`, `OneEuro` defaults (1 Hz, beta 1, d 1), `Occlusion`, `ReacquireSlew` | `plugin/synthexis/src/cond.rs` | held poses (block A) against ramps; see below |
| which routes exist | `default_routes`, `PERCEPTION_DEFAULTS` (test routes, only with "Test signal" on) | `plugin/synthexis/src/route.rs` | Noa: only assigned routes move a macro |

## Workflow

1. Build: `cd plugin && ./guarded-cargo.sh build -p synthexis --release`.
2. Dumps for the lab: `../probes/plugin-perception/lab/dump.sh` writes `ba_*`, `bc_*`, `mc_*` and `pn_*` into `data/lab/` (or `$SYNTHEXIS_LAB`). It needs `data/recordings/` S1 to S3; both folders are git-ignored and live only on Noa's Mac.
3. Try candidates offline with the lab scripts in `lab/` (next section). They reimplement the Rust formulas in Python, so a candidate is cheap to score.
4. Change the constant in Rust, run `./guarded-cargo.sh test -p synthexis --release`, rebuild, re-dump and check the same numbers from the Rust output.
5. Score S4 once: `SYNTHEXIS_PERCEPTION=hybrid SYNTHEXIS_BENCH=../data/recordings/2026-11-01-S4/cam0.mov SYNTHEXIS_BENCH_SECONDS=266 SYNTHEXIS_BENCH_OUT=/tmp/s4.jsonl target/release/synthexis`, then `python3 ../probes/plugin-perception/eval.py /tmp/s4.jsonl ../data/recordings/2026-11-01-S4/segments.json`.
6. Install with FL closed: `./install.sh` (it refuses while FL runs and synthexis is installed).
7. Live: FL open with synthexis on a channel, the FL bridge running (`probes/fl-camera-plugin/bridge/README.md`), then `python3 probes/plugin-perception/fl_live.py` from the repo root. It turns "Test signal" on, walks through eight phases, records 10 s of automation and writes `runs/<time>-fl_live.json`. Latency lines are in `~/Library/Logs/synthexis.log` (`cam_to_param_ms`, `perception hybrid ... cb_to_emit cb_to_face`). The run file stores only min, max and distinct values per phase, so the middle band cannot be read from it.

Bench notes: `SYNTHEXIS_BENCH_SECONDS` is the absolute end time in the video, not a duration. `SYNTHEXIS_BENCH_RAW=1` adds every hand candidate with keypoints (`raw`), which the finger and side scripts need. `SYNTHEXIS_MODEL_DIR` points the bench at another model folder for an A/B.

## Lab scripts (`lab/`)

| script | what it answers |
|---|---|
| `paths.py` | shared paths, the session ids, the dev segments from the manifest |
| `ramps.py` | helpers: segment windows, per-finger cues (bend, reach), the current finger formula in Python, a One Euro filter |
| `eqlab.py` | fist curve: `make(p_fold, p_ext, eq_w)` builds anchors and the decile map, `evaluate()` gives dev finger accuracy, open and fist readings, time in the middle band and 20% to 80% time on the ramps |
| `euro_sweep.py`, `euro_med.py` | smoothing: jitter on held poses against ramp error and step response, for One Euro settings and a median prefilter (`python3 euro_med.py 3,1,1,1,0 5,0.5,4,0.3,0.01`: median, min cutoff, beta, d cutoff, dead band) |
| `mouthlab.py` | mouth cue anchors per session and the sweep, smile and shut-mouth readings for a range (`python3 mouthlab.py 0.06 0.6`) |
| `devscore.py` | dev block A scores for any dump pattern (`python3 devscore.py 'ba_{s}.jsonl'`) |
| `facesegs.py` | eye error and segment means for one block C dump (`python3 facesegs.py data/lab/mc_S1.jsonl data/recordings/2026-10-27-S1/segments.json S1`) |
| `eyelab.py`, `lsq.py` | eye feature extraction with running references and a small least-squares fit (how `FaceAdapt` was found) |
| `sidelab.py`, `sidesim.py` | the single-hand side rule replayed on raw dumps |
| `ratio.py` | area of the second hand blob against the first, in two-hand and one-hand frames |

## Typical feedback and where to start

- "Jumps between closed and open" or "pulls towards 0.5": the finger values are linear in bend and reach, openness linear between `OPEN_LO` and `OPEN_HI`. A decile map (`eqlab.make`, weight `eq_w`) slows the middle but holds half-curled hands at 0.5 to 0.6 and keeps fists off 0 (Noa, 11-03); wider anchors stop the ends from reaching 0 and 1. Check the open and fist readings and the held-pose step together; a slow live recording would settle the middle, the protocol ramps cannot.
- "Never reaches 1" (or 0): move that end's anchor quantile towards the middle (open: lower `p_ext` quantile of extended frames; pinch: raise the 0.25). The slope rises with it.
- "Too jittery when I hold still": held hand poses wander with a standard deviation of about 0.05, mostly slow drift that no filter removes without lag. Frame spikes are already cut by `Median3`. A wider median (5) or a lower One Euro min cutoff trades a little lag for less jitter (`euro_med.py`).
- "Mouth or eye fires when I turn my head or smile": `mouthlab.py` shows the shut-mouth and smile readings for any range; the eye logic is in `FaceAdapt`.
- A new person or camera: the anchors come from three recording sessions (S1 to S3). A per-person range (like `FaceAdapt`'s running references) is the next step if fixed anchors stop fitting.

## Known limits

- The hand keypoints come from the hand student; the `vision` backend feeds Apple Vision keypoints into the same formulas, but its keypoints were not checked against these anchors.
- The protocol's open ramps are fast open-and-hold moves (about 0.4 s per opening), not slow ramps, so they say little about fine control in the middle. Only live tests settle the feel.
- Live latency is mostly the camera: 50 to 130 ms from frame timestamp to callback. Perception adds 3 to 20 ms for hands (median) and the median filter one frame.
- Without "Test signal" no route moves a macro until assigned routes from the UI land (`docs/ux-plugin.md`).
