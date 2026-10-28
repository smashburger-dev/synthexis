"""SAM 3.1 real-weight probe on Modal.

Same image setup as sam31_probe.py (sam3 commit f1647e8, torch 2.10 cu128),
but loads the real facebook/sam3.1 checkpoint via HF_TOKEN (Modal secret
`huggingface`). The HF cache is pointed at a Modal Volume (`sam31-weights`)
so the checkpoint is downloaded once and reused on later runs.

Measures:
  1. image mode: 20 evenly spaced frames x 5 text prompts
     (person, face, eye, mouth, hand) — per-prompt detection count + score,
     per-(frame, prompt) mask-overlay JPEGs and a contact sheet, all returned
     to the local entrypoint which writes them to data/sam31_real/.
  2. video mode: per concept (same 5) over 300 consecutive frames —
     ms/frame median + p95, tracked-object counts, wall time, peak GPU mem.
  3. cost estimate from measured GPU seconds x Modal H100 rate.

Run:
    PROBE_GPU=H100 uvx modal run probes/sam31-modal/sam31_real.py \
        2>&1 | tee probes/sam31-modal/raw_h100_real.log
"""

import io
import json
import os
from pathlib import Path

import modal

SAM3_COMMIT = "f1647e8d0c1d280991f4d1b7d974c2704537fe66"
GPU = os.environ.get("PROBE_GPU", "H100")

# Modal on-demand rate for H100 (same figure used for the random-weight
# probe in RESULT.md): $3.95/h. Timeout caps a single run near $2.30 so the
# whole task cannot blow the $5 budget even with one retry.
GPU_RATE_USD_PER_S = 0.001097
FUNCTION_TIMEOUT_S = 2100  # 2100 s x $0.001097/s = $2.30 hard cap

FRAMES_REMOTE = "/data/test_frames"
WEIGHTS_MOUNT = "/weights"
PROMPTS = ["person", "face", "eye", "mouth", "hand"]
N_IMAGE_FRAMES = 20
PROMPT_COLORS = {
    "person": (0, 220, 80),
    "face": (255, 70, 70),
    "eye": (0, 200, 255),
    "mouth": (255, 0, 255),
    "hand": (255, 210, 0),
}

_here = Path(__file__).resolve()
_candidates = [
    _here.parents[2] / "data" / "test_frames" if len(_here.parents) > 2 else None,
    Path.cwd() / "data" / "test_frames",
    Path(FRAMES_REMOTE),
]
FRAMES_LOCAL = str(next(p for p in _candidates if p and p.is_dir()))

image = (
    modal.Image.debian_slim(python_version="3.12")
    .apt_install("git")
    .pip_install(
        "torch==2.10.0",
        "torchvision",
        extra_index_url="https://download.pytorch.org/whl/cu128",
    )
    .run_commands(
        "git clone https://github.com/facebookresearch/sam3.git /opt/sam3",
        f"cd /opt/sam3 && git checkout {SAM3_COMMIT}",
    )
    .pip_install(
        "einops",
        "ninja",
        "setuptools<81",
        "pillow",
        "pycocotools",
        "opencv-python-headless",
        "hydra-core",
        "omegaconf",
        "scipy",
        "scikit-image",
        "matplotlib",
        "pandas",
        "psutil",
        "huggingface_hub",
    )
    .run_commands("cd /opt/sam3 && pip install -e .")
    # hf_hub_download caches into the mounted weights volume.
    .env({"HF_HUB_CACHE": f"{WEIGHTS_MOUNT}/hf-cache"})
    .add_local_dir(FRAMES_LOCAL, remote_path=FRAMES_REMOTE)
)

weights_vol = modal.Volume.from_name("sam31-weights", create_if_missing=True)

app = modal.App("synthexis-sam31-real", image=image)


