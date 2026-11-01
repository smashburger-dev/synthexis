# /// script
# requires-python = ">=3.11"
# dependencies = ["torch", "safetensors", "numpy"]
# ///
"""Export parser runs for the plugin runtime (plugin/parser, Rust).

  uv run tools/mapping/export_runtime.py --run p-v7a-s0,p-v7a-s1

Per run writes into data/mapping/runtime/<run>/:
  model.safetensors  the f16 state dict of model.f16.pt, same keys
  parser.json        config, frame vocab and decode limits from meta.json
and copies the backbone's tokenizer.json and config.json once into
data/mapping/runtime/ (shared by all runs of one backbone).
"""

import argparse
import json
import shutil
import sys
from pathlib import Path

import torch
from safetensors.torch import save_file

sys.path.insert(0, str(Path(__file__).resolve().parent))
import frame_parser as fp  # noqa: E402

DATA = Path(__file__).resolve().parents[2] / "data" / "mapping"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    a = ap.parse_args()
    out_root = DATA / "runtime"
    backbone = None
    for name in a.run.split(","):
        run = DATA / "train" / "runs" / name
        meta = json.loads((run / "meta.json").read_text())
        cfg = meta["config"]
        assert backbone in (None, cfg["hf"]), "runs use different backbones"
        backbone = cfg["hf"]
        sd = torch.load(run / "model.f16.pt", map_location="cpu")
        sd = {k: v.contiguous() for k, v in sd.items()}
        out = out_root / name
        out.mkdir(parents=True, exist_ok=True)
        save_file(sd, out / "model.safetensors")
        (out / "parser.json").write_text(json.dumps({
            "run": name, "config": cfg, "frame_vocab": meta["frame_vocab"],
            "max_src": meta.get("max_src", fp.HF_MAX_SRC),
            "max_steps": meta.get("max_steps", fp.MAX_STEPS),
            "max_span": meta.get("max_span", fp.MAX_SPAN),
            "ptr_kinds": list(fp.PTR_KINDS), "special": fp.SPECIAL,
        }, ensure_ascii=False, indent=1))
        n = sum(v.numel() for v in sd.values())
        print(f"{name}: {len(sd)} tensors, {n / 1e6:.2f}M params, "
              f"{(out / 'model.safetensors').stat().st_size / 1e6:.1f} MB")
    for f in ("tokenizer.json", "config.json"):
        shutil.copyfile(fp.backbone_dir(backbone) / f, out_root / f)


if __name__ == "__main__":
    main()
