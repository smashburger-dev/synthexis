# name=synthexis bridge
# supportedHardwareIds=7D 53 58 46 4C
# supportedDevices=synthexis
#
# synthexis FL bridge, protocol 5: the device script the synthexis plugin
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
#   inv    mixer effect slots and channel rack channels, paged
#          ("start", "budget"); rows ["fx", track, slot, plugin, user,
#          track name], ["gen", channel, plugin, user, channel name,
#          target mixer track, its name] and, for a channel without a
#          plugin that plays notes (Sampler, Layer), ["ch", channel,
#          channel name, target mixer track, its name]; audio and
#          automation clips are left out
#   plist  parameter names of one plugin ("index", "slot", -1 for a
#          generator), paged; FL's MIDI CC entries (index >= 4096) left out
#   focus  the focused plugin, same shape as the focus event
#   rec    set parameter "i" of plugin "p" to "v" (0..1): "fx": [track, slot]
#          through FL's REC event (recorded as automation while FL records),
#          "ch": channel through plugins.setParamValue (recorded too; both
#          measured in probes/fl-target-picker and probes/fl-script-bridge).
#          Nothing is written when another plugin sits there now. Sent
#          without an id by the route writer, up to 30 times a second per
#          parameter, so it gets no reply. Route values come on the 128
#          steps of a MIDI CC and only when they moved a step (route.rs,
#          thin), so FL records as few points as from a hardware knob.
#   nmap   {"m": {"<slot>": channel}}: which channel plays the notes of
#          each slot (MIDI channel 0..15 of the plain notes below); replaces
#          the whole map and ends the notes the old one started
#   note   {"c": channel, "n": note, "v": velocity 1..127, 0 = off}
#   notes  {"e": [[channel, note, velocity], ...]}, the player's batch
#   alloff ends every note this script started
#   (note, notes and rec without an id get no reply)
#
# Channels: every channel index both ways is FL's global index, the one
# midiNoteOn takes. The plain index counts only the channel rack's current
# group filter, so switching the filter would renumber the channels under
# a part and under a held note. So every channel call passes FL's last
# flag, globalCount or useGlobalIndex, as True (FL API docs; FL 26 has it,
# no run measured it with groups). No fallback to the plain index: FL may
# raise TypeError for an empty slot too, and a retry by plain index could
# write to another channel.
#
# Notes: the plugin plays notes as plain note on/off on its source, one
# MIDI channel per slot, 3 bytes each instead of a JSON frame. OnMidiMsg
# plays a mapped slot's notes on its channel with channels.midiNoteOn and
# marks them handled; an unmapped slot's notes go on to FL, which plays
# them on the selected channel like a keyboard's. Measured in
# probes/fl-midi-notes/RESULT.md.
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

