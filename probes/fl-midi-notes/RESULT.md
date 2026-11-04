# FL MIDI notes: result

FL Studio 26.1.7 on macOS, M1 Pro. Question: how do notes synthexis makes
reach an FL channel the user picks in synthexis, get heard and get recorded
by FL, with nobody opening MIDI settings or the plugin wrapper?

## Paths

- N1 plugin MIDI out (VST3 event bus / CLAP note port from `process()`).
  Sample-accurate, but FL passes it only from the wrapper's MIDI output port
  to channels with the same input port, set by hand; no script API sets
  either. It fails "no settings" by construction, so this run leaves it out.
  It gets measured only if N2 and N3 both fail "recorded".
- N2 pass: plain note on/off on the "synthexis" CoreMIDI source, left
  unhandled by the script. FL treats them like a keyboard's notes: the
  selected channel plays them (documented).
- N3a route: the same notes; the script's `OnMidiMsg` calls
  `channels.midiNoteOn(target, note, velocity)` and marks the event handled.
  The target per MIDI channel (16 slots) comes from a SysEx `nmap` command,
  so a note costs 3 bytes, not a JSON frame.
- N3b sysex: a JSON frame `{"cmd":"note"}` per note, handled in `OnSysEx`
  with the same `channels.midiNoteOn`.
- N4 timestamps: four notes 125 ms apart sent in one CoreMIDI packet list,
  each with its future host timestamp, through N2.

## Setup

- New FL project, tempo 120, pattern mode, synthexis not loaded. The probe
  (`noteprobe.swift`) opens the "synthexis" source and destination itself
  and answers FL's device inquiry like the plugin, so FL binds them to the
  "synthexis bridge" script on port 236 as it does for the plugin.
- Script during the run: `plugin/fl/device_synthexis_bridge.py` (protocol 5,
  with `nmap`, `note` and `OnMidiMsg`) with `probe_ext.py` appended; the
  extension adds probe-only commands: `sel` (select a channel), `chans`
  (all channel names), `transport` (record, play, stop), `held` (note-ons
  minus note-offs per channel and note), and an echo of every note the
  script sees. Copied while FL is closed; the product script goes back
  after the run.
