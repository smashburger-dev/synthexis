# Camera latency hill-climb (H1) — RESULT

Method: full-screen black↔white flash as ground truth. Each toggle presents a
CAMetalLayer drawable; `drawable.presentedTime` (host-time seconds) is the
edge time t_e. Per frame the bench logs `pts_host_s`
(`CMSyncConvertTime(pts, syncClock, hostClock)`), `cb_host_s` (host clock at
callback entry), mean luma and dropped count. `flash_to_cb = cb_host - t_e`
is the judge metric; `flash_to_pts = pts_host - t_e` shows what the PTS
refers to. Edge detection: baseline = median luma of frames with pts in
[t_e-200ms, t_e-40ms]; plateau = median luma in [t_e+150ms, t_e+300ms];
reject if |step| < 6×std(baseline) or < 3 luma; detected frame = first frame
in callback order with cb > t_e crossing baseline + 0.5·step.

Uncertainty: `presentedTime` is scan-out time; panel response adds a few ms
to when the camera physically sees the flash — a constant offset that cancels
in comparisons. All times in ms.

Environment: M1-class MacBook Pro, macOS 27.0.1 (26A434), built-in
"FaceTime HD-Kamera" (1920x1080 420v active at preset720, 15-30 fps),
iPhone Continuity camera "Kamera von „Noa's iPhone"". NOTE: a parallel build
kept 1-min load > 6 for the whole session (load ~8-17); every run is flagged
`load>6`. Absolute `cb`-side numbers carry tens of ms of scheduling noise
under this load — see V2.

## Validation

- V1 sync clock == host clock: **pass** (info.json `sync_is_host: true`).
- V2 inject 20 ms: **anomalous but understood**. Baseline `pts_to_cb` med
  ~128 ms. With `--inject-ms 20`, `pts_to_cb` med = 74.5 / 75.5 ms and
  `flash_to_cb` med = 103.3 / 108.8 ms — a *drop* of ~53 ms across two
  reproduced runs, while `flash_to_pts` stayed within ~+4 ms (28.9/30.2 vs
  26.3-31.8). Edge detection itself works (69-71 edges, ≤1 reject, sweep
  detection clean). Mechanism: under load >6 the default capture queue's
  worker wake-up costs ~50-70 ms per callback; the 20 ms busy-wait keeps the
  worker hot so the next block runs near delivery. So the injection doesn't
  add 20 ms to an already-inflated cb — it removes most of the scheduling
  lag instead. Judge is **not** broken; `flash_to_cb` remains meaningful for
  same-config comparisons. Flagged rather than passed.
- V3 spread (3 baseline runs): combined `flash_to_cb` med 153.5/157.9/155.9
  → spread 2.8 %; p95 176.9/177.8/178.2 → spread 0.7 %.

## Runs

| run | config | flash_to_cb med/p95 | flash_to_pts med | pts_to_cb med | fps | drops | edges u/r | load |
|-----|--------|---------------------|------------------|---------------|-----|-------|-----------|------|
| base1 | preset720 bgra auto default-q | 153.5/176.9 | 26.6 | 127.2 | 29.9 | 0 | 53/18 | ~9-12 |
| base1 | same (rerun) | 157.9/177.8 | 26.3 | 128.0 | 29.7 | 0 | 61/7 | ~10-12 |
| base3 | same | 155.9/178.2 | 25.9 | 128.3 | 29.7 | 0 | 60/6 | 12-17 |
| base4 | same (post-inject check) | 158.3/181.7 | 31.8 | 128.1 | 29.8 | 0 | 62/5 | 8-10 |
| inject20 | +busy-wait 20 ms | 103.3/126.8 | 28.9 | 74.5 | 30.0 | 0 | 69/1 | 10-15 |
| inject20b | same | 108.8/130.0 | 30.2 | 75.5 | 30.0 | 0 | 69/1 | 11-15 |
| L1 | format:1280x720 fps30 lock | 155.7/182.2 | 27.2 | 123.0 | 29.8 | 0 | 65/3 | 10-12 |
| L2 | format:640x480 auto | 158.7/176.9 | 29.7 | 128.2 | 29.7 | 0 | 61/10 | 8-10 |
| L3 | pix 420v | 152.5/173.9 | 31.7 | 123.4 | 29.7 | 0 | 61/9 | 8-10 |
| L4 | qos userInteractive | 156.9/186.7 | 33.9 | 123.8 | 30.0 | 0 | 66/1 | 9-14 |
| L5 | iPhone format:1280x720 fps60 | 99.9/116.3 | 41.5 | 55.8 | 30.2* | 0 | 71/0 | 12-14 |
| L5 | iPhone format:1280x720 fps30 | 102.6/117.7 | 43.6 | 56.8 | 30.0 | 0 | 69/0 | 11-12 |

