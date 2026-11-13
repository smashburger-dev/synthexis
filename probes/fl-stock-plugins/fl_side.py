# Runs inside FL through device_synthexis_dev.py (exec'd into its namespace
# by run_stock.py). Every plugin of the open project as the synthexis bridge
# sees it, plus a write test along the bridge's own write paths.

import time

FL_CC_FIRST = 4096


def places():
    """[(kind, index, slot, global, plugin name)] like the bridge inventory."""
    out = []
    for t in range(mixer.trackCount()):
        for s in range(10):
            if plugins.isValid(t, s):
                out.append(("fx", t, s, False, plugins.getPluginName(t, s)))
    for i in range(channels.channelCount(True)):
        if plugins.isValid(i, -1, True):
            out.append(("gen", i, -1, True, plugins.getPluginName(i, -1, False, True)))
    return out


def names(index, slot, g):
    """The bridge's plist: indices below 4096 with a non-empty name."""
    n = min(plugins.getParamCount(index, slot, g), FL_CC_FIRST)
    out = []
    for p in range(n):
        name = plugins.getParamName(p, index, slot, g)
        if name:
            out.append((p, name))
    return out


def write(kind, index, slot, p, v):
    """The bridge's rec handler, without its plugin-name check."""
    if kind == "fx":
        eid = mixer.getTrackPluginId(index, slot) + midi.REC_Plug_Plugin_First + p
        general.processRECEvent(eid, int(v * (1 << 30)), midi.REC_MIDIController)
    else:
        plugins.setParamValue(v, p, index, -1, midi.PIM_None, True)


_pending = {}


def write_test_start(key, limit=16):
    """Moves the first `limit` named parameters far from where they are;
    returns how many."""
    kind, index, slot, g, _ = [x for x in places() if (x[0], x[1], x[2]) == tuple(key)][0]
    rows = []
    for p, name in names(index, slot, g)[:limit]:
        v0 = plugins.getParamValue(p, index, slot, g)
        target = 0.1 if v0 >= 0.5 else 0.9
        write(kind, index, slot, p, target)
        rows.append((p, name, v0, target))
    _pending[tuple(key)] = rows
    return len(rows)


def write_test_finish(key):
    """Reads back, restores; returns [(p, name, v0, target, after)] that did not move."""
    kind, index, slot, g, _ = [x for x in places() if (x[0], x[1], x[2]) == tuple(key)][0]
    stuck, moved = [], 0
    for p, name, v0, target in _pending.pop(tuple(key)):
        after = plugins.getParamValue(p, index, slot, g)
        if abs(after - v0) > 1e-4:
            moved += 1
        else:
            stuck.append((p, name, round(v0, 4), target, round(after, 4)))
        plugins.setParamValue(v0, p, index, slot, midi.PIM_None, g)
    return {"moved": moved, "stuck": stuck}
