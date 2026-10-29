# /// script
# requires-python = ">=3.11"
# dependencies = ["modal>=1.6", "numpy", "pillow", "pycocotools"]
# ///
"""Image-mode pilot vs the existing S1 video-mode labels: cost + agreement.

  uv run tools/labels/compare_modes.py data/recordings/2026-10-27-S1 \
      --max-usd 0.75 [--variants seq,grouped] [--dry-run]

Picks the chunk with the most A_both_* frames (P1) and the chunk with the
most block-F frames (P2), labels them with the image-mode worker (P1 in
every requested variant, P2 in seq only), then runs prompt-sensitivity
tests on every 5th frame of E_mug / E_pen (seq). Raw output goes to
/vol/pilot/<session>/<cam>/<name>/ and data/labels/_pilot/<session>/<name>/
— never into the real labels dirs or the raw/ volume prefix. Afterwards it
post-processes locally (same filters + semantic PNGs as the real pipeline)
and compares against the video labels of the same chunks: per-class IoU,
detection counts, grouped-vs-seq IoU, $/1000 frame-concepts, a
disagreement sheet for the worst hand-IoU frames and a prompt-test sheet.

Cost guards: up-front estimate refused above --max-usd, re-estimate after
the first measured job, and launches stop once finished GPU seconds reach
the cap. --dry-run prints picks and the estimate; spends nothing.

--reverse flips the check: picks become the chunks with the most block-C
(C1) and block-H (H1) frames, jobs run the VIDEO worker (LabelerL40S,
5 base prompts) into <chunk>__video pilot dirs, and the comparison reads
reference = the session's image-mode labels, candidate = the video pilot.
Also writes review_<chunk>.jpg (15 frames, image|video overlays) and
review_face_<chunk>.jpg (same frames at the face union box +20 px, 2x)
per pick. --analyze-only works in both directions.
"""

import argparse
import json
import sys
import time
from pathlib import Path

import modal

sys.path.insert(0, str(Path(__file__).resolve().parent))
from extract import load_session, repo_root  # noqa: E402
from postprocess import process_chunk  # noqa: E402
from run import (  # noqa: E402
    BASE_CONCEPTS,
    EST_JOB_OVERHEAD_S,
    EST_MS_PER_FRAME_CONCEPT,
    GPU_RATES,
    SEM_COLORS,
    build_chunks,
    object_prompts_for,
    pause_skipped,
    upload_frames,
)

RATE = GPU_RATES["L40S"]
EST_IMAGE_MS = EST_MS_PER_FRAME_CONCEPT["image"]["L40S"]
VOL_PILOT_REMOTE = "pilot"
COMPARE_CLASSES = ["person", "face", "eye", "mouth", "hand"]
CLASS_ID = {"person": 1, "face": 2, "eye": 3, "mouth": 4, "hand": 5, "object": 6}

MUG_TEST = {"segment": "E_mug", "object": "mug",
            "prompts": ["soda can", "can", "drink can"]}
PEN_TEST = {"segment": "E_pen", "object": "pen",
            "prompts": ["drumstick", "wooden stick", "stick", "pen"]}


def frame_seg_map(index_frames):
    return {f["file"]: f["segment_id"] for f in index_frames}


def pick_chunk(chunks, seg_of, pred):
    """Chunk with the most frames whose segment satisfies pred."""
    best, best_n = None, 0
    for c in chunks:
        n = sum(1 for fn in c["frames"] if pred(seg_of.get(fn)))
        if n > best_n:
            best, best_n = c, n
    return best, best_n


def assert_chunk_matches(c, local_dir):
    cj_path = local_dir / c["chunk"] / "chunk.json"
    if not cj_path.exists():
        raise SystemExit(f"missing local video labels: {cj_path}")
    cj = json.loads(cj_path.read_text())
    if c["frames"] != list(cj["frames"].keys()):
        raise SystemExit(
            f"{c['chunk']}: rebuilt frame list != local chunk.json frames "
            "(chunking diverged — do not trust the comparison)"
        )


def chunk_json_frames(c, local_dir):
    """Frame list from the local chunk.json, or None if not written yet."""
    cj_path = local_dir / c["chunk"] / "chunk.json"
    if not cj_path.exists():
        return None
    return list(json.loads(cj_path.read_text())["frames"].keys())


def prompt_test_specs(session, cam, index_frames, test):
    frames = [
        f["file"] for f in index_frames if f["segment_id"] == test["segment"]
    ][::5]
    prompts = [
        {
            "concept": f"{test['object']}__{p.replace(' ', '_')}",
            "prompt": p,
            "cls": "object",
        }
        for p in test["prompts"]
    ]
    return {
        "session": session,
        "cam": cam,
        "chunk": f"prompt_{test['object']}",
        "frames": frames,
        "prompts": prompts,
    }


def download_pilot(vol, session, cam, name, dest_dir: Path) -> int:
    remote_root = f"{VOL_PILOT_REMOTE}/{session}/{cam}/{name}"
    try:
        files = [
            fi
            for fi in vol.listdir(remote_root, recursive=True)
            if fi.path.endswith(".json")
        ]
    except Exception:
        return 0
    dest_dir.mkdir(parents=True, exist_ok=True)
    n = 0
    for fi in files:
        dest = dest_dir / Path(fi.path).name
        dest.write_bytes(b"".join(vol.read_file(fi.path)))
        n += 1
    return n


