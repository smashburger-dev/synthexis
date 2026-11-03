#!/usr/bin/env python3
"""Runs plugin/fl/device_synthexis_bridge.py outside FL, against fake FL
modules and a small fixed project, for the end-to-end test without FL
(fakefl.swift carries the frames over CoreMIDI).

stdin: one hex-encoded SysEx frame per line (plugin -> FL).
stdout: one hex-encoded SysEx frame per line (FL -> plugin).
stderr: what happened.

Project: Master slot 0 ValhallaSupermassive, slot 1 Portal; track 2 slot 0
synthexis; channel 0 Sampler (no plugin), channel 1 Serum 2 with 2,700
named parameters, own CC names and FL's CC entries from 4096. The focus
script clicks Portal, the mixer, synthexis, Serum 2, then adds Pro-Q 3 to
track 3 slot 0 and clicks it.
"""

import json
import os
import select
import sys
import time
import types

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "..", "device_synthexis_bridge.py")

FX = {(0, 0): "ValhallaSupermassive", (0, 1): "Portal", (2, 0): "synthexis"}
GENS = {0: ("Sampler", None), 1: ("Serum 2", "Serum 2")}
TRACKS = ["Master", "Insert 1", "Insert 2", "Insert 3"]
FOCUS = {"name": "", "fx": None, "form": -1}


def params_of(plugin):
    if plugin == "Serum 2":
        names = {i: "Param %d" % i for i in range(2700)}
        names[0] = "Filter 1 Freq"
        names.update({2700 + i: "CC#%d Chan 1" % i for i in range(17)})
    else:
        names = {i: "%s P%d" % (plugin, i) for i in range(40)}
    names.update({4096 + i: "CC %d" % i for i in range(144)})
    return names


def plugin_at(index, slot):
    if slot == -1:
        return GENS.get(index, (None, None))[1]
    return FX.get((index, slot))


def log(*a):
    print(*a, file=sys.stderr, flush=True)


def out(data):
    sys.stdout.write(bytes(data).hex() + "\n")
    sys.stdout.flush()


def install_fakes():
    m = types.ModuleType
    device, midi, mixer, plugins, channels, ui, general = (m(n) for n in
                                                           ("device", "midi", "mixer", "plugins", "channels", "ui",
                                                            "general"))
    device.midiOutSysex = out
    midi.HW_Dirty_FocusedWindow = 32
    midi.REC_Plug_Plugin_First = 1879080960
    midi.REC_MIDIController = 4
    mixer.getTrackPluginId = lambda t, s: (t << 22) + (s << 16)
    general.processRECEvent = lambda eid, v, flags: log("fake: rec", eid, v) or v
    plugins.setParamValue = lambda v, p, index, slot=-1, *_: log("fake: set", index, slot, p, v)
    mixer.trackCount = lambda: len(TRACKS)
    mixer.getTrackName = lambda t: TRACKS[t]
    mixer.getActiveEffectIndex = lambda: FOCUS["fx"]
    channels.channelCount = lambda: len(GENS)
    channels.getChannelName = lambda i: GENS[i][0]
    channels.getTargetFxTrack = lambda i: 0

    def valid(index, slot=-1, *_):
        return plugin_at(index, slot) is not None

    def name(index, slot=-1, user=False, *_):
        p = plugin_at(index, slot)
        if p is None:
            raise TypeError("no plugin")
        return "" if user else p

    plugins.isValid = valid
    plugins.getPluginName = name
    plugins.getParamCount = lambda index, slot=-1, *_: 4240
    plugins.getParamName = lambda p, index, slot=-1, *_: params_of(plugin_at(index, slot)).get(p, "")
    ui.getFocusedPluginName = lambda: FOCUS["name"]
    ui.getFocusedFormID = lambda: FOCUS["form"]
    for mod in (device, midi, mixer, plugins, channels, ui, general):
        sys.modules[mod.__name__] = mod


class Msg:
    def __init__(self, data):
        self.sysex = data


def click(name, fx=None, form=-1):
    FOCUS.update(name=name, fx=fx, form=form)
    log("fake: focus", name or "(mixer)", fx, form)


STEPS = [
    (1.0, lambda: click("Portal", (0, 1), 65536)),
    (2.0, lambda: click("", None, 0)),
    (3.0, lambda: click("synthexis", (2, 0), (2 << 22))),
    (4.0, lambda: click("Serum 2", None, 1)),
    (5.0, lambda: (FX.__setitem__((3, 0), "Pro-Q 3"), log("fake: Pro-Q 3 added"))),
    (6.0, lambda: click("Pro-Q 3", (3, 0), (3 << 22))),
]


def main():
    install_fakes()
    bridge = types.ModuleType("bridge")
    exec(compile(open(SCRIPT).read(), SCRIPT, "exec"), bridge.__dict__)
    bridge.OnInit()
    t0, step, served, biggest = time.time(), 0, 0, 0
    while True:
        r, _, _ = select.select([sys.stdin], [], [], 0.05)
        if r:
            line = sys.stdin.readline()
            if not line:
                break
            data = bytes.fromhex(line.strip())
            bridge.OnSysEx(Msg(data))
            served += 1
        now = time.time() - t0
        while step < len(STEPS) and now >= STEPS[step][0]:
            STEPS[step][1]()
            bridge.OnRefresh(32 if step != 4 else 2)
            step += 1
        bridge.OnIdle()


if __name__ == "__main__":
    real_out = out

    def counting(data):
        if len(data) > 60000:
            log("fake: frame over 60,000 bytes!", len(data))
        msg = json.loads(bytes(data[2:-1]).decode("ascii"))
        log("fake: ->", {k: (v if k not in ("rows", "p") else "%d items" % len(v)) for k, v in msg.items()},
            len(data), "B")
        real_out(data)

    out = counting
    main()