PROTO = 5
SLOTS = 10
FL_CC_FIRST = 4096  # FL appends 144 MIDI CC pseudo-parameters from here
PAGE_MAX = 16000
INV_CHECK_S = 0.5  # OnRefresh and OnDirty* fire during playback too
MAX_FRAME = 60000
# channels without a plugin that play notes; None = FL did not say, kept.
# Audio clips (CT_AudioClip) and automation clips (CT_AutoClip) play none,
# nor does a plugin channel whose plugin failed to load (CT_GenPlug).
NOTE_TYPES = (midi.CT_Sampler, midi.CT_Hybrid, midi.CT_Layer, None)


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
    for i in range(channels.channelCount(True)):
        fx = _safe(channels.getTargetFxTrack, i, True) or 0
        if plugins.isValid(i, -1, True):
            rows.append(["gen", i, _safe(plugins.getPluginName, i, -1, False, True) or "",
                         _safe(plugins.getPluginName, i, -1, True, True) or "",
                         _safe(channels.getChannelName, i, True) or "", fx,
                         _safe(mixer.getTrackName, fx) or ""])
        elif _safe(channels.getChannelType, i, True) in NOTE_TYPES:
            # a Sampler or Layer: no parameters, but it plays notes
            rows.append(["ch", i, _safe(channels.getChannelName, i, True) or "", fx,
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
    if name and fx is None and isinstance(form, int) and 0 <= form < channels.channelCount(True):
        ch = form  # a generator's form ID is its (global) channel index
    return {"fx": list(fx) if fx is not None else None, "ch": ch, "plugin": name}


_last_focus = [None]


# --- notes ------------------------------------------------------------------

_nmap = {}  # slot (MIDI channel 0..15) -> global channel index
# (global channel, note) started here and not ended yet -> the channel's
# name at the note-on, so an off still finds its channel after a channel
# above it was deleted or the rack was reordered
_held = {}
OFF_VEL = 0


def _name(ch):
    return _safe(channels.getChannelName, ch, True)


def _where(ch, name):
    """The channel that holds the note started on `ch` as `name` now."""
    if name is None or _name(ch) == name:
        return ch
    for i in range(channels.channelCount(True)):
        if _name(i) == name:
            return i
    return None  # deleted: its notes went with it


def _play(ch, note, vel):
    # ch is global, so an off reaches the channel of its on whatever group
    # the channel rack shows meanwhile
    if vel > 0:
        channels.midiNoteOn(ch, note, vel)
        _held[(ch, note)] = _name(ch)
        return
    at = _where(ch, _held.pop((ch, note), None))
    if at is not None:
        channels.midiNoteOn(at, note, OFF_VEL)


def _all_off():
    for (ch, note), name in list(_held.items()):
        at = _safe(_where, ch, name)
        if at is not None:
            _safe(channels.midiNoteOn, at, note, OFF_VEL)
    _held.clear()


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
        gen = slot == -1  # a generator by global channel index
        stop = min(_safe(plugins.getParamCount, index, slot, gen) or 0, FL_CC_FIRST)
        budget = _budget(cmd)
        out, used, p = [], 80, int(cmd.get("start", 0))
        while p < stop:
            name = plugins.getParamName(p, index, slot, gen)
            if name:
                n = _size([p, name])
                if out and used + n > budget:
                    break
                out.append([p, name])
                used += n
            p += 1
        return {"p": out, "next": p if p < stop else None, "plugin":
                _safe(plugins.getPluginName, index, slot, False, gen) or ""}
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
            if _safe(plugins.getPluginName, ch, -1, False, True) != cmd.get("p"):
                return {"error": "another plugin on that channel"}
            plugins.setParamValue(v, i, ch, -1, midi.PIM_None, True)
        return {"ok": True}
    if op == "nmap":
        _all_off()
        _nmap.clear()
        for slot, ch in cmd.get("m", {}).items():
            _nmap[int(slot) & 0x0F] = int(ch)
        return {"ok": True}
    if op == "note":
        _play(int(cmd["c"]), int(cmd["n"]) & 0x7F, max(0, min(127, int(cmd.get("v", 0)))))
        return {"ok": True}
    if op == "notes":
        for c, n, v in cmd.get("e", []):
            _safe(_play, int(c), int(n) & 0x7F, max(0, min(127, int(v))))
        return {"ok": True}
    if op == "alloff":
        _all_off()
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
    if rid is None and cmd.get("cmd") in ("rec", "note", "notes"):
        return
    res["id"] = rid
    _send(res)


def OnMidiMsg(event):
    kind = event.status & 0xF0
    if kind not in (0x80, 0x90):
        return
    ch = _nmap.get(event.status & 0x0F)
    if ch is None:
        return
    _safe(_play, ch, event.data1, event.data2 if kind == 0x90 else 0)
    event.handled = True


# --- events -----------------------------------------------------------------

def OnInit():
    _inv["dirty"] = True
    _last_focus[0] = _focus()
    _send({"ev": "hello", "proto": PROTO})


def OnDeInit():
    # the plugin's ports went away (unloaded, FL closing): nothing may hang
    _all_off()


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
