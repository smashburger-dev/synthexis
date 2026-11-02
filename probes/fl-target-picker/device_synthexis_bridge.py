# name=synthexis bridge
#
# synthexis FL bridge, protocol 2 (target-picker probe). Superset of
# probes/fl-camera-plugin/bridge/device_synthexis_bridge.py (protocol 1):
# every protocol-1 command answers the same way, so h2_run.py and
# gate_run.py keep working. Test-only tooling, never shipped.
#
# Commands arrive as: F0 7D <7-bit-safe ASCII JSON> F7. Every reply goes out
# on the assigned output via device.midiOutSysex in the same framing and
# carries the request "id". One command = quick API calls; nothing blocks.
#
# New in protocol 2:
#   mixer     mixer tracks with names and effect slots (paged by "budget")
#   gens      channel rack generators with plugin, type, target track
#   plist     parameter names of one plugin, generator (slot -1) or effect
#   focus     one snapshot of every focus/selection call
#   events    OnRefresh / OnDirty* / focus-change log since a sequence number
#   poll      focus polling in OnIdle on/off, with timing stats
#   rec_fx    REC-event write into a mixer-slot plugin parameter
#   set_fx    plugins.setParamValue into a mixer-slot plugin parameter
#   getv      value + value string of any plugin parameter
#   meter     peak meter of a mixer track sampled in OnIdle
#   blob      reply of n bytes, to find the SysEx size limit
#   threads   thread idents of OnSysEx / OnIdle / OnRefresh and this call
#   eval      run Python in FL (exploration only; same-user unix socket)
# Any command with "main": true is queued in OnSysEx (CoreMIDI thread) and
# run in the next OnIdle instead, to measure the main-thread variant.

import _thread
import json
import os
import time

import channels
import device
import general
import midi
import mixer
import patterns
import plugins
import transport
import ui

MAX_PARAMS = 128
SLOTS = 10
DEFAULT_BUDGET = 3500  # reply bytes per page; fits protocol-1 daemons too
# Largest SysEx frame sent. FL 26.1.7 delivered frames up to 65,534 bytes;
# a 65,536-byte frame opened a modal dialog on FL's main thread while the
# CoreMIDI thread held the Python GIL, and FL deadlocked (RESULT.md, 1b).
MAX_FRAME = 60000


def _reply(request_id, payload):
    msg = {"id": request_id}
    msg.update(payload)
    data = json.dumps(msg, separators=(",", ":"), default=str).encode("ascii")
    if len(data) + 3 > MAX_FRAME:
        data = json.dumps({"id": request_id, "error": "reply too large",
                           "bytes": len(data) + 3}).encode("ascii")
    device.midiOutSysex(bytes([0xF0, 0x7D]) + data + bytes([0xF7]))


def _sysex_payload(msg):
    """Extract our JSON payload from a FlMidiMsg, or None."""
    try:
        data = bytes(msg.sysex)
    except Exception:
        return None
    try:
        start = data.index(0xF0)
        if start + 1 >= len(data) or data[start + 1] != 0x7D:
            return None
        end = data.index(0xF7, start + 2)
        return data[start + 2:end]
    except ValueError:
        return None


def _call(fn, *args):
    try:
        v = fn(*args)
    except Exception as e:
        return "err:" + str(e)
    return list(v) if isinstance(v, tuple) else v


def _size(obj):
    return len(json.dumps(obj, separators=(",", ":"), default=str))


def _page(items, start, budget, key):
    """Take items from start while the JSON stays under budget bytes."""
    out, used = [], 40
    i = start
    while i < len(items):
        n = _size(items[i]) + 1
        if out and used + n > budget:
            break
        out.append(items[i])
        used += n
        i += 1
    return {key: out, "next": i if i < len(items) else None, "total": len(items)}


# --- focus and events -----------------------------------------------------

_FOCUS_CALLS = (
    ("formID", ui.getFocusedFormID),
    ("plugin", ui.getFocusedPluginName),
    ("caption", ui.getFocusedFormCaption),
    ("activeFx", mixer.getActiveEffectIndex),
    ("selCh", channels.selectedChannel),
    ("track", mixer.trackNumber),
)


