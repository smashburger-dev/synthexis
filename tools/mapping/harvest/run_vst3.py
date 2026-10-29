# /// script
# requires-python = ">=3.11"
# ///
"""VST3 parameter harvest runner: one subprocess per .vst3 bundle,
60 s timeout, 3 workers, resumable. Writes
data/mapping/harvest/vst3/{plugins,params}.jsonl (+ summary.json).

  uv run tools/mapping/harvest/run_vst3.py [--match STR]
      [--retry-failed] [--workers 3]
"""

import argparse
import json
import os
import signal
import statistics
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
BIN = ROOT / "data" / "mapping" / "bin" / "vst3_harvest"
OUT_DIR = ROOT / "data" / "mapping" / "harvest" / "vst3"
PLUGINS_JSONL = OUT_DIR / "plugins.jsonl"
PARAMS_JSONL = OUT_DIR / "params.jsonl"
SCAN_DIRS = [Path("/Library/Audio/Plug-Ins/VST3"),
             Path("~/Library/Audio/Plug-Ins/VST3").expanduser()]


def scan() -> list[Path]:
    out = []
    for d in SCAN_DIRS:
        if d.is_dir():
            out += sorted(d.rglob("*.vst3"))
    return [p for p in out
            if "synthexis" not in p.name.lower()]


def run_one(bundle: Path, timeout: int, tmpdir: Path) -> dict:
    key = str(bundle).replace("/", "_")
    out_path = tmpdir / f"{key}-{os.getpid()}-{threading.get_ident()}.json"
    cmd = [str(BIN), "--dump", str(bundle), "--out", str(out_path)]
    t0 = time.monotonic()
    rec = {"bundle": str(bundle), "name": bundle.stem,
           "key": f"vst3/{bundle.stem}", "plugin_id": "",
           "status": "error", "n_params": 0, "params": [],
           "format": "vst3"}
    try:
        p = subprocess.Popen(cmd, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, text=True,
                             start_new_session=True)
        try:
            out, err = p.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(os.getpgid(p.pid), signal.SIGKILL)
            except ProcessLookupError:
                pass
            p.wait()
            rec.update(status="timeout",
                       error=f"killed after {timeout}s")
            return rec
        noise = ((err or "")[-160:] + " | " + (out or "")[-160:]
                 ).strip(" |")
        rec["log_tail"] = noise[-300:]
        doc = None
        if out_path.exists():
            try:
                doc = json.loads(out_path.read_text())
            except (json.JSONDecodeError, OSError):
                doc = None
        if doc is None:
            rec["error"] = f"exit {p.returncode}: {noise}" \
                if noise else f"exit {p.returncode}"
            return rec
        classes = [c for c in doc.get("classes", [])
                   if c.get("params") is not None]
        rec["classes"] = classes
        params = []
        for ci, c in enumerate(classes):
            for prm in c.get("params") or []:
                prm["class_name"] = c.get("name")
                params.append(prm)
        rec["params"] = params
        rec["n_params"] = len(params)
        c0 = classes[0] if classes else {}
        rec["manufacturer_name"] = c0.get("vendor") or "?"
        rec["name"] = c0.get("name") or bundle.stem
        rec["type"] = "vst3"
        rec["subtype"] = "vst3"
        rec["manufacturer"] = c0.get("vendor") or "?"
        rec["type_name"] = "VST3"
        rec["version"] = 0
        rec["tags"] = []
        rec["plugin_id"] = (f"{rec['manufacturer_name']}/"
                            f"{rec['name']}")
        rec["status"] = ("ok" if params
                         else "error" if not classes else "no_params")
        if any(c.get("error") for c in classes):
            rec["error"] = "; ".join(
                c["error"] for c in classes if c.get("error"))[:300]
            rec["status"] = "error" if not params else "ok"
        return rec
    except Exception as e:
        rec["error"] = str(e)[:300]
        return rec
    finally:
        out_path.unlink(missing_ok=True)
        rec["load_ms"] = round((time.monotonic() - t0) * 1000)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--match", default="")
    ap.add_argument("--retry-failed", action="store_true")
    ap.add_argument("--workers", type=int, default=3)
    ap.add_argument("--timeout", type=int, default=60)
    args = ap.parse_args()

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    bundles = scan()
    done = {}
    if PLUGINS_JSONL.exists():
        for line in PLUGINS_JSONL.read_text().splitlines():
            if line.strip():
                r = json.loads(line)
                done[r["key"]] = r
    todo = []
    for b in bundles:
        key = f"vst3/{b.stem}"
        prev = done.get(key)
        if prev is None or (args.retry_failed
                            and prev["status"] in ("error", "timeout")):
            todo.append(b)
    if args.match:
        todo = [b for b in todo
                if args.match.lower() in b.name.lower()]
    print(f"{len(bundles)} bundles, {len(done)} recorded, "
          f"{len(todo)} to harvest")

    t0 = time.monotonic()
    n_done = 0
    tmpdir = OUT_DIR / "tmp"
    tmpdir.mkdir(exist_ok=True)
    pf = PLUGINS_JSONL.open("a")
    af = PARAMS_JSONL.open("a")
    try:
        with ThreadPoolExecutor(max_workers=args.workers) as ex:
            futs = {ex.submit(run_one, b, args.timeout, tmpdir): b
                    for b in todo}
            for fut in as_completed(futs):
                rec = fut.result()
                n_done += 1
                params = rec.pop("params", [])
                classes = rec.pop("classes", [])
                rec["n_classes"] = len(classes)
                pf.write(json.dumps(rec) + "\n")
                pf.flush()
                for prm in params:
                    prm["plugin_id"] = rec["plugin_id"]
                    af.write(json.dumps(prm) + "\n")
                af.flush()
                if n_done % 10 == 0 or n_done == len(todo):
                    print(f"[{n_done}/{len(todo)}] {rec['status']} "
                          f"{rec['plugin_id']} "
                          f"({rec['n_params']} params, "
                          f"{rec['load_ms']} ms)")
    finally:
        pf.close()
        af.close()

    last = {}
    for line in PLUGINS_JSONL.read_text().splitlines():
        if line.strip():
            r = json.loads(line)
            last[r["key"]] = r
    counts = {}
    total_p = 0
    per_plugin = []
    mfr = {}
    for r in last.values():
        counts[r["status"]] = counts.get(r["status"], 0) + 1
        total_p += r["n_params"]
        per_plugin.append(r["n_params"])
        m = r.get("manufacturer_name", "?")
        mfr[m] = mfr.get(m, 0) + 1
    summary = {
        "components": len(last), "by_status": counts,
        "total_params": total_p,
        "params_median": (statistics.median(per_plugin)
                          if per_plugin else 0),
        "params_max": max(per_plugin) if per_plugin else 0,
        "top_manufacturers": sorted(
            mfr.items(), key=lambda x: -x[1])[:20],
        "wall_s": round(time.monotonic() - t0),
    }
    (OUT_DIR / "summary.json").write_text(
        json.dumps(summary, indent=1))
    print("summary:", summary)
    print(f"wall {summary['wall_s']}s -> {OUT_DIR}")


if __name__ == "__main__":
    main()
