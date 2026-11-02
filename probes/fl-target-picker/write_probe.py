#!/usr/bin/env python3
"""Write probe: does the REC-event path (and plugins.setParamValue) set
third-party parameters in mixer slots while the song plays, does the sound
change, and does FL record the writes as automation?

  python3 probes/fl-target-picker/write_probe.py --pattern 80 [--record]

Sound source: the first generator channel gets an eighth-note step pattern
in the given (empty) pattern; transport plays in pattern mode. Judge: the
peak meter of the plugin's mixer track, sampled in the script's OnIdle and
averaged over 2 s per setting, A/B/A/B; a change counts as audible when the
A-B difference exceeds 1 dB and three times the A-A difference. The peak
meter shows level, not timbre; tonal changes at equal level stay invisible
to it. Every parameter is restored after its test; with --record, pattern
<n> keeps the test notes and the recorded automation (delete it in FL).
"""

import argparse
import math
import statistics
import time

import fl
from flb import call

# (plugin substring, parameter name, value A, value B)
TESTS = [
    ("ShaperBox", "DriveShaper On", 0.0, 1.0),
    ("ShaperBox", "Drive Low Mix", 0.0, 1.0),
    ("ShaperBox", "Drive Mid Mix", 0.0, 1.0),
    ("ShaperBox", "Master Mix", 0.0, 1.0),
    ("Portal", "MACRO 1", 0.0, 1.0),
    ("Portal", "MACRO 2", 0.0, 1.0),
    ("Portal", "MASTER DRY/WET", 0.0, 1.0),
    ("Thermal", "DRIVE", 0.0, 1.0),
    ("Thermal", "MIX", 0.0, 1.0),
]
out = {}
RECORD = [("ShaperBox", "Drive Low Mix"), ("Portal", "MACRO 1"), ("Thermal", "DRIVE")]


def db(x):
    return 20 * math.log10(max(x, 1e-6))


def read(cmd, **args):
    """Idempotent read; retried once on a bridge timeout."""
    r = call(cmd, **args)
    if r.get("error") == "timeout":
        out["timeouts"] = out.get("timeouts", 0) + 1
        r = call(cmd, **args)
    return fl.must(r)


def meter(track, secs, hz=50):
    # OnIdle runs at about 4 Hz while FL is in the background, so the
    # client polls the peak meter itself.
    vals = []
    t_end = time.time() + secs
    while time.time() < t_end:
        v = read("eval", code=f"mixer.getTrackPeaks({track}, midi.PEAK_LR)")["r"]
        if isinstance(v, float):
            vals.append(v)
        time.sleep(1 / hz)
    return {"n": len(vals), "mean": statistics.mean(vals) if vals else 0.0,
            "max": max(vals, default=0.0)}


def params_of(p):
    items, _ = fl.paged("plist", "p", index=p["index"], slot=p["slot"], budget=56000)
    return {name: i for i, name in items}


def write(p, idx, v, how):
    cmd = "rec_fx" if how == "rec" else "set_fx"
    return fl.must(call(cmd, track=p["index"], slot=p["slot"], idx=idx, value=v))


def getv(p, idx):
    return read("getv", index=p["index"], slot=p["slot"], idx=idx)


def ab_test(p, idx, a, b, how, settle=0.8, secs=2.0):
    seq = []
    for v in (a, b, a, b):
        write(p, idx, v, how)
        time.sleep(settle)
        m = meter(p["index"], secs)
        g = getv(p, idx)
        seq.append({"set": v, "v": g["v"], "s": g["s"], "mean": m["mean"],
                    "max": m["max"], "n": m["n"]})
    da = abs(db(seq[0]["mean"]) - db(seq[2]["mean"]))
    dab = statistics.mean([db(seq[1]["mean"]), db(seq[3]["mean"])]) - \
        statistics.mean([db(seq[0]["mean"]), db(seq[2]["mean"])])
    readback = all(abs(s["v"] - s["set"]) < 0.02 for s in seq if isinstance(s["v"], float))
    return {"seq": seq, "ab_db": dab, "aa_db": da, "readback": readback,
            "audible": abs(dab) > max(1.0, 3 * da)}