def _focus():
    f = {k: _call(fn) for k, fn in _FOCUS_CALLS}
    f["wid"] = [w for w in range(9) if _call(ui.getFocused, w) in (True, 1)]
    return f


_events = []      # (seq, t, kind, data)
_seq = [0]
_last_focus = [None]
_poll = {"on": False, "n": 0, "sum": 0.0, "max": 0.0, "changes": 0}
_idle = {"n": 0, "t0": 0.0, "last": 0.0, "gap_max": 0.0}
_meter = {"track": None, "n": 0, "max": 0.0, "sum": 0.0}


def _event(kind, data):
    _seq[0] += 1
    _events.append((_seq[0], time.time(), kind, data))
    if len(_events) > 2000:
        del _events[:500]


def _check_focus(source):
    f = _focus()
    if f != _last_focus[0]:
        _last_focus[0] = f
        _event("focus", {"by": source, "f": f})
        return True
    return False


# --- inventory --------------------------------------------------------------

def _slot_info(track, slot):
    if not plugins.isValid(track, slot):
        return None
    return {
        "s": slot,
        "plugin": _call(plugins.getPluginName, track, slot),
        "user": _call(plugins.getPluginName, track, slot, True),
        "n": _call(plugins.getParamCount, track, slot),
    }


def _mixer_items(with_empty):
    out = []
    for t in range(mixer.trackCount()):
        slots = [x for x in (_slot_info(t, s) for s in range(SLOTS)) if x]
        name = _call(mixer.getTrackName, t)
        if slots or with_empty:
            out.append({"t": t, "name": name, "slots": slots})
    return out


def _gen_items():
    out = []
    for i in range(channels.channelCount()):
        valid = plugins.isValid(i)
        out.append({
            "i": i,
            "name": _call(channels.getChannelName, i),
            "type": _call(channels.getChannelType, i),
            "fx": _call(channels.getTargetFxTrack, i),
            "plugin": _call(plugins.getPluginName, i) if valid else "",
            "user": _call(plugins.getPluginName, i, -1, True) if valid else "",
            "n": _call(plugins.getParamCount, i) if valid else 0,
        })
    return out


_NS = {}


def _eval(code):
    ns = _NS
    if not ns:
        ns.update(channels=channels, device=device, general=general, midi=midi,
                  mixer=mixer, patterns=patterns, plugins=plugins,
                  transport=transport, ui=ui, time=time, json=json)
    try:
        return eval(compile(code, "<bridge>", "eval"), ns)
    except SyntaxError:
        exec(compile(code, "<bridge>", "exec"), ns)
        return ns.get("_")


