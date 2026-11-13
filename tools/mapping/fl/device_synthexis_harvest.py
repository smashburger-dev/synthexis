# name=synthexis harvest
# FL Studio MIDI controller script: dump every plugin + parameter of
# the open project to ~/Documents/synthexis-harvest/<ts>.json.
# Runs on OnInit and again on any note-on. One plugin per OnIdle call
# so FL never blocks for long.
#
# FL 2026 (API 45) gives scripts no file writes: os.makedirs and open(...,
# "w") raise SystemError, so _flush fails there. The dump that ran on
# 2026-11-13 exec'd this file in probes/fl-stock-plugins/device_synthexis_dev.py
# and fetched _dump_one page by page over MIDI (probes/fl-stock-plugins/run_stock.py).

import json
import os
import time

import channels
import mixer
import plugins

OUT_DIR = os.path.expanduser("~/Documents/synthexis-harvest")
T_STEPS = [0.0, 0.2, 0.5, 0.8, 1.0]
MAX_PARAM = 4096

# work item: ("chan", ch) | ("mix", track, slot)
_queue = []
_result = []
_busy = False


def _enqueue():
    global _queue, _result
    _queue, _result = [], []
    try:
        for ch in range(channels.channelCount(True)):
            _queue.append(("chan", ch))
    except Exception:
        pass
    try:
        for tr in range(mixer.trackCount()):
            for slot in range(10):
                try:
                    if plugins.isValid(tr, slot):
                        _queue.append(("mix", tr, slot))
                except Exception:
                    pass
    except Exception:
        pass


def _try(fn, *args):
    try:
        return fn(*args)
    except Exception:
        return None


def _dump_one(item, start=0, stop=MAX_PARAM):
    """One plugin, parameters start..stop (a page, so a big plugin fits a
    SysEx reply). Channels go by FL's global index (g=True), so the channel
    rack's group filter cannot hide or renumber them."""
    kind = item[0]
    rec = {"params": []}
    if kind == "chan":
        _, idx = item
        sl, g = -1, True
        if not _try(plugins.isValid, idx, -1, True):
            return None  # Sampler, audio clip, automation clip: no plugin
    else:
        _, idx, sl = item
        g = False
    name = _try(plugins.getPluginName, idx, sl, False, g)
    user = _try(plugins.getPluginName, idx, sl, True, g)
    n = _try(plugins.getParamCount, idx, sl, g) or 0
    rec.update({"location": f"{kind}:{item[1:]}",
                "name": name or user or "?", "user_name": user,
                "n_params": n})
    for i in range(start, min(n, stop, MAX_PARAM)):
        pname = _try(plugins.getParamName, i, idx, sl, g)
        if not pname:
            continue
        cur = _try(plugins.getParamValue, i, idx, sl, g)
        strs = []
        # setParamValue(value, paramIndex, index, slotIndex, pickupMode, useGlobalIndex)
        for t in T_STEPS:
            _try(plugins.setParamValue, t, i, idx, sl, 0, g)
            s = _try(plugins.getParamValueString, i, idx, sl, g)
            strs.append([t, s if s is not None else ""])
        if cur is not None:
            _try(plugins.setParamValue, cur, i, idx, sl, 0, g)
        rec["params"].append(
            {"index": i, "name": pname, "value": cur, "samples": strs})
    return rec


def _flush():
    os.makedirs(OUT_DIR, exist_ok=True)
    ts = time.strftime("%Y%m%d-%H%M%S")
    path = os.path.join(OUT_DIR, f"{ts}.json")
    try:
        with open(path, "w") as f:
            json.dump({"ts": ts, "plugins": _result}, f)
        print("synthexis harvest ->", path)
    except Exception as e:
        print("synthexis harvest write failed:", e)


def OnInit():
    global _busy
    _busy = True
    _enqueue()


def OnMidiMsg(event):
    global _busy
    try:
        if (event.status & 0xF0) == 0x90 and event.data2 > 0:
            _busy = True
            _enqueue()
    except Exception:
        pass


def OnIdle():
    global _busy
    if not _busy:
        return
    if _queue:
        item = _queue.pop(0)
        r = _try(_dump_one, item)
        if r is not None:
            _result.append(r)
    else:
        _busy = False
        _flush()
