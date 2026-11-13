"""The bridge's own rec handler (plugin/fl/device_synthexis_bridge.py, run
inside FL through the dev script) on a stock plugin, with FL recording:
does the knob move, and does playback repeat the recorded automation?

  python3 probes/fl-stock-plugins/rec_test.py [--track 1 --slot 0 --plugin "Fruity Reeverb 2" --param 12]
  python3 probes/fl-stock-plugins/rec_test.py --ch 0 --plugin "3x Osc" --param 6   # a generator

Needs FL with the project open, the dev script and `fldev.py serve`, and no
synthexis instance in the project (README.md), and FL's record button off.
"""

import argparse
import json
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
from fldev import call  # noqa: E402

BRIDGE = ROOT / "plugin/fl/device_synthexis_bridge.py"


def must(code):
    r = call(code)
    if "error" in r:
        raise RuntimeError(f"{code[:70]}: {r['error']}")
    return r.get("r")


def where(a):
    """index, slot, useGlobalIndex of the plugin, as the bridge addresses it."""
    return (a.ch, -1, True) if a.ch is not None else (a.track, a.slot, False)


def value(a):
    i, s, g = where(a)
    return must(f"plugins.getParamValue({a.param}, {i}, {s}, {g})")


def sample(a, seconds, dt=0.1):
    out = []
    t0 = time.time()
    while time.time() - t0 < seconds:
        out.append(round(value(a), 4))
        time.sleep(dt)
    return out


def play_and_sample(a, seconds):
    i, s, g = where(a)
    must(f"plugins.setParamValue(0.5, {a.param}, {i}, {s}, 0, {g})")
    must("transport.stop()")
    must("transport.setSongPos(0)")
    must("transport.start()")
    vals = sample(a, seconds)
    must("transport.stop()")
    return {"min": min(vals), "max": max(vals), "distinct": len(set(vals)), "n": len(vals)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--track", type=int, default=1)
    ap.add_argument("--slot", type=int, default=0)
    ap.add_argument("--plugin", default="Fruity Reeverb 2")
    ap.add_argument("--param", type=int, default=12)
    ap.add_argument("--ch", type=int, default=None, help="a generator's global channel index")
    ap.add_argument("--seconds", type=float, default=3.0)
    a = ap.parse_args()
    must(f"B = {{}}; exec(open({str(BRIDGE)!r}).read(), B)")
    place = f"'ch': {a.ch}" if a.ch is not None else f"'fx': [{a.track}, {a.slot}]"
    rec = lambda v, p=a.plugin: must(  # noqa: E731
        f"B['_handle']({{'cmd': 'rec', {place}, 'p': {p!r}, 'i': {a.param}, 'v': {v}}})")
    i, s, g = where(a)
    res = {"plugin": a.plugin, "param": must(f"plugins.getParamName({a.param}, {i}, {s}, {g})")}

    # 1. the handler's writes move the parameter; another name is refused
    v0 = value(a)
    res["wrong_name"] = rec(0.9, "Some Other Plugin")
    time.sleep(0.3)
    res["after_wrong_name"] = value(a) == v0
    rec(0.2)
    time.sleep(0.3)
    res["write_0.2_reads"] = round(value(a), 4)

    # 2. playback before recording: the parameter stays where it is put
    res["before"] = play_and_sample(a, a.seconds)

    # 3. record a ramp through the handler, on the 128 steps the route writer sends
    must("transport.stop()")
    must("transport.setSongPos(0)")
    must("transport.record()")
    res["recording_flag"] = must("transport.isRecording()")
    must("transport.start()")
    n = int(a.seconds * 20)
    t0 = time.time()
    for k in range(n):
        rec(round(k / (n - 1) * 127) / 127)
        time.sleep(max(0.0, t0 + (k + 1) / 20 - time.time()))
    must("transport.stop()")
    # isRecording() reads 0 even while recording (probes/fl-target-picker), so
    # the record button is toggled back blindly: start every run with it off
    must("transport.record()")

    # 4. playback after recording, parameter parked at 0.5 first
    res["after"] = play_and_sample(a, a.seconds)
    print(json.dumps(res, indent=1))


if __name__ == "__main__":
    main()
