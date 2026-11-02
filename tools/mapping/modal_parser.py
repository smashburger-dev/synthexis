"""Parser training on a Modal GPU (L4, else A10G or L40S; v7: mmBERT encoder).

One-time setup (data and backbone go to the volume once, never into the image):
    modal volume create synthexis-parser
    modal volume put synthexis-parser data/mapping/train/ds-v7p /train/ds-v7p
    modal volume put synthexis-parser data/mapping/backbones/mmbert-small-de-en-32k \
        /backbones/mmbert-small-de-en-32k
    modal deploy tools/mapping/modal_parser.py
Spawn (a deployed function survives the local client; a `modal run` job dies
with it), then fetch only the run dir:
    python tools/mapping/modal_parser.py spawn p-v7-s0 "--seed 0 ..." ...
    python tools/mapping/modal_parser.py wait <call ids>
    modal volume get synthexis-parser /train/runs/p-v7-s0 data/mapping/train/runs/

Domain MLM post-training of the encoder (v8): upload the corpus to
/train/mlm/, then
    python tools/mapping/modal_parser.py mlm <backbone name> "--corpus ..."
writes /backbones/<name>; fetch it with `modal volume get` into
data/mapping/backbones/ before local decoding.

Cost rules: the image holds only pinned packages and two small source files,
so it builds once and is cached; each spawn is one GPU container that starts
training at once and exits when done (no keep-warm, no idle polling inside
the container); the volume is committed once at the end, also on failure.
"""
import sys
from pathlib import Path

import modal

HERE = Path(__file__).resolve().parent
app = modal.App("synthexis-parser")
vol = modal.Volume.from_name("synthexis-parser", create_if_missing=True)
image = (
    modal.Image.debian_slim(python_version="3.12")
    .pip_install("torch==2.7.0", "transformers==4.57.6", "tokenizers==0.22.2",
                 "sentencepiece==0.2.0", "protobuf==6.31.1", "numpy==2.2.6",
                 "huggingface_hub==0.36.2")
    .env({"SYNTHEXIS_BACKBONES": "/vol/backbones", "PYTHONUNBUFFERED": "1"})
    .add_local_file(str(HERE / "frame_parser.py"),
                    "/app/tools/mapping/frame_parser.py")
    .add_local_file(str(HERE / "train_parser.py"),
                    "/app/tools/mapping/train_parser.py")
    .add_local_file(str(HERE / "mlm_backbone.py"),
                    "/app/tools/mapping/mlm_backbone.py")
)


@app.function(image=image, gpu=["L4", "A10G", "L40S"], timeout=2 * 3600,
              volumes={"/vol": vol})
def train(name: str, args: str) -> dict:
    import json
    import shlex
    import subprocess
    vol.reload()            # a reused container may hold an older volume view
    data = Path("/app/data/mapping")
    data.mkdir(parents=True, exist_ok=True)
    if not (data / "train").exists():
        (data / "train").symlink_to("/vol/train")
    try:
        subprocess.run(["python", "/app/tools/mapping/train_parser.py",
                        "--name", name, "--device", "cuda",
                        *shlex.split(args)], check=True)
    finally:
        vol.commit()
    return json.loads((data / "train" / "runs" / name / "metrics.json")
                      .read_text())


@app.function(image=image, gpu=["L4", "A10G", "L40S"], timeout=2 * 3600,
              volumes={"/vol": vol})
def mlm(name: str, args: str) -> dict:
    """Domain MLM post-training (mlm_backbone.py train) onto the volume."""
    import json
    import shlex
    import subprocess
    vol.reload()
    data = Path("/app/data/mapping")
    data.mkdir(parents=True, exist_ok=True)
    if not (data / "train").exists():
        (data / "train").symlink_to("/vol/train")
    try:
        subprocess.run(["python", "/app/tools/mapping/mlm_backbone.py",
                        "train", "--name", name, *shlex.split(args)],
                       check=True)
    finally:
        vol.commit()
    return json.loads(Path(f"/vol/backbones/{name}/mlm.json").read_text())


@app.function(image=image, timeout=30 * 60, volumes={"/vol": vol})
def pull(repo: str, name: str, revision: str) -> str:
    """CPU only: a Hugging Face backbone straight onto the volume."""
    from huggingface_hub import snapshot_download
    path = snapshot_download(repo, revision=revision,
                             local_dir=f"/vol/backbones/{name}",
                             allow_patterns=["*.json", "*.safetensors",
                                             "pytorch_model.bin"])
    vol.commit()
    return path


def main() -> None:
    fn = modal.Function.from_name("synthexis-parser", "train")
    if sys.argv[1] == "fetch":
        f = modal.Function.from_name("synthexis-parser", "pull")
        print(f.remote(*sys.argv[2:5]))
    elif sys.argv[1] in ("spawn", "mlm"):
        if sys.argv[1] == "mlm":
            fn = modal.Function.from_name("synthexis-parser", "mlm")
        it = iter(sys.argv[2:])
        for name, args in zip(it, it):
            print(name, fn.spawn(name, args).object_id)
    elif sys.argv[1] == "wait":
        for cid in sys.argv[2:]:
            m = modal.FunctionCall.from_id(cid).get()
            print(cid, {k: m[k]["route_edit"]["acc"] for k in m
                        if isinstance(m[k], dict) and "route_edit" in m[k]})


if __name__ == "__main__":
    main()