def _stats(times):
    times = sorted(times)
    n = len(times)
    median = times[n // 2] if n % 2 else 0.5 * (times[n // 2 - 1] + times[n // 2])
    p95 = times[min(n - 1, int(0.95 * n))]
    return median, p95


@app.function(
    gpu=GPU,
    timeout=FUNCTION_TIMEOUT_S,
    secrets=[modal.Secret.from_name("huggingface")],
    volumes={WEIGHTS_MOUNT: weights_vol},
)
def run_probe():
    import subprocess
    import time

    import torch

    t_fn_start = time.time()
    print(f"FN_START_EPOCH {t_fn_start:.3f}", flush=True)
    print(f"gpu_env {GPU} timeout_s {FUNCTION_TIMEOUT_S}", flush=True)

    # --- install / import info -----------------------------------------
    print(f"torch {torch.__version__}", flush=True)
    print(f"torch.version.cuda {torch.version.cuda}", flush=True)
    print(f"gpu_name {torch.cuda.get_device_name(0)}", flush=True)
    sam3_commit = (
        subprocess.check_output(["git", "-C", "/opt/sam3", "rev-parse", "HEAD"])
        .decode()
        .strip()
    )
    print(f"sam3_commit {sam3_commit}", flush=True)
    # Never print the token; only confirm it is present.
    print(f"hf_token_present {bool(os.environ.get('HF_TOKEN'))}", flush=True)

    import sam3.model_builder as mb
    from sam3.model.sam3_image_processor import Sam3Processor

    # --- real checkpoint from HF (cached in the volume) -----------------
    t0 = time.time()
    ckpt_path = mb.download_ckpt_from_hf(version="sam3.1")
    print(
        f"ckpt_download_s {time.time() - t0:.2f} ckpt_path {ckpt_path}",
        flush=True,
    )
    try:
        weights_vol.commit()
    except Exception as e:  # commit happens at container stop anyway
        print(f"volume_commit_warn {e}", flush=True)

    # --- image model ----------------------------------------------------
    t0 = time.time()
    image_model = mb.build_sam3_image_model(
        checkpoint_path=ckpt_path,
        load_from_HF=False,
        device="cuda",
        eval_mode=True,
    )
    torch.cuda.synchronize()
    t_img_model = time.time()
    print(f"image_model_build_s {t_img_model - t0:.2f}", flush=True)

    # --- image mode: 20 frames x 5 prompts ------------------------------
    import numpy as np
    from PIL import Image, ImageDraw, ImageFont

    frame_paths = sorted(
        p for p in Path(FRAMES_REMOTE).iterdir() if p.suffix == ".jpg"
    )
    n_frames = len(frame_paths)
    print(f"frames {n_frames}", flush=True)
    sel_idx = np.linspace(0, n_frames - 1, N_IMAGE_FRAMES).astype(int)
    sel_frames = [frame_paths[i] for i in sel_idx]
    processor = Sam3Processor(image_model, device="cuda")

    def image_one(pil_img, text):
        with torch.autocast(device_type="cuda", dtype=torch.bfloat16):
            state = processor.set_image(pil_img)
            out = processor.set_text_prompt(text, state)
        return out

    def render_overlay(pil_img, out, prompt):
        color = PROMPT_COLORS[prompt]
        base = pil_img.convert("RGB").copy()
        masks = out["masks"].cpu().numpy()  # [N, H, W] bool
        boxes = out["boxes"].float().cpu().numpy()  # [N, 4] xyxy pixels
        scores = out["scores"].float().cpu().numpy()
        if masks.size:
            overlay = np.asarray(base).astype(np.float32)
            # masks arrive as [N, 1, H, W]; collapse to a single [H, W] map
            m = masks.reshape(-1, masks.shape[-2], masks.shape[-1]).any(axis=0)
            for c in range(3):
                overlay[..., c][m] = (
                    0.5 * overlay[..., c][m] + 0.5 * color[c]
                )
            base = Image.fromarray(overlay.astype(np.uint8))
        draw = ImageDraw.Draw(base)
        for i, box in enumerate(boxes):
            x0, y0, x1, y1 = [float(v) for v in box]
            for w in range(3):
                draw.rectangle(
                    [x0 - w, y0 - w, x1 + w, y1 + w], outline=color
                )
            draw.text(
                (x0 + 2, y0 + 2),
                f"{prompt} {scores[i]:.2f}",
                fill=color,
            )
        return base

    files = {}  # relpath -> bytes, returned to the local entrypoint
    det_records = []  # {frame, prompt, n, score_max, score_med, ms}
    overlays_for_sheet = {}  # (frame_stem, prompt) -> PIL image

    torch.cuda.reset_peak_memory_stats()
    # warmup: one full frame+prompt cycle (untimed for stats)
    image_one(Image.open(sel_frames[0]).convert("RGB"), "hand")
    torch.cuda.synchronize()

    infer_ms = []
    for fp in sel_frames:
        pil_img = Image.open(fp).convert("RGB")
        with torch.autocast(device_type="cuda", dtype=torch.bfloat16):
            state = processor.set_image(pil_img)
        torch.cuda.synchronize()
        for prompt in PROMPTS:
            ta = time.perf_counter()
            with torch.autocast(device_type="cuda", dtype=torch.bfloat16):
                out = processor.set_text_prompt(prompt, state)
            torch.cuda.synchronize()
            ms = (time.perf_counter() - ta) * 1000
            infer_ms.append(ms)
            sc = out["scores"].float().cpu().numpy()
            n_det = int(sc.shape[0])
            det_records.append(
                {
                    "frame": fp.stem,
                    "prompt": prompt,
                    "n": n_det,
                    "score_max": float(sc.max()) if n_det else 0.0,
                    "score_med": float(np.median(sc)) if n_det else 0.0,
                    "ms": round(ms, 1),
                }
            )
            ov = render_overlay(pil_img, out, prompt)
            overlays_for_sheet[(fp.stem, prompt)] = ov
            buf = io.BytesIO()
            ov.save(buf, format="JPEG", quality=85)
            files[f"overlays/{fp.stem}__{prompt}.jpg"] = buf.getvalue()

    img_med, img_p95 = _stats(infer_ms)
    img_peak = torch.cuda.max_memory_allocated() / 2**30
    print(
        f"IMAGE_MODE text_prompt_ms_med {img_med:.2f} "
        f"text_prompt_ms_p95 {img_p95:.2f} peak_mem_gib {img_peak:.2f}",
        flush=True,
    )
    for prompt in PROMPTS:
        recs = [r for r in det_records if r["prompt"] == prompt]
        ns = [r["n"] for r in recs]
        smax = [r["score_max"] for r in recs]
        det_med = sorted(ns)[len(ns) // 2]
        det_mean = sum(ns) / len(ns)
        smax_med = sorted(smax)[len(smax) // 2]
        frames_hit = sum(1 for r in recs if r["n"] > 0)
        print(
            f"IMAGE_DET prompt {prompt} det_med {det_med} "
            f"det_mean {det_mean:.2f} score_max_med {smax_med:.3f} "
            f"frames_with_det {frames_hit}/{len(recs)}",
            flush=True,
        )

    # contact sheet: 5 frames (subset of the 20) x 5 prompts
    sheet_frames = [sel_frames[i].stem for i in range(0, len(sel_frames), 4)][
        :5
    ]
    cell_w, cell_h = 480, 270
    sheet = Image.new(
        "RGB", (cell_w * len(PROMPTS), cell_h * len(sheet_frames)), (16, 16, 20)
    )
    try:
        font = ImageFont.load_default(size=18)
    except TypeError:
        font = ImageFont.load_default()
    for r, stem in enumerate(sheet_frames):
        for c, prompt in enumerate(PROMPTS):
            cell = overlays_for_sheet[(stem, prompt)].resize(
                (cell_w, cell_h)
            )
            d = ImageDraw.Draw(cell)
            d.rectangle([0, 0, cell_w - 1, 24], fill=(0, 0, 0))
            d.text(
                (6, 3),
                f"{stem} | {prompt}",
                fill=PROMPT_COLORS[prompt],
                font=font,
            )
            sheet.paste(cell, (c * cell_w, r * cell_h))
    buf = io.BytesIO()
    sheet.save(buf, format="JPEG", quality=88)
    files["contact_sheet.jpg"] = buf.getvalue()

    det_json = json.dumps(det_records, indent=1).encode()
    files["detections.json"] = det_json

    del image_model, processor
    torch.cuda.empty_cache()

    # --- video mode: 5 concepts x 300 frames ----------------------------
    t0 = time.time()
    video_predictor = mb.build_sam3_multiplex_video_predictor(
        checkpoint_path=ckpt_path,
        use_fa3=False,
        compile=False,
        warm_up=False,
    )
    torch.cuda.synchronize()
    print(f"video_predictor_build_s {time.time() - t0:.2f}", flush=True)

    # Same pinned-commit shim as sam31_probe.py: start_session always passes
    # offload_state_to_cpu but the multiplex init_state doesn't accept it.
    _orig_init_state = video_predictor.model.init_state

    def _init_state_shim(**kwargs):
        kwargs.pop("offload_state_to_cpu", None)
        return _orig_init_state(**kwargs)

    video_predictor.model.init_state = _init_state_shim

    def run_concept(text):
        wall0 = time.perf_counter()
        resp = video_predictor.handle_request(
            {"type": "start_session", "resource_path": FRAMES_REMOTE}
        )
        sid = resp["session_id"]
        video_predictor.handle_request(
            {
                "type": "add_prompt",
                "session_id": sid,
                "frame_index": 0,
                "text": text,
            }
        )
        torch.cuda.synchronize()
        t_first = t_prev = time.perf_counter()
        # propagate_in_video streams outputs in ~16-frame multiplex batches,
        # so per-yield deltas are bimodal (~0 ms inside a burst, ~900 ms at
        # batch boundaries). The meaningful per-frame statistic is the
        # cumulative elapsed/frames-done rate at each yield; per-yield
        # deltas are still recorded to document the burst pattern.
        frame_ms, cum_ms, obj_counts = [], [], []
        printed_keys = False
        for out in video_predictor.handle_stream_request(
            {
                "type": "propagate_in_video",
                "session_id": sid,
                "propagation_direction": "forward",
            }
        ):
            torch.cuda.synchronize()
            t_now = time.perf_counter()
            frame_ms.append((t_now - t_prev) * 1000)
            cum_ms.append((t_now - t_first) * 1000 / len(frame_ms))
            t_prev = t_now
            outputs = out["outputs"]
            if not printed_keys:
                print(
                    f"propagate output keys {sorted(outputs.keys())}",
                    flush=True,
                )
                printed_keys = True
            for key in ("out_obj_ids", "obj_ids"):
                if key in outputs:
                    obj_counts.append(len(outputs[key]))
                    break
        wall = time.perf_counter() - wall0
        video_predictor.handle_request(
            {"type": "close_session", "session_id": sid}
        )
        med, p95 = _stats(cum_ms)
        yield_med, yield_p95 = _stats(frame_ms)
        med_objs = (
            sorted(obj_counts)[len(obj_counts) // 2] if obj_counts else -1
        )
        max_objs = max(obj_counts) if obj_counts else -1
        return {
            "concept": text,
            "frames": len(frame_ms),
            "wall_s": wall,
            "ms_med": med,
            "ms_p95": p95,
            "ms_avg": wall * 1000 / len(frame_ms),
            "yield_ms_med": yield_med,
            "yield_ms_p95": yield_p95,
            "obj_med": med_objs,
            "obj_max": max_objs,
        }

    torch.cuda.reset_peak_memory_stats()
    video_stats = []
    t_vid_wall0 = time.time()
    for text in PROMPTS:
        st = run_concept(text)
        video_stats.append(st)
        print(
            f"VIDEO_MODE {text} frames {st['frames']} wall_s "
            f"{st['wall_s']:.2f} ms_avg {st['ms_avg']:.2f} "
            f"ms_med {st['ms_med']:.2f} "
            f"ms_p95 {st['ms_p95']:.2f} fps {1000 / st['ms_avg']:.2f} "
            f"yield_ms_med {st['yield_ms_med']:.3f} "
            f"yield_ms_p95 {st['yield_ms_p95']:.1f} "
            f"objs_med {st['obj_med']} objs_max {st['obj_max']}",
            flush=True,
        )
    vid_wall = time.time() - t_vid_wall0
    vid_peak = torch.cuda.max_memory_allocated() / 2**30
    print(
        f"VIDEO_MODE_TOTAL wall_s {vid_wall:.2f} peak_mem_gib "
        f"{vid_peak:.2f}",
        flush=True,
    )

    # --- cost -----------------------------------------------------------
    t_fn_end = time.time()
    fn_gpu_s = t_fn_end - t_fn_start
    est_cost = fn_gpu_s * GPU_RATE_USD_PER_S
    print(f"FN_END_EPOCH {t_fn_end:.3f}", flush=True)
    print(
        f"COST_EST fn_gpu_s {fn_gpu_s:.1f} rate_usd_per_s "
        f"{GPU_RATE_USD_PER_S} est_usd {est_cost:.3f} "
        f"(excl. container spin-up/queue)",
        flush=True,
    )

    return {
        "files": files,
        "image": {
            "text_prompt_ms_med": img_med,
            "text_prompt_ms_p95": img_p95,
            "peak_mem_gib": img_peak,
        },
        "video": video_stats,
        "video_total_wall_s": vid_wall,
        "video_peak_mem_gib": vid_peak,
        "fn_gpu_s": fn_gpu_s,
        "est_cost_usd": est_cost,
    }


@app.local_entrypoint()
def main():
    result = run_probe.remote()

    out_dir = (
        Path(__file__).resolve().parents[2] / "data" / "sam31_real"
        if len(Path(__file__).resolve().parents) > 2
        else Path("data/sam31_real")
    )
    out_dir.mkdir(parents=True, exist_ok=True)
    total_bytes = 0
    for relpath, data in result["files"].items():
        dest = out_dir / relpath
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
        total_bytes += len(data)
    print(f"wrote {len(result['files'])} files, {total_bytes / 1e6:.1f} MB")
    print(f"output_dir {out_dir}")
    print("RESULT_JSON " + json.dumps(result["image"]))
    print("RESULT_JSON_VIDEO " + json.dumps(result["video"]))
    print(
        f"EST_COST_USD {result['est_cost_usd']:.3f} "
        f"fn_gpu_s {result['fn_gpu_s']:.1f}"
    )
