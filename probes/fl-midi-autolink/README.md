# FL MIDI auto-link probe

Can FL Studio set up the synthexis bridge ports by itself, so nobody opens
Options > MIDI settings? Results: [RESULT.md](RESULT.md).

## Files

- `midiprobe.swift`: opens a CoreMIDI virtual source and destination under
  any names, logs what FL sends (device inquiries, JSON frames), optionally
  answers the Universal Device Inquiry with an identity and pings a device
  script once a second.
- `device_synthexis_probe_hw.py`: probe device script matched by
  `# supportedHardwareIds=7D 53 58 01`.
- `device_synthexis_probe_name.py`: probe device script matched by
  `# supportedDevices=synthexis probe g,synthexis probe h`.
  Both say hello in `OnInit` and answer pings with their device name, port
  and `device.isAssigned()`, so a reply proves FL linked the output.
- `runs/*.log`: the runs in RESULT.md.

## Run

```bash
xcrun swiftc -O probes/fl-midi-autolink/midiprobe.swift -o /tmp/midiprobe
# FL closed: one folder per script in FL's Hardware folder
H="$HOME/Documents/Image-Line/FL Studio/Settings/Hardware"
mkdir -p "$H/synthexis probe hw" "$H/synthexis probe name"
cp probes/fl-midi-autolink/device_synthexis_probe_hw.py "$H/synthexis probe hw/"
cp probes/fl-midi-autolink/device_synthexis_probe_name.py "$H/synthexis probe name/"
# start FL, then (a port name FL has never seen):
/tmp/midiprobe --src "synthexis probe x" --dst "synthexis probe x" --id "7D 53 58 01 00 00 00 00 00" --ping --seconds 25
```

FL keeps every MIDI device name it saw in
`~/Library/Preferences/Image-Line/reg.xml` and never asks a stored name
again, so each run needs a new name.
