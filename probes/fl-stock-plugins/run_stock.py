"""Loads every FL stock plugin into throwaway projects, dumps its parameters
(tools/mapping/fl/device_synthexis_harvest.py, fetched page by page: FL
2026 scripts cannot write files) and tests the bridge's write
paths on every parameter (fl_side.py). Needs FL running with the dev script
and `fldev.py serve` (README.md).

  python3 probes/fl-stock-plugins/run_stock.py [--only "Fruity Filter,3x Osc"]

Each batch is a copy of FL's Empty.flp in runs/flp/: generators and
effects ten per batch (FL puts an opened effect preset into the
next free master slot). Results go to runs/writes.jsonl, one line per
plugin, and the dumps to ~/Documents/synthexis-harvest/.
"""

import argparse
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
from fldev import call  # noqa: E402

FL = "FL Studio 2026"
EMPTY = Path(f"/Applications/{FL}.app/Contents/Resources/FL/Data/System/Empty.flp")
DB = Path.home() / "Documents/Image-Line/FL Studio/Presets/Plugin database/Installed"
RUNS = HERE / "runs"
HARVEST = ROOT / "tools/mapping/fl/device_synthexis_harvest.py"

# No parameters to route, or a window, file or device they need first.
SKIP = {
    "Control Surface", "Edison", "FL Studio Mobile Rack FX", "Fruity Big Clock",
    "Fruity HTML NoteBook", "Fruity NoteBook", "Fruity NoteBook 2", "Fruity Wrapper",
    "Razer Chroma", "Wave Candy", "ZGameEditor Visualizer", "Fruity Spectroman",
    "Fruity dB meter", "Tuner", "Dashboard", "FL Studio Mobile Rack", "FL Studio Mobile",
    "Fruity Video Player", "MIDI Out", "ReWired", "Transmitter", "Newtime", "Newtone",
    "VFX Color Mapper", "VFX Envelope", "VFX Key Mapper", "VFX Keyboard Splitter",
    "VFX Level Scaler", "VFX Script", "VFX Sequencer",
}


UNSAFE = []  # (code, attempt) for every "Operation unsafe at current time"


def must(code, main=False):
    for attempt in range(20):
        r = call(code, main)
        if "unsafe" not in r.get("error", ""):
            break
        UNSAFE.append((code[:60], attempt))
        time.sleep(0.5)
    if "error" in r:
        raise RuntimeError(f"{code[:60]}: {r['error']}")
    return r.get("r")


def wait_up(tries=60):
    for _ in range(tries):
        try:
            return must("general.getVersion()")
        except Exception:
            time.sleep(1)
    raise RuntimeError("FL dev script not answering")


def open_in_fl(path):
    for _ in range(20):  # -600 while the previous FL is still quitting
        if subprocess.run(["open", "-g", "-a", FL, str(path)]).returncode == 0:
            return
        time.sleep(1)
    raise RuntimeError(f"open {path} failed")


def restart_fl(flp):
    subprocess.run(["pkill", "-TERM", "-x", "OsxFL"])
    for _ in range(20):
        if subprocess.run(["pgrep", "-x", "OsxFL"], capture_output=True).returncode:
            break
        time.sleep(0.5)
    else:
        subprocess.run(["pkill", "-KILL", "-x", "OsxFL"])
        time.sleep(1)
    open_in_fl(flp)
    time.sleep(8)
    wait_up(120)


def batches(only):
    gens = sorted(p for p in (DB / "Generators/Fruity").glob("*.fst") if p.stem not in SKIP)
    fx = sorted(p for p in (DB / "Effects/Fruity").glob("*.fst") if p.stem not in SKIP)
    if only:
        gens = [p for p in gens if p.stem in only]
        fx = [p for p in fx if p.stem in only]
    out = [gens[i:i + 10] for i in range(0, len(gens), 10)]
    out += [fx[i:i + 10] for i in range(0, len(fx), 10)]
    return out


PAGE = 25  # parameters per reply: 5 value strings each stay far below 60 KB


def dump_plugin(kind, index, slot):
    """The harvest record of one plugin, fetched a page at a time."""
    item = ("chan", index) if kind == "gen" else ("mix", index, slot)
    rec, start = None, 0
    while True:
        page = must(f"H['_dump_one']({item!r}, {start}, {start + PAGE})")
        if rec is None:
            rec = page
        else:
            rec["params"] += page["params"]
        start += PAGE
        if start >= min(page["n_params"], 4096):
            return rec


WRITE_N = 16  # parameters written per plugin: a full sweep once emptied the project (BassDrum)


def run_batch(n, fsts, log, dumped, save):
    flp = RUNS / "flp" / f"batch{n}.flp"
    flp.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy(EMPTY, flp)
    # FL is quit (not asked) between batches: saving or leaving a changed
    # project opens a dialog ("Save changes?", the trial-plugin warning)
    # that FL draws off screen here and that blocks REC writes
    # (general.safeToEdit() 0) until someone answers it
    restart_fl(flp)
    for f in fsts:
        open_in_fl(f)
        time.sleep(2.5)
    wait_up()
    if not must("general.safeToEdit()"):
        raise RuntimeError("FL is not safe to edit (a dialog open?)")
    must(open(HERE / "fl_side.py").read())
    must(f"H = {{}}; exec(open({str(HARVEST)!r}).read(), H)")
    for kind, index, slot, _, name in must("places()"):
        key = [kind, index, slot]
        row = {"plugin": name, "kind": kind, "index": index, "slot": slot}
        try:
            rec = dump_plugin(kind, index, slot)
            dumped.append(rec)
            save()
            row["params"] = must(f"write_test_start({key!r}, {WRITE_N})")
            time.sleep(0.6)
            res = must(f"write_test_finish({key!r})")
            row.update(moved=res["moved"], stuck=res["stuck"])
            print(f"  {name}: {len(rec['params'])} named, wrote {row['params']}, "
                  f"{res['moved']} moved, {len(res['stuck'])} stuck", flush=True)
        except Exception as e:
            row["error"] = str(e)
            print(f"  {name}: {e}", flush=True)
            wait_up()
        log.write(json.dumps(row) + "\n")
        log.flush()
    loaded = {x[4] for x in must("places()")}
    missing = [f.stem for f in fsts if f.stem not in loaded]
    if missing:
        print(f"  not loaded under their file name: {missing}", flush=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", default="")
    ap.add_argument("--start", type=int, default=0)
    a = ap.parse_args()
    only = {s.strip() for s in a.only.split(",") if s.strip()}
    RUNS.mkdir(exist_ok=True)
    dumped = []
    ts = time.strftime("%Y%m%d-%H%M%S")
    out = Path.home() / "Documents/synthexis-harvest" / f"{ts}.json"
    out.parent.mkdir(parents=True, exist_ok=True)

    def save():
        out.write_text(json.dumps({"ts": ts, "plugins": dumped}))
    with open(RUNS / "writes.jsonl", "a") as log:
        for n, fsts in enumerate(batches(only)):
            if n >= a.start:
                run_batch(n, fsts, log, dumped, save)
    print(f"dump: {len(dumped)} plugins -> {out}")
    print(f"unsafe retries: {len(UNSAFE)} {UNSAFE[:10]}")


if __name__ == "__main__":
    main()
