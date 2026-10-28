# /// script
# requires-python = ">=3.11"
# dependencies = ["modal>=1.6", "numpy", "pillow", "pycocotools"]
# ///
"""SAM 3.1 label pipeline, one entrypoint.

  uv run tools/labels/run.py <session_dir> [--max-usd 8] [--fps 15]
      [--chunk-size 300] [--gpu H100|L40S] [--force]

Flow: extract 15 fps frames -> upload -> label chunks on GPU (SAM only,
raw instances as COCO RLE per concept on the volume) -> download raw
files -> local filtering/painting (postprocess.py) -> manifest +
contact sheet.

Caching is per (chunk, concept): a later run pays only for missing
concepts. Pause skipping: frames in no segment and >2.0 s from every
segment boundary are not labelled (skipped_pause in the manifest).

Cost guards (the S1 overrun must not repeat):
  * up-front estimate printed and refused above --max-usd
  * pilot: first 2 chunks measure the real ms/frame-concept; if the
    re-estimate exceeds --max-usd or 1.3x the up-front estimate, abort
  * rolling launch: stop spawning chunks once finished GPU seconds +
    in-flight elapsed reach --max-usd; running chunks finish, the rest
    resumes later.
"""

import argparse
import json
import random
import sys
import time
from pathlib import Path

import modal

sys.path.insert(0, str(Path(__file__).resolve().parent))
from extract import extract_session, load_session, repo_root  # noqa: E402
from postprocess import process_chunk  # noqa: E402

GPU_RATES = {"H100": 0.001097, "L40S": 0.000542}
# measured on the GPU-only worker (S1, real weights):
#   H100 5-prompt 300f chunk: 130.13 s -> 86.75 ms/fc -> $0.0952/1000fc
#   L40S 5-prompt 300f chunk: 227.09 s -> 151.39 ms/fc -> $0.0821/1000fc
#   L40S 1-prompt 300f chunks: 69.0/108.4 s -> 180-230 ms/fc incl. a
#   ~20 s per-job session+IO overhead. Estimate = prompts x prop_ms
#   + per-job overhead; the pilot guard re-estimates anyway.
EST_MS_PER_FRAME_CONCEPT = {"H100": 80.0, "L40S": 150.0}
EST_JOB_OVERHEAD_S = 20.0
VOL_FRAMES_REMOTE = "frames"
VOL_RAW_REMOTE = "raw"
PAUSE_MARGIN_S = 2.0
BASE_CONCEPTS = [
    {"concept": "person", "prompt": "person", "cls": "person"},
    {"concept": "face", "prompt": "face", "cls": "face"},
    {"concept": "eye", "prompt": "eye", "cls": "eye"},
    {"concept": "mouth", "prompt": "mouth", "cls": "mouth"},
    {"concept": "hand", "prompt": "hand", "cls": "hand"},
]
SEM_COLORS = {
    0: (0, 0, 0),
    1: (0, 220, 80),
    2: (255, 70, 70),
    3: (0, 200, 255),
    4: (255, 0, 255),
    5: (255, 210, 0),
    6: (255, 130, 0),
}


def split_for(meta: dict) -> str:
    return "heldout" if meta.get("session") == "S4" else "train"


def object_prompts_for(session_name: str) -> dict:
    cfg = json.loads((Path(__file__).parent / "objects.json").read_text())
    mapping = dict(cfg["default"])
    mapping.update(cfg.get("sessions", {}).get(session_name, {}))
    return mapping


def pause_skipped(frames, segments, cam="cam0"):
    """Index frames in no segment and >2.0s from every boundary."""
    bounds = [
        b for s in segments for b in (s[f"start_s_{cam}"], s[f"end_s_{cam}"])
    ]
    out = set()
    for f in frames:
        if f["segment_id"] is not None:
            continue
        if bounds and min(abs(f["t"] - b) for b in bounds) <= PAUSE_MARGIN_S:
            continue
        out.add(f["file"])
    return out