def _handle(cmd):
    op = cmd.get("cmd")
    if op == "ping":
        return {"ok": True, "proto": 2}
    if op == "channels":
        out = []
        for i in range(channels.channelCount()):
            name = ""
            try:
                name = channels.getChannelName(i)
            except Exception:
                pass
            plugin = ""
            try:
                if plugins.isValid(i):
                    plugin = plugins.getPluginName(i)
            except Exception:
                pass
            out.append({"i": i, "name": name, "plugin": plugin})
        return {"channels": out}
    if op == "params":
        ch = int(cmd["ch"])
        cap = min(int(cmd.get("max", MAX_PARAMS)), MAX_PARAMS)
        total = plugins.getParamCount(ch)
        out = []
        for p in range(min(total, cap)):
            name = plugins.getParamName(p, ch)
            if name:
                out.append({"i": p, "name": name})
        return {"total": total, "params": out}
    if op == "get":
        return {"v": plugins.getParamValue(int(cmd["idx"]), int(cmd["ch"]))}
    if op == "set":
        plugins.setParamValue(
            float(cmd["value"]), int(cmd["idx"]), int(cmd["ch"])
        )
        return {"ok": True}
    if op == "rec":
        # Set a plugin parameter through FL's REC event path (like a linked
        # MIDI controller). Value 0..1 maps to 0..2^30 with REC_MIDIController.
        ch = int(cmd["ch"])
        eid = channels.getRecEventId(ch) + midi.REC_Chan_Plugin_First + int(cmd["idx"])
        val = int(max(0.0, min(1.0, float(cmd["value"]))) * (1 << 30))
        res = general.processRECEvent(eid, val, midi.REC_MIDIController)
        return {"ok": True, "res": res, "eid": eid}
    if op == "show":
        channels.showEditor(int(cmd["ch"]), 1 if cmd.get("on") else 0)
        return {"ok": True}
    if op == "transport":
        sub = cmd.get("op")
        if sub == "start":
            transport.start()
        elif sub == "stop":
            transport.stop()
        elif sub == "record":
            transport.record()
        elif sub == "status":
            return {
                "playing": transport.isPlaying(),
                "recording": transport.isRecording(),
                "pos": transport.getSongPos(),
                "loop": transport.getLoopMode(),
            }
        else:
            return {"error": "unknown transport op " + str(sub)}
        return {"ok": True}
    if op == "setpos":
        transport.setSongPos(float(cmd["pos"]), int(cmd.get("mode", -1)))
        return {"ok": True}
    if op == "pattern":
        patterns.jumpToPattern(int(cmd["n"]))
        return {"pattern": patterns.patternNumber()}
    if op == "link_last":
        res = device.linkToLastTweaked(
            int(cmd.get("control", 0)), int(cmd.get("channel", 0))
        )
        return {"res": res}

    # --- protocol 2 ---
    budget = int(cmd.get("budget", DEFAULT_BUDGET))
    if op == "mixer":
        t0 = time.perf_counter()
        items = _mixer_items(bool(cmd.get("empty")))
        res = _page(items, int(cmd.get("start", 0)), budget, "tracks")
        res["count"] = mixer.trackCount()
        res["ms"] = (time.perf_counter() - t0) * 1000
        return res
    if op == "gens":
        t0 = time.perf_counter()
        res = _page(_gen_items(), int(cmd.get("start", 0)), budget, "gens")
        res["ms"] = (time.perf_counter() - t0) * 1000
        return res
    if op == "plist":
        # Scan from "start" until the page is full; "next" is the parameter
        # index to resume at. Unnamed parameters are skipped.
        index, slot = int(cmd["index"]), int(cmd.get("slot", -1))
        start = int(cmd.get("start", 0))
        total = plugins.getParamCount(index, slot)
        stop = min(total, start + int(cmd.get("count", total)))
        t0 = time.perf_counter()
        out, used, p = [], 60, start
        while p < stop:
            name = plugins.getParamName(p, index, slot)
            if name:
                n = _size([p, name]) + 1
                if out and used + n > budget:
                    break
                out.append([p, name])
                used += n
            p += 1
        ms = (time.perf_counter() - t0) * 1000
        return {"p": out, "next": p if p < stop else None, "total": total,
                "scanned": p - start, "ms": ms}
    if op == "focus":
        t0 = time.perf_counter()
        f = _focus()
        f["ms"] = (time.perf_counter() - t0) * 1000
        return f
    if op == "events":
        since = int(cmd.get("since", 0))
        rows = [list(e) for e in _events if e[0] > since]
        res = _page(rows, 0, budget, "ev")
        res["seq"] = _seq[0]
        res["idle"] = dict(_idle)
        res["poll"] = dict(_poll)
        return res
    if op == "poll":
        _poll.update(on=bool(cmd.get("on")), n=0, sum=0.0, max=0.0, changes=0)
        return {"ok": True}
    if op in ("rec_fx", "set_fx"):
        t, s, idx = int(cmd["track"]), int(cmd["slot"]), int(cmd["idx"])
        v = max(0.0, min(1.0, float(cmd["value"])))
        if op == "set_fx":
            plugins.setParamValue(v, idx, t, s)
            return {"ok": True}
        eid = mixer.getTrackPluginId(t, s) + midi.REC_Plug_Plugin_First + idx
        res = general.processRECEvent(eid, int(v * (1 << 30)), midi.REC_MIDIController)
        return {"ok": True, "res": res, "eid": eid}
    if op == "getv":
        index, slot, idx = int(cmd["index"]), int(cmd.get("slot", -1)), int(cmd["idx"])
        return {"v": _call(plugins.getParamValue, idx, index, slot),
                "s": _call(plugins.getParamValueString, idx, index, slot)}
    if op == "meter":
        res = {k: _meter[k] for k in ("track", "n", "max")}
        res["mean"] = _meter["sum"] / _meter["n"] if _meter["n"] else 0.0
        _meter.update(track=cmd.get("track", _meter["track"]), n=0, max=0.0, sum=0.0)
        return res
    if op == "blob":
        return {"b": "x" * int(cmd["n"])}
    if op == "threads":
        return {"now": _thread.get_ident(), "seen": dict(_threads),
                "queue_ms": _idle.get("queue_ms")}
    if op == "eval":
        t0 = time.perf_counter()
        r = _eval(cmd["code"])
        return {"r": r, "ms": (time.perf_counter() - t0) * 1000}
    return {"error": "unknown cmd " + str(op)}


