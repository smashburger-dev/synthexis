#!/usr/bin/env python3
"""Live check in FL Studio: do real hand and face movements drive the
synthexis parameters? Uses the FL bridge (flbridged + the "synthexis bridge"
device script, probes/fl-camera-plugin/bridge/README.md) and never opens the
plugin window.

  python3 probes/plugin-perception/fl_live.py [--phase-s 8] [--pattern 60]

Per phase it prints what to do in front of the camera, then reads the
routed macro at 20 Hz (plugins.getParamValue through the bridge) and reports
its range. Last, it records 10 s of automation from right-hand open/close
with Live on and replays the pattern with Live off: a range above 0.3 means
FL recorded the perception signal as automation. Routes are the plugin's
built-in test routes (route.rs PERCEPTION_DEFAULTS), which run only while
the "Test signal" parameter is on; the script turns it on and restores it.
"""

import argparse
import json
import sys
import time
from pathlib import Path

BRIDGE = Path(__file__).resolve().parents[1] / "fl-camera-plugin" / "bridge"
sys.path.insert(0, str(BRIDGE))
from flb import call  # noqa: E402

OUT = Path(__file__).parent / "runs"

# (macro, signal id, instruction)
PHASES = [
    ("Signal 3", "cam0.hand.R.openness", "RIGHT hand up, open and close it slowly"),
    ("Signal 4", "cam0.hand.L.openness", "LEFT hand up, open and close it slowly"),
    ("Signal 6", "cam0.hand.R.y", "RIGHT hand open, move it up and down"),
    ("Signal 7", "cam0.hand.R.x", "RIGHT hand open, move it left and right"),
    ("Signal 8", "cam0.hand.R.pinch", "RIGHT hand: pinch thumb and index together, then apart"),
    ("Signal 5", "cam0.mouth.open", "hands down, open and close your mouth wide"),
    ("Signal 9", "cam0.eye.L.open", "close your LEFT eye, open it, repeat"),
    ("Signal 10", "cam0.head.yaw", "turn your head left and right"),
]


def must(r):
    if "error" in r:
        raise RuntimeError(r)
    return r


def channel():
    for c in must(call("channels"))["channels"]:
        if "synthexis" in (c.get("plugin") or "").lower():
            return c["i"]
    raise RuntimeError("no channel with plugin 'synthexis'")


def sample(ch, idx, secs, hz=20):
    vals, t0 = [], time.time()
    while time.time() - t0 < secs:
        v = call("get", ch=ch, idx=idx).get("v")
        if v is not None:
            vals.append(v)
        time.sleep(max(0.0, t0 + len(vals) / hz - time.time()))
    return vals


def summary(vals):
    if not vals:
        return {"n": 0}
    return {"n": len(vals), "min": round(min(vals), 3), "max": round(max(vals), 3),
            "range": round(max(vals) - min(vals), 3), "distinct": len({round(v, 3) for v in vals})}


def countdown(text, n=3):
    print(f"\n>>> {text}")
    for i in range(n, 0, -1):
        print(f"    {i} ...", flush=True)
        time.sleep(1)
    print("    go", flush=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--phase-s", type=float, default=8)
    ap.add_argument("--pattern", type=int, default=60)
    a = ap.parse_args()
    must(call("ping"))
    ch = channel()
    idx = {p["name"]: p["i"] for p in must(call("params", ch=ch, max=128))["params"]}
    test_was = must(call("get", ch=ch, idx=idx["Test signal"])).get("v", 0.0)
    must(call("rec", ch=ch, idx=idx["Test signal"], value=1.0))
    must(call("rec", ch=ch, idx=idx["Live"], value=1.0))
    log = {"channel": ch, "phases": []}
    for macro, sig, text in PHASES:
        countdown(text)
        s = summary(sample(ch, idx[macro], a.phase_s))
        s.update(macro=macro, signal=sig, task=text)
        log["phases"].append(s)
        print(f"    {macro} {sig}: {s}")

    # Automation: record right-hand openness, replay with Live off.
    must(call("pattern", n=a.pattern))
    must(call("setpos", pos=0))
    countdown("RIGHT hand: open and close for 10 s (recording automation)")
    must(call("transport", op="record"))
    must(call("transport", op="start"))
    time.sleep(10)
    must(call("transport", op="stop"))
    if must(call("transport", op="status")).get("recording"):
        must(call("transport", op="record"))
    must(call("rec", ch=ch, idx=idx["Live"], value=0.0))
    time.sleep(0.5)
    must(call("pattern", n=a.pattern))
    must(call("setpos", pos=0))
    print("\n>>> replaying the recorded pattern (Live off), keep still")
    must(call("transport", op="start"))
    replay = summary(sample(ch, idx["Signal 3"], 9))
    must(call("transport", op="stop"))
    must(call("rec", ch=ch, idx=idx["Live"], value=1.0))
    replay["verdict"] = "recorded" if replay.get("range", 0) > 0.3 else "flat"
    log["automation_replay"] = replay
    print(f"    replay Signal 3: {replay}")
    must(call("rec", ch=ch, idx=idx["Test signal"], value=test_was))

    OUT.mkdir(exist_ok=True)
    out = OUT / f"{int(time.time())}-fl_live.json"
    out.write_text(json.dumps(log, indent=1) + "\n")
    print(f"\n-> {out}")


if __name__ == "__main__":
    main()
