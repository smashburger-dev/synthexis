# FL stock plugins probe

Do FL Studio's own plugins work as synthexis route targets, and what was missing. Results: [RESULT.md](RESULT.md).

## Files

- `device_synthexis_dev.py`: FL device script for probes only. It runs any Python it is sent inside FL, so it is never installed by `plugin/install.sh`; copy it into `Settings/Hardware/sxdev/` for a probe and delete it after.
- `fldev.py`: owns the virtual MIDI ports "sxdev", answers FL's device inquiry with `7D 53 58 44 56` so FL loads the dev script by itself, and relays code from a unix socket (`/tmp/synthexis-fldev.sock`).
- `run_stock.py` + `fl_side.py`: loads every stock plugin preset ten per throwaway project, dumps parameters (`tools/mapping/fl/device_synthexis_harvest.py`) and tests the bridge's write paths.
- `rec_test.py`: the bridge's own `rec` handler on one plugin, with FL recording and playback.

## Gotchas found on the way

- Do not name a port "synthexis ...": FL matches `supportedDevices` by prefix and gave "synthexis dev" the bridge script.
- FL may give the dev device and the bridge the same port number, so each script's replies reach both clients. `fldev.py` uses request ids above 1,000,000 and drops the rest. Even so, the synthexis plugin lost its link whenever `fldev.py serve` (re)started in the same FL session: run the dev channel and synthexis in separate FL sessions.
- FL 2026 scripts cannot write files (`os.makedirs`, `open(..., "w")` raise SystemError); dumps come back over MIDI in pages.
- FL draws its modal dialogs off screen here ("Save changes?", the trial-plugin warning on save). While one is open `general.safeToEdit()` is 0 and REC writes raise "Operation unsafe at current time"; `transport.globalTransport(FPT_No/FPT_Escape)` does not close them. `run_stock.py` therefore quits FL between batches instead of saving.
- `transport.isRecording()` reads 0 while recording.
