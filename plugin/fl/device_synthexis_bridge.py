# name=synthexis bridge
# supportedHardwareIds=7D 53 58 46 4C
# supportedDevices=synthexis
#
# synthexis FL bridge, protocol 4: the device script the synthexis plugin
# talks to. The plugin owns a virtual MIDI source and destination, both
# named "synthexis"; see plugin/synthexis/src/fl.rs. Measurements behind
# every rule here: probes/fl-target-picker/RESULT.md.
#
# Setup: none in MIDI settings. FL asks each new MIDI device who it is; the
# plugin answers 7D 53 58 46 4C ..., which matches supportedHardwareIds, so
# FL enables the input, links the output and loads this script on its own
# (supportedDevices is the second way, by name). FL reads this folder only
# at start: the script has to be here before FL first sees the "synthexis"
# ports, or FL stores them without a script (probes/fl-midi-autolink).
#
# Frames both ways: F0 7D <7-bit ASCII JSON> F7.
#
# Requests carry an "id"; the reply carries it back and is sent from
# OnSysEx (CoreMIDI thread), so nothing here may open FL UI:
#   ping   {"proto": 3, "h": inventory hash}
#   inv    mixer effect slots and channel rack generators, paged
#          ("start", "budget"); rows ["fx", track, slot, plugin, user,
#          track name] and ["gen", channel, plugin, user, channel name,
#          target mixer track, its name]
#   plist  parameter names of one plugin ("index", "slot", -1 for a
#          generator), paged; FL's MIDI CC entries (index >= 4096) left out
#   focus  the focused plugin, same shape as the focus event
#   rec    set parameter "i" of plugin "p" to "v" (0..1): "fx": [track, slot]
#          through FL's REC event (recorded as automation while FL records),
#          "ch": channel through plugins.setParamValue (recorded too; both
#          measured in probes/fl-target-picker and probes/fl-script-bridge).
#          Nothing is written when another plugin sits there now. Sent
#          without an id by the route writer, up to 30 times a second per
#          parameter, so it gets no reply.
#
# Events carry "ev" and no "id":
#   hello  sent by OnInit, so the plugin reloads after FL re-inits the device
#   focus  {"fx": [track, slot] or null, "ch": channel or null, "plugin"}
#          on every OnRefresh with HW_Dirty_FocusedWindow that changes it;
#          an empty "plugin" means a non-plugin window has focus
#   inv    {"h": hash} when the mixer slots or generators changed
#
# Size rule: a 65,536-byte frame deadlocked FL 26.1.7, so no frame goes out
# above MAX_FRAME and pages stay at PAGE_MAX or less.

import json
import time

import channels
import device
import general
import midi
import mixer
import plugins
import ui

PROTO = 4
SLOTS = 10
FL_CC_FIRST = 4096  # FL appends 144 MIDI CC pseudo-parameters from here
PAGE_MAX = 16000
INV_CHECK_S = 0.5  # OnRefresh and OnDirty* fire during playback too
MAX_FRAME = 60000


def _send(msg):
    data = json.dumps(msg, separators=(",", ":")).encode("ascii")
    if len(data) + 3 > MAX_FRAME:
        data = json.dumps({"id": msg.get("id"), "error": "reply too large",
                           "bytes": len(data) + 3}).encode("ascii")
    device.midiOutSysex(bytes([0xF0, 0x7D]) + data + bytes([0xF7]))


def _payload(msg):
    try:
        data = bytes(msg.sysex)
        start = data.index(0xF0)
        if data[start + 1] != 0x7D:
            return None
        return data[start + 2:data.index(0xF7, start + 2)]
    except Exception:
        return None


def _safe(fn, *args):
    try:
        return fn(*args)
    except Exception:
        return None


def _size(obj):
    return len(json.dumps(obj, separators=(",", ":"))) + 1


def _budget(cmd):
    return max(1000, min(PAGE_MAX, int(cmd.get("budget", PAGE_MAX))))


# --- inventory --------------------------------------------------------------

def _inventory():
    rows = []
    for t in range(mixer.trackCount()):
        tname = _safe(mixer.getTrackName, t) or ""
        for s in range(SLOTS):
            if plugins.isValid(t, s):
                rows.append(["fx", t, s, _safe(plugins.getPluginName, t, s) or "",
                             _safe(plugins.getPluginName, t, s, True) or "", tname])
    for i in range(channels.channelCount()):
        if plugins.isValid(i):
            fx = _safe(channels.getTargetFxTrack, i) or 0
            rows.append(["gen", i, _safe(plugins.getPluginName, i) or "",
                         _safe(plugins.getPluginName, i, -1, True) or "",
                         _safe(channels.getChannelName, i) or "", fx,
                         _safe(mixer.getTrackName, fx) or ""])
    return rows


