# name=synthexis bridge probe
# url=
# receiveFrom=IAC Driver Bus 1
#
# synthexis bridge probe (P3) — FL Studio Python MIDI controller script.
# Install: this file goes in
#   ~/Documents/Image-Line/FL Studio/Settings/Hardware/synthexis bridge probe/
# Behavior on incoming CC:
#   CC 21 (any value): dump name + param count + first 200 params (index,
#     name, value, value-string if available) of the SELECTED channel's
#     plugin to the script output window.
#   CC 22 (value v):  select parameter index P = v (0..127).
#   CC 20 (value v):  set param P to v/127 via plugins.setParamValue().
#   CC 23 (value v):  same target through the recordable-event path:
#     general.processRECEvent(midi.REC_PlugBase | (channel << 16) | P,
#                             int(v/127*65535), midi.REC_FromMIDI).
#     If that formula is wrong on this FL build the dump output prints the
#     API surface (REC_* constants) so we can fix it.

import channels
import general
import midi
import plugins

_param_index = 0
_api_dumped = False


def _cc(event):
    """Return (control, value) if event is a CC message."""
    data1 = getattr(event, "data1", None)
    data2 = getattr(event, "data2", None)
    if data1 is None:
        return None
    status = getattr(event, "status", -1)
    midi_id = getattr(event, "midiId", -1)
    is_cc = midi_id == getattr(midi, "MIDI_CONTROLCHANGE", 0xB0) or (
        0xB0 <= status <= 0xBF
    )
    return (data1, data2) if is_cc else None


def _dump_api_surface():
    global _api_dumped
    if _api_dumped:
        return
    _api_dumped = True
    print("[bridge] midi.REC_* constants:",
          [c for c in dir(midi) if c.startswith("REC_")])
    print("[bridge] plugins API:",
          [c for c in dir(plugins) if "Param" in c or "Plugin" in c])
    print("[bridge] processRECEvent:",
          hasattr(general, "processRECEvent"))


def _sel_chan():
    try:
        return channels.selectedChannel()
    except Exception as e:
        print("[bridge] selectedChannel failed:", e)
        return 0


def _dump_plugin(ch):
    try:
        name = plugins.getPluginName(ch)
    except Exception as e:
        name = f"<getPluginName failed: {e}>"
    try:
        n = plugins.getParamCount(ch)
    except Exception as e:
        n = -1
        print("[bridge] getParamCount failed:", e)
    print(f"[bridge] channel={ch} plugin='{name}' paramCount={n}")
    for i in range(min(max(n, 0), 200)):
        try:
            pname = plugins.getParamName(i, ch)
        except Exception:
            pname = "?"
        try:
            pval = plugins.getParamValue(i, ch)
        except Exception:
            pval = "?"
        vstr = "?"
        try:
            if hasattr(plugins, "getParamValueString"):
                vstr = plugins.getParamValueString(i, ch)
        except Exception:
            pass
        print(f"[bridge]   [{i}] {pname} = {pval}  ({vstr})")


def _set_param(ch, v):
    try:
        plugins.setParamValue(_param_index, v / 127.0, ch)
        print(f"[bridge] setParamValue({_param_index}, {v/127.0:.3f}, ch={ch})")
    except Exception as e:
        print("[bridge] setParamValue failed:", e)


def _set_param_rec(ch, v):
    # Recordable-event path. Event-id formula per FL API docs convention
    # (REC_PlugBase + channel<<16 + index); verify against dump output.
    try:
        base = getattr(midi, "REC_PlugBase", 0x00008000)
        evt = base + (ch << 16) + _param_index
        general.processRECEvent(evt, int(v / 127.0 * 65535),
                                getattr(midi, "REC_FromMIDI", 0))
        print(f"[bridge] processRECEvent(evt=0x{evt:08x}, v={v}/127)")
    except Exception as e:
        print("[bridge] processRECEvent failed:", e)


def OnMidiMsg(event, timestamp):
    global _param_index
    cc = _cc(event)
    if cc is None:
        return
    control, value = cc
    ch = _sel_chan()
    if control == 21:
        _dump_api_surface()
        _dump_plugin(ch)
        event.handled = True
    elif control == 22:
        _param_index = value
        print(f"[bridge] param index -> {value}")
        event.handled = True
    elif control == 20:
        _set_param(ch, value)
        event.handled = True
    elif control == 23:
        _set_param_rec(ch, value)
        event.handled = True


def OnInit():
    print("[bridge] synthexis bridge probe loaded")
