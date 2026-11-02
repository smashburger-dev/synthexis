# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Extract 15 fps frames per camera from a recording session.

Reads  <session>/cam<N>.mov, segments.json, session.json
Writes data/frames/<session_name>/<cam>/frame_%06d.jpg (native res,
-q:v 2) and data/frames/<session_name>/<cam>/frames_index.json.

Frame times come from the actual output pts (parsed from ffmpeg's
showinfo filter), offset by the container start_time so t=0 is the
first frame — the same "movie time" segments.json uses.

Index entry per kept frame:
  {idx, file, t, segment_id, labels, label_ok}
Frames inside `bad` segments are dropped entirely (not indexed, counted
in stats.dropped_bad). Frames inside margins/gaps keep their entry but
get labels=None and label_ok=false. So do count segments (`*_count*`):
they are acted out as counting up from one finger, so the target finger
state holds only at the end, not across the segment.

Rerunning on an extracted session reuses the jpegs and re-labels the
index with the current rules.

Usage: uv run tools/labels/extract.py <session_dir> [--fps 15]
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

FFMPEG = "/opt/homebrew/bin/ffmpeg"
FFPROBE = "/opt/homebrew/bin/ffprobe"
RE_PTS = re.compile(r"pts_time:([0-9.]+)")
RE_FRAME = re.compile(r"n:\s*(\d+)")


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def movie_start_time(mov: Path) -> float:
    out = subprocess.check_output(
        [
            FFPROBE, "-v", "error",
            "-show_entries", "format=start_time",
            "-of", "json", str(mov),
        ]
    )
    return float(json.loads(out)["format"]["start_time"])


