
# --- probes/fl-midi-notes: probe-only commands --------------------------------
# Appended to plugin/fl/device_synthexis_bridge.py for the run only:
#   cat plugin/fl/device_synthexis_bridge.py probes/fl-midi-notes/probe_ext.py
# Never shipped.

import transport

_echo = [False]
_pass_held = {}  # unmapped notes FL played itself: (status & 0x0F, note) -> count
_product_handle = _handle
_product_midi = OnMidiMsg


def _handle(cmd):
    op = cmd.get("cmd")
    if op == "sel":
        channels.selectOneChannel(int(cmd["c"]))
        return {"sel": channels.selectedChannel()}
    if op == "chans":
        return {"c": [[i, _safe(channels.getChannelName, i) or "", _safe(channels.getChannelIndex, i)]
                      for i in range(channels.channelCount())], "sel": channels.selectedChannel()}
    if op == "transport":
        want = cmd.get("do")
        if want == "rec" and not transport.isRecording():
            transport.record()
        if want == "norec" and transport.isRecording():
            transport.record()
        if want == "play" and not transport.isPlaying():
            transport.start()
        if want == "stop":
            transport.stop()
        return {"rec": transport.isRecording(), "play": transport.isPlaying()}
    if op == "held":
        return {"h": [[c, n] for c, n in _held], "pass": [[k[0], k[1], v] for k, v in _pass_held.items() if v]}
    if op == "echo":
        _echo[0] = bool(cmd.get("on"))
        return {"echo": _echo[0]}
    if op == "offvel":
        global OFF_VEL
        OFF_VEL = int(cmd["v"])
        return {"offvel": OFF_VEL}
    if op == "level":
        c = int(cmd["c"])
        return {"level": _safe(channels.getActivityLevel, c)}
    return _product_handle(cmd)


def OnMidiMsg(event):
    kind = event.status & 0xF0
    if kind in (0x80, 0x90):
        if _echo[0]:
            _send({"ev": "seen", "s": event.status, "n": event.data1, "v": event.data2})
        slot = event.status & 0x0F
        if slot not in _nmap:
            key = (slot, event.data1)
            on = kind == 0x90 and event.data2 > 0
            _pass_held[key] = _pass_held.get(key, 0) + (1 if on else -1)
    _product_midi(event)
