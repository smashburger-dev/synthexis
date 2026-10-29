# name=synthexis harvest
# FL Studio MIDI controller script: dump every plugin + parameter of
# the open project to ~/Documents/synthexis-harvest/<ts>.json.
# Runs on OnInit and again on any note-on. One plugin per OnIdle call
# so FL never blocks for long.

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
        for ch in range(channels.channelCount()):
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


def _chan_params(ch):
    """Channel rack plugins: index = channel, slotIndex = -1 (FL API stubs)."""
    try:
        n = plugins.getParamCount(ch, -1)
        if n and n > 0:
            return ch, -1, n
    except Exception:
        pass
    return None, None, 0


def _dump_one(item):
    kind = item[0]
    rec = {"params": []}
    if kind == "chan":
        _, ch = item
        name = _try(plugins.getPluginName, ch, -1, True)
        name2 = _try(plugins.getPluginName, ch, -1)
        tr, sl, n = _chan_params(ch)
        if tr is None:
            rec.update({"name": name or name2, "n_params": 0})
            return rec
    else:
        _, tr, sl = item
        name = _try(plugins.getPluginName, tr, sl, True)
        name2 = _try(plugins.getPluginName, tr, sl)
        n = _try(plugins.getParamCount, tr, sl) or 0
    rec.update({"location": f"{kind}:{item[1:]}",
                "name": name or name2 or "?", "name_vst": name2,
                "n_params": n})
    for i in range(min(n or 0, MAX_PARAM)):
        pname = _try(plugins.getParamName, i, tr, sl)
        if not pname:
            continue
        cur = _try(plugins.getParamValue, i, tr, sl)
        strs = []
        # setParamValue(value, paramIndex, index, slotIndex): value first
        for t in T_STEPS:
            _try(plugins.setParamValue, t, i, tr, sl)
            s = _try(plugins.getParamValueString, i, tr, sl)
            strs.append([t, s if s is not None else ""])
        if cur is not None:
            _try(plugins.setParamValue, cur, i, tr, sl)
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
        try:
            _result.append(_dump_one(item))
        except Exception:
            pass
    else:
        _busy = False
        _flush()
