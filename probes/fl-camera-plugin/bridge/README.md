# synthexis FL bridge

Scripted control of FL Studio for the H2 automation tests (records
parameter automation from a plugin whose editor is closed).

## One-time FL setup (Noa)

1. Quit and restart FL Studio 2026.
2. Options -> MIDI settings (F10):
   - Input: **synthexis bridge** -> Enable; Controller type:
     **synthexis bridge** (the device script); Port: **77**.
   - Output: **synthexis bridge return** -> Port: **77**.
3. Right-click the Record button -> tick **Automation**.
4. Use an empty project with **synthexis probe (VST3)** in the Channel
   Rack; add the CLAP build as a second channel for the CLAP condition.

The device script is already installed at
`~/Documents/Image-Line/FL Studio/Settings/Hardware/synthexis bridge/`.
FL picks it up on restart / when the MIDI settings rescan.

## Running the H2 tests (lead)

```
xcrun swiftc -O bridge/flbridged.swift -o bridge/flbridged   # once
bridge/flbridged &                                          # daemon
python3 bridge/h2_run.py                                    # VST3 channel
python3 bridge/h2_run.py --channel-name '<clap channel>'    # CLAP channel
```

Quick check that the FL side is wired: `python3 bridge/flb.py call
'{"cmd":"ping"}'` should return `{"id":1,"ok":true,"proto":1}`.

Results land in `bridge/runs/<ts>-<cond>.json` plus a one-line summary
table per condition (verdict: `recorded` | `flat` | `abort:*` | `error:*`).

## Files

- `device_synthexis_bridge.py` — FL device script (SysEx JSON executor)
- `flbridged.swift` — CoreMIDI daemon (virtual source + return, unix
  socket `/tmp/synthexis-flbridge.sock`, log
  `~/Library/Logs/synthexis-flbridge.log`)
- `flb.py` — socket client (`flb.py call '<json>'`, `flb.call()`)
- `sysexcodec.py` — shared F0 7D <json> F7 framing
- `test_protocol.py` — codec round-trip/guard tests
- `h2_run.py` — the experiment runner
