#!/usr/bin/env python3
"""CamLat analysis: per flash edge, measure when the camera saw it.

Reads <ts>-<label>.jsonl (frame + edge records) and <ts>-<label>.info.json,
writes <ts>-<label>.summary.json and prints one result line.

Edge detection per edge t_e:
  baseline = median luma of frames with pts_host_s in [t_e-200ms, t_e-40ms]
  plateau  = median luma of frames with pts_host_s in [t_e+150ms, t_e+300ms]
  step     = plateau - baseline; edge rejected if |step| < 6*std(baseline)
  detected = first frame in callback order with cb_host_s > t_e whose luma
             crosses baseline + 0.5*step
"""

import json
import math
import statistics
import sys
from pathlib import Path


def med_p95(vals):
    if not vals:
        return (None, None)
    s = sorted(vals)
    n = len(s)
    med = s[n // 2] if n % 2 else 0.5 * (s[n // 2 - 1] + s[n // 2])
    p95 = s[round((n - 1) * 0.95)]
    return (med, p95)


def analyze(jsonl_path: Path):
    frames, edges = [], []
    for line in jsonl_path.read_text().splitlines():
        if not line.strip():
            continue
        rec = json.loads(line)
        if rec.get("t") == "e":
            if rec.get("t_e", 0) > 0:
                edges.append(rec)
        elif rec.get("t") == "f":
            frames.append(rec)
    frames.sort(key=lambda f: f["cb_host_s"])

    results = {"rising": [], "falling": []}
    rejected = 0
    for e in edges:
        t_e = e["t_e"]
        base = [f["luma"] for f in frames
                if t_e - 0.200 <= f["pts_host_s"] <= t_e - 0.040]
        plat = [f["luma"] for f in frames
                if t_e + 0.150 <= f["pts_host_s"] <= t_e + 0.300]
        if len(base) < 3 or not plat:
            rejected += 1
            continue
        baseline = statistics.median(base)
        plateau = statistics.median(plat)
        std = statistics.pstdev(base) if len(base) > 1 else 0.0
        step = plateau - baseline
        if abs(step) < 6 * std or abs(step) < 3.0:
            rejected += 1
            continue
        half = baseline + 0.5 * step
        det = None
        for f in frames:
            if f["cb_host_s"] <= t_e:
                continue
            ok = f["luma"] >= half if step > 0 else f["luma"] <= half
            if ok:
                det = f
                break
        if det is None:
            rejected += 1
            continue
        bucket = "rising" if step > 0 else "falling"
        results[bucket].append({
            "flash_to_cb": det["cb_host_s"] - t_e,
            "flash_to_pts": det["pts_host_s"] - t_e,
        })

    def agg(key):
        out = {}
        allv = results["rising"] + results["falling"]
        for name, group in [("combined", allv),
                            ("rising", results["rising"]),
                            ("falling", results["falling"])]:
            med, p95 = med_p95([g[key] * 1000 for g in group])
            out[f"{key}_{name}_med_ms"] = med
            out[f"{key}_{name}_p95_ms"] = p95
            out[f"{key}_{name}_n"] = len(group)
        return out

    pts_to_cb = [(f["cb_host_s"] - f["pts_host_s"]) * 1000 for f in frames]
    cb_med, cb_p95 = med_p95(pts_to_cb)

    # 1-s bucketed median pts_to_cb series + first second over 100 ms.
    series = []
    t_high = None
    if frames:
        t0 = frames[0]["cb_host_s"]
        buckets: dict[int, list[float]] = {}
        for f in frames:
            sec = int(f["cb_host_s"] - t0)
            buckets.setdefault(sec, []).append(
                (f["cb_host_s"] - f["pts_host_s"]) * 1000)
        series = [round(statistics.median(v)) for _, v in sorted(buckets.items())]
        for i, m in enumerate(series):
            if m > 100:
                t_high = i
                break
    fps = (len(frames) - 1) / (frames[-1]["cb_host_s"] - frames[0]["cb_host_s"]) \
        if len(frames) > 1 else 0.0
    summary = {
        "file": jsonl_path.name,
        "frames": len(frames),
        "dropped": frames[-1]["dropped"] if frames else 0,
        "delivered_fps": round(fps, 2),
        "mean_luma": round(statistics.mean([f["luma"] for f in frames]), 1)
        if frames else None,
        "edges_used": len(results["rising"]) + len(results["falling"]),
        "edges_rejected": rejected,
        "pts_to_cb_med_ms": cb_med,
        "pts_to_cb_p95_ms": cb_p95,
        "pts_to_cb_series_ms": series,
        "pts_to_cb_t_high_s": t_high,
        "low_frac": round(sum(1 for m in series if m < 95) / len(series), 3)
        if series else None,
        **agg("flash_to_cb"),
        **agg("flash_to_pts"),
    }
    info_path = jsonl_path.with_suffix(".info.json")
    if info_path.exists():
        summary["info"] = json.loads(info_path.read_text())
    out_path = jsonl_path.with_suffix(".summary.json")
    out_path.write_text(json.dumps(summary, indent=1) + "\n")

    def f(v):
        return f"{v:.1f}" if v is not None else "n/a"

    print(
        f"{jsonl_path.name}: flash_to_cb med={f(summary['flash_to_cb_combined_med_ms'])} "
        f"p95={f(summary['flash_to_cb_combined_p95_ms'])}ms "
        f"(r {f(summary['flash_to_cb_rising_med_ms'])}/{f(summary['flash_to_cb_rising_p95_ms'])} "
        f"f {f(summary['flash_to_cb_falling_med_ms'])}/{f(summary['flash_to_cb_falling_p95_ms'])}) "
        f"flash_to_pts med={f(summary['flash_to_pts_combined_med_ms'])} "
        f"p95={f(summary['flash_to_pts_combined_p95_ms'])} "
        f"pts_to_cb med={f(cb_med)} p95={f(cb_p95)} "
        f"t100={t_high}s low_frac={summary['low_frac']} "
        f"p2c_1s={','.join(str(m) for m in series)} "
        f"fps={summary['delivered_fps']} drops={summary['dropped']} "
        f"edges={summary['edges_used']}/{rejected} "
        f"luma={summary['mean_luma']} load={summary.get('info', {}).get('load_start')}"
        f"->{summary.get('info', {}).get('load_end')}"
    )
    return summary


if __name__ == "__main__":
    for arg in sys.argv[1:]:
        for p in sorted(Path(arg).parent.glob(Path(arg).name)):
            analyze(p)
