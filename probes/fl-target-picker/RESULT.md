# FL target picker probe: result

Measured 2026-10-31 in FL Studio Producer Edition 26.1.7 [build 5421] on
macOS, through the protocol-2 bridge in this folder. Project: 18 mixer
tracks; Master slots 0-5 hold ValhallaSupermassive, Portal, Pro-Q 3,
Archetype John Mayer X, ShaperBox 3, ValhallaFutureVerb; channels Sampler
and Serum 2. Raw runs: `runs/20261031-155838-inventory.json`,
`runs/20261031-155942-focus.json`, `runs/20261031-160400-write.json`
(gitignored, local only).

The listening test ran on 2026-11-03 with Noa at FL (section 4, runs
`20261103-114044-listen.json`, `20261103-114415-listen.json`) and the
focus test with FL in front (section 3, `20261103-124703-focus.json`).
Sections 1 and 2 were measured with FL in the background.

## 1. SysEx size limit and the deadlock

- Reply frames up to 65,534 bytes arrive in 0.3 to 2.7 ms.
- A reply frame of 65,536 bytes froze FL. `sample` showed the cause:
  `OnSysEx` runs on a CoreMIDI thread (ident 6164885504) and holds the
  Python GIL; inside that call FL did a
  `performSelectorOnMainThread:waitUntilDone` that opened a modal dialog on
  the main thread (8381857664), and a timer in that dialog
  (QuickFontCache, FLEngine) waits for the GIL. FL had to be force-quit.
- Frames of 65,984 bytes and more do not freeze FL; they time out and the
  next request times out too.
- The script now refuses replies over 60,000 bytes (`MAX_FRAME`) with
  `{"error": "reply too large"}`. Pages default to 3,500 bytes.

## 2. Inventory

| Call | Content | FL time | Size |
|---|---|---|---|
| `mixer` | all 18 tracks, 10 slots each, plugin names, param counts | 0.4 ms | 513 B |
| `gens` | channel rack generators | 0.3 ms | small |

Every VST reports `getParamCount` = 4240. FL appends 144 MIDI-CC
pseudo-parameters at index 4096 to 4239 to every plugin; Valhalla plugins
and Serum also name their own CC slots (Serum: 2048 "CC# Chan #"). A
resolver must drop index >= 4096 and the plugin's own CC names.

| Plugin | Named | Real | FL scan | Wall time at budget 3.5k / 16k / 56k |
|---|---|---|---|---|
| Serum 2 | 2765 | 2604 (idx 0..2620) | 7 ms | 20 / 14 / 13 ms |
| ShaperBox 3 | 1020 | 876 (idx 2..2956; harvest 5037, 875 shared) | 4 ms | 16 / 8 / 7 ms |
| Pro-Q 3 | 502 | 358 | 2 ms | 2-5 ms |
| Archetype John Mayer X | 316 | 172 (harvest 2253) | 2 ms | 2-5 ms |
| Portal | 287 | 143 | 1 ms | 2-5 ms |
| FutureVerb | 176 | 24 | 1 ms | 2-5 ms |
| Supermassive | 162 | 14 | 1 ms | 2-5 ms |

All names of all 7 plugins: 55 ms at budget 3.5k, 34 ms at 16k, 32 ms at
56k, about 136 KB in total. A project with 40 plugins extrapolates to
roughly 0.2 to 0.5 s (estimate, not measured). FL exposes far fewer
parameters than the offline harvest for ShaperBox and Archetype, so the
picker must use FL's list, not the harvest.

## 3. Focus

With FL in the background:

| Call | Result |
|---|---|
| `ui.getFocusedFormID()` | -1, also right after `mixer.focusEditor` |
| `ui.getFocusedPluginName()` | empty |
| `mixer.getActiveEffectIndex()` | None |
| `ui.getFocused(wid)` | 0 or 1 after `ui.showWindow`/`ui.setFocused`, formID then 0 or 1 |
| `channels.selectedChannel()` | follows `selectOneChannel` at once |
| `mixer.trackNumber()` | follows `setTrackNumber` at once |

OnRefresh flags seen: 65831 on `selectOneChannel`, 263 on `setTrackNumber`,
256, and 32 (`HW_Dirty_FocusedWindow`) when the channel selection was
restored. So FL does call back on selection changes.