def objects_of_segment(s: dict, objmap: dict) -> list[dict]:
    """Object prompts for a block-E segment. labels.objects may be a
    string ('other' from the pre-fix recorder) -> fall back to the
    segment id E_<name>. Never iterate a string."""
    raw = s.get("labels", {}).get("objects")
    if isinstance(raw, list):
        names = raw
    else:
        sid = s.get("id", "")
        names = [sid[2:]] if sid.startswith("E_") else []
    return [
        {
            "concept": name,
            "prompt": objmap.get(name, name.replace("_", " ")),
            "cls": "object",
        }
        for name in names
    ]


def build_chunks(session_name, cam, frames, segments, objmap, chunk_size, skip_files):
    labelable = [f for f in frames if f["file"] not in skip_files]
    chunks = []
    for start in range(0, len(labelable), chunk_size):
        part = labelable[start : start + chunk_size]
        t_lo, t_hi = part[0]["t"], part[-1]["t"]
        prompts = list(BASE_CONCEPTS)
        for s in segments:
            if s.get("block") != "E" or s.get("bad"):
                continue
            if s[f"end_s_{cam}"] <= t_lo or s[f"start_s_{cam}"] >= t_hi:
                continue
            prompts += objects_of_segment(s, objmap)
        chunks.append(
            {
                "session": session_name,
                "cam": cam,
                "chunk": f"chunk_{start // chunk_size:04d}",
                "frames": [f["file"] for f in part],
                "prompts": prompts,
            }
        )
    return chunks


def upload_frames(vol, cam_dir: Path, session: str, cam: str):
    remote = f"{VOL_FRAMES_REMOTE}/{session}/{cam}"
    try:
        have = {fi.path for fi in vol.listdir(remote, recursive=True)}
    except Exception:
        have = set()
    todo = [
        p
        for p in sorted(cam_dir.iterdir())
        if p.is_file() and f"{remote}/{p.name}" not in have
    ]
    if todo:
        with vol.batch_upload(force=True) as batch:
            for p in todo:
                batch.put_file(str(p), f"{remote}/{p.name}")
    print(f"  upload {cam}: {len(todo)} new files (rest already present)")


def remote_raw_files(vol, session, cam) -> set:
    remote_root = f"{VOL_RAW_REMOTE}/{session}/{cam}"
    try:
        return {
            fi.path[len(remote_root) + 1 :]
            for fi in vol.listdir(remote_root, recursive=True)
        }
    except Exception:
        return set()


def missing_specs(chunks, remote_files) -> list[dict]:
    """Per chunk: prompts whose raw_<concept>.json is not cached."""
    specs = []
    for c in chunks:
        have_prefix = f"{c['chunk']}/raw_"
        have = {
            f[len(have_prefix) : -len(".json")]
            for f in remote_files
            if f.startswith(have_prefix)
        }
        missing = [p for p in c["prompts"] if p["concept"] not in have]
        if missing:
            specs.append({**c, "prompts": missing})
    return specs


def download_raw(vol, session, cam, labels_root: Path, force: bool = False):
    remote_root = f"{VOL_RAW_REMOTE}/{session}/{cam}"
    n = 0
    try:
        files = [
            fi
            for fi in vol.listdir(remote_root, recursive=True)
            if fi.path.endswith(".json")
        ]
    except Exception:
        return 0
    for fi in files:
        rel = fi.path[len(remote_root) + 1 :]
        dest = labels_root / cam / rel
        if dest.exists() and not force:
            continue
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(b"".join(vol.read_file(fi.path)))
        n += 1
    return n


# --- old-cache migration -------------------------------------------------