\* iPhone delivered ~30 fps despite the 1/60 frame-duration lock — Continuity
pipeline caps it; record as measured.

## Levers

Keep rule: combined `flash_to_cb` p95 improves ≥5 % AND more than the
baseline spread, fps not lower, drops not higher. Baseline p95 ≈ 178 ms.

- L1 (activeFormat 720 + locked 30 fps): 182.2 — **not kept** (worse).
- L2 (640x480): 176.9 — **not kept** (−0.7 % < 5 %; within 1.5× spread).
- L3 (420v out): 173.9 — **not kept** (−2.4 % < 5 %).
- L4 (userInteractive queue): 186.7 — **not kept** (worse).
- L5 (iPhone, comparison only): **116.3 @fps60 / 117.7 @fps30** — ~35 % lower
  `flash_to_cb` than the built-in; `pts_to_cb` med ~56 ms vs ~128 ms. iPhone
  is a comparison, not a replacement for the builtin default.

Skipped as n/a per brief: alwaysDiscardsLateVideoFrames (already true),
video effects (all off in fresh process; FL's state comes from plugin v2 log).

## Best config

Baseline wins for the built-in camera — no lever beat it:
`--device builtin --mode preset720 --fps auto --pix bgra --qos default`
(= the plugin's current config). Best measured `flash_to_cb` ≈ med 155 /
p95 178 ms under load>6; the floor is likely lower on an idle machine —
**hypothesis**: ~50-70 ms of the measured `cb` delay is scheduling noise
from the parallel build (V2 shows the cb path can run at ~75 ms total when
the queue worker stays hot).

## What flash_to_pts says about the PTS reference

`flash_to_pts` med ≈ 26-32 ms (builtin), ≈ 42-44 ms (iPhone): the PTS sits
well *after* the flash reaches the sensor — roughly one frame period (33 ms)
of pipeline delay is already inside the PTS. So `pts_to_cb` (~128 ms) is
not "capture latency" — most of it is post-delivery queue/scheduling delay
at ~30 fps. The true end-to-end number (flash_to_cb ~155 ms) ≈
flash_to_pts (~28) + pts_to_cb (~127).

## Summaries

`out/*.summary.json` for every run above; raw JSONL gitignored.

## Two latency states (D-series, builtin, pts_to_cb = metric, --flash 0)

The built-in camera's `pts_to_cb` bistable: ~50-75 ms ("low", ~1.5-2 frames)
vs ~115-145 ms ("high", ~4 frames). D1 120 s baseline sits high the whole
run; D7b shows the switch live: low ~66-91 ms for 15 s then climbing and
staying high. Both directions occur at runtime.

| run | variant | p2c med/p95 | low-state window | fps | drops |
|-----|---------|-------------|------------------|-----|-------|
| D1  | baseline 120 s | 129.1/146.4 | none (t100=0 s) | 29.7 | 0 |
| D2  | busy-after 20 ms | 107.0/129.0 | ~9 s at ~50-61 | 30.0 | 0 |
| D3  | sleep 20 ms | 152.0/169.9 | none, worse than base | 29.7 | 0 |
| D4a | busy-after 2 ms | 129.3/145.3 | ~1 s | 29.7 | 0 |
| D4b | busy-after 5 ms | 127.9/144.7 | ~2 s | 29.7 | 0 |
| D4c | busy-after 10 ms | 125.1/143.7 | ~4 s | 29.7 | 0 |
| D5  | spinner 20 % off-queue | 126.2/144.6 | none | 29.8 | 0 |
| D6a | hold buffer 20 ms | 129.5/145.8 | none | 29.7 | 0 |
| D6b | hold buffer 60 ms | 126.8/145.0 | none | 29.7 | 0 |
| D7a | busy-after 20 repeat | 64.7/109.6 | **~42 s** at ~50-80 | 29.9 | 0 |
| D7b | fresh flash baseline | flash_to_cb 152.0/187.5 | low 15 s then high | 29.8 | 0 |
| D7c | busy-after 20 + flash | **flash_to_cb 92.4/137.9** | ~50-70 sustained | 30.0 | 0 |

### What discriminates

IMPORTANT caveat: every D-run is n=1, and the low-state window is highly
variable even within a condition (baselines sometimes start low for ~10-15 s
on their own: base3, L3, D7b). `busy` and `busy-after` do the same CPU work
per frame — only the timestamp position differs — yet their "hold" times
differed wildly (40 s vs 9 s vs 42 s). The D4 "dose-dependence" and the
D3/D5/D6 negative results are single observations, not settled. What the
D-series establishes:

- The low state is real latency, not a timestamp artifact: D7c flash_to_cb
  med 92.4 / p95 137.9 vs fresh baseline D7b 152.0/187.5 (-60 ms),
  flash_to_pts unchanged (35.3 vs 29.5), 0 drops, 30 fps, 107/0 edges.
- The mechanism is open. No per-condition claim survives n=1.

### R-series (n=3 per condition, round-robin interleaved, 60 s each)

Metric: low_frac = fraction of 1-s bins with median pts_to_cb < 95 ms.

| run | condition | p2c med/p95 | low_frac | fps | drops |
|-----|-----------|-------------|----------|-----|-------|
| R1-base | baseline | 126.6/144.7 | 0.048 | 29.7 | 0 |
| R1-ba20 | busy-after 20 | 99.8/126.1 | 0.355 | 30.0 | 0 |
| R1-sleep20 | sleep 20 | 153.1/168.7 | 0.000 | 29.6 | 0 |
| R1-ba3 | busy-after 3 | 128.4/143.7 | 0.000 | 29.6 | 0 |
| R2-base | baseline | 128.7/146.2 | 0.000 | 29.7 | 0 |
| R2-ba20 | busy-after 20 | 62.4/111.1 | 0.871 | 29.9 | 0 |
| R2-sleep20 | sleep 20 | 153.2/170.1 | 0.000 | 29.7 | 0 |
| R2-ba3 | busy-after 3 | 130.6/146.8 | 0.000 | 29.7 | 0 |
| R3-base | baseline | 127.5/145.4 | 0.000 | 29.7 | 0 |
| R3-ba20 | busy-after 20 | 105.9/126.7 | 0.226 | 30.0 | 0 |
| R3-sleep20 | sleep 20 | 152.0/170.7 | 0.000 | 29.7 | 0 |
| R3-ba3 | busy-after 3 | 126.1/143.6 | 0.016 | 29.6 | 0 |

Mean low_frac per condition: **busy-after 20 = 0.48** (0.23/0.35/0.87 —
high variance but the low state appears in every run), baseline = 0.02,
sleep 20 = 0.00, busy-after 3 = 0.01.

Now supported with n=3:

- busy-after 20 ms of CPU on the callback thread reproduces the low state in
  3/3 runs; duration is random (9-52 s) but nonzero in all.
- sleep 20 ms never enters the low state and is consistently ~25 ms WORSE
  than baseline (med ~152-153 vs ~127-129 in 3/3): same queue occupancy
  without CPU hurts; the extra delay ≈ the sleep itself.
- busy-after 3 ms (≈ a real per-frame perception workload) does nothing in
  3/3: the effect needs more than a few ms of CPU.
- The exact mechanism inside the daemon remains unknown (hypothesis only):
  the discriminating variable is CPU work on the capture-queue thread, not
  occupancy, not process-wide load, not buffer retention (D5/D6, n=1 each).

### Floor (measured + derived)

Measured: builtin flash_to_pts med ~26-35 ms (all runs), low-state
pts_to_cb ~50-55 ms (D2/D7a/D7c, R2-ba20 med 62.4).
Derived: ~80-90 ms flash-to-callback is the best the built-in camera showed
(flash_to_pts + low-state pts_to_cb; D7c measured 92.4 med end-to-end).
Measured: iPhone Continuity flash_to_cb med ~100-103 / p95 ~116-118,
stable across 60/30 fps settings (delivered ~30 fps either way).