def extract_cam(mov: Path, out_dir: Path, fps: int) -> list[dict]:
    """Extract frames; return per-frame {idx, file, t} using output pts."""
    out_dir.mkdir(parents=True, exist_ok=True)
    pattern = out_dir / "frame_%06d.jpg"
    cmd = [
        FFMPEG, "-hide_banner", "-y",
        "-i", str(mov),
        "-vf", f"fps={fps},showinfo",
        "-q:v", "2",
        str(pattern),
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        raise SystemExit(f"ffmpeg failed on {mov}")

    # showinfo lines appear once per output frame, in order.
    pts, frame_files = [], []
    n = 0
    for line in proc.stderr.splitlines():
        if "Parsed_showinfo" in line and "pts_time:" in line:
            pts.append(float(RE_PTS.search(line).group(1)))
            n += 1
    files = sorted(p.name for p in out_dir.glob("frame_*.jpg"))
    if len(files) != len(pts):
        raise SystemExit(
            f"{mov}: {len(files)} jpegs written but {len(pts)} "
            "showinfo pts seen — cannot map frame times"
        )
    start = movie_start_time(mov)
    first_pts = pts[0] if pts else start
    print(
        f"  {mov.name}: start_time {start:.4f}, first output pts_time "
        f"{first_pts:.4f} -> t = pts_time - start_time "
        f"(first frame t={first_pts - start:.4f})"
    )
    frames = [
        {"idx": i, "file": fn, "t": round(p - start, 6)}
        for i, (fn, p) in enumerate(zip(files, pts))
    ]
    return frames


def load_session(session_dir: Path):
    session_meta = {}
    segments = []
    sj = session_dir / "session.json"
    if sj.exists():
        session_meta = json.loads(sj.read_text())
    seg = session_dir / "segments.json"
    if seg.exists():
        segments = json.loads(seg.read_text())
    return session_meta, segments


def index_frames(
    frames: list[dict], segments: list[dict], cam: str = "cam0"
) -> tuple[list[dict], int]:
    """Attach segment/labels/label_ok. Returns (kept_entries, dropped_bad).

    Segment times are per camera (start_s_cam<N>), each in that movie's timeline.
    """
    kept, dropped = [], 0
    k0, k1 = f"start_s_{cam}", f"end_s_{cam}"
    for f in frames:
        t = f["t"]
        seg = next(
            (
                s
                for s in segments
                if s[k0] <= t < s[k1]
            ),
            None,
        )
        if seg is not None and seg.get("bad"):
            dropped += 1
            continue
        if seg is None:
            kept.append({**f, "segment_id": None, "labels": None, "label_ok": False})
            continue
        m0, m1 = seg.get("margin_s", [0, 0])
        label_ok = ((seg[k0] + m0) <= t <= (seg[k1] - m1)
                    and "_count" not in seg["id"])
        kept.append(
            {
                **f,
                "segment_id": seg["id"],
                "labels": seg.get("labels") if label_ok else None,
                "label_ok": bool(label_ok),
            }
        )
    return kept, dropped


def boundary_check(frames: list[dict], segments: list[dict], fps: int, cam: str):
    """Print how each segment's movie-time range maps onto frames."""
    for s in segments:
        lo, hi = s[f"start_s_{cam}"], s[f"end_s_{cam}"]
        inside = [f for f in frames if lo <= f["t"] < hi]
        expected = round((hi - lo) * fps)
        if not inside:
            print(f"  CHECK {cam} {s['id']}: NO FRAMES in [{lo:.3f},{hi:.3f})")
            continue
        first, last = inside[0], inside[-1]
        ok = abs(len(inside) - expected) <= 2 and first["t"] - lo < 2.0 / fps
        flag = "OK " if ok else "WARN"
        print(
            f"  CHECK {cam} {s['id']} [{lo:.3f},{hi:.3f}): "
            f"{len(inside)} frames (expected ~{expected}), "
            f"first t={first['t']:.3f} (+{first['t'] - lo:.3f}s), "
            f"last t={last['t']:.3f} (-{hi - last['t']:.3f}s) {flag}"
        )


def extract_session(
    session_dir: Path, frames_root: Path, fps: int, session_name: str = ""
) -> dict:
    session_dir = session_dir.resolve()
    session_name = session_name or session_dir.name
    meta, segments = load_session(session_dir)
    movs = sorted(session_dir.glob("cam*.mov"))
    if not movs:
        raise SystemExit(f"no cam*.mov in {session_dir}")
    stats = {"session": session_name, "fps": fps, "cams": {}}
    for mov in movs:
        cam = mov.stem
        out_dir = frames_root / session_name / cam
        existing = sorted(out_dir.glob("frame_*.jpg")) if out_dir.is_dir() else []
        idx_path = out_dir / "frames_index.json"
        if existing and idx_path.exists():
            print(f"  {cam}: {len(existing)} frames already extracted, reusing")
            index = json.loads(idx_path.read_text())
            base = [{k: f[k] for k in ("idx", "file", "t")}
                    for f in index["frames"]]
            frames, _ = index_frames(base, segments, cam)
            dropped = index["stats"]["dropped_bad"]
            kept = len(frames)
            index["frames"] = frames
            index["stats"]["label_ok"] = sum(1 for f in frames if f["label_ok"])
            idx_path.write_text(json.dumps(index, indent=1))
        else:
            frames_all = extract_cam(mov, out_dir, fps)
            frames, dropped = index_frames(frames_all, segments, cam)
            kept = len(frames)
            index = {
                "session": session_name,
                "cam": cam,
                "fps": fps,
                "frames": frames,
                "stats": {
                    "extracted": len(frames_all),
                    "kept": kept,
                    "dropped_bad": dropped,
                    "label_ok": sum(1 for f in frames if f["label_ok"]),
                },
            }
            idx_path.write_text(json.dumps(index, indent=1))
            boundary_check(frames_all, segments, fps, cam)
        stats["cams"][cam] = {
            "kept": kept,
            "dropped_bad": dropped,
            "dir": str(out_dir),
        }
        print(f"  {cam}: kept {kept} frames, dropped {dropped} (bad segments)")
    return stats


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("session_dir", type=Path)
    ap.add_argument("--fps", type=int, default=15)
    args = ap.parse_args()
    stats = extract_session(
        args.session_dir, repo_root() / "data" / "frames", args.fps
    )
    print(json.dumps(stats, indent=1))


if __name__ == "__main__":
    main()
