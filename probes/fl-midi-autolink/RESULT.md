# FL MIDI auto-link: result

FL Studio 26.1.7 on macOS, 2026-11-03. Question: which of the manual
steps for the synthexis bridge can the plugin or the installer take over?

## Manual steps before this probe

1. Copy `plugin/fl/device_synthexis_bridge.py` into
   `~/Documents/Image-Line/FL Studio/Settings/Hardware/synthexis bridge/`
   (`plugin/install.sh` did not).
2. Load synthexis; it opens the virtual source "synthexis bridge" and the
   destination "synthexis bridge return".
3. Options > MIDI settings: input "synthexis bridge": enable, controller
   type "synthexis bridge", port 77.
4. Output "synthexis bridge return": port 77.

FL keeps 3 and 4 per port name in `~/Library/Preferences/Image-Line/reg.xml`
(`HKCU/Software/Image-Line/FL Studio 26/Devices/MIDI input|output/<name>`),
so Noa did them once. A renamed port or another Mac starts over.

## How FL sets up hardware controllers

reg.xml on this Mac (FL 21 to 26) has the entries FL wrote for controllers
it set up itself (FLkey 37, Komplete Kontrol A49): input Type -3, ScriptFolder
of the vendor script, IDString = the controller's reply to the Universal
Device Inquiry, LinkedOutName = its output (FLkey: input "FLkey 37 FLkey MIDI
Out", output "FLkey 37 FLkey MIDI In"), Port 236 or 237 on input and output.
Vendor scripts carry `# supportedHardwareIds=<id prefix>` or
`# supportedDevices=<port names>`. The old "synthexis bridge" entry had
IDString "-": the plugin never answered the inquiry.

Sources: FL MIDI scripting manual (supportedDevices matches device names),
IL-Group FL-Studio-API-Stubs script metadata (supportedHardwareIds is a
prefix match on the reply minus F0 7E <ch> 06 02 and F7), FL changelog 20.9
(identification message sent when a new MIDI device is detected; linked
inputs and outputs found through the hardware ID string). Flapi, a project
with the same design, worked on macOS only once each port was a source and
a destination with one name.

## Rule (written before the runs)

Probe ports under names FL has never seen, so the saved "synthexis bridge"
entries are not involved; nothing in FL's settings is edited by hand.

- M1, FL running, no probe script: does FL send F0 7E 7F 06 01 F7 to the
  destination within 10 s of the ports appearing, with and without an
  identity reply (7D 53 58 01 00 00 00 00 00, manufacturer 7D =
  non-commercial)? Observation, no pass/fail.
- M2, probe scripts installed while FL was closed, FL restarted, nobody in
  MIDI settings: pass if the script's OnInit hello arrives and a ping gets
  its reply within 15 s of the ports appearing. Ports opened before FL
  starts and after FL is up; "X" / "X return" and one name for both.

## M1: FL running, no probe script

| Run | Ports | Identity | Inquiry after |
|---|---|---|---|
| m1a | "synthexis probe" / "... return" | no | 1.01 s |
| m1b | same names again, same FL run | yes | none |
| m1c | "synthexis probe b" / "... return" | yes | 0.93 s |
| m1d | "synthexis probe c", one name | yes | 0.96 s |

FL asks every new virtual endpoint name, also one created while FL runs. On
quit it stored IDString "7D 53 58 01 00 00 00 00 00" and LinkedOutName
("synthexis probe b return" for b, "synthexis probe c" for c) with Port -1
and Enabled 0 (no script matched yet); "synthexis probe" got IDString "-".

## M2: probe scripts installed

| Run | Ports | Matched by | Hello | Port | Result |
|---|---|---|---|---|---|
| m2i | one name, open before FL started | hardware ID | 2.2 s after FL's inquiry | 236 | pass |
| m2e | "X" / "X return", after FL was up | hardware ID | 3.1 s | 237 | pass |
| m2f | one name | hardware ID | 3.1 s | 237 | pass |
| m2g | "X" / "X return" | name only | none | | fail |
| m2h | one name | name only | 3.1 s | 237 | pass |
| m2u1 | "synthexis probe b", stored with ID, no script then | hardware ID | no inquiry | | fail |
| m2u2 | "synthexis probe", stored with "-" | hardware ID | no inquiry | | fail |

Every pass answered all pings (21 to 22 in 25 s). The hardware ID pairs
input and output whatever their names; a name-only match needs one name for
both. A name FL stored once is never asked again, also after an FL restart,
even when a matching script exists now.

## What ships

- The plugin's ports are one name both ways, "synthexis" (new, so every
  machine takes the automatic path once; the old "synthexis bridge" entries
  stay unused in reg.xml).
- The destination's callback answers the inquiry with 7D 53 58 46 4C 04 00
  00 00 (family "SX", model "FL", version = protocol 4), on the CoreMIDI
  thread (`plugin/synthexis/src/fl.rs`).
- The script declares `# supportedHardwareIds=7D 53 58 46 4C` and
  `# supportedDevices=synthexis`.
- `plugin/install.sh` copies the script into FL's Hardware folder and asks
  for an FL restart when FL is running: the script has to be there before
  FL first sees the "synthexis" ports (m2u1).
- Fallback, shown by the plugin when FL does not answer: restart FL; then
  MIDI settings, input "synthexis" with controller type "synthexis bridge",
  output "synthexis" on the same port.

## Plugin test in FL with Noa (2026-11-03, 23:15 to 23:25)

New script copied and new bundle installed while FL was closed; "synthexis"
was not in reg.xml yet. Nobody opened MIDI settings.

- First start: ports open at 0 s, FL's inquiry answered at 0.84 s, script
  answered the ping at 7.0 s; the picker followed FL focus and loaded the
  parameter names of Serum 2 (541) and Vital (772). Between 2 s and 7 s the
  plugin showed "FL does not answer", so the plugin now reports "waiting
  for FL" for the first 20 s after its ports open (`SETUP` in fl.rs).
- FL wrote on quit: input "synthexis" Type -3, ScriptFolder "synthexis
  bridge", IDString "7D 53 58 46 4C 04 00 00 00", LinkedOutName "synthexis",
  Port 236, Enabled 1; output "synthexis" Port 236.
- Second start with the same project: no inquiry (FL knows the name),
  script answered at 4.0 s, no message.

Not used: writing the setup into reg.xml from the installer (undocumented
format, per-version key, Noa's FL configuration). "Auto accept detected
controller" in MIDI settings is about the Link to controller dialog, not
about device detection.
