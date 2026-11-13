# name=sxdev
# supportedHardwareIds=7D 53 58 44 56
# supportedDevices=sxdev
#
# Probe-only FL device script: runs Python sent by fldev.py inside FL, so a
# probe can read and write FL's script API from the shell. It evaluates any
# code it receives, so it is never installed by plugin/install.sh and never
# shipped; copy it into FL's Hardware folder for a probe and delete it after.
#
# fldev.py owns a virtual MIDI source and destination named "sxdev" (no "synthexis" prefix: FL matches supportedDevices by prefix and gave a port called "synthexis dev" the bridge script)
# and answers FL's device inquiry with 7D 53 58 44 56, which FL matches
# against supportedHardwareIds above, so no MIDI settings are needed (FL reads
# the Hardware folder only at start).
#
# Frames both ways: F0 7D <7-bit ASCII JSON> F7.
# Request {"id": n, "py": "<code>", "main": false}: the code runs in one
# namespace kept across calls; if it is an expression, its value is the
# reply's "r". "main": true runs it in OnIdle (FL's main thread) instead of
# OnSysEx (the CoreMIDI thread).

import json
import sys

import device

MAX_FRAME = 60000
NS = {}
exec("import channels, device, general, midi, mixer, patterns, plugins, transport, ui", NS)
_queue = []


class _Out:
    def __init__(self):
        self.parts = []

    def write(self, s):
        self.parts.append(s)

    def flush(self):
        pass


def _send(msg):
    data = json.dumps(msg, separators=(",", ":"), default=repr).encode("ascii")
    if len(data) + 3 > MAX_FRAME:
        data = json.dumps({"id": msg.get("id"), "error": "reply too large",
                           "bytes": len(data) + 3}).encode("ascii")
    device.midiOutSysex(bytes([0xF0, 0x7D]) + data + bytes([0xF7]))


def _run(cmd):
    code = cmd.get("py", "")
    out, old = _Out(), sys.stdout
    sys.stdout = out
    res = {"id": cmd.get("id")}
    try:
        try:
            res["r"] = eval(compile(code, "<fldev>", "eval"), NS)
        except SyntaxError:
            exec(compile(code, "<fldev>", "exec"), NS)
            res["r"] = None
    except Exception as e:
        res["error"] = type(e).__name__ + ": " + str(e)
    finally:
        sys.stdout = old
    res["out"] = "".join(out.parts)
    _send(res)


def OnSysEx(msg):
    try:
        data = bytes(msg.sysex)
        start = data.index(0xF0)
        if data[start + 1] != 0x7D:
            return
        cmd = json.loads(data[start + 2:data.index(0xF7, start + 2)].decode("ascii"))
    except Exception:
        return
    msg.handled = True
    if cmd.get("main"):
        _queue.append(cmd)
    else:
        _run(cmd)


def OnIdle():
    while _queue:
        _run(_queue.pop(0))


def OnInit():
    _send({"ev": "sxdev"})  # not "hello": the plugin would take it as the bridge restarting