With FL in front (`focus_probe.py --manual 90 --skip-scripted`, Noa
clicking plugin windows, the channel rack and mixer tracks for 90 s), FL
reports the focused plugin and calls back on every change:

| Clicked | `getFocusedPluginName` | `getFocusedFormID` | `getActiveEffectIndex` | `getFocused` wid |
|---|---|---|---|---|
| Portal window (Master slot 1) | Portal | 65536 | (0, 1) | 5, 6 |
| ShaperBox window (Master slot 4) | ShaperBox 3 | 262144 | (0, 4) | 5, 6 |
| Archetype window (Master slot 3) | Archetype John Mayer X | 196608 | (0, 3) | 5, 6 |
| Serum 2 window (channel 1) | Serum 2 | 1 | None | 5, 7 |
| mixer, channel rack, playlist | empty | 0 to 3 | None | 0 to 3 |

- For a mixer effect the form ID equals `getTrackPluginId(track, slot)`
  and `getActiveEffectIndex` returns (track, slot). For a generator the
  form ID is the channel index. Window ids: 5 = any plugin, 6 = effect,
  7 = generator.
- Every plugin focus change arrived through `OnRefresh` with
  `HW_Dirty_FocusedWindow`, so follow mode needs no polling. Mixer track
  changes (tracks 1, 2, 4) arrived only through the poll, because they do
  not raise that flag; the inventory events cover them.
- Clicking a non-plugin window (mixer, channel rack) clears the plugin
  name. Follow mode must keep the last focused plugin, not drop to none.
300 `getParamName` calls raised no refresh flags (the NI Komplete Kontrol
script claims they raise `HW_Dirty_FocusedWindow`; not confirmed here).

Polling: `OnIdle` ran at 4.2 Hz with gaps up to 6.9 s while FL was in the
background. One focus poll costs 0.04 ms mean, 0.09 ms max. A direct
`ping` (answered in `OnSysEx`) takes 0.26 ms median, 0.40 ms p95; queued
to `OnIdle` it takes 203 ms median, 284 ms p95.

## 4. Writes into mixer slots while the song plays

Event id for parameter `idx` in mixer track `t`, slot `s`:
`mixer.getTrackPluginId(t, s) + midi.REC_Plug_Plugin_First + idx`, where
`getTrackPluginId(t, s) = (t << 22) + (s << 16)` and
`REC_Plug_Plugin_First = 1879080960`; value `round(v * 2**30)` with
`REC_MIDIController`. `plugins.setParamValue(v, idx, t, s)` works too.

Pattern 80 played Serum eighth notes; each parameter went A/B/A/B (0 and
1) through both paths:

| Plugin / parameter | idx | rec: A-B / A-A dB | set: A-B / A-A dB | readback |
|---|---|---|---|---|
| ShaperBox / DriveShaper On | 104 | +0.18 / 0.35 | -0.15 / 0.03 | rec yes, set once lagged |
| ShaperBox / Drive Low Mix | 105 | -0.47 / 0.38 | +0.01 / 0.46 | yes |
| ShaperBox / Drive Mid Mix | 106 | +0.18 / 0.10 | +0.18 / 0.36 | yes |
| ShaperBox / Master Mix | 3 | -0.32 / 0.29 | +0.07 / 0.31 | yes |
| Portal / MACRO 1 | 1 | -0.18 / 0.06 | -0.03 / 0.18 | yes |
| Portal / MACRO 2 | 66 | +0.09 / 0.29 | +0.40 / 0.02 | yes |
| Portal / MASTER DRY/WET | 93 | +0.17 / 0.07 | +0.02 / 0.20 | yes |
| Thermal | - | - | - | not in project |

FL accepts every write and reads the value back. The master peak meter
moved by 0.5 dB at most; the master sits near -1 dB peak, so the meter
cannot judge timbre. Whether the sound changes is open until the listening
test. A read right after a write sometimes returns the old value; after
300 ms it is correct. `DriveShaper On` is a toggle but stores 0.4 and 0.6
without snapping.

Listening test (`listen.py`, pattern 80 playing, each parameter 0/1/0/1
for 3 s through REC, plugin windows open, Noa listening):

