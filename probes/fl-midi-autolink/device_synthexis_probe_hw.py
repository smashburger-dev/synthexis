# name=synthexis probe hw
# supportedHardwareIds=7D 53 58 01

# Probe for FL auto-assign (probes/fl-midi-autolink): says hello on init and
# answers {"cmd":"ping"} so the probe sees whether the output got linked.
import json
import device

SCRIPT = "hw"


def _send(msg):
    device.midiOutSysex(bytes([0xF0, 0x7D]) + json.dumps(msg).encode("ascii") + bytes([0xF7]))


def _info():
    return {"script": SCRIPT, "dev": device.getName(), "port": device.getPortNumber(),
            "assigned": bool(device.isAssigned())}


def OnInit():
    msg = _info()
    msg["ev"] = "hello"
    _send(msg)


def OnSysEx(event):
    data = bytes(event.sysex)
    if len(data) < 4 or data[1] != 0x7D:
        return
    try:
        cmd = json.loads(data[2:data.index(0xF7)].decode("ascii"))
    except Exception:
        return
    msg = _info()
    msg["id"] = cmd.get("id")
    _send(msg)
    event.handled = True