def run_pilot(jobs, max_usd, upfront_est, labeler_name="ImageLabelerL40S"):
    """Sequential launch under the cap; re-estimate after the first job."""
    import sam_label

    total_fp = sum(len(j["spec"]["frames"]) * len(j["spec"]["prompts"])
                   for j in jobs)
    spent_s = 0.0
    done_fp = 0
    results = {}
    with modal.enable_output():
        with sam_label.app.run():
            labeler = getattr(sam_label, labeler_name)()
            for k, job in enumerate(jobs):
                spec = job["spec"]
                if k == 1:
                    measured = results[jobs[0]["name"]]["ms_per_frame_concept"]
                    rem_fp = total_fp - done_fp
                    new_est = spent_s * RATE + (
                        rem_fp * measured / 1000
                        + (len(jobs) - k) * EST_JOB_OVERHEAD_S
                    ) * RATE
                    print(
                        f"  pilot measured {measured} ms/fc -> re-estimate "
                        f"${new_est:.3f} (up-front ${upfront_est:.3f})",
                        flush=True,
                    )
                    if new_est > max_usd:
                        print(
                            f"ABORT: pilot re-estimate ${new_est:.3f} > "
                            f"--max-usd {max_usd}; stopping launches",
                            flush=True,
                        )
                        break
                spend = spent_s * RATE
                if spend >= max_usd:
                    print(
                        f"BUDGET: stopping launches at ${spend:.3f}; "
                        f"{len(jobs) - k} job(s) left",
                        flush=True,
                    )
                    break
                t0 = time.time()
                r = labeler.label_chunk.spawn(spec).get()
                dt = time.time() - t0
                spent_s += r["gpu_s"]
                done_fp += len(spec["frames"]) * len(spec["prompts"])
                results[job["name"]] = r
                print(
                    f"  job {job['name']} {r['gpu_s']}s "
                    f"({dt:.0f}s wall) {r['ms_per_frame_concept']} ms/fc "
                    f"(spend ${spent_s * RATE:.3f})",
                    flush=True,
                )
    return results, spent_s


def sem_png(d: Path, fn: str):
    import numpy as np
    from PIL import Image

    p = d / (Path(fn).stem + ".png")
    return np.asarray(Image.open(p)) if p.exists() else None


def class_ious(sem_a, sem_b):
    """Per-class IoU; None where neither side has the class."""
    out = {}
    for c in COMPARE_CLASSES:
        cid = CLASS_ID[c]
        a, b = sem_a == cid, sem_b == cid
        u = int((a | b).sum())
        out[c] = (int((a & b).sum()) / u) if u else None
    return out


