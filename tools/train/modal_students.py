"""Student training on a Modal L4, A10G as fallback (train.students, train.setup.gpu).

One-time setup (shards go to the volume once, never into the image):
    modal volume create synthexis-students
    modal volume put synthexis-students data/packed/seg  /packed/seg
    modal volume put synthexis-students data/packed/hand /packed/hand
    modal volume put synthexis-students data/packed/face /packed/face
    modal deploy tools/train/modal_students.py
Spawn with a cap (refused when the up-front estimate exceeds it), wait, fetch:
    python tools/train/modal_students.py spawn <name> --max-usd 0.8 --est-min 30 -- --student seg --epochs 12
    python tools/train/modal_students.py wait <call id> ...
    modal volume get synthexis-students /runs/<name> data/train/runs/

Guards (as tools/labels/run.py): the up-front estimate (--est-min x rate) is
refused above --max-usd; train.py re-estimates after the first epoch (pilot)
and stops above the cap; train.py checks elapsed cost every step and stops at
the cap. The function timeout is the last stop. The rate counts the L4, the
CPU cores and the memory asked for below (Modal list prices, 2026-11).

Cost rules: the container copies its shards to local disk once, trains, and
exits; the volume is committed at the end, also on failure.
"""
import argparse
import os
import sys
import time
from pathlib import Path

import modal

HERE = Path(__file__).resolve().parent
APP = os.environ.get("SYNTHEXIS_MODAL_APP", "synthexis-students")
# L4 first; A10G when L4 has no capacity (L4 was out for 12 min on 11-02, and
# the first spawns here sat pending). The guards price the dearer A10G.
GPU, CPU, MEM_MB = ["L4", "A10G"], 4.0, 12288
# $/s: L4 0.000222, A10G 0.000306, CPU 0.0000131 per core, memory 0.00000222 per GiB
USD_PER_HOUR = round(3600 * (0.000306 + CPU * 0.0000131 + MEM_MB / 1024 * 0.00000222), 3)
TIMEOUT_S = 2 * 3600

app = modal.App(APP)
vol = modal.Volume.from_name("synthexis-students", create_if_missing=True)
image = (
    modal.Image.debian_slim(python_version="3.12")
    .pip_install("torch==2.7.0", "timm==1.0.15", "numpy==2.2.6", "pillow==11.2.1",
                 "huggingface_hub==0.36.2", "safetensors==0.6.2")
    .env({"PYTHONUNBUFFERED": "1", "HF_HUB_DISABLE_PROGRESS_BARS": "1"})
    .add_local_file(str(HERE / "students.py"), "/app/tools/train/students.py")
    .add_local_file(str(HERE / "train.py"), "/app/tools/train/train.py")
)


@app.function(image=image, gpu=GPU, cpu=CPU, memory=MEM_MB, timeout=TIMEOUT_S, volumes={"/vol": vol})
def train(name: str, args: list[str], max_usd: float) -> dict:
    import json
    import shutil
    import subprocess
    t0 = time.time()
    vol.reload()
    student = args[args.index("--student") + 1]
    local = Path("/tmp/packed")
    shutil.copytree(f"/vol/packed/{student}", local / student)
    print(f"copied {student} shards in {time.time() - t0:.0f} s", flush=True)
    # the copy and the container start count against the cap too
    spent = (time.time() - t0) / 3600 * USD_PER_HOUR
    try:
        subprocess.run(["python", "/app/tools/train/train.py", "--name", name, "--device", "cuda",
                        "--data", str(local), "--runs", "/vol/runs", "--workers", "7",
                        "--usd-per-hour", str(USD_PER_HOUR), "--max-usd", str(round(max_usd - spent, 4)),
                        *args], check=True)
    finally:
        vol.commit()
    summary = json.loads(Path(f"/vol/runs/{name}/summary.json").read_text())
    summary["container_s"] = round(time.time() - t0, 1)
    summary["usd_estimate"] = round(summary["container_s"] / 3600 * USD_PER_HOUR, 4)
    return summary


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    sp = sub.add_parser("spawn")
    sp.add_argument("name")
    sp.add_argument("--max-usd", type=float, required=True)
    sp.add_argument("--est-min", type=float, required=True, help="up-front estimate of container minutes")
    wp = sub.add_parser("wait")
    wp.add_argument("calls", nargs="+")
    argv = sys.argv[1:]
    cut = argv.index("--") if "--" in argv else len(argv)
    a = ap.parse_args(argv[:cut])
    if a.cmd == "spawn":
        targs = argv[cut + 1:]
        est = a.est_min / 60 * USD_PER_HOUR
        print(f"{a.name}: up-front estimate {a.est_min} min x {USD_PER_HOUR} $/h = ${est:.3f}, cap ${a.max_usd}")
        if est > a.max_usd:
            sys.exit(f"REFUSED: estimate ${est:.3f} > --max-usd {a.max_usd}")
        fn = modal.Function.from_name(APP, "train")
        call = fn.spawn(a.name, targs, a.max_usd)
        print(f"spawned {a.name}: {call.object_id}")
    else:
        for cid in a.calls:
            print(modal.FunctionCall.from_id(cid).get())


if __name__ == "__main__":
    main()
