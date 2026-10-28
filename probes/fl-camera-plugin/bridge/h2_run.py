#!/usr/bin/env python3
"""H2 experiment runner: does FL Studio 2026 record automation from a
plugin whose editor is closed, per write path / gesture?

Usage: python3 h2_run.py [--channel-name NAME] [--cond NAME ...]
Requires flbridged running and the device script loaded in FL.

Per condition:
  1. control  Drive=Off, setpos 0, start, sample Signal 1 @20 Hz 6 s, stop
              (must be flat — guards false positives)
  2. arm      window on 1 s then off (captures GuiContext), set
              Path/Source=Triangle/Gesture, jump to fresh pattern, setpos 0
  3. record   Drive=Always (window closed), record on, start, 10 s, stop,
              record off
  4. judge    Drive=Off, setpos 0, start, sample Signal 1 @20 Hz 8 s, stop
              recorded = range > 0.3 and >= 20 distinct values
"""

import argparse
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from flb import call  # noqa: E402

RUNS = Path(__file__).parent / "runs"
RUNS.mkdir(exist_ok=True)

PLUGIN_NAME = "synthexis probe"
PARAM_NAMES = {
    "signal": "Signal 1",
    "drive": "Drive",
    "path": "Path",
    "source": "Source",
    "gesture": "Gesture",
}
# EnumParam -> normalized = index / (count - 1)
ENUM = {
    "drive": {"off": 0.0, "editor_only": 0.5, "always": 1.0},
    "path": {"a": 0.0, "b": 0.5, "d": 1.0},
    "source": {"lum": 0.0, "tri": 1.0},
    "gesture": {"per_write": 0.0, "held": 1.0},
}

CONDITIONS = [
    ("a_perwrite", dict(path="a", gesture="per_write", window=False)),
    ("b_perwrite", dict(path="b", gesture="per_write", window=False)),
    ("a_held", dict(path="a", gesture="held", window=False)),
    ("b_held", dict(path="b", gesture="held", window=False)),
    ("a_open_ref", dict(path="a", gesture="per_write", window=True)),
]


def must(reply):
    if "error" in reply:
        raise RuntimeError(f"bridge error: {reply}")
    return reply


def find_channel(name_hint):
    """Channel index whose plugin name (or channel name) matches."""
    chans = must(call("channels"))["channels"]
    hint = (name_hint or PLUGIN_NAME).lower()
    for c in chans:
        if hint in (c.get("plugin") or "").lower() or hint in (
            c.get("name") or ""
        ).lower():
            return c["i"], c
    raise RuntimeError(f"no channel matching {hint!r}: {chans}")


def find_params(ch):
    plist = must(call("params", ch=ch, max=128))["params"]
    idx = {}
    for key, want in PARAM_NAMES.items():
        for p in plist:
            if p["name"] == want:
                idx[key] = p["i"]
                break
        else:
            raise RuntimeError(f"param {want!r} not found in {plist}")
    return idx


def set_enum(pidx, ch, key, variant):
    must(call("set", ch=ch, idx=pidx[key], value=ENUM[key][variant]))


def sample(ch, sig_idx, seconds, hz=20):
    must(call("transport", op="start"))
    vals = []
    t0 = time.time()
    n = int(seconds * hz)
    for _ in range(n):
        vals.append(call("get", ch=ch, idx=sig_idx).get("v"))
        time.sleep(max(0.0, t0 + (len(vals)) / hz - time.time()))
    must(call("transport", op="stop"))
    return [v for v in vals if v is not None]


def run_condition(name, cfg, ch, pidx, pattern):
    log = {"condition": name, "config": cfg, "channel": ch, "params": pidx}
    # 1. control — must be flat
    must(call("setpos", pos=0))
    set_enum(pidx, ch, "drive", "off")
    control = sample(ch, pidx["signal"], 6)
    rng = max(control) - min(control) if control else 0.0
    log["control"] = {"n": len(control), "range": rng}
    if rng > 0.3:
        log["verdict"] = "abort:control_not_flat"
        return log
    # 2. arm — show window briefly for a GuiContext, configure, fresh pattern
    must(call("show", ch=ch, on=1))
    time.sleep(1.0)
    must(call("show", ch=ch, on=0))
    set_enum(pidx, ch, "path", cfg["path"])
    set_enum(pidx, ch, "source", "tri")
    set_enum(pidx, ch, "gesture", cfg["gesture"])
    must(call("pattern", n=pattern))
    must(call("setpos", pos=0))
    # 3. record — Drive=Always, window closed (or open for the reference)
    if cfg["window"]:
        must(call("show", ch=ch, on=1))
    set_enum(pidx, ch, "drive", "always")
    must(call("transport", op="record"))
    must(call("transport", op="start"))
    time.sleep(10)
    must(call("transport", op="stop"))
    must(call("transport", op="record"))
    if cfg["window"]:
        must(call("show", ch=ch, on=0))
    # 4. judge — replay and watch Signal 1
    set_enum(pidx, ch, "drive", "off")
    must(call("setpos", pos=0))
    series = sample(ch, pidx["signal"], 8)
    rng = max(series) - min(series) if series else 0.0
    distinct = len({round(v, 4) for v in series})
    log["judge"] = {
        "n": len(series), "range": rng, "distinct": distinct,
        "min": min(series) if series else None,
        "max": max(series) if series else None,
        "series": series,
    }
    log["verdict"] = (
        "recorded" if (rng > 0.3 and distinct >= 20) else "flat"
    )
    return log


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--channel-name", default=None)
    ap.add_argument("--cond", nargs="*", default=None)
    args = ap.parse_args()

    ping = call("ping")
    must(ping)
    ch, chinfo = find_channel(args.channel_name)
    pidx = find_params(ch)
    print(f"channel {ch} {chinfo} params {pidx}")

    conds = CONDITIONS
    if args.cond:
        conds = [c for c in CONDITIONS if c[0] in args.cond]
        if not conds:
            sys.exit(f"unknown conditions: {args.cond}")

    print(f"{'condition':14} {'control_rng':>11} {'judge_rng':>9} "
          f"{'distinct':>8} verdict")
    for i, (name, cfg) in enumerate(conds, start=10):
        try:
            log = run_condition(name, cfg, ch, pidx, pattern=i)
        except Exception as e:
            log = {"condition": name, "verdict": f"error:{e}"}
        out = RUNS / f"{int(time.time())}-{name}.json"
        out.write_text(json.dumps(log, indent=1) + "\n")
        j = log.get("judge", {})
        print(f"{name:14} {log.get('control', {}).get('range', float('nan')):11.3f} "
              f"{j.get('range', float('nan')):9.3f} {j.get('distinct', 0):8} "
              f"{log['verdict']}   -> {out}")


if __name__ == "__main__":
    main()