def migrate_old_cache(labels_root: Path, cam: str, chunks, vol, session):
    """Old format: chunk_<k>/chunk.json with filtered per-frame instances
    and stats.json (no raw_*.json). Archives those dirs, then rebuilds
    raw_<concept>.json per NEW chunking from the archived instances —
    one file per concept seen in the old data (empty instance lists are
    valid cached results and are written too; bogus single-letter
    concepts are dropped). Returns the historical GPU seconds spent."""
    cam_root = labels_root / cam
    archive = labels_root / "_pre_rewrite" / cam
    for d in sorted(cam_root.glob("chunk_*")):
        if (d / "chunk.json").exists() and not list(d.glob("raw_*.json")):
            archive.mkdir(parents=True, exist_ok=True)
            d.rename(archive / d.name)
    old_dirs = sorted(archive.glob("chunk_*")) if archive.is_dir() else []
    if not old_dirs:
        return 0.0

    marker = archive / ".migrated"
    if marker.exists():
        return json.loads(marker.read_text())["gpu_hist"]

    gpu_hist = 0.0
    frame_map = {}  # frame_name -> concept -> [inst]
    concepts_seen = set()
    for d in old_dirs:
        sj = d / "stats.json"
        if sj.exists():
            gpu_hist += json.loads(sj.read_text()).get("gpu_s", 0.0)
        cj = json.loads((d / "chunk.json").read_text())
        for fn, rec in cj.get("frames", {}).items():
            for inst in rec.get("instances", []):
                concept = inst.get("concept", "")
                if len(concept) < 2:
                    continue  # bogus single-letter prompts (o,t,h,e,r)
                concepts_seen.add(concept)
                frame_map.setdefault(fn, {}).setdefault(concept, []).append(inst)

    cls_of = {p["concept"]: p["cls"] for c in chunks for p in c["prompts"]}
    uploads = []
    n_written = 0
    for spec in chunks:
        cdir = cam_root / spec["chunk"]
        cdir.mkdir(parents=True, exist_ok=True)
        for concept in concepts_seen:
            frames_json = {
                fn: frame_map.get(fn, {}).get(concept, [])
                for fn in spec["frames"]
            }
            raw = {
                "concept": concept,
                "cls": cls_of.get(concept, concept),
                "prompt": None,
                "call_id": f"migrated-{spec['chunk']}",
                "method_gpu_s": 0.0,
                "prompt_gpu_s": 0.0,
                "migrated": True,
                "frames": frames_json,
            }
            p = cdir / f"raw_{concept}.json"
            # rewrite every run: cheap, and repairs partial migrations
            p.write_text(json.dumps(raw))
            n_written += 1
            uploads.append(
                (p, f"{VOL_RAW_REMOTE}/{session}/{cam}/{spec['chunk']}/{p.name}")
            )
    if uploads:
        with vol.batch_upload(force=True) as batch:
            for lp, rp in uploads:
                batch.put_file(str(lp), rp)
        print(
            f"  migrated {len(old_dirs)} old dirs -> {n_written} raw "
            f"files (concepts {sorted(concepts_seen)}) -> volume"
        )
    marker.write_text(json.dumps({"gpu_hist": gpu_hist}))
    return gpu_hist


# --- guarded remote labeling ----------------------------------------------


