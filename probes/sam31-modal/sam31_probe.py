"""SAM 3.1 timing probe on Modal.

Measures (per the probe plan):
  1. install/import success, torch + CUDA versions, GPU name, sam3 commit
  2. cold start: function entry -> model ready
  3. image mode: Sam3Processor.set_image + set_text_prompt("hand") per frame
  4. video mode: SAM 3.1 multiplex predictor session on the 300-frame JPEG dir
  5. peak GPU memory per mode

Weights: random init. The checkpoint download is stubbed out (facebook/sam3.1
is gated and no HF token is provisioned), so all numbers are random-weight
timings, not accuracy.

Run:
    PROBE_GPU=H100 uvx modal run probes/sam31-modal/sam31_probe.py
"""

import os
from pathlib import Path

import modal

SAM3_COMMIT = "f1647e8d0c1d280991f4d1b7d974c2704537fe66"
GPU = os.environ.get("PROBE_GPU", "H100")

FRAMES_REMOTE = "/data/test_frames"
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
    )
    .run_commands("cd /opt/sam3 && pip install -e .")
    .add_local_dir(FRAMES_LOCAL, remote_path=FRAMES_REMOTE)
)

app = modal.App("synthexis-sam31-probe", image=image)


def _stats(times):
    times = sorted(times)
    n = len(times)
    median = times[n // 2] if n % 2 else 0.5 * (times[n // 2 - 1] + times[n // 2])
    p95 = times[min(n - 1, int(0.95 * n))]
    return median, p95


@app.function(gpu=GPU, timeout=3600)
def run_probe():
    import subprocess
    import time

    import torch

    t_fn_start = time.time()
    print(f"FN_START_EPOCH {t_fn_start:.3f}", flush=True)

    # --- 1. install / import info --------------------------------------
    print(f"torch {torch.__version__}", flush=True)
    print(f"torch.version.cuda {torch.version.cuda}", flush=True)
    print(f"gpu_name {torch.cuda.get_device_name(0)}", flush=True)
    sam3_commit = (
        subprocess.check_output(["git", "-C", "/opt/sam3", "rev-parse", "HEAD"])
        .decode()
        .strip()
    )
    print(f"sam3_commit {sam3_commit}", flush=True)

    import sam3.model_builder as mb
    from sam3.model.sam3_image_processor import Sam3Processor

    # facebook/sam3.1 weights are gated; stub the download so the builders
    # construct the full model with random init instead.
    mb.download_ckpt_from_hf = lambda *a, **k: None

    # --- 2. image model build + cold start ------------------------------
    t0 = time.time()
    image_model = mb.build_sam3_image_model(
        checkpoint_path=None,
        load_from_HF=False,
        device="cuda",
        eval_mode=True,
    )
    torch.cuda.synchronize()
    t_img_model = time.time()
    print(f"MODEL_READY_EPOCH {t_img_model:.3f}", flush=True)
    print(
        f"image_model_build_s {t_img_model - t0:.2f} "
        f"fn_entry_to_ready_s {t_img_model - t_fn_start:.2f}",
        flush=True,
    )
    n_params = sum(p.numel() for p in image_model.parameters())
    print(f"image_model_params {n_params / 1e6:.1f}M", flush=True)

    # --- 3. image mode ---------------------------------------------------
    from PIL import Image

    frame_paths = sorted(
        p for p in Path(FRAMES_REMOTE).iterdir() if p.suffix == ".jpg"
    )
    print(f"frames {len(frame_paths)}", flush=True)
    processor = Sam3Processor(image_model, device="cuda")

    def image_one(pil_img):
        # SAM3.1 inference is designed to run under bf16 autocast (the video
        # predictor wraps its calls the same way); without it the ViT hits
        # BFloat16/Float dtype mismatches.
        with torch.autocast(device_type="cuda", dtype=torch.bfloat16):
            state = processor.set_image(pil_img)
            out = processor.set_text_prompt("hand", state)
        return out

    torch.cuda.reset_peak_memory_stats()
    for p in frame_paths[:10]:
        image_one(Image.open(p).convert("RGB"))
    torch.cuda.synchronize()

    img_times, det_counts = [], []
    for p in frame_paths:
        pil_img = Image.open(p).convert("RGB")
        torch.cuda.synchronize()
        ta = time.perf_counter()
        out = image_one(pil_img)
        torch.cuda.synchronize()
        img_times.append((time.perf_counter() - ta) * 1000)
        det_counts.append(int(out["scores"].shape[0]))
    img_med, img_p95 = _stats(img_times)
    img_peak = torch.cuda.max_memory_allocated() / 2**30
    det_counts_sorted = sorted(det_counts)
    print(
        f"IMAGE_MODE ms_med {img_med:.2f} ms_p95 {img_p95:.2f} "
        f"fps {1000.0 / img_med:.2f} det_med "
        f"{det_counts_sorted[len(det_counts_sorted) // 2]} "
        f"peak_mem_gib {img_peak:.2f}",
        flush=True,
    )

    del image_model, processor
    torch.cuda.empty_cache()

    # --- 4. video mode (SAM 3.1 multiplex) --------------------------------
    t0 = time.time()
    video_predictor = mb.build_sam3_multiplex_video_predictor(
        checkpoint_path=None,
        use_fa3=False,
        compile=False,
        warm_up=False,
    )
    torch.cuda.synchronize()
    t_vid_model = time.time()
    print(f"video_predictor_build_s {t_vid_model - t0:.2f}", flush=True)

    # Pinned-commit API mismatch: Sam3BasePredictor.start_session always
    # passes offload_state_to_cpu, but Sam3MultiplexTrackingWithInteractivity
    # .init_state does not accept it. Shim drops the kwarg (we don't want CPU
    # offloading anyway, so behavior is unchanged).
    _orig_init_state = video_predictor.model.init_state

    def _init_state_shim(**kwargs):
        kwargs.pop("offload_state_to_cpu", None)
        return _orig_init_state(**kwargs)

    video_predictor.model.init_state = _init_state_shim

    def run_concept(text):
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
        ta = time.perf_counter()
        n_frames = 0
        obj_counts = []
        printed_keys = False
        for out in video_predictor.handle_stream_request(
            {
                "type": "propagate_in_video",
                "session_id": sid,
                "propagation_direction": "forward",
            }
        ):
            outputs = out["outputs"]
            if not printed_keys:
                print(
                    f"propagate output keys {sorted(outputs.keys())}", flush=True
                )
                printed_keys = True
            n_frames += 1
            for key in ("out_obj_ids", "obj_ids"):
                if key in outputs:
                    obj_counts.append(len(outputs[key]))
                    break
        torch.cuda.synchronize()
        dur = time.perf_counter() - ta
        video_predictor.handle_request(
            {"type": "close_session", "session_id": sid}
        )
        med_objs = sorted(obj_counts)[len(obj_counts) // 2] if obj_counts else -1
        return dur, n_frames, med_objs

    torch.cuda.reset_peak_memory_stats()
    # warmup concept: "hand" once (timed anyway, reported separately)
    dur, nf, mo = run_concept("hand")
    print(
        f"VIDEO_MODE hand frames {nf} total_s {dur:.2f} "
        f"ms_per_frame {1000 * dur / nf:.2f} fps {nf / dur:.2f} "
        f"tracked_objs_med {mo}",
        flush=True,
    )
    vid_peak = torch.cuda.max_memory_allocated() / 2**30

    total_t = 0.0
    for text in ["hand", "face", "mouth", "person"]:
        dur, nf, mo = run_concept(text)
        total_t += dur
        print(
            f"VIDEO_4PROMPT {text} frames {nf} total_s {dur:.2f} "
            f"ms_per_frame {1000 * dur / nf:.2f} fps {nf / dur:.2f} "
            f"tracked_objs_med {mo}",
            flush=True,
        )
    print(f"VIDEO_4PROMPT_TOTAL_s {total_t:.2f}", flush=True)
    vid_peak = max(vid_peak, torch.cuda.max_memory_allocated() / 2**30)
    print(f"VIDEO_MODE peak_mem_gib {vid_peak:.2f}", flush=True)
    print(f"FN_END_EPOCH {time.time():.3f}", flush=True)


@app.local_entrypoint()
def main():
    run_probe.remote()
