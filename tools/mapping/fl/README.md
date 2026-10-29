# FL Studio parameter harvest

Dumps every plugin (channel rack + mixer slots) of the open FL project
with all parameter names, values and display strings to
`~/Documents/synthexis-harvest/<timestamp>.json`.

## Steps for Noa

1. Copy `device_synthexis_harvest.py` into FL's script folder:
   `~/Documents/Image-Line/FL Studio/Settings/Hardware/synthexis harvest/`
   (create the folder).
2. Open a **throwaway project** and load every FL Studio stock plugin
   you want harvested (channel rack generators + mixer effect slots).
   The harvester briefly changes every parameter to sample its display
   strings — do not use a real project.
3. Options > MIDI settings: select your input, set controller type to
   **synthexis harvest**.
4. Wait. The dump processes one plugin per idle tick; when it is done
   the FL script log prints `synthexis harvest -> <path>`.
   Pressing any MIDI note retriggers a dump.
5. Convert with `uv run tools/mapping/fl/import_fl_dump.py`
   (writes `data/mapping/harvest/fl/{plugins,params}.jsonl`).

Params 4096+ (per-CC/aftertouch ranges) are skipped. Plugins whose
parameter API is unreachable from the script land with `n_params: 0`.
