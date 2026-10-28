#!/usr/bin/env python3
"""Gate test for the synthexis plugin (run later, on the lead's signal):
does FL record automation written by the plugin with its window closed?

Uses flb.py against the running flbridged + device script. NEVER opens the
plugin window. Steps:
  1. find the channel whose plugin is "synthexis"
  2. Test signal on, Live on; jump to a fresh pattern (50); setpos 0
  3. control: Live off, playback, sample Signal 32 @20 Hz 8 s — must be flat
  4. record: Live on, record on, start, 10 s, stop, record off, Live off
     (also samples Signal 1/2 mid-record to show camera-driven motion)
  5. judge: setpos 0, playback, sample Signal 32 @20 Hz 8 s
     verdict recorded = range > 0.3 and >= 20 distinct values
"""

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from flb import call  # noqa: E402

RUNS = Path(__file__).parent / "runs"
RUNS.mkdir(exist_ok=True)
PLUGIN = "synthexis"


def must(r):
    if "error" in r:
        raise RuntimeError(r)
    return r


def find():
    for c in must(call("channels"))["channels"]:
        if PLUGIN in (c.get("plugin") or "").lower():
            return c["i"]
    raise RuntimeError("no channel with plugin 'synthexis'")


def params(ch):
    idx = {}
    for p in must(call("params", ch=ch, max=128))["params"]:
        idx[p["name"]] = p["i"]
    for want in ["Signal 1", "Signal 2", "Signal 32", "Live", "Test signal"]:
        if want not in idx:
            raise RuntimeError(f"param {want!r} missing: {sorted(idx)}")
    return idx


def setp(ch, idx, name, v):
    must(call("set", ch=ch, idx=idx[name], value=v))


def getp(ch, idx, name):
    return call("get", ch=ch, idx=idx[name]).get("v")


def sample(ch, idx, name, secs, hz=20):
    must(call("transport", op="start"))
    vals = []
    t0 = time.time()
    for _ in range(int(secs * hz)):
        vals.append(getp(ch, idx, name))
        time.sleep(max(0.0, t0 + len(vals) / hz - time.time()))
    must(call("transport", op="stop"))
    return [v for v in vals if v is not None]


def main():
    must(call("ping"))
    ch = find()
    idx = params(ch)
    print(f"channel {ch} params { {k: v for k, v in idx.items()} }")
    log = {"channel": ch, "params": idx}

    setp(ch, idx, "Test signal", 1.0)
    setp(ch, idx, "Live", 1.0)
    must(call("pattern", n=50))
    must(call("setpos", pos=0))

    # Control: Live off -> Signal 32 must be flat on playback.
    setp(ch, idx, "Live", 0.0)
    control = sample(ch, idx, "Signal 32", 8)
    cr = max(control) - min(control) if control else 0.0
    log["control"] = {"n": len(control), "range": cr}
    if cr > 0.3:
        log["verdict"] = "abort:control_not_flat"
        _dump(log)
        return
    setp(ch, idx, "Live", 1.0)

    # Record 10 s; sample Signal 1/2 midway to show camera motion.
    must(call("transport", op="record"))
    must(call("transport", op="start"))
    time.sleep(5)
    log["mid_record"] = {
        "sig1": getp(ch, idx, "Signal 1"),
        "sig2": getp(ch, idx, "Signal 2"),
        "sig32": getp(ch, idx, "Signal 32"),
    }
    time.sleep(5)
    must(call("transport", op="stop"))
    must(call("transport", op="record"))
    setp(ch, idx, "Live", 0.0)

    # Judge: replay and watch Signal 32.
    must(call("setpos", pos=0))
    series = sample(ch, idx, "Signal 32", 8)
    rng = max(series) - min(series) if series else 0.0
    distinct = len({round(v, 4) for v in series})
    log["judge"] = {
        "n": len(series), "range": rng, "distinct": distinct,
        "min": min(series) if series else None,
        "max": max(series) if series else None,
        "series": series,
    }
    log["verdict"] = "recorded" if (rng > 0.3 and distinct >= 20) else "flat"
    _dump(log)


def _dump(log):
    out = RUNS / f"{int(time.time())}-gate_synthexis.json"
    out.write_text(json.dumps(log, indent=1) + "\n")
    j = log.get("judge", {})
    print(f"control_range={log.get('control', {}).get('range')} "
          f"judge_range={j.get('range')} distinct={j.get('distinct')} "
          f"mid={log.get('mid_record')} verdict={log['verdict']} -> {out}")


if __name__ == "__main__":
    main()