| Plugin / parameter | Knob moves in the plugin window | Audible |
|---|---|---|
| Portal / MACRO 1, MACRO 2, MASTER DRY/WET | yes | yes, on a preset with a clear effect (first run on the old preset: MACRO 1 moved, no audible change) |
| ShaperBox / CrushShaper On, Crush Mid Mix, Crush Mid Bits Push, Crush Mid Treble | yes | no difference, because the Crush curves sat at their defaults |
| ShaperBox / Master Mix, DriveShaper On, Drive Low Mix (first run) | not reported | no: the preset has no Drive module, only Crush and Time |

So REC writes reach third-party plugins in mixer slots while the song
plays, the plugin GUIs follow, and Portal changes audibly.

ShaperBox does not expose the Bits and Resample amounts as host
parameters. The amount of a ShaperBox module is the drawn curve per band,
and FL only lists its frame settings per band and module: On, Mix, Split,
Length, Speed, Wave, Smooth, Timing Mode, envelope follower, Push, Dither,
Jitter, Pre-Filter, Treble (876 real parameters). Noa checked by hand:
moving the Bits curve does not register as FL's "last tweaked parameter",
so no host or script can automate it. For bit and sample-rate reduction
the demo uses a plugin with knobs for it (Kilohearts). The picker must not
promise parameters that a plugin keeps internal.

Thermal does not load as a plugin on Noa's machine and was not tested.

Automation recording (record on, 4 s ramp 0 to 1 at 20 Hz through REC,
then playback with the parameter parked at 0.5):

| Parameter | Playback range | Distinct values |
|---|---|---|
| ShaperBox / Drive Low Mix | 0.95 | 66 |
| Portal / MACRO 1 | 0.99 | 64 |

FL records REC writes as automation. `transport.isRecording()` read 0
throughout, so the script cannot trust it to know the record state.
Pattern 80 now holds test notes and recorded automation; delete it.

## Spec proposal

### Picker modes

1. Follow FL. The target is the last plugin FL reported as focused:
   `getActiveEffectIndex` gives (track, slot) for an effect,
   `getFocusedFormID` the channel for a generator, `getFocusedPluginName`
   the name. Updated on `OnRefresh` with `HW_Dirty_FocusedWindow`; focus
   on a non-plugin window keeps the last plugin; the synthexis window itself
   never becomes the target.
2. Track then slot. The user picks a mixer track, then one of its 10 slots.
   Built from the `mixer` inventory (0.4 ms). Works whatever FL reports.
3. Channel. The user picks a channel rack generator from `gens`.

Mode 1 is the default: it worked for effects and generators with FL in
front. Modes 2 and 3 are the manual fallback, and the picker list for
pinning a target. While FL is in the background focus reads stay empty,
so the target simply keeps its last value.

### What the picker shows

Per entry: track number and name, slot, plugin name, user name if FL has
one, count of real parameters (CC pseudo-parameters dropped). Several
plugins of one kind (two Portals) are told apart by track and slot, shown
as "Portal, Master slot 1" and "Portal, Track 3 slot 0". The binding stores
(track, slot, plugin name); if the plugin name in that slot changes, the
binding is dropped and the picker asks again.

### `bridge.fl_script` (spec text)

- Answer inventory and focus reads directly in `OnSysEx`. Keep every reply
  at 16 KB or less and never above 60,000 bytes; page with a `next` cursor.
  A 65,536-byte frame deadlocks FL 26.1.7.
- Run nothing in `OnSysEx` that can open FL UI (editors, dialogs, window
  focus); queue those to `OnIdle`. Expect up to 7 s delay in `OnIdle`
  while FL is in the background.
- Mixer-slot writes use the REC event id above with `REC_MIDIController`;
  `setParamValue` as fallback. Writes are recorded as automation when FL
  records.
- Fetch parameter names lazily when a plugin is picked (7 to 20 ms per
  plugin), not for the whole project at insert. Drop index >= 4096.
- Send inventory changes from `OnRefresh` (flags 256, 263, 65831 seen) as
  events; the client never needs a full rescan after the first one.

### `ui.target_picker` (new node)

- Inputs: FL inventory (tracks, slots, generators), focus events.
- Output: one target (track, slot, plugin name, parameter list) for the
  mapping resolver.
- States: following FL, picked by hand, stale (plugin in slot changed),
  none.
- Shows modes 1 to 3 above; a manual pick overrides follow mode until the
  user switches back.