def run_jobs(app, labeler_cls, specs, max_usd, upfront_est_usd, rate):
    """Pilot 2 chunks, re-estimate, then rolling launch under budget."""
    labeler = labeler_cls()
    total_fp = sum(len(s["frames"]) * len(s["prompts"]) for s in specs)

    def fp(spec):
        return len(spec["frames"]) * len(spec["prompts"])

    results = []
    with modal.enable_output():
        with app.run():
            # --- pilot ---
            pilot, rest = specs[:2], specs[2:]
            pilot_calls = [
                (labeler.label_chunk.spawn(s), s, time.time()) for s in pilot
            ]
            spent_s = 0.0
            for call, s, t0 in pilot_calls:
                r = call.get()
                results.append(r)
                spent_s += r["gpu_s"]
                print(
                    f"  pilot {s['cam']}/{s['chunk']} {r['gpu_s']}s "
                    f"{r['ms_per_frame_concept']} ms/fc "
                    f"(spend ${spent_s * rate:.3f})",
                    flush=True,
                )
            # mean over the pilots: a median of 2 is just the max and
            # fires the guard on a single slow outlier chunk
            measured_ms = sum(r["ms_per_frame_concept"] for r in results) / len(
                results
            )
            new_est = spent_s * rate + (
                (total_fp - sum(fp(s) for s in pilot))
                * measured_ms / 1000 * rate
            )
            print(
                f"  pilot measured {measured_ms} ms/fc -> re-estimate "
                f"${new_est:.2f} (up-front ${upfront_est_usd:.2f})",
                flush=True,
            )
            if new_est > max_usd:
                raise SystemExit(
                    f"ABORT: pilot re-estimate ${new_est:.2f} > --max-usd "
                    f"{max_usd}. Cached chunks stay usable; rerun later."
                )
            if new_est > 1.3 * upfront_est_usd:
                raise SystemExit(
                    f"ABORT: pilot re-estimate ${new_est:.2f} > 1.3x the "
                    f"up-front estimate ${upfront_est_usd:.2f}. Investigate "
                    f"before retrying."
                )
            # --- rolling launch under budget ---
            in_flight = []  # (call, spec, t0)
            queue = list(rest)
            while queue or in_flight:
                still = []
                for call, s, t0 in in_flight:
                    try:
                        r = call.get(timeout=0)
                        results.append(r)
                        spent_s += r["gpu_s"]
                    except (TimeoutError, modal.exception.TimeoutError):
                        still.append((call, s, t0))
                in_flight = still
                in_flight_usd = sum(
                    (time.time() - t0) * rate for _, _, t0 in in_flight
                )
                spend_usd = spent_s * rate + in_flight_usd
                while queue and len(in_flight) < 4 and spend_usd < max_usd:
                    s = queue.pop(0)
                    in_flight.append(
                        (labeler.label_chunk.spawn(s), s, time.time())
                    )
                if in_flight or queue:
                    print(
                        f"  spend ${spend_usd:.2f}/${max_usd} "
                        f"| in-flight {len(in_flight)} queued {len(queue)}",
                        flush=True,
                    )
                if in_flight:
                    time.sleep(1.0)
                elif queue:
                    print(
                        f"  BUDGET: stopping launches at ${spend_usd:.2f}; "
                        f"{len(queue)} chunks left for a later run"
                    )
                    break
    leftover = len(queue)
    return results, leftover