_LOG_PATH = os.path.expanduser("~/Library/Logs/synthexis-flbridge-fl.log")
_last_beat = [0.0]


def _log(text):
    try:
        with open(_LOG_PATH, "a") as f:
            f.write("[%.3f] %s\n" % (time.time(), text))
    except Exception:
        pass


def OnInit():
    _log("OnInit port=%s proto=2" % _safe(device.getPortNumber))
    _last_focus[0] = _focus()


def _safe(fn):
    try:
        return fn()
    except Exception as e:
        return "err:" + str(e)


def OnMidiIn(msg):
    try:
        _log("OnMidiIn status=0x%02x sysex=%s" % (msg.status, msg.sysex is not None))
    except Exception as e:
        _log("OnMidiIn err " + str(e))


_queue = []        # commands sent with "main": true, run in OnIdle
_threads = {}      # callback name -> thread ident, to show which thread runs what


def _run(cmd):
    rid = cmd.get("id")
    try:
        _reply(rid, _handle(cmd))
    except Exception as e:
        _reply(rid, {"error": str(e)})


def OnSysEx(msg):
    _threads["OnSysEx"] = _thread.get_ident()
    try:
        payload = _sysex_payload(msg)
        if payload is None:
            return
        cmd = json.loads(payload.decode("ascii"))
        if cmd.get("main"):
            cmd["_queued"] = time.perf_counter()
            _queue.append(cmd)
            return
        _run(cmd)
    except Exception:
        # Never let a bad packet break the script.
        pass


def OnRefresh(flags):
    _threads["OnRefresh"] = _thread.get_ident()
    _event("refresh", flags)
    if flags & midi.HW_Dirty_FocusedWindow:
        _check_focus("refresh")


def OnDirtyMixerTrack(index):
    _event("mixer", index)


def OnDirtyChannel(index, flag):
    _event("channel", [index, flag])


def OnProjectLoad(status):
    _event("project", status)


def OnIdle():
    _threads["OnIdle"] = _thread.get_ident()
    while _queue:
        cmd = _queue.pop(0)
        _idle["queue_ms"] = (time.perf_counter() - cmd.pop("_queued")) * 1000
        _run(cmd)
    now = time.time()
    if _idle["n"] == 0:
        _idle["t0"] = now
    elif now - _idle["last"] > _idle["gap_max"]:
        _idle["gap_max"] = now - _idle["last"]
    _idle["n"] += 1
    _idle["last"] = now
    if _poll["on"]:
        t0 = time.perf_counter()
        changed = _check_focus("poll")
        dt = (time.perf_counter() - t0) * 1000
        _poll["n"] += 1
        _poll["sum"] += dt
        _poll["max"] = max(_poll["max"], dt)
        _poll["changes"] += 1 if changed else 0
    if _meter["track"] is not None:
        v = _call(mixer.getTrackPeaks, int(_meter["track"]), midi.PEAK_LR)
        if isinstance(v, float):
            _meter["n"] += 1
            _meter["sum"] += v
            _meter["max"] = max(_meter["max"], v)
    if now - _last_beat[0] > 10:
        _last_beat[0] = now
        _log("alive")
