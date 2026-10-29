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

ImageLabelerL40S is the image-mode variant: the Sam3Processor image model
runs its backbone once per frame and grounds every prompt on the shared
backbone state — no tracker, no per-prompt video session. It writes the
same raw_<concept>.json contract (obj_id=None, no tracking) plus
"mode": "image". spec["variant"]: "seq" (set_image once per frame, then
set_text_prompt per prompt) or "grouped" (set_image_batch over B frames,
one forward_text over all prompts, one forward_grounding for all
frame x prompt pairs via FindStage img_ids/text_ids).
"""

import modal

SAM3_COMMIT = "f1647e8d0c1d280991f4d1b7d974c2704537fe66"
WEIGHTS_MOUNT = "/weights"
VOL_MOUNT = "/vol"
VOL_FRAMES = f"{VOL_MOUNT}/frames"
VOL_RAW = f"{VOL_MOUNT}/raw"
FUNCTION_TIMEOUT_S = 3600  # ~$3.95 hard cap per container on H100
IMAGE_FUNCTION_TIMEOUT_S = 1800
IMAGE_BATCH = 8

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


class _ImageLabelerBase:
    """Image-mode labeler: backbone once per frame, prompts grounded on it."""

    @modal.enter()
    def load(self):
        import time

        import sam3.model_builder as mb
        from sam3.model.sam3_image_processor import Sam3Processor

        t0 = time.time()
        ckpt_path = mb.download_ckpt_from_hf(version="sam3.1")
        self.model = mb.build_sam3_image_model(
            checkpoint_path=ckpt_path,
            load_from_HF=False,
            device="cuda",
            eval_mode=True,
        )
        self.processor = Sam3Processor(self.model, device="cuda")
        self.build_s = time.time() - t0
        print(
            f"container ready, image model build {self.build_s:.1f}s",
            flush=True,
        )

    @staticmethod
    def _det_json(masks, boxes, scores, mask_utils):
        """masks [N,H,W] bool / boxes [N,4] xyxy px / scores [N]
        -> [{obj_id, score, box_xywh, rle}] (same contract as video)."""
        import numpy as np

        masks = np.asarray(masks).reshape(
            len(scores), *np.asarray(masks).shape[-2:]
        )
        boxes = np.asarray(boxes).reshape(len(scores), 4)
        inst = []
        for i in range(len(scores)):
            m = np.asfortranarray(masks[i].astype(np.uint8))
            rle = mask_utils.encode(m)
            x0, y0, x1, y1 = (float(v) for v in boxes[i])
            inst.append(
                {
                    "obj_id": None,
                    "score": float(np.asarray(scores[i])),
                    "box_xywh": [x0, y0, x1 - x0, y1 - y0],
                    "rle": {
                        "size": list(rle["size"]),
                        "counts": rle["counts"].decode("ascii"),
                    },
                }
            )
        return inst

    def _seq_label(self, frames, prompts, in_dir, frames_json, prompt_gpu_s):
        """Variant A: set_image once per frame, set_text_prompt per prompt."""
        import time

        import torch
        from PIL import Image
        from pycocotools import mask as mask_utils

        backbone_s = 0.0
        for fn in frames:
            pil = Image.open(in_dir / fn).convert("RGB")
            with torch.autocast(device_type="cuda", dtype=torch.bfloat16):
                tb = time.time()
                state = self.processor.set_image(pil)
                torch.cuda.synchronize()
                backbone_s += time.time() - tb
                for p in prompts:
                    tp = time.time()
                    out = self.processor.set_text_prompt(p["prompt"], state)
                    torch.cuda.synchronize()
                    prompt_gpu_s[p["concept"]] += time.time() - tp
                    frames_json[p["concept"]][fn] = self._det_json(
                        out["masks"].cpu().numpy()[:, 0],
                        out["boxes"].float().cpu().numpy(),
                        out["scores"].float().cpu().numpy(),
                        mask_utils,
                    )
        return backbone_s

    def _grounding_batch(self, backbone_out, n_img, prompt_strs, hw_list):
        """One forward_grounding over all (frame, prompt) pairs.

        FindStage carries per-query img_ids/text_ids (sam3 runs the
        encoder/decoder with the query count as its batch dim); the dummy
        geometric prompt is built with num_prompts = n_img * n_prompts.
        Post-processing mirrors Sam3Processor._forward_grounding per query.
        Returns outs[query] -> (masks [N,H,W] bool np, boxes [N,4] xyxy px,
        scores [N]) in (frame-major, prompt-minor) order.
        """
        import torch

        with torch.inference_mode():
            return self._grounding_batch_inner(
                backbone_out, n_img, prompt_strs, hw_list
            )

    def _grounding_batch_inner(
        self, backbone_out, n_img, prompt_strs, hw_list
    ):
        import torch
        from sam3.model import box_ops
        from sam3.model.data_misc import FindStage, interpolate

        n_prompts = len(prompt_strs)
        n_q = n_img * n_prompts
        text_outputs = self.model.backbone.forward_text(
            prompt_strs, device="cuda"
        )
        backbone_out.update(text_outputs)
        find_input = FindStage(
            img_ids=torch.tensor(
                [i for i in range(n_img) for _ in range(n_prompts)],
                device="cuda",
                dtype=torch.long,
            ),
            text_ids=torch.tensor(
                list(range(n_prompts)) * n_img,
                device="cuda",
                dtype=torch.long,
            ),
            input_boxes=None,
            input_boxes_mask=None,
            input_boxes_label=None,
            input_points=None,
            input_points_mask=None,
        )
        outputs = self.model.forward_grounding(
            backbone_out=backbone_out,
            find_input=find_input,
            geometric_prompt=self.model._get_dummy_prompt(num_prompts=n_q),
            find_target=None,
        )
        out_bbox = outputs["pred_boxes"]  # [Q, N, 4] cxcywh normalized
        out_logits = outputs["pred_logits"]  # [Q, N, 1]
        out_masks = outputs["pred_masks"]  # [Q, N, h, w]
        out_probs = out_logits.sigmoid()
        presence = outputs["presence_logit_dec"].sigmoid().unsqueeze(1)
        out_probs = (out_probs * presence).squeeze(-1)  # [Q, N]
        keep = out_probs > self.processor.confidence_threshold

        outs = []
        for q in range(n_q):
            i = q // n_prompts
            k = keep[q]
            scores = out_probs[q][k]
            boxes = box_ops.box_cxcywh_to_xyxy(out_bbox[q][k])
            img_h, img_w = hw_list[i]
            scale = torch.tensor(
                [img_w, img_h, img_w, img_h], device=boxes.device
            )
            boxes = boxes * scale[None, :]
            masks = (
                interpolate(
                    out_masks[q][k].unsqueeze(1),
                    (img_h, img_w),
                    mode="bilinear",
                    align_corners=False,
                )
                .sigmoid()
                > 0.5
            )
            outs.append(
                (
                    masks.squeeze(1).cpu().numpy(),
                    boxes.float().cpu().numpy(),
                    scores.float().cpu().numpy(),
                )
            )
        return outs

    def _grouped_label(self, frames, prompts, in_dir, frames_json, prompt_gpu_s):
        """Variant B: backbone over B-frame batches, one grounding call for
        all frame x prompt pairs."""
        import time

        import torch
        from PIL import Image
        from pycocotools import mask as mask_utils

        # S2 (2026-10-29): 4,260 of 5,039 GPU-seconds went to reading JPEGs
        # one by one from the volume. Read all chunk files in parallel first.
        import io
        from concurrent.futures import ThreadPoolExecutor

        t_io = time.time()
        with ThreadPoolExecutor(32) as ex:
            blobs = dict(
                zip(frames, ex.map(lambda fn: (in_dir / fn).read_bytes(), frames))
            )
        self.io_s = time.time() - t_io

        backbone_s = 0.0
        for b0 in range(0, len(frames), IMAGE_BATCH):
            part = frames[b0 : b0 + IMAGE_BATCH]
            pils = [Image.open(io.BytesIO(blobs[fn])).convert("RGB") for fn in part]
            with torch.autocast(device_type="cuda", dtype=torch.bfloat16):
                tb = time.time()
                state = self.processor.set_image_batch(pils)
                torch.cuda.synchronize()
                backbone_s += time.time() - tb
                tg = time.time()
                outs = self._grounding_batch(
                    state["backbone_out"],
                    len(part),
                    [p["prompt"] for p in prompts],
                    list(
                        zip(
                            state["original_heights"],
                            state["original_widths"],
                        )
                    ),
                )
                torch.cuda.synchronize()
                gdt = time.time() - tg
            for qi, (masks, boxes, scores) in enumerate(outs):
                i, j = divmod(qi, len(prompts))
                frames_json[prompts[j]["concept"]][part[i]] = self._det_json(
                    masks, boxes, scores, mask_utils
                )
            # one call covered every prompt; split its time evenly so the
            # per-concept shares sum to the real GPU time
            for p in prompts:
                prompt_gpu_s[p["concept"]] += gdt / len(prompts)
        return backbone_s

    @modal.method()
    def label_chunk(self, spec: dict) -> dict:
        import json
        import time
        import uuid
        from pathlib import Path

        t_method0 = time.time()
        call_id = uuid.uuid4().hex[:12]
        frames = spec["frames"]
        prompts = spec["prompts"]
        variant = spec.get("variant", "seq")
        self.io_s = 0.0
        in_dir = Path(VOL_FRAMES) / spec["session"] / spec["cam"]
        out_dir = Path(spec["out_dir"])
        out_dir.mkdir(parents=True, exist_ok=True)

        frames_json = {
            p["concept"]: {fn: [] for fn in frames} for p in prompts
        }
        prompt_gpu_s = {p["concept"]: 0.0 for p in prompts}
        if variant == "grouped":
            backbone_s = self._grouped_label(
                frames, prompts, in_dir, frames_json, prompt_gpu_s
            )
        else:
            backbone_s = self._seq_label(
                frames, prompts, in_dir, frames_json, prompt_gpu_s
            )

        gpu_s = time.time() - t_method0
        det_counts = {}
        raws = []
        for p in prompts:
            fj = frames_json[p["concept"]]
            det_counts[p["concept"]] = {
                "frames_with_det": sum(1 for v in fj.values() if v),
                "instances": sum(len(v) for v in fj.values()),
            }
            raws.append(
                {
                    "concept": p["concept"],
                    "cls": p["cls"],
                    "prompt": p["prompt"],
                    "call_id": call_id,
                    "prompt_gpu_s": round(prompt_gpu_s[p["concept"]], 2),
                    "method_gpu_s": round(gpu_s, 2),
                    "mode": "image",
                    "variant": variant,
                    "frames": fj,
                }
            )
        for raw in raws:
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
            "mode": "image",
            "variant": variant,
            "backbone_gpu_s": round(backbone_s, 2),
            "io_s": round(getattr(self, "io_s", 0.0), 2),
        }


@app.cls(
    gpu="L40S",
    timeout=IMAGE_FUNCTION_TIMEOUT_S,
    max_containers=4,
    secrets=[modal.Secret.from_name("huggingface")],
    volumes={WEIGHTS_MOUNT: weights_vol, VOL_MOUNT: recordings_vol},
)
class ImageLabelerL40S(_ImageLabelerBase):
    pass


LABELERS = {"H100": LabelerH100, "L40S": LabelerL40S}
IMAGE_LABELERS = {"L40S": ImageLabelerL40S}
