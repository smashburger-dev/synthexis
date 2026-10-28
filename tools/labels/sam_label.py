# /// script
# requires-python = ">=3.11"
# dependencies = ["modal>=1.6"]
# ///
"""SAM 3.1 labeling worker on Modal — GPU does only SAM.

One container builds the multiplex video predictor once (@modal.enter);
run.py drives chunks with .spawn (pilot + in-flight budget guards) and
may fan out up to max_containers. Each call labels ONE chunk for the
prompts in the spec (a subset when resuming): frames are symlinked into
a session dir, one video session propagates per prompt, raw instances
are encoded as COCO RLE and written as raw_<concept>.json per concept
to the synthexis-recordings volume. All filtering/painting happens
locally in tools/labels/postprocess.py.

Two class variants expose the GPU choice: LabelerH100 / LabelerL40S.
"""

import modal

SAM3_COMMIT = "f1647e8d0c1d280991f4d1b7d974c2704537fe66"
WEIGHTS_MOUNT = "/weights"
VOL_MOUNT = "/vol"
VOL_FRAMES = f"{VOL_MOUNT}/frames"
VOL_RAW = f"{VOL_MOUNT}/raw"
FUNCTION_TIMEOUT_S = 3600  # ~$3.95 hard cap per container on H100

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
        "numpy<2",
    )
    .run_commands("cd /opt/sam3 && pip install -e .")
    .env({"HF_HUB_CACHE": f"{WEIGHTS_MOUNT}/hf-cache"})
)

weights_vol = modal.Volume.from_name("sam31-weights", create_if_missing=True)
recordings_vol = modal.Volume.from_name(
    "synthexis-recordings", create_if_missing=True
)

app = modal.App("synthexis-sam-label", image=image)


