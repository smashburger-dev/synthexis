"""Scales the raw window captures for the README and writes them to docs/img.

python3 resize.py <capture dir> <docs/img>

The hero keeps 1200 px width, the gallery shots 960 px. pngquant shrinks
the files when it is installed.
"""
import shutil
import subprocess
import sys
from pathlib import Path

from PIL import Image

WIDTHS = {"hero": 1200, "hands": 960, "routes": 960, "head": 960}


def main(src: Path, dst: Path) -> None:
    dst.mkdir(parents=True, exist_ok=True)
    for name, width in WIDTHS.items():
        im = Image.open(src / f"{name}.png")
        im = im.resize((width, round(im.height * width / im.width)), Image.LANCZOS)
        out = dst / f"{name}.png"
        im.save(out, optimize=True)
        if shutil.which("pngquant"):
            subprocess.run(["pngquant", "--force", "--skip-if-larger", "--quality=80-95",
                            "--output", str(out), str(out)], check=False)
        print(out, im.size, out.stat().st_size // 1024, "KB")


if __name__ == "__main__":
    main(Path(sys.argv[1]), Path(sys.argv[2]))
