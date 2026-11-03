#!/bin/bash
# Bench dumps for the lab scripts (S1-S3 only; S4 is scored once per frozen
# config with eval.py, never tuned on). Run after a release build:
#   cd plugin && ./guarded-cargo.sh build -p synthexis --release
#   ../probes/plugin-perception/lab/dump.sh
# SYNTHEXIS_BENCH_SECONDS is the absolute end time, not a duration.
set -euo pipefail
cd "$(dirname "$0")/../../../plugin"
R=../data/recordings
W=${SYNTHEXIS_LAB:-../data/lab}
mkdir -p "$W"
bench() { # out backend start end session [raw]
    env SYNTHEXIS_PERCEPTION="$2" SYNTHEXIS_BENCH="$R/$5/cam0.mov" SYNTHEXIS_BENCH_START="$3" SYNTHEXIS_BENCH_SECONDS="$4" \
        ${6:+SYNTHEXIS_BENCH_RAW=1} SYNTHEXIS_BENCH_OUT="$W/$1" target/release/synthexis 2>&1 | sed -n 1p
}
# block A (finger states), raw candidates with keypoints: ba_S*.jsonl
bench ba_S1.jsonl students 0 258 2026-10-27-S1 raw
bench ba_S2.jsonl students 0 213 2026-10-29-S2 raw
bench ba_S3.jsonl students 0 213 2026-11-01-S3 raw
# blocks B and C (open ramps, face actions), raw: bc_S*.jsonl
bench bc_S1.jsonl hybrid 256 642 2026-10-27-S1 raw
bench bc_S2.jsonl hybrid 211 448 2026-10-29-S2 raw
bench bc_S3.jsonl hybrid 211 448 2026-11-01-S3 raw
# block C with a 60 s lead-in (FaceAdapt's running references settle as
# live) for the mouth and eye cues: mc_S*.jsonl
bench mc_S1.jsonl hybrid 469 642 2026-10-27-S1
bench mc_S2.jsonl hybrid 275 448 2026-10-29-S2
bench mc_S3.jsonl hybrid 275 448 2026-11-01-S3
# pinch drags (S2, S3), raw: pn_S*.jsonl
bench pn_S2.jsonl students 765 777 2026-10-29-S2 raw
bench pn_S3.jsonl students 754 766 2026-11-01-S3 raw