_inv = {"h": None, "dirty": True, "checked": 0.0}


def _inv_hash():
    if _inv["dirty"]:
        _inv["dirty"] = False
        _inv["h"] = hash(json.dumps(_inventory())) & 0x7FFFFFFF
    return _inv["h"]


def _page(rows, start, budget):
    out, used, i = [], 80, start
    while i < len(rows):
        n = _size(rows[i])
        if out and used + n > budget:
            break
        out.append(rows[i])
        used += n
        i += 1
    return out, (i if i < len(rows) else None)


# --- focus ------------------------------------------------------------------

def _focus():
    name = _safe(ui.getFocusedPluginName) or ""
    fx = _safe(mixer.getActiveEffectIndex)
    form = _safe(ui.getFocusedFormID)
    ch = None
    if name and fx is None and isinstance(form, int) and 0 <= form < channels.channelCount():
        ch = form  # a generator's form ID is its channel index
    return {"fx": list(fx) if fx is not None else None, "ch": ch, "plugin": name}


_last_focus = [None]


# --- requests ---------------------------------------------------------------

def _handle(cmd):
    op = cmd.get("cmd")
    if op == "ping":
        return {"proto": PROTO, "h": _inv_hash()}
    if op == "inv":
        rows = _inventory()
        _inv["h"], _inv["dirty"] = hash(json.dumps(rows)) & 0x7FFFFFFF, False
        page, nxt = _page(rows, int(cmd.get("start", 0)), _budget(cmd))
        return {"rows": page, "next": nxt, "h": _inv["h"]}
    if op == "plist":
        index, slot = int(cmd["index"]), int(cmd.get("slot", -1))
        stop = min(_safe(plugins.getParamCount, index, slot) or 0, FL_CC_FIRST)
        budget = _budget(cmd)
        out, used, p = [], 80, int(cmd.get("start", 0))
        while p < stop:
            name = plugins.getParamName(p, index, slot)
            if name:
                n = _size([p, name])
                if out and used + n > budget:
                    break
                out.append([p, name])
                used += n
            p += 1
        return {"p": out, "next": p if p < stop else None, "plugin":
                _safe(plugins.getPluginName, index, slot) or ""}
    if op == "focus":
        return _focus()
    if op == "rec":
        i, v = int(cmd["i"]), max(0.0, min(1.0, float(cmd["v"])))
        if "fx" in cmd:
            t, s = int(cmd["fx"][0]), int(cmd["fx"][1])
            if _safe(plugins.getPluginName, t, s) != cmd.get("p"):
                return {"error": "another plugin in that slot"}
            eid = mixer.getTrackPluginId(t, s) + midi.REC_Plug_Plugin_First + i
            general.processRECEvent(eid, int(v * (1 << 30)), midi.REC_MIDIController)
        else:
            ch = int(cmd["ch"])
            if _safe(plugins.getPluginName, ch, -1) != cmd.get("p"):
                return {"error": "another plugin on that channel"}
            plugins.setParamValue(v, i, ch)
        return {"ok": True}
    return {"error": "unknown cmd " + str(op)}


def OnSysEx(msg):
    payload = _payload(msg)
    if payload is None:
        return
    try:
        cmd = json.loads(payload.decode("ascii"))
    except Exception:
        return
    rid = cmd.get("id")
    try:
        res = _handle(cmd)
    except Exception as e:
        res = {"error": str(e)}
    if rid is None and cmd.get("cmd") == "rec":
        return
    res["id"] = rid
    _send(res)


# --- events -----------------------------------------------------------------

def OnInit():
    _inv["dirty"] = True
    _last_focus[0] = _focus()
    _send({"ev": "hello", "proto": PROTO})


def OnRefresh(flags):
    if flags & midi.HW_Dirty_FocusedWindow:
        f = _focus()
        if f != _last_focus[0]:
            _last_focus[0] = f
            ev = dict(f)
            ev["ev"] = "focus"
            _send(ev)
    if flags & ~midi.HW_Dirty_FocusedWindow:
        _inv["dirty"] = True


def OnDirtyMixerTrack(index):
    _inv["dirty"] = True


def OnDirtyChannel(index, flag):
    _inv["dirty"] = True


def OnProjectLoad(status):
    _inv["dirty"] = True


def OnIdle():
    now = time.time()
    if _inv["dirty"] and now - _inv["checked"] >= INV_CHECK_S:
        _inv["checked"] = now
        old = _inv["h"]
        if _inv_hash() != old:
            _send({"ev": "inv", "h": _inv["h"]})
