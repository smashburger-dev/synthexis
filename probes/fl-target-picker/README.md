# FL target picker probe

What a target picker in the synthexis UI can rely on in FL Studio, measured
through the test bridge. Results and the spec proposal: [RESULT.md](RESULT.md).

## Files

- `device_synthexis_bridge.py`: FL device script, protocol 2. Superset of
  `../fl-camera-plugin/bridge/device_synthexis_bridge.py`, so `h2_run.py`
  and `gate_run.py` keep working. Adds inventory (`mixer`, `gens`,
  `plist`), focus (`focus`, `events`, `poll`, `threads`), mixer-slot
  writes (`rec_fx`, `set_fx`, `getv`), a peak meter (`meter`), `blob` for
  the SysEx size limit, `eval` for exploration, and `"main": true` to run a
  command in `OnIdle` instead of on the CoreMIDI thread. Replies over
  60,000 bytes come back as an error instead of being sent.
- `flbridged.swift`: daemon variant (same port names as the original;
  payload limit 1 MiB, timeout 5 s, request frames of any size,
  `FLB_DEBUG=1` logs every inbound packet).
- `fl.py`: client helpers (timed calls, paging, run files).
- `inventory.py`, `focus_probe.py`, `write_probe.py`: the three probes;
  `listen.py` switches parameters 0/1 while a person listens.

## Run

```bash
xcrun swiftc -O probes/fl-target-picker/flbridged.swift -o probes/fl-target-picker/flbridged
cp probes/fl-target-picker/device_synthexis_bridge.py "$HOME/Documents/Image-Line/FL Studio/Settings/Hardware/synthexis bridge/"
probes/fl-target-picker/flbridged &
python3 probes/fl-camera-plugin/bridge/flb.py call '{"cmd":"ping"}'   # proto 2
python3 probes/fl-target-picker/inventory.py
python3 probes/fl-target-picker/focus_probe.py --manual 40
python3 probes/fl-target-picker/write_probe.py --pattern 80 --record
```

FL setup as in `../fl-camera-plugin/bridge/README.md` (input "synthexis
bridge" with controller type "synthexis bridge", port 77; output "synthexis
bridge return", port 77). FL reloads the script when the daemon restarts:
the virtual port disappears and comes back, FL re-initialises the device.

Never request a reply near 64 KiB: a 65,536-byte frame froze FL (RESULT.md,
section 1).
