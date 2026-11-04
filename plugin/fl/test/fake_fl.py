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

--check [script]: no frames, no plugin. Runs the script's channel rules
against a rack with an automation clip, an audio clip and a group filter
that renumbers the plain channel index; exits 1 when one breaks.
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
GENS = {0: ("Sampler", None), 1: ("Serum 2", "Serum 2")}  # global index -> (name, plugin)
TYPES = {}  # global index -> midi.CT_*, when not Sampler / plugin channel
TRACKS = ["Master", "Insert 1", "Insert 2", "Insert 3"]
FOCUS = {"name": "", "fx": None, "form": -1}
# global indices the channel rack's group filter shows, None = "All"; the
# plain (group) index counts these only
RACK = {"group": None}
NOTES = []  # (global channel, note, velocity) as midiNoteOn got them
SETS = []  # (index, slot, param, value) as setParamValue got them
CT_SAMPLER, CT_HYBRID, CT_GENPLUG, CT_LAYER, CT_AUDIOCLIP, CT_AUTOCLIP = range(6)


def rack(i, glob):
    """Global channel index of channel `i`, a group index unless `glob`."""
    if glob or RACK["group"] is None:
        if i not in GENS:
            raise TypeError("channel index out of range")
        return i
    if not 0 <= i < len(RACK["group"]):
        raise TypeError("channel index out of range")
    return RACK["group"][i]


def params_of(plugin):
    if plugin == "Serum 2":
        names = {i: "Param %d" % i for i in range(2700)}
        names[0] = "Filter 1 Freq"
        names.update({2700 + i: "CC#%d Chan 1" % i for i in range(17)})
    else:
        names = {i: "%s P%d" % (plugin, i) for i in range(40)}
    names.update({4096 + i: "CC %d" % i for i in range(144)})
    return names


def plugin_at(index, slot, glob=False):
    if slot == -1:
        return GENS[rack(index, glob)][1]
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
    midi.PIM_None = 0
    (midi.CT_Sampler, midi.CT_Hybrid, midi.CT_GenPlug, midi.CT_Layer, midi.CT_AudioClip,
     midi.CT_AutoClip) = range(6)
    mixer.getTrackPluginId = lambda t, s: (t << 22) + (s << 16)
    general.processRECEvent = lambda eid, v, flags: log("fake: rec", eid, v) or v

    def set_param(v, p, index, slot=-1, pickup=0, glob=False):
        at = rack(index, glob) if slot == -1 else index
        SETS.append((at, slot, p, v))
        log("fake: set", at, slot, p, v)

    plugins.setParamValue = set_param
    mixer.trackCount = lambda: len(TRACKS)
    mixer.getTrackName = lambda t: TRACKS[t]
    mixer.getActiveEffectIndex = lambda: FOCUS["fx"]
    channels.channelCount = lambda glob=False: len(GENS) if glob or RACK["group"] is None else len(RACK["group"])
    channels.getChannelName = lambda i, glob=False: GENS[rack(i, glob)][0]
    channels.getChannelIndex = lambda i: rack(i, False)

    def target_fx(i, glob=False):
        rack(i, glob)  # raises for a channel outside the rack, like FL
        return 0

    def channel_type(i, glob=False):
        g = rack(i, glob)
        return TYPES.get(g, CT_SAMPLER if GENS[g][1] is None else CT_GENPLUG)

    def note_on(c, n, v, *_):
        NOTES.append((c, n, v))
        log("fake: note", c, n, v)

    channels.getTargetFxTrack = target_fx
    channels.getChannelType = channel_type
    channels.midiNoteOn = note_on

    def valid(index, slot=-1, glob=False):
        try:
            return plugin_at(index, slot, glob) is not None
        except TypeError:
            return False

    def name(index, slot=-1, user=False, glob=False):
        p = plugin_at(index, slot, glob)
        if p is None:
            raise TypeError("no plugin")
        return "" if user else p

    plugins.isValid = valid
    plugins.getPluginName = name
    plugins.getParamCount = lambda index, slot=-1, glob=False: 4240
    plugins.getParamName = lambda p, index, slot=-1, glob=False: params_of(plugin_at(index, slot, glob)).get(p, "")
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


def load(script):
    bridge = types.ModuleType("bridge")
    exec(compile(open(script).read(), script, "exec"), bridge.__dict__)
    return bridge


def check(script):
    install_fakes()
    sys.modules["device"].midiOutSysex = lambda data: None
    GENS.clear()
    GENS.update({0: ("Kick", None), 1: ("Filter Cutoff", None), 2: ("3x Osc", "3x Osc"), 3: ("Loop", None),
                 4: ("Perc", None)})
    TYPES.update({1: CT_AUTOCLIP, 3: CT_AUDIOCLIP})
    bridge = load(script)
    bridge.OnInit()
    failed = []

    def expect(rule, got, want):
        ok = got == want
        log(("ok  " if ok else "FAIL"), rule, "" if ok else "got %r, want %r" % (got, want))
        if not ok:
            failed.append(rule)

    def rows():
        return [r[:3] for r in bridge._handle({"cmd": "inv"})["rows"] if r[0] != "fx"]

    every = [["ch", 0, "Kick"], ["gen", 2, "3x Osc"], ["ch", 4, "Perc"]]
    expect("note channels: the clips are left out", rows(), every)
    bridge._handle({"cmd": "notes", "e": [[2, 60, 100]]})
    # a group without 3x Osc: its plain index 2 is Perc
    RACK["group"] = [0, 1, 4]
    expect("inventory: the group filter changes no index", rows(), every)
    bridge._handle({"cmd": "notes", "e": [[2, 60, 0], [2, 62, 100]]})
    expect("an off after a filter switch reaches its on's channel", NOTES[:2], [(2, 60, 100), (2, 60, 0)])
    expect("a note under a group filter plays on the global channel", NOTES[2:], [(2, 62, 100)])
    bridge._handle({"cmd": "alloff"})
    expect("alloff ends it there, nothing held", (NOTES[3:], sorted(bridge._held)), ([(2, 62, 0)], []))
    expect("rec on a generator by global index",
           (bridge._handle({"cmd": "rec", "ch": 2, "p": "3x Osc", "i": 0, "v": 0.5}), SETS),
           ({"ok": True}, [(2, -1, 0, 0.5)]))
    expect("plist of a generator by global index",
           bridge._handle({"cmd": "plist", "index": 2, "slot": -1}).get("plugin"), "3x Osc")
    RACK["group"] = [0, 4]
    FOCUS.update(name="3x Osc", fx=None, form=2)
    expect("focus on a generator outside the group", bridge._focus().get("ch"), 2)
    # a channel above a sounding note is deleted: the off follows the name
    RACK["group"] = None
    del NOTES[:]
    bridge._handle({"cmd": "notes", "e": [[2, 64, 100]]})
    moved = {i - 1 if i > 0 else i: v for i, v in GENS.items() if i != 0}
    GENS.clear()
    GENS.update(moved)
    bridge._handle({"cmd": "notes", "e": [[2, 64, 0]]})
    expect("an off after a deleted channel above reaches the moved channel", NOTES, [(2, 64, 100), (1, 64, 0)])
    expect("nothing held after it", dict(bridge._held), {})
    return 1 if failed else 0


def main():
    install_fakes()
    bridge = load(SCRIPT)
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
    if sys.argv[1:2] == ["--check"]:
        sys.exit(check(sys.argv[2] if len(sys.argv) > 2 else SCRIPT))
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
