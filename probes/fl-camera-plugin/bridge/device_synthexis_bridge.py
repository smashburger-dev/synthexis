# name=synthexis bridge
#
# synthexis FL bridge — SysEx command executor for the H2 automation tests.
# Commands arrive as: F0 7D <7-bit-safe UTF-8 JSON> F7
# (0x7D = non-commercial manufacturer ID; JSON is ASCII-only so 7-bit safe.)
# Every reply goes out on the assigned output via device.midiOutSysex in the
# same framing and carries the request "id".
# One command = one quick API call; nothing here blocks or sleeps.

import json
import os
import time

import channels
import device
import general
import midi
import patterns
import plugins
import transport

MAX_PARAMS = 128


def _reply(request_id, payload):
    msg = {"id": request_id}
    msg.update(payload)
    data = json.dumps(msg, separators=(",", ":")).encode("ascii")
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


def _handle(cmd):
    op = cmd.get("cmd")
    if op == "ping":
        return {"ok": True, "proto": 1}
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
    _log("OnInit port=%s" % _safe(device.getPortNumber))


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


def OnSysEx(msg):
    try:
        payload = _sysex_payload(msg)
        _log("OnSysEx payload=%s" % (payload[:80] if payload else None))
        if payload is None:
            return
        cmd = json.loads(payload.decode("ascii"))
        rid = cmd.get("id")
        try:
            _reply(rid, _handle(cmd))
        except Exception as e:
            _reply(rid, {"error": str(e)})
    except Exception:
        # Never let a bad packet break the script.
        pass


def OnIdle():
    now = time.time()
    if now - _last_beat[0] > 10:
        _last_beat[0] = now
        _log("alive")