def contact_sheet(frames, labels_root, frames_root, segments, cam, out_path):
    import numpy as np
    from PIL import Image, ImageDraw, ImageFont

    seg_by_id = {s["id"]: s for s in segments}
    strata = {}
    for f in frames:
        sid = f["segment_id"]
        block = seg_by_id[sid]["block"] if sid in seg_by_id else "gap"
        strata.setdefault(block, []).append(f)
    rng = random.Random(0)
    picked = []
    total = sum(len(v) for v in strata.values())
    for block, fs in sorted(strata.items()):
        k = max(1, round(30 * len(fs) / total))
        picked += rng.sample(fs, min(k, len(fs)))
    rng.shuffle(picked)
    picked = picked[:30]

    stem_chunk = {}
    for cdir in sorted((labels_root / cam).glob("chunk_*")):
        for png in cdir.glob("*.png"):
            stem_chunk[png.stem] = cdir

    cell_w, cell_h = 400, 225
    cols = 6
    rows = (len(picked) + cols - 1) // cols
    sheet = Image.new("RGB", (cols * cell_w, rows * cell_h), (14, 14, 18))
    try:
        font = ImageFont.load_default(size=13)
    except TypeError:
        font = ImageFont.load_default()
    for i, f in enumerate(picked):
        stem = Path(f["file"]).stem
        cdir = stem_chunk.get(stem)
        frame_img = Image.open(frames_root / cam / f["file"]).convert("RGB")
        base = np.asarray(frame_img).astype(np.float32)
        if cdir is not None:
            sem = np.asarray(Image.open(cdir / f"{stem}.png"))
            if sem.shape[:2] == base.shape[:2]:
                mask = sem > 0
                lut = np.zeros((256, 3), np.float32)
                for cid, rgb in SEM_COLORS.items():
                    lut[cid] = rgb
                colored = lut[sem]
                base[mask] = 0.45 * base[mask] + 0.55 * colored[mask]
        cell = Image.fromarray(base.astype(np.uint8)).resize((cell_w, cell_h))
        d = ImageDraw.Draw(cell)
        label_txt = f["segment_id"] or "gap"
        if f["labels"]:
            label_txt += " " + json.dumps(f["labels"])[:40]
        d.rectangle([0, cell_h - 18, cell_w, cell_h], fill=(0, 0, 0))
        d.text(
            (4, cell_h - 16), f"{f['t']:.2f}s {label_txt}",
            fill=(230, 230, 230), font=font,
        )
        sheet.paste(cell, ((i % cols) * cell_w, (i // cols) * cell_h))
    sheet.save(out_path, quality=88)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("session_dir", type=Path)
    ap.add_argument("--session-name")
    ap.add_argument("--max-usd", type=float, default=8.0)
    ap.add_argument("--fps", type=int, default=15)
    ap.add_argument("--chunk-size", type=int, default=300)
    ap.add_argument("--gpu", choices=sorted(GPU_RATES), default="L40S")
    ap.add_argument("--force", action="store_true")
    args = ap.parse_args()

    rate = GPU_RATES[args.gpu]
    root = repo_root()
    session_dir = args.session_dir.resolve()
    meta, segments = load_session(session_dir)
    session = args.session_name or session_dir.name
    split = split_for(meta)
    objmap = object_prompts_for(session)
    frames_root = root / "data" / "frames" / session
    labels_root = root / "data" / "labels" / session

    print(f"[1/8] extract frames ({args.fps} fps) -> {frames_root}")
    extract_stats = extract_session(
        session_dir, root / "data" / "frames", args.fps, session
    )

    chunks = []
    skipped_pause = {}
    for cam in extract_stats["cams"]:
        index = json.loads(
            (frames_root / cam / "frames_index.json").read_text()
        )
        skip_files = pause_skipped(index["frames"], segments, cam)
        skipped_pause[cam] = len(skip_files)
        chunks += build_chunks(
            session, cam, index["frames"], segments, objmap,
            args.chunk_size, skip_files,
        )
    print(
        f"  {len(chunks)} chunk(s); skipped_pause "
        f"{sum(skipped_pause.values())} frames"
    )

    vol = modal.Volume.from_name(
        "synthexis-recordings", create_if_missing=True
    )
    gpu_hist = {}
    for cam in extract_stats["cams"]:
        gpu_hist[cam] = migrate_old_cache(
            labels_root, cam, [c for c in chunks if c["cam"] == cam],
            vol, session,
        )
        if gpu_hist[cam]:
            print(
                f"  historical GPU spend from old run: "
                f"{gpu_hist[cam]:.0f}s"
            )

    # per-concept cache check
    todo = []
    for cam in extract_stats["cams"]:
        remote_files = remote_raw_files(vol, session, cam)
        cam_chunks = [c for c in chunks if c["cam"] == cam]
        for c in cam_chunks:
            c["out_dir"] = (
                f"/vol/{VOL_RAW_REMOTE}/{session}/{cam}/{c['chunk']}"
            )
        missing = missing_specs(cam_chunks, remote_files)
        if args.force:
            missing = cam_chunks
        todo += missing
        print(
            f"  {cam}: {len(cam_chunks)} chunks, "
            f"{len(missing)} need work"
        )

    n_fp = sum(len(s["frames"]) * len(s["prompts"]) for s in todo)
    est = (
        n_fp * EST_MS_PER_FRAME_CONCEPT[args.gpu] / 1000
        + len(todo) * EST_JOB_OVERHEAD_S
    ) * rate
    print(
        f"[4/8] {len(todo)} chunk-job(s), {n_fp} frame-prompts, "
        f"est ${est:.3f} (cap ${args.max_usd})"
    )
    if est > args.max_usd:
        raise SystemExit(
            f"estimate ${est:.2f} exceeds --max-usd {args.max_usd}; refusing"
        )

    print("[5/8] upload frames")
    for cam in extract_stats["cams"]:
        upload_frames(vol, frames_root / cam, session, cam)

    import sam_label

    results = []
    if todo:
        print(f"[6/8] label {len(todo)} chunk-job(s) on {args.gpu}")
        results, leftover = run_jobs(
            sam_label.app, sam_label.LABELERS[args.gpu], todo,
            args.max_usd, est, rate,
        )
        if leftover:
            print(f"  {leftover} chunk-jobs deferred (budget)")
    else:
        print("[6/8] all concepts cached")

    print("[7/8] download raw labels + local processing")
    for cam in extract_stats["cams"]:
        n = download_raw(vol, session, cam, labels_root, args.force)
        print(f"  {cam}: {n} new raw files")
        cam_chunks = [c for c in chunks if c["cam"] == cam]
        for c in cam_chunks:
            cdir = labels_root / cam / c["chunk"]
            if cdir.is_dir():
                process_chunk(cdir, c["frames"], frames_root / cam)

    print("[8/8] manifest + contact sheet")
    labels_root.mkdir(parents=True, exist_ok=True)
    manifest = {
        "session": session,
        "split": split,
        "fps": args.fps,
        "gpu": args.gpu,
    }
    total_gpu_s = 0.0
    total_hist_s = 0.0
    fresh_ms = [r["ms_per_frame_concept"] for r in results]
    for cam in extract_stats["cams"]:
        index = json.loads(
            (frames_root / cam / "frames_index.json").read_text()
        )
        cam_chunks = [c for c in chunks if c["cam"] == cam]
        det, face_removed, hand_dropped = {}, 0, 0
        gpu_s = 0.0
        n_processed = 0
        for cdir in sorted((labels_root / cam).glob("chunk_*")):
            sj = cdir / "stats.json"
            if not sj.exists():
                continue
            r = json.loads(sj.read_text())
            n_processed += 1
            gpu_s += r["gpu_s"]
            face_removed += r["face_filter_removed"]
            hand_dropped += r.get("hand_filter_dropped", 0)
            for concept, d in r["det_counts"].items():
                a = det.setdefault(
                    concept, {"frames_with_det": 0, "instances": 0}
                )
                a["frames_with_det"] += d["frames_with_det"]
                a["instances"] += d["instances"]
        manifest[cam] = {
            "chunks": len(cam_chunks),
            "chunks_processed": n_processed,
            "frames_total": index["stats"]["extracted"],
            "frames_dropped_bad": index["stats"]["dropped_bad"],
            "frames_kept": index["stats"]["kept"],
            "frames_label_ok": index["stats"]["label_ok"],
            "skipped_pause": skipped_pause.get(cam, 0),
            "detections": det,
            "face_filter_removed": face_removed,
            "hand_filter_dropped": hand_dropped,
            "gpu_s": round(gpu_s, 2),
            # historical seconds were billed on H100
            "gpu_s_previous_runs": round(gpu_hist.get(cam, 0.0), 2),
            "cost_usd": round(
                gpu_s * rate + gpu_hist.get(cam, 0.0) * GPU_RATES["H100"], 4
            ),
        }
        total_gpu_s += gpu_s
        total_hist_s += gpu_hist.get(cam, 0.0)
        sheet_path = labels_root / "contact_sheet.jpg"
        contact_sheet(
            index["frames"], labels_root, frames_root,
            segments, cam, sheet_path,
        )
        print(
            f"  {cam}: {index['stats']['kept']} kept, "
            f"{skipped_pause.get(cam, 0)} skipped_pause, "
            f"{n_processed}/{len(cam_chunks)} chunks processed, "
            f"sheet -> {sheet_path}"
        )
    manifest["cost_usd_total"] = round(
        total_gpu_s * rate + total_hist_s * GPU_RATES["H100"], 4
    )
    manifest["estimated_cost_usd"] = round(est, 4)
    if fresh_ms:
        manifest["ms_per_frame_concept_measured"] = round(
            sorted(fresh_ms)[len(fresh_ms) // 2], 2
        )
    (labels_root / "manifest.json").write_text(json.dumps(manifest, indent=1))
    total_usd = total_gpu_s * rate + total_hist_s * GPU_RATES["H100"]
    print(
        f"SUMMARY {session} split={split} gpu_s={total_gpu_s:.1f} "
        f"hist_s={total_hist_s:.0f} "
        f"cost=${total_usd:.3f} est=${est:.3f} "
        f"manifest={labels_root / 'manifest.json'}"
    )


if __name__ == "__main__":
    main()
