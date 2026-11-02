#!/usr/bin/env python3
"""Listening test: FL plays pattern <n>; each parameter is switched
A, B, A, B through the REC-event path, 3 s per state, while a person listens
and watches the plugin window. Prints a timeline; restores every value.

  python3 probes/fl-target-picker/listen.py --pattern 80
"""

import argparse
import time

import fl
from flb import call

ORDER = [  # (plugin, parameter, A, B)
    ("Portal", "MASTER DRY/WET", 0.0, 1.0),
    ("Portal", "MACRO 1", 0.0, 1.0),
    ("Portal", "MACRO 2", 0.0, 1.0),
    ("ShaperBox", "Master Mix", 0.0, 1.0),
    ("ShaperBox", "DriveShaper On", 0.0, 1.0),
    ("ShaperBox", "Drive Low Mix", 0.0, 1.0),
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--pattern", type=int, required=True)
    ap.add_argument("--secs", type=float, default=3.0)
    args = ap.parse_args()
    plugs = fl.plugins_in_project()
    names = {}
    fl.must(call("pattern", n=args.pattern))
    fl.must(call("setpos", pos=0))
    fl.must(call("transport", op="start"))
    t0 = time.time()
    log = []
    try:
        for sub, name, a, b in ORDER:
            p = next(q for q in plugs if q["kind"] == "fx" and sub.lower() in q["plugin"].lower())
            key = (p["index"], p["slot"])
            if key not in names:
                items, _ = fl.paged("plist", "p", index=p["index"], slot=p["slot"], budget=56000)
                names[key] = {n: i for i, n in items}
            idx = names[key][name]
            v0 = fl.must(call("getv", index=p["index"], slot=p["slot"], idx=idx))["v"]
            for v in (a, b, a, b):
                fl.must(call("rec_fx", track=p["index"], slot=p["slot"], idx=idx, value=v))
                print(f"t={time.time() - t0:5.1f}s  {sub} / {name} = {v}", flush=True)
                log.append([time.time() - t0, sub, name, v])
                time.sleep(args.secs)
            fl.must(call("rec_fx", track=p["index"], slot=p["slot"], idx=idx, value=v0))
            time.sleep(0.3)
            back = fl.must(call("getv", index=p["index"], slot=p["slot"], idx=idx))["v"]
            print(f"           restored {name} to {v0} (reads {back})")
    finally:
        fl.must(call("transport", op="stop"))
    print("->", fl.save("listen", {"log": log}).name)


if __name__ == "__main__":
    main()
