#!/usr/bin/env python3
"""Focus probe: which FL script calls report the plugin the user works on,
whether FL calls back on focus changes (OnRefresh flags) or the script must
poll, and what polling costs. Also: which thread runs OnSysEx / OnIdle, and
what queuing commands into OnIdle costs in latency.

  python3 probes/fl-target-picker/focus_probe.py            # scripted steps
  python3 probes/fl-target-picker/focus_probe.py --manual 40  # + 40 s of
      live logging while a person clicks plugin windows in FL

Scripted steps change focus and selection through the API (open editors,
select channel and track, focus windows) and restore the selection at the
end. Plugin windows opened here stay open. Writes runs/<ts>-focus.json.
"""

import argparse
import statistics
import time

import fl
from flb import call

FOCUS = ("formID", "plugin", "caption", "activeFx", "selCh", "track", "wid")


def snap():
    return fl.must(call("focus"))


def events_since(seq):
    rows, nxt = [], seq
    while True:
        r = fl.must(call("events", since=nxt, budget=16000))
        rows += r["ev"]
        if r["next"] is None:
            return rows, r
        nxt = rows[-1][0]


def find(plugs, name):
    for p in plugs:
        if name.lower() in p["plugin"].lower():
            return p
    return None


def step(label, code, log, wait=0.5):
    seq = fl.must(call("events", since=10**9))["seq"]
    err = None
    if code:
        try:
            fl.ev(code)
        except RuntimeError as e:
            err = str(e)
    time.sleep(wait)
    f = snap()
    ev, _ = events_since(seq)
    flags = [e[3] for e in ev if e[2] == "refresh"]
    row = {"step": label, "code": code, "error": err,
           "focus": {k: f.get(k) for k in FOCUS}, "focus_ms": f.get("ms"),
           "refresh_flags": flags,
           "focus_events": [e[3]["by"] for e in ev if e[2] == "focus"],
           "other": [e[2:] for e in ev if e[2] not in ("refresh", "focus")][:20]}
    log.append(row)
    fx = row["focus"]
    print(f"{label:34s} formID={fx['formID']!s:>9} plugin={fx['plugin']!r:24} "
          f"activeFx={fx['activeFx']} selCh={fx['selCh']} track={fx['track']} "
          f"wid={fx['wid']} refresh={sorted(set(flags))} {err or ''}")
    return row


def scripted(out):
    plugs = fl.plugins_in_project()
    sb, portal = find(plugs, "ShaperBox"), find(plugs, "Portal")
    gen = next((p for p in plugs if p["kind"] == "gen"), None)
    f0 = snap()
    log = out["steps"]
    step("baseline", None, log)
    if sb:
        step(f"mixer.focusEditor({sb['index']},{sb['slot']}) ShaperBox",
             f"mixer.focusEditor({sb['index']}, {sb['slot']})", log)
    if portal:
        step(f"mixer.focusEditor({portal['index']},{portal['slot']}) Portal",
             f"mixer.focusEditor({portal['index']}, {portal['slot']})", log)
    if gen:
        step(f"channels.showEditor({gen['index']},1) {gen['plugin']}",
             f"channels.showEditor({gen['index']}, 1)", log)
    step("channels.selectOneChannel(0)", "channels.selectOneChannel(0)", log)
    step("mixer.setTrackNumber(2)", "mixer.setTrackNumber(2)", log)
    step("focus mixer window",
         "ui.showWindow(midi.widMixer) or ui.setFocused(midi.widMixer)", log)
    step("focus channel rack",
         "ui.showWindow(midi.widChannelRack) or ui.setFocused(midi.widChannelRack)", log)
    if gen:
        step(f"channels.showEditor({gen['index']},0)", f"channels.showEditor({gen['index']}, 0)", log)
    # restore selection
    if isinstance(f0.get("selCh"), int) and f0["selCh"] >= 0:
        step("restore selected channel", f"channels.selectOneChannel({f0['selCh']})", log)
    if isinstance(f0.get("track"), int):
        step("restore mixer track", f"mixer.setTrackNumber({f0['track']})", log)

    # does getParamName raise HW_Dirty_FocusedWindow (claim in NI's script)?
    if portal:
        seq = fl.must(call("events", since=10**9))["seq"]
        fl.must(call("plist", index=portal["index"], slot=portal["slot"], count=300, budget=56000))
        time.sleep(1.0)
        ev, _ = events_since(seq)
        out["getParamName_side_effect"] = {
            "refresh_flags": [e[3] for e in ev if e[2] == "refresh"],
            "focus_events": len([e for e in ev if e[2] == "focus"])}
        print("getParamName x300 ->", out["getParamName_side_effect"])

    # idle rate and polling cost
    r0 = fl.must(call("events", since=10**9))
    fl.must(call("poll", on=True))
    time.sleep(10)
    r1 = fl.must(call("events", since=10**9))
    fl.must(call("poll", on=False))
    i0, i1, pl = r0["idle"], r1["idle"], r1["poll"]
    dt = i1["last"] - i0["last"]
    out["idle"] = {"calls": i1["n"] - i0["n"], "secs": dt,
                   "hz": (i1["n"] - i0["n"]) / dt if dt else None,
                   "gap_max_s": i1["gap_max"]}
    out["poll"] = {"n": pl["n"], "mean_ms": pl["sum"] / pl["n"] if pl["n"] else None,
                   "max_ms": pl["max"], "changes": pl["changes"]}
    print("OnIdle", out["idle"], "poll", out["poll"])

    # which thread runs what; latency of the OnIdle queue
    out["threads"] = {"sysex": fl.must(call("threads")),
                      "queued": fl.must(call("threads", main=True))}
    print("threads", out["threads"])
    lat = {}
    for mode in ("direct", "queued"):
        ms = []
        for _ in range(50):
            t0 = time.perf_counter()
            fl.must(call("ping", main=(mode == "queued")))
            ms.append((time.perf_counter() - t0) * 1000)
        lat[mode] = {"median": statistics.median(ms), "p95": sorted(ms)[47], "max": max(ms)}
    out["ping_ms"] = lat
    print("ping ms", lat)


def manual(out, secs):
    print(f"\nlogging focus for {secs} s: click plugin windows, mixer slots, "
          "channels in FL now")
    seq = fl.must(call("events", since=10**9))["seq"]
    fl.must(call("poll", on=True))
    t_end = time.time() + secs
    seen = []
    while time.time() < t_end:
        ev, _ = events_since(seq)
        for e in ev:
            seq = e[0]
            if e[2] == "focus":
                f = e[3]["f"]
                seen.append({"t": e[1], "by": e[3]["by"], "focus": f})
                print(f"  {e[3]['by']:7s} plugin={f['plugin']!r:24} formID={f['formID']} "
                      f"activeFx={f['activeFx']} selCh={f['selCh']} track={f['track']} wid={f['wid']}")
        time.sleep(0.2)
    fl.must(call("poll", on=False))
    out["manual"] = seen


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--manual", type=int, default=0)
    ap.add_argument("--skip-scripted", action="store_true")
    args = ap.parse_args()
    out = {"steps": []}
    if not args.skip_scripted:
        scripted(out)
    if args.manual:
        manual(out, args.manual)
    print("->", fl.save("focus", out).name)


if __name__ == "__main__":
    main()