def setup_sound(gen, pattern):
    st = {"pattern0": fl.ev("patterns.patternNumber()"),
          "loop0": fl.must(call("transport", op="status"))["loop"]}
    fl.must(call("pattern", n=pattern))
    for step in range(0, 16, 2):
        fl.ev(f"channels.setGridBit({gen['index']}, {step}, 1)")
    if st["loop0"] != 0:  # 0 = pattern mode
        fl.ev("transport.setLoopMode()")
    fl.must(call("setpos", pos=0))
    fl.must(call("transport", op="start"))
    return st


def record_test(p, idx, secs=4.0, hz=20):
    v0 = getv(p, idx)["v"]
    fl.must(call("transport", op="stop"))
    fl.must(call("setpos", pos=0))
    if not fl.must(call("transport", op="status"))["recording"]:
        fl.must(call("transport", op="record"))
    rec_on = fl.must(call("transport", op="status"))["recording"]
    fl.must(call("transport", op="start"))
    n = int(secs * hz)
    for k in range(n + 1):
        write(p, idx, k / n, "rec")
        time.sleep(1 / hz)
    fl.must(call("transport", op="stop"))
    if fl.must(call("transport", op="status"))["recording"]:
        fl.must(call("transport", op="record"))
    write(p, idx, 0.5, "rec")  # park mid-range: playback must move it
    fl.must(call("setpos", pos=0))
    fl.must(call("transport", op="start"))
    vals = []
    for _ in range(n):
        vals.append(getv(p, idx)["v"])
        time.sleep(1 / hz)
    fl.must(call("transport", op="stop"))
    write(p, idx, v0, "rec")
    vals = [v for v in vals if isinstance(v, float)]
    rng = max(vals) - min(vals) if vals else 0.0
    return {"record_on": rec_on, "playback_range": rng,
            "distinct": len({round(v, 3) for v in vals}),
            "recorded": rng > 0.3}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--pattern", type=int, required=True, help="empty pattern to use")
    ap.add_argument("--record", action="store_true")
    ap.add_argument("--methods", default="rec,set")
    args = ap.parse_args()
    plugs = fl.plugins_in_project()
    gen = next((p for p in plugs if p["kind"] == "gen"), None)
    if gen is None:
        raise SystemExit("no generator channel for the test notes")
    out.update(plugins=plugs, tests=[], records=[])
    found = {}
    for sub, *_ in TESTS:
        if sub not in found:
            p = next((q for q in plugs if q["kind"] == "fx" and sub.lower() in q["plugin"].lower()), None)
            found[sub] = (p, params_of(p) if p else {})
    out["sound"] = setup_sound(gen, args.pattern)
    time.sleep(1.0)
    for sub, (p, _) in found.items():
        if p:
            base = meter(p["index"], 2.0)
            print(f"{sub}: track {p['index']} slot {p['slot']}, baseline peak mean "
                  f"{db(base['mean']):.1f} dB over {base['n']} samples")
            out.setdefault("baseline", {})[sub] = base
    try:
        for sub, name, a, b in TESTS:
            p, names = found[sub]
            row = {"plugin": sub, "param": name}
            if p is None:
                row["result"] = "plugin not in project"
            elif name not in names:
                row["result"] = "parameter not listed by FL"
                row["similar"] = [n for n in names if name.split()[0].lower() in n.lower()][:8]
            else:
                idx = names[name]
                v0 = getv(p, idx)
                row.update(idx=idx, v0=v0)
                for how in args.methods.split(","):
                    row[how] = ab_test(p, idx, a, b, how)
                write(p, idx, v0["v"], "rec")
                row["restored"] = getv(p, idx)
            out["tests"].append(row)
            msg = row.get("result") or "; ".join(
                f"{how}: AB {row[how]['ab_db']:+.1f} dB (AA {row[how]['aa_db']:.1f}), "
                f"readback {row[how]['readback']}, "
                f"strings {[s['s'] for s in row[how]['seq'][:2]]}"
                for how in args.methods.split(","))
            print(f"  {sub} / {name}: {msg}")
        if args.record:
            for sub, name in RECORD:
                p, names = found[sub]
                if p and name in names:
                    r = record_test(p, names[name])
                    r.update(plugin=sub, param=name)
                    out["records"].append(r)
                    print(f"  record {sub} / {name}: {r}")
    finally:
        fl.must(call("transport", op="stop"))
    print("->", fl.save("write", out).name)


if __name__ == "__main__":
    main()