- Channels: X = a sustaining synth (FL's 3x Osc), the target; Y = another
  channel, selected during N3, so a note heard on X cannot come from the
  selection.

## Rule (written 2026-11-03 23:45 CET, before the run)

Per path N2, N3a, N3b:

1. Sound: N2 plays on the selected channel Y; N3a and N3b play on X while Y
   stays selected. Checked by ear (Noa) and by the channel's activity light
   in a screenshot.
2. Recorded: FL records (record mode, playback running) while the probe
   sends 16 notes, one per 1/16 at 120 BPM (one bar, so a 1-bar pattern
   loop cannot stack them; changed from 1/8 before the recording run),
   each 60 ms long, on one pitch
   per path (N2 C5 = MIDI 60 in FL's naming, N3a E5 = 64, N3b G5 = 67). Pass:
   16 of 16 notes of that pitch in the pattern of the channel that played
   them, read in its piano roll.
3. Delivery: the script echoes every note it sees; probe send to echo
   arrival, over at least 200 notes. Pass: p95 at most 10 ms. The SysEx ping
   round trip of the same run is the baseline. N2 is unhandled, so its
   echo comes from `OnMidiMsg` before FL plays the note.
4. Note-offs: after each run `held` reports 0 for every channel and note,
   and X is silent 1 s after the last note-off (screenshot of its light).
5. Load: 30 note-ons per second, each with its note-off 20 ms later, for
   20 s, while the probe pings every 100 ms. Pass: every ping answered, ping
   round trip p95 at most 50 ms, FL's playhead moves on screenshots taken
   during the load, `held` 0 afterwards.

N4: pass when the four recorded notes sit at least 100 ms apart (FL honours
the timestamps), fail when they stack within 20 ms (FL uses arrival time).
It decides whether the plugin may send notes ahead with timestamps or has to
send each at its time; it does not rule a path in or out.

Not measured in this run: timing against FL's grid. The probe's clock is not
FL's. The run with the built plugin (R2, below) reads the recorded positions
of a 1/16 pattern against the grid: pass when every note sits within 10 ms
of its grid line after removing one constant offset, which is reported as
the lead the plugin sends ahead.

Decision:
- N3a passes 1 to 5: build on N3a. The plugin sends plain notes, one MIDI
  channel per target slot; the script routes each slot to the channel picked
  in synthexis.
- N3a plays on X but fails "recorded" while N2 passes it: ask Noa (sound
  goes to any channel; recording needs the target selected in FL).
- N3a and N3b both fail "sound": N2 with the script selecting the target
  channel (one target at a time), then measure N1.

## Results (FL 26.1.7, 2026-11-03 23:55 to 2026-11-04 00:20 CET)

Project: channel 0 Sampler, channel 1 3x Osc, Pattern 2 empty. The script
said hello 18 s after the probe opened the ports with FL starting; nobody
opened MIDI settings. Log: `runs/2026-11-03-noteprobe.log`. Piano roll read
in screenshots of FL's window.

| | N2 pass | N3a route | N3b sysex |
|---|---|---|---|
| 1 sound | FL received them (hint bar "Note off: C5"), the Sampler had no sample, so nothing to hear | heard on 3x Osc while the Sampler was selected (Noa); activity 0.5 during a note, 0 after | heard on 3x Osc while the Sampler was selected (Noa) |
| 2 recorded | 16 of 16 C5 in the Sampler's pattern | 16 of 16 E5 in 3x Osc's pattern | 16 of 16 G5 in 3x Osc's pattern |
| 3 delivery, send to echo | 0.31 / 0.42 ms (median / p95, n = 64, N2 and N3a together) | same | 0.47 / 0.62 ms (reply round trip, n = 32) |
| 4 note-offs | `held` empty, activity 0 | `held` empty, activity 0 | `held` empty |

All three pass 1 to 4 (N2's sound by FL's hint bar only). The recorded notes
sit 1/16 apart at the probe's clock, off FL's grid lines (the second group
starts 5.33 steps into its bar), so FL records arrival positions without
quantizing.

5 load: 600 note-ons and 600 note-offs in 20 s on N3a, two runs. Transport
stopped: 200 of 200 pings answered, round trip 0.49 / 1.17 / 4.95 ms (median
/ p95 / max). Playing (and recording, record mode was still on): 200 of 200,
0.45 / 0.90 / 4.63 ms; the song position moved from 0:03 to 0:17 between two
captures and the hint bar showed the arriving notes; every note echoed
(p95 0.37 ms); `held` empty and the 3x Osc silent afterwards. PASS.

N4: four notes sent in one packet list with timestamps 125 ms apart, once
unmapped (C6, played by the then selected 3x Osc) and once mapped (E6 on
3x Osc through `midiNoteOn`): both groups recorded as four adjacent 1/16
notes, none stacked. PASS: FL's MIDI input delivers each packet at its
timestamp, also into the script.

Side observation: `transport.record()` from the script switched record mode
on, but the second call did not switch it off (the rec button stayed lit).

## Decision

N3 it is: synthexis names the FL channel per part, the script plays the
notes there with `channels.midiNoteOn`, FL records them into that
channel's pattern, nobody touches MIDI or wrapper settings. The player sends
batches as `{"cmd":"notes"}` frames (N3b, one frame per batch, no slot table
to keep in sync); `nmap` and `OnMidiMsg` (N3a) stay in the script as the
3-byte path if frames ever cost too much. N1 is not needed. Because FL
honours timestamps (N4), the player may send notes ahead with their
timestamp instead of relying on its own wake-up.

## R2 (open): timing against the grid with the built plugin

Rule as above: a 1/16 hi-hat clip from the player, recorded while FL plays;
pass when every note sits within 10 ms of its grid line after removing one
constant offset, reported as the lead.