class _LabelerBase:
    @modal.enter()
    def load(self):
        import time

        import sam3.model_builder as mb

        t0 = time.time()
        ckpt_path = mb.download_ckpt_from_hf(version="sam3.1")
        self.predictor = mb.build_sam3_multiplex_video_predictor(
            checkpoint_path=ckpt_path,
            use_fa3=False,
            compile=False,
            warm_up=False,
        )
        # pinned-commit shim (see probes/sam31-modal): start_session
        # always passes offload_state_to_cpu but the multiplex
        # init_state doesn't accept it.
        _orig_init_state = self.predictor.model.init_state

        def _init_state_shim(**kwargs):
            kwargs.pop("offload_state_to_cpu", None)
            return _orig_init_state(**kwargs)

        self.predictor.model.init_state = _init_state_shim
        self.build_s = time.time() - t0
        print(f"container ready, model build {self.build_s:.1f}s", flush=True)

    def _run_prompt_session(self, chunk_dir, frame_names, text):
        """Propagate one text prompt; return {frame_name: outputs}."""
        import torch

        resp = self.predictor.handle_request(
            {"type": "start_session", "resource_path": str(chunk_dir)}
        )
        sid = resp["session_id"]
        self.predictor.handle_request(
            {
                "type": "add_prompt",
                "session_id": sid,
                "frame_index": 0,
                "text": text,
            }
        )
        out_by_frame = {}
        for out in self.predictor.handle_stream_request(
            {
                "type": "propagate_in_video",
                "session_id": sid,
                "propagation_direction": "forward",
            }
        ):
            idx = out["frame_index"]
            if idx < len(frame_names):
                out_by_frame[frame_names[idx]] = out["outputs"]
        self.predictor.handle_request(
            {"type": "close_session", "session_id": sid}
        )
        torch.cuda.synchronize()
        return out_by_frame

    @staticmethod
    def _instances_rle(outputs, mask_utils):
        """propagate outputs -> [{rle, box_xywh, score, obj_id}]."""
        import numpy as np

        ids = outputs.get("out_obj_ids")
        masks = outputs.get("out_binary_masks")
        boxes = outputs.get("out_boxes_xywh")
        probs = outputs.get("out_probs")
        if ids is None or masks is None or len(ids) == 0:
            return []
        masks = np.asarray(masks).reshape(
            len(ids), *np.asarray(masks).shape[-2:]
        )
        inst = []
        for i in range(len(ids)):
            m = np.asfortranarray(masks[i].astype(np.uint8))
            rle = mask_utils.encode(m)
            inst.append(
                {
                    "obj_id": int(np.asarray(ids[i])),
                    "score": (
                        float(np.asarray(probs[i]))
                        if probs is not None
                        else None
                    ),
                    "box_xywh": (
                        [float(v) for v in np.asarray(boxes[i])]
                        if boxes is not None
                        else None
                    ),
                    "rle": {
                        "size": list(rle["size"]),
                        "counts": rle["counts"].decode("ascii"),
                    },
                }
            )
        return inst

    @modal.method()
    def label_chunk(self, spec: dict) -> dict:
        import json
        import shutil
        import time
        import uuid
        from pathlib import Path

        import torch
        from pycocotools import mask as mask_utils

        t_method0 = time.time()
        call_id = uuid.uuid4().hex[:12]
        frames = spec["frames"]
        prompts = spec["prompts"]  # [{concept, prompt, cls}]
        in_dir = Path(VOL_FRAMES) / spec["session"] / spec["cam"]
        out_dir = Path(spec["out_dir"])
        out_dir.mkdir(parents=True, exist_ok=True)

        chunk_dir = Path(
            f"/tmp/chunk_{spec['session']}_{spec['cam']}_{spec['chunk']}"
        )
        if chunk_dir.exists():
            shutil.rmtree(chunk_dir)
        chunk_dir.mkdir(parents=True)
        for fn in frames:
            src = in_dir / fn
            dst = chunk_dir / fn
            try:
                dst.symlink_to(src)
            except OSError:
                shutil.copy2(src, dst)

        det_counts = {}
        prompt_gpu_s = {}
        raws = []
        for p in prompts:
            t0 = time.time()
            outs = self._run_prompt_session(chunk_dir, frames, p["prompt"])
            torch.cuda.synchronize()
            prompt_gpu_s[p["concept"]] = time.time() - t0
            frames_json = {}
            n_inst = 0
            n_det_frames = 0
            for fn in frames:
                inst = self._instances_rle(outs.get(fn, {}), mask_utils)
                frames_json[fn] = inst
                n_inst += len(inst)
                n_det_frames += 1 if inst else 0
            det_counts[p["concept"]] = {
                "frames_with_det": n_det_frames,
                "instances": n_inst,
            }
            raws.append(
                {
                    "concept": p["concept"],
                    "cls": p["cls"],
                    "prompt": p["prompt"],
                    "call_id": call_id,
                    "prompt_gpu_s": round(prompt_gpu_s[p["concept"]], 2),
                    "frames": frames_json,
                }
            )
        gpu_s = time.time() - t_method0
        for raw in raws:
            raw["method_gpu_s"] = round(gpu_s, 2)
            (out_dir / f"raw_{raw['concept']}.json").write_text(
                json.dumps(raw)
            )
        recordings_vol.commit()

        n_prompt_frames = len(frames) * len(prompts)
        return {
            "chunk": spec["chunk"],
            "cam": spec["cam"],
            "call_id": call_id,
            "frames": len(frames),
            "prompts": len(prompts),
            "gpu_s": round(gpu_s, 2),
            "ms_per_frame_concept": round(1000 * gpu_s / n_prompt_frames, 2),
            "prompt_gpu_s": {k: round(v, 2) for k, v in prompt_gpu_s.items()},
            "det_counts": det_counts,
            "container_build_s": round(self.build_s, 2),
        }


@app.cls(
    gpu="H100",
    timeout=FUNCTION_TIMEOUT_S,
    max_containers=4,
    secrets=[modal.Secret.from_name("huggingface")],
    volumes={WEIGHTS_MOUNT: weights_vol, VOL_MOUNT: recordings_vol},
)
class LabelerH100(_LabelerBase):
    pass


@app.cls(
    gpu="L40S",
    timeout=FUNCTION_TIMEOUT_S,
    max_containers=4,
    secrets=[modal.Secret.from_name("huggingface")],
    volumes={WEIGHTS_MOUNT: weights_vol, VOL_MOUNT: recordings_vol},
)
class LabelerL40S(_LabelerBase):
    pass


LABELERS = {"H100": LabelerH100, "L40S": LabelerL40S}