def compare_dirs(video_dir, pilot_dir, frames):
    """Per class: median + p10 IoU over frames where either side has it;
    plus per-frame hand IoU for the disagreement sheet."""
    per_class = {c: [] for c in COMPARE_CLASSES}
    hand_rows = []
    n_both = 0
    for fn in frames:
        a = sem_png(video_dir, fn)
        b = sem_png(pilot_dir, fn)
        if a is None or b is None or a.shape != b.shape:
            continue
        n_both += 1
        ious = class_ious(a, b)
        for c, v in ious.items():
            if v is not None:
                per_class[c].append(v)
        if ious["hand"] is not None:
            hand_rows.append({"frame": fn, "iou": ious["hand"]})
    agg = {}
    for c, vals in per_class.items():
        if not vals:
            agg[c] = {"median": None, "p10": None, "n": 0}
            continue
        vals = sorted(vals)
        agg[c] = {
            "median": round(vals[len(vals) // 2], 4),
            "p10": round(vals[max(0, int(0.1 * len(vals)))], 4),
            "n": len(vals),
        }
    return agg, hand_rows, n_both


def det_counts(stats_path: Path):
    if not stats_path.exists():
        return {}
    return json.loads(stats_path.read_text()).get("det_counts", {})


def chunk_call_costs(d: Path):
    """Fresh GPU calls in one chunk dir: {call_id: {gpu_s, fp}}.

    stats.json gpu_s alone is misleading: chunks may mix migrated raws
    (gpu_s 0) with single-concept calls. Each raw_<concept>.json carries
    call_id + method_gpu_s, so a call's frame-prompts = frames x concepts
    covered by that call."""
    calls = {}
    for rp in d.glob("raw_*.json"):
        r = json.loads(rp.read_text())
        cid = r.get("call_id")
        g = r.get("method_gpu_s") or 0.0
        if not cid or r.get("migrated") or g <= 0:
            continue
        e = calls.setdefault(cid, {"gpu_s": g, "fp": 0})
        e["fp"] += len(r["frames"])
    return calls


def video_cost_ref(cam_dir: Path, mode_note="video-mode"):
    """$/1000 frame-concepts per fresh call, grouped by call_id.
    Returns (median_usd_per_1000fc, source_note)."""
    per_call = [
        (c["gpu_s"], c["fp"])
        for d in sorted(cam_dir.glob("chunk_*"))
        for c in chunk_call_costs(d).values()
    ]
    if not per_call:
        return None, "no fresh calls (all migrated)"
    rates = sorted(g * RATE * 1000 / fp for g, fp in per_call if fp)
    med = rates[len(rates) // 2]
    return med, f"median of {len(rates)} fresh {mode_note} calls (L40S)"


def overlay(sem, frame_img, class_id, color):
    import numpy as np
    from PIL import Image

    base = np.asarray(frame_img).astype(np.float32)
    m = sem == class_id
    if m.any():
        for c in range(3):
            base[..., c][m] = 0.45 * base[..., c][m] + 0.55 * color[c]
    return Image.fromarray(base.astype(np.uint8))


def _cell(img, w, h, label, font):
    from PIL import ImageDraw

    cell = img.resize((w, h))
    d = ImageDraw.Draw(cell)
    d.rectangle([0, h - 18, w, h], fill=(0, 0, 0))
    d.text((4, h - 16), label, fill=(230, 230, 230), font=font)
    return cell


def disagree_sheet(rows, video_dirs, pilot_dirs, frames_dir, out_path):
    """12 frames of lowest hand IoU; cell = video | image overlay."""
    from PIL import Image, ImageFont

    rows = sorted(rows, key=lambda r: r["iou"])[:12]
    if not rows:
        return None
    try:
        font = ImageFont.load_default(size=13)
    except TypeError:
        font = ImageFont.load_default()
    sub_w, sub_h = 400, 225
    cols = 4
    sheet = Image.new(
        "RGB", (cols * 2 * sub_w, ((len(rows) + cols - 1) // cols) * sub_h),
        (14, 14, 18),
    )
    for i, r in enumerate(rows):
        fn = r["frame"]
        try:
            img = Image.open(frames_dir / fn).convert("RGB")
        except OSError:
            continue
        va = sem_png(video_dirs[r["chunk"]], fn)
        pb = sem_png(pilot_dirs[r["job"]], fn)
        if va is None or pb is None:
            continue
        left = _cell(
            overlay(va, img, CLASS_ID["hand"], SEM_COLORS[5]),
            sub_w, sub_h, f"video {fn}", font,
        )
        right = _cell(
            overlay(pb, img, CLASS_ID["hand"], SEM_COLORS[5]),
            sub_w, sub_h, f"image IoU {r['iou']:.3f}", font,
        )
        x = (i % cols) * 2 * sub_w
        y = (i // cols) * sub_h
        sheet.paste(left, (x, y))
        sheet.paste(right, (x + sub_w, y))
    sheet.save(out_path, quality=88)
    return out_path


def prompt_overlay_cells(raw_path: Path, frames_dir: Path, frames):
    """4 sample overlays per prompt decoded from that concept's raw RLEs."""
    import numpy as np
    from PIL import Image, ImageFont
    from pycocotools import mask as mask_utils

    raw = json.loads(raw_path.read_text())
    det_frames = [fn for fn in frames if raw["frames"].get(fn)]
    if det_frames:
        picks = [
            det_frames[i * (len(det_frames) - 1) // 3] for i in range(4)
        ]
    else:
        picks = frames[:4]
    try:
        font = ImageFont.load_default(size=13)
    except TypeError:
        font = ImageFont.load_default()
    w, h = 400, 225
    cells = []
    for fn in picks:
        insts = raw["frames"].get(fn, [])
        img = Image.open(frames_dir / fn).convert("RGB")
        if insts:
            best = max(insts, key=lambda i: i["score"] or 0)
            m = mask_utils.decode(
                {
                    "size": best["rle"]["size"],
                    "counts": best["rle"]["counts"].encode("ascii"),
                }
            ).astype(bool)
            arr = np.asarray(img).astype(np.float32)
            for c in range(3):
                arr[..., c][m] = (
                    0.45 * arr[..., c][m] + 0.55 * SEM_COLORS[6][c]
                )
            img = Image.fromarray(arr.astype(np.uint8))
            label = f"{fn} {best['score']:.2f}"
        else:
            label = f"{fn} no det"
        cells.append(_cell(img, w, h, label, font))
    return cells


# review-sheet overlay: eye cyan, mouth magenta, hand yellow, face light
# red at low alpha; person (1) and object (6) not drawn
REVIEW_CLASSES = [
    (CLASS_ID["face"], SEM_COLORS[2], 0.35),
    (CLASS_ID["eye"], SEM_COLORS[3], 0.55),
    (CLASS_ID["mouth"], SEM_COLORS[4], 0.55),
    (CLASS_ID["hand"], SEM_COLORS[5], 0.55),
]


def pick_even(frames, n):
    if len(frames) <= n:
        return list(frames)
    import numpy as np

    return [frames[i] for i in np.linspace(0, len(frames) - 1, n).astype(int)]


def review_overlay(sem, img):
    """Multi-class tint per REVIEW_CLASSES (person/object not drawn)."""
    import numpy as np
    from PIL import Image

    if sem is None:
        return img
    base = np.asarray(img).astype(np.float32)
    for cid, color, a in REVIEW_CLASSES:
        m = sem == cid
        if m.any():
            for c in range(3):
                base[..., c][m] = (
                    (1 - a) * base[..., c][m] + a * color[c]
                )
    return Image.fromarray(base.astype(np.uint8))


def face_union_box(sem_a, sem_b, pad=20):
    """xyxy box of face-class pixels across both sem maps +pad, or None."""
    import numpy as np

    m = None
    for s in (sem_a, sem_b):
        if s is None:
            continue
        sm = s == CLASS_ID["face"]
        m = sm if m is None else (m | sm)
    if m is None or not m.any():
        return None
    ys, xs = np.where(m)
    h, w = m.shape
    return (
        max(0, int(xs.min()) - pad),
        max(0, int(ys.min()) - pad),
        min(w, int(xs.max()) + pad),
        min(h, int(ys.max()) + pad),
    )


def review_sheet(ref_dir, cand_dir, frames, frames_dir, out_path,
                 tag_ref="image", tag_cand="video", n=15):
    """n evenly spaced frames; row = reference overlay | candidate overlay."""
    from PIL import Image, ImageFont

    try:
        font = ImageFont.load_default(size=13)
    except TypeError:
        font = ImageFont.load_default()
    w, h = 480, 270
    rows = []
    for fn in pick_even(frames, n):
        a = sem_png(ref_dir, fn)
        b = sem_png(cand_dir, fn)
        try:
            img = Image.open(frames_dir / fn).convert("RGB")
        except OSError:
            continue
        left = _cell(
            review_overlay(a, img), w, h, f"{tag_ref} {fn}", font
        )
        right = _cell(
            review_overlay(b, img), w, h, f"{tag_cand} {fn}", font
        )
        rows.append((left, right))
    if not rows:
        return None
    sheet = Image.new("RGB", (2 * w, len(rows) * h), (14, 14, 18))
    for i, (l, r) in enumerate(rows):
        sheet.paste(l, (0, i * h))
        sheet.paste(r, (w, i * h))
    out_path.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(out_path, quality=88)
    return out_path


def face_review_sheet(ref_dir, cand_dir, frames, frames_dir, out_path,
                      tag_ref="image", tag_cand="video", n=15):
    """Same frames cropped to the face union box +20 px, scaled 2x."""
    from PIL import Image, ImageFont

    try:
        font = ImageFont.load_default(size=13)
    except TypeError:
        font = ImageFont.load_default()
    tile = 320
    rows = []
    for fn in pick_even(frames, n):
        a = sem_png(ref_dir, fn)
        b = sem_png(cand_dir, fn)
        try:
            img = Image.open(frames_dir / fn).convert("RGB")
        except OSError:
            continue
        box = face_union_box(a, b) or (0, 0, img.width, img.height)
        cells = []
        for sem, tag in ((a, tag_ref), (b, tag_cand)):
            ov = review_overlay(sem, img).crop(box)
            ov = ov.resize((ov.width * 2, ov.height * 2),
                           Image.Resampling.NEAREST)
            ov.thumbnail((tile, tile - 20), Image.Resampling.NEAREST)
            cell = Image.new("RGB", (tile, tile), (14, 14, 18))
            cell.paste(ov, ((tile - ov.width) // 2,
                            (tile - 20 - ov.height) // 2))
            cells.append(_cell(cell, tile, tile, f"{tag} {fn}", font))
        rows.append(cells)
    if not rows:
        return None
    sheet = Image.new("RGB", (2 * tile, len(rows) * tile), (14, 14, 18))
    for i, (l, r) in enumerate(rows):
        sheet.paste(l, (0, i * tile))
        sheet.paste(r, (tile, i * tile))
    out_path.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(out_path, quality=88)
    return out_path


def prompt_sheet(jobs, pilot_dirs, cam, frames_dir, out_path):
    """Grid: one row per prompt-test concept, 4 overlay cells each."""
    from PIL import Image, ImageDraw, ImageFont

    rows = []
    for job in jobs:
        if job["kind"] != "prompt" or job["spec"]["cam"] != cam:
            continue
        d = pilot_dirs.get(job["name"])
        if d is None:
            continue
        for p in job["spec"]["prompts"]:
            rp = d / f"raw_{p['concept']}.json"
            if rp.exists():
                rows.append(
                    (
                        p["concept"],
                        prompt_overlay_cells(
                            rp, frames_dir, job["spec"]["frames"]
                        ),
                    )
                )
    if not rows:
        return None
    try:
        font = ImageFont.load_default(size=13)
    except TypeError:
        font = ImageFont.load_default()
    w, h = rows[0][1][0].size
    sheet = Image.new("RGB", (4 * w, len(rows) * h), (14, 14, 18))
    for ri, (concept, cells) in enumerate(rows):
        for ci, cell in enumerate(cells):
            cell = cell.copy()
            ImageDraw.Draw(cell).text(
                (4, 2), concept, fill=(230, 230, 230), font=font
            )
            sheet.paste(cell, (ci * w, ri * h))
    out_path.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(out_path, quality=88)
    return out_path


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("session_dir", type=Path)
    ap.add_argument("--session-name")
    ap.add_argument(
        "--max-usd", type=float, default=None,
        help="spend cap (default 0.75 forward, 0.40 --reverse)",
    )
    ap.add_argument("--variants", default="seq,grouped")
    ap.add_argument("--chunk-size", type=int, default=300)
    ap.add_argument(
        "--reverse", action="store_true",
        help="video-mode jobs on block-C/block-H chunks vs image labels",
    )
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument(
        "--analyze-only", action="store_true",
        help="no GPU: rebuild results from already downloaded pilot raws",
    )
    args = ap.parse_args()
    max_usd = args.max_usd
    if max_usd is None:
        max_usd = 0.40 if args.reverse else 0.75

    variants = [v.strip() for v in args.variants.split(",") if v.strip()]
    bad = set(variants) - {"seq", "grouped"}
    if bad:
        ap.error(f"unknown variant(s): {sorted(bad)}")

    root = repo_root()
    session_dir = args.session_dir.resolve()
    _meta, segments = load_session(session_dir)
    session = args.session_name or session_dir.name
    seg_by_id = {s["id"]: s for s in segments}
    objmap = object_prompts_for(session)
    frames_root = root / "data" / "frames" / session
    labels_root = root / "data" / "labels" / session
    pilot_root = root / "data" / "labels" / "_pilot" / session

    # ---- rebuild S1's chunking exactly, then pick the pilot jobs --------
    jobs = []  # {"name", "kind": "chunk"|"prompt", "spec"}
    picks = {}
    for cam_dir in sorted(frames_root.iterdir()):
        if not cam_dir.is_dir() or not (cam_dir / "frames_index.json").exists():
            continue
        cam = cam_dir.name
        index = json.loads((cam_dir / "frames_index.json").read_text())
        skip = pause_skipped(index["frames"], segments, cam)
        chunks = build_chunks(
            session, cam, index["frames"], segments, objmap,
            args.chunk_size, skip,
        )
        seg_of = frame_seg_map(index["frames"])

        def segs_of(c):
            return sorted(
                {seg_of[fn] for fn in c["frames"] if seg_of.get(fn)}
            )

        print(f"{cam}: {len(chunks)} chunks")
        if args.reverse:
            block_of = lambda s: (  # noqa: E731
                seg_by_id.get(s, {}).get("block") if s else None
            )
            c1, n1 = pick_chunk(
                chunks, seg_of, lambda s: block_of(s) == "C"
            )
            h1, n2 = pick_chunk(
                chunks, seg_of, lambda s: block_of(s) == "H"
            )
            if c1 is None or h1 is None:
                raise SystemExit(
                    f"{cam}: no block-C or block-H frames found"
                )
            picks[cam] = {"C1": c1, "H1": h1}
            for tag, c, n in (("C1", c1, n1), ("H1", h1, n2)):
                ref = chunk_json_frames(c, labels_root / cam)
                if ref is not None and c["frames"] != ref:
                    raise SystemExit(
                        f"{c['chunk']}: rebuilt frame list != local "
                        "chunk.json frames (chunking diverged)"
                    )
                pending = " [labels pending on disk]" if ref is None else ""
                print(
                    f"  {tag} {c['chunk']}: {n} block-{tag[0]} frames of "
                    f"{len(c['frames'])}; segments {segs_of(c)}{pending}"
                )
                name = f"{c['chunk']}__video"
                jobs.append({
                    "name": name,
                    "kind": "rev",
                    "ref_chunk": c["chunk"],
                    "spec": {
                        **c,
                        "chunk": name,
                        "prompts": list(BASE_CONCEPTS),
                        "variant": "video",
                        "out_dir": (
                            f"/vol/{VOL_PILOT_REMOTE}/{session}/{cam}/{name}"
                        ),
                    },
                })
            continue

        p1, n1 = pick_chunk(
            chunks, seg_of, lambda s: s is not None and s.startswith("A_both_")
        )
        p2, n2 = pick_chunk(
            chunks, seg_of,
            lambda s: s is not None and seg_by_id.get(s, {}).get("block") == "F",
        )
        if p1 is None or p2 is None:
            raise SystemExit(f"{cam}: no A_both_* or block-F frames found")
        for c in (p1, p2):
            assert_chunk_matches(c, labels_root / cam)
        picks[cam] = {"P1": p1, "P2": p2, "n1": n1, "n2": n2}

        print(
            f"  P1 {p1['chunk']}: {n1} A_both_* frames of "
            f"{len(p1['frames'])}; segments {segs_of(p1)}"
        )
        print(
            f"  P2 {p2['chunk']}: {n2} block-F frames of "
            f"{len(p2['frames'])}; segments {segs_of(p2)}"
        )

        for v in variants:
            name = f"{p1['chunk']}__{v}"
            jobs.append({
                "name": name,
                "kind": "chunk",
                "video_chunk": p1["chunk"],
                "spec": {
                    **p1,
                    "chunk": name,
                    "variant": v,
                    "out_dir": f"/vol/{VOL_PILOT_REMOTE}/{session}/{cam}/{name}",
                },
            })
        name = f"{p2['chunk']}__seq"
        jobs.append({
            "name": name,
            "kind": "chunk",
            "video_chunk": p2["chunk"],
            "spec": {
                **p2,
                "chunk": name,
                "variant": "seq",
                "out_dir": f"/vol/{VOL_PILOT_REMOTE}/{session}/{cam}/{name}",
            },
        })
        for test in (MUG_TEST, PEN_TEST):
            spec = prompt_test_specs(session, cam, index["frames"], test)
            name = f"{spec['chunk']}__seq"
            spec["variant"] = "seq"
            spec["out_dir"] = (
                f"/vol/{VOL_PILOT_REMOTE}/{session}/{cam}/{name}"
            )
            print(
                f"  {spec['chunk']}: {len(spec['frames'])} frames x "
                f"{len(spec['prompts'])} prompts"
            )
            jobs.append({"name": name, "kind": "prompt", "spec": spec})

    total_fp = sum(
        len(j["spec"]["frames"]) * len(j["spec"]["prompts"]) for j in jobs
    )
    est_ms = (
        EST_MS_PER_FRAME_CONCEPT["video"]["L40S"]
        if args.reverse
        else EST_IMAGE_MS
    )
    est = (
        total_fp * est_ms / 1000 + len(jobs) * EST_JOB_OVERHEAD_S
    ) * RATE
    print(
        f"{len(jobs)} job(s), {total_fp} frame-prompts, "
        f"est ${est:.3f} (cap ${max_usd})"
    )
    if args.dry_run:
        print("dry-run: nothing launched")
        return
    if est > max_usd:
        raise SystemExit(
            f"estimate ${est:.2f} exceeds --max-usd {max_usd}; refusing"
        )

    if args.analyze_only:
        results = {}
        for job in jobs:
            raws = sorted((pilot_root / job["name"]).glob("raw_*.json"))
            if not raws:
                continue
            d = json.loads(raws[0].read_text())
            fp = len(job["spec"]["frames"]) * len(job["spec"]["prompts"])
            results[job["name"]] = {
                "gpu_s": d["method_gpu_s"],
                "ms_per_frame_concept": round(1000 * d["method_gpu_s"] / fp, 2),
                "call_id": d["call_id"],
                "variant": d.get("variant"),
            }
        spent_s = sum(r["gpu_s"] for r in results.values())
    else:
        # ---- GPU: upload (idempotent), launch, download ------------------
        vol = modal.Volume.from_name(
            "synthexis-recordings", create_if_missing=True
        )
        for cam in picks:
            upload_frames(vol, frames_root / cam, session, cam)

        results, spent_s = run_pilot(
            jobs, max_usd, est,
            "LabelerL40S" if args.reverse else "ImageLabelerL40S",
        )

        for job in jobs:
            if job["name"] not in results:
                continue
            dest = pilot_root / job["name"]
            n = download_pilot(
                vol, session, job["spec"]["cam"], job["name"], dest
            )
            print(f"  {job['name']}: {n} raw file(s) -> {dest}")

    # ---- local post-process + comparison --------------------------------
    pilot_dirs = {}
    for job in jobs:
        d = pilot_root / job["name"]
        if not any(d.glob("raw_*.json")):
            continue
        process_chunk(d, job["spec"]["frames"], frames_root / job["spec"]["cam"])
        pilot_dirs[job["name"]] = d

    out = {
        "session": session,
        "direction": "reverse" if args.reverse else "forward",
        "estimate_usd": round(est, 4),
        "picks": {
            cam: {
                k: picks[cam][k]["chunk"]
                for k in ("P1", "P2", "C1", "H1")
                if k in picks[cam]
            }
            for cam in picks
        },
        "jobs": {},
        "chunks": {},
        "prompt_tests": {},
        "cost": {},
    }
    for job in jobs:
        r = results.get(job["name"])
        if r:
            fp = len(job["spec"]["frames"]) * len(job["spec"]["prompts"])
            out["jobs"][job["name"]] = {
                **r,
                "usd_per_1000fc": round(r["gpu_s"] * RATE * 1000 / fp, 4),
            }
    out["cost"]["spent_gpu_s"] = round(spent_s, 2)
    out["cost"]["spent_usd"] = round(spent_s * RATE, 4)

    hand_rows = []
    video_dirs = {}
    for cam in picks:
        cam_dir = labels_root / cam
        manifest_p = labels_root / "manifest.json"
        manifest = (
            json.loads(manifest_p.read_text()) if manifest_p.exists() else {}
        )
        ref, ref_note = video_cost_ref(
            cam_dir, "image-mode" if args.reverse else "video-mode"
        )
        ref_key = "image" if args.reverse else "video"
        out["cost"][f"{cam}_{ref_key}_reference"] = {
            "usd_per_1000fc": round(ref, 4) if ref else None,
            "note": ref_note,
            "manifest_gpu": manifest.get("gpu"),
            "gpu_s_previous_runs_h100": manifest.get(cam, {}).get(
                "gpu_s_previous_runs"
            ),
        }
        for job in jobs:
            if job["kind"] != "chunk" or job["spec"]["cam"] != cam:
                continue
            if job["name"] not in pilot_dirs:
                continue
            vdir = cam_dir / job["video_chunk"]
            pdir = pilot_dirs[job["name"]]
            video_dirs[job["video_chunk"]] = vdir
            agg, rows, n_both = compare_dirs(
                vdir, pdir, job["spec"]["frames"]
            )
            for r in rows:
                r["chunk"] = job["video_chunk"]
                r["job"] = job["name"]
            hand_rows += rows
            vstats = det_counts(vdir / "stats.json")
            pstats = det_counts(pdir / "stats.json")
            vcalls = chunk_call_costs(vdir)
            vgpu = sum(c["gpu_s"] for c in vcalls.values())
            vfp = sum(c["fp"] for c in vcalls.values())
            fp = len(job["spec"]["frames"]) * len(job["spec"]["prompts"])
            out["chunks"][job["name"]] = {
                "video_chunk": job["video_chunk"],
                "variant": job["spec"]["variant"],
                "frames_compared": n_both,
                "iou": agg,
                "frames_with_det": {
                    c: {
                        "video": vstats.get(c, {}).get("frames_with_det", 0),
                        "image": pstats.get(c, {}).get("frames_with_det", 0),
                    }
                    for c in sorted(set(vstats) | set(pstats))
                },
                "video_stats": {
                    "gpu_s": round(vgpu, 2),
                    "migrated": not vcalls,
                    "usd_per_1000fc": (
                        round(vgpu * RATE * 1000 / vfp, 4) if vfp else None
                    ),
                },
            }

        # grouped vs seq on the same chunk (forward picks only)
        seq_name = f"{picks[cam].get('P1', {}).get('chunk')}__seq"
        grp_name = f"{picks[cam].get('P1', {}).get('chunk')}__grouped"
        if (
            "P1" in picks[cam]
            and seq_name in pilot_dirs
            and grp_name in pilot_dirs
        ):
            agg, _, n_both = compare_dirs(
                pilot_dirs[seq_name], pilot_dirs[grp_name],
                picks[cam]["P1"]["frames"],
            )
            out["chunks"][f"{picks[cam]['P1']['chunk']}__seq_vs_grouped"] = {
                "frames_compared": n_both,
                "iou": agg,
            }

        # reverse: video pilot dirs vs the image-mode reference labels
        for job in jobs:
            if job["kind"] != "rev" or job["spec"]["cam"] != cam:
                continue
            pdir = pilot_dirs.get(job["name"])
            if pdir is None:
                continue
            rdir = cam_dir / job["ref_chunk"]
            agg, rows, n_both = compare_dirs(
                rdir, pdir, job["spec"]["frames"]
            )
            for r in rows:
                r["chunk"] = job["ref_chunk"]
                r["job"] = job["name"]
            hand_rows += rows
            istats = det_counts(rdir / "stats.json")
            vstats = det_counts(pdir / "stats.json")
            icalls = chunk_call_costs(rdir)
            igpu = sum(c["gpu_s"] for c in icalls.values())
            ifp = sum(c["fp"] for c in icalls.values())
            if not n_both:
                print(
                    f"  WARN {job['name']}: no comparable frames — "
                    f"reference labels missing in {rdir}?"
                )
            out["chunks"][job["name"]] = {
                "direction": "reverse",
                "ref_chunk": job["ref_chunk"],
                "frames_compared": n_both,
                "iou": agg,
                "frames_with_det": {
                    c: {
                        "image": istats.get(c, {}).get("frames_with_det", 0),
                        "video": vstats.get(c, {}).get("frames_with_det", 0),
                    }
                    for c in sorted(set(istats) | set(vstats))
                },
                "image_stats": {
                    "gpu_s": round(igpu, 2),
                    "usd_per_1000fc": (
                        round(igpu * RATE * 1000 / ifp, 4) if ifp else None
                    ),
                },
            }
            rs = review_sheet(
                rdir, pdir, job["spec"]["frames"], frames_root / cam,
                pilot_root / f"review_{job['ref_chunk']}.jpg",
            )
            fs = face_review_sheet(
                rdir, pdir, job["spec"]["frames"], frames_root / cam,
                pilot_root / f"review_face_{job['ref_chunk']}.jpg",
            )
            if rs:
                out["chunks"][job["name"]]["review_sheet"] = str(rs)
            if fs:
                out["chunks"][job["name"]]["face_sheet"] = str(fs)

        # prompt tests: per concept coverage + score
        for job in jobs:
            if job["kind"] != "prompt" or job["spec"]["cam"] != cam:
                continue
            d = pilot_dirs.get(job["name"])
            if d is None:
                continue
            entry = {}
            for p in job["spec"]["prompts"]:
                rp = d / f"raw_{p['concept']}.json"
                if not rp.exists():
                    continue
                raw = json.loads(rp.read_text())
                maxes = [
                    max(i["score"] for i in insts)
                    for insts in raw["frames"].values()
                    if insts
                ]
                entry[p["concept"]] = {
                    "prompt": p["prompt"],
                    "frames_with_det": len(maxes),
                    "frames_total": len(job["spec"]["frames"]),
                    "mean_max_score": (
                        round(sum(maxes) / len(maxes), 4) if maxes else None
                    ),
                }
            out["prompt_tests"][job["name"]] = entry
        sp = pilot_root / "prompts.jpg"
        if prompt_sheet(jobs, pilot_dirs, cam, frames_root / cam, sp):
            out["prompt_sheet"] = str(sp)

    pilot_root.mkdir(parents=True, exist_ok=True)
    sp = pilot_root / "disagree_hand.jpg"
    sheet = None
    for cam in picks:
        seq_pilots = {
            j["name"]: pilot_dirs[j["name"]]
            for j in jobs
            if j["kind"] == "chunk"
            and j["spec"]["cam"] == cam
            and j["spec"]["variant"] == "seq"
            and j["name"] in pilot_dirs
        }
        rows = [r for r in hand_rows if r["job"] in seq_pilots]
        if rows:
            sheet = disagree_sheet(
                rows,
                {c["video_chunk"]: labels_root / cam / c["video_chunk"]
                 for c in jobs if c["kind"] == "chunk"},
                seq_pilots,
                frames_root / cam,
                sp,
            )
    if sheet:
        out["disagree_sheet"] = str(sheet)

    (pilot_root / "compare.json").write_text(json.dumps(out, indent=1))

    # ---- stdout table ----------------------------------------------------
    rev_entries = {
        k: v for k, v in out["chunks"].items()
        if v.get("direction") == "reverse"
    }
    if rev_entries:
        print("\n=== IoU video-mode vs image labels (reverse) ===")
        for name, c in rev_entries.items():
            print(
                f"\n{name} (image {c['ref_chunk']}, "
                f"{c['frames_compared']} frames)"
            )
            ist = c["image_stats"]
            print(
                f"  image ref gpu_s {ist['gpu_s']} "
                f"(${ist['usd_per_1000fc']}/1000fc)"
            )
            print(f"  {'class':<8} {'IoU med':>8} {'IoU p10':>8} {'n':>5}")
            for cls in COMPARE_CLASSES:
                a = c["iou"][cls]
                med = f"{a['median']:.3f}" if a["median"] is not None else "-"
                p10 = f"{a['p10']:.3f}" if a["p10"] is not None else "-"
                print(f"  {cls:<8} {med:>8} {p10:>8} {a['n']:>5}")
            print("  det frames (video/image):")
            for concept, d in c["frames_with_det"].items():
                print(f"    {concept:<14} {d['video']:>5} / {d['image']:<5}")

    print("\n=== IoU image-mode vs video labels (semantic PNGs) ===")
    for name, c in out["chunks"].items():
        if "iou" not in c or "video_chunk" not in c:
            continue
        print(f"\n{name} (video {c['video_chunk']}, {c['frames_compared']} frames)")
        vs = c["video_stats"]
        print(
            f"  video stats.json gpu_s {vs['gpu_s']}"
            + (
                " (migrated — old H100 run, per-chunk cost n/a)"
                if vs["migrated"]
                else ""
            )
        )
        print(f"  {'class':<8} {'IoU med':>8} {'IoU p10':>8} {'n':>5}")
        for cls in COMPARE_CLASSES:
            a = c["iou"][cls]
            med = f"{a['median']:.3f}" if a["median"] is not None else "-"
            p10 = f"{a['p10']:.3f}" if a["p10"] is not None else "-"
            print(f"  {cls:<8} {med:>8} {p10:>8} {a['n']:>5}")
        print("  det frames (video/image):")
        for concept, d in c["frames_with_det"].items():
            print(f"    {concept:<14} {d['video']:>5} / {d['image']:<5}")
    for name, c in out["chunks"].items():
        if "seq_vs_grouped" in name:
            print(f"\n=== grouped vs seq ({name}) ===")
            for cls in COMPARE_CLASSES:
                a = c["iou"][cls]
                med = f"{a['median']:.3f}" if a["median"] is not None else "-"
                print(f"  {cls:<8} IoU med {med} (n={a['n']})")
    print("\n=== cost ===")
    for name, j in out["jobs"].items():
        print(
            f"  {name:<28} {j['gpu_s']:>8.2f}s  "
            f"{j['ms_per_frame_concept']:>7.2f} ms/fc  "
            f"${j['usd_per_1000fc']:.4f}/1000fc"
        )
    for k, v in out["cost"].items():
        if isinstance(v, dict) and "note" in v:
            print(
                f"  video ref {k}: ${v['usd_per_1000fc']}/1000fc "
                f"({v['note']}; manifest gpu={v['manifest_gpu']})"
            )
    print(f"  total spend ${out['cost']['spent_usd']:.4f}")
    print("\n=== prompt tests ===")
    for name, entry in out["prompt_tests"].items():
        print(f"{name}:")
        for concept, d in entry.items():
            s = (
                f"{d['mean_max_score']:.3f}"
                if d["mean_max_score"] is not None
                else "-"
            )
            print(
                f"  {concept:<28} det {d['frames_with_det']:>3}/"
                f"{d['frames_total']:<3} mean_max_score {s}"
            )
    print(f"\ncompare.json -> {pilot_root / 'compare.json'}")
    if sheet:
        print(f"disagreement sheet -> {sheet}")
    if out.get("prompt_sheet"):
        print(f"prompt sheet -> {out['prompt_sheet']}")


if __name__ == "__main__":
    main()
