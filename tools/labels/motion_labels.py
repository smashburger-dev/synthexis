# /// script
# requires-python = ">=3.11"
# dependencies = ["httpx>=0.27", "numpy", "pillow"]
# ///
"""motion.labels: Muse Spark Open freestyle timelines and segment QA.

  uv run tools/labels/motion_labels.py <session> [--max-usd 15]
      [--passes 1|2] [--only qa|timeline] [--limit-jobs N] [--dry-run]
  uv run tools/labels/motion_labels.py report <session> [<session> ...]

<session> is a name under data/recordings (e.g. 2026-10-29-S2). Needs
the 15 fps frames (data/frames) and the SAM labels (data/labels) of
that session.

Two job kinds, both sent through tools/mapping/spark.py (ledger, cache,
cost guards):

  timeline  every freestyle segment (block G) cut into 10 s windows,
            frames at 2 fps (20 images per request); the reply is a list
            of events in the motion.primitives vocabulary plus a
            free-text name per event.
  qa        3 frames per scripted segment (20/50/80 % of the active
            span), 8 segments per request; the reply says per segment
            whether the frames match its instruction and labels.

Frames are 768x432 JPEGs with SAM masks overlaid (hand green, face
blue outline, object orange; person and eye/mouth not painted) and a
yellow dotted trail of hand-box centres over the previous 0.5 s from
the 15 fps SAM labels, so the 2 fps frames still show direction and
speed.

Budget: --max-usd caps the sum of all motion-* runs in the ledger, so
reruns and later sessions share one cap. spark.run adds the up-front
estimate, pilot re-estimate and rolling stop on top.

Outputs (data/motion_labels/<session>/, never committed; faces):
  frames/       the exact images sent
  timeline.json events per freestyle window (pass a, plus pass b
                agreement when --passes 2)
  qa.json       per scripted segment verdict, mismatches flagged
  review.html   spot-check page for Noa
"""

import argparse
import base64
import io
import json
import re
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools" / "mapping"))
import spark  # noqa: E402

spark.IMAGE_TOKENS_EST = 1750   # 2x2 grid of 768x432 ~ 1,790 tokens

DATA = ROOT / "data"
OUT_ROOT = DATA / "motion_labels"
PROTOCOL = ROOT / "spec" / "recording-protocol.json"
OBJECTS = ROOT / "tools" / "labels" / "objects.json"
CAM = "cam0"

W, H = 768, 432
JPEG_Q = 82
STRIP_FPS = 2.0
WINDOW_S = 10.0
QA_POINTS = (0.2, 0.5, 0.8)
# Baseten rejects more than 8 images per request; billing is per 28x28
# patch, so tiles cost the same as single frames (no internal downscale
# measured at 1536x864 and 2304x432)
QA_PER_JOB = 8             # one 3-frame row per segment
TL_TILE = (2, 2)           # timeline: 20 frames -> 5 grids of 2x2
TRAIL_S = 0.5
EST_OUT_TOKENS = 5800      # medium reasoning, budget probe scenario B

# semantic class ids from tools/labels/postprocess.py
SEM_COLORS = {2: (60, 120, 255), 5: (40, 230, 90), 6: (255, 150, 30)}
SEM_ALPHA = {5: 0.45, 6: 0.45}
OUTLINE_ONLY = {2}         # face: outline, so expressions stay visible

PRIMITIVES = [
    "throw", "swipe", "push_pull", "hold", "pinch", "circle",
    "grab_release", "stretch", "orb", "clap", "face_head", "other",
]
HANDS = ["R", "L", "both", "none"]

COMMON = """You label video frames of one person in front of a webcam for synthexis, an FL Studio plugin that turns hand and face movement into music parameters.

How to read the frames:
- The video is NOT mirrored. The person's RIGHT hand appears on the LEFT side of the image, the LEFT hand on the right side. Always report the person's anatomical side (R = their right hand).
- Directions (left, right, up, down, top border, ...) are in IMAGE coordinates as shown: "right" means towards the right edge of the image.
- Overlays come from an automatic segmenter and can be wrong or missing: green fill = hand mask, blue outline = face, orange fill = held object. A white ring marks each hand centre in this frame. Yellow dots mark where hand centres were during the previous 0.5 s (small and dark = older, large and bright = recent); they show the direction and speed of the movement that ended in this frame. Trust the pixels over the overlay.
- Every image has its id and time burned into the top-left corner.
Reply with one JSON object only, no prose before or after it."""

TIMELINE_SYS = COMMON + """

Task: write a timeline of what the hands (and, in face freestyle, the face and head) do across the frames you get. Frames are 0.5 s apart, so fast movements show up as motion blur, a big jump between frames or a long trail.

Primitive vocabulary (pick the closest; use "other" when none fits and describe it in name):
- throw: fast hand movement that ends in a release (hand opens) or a stop, like throwing a ball; params: direction (8 sectors: up, up_right, right, down_right, down, down_left, left, up_left), border (the image border the throw points at: top, right, bottom, left), speed (slow, medium, fast)
- swipe: sustained movement of an open hand without a release; params: direction (8 sectors), speed, distance (short, medium, long)
- push_pull: hand moves towards the camera (grows) or away (shrinks); params: direction (push or pull)
- hold: hand stays still for at least 0.3 s; params: position (rough image area, e.g. "upper left"), finger_state (fist, open, index only, peace, count of fingers, ...)
- pinch: thumb and index tip together, often dragged; params: drag (direction or "none")
- circle: hand traces a circle; params: rotation (cw or ccw as seen in the image), radius (small, medium, large)
- grab_release: hand closes (grab) or opens (release) in place; params: kind (grab or release)
- stretch: both hands move apart or together like a rubber band; params: kind (apart or together)
- orb: both palms face each other as if holding a ball; params: action (hold, grow, shrink, rotate, carry, throw)
- clap: hands meet fast; params: none
- face_head: facial expression or head movement (blink, wink, smile, mouth open, nod, head turn or tilt); params: what
- other: anything else (finger play, pointing, drumming in the air, playing an imaginary keyboard ...)

Output format:
{"events": [{"t_start": <s>, "t_end": <s>, "hand": "R|L|both|none", "primitive": "<one of the list>", "params": {...}, "name": "<short free-text name of what the person does, 2-8 words>", "confidence": <0..1>}],
 "summary": "<one sentence on the whole window>"}
Rules: times in seconds as burned into the frames, t_start <= t_end, both inside the window. One event per distinct movement; split repeated throws into separate events. An event cut by the window start or end still gets an entry. If the hands do nothing noteworthy, return an empty events list."""

QA_SYS = COMMON + """

Task: quality check of a scripted recording. Each segment had a German instruction that the person read and followed; you see 3 frames from the middle part of each segment. Decide per segment whether the frames match the instruction and labels.

Label conventions:
- hand_R / hand_L finger states are 5 bits in the order thumb, index, middle, ring, pinky; 1 = extended. "00000" is a fist, "11111" an open hand, "01100" index and middle (peace). "absent" means that hand should be out of the image.
- Other label values name the task (e.g. "open", "spread", "count3", "smile", "mug").
- Movement instructions (wipe, throw, circle ...) cannot be fully judged from 3 frames: flag them only if the frames clearly show something else (wrong hand, no movement at all, hands absent).
- Lighting, distance, blur and the hand moving through the image are expected variation, not errors.
- Block E objects: the session note lists which real object stood in for each label (e.g. mug = soda can). A stand-in is not an error.
- Judge every frame on its own; one wrong frame out of three still means match "no".

Output format:
{"segments": [{"id": "<segment id>", "match": "yes|no|unsure", "frames": ["ok|wrong|unclear", "ok|wrong|unclear", "ok|wrong|unclear"], "issue": "<empty if yes, else what is wrong, short, in English>"}]}
Return exactly one entry per segment id you were given, in the same order."""


# --- session loading ---------------------------------------------------------


def load_session(session: str) -> dict:
    rec = DATA / "recordings" / session
    segs = json.loads((rec / "segments.json").read_text())
    index = json.loads(
        (DATA / "frames" / session / CAM / "frames_index.json").read_text())
    proto = json.loads(PROTOCOL.read_text())["protocols"]
    instr = {}
    for p in proto.values():
        for s in p["segments"]:
            instr.setdefault(s["id"], s)
    labels_cam = DATA / "labels" / session / CAM
    stem_chunk = {}
    for cdir in sorted(labels_cam.glob("chunk_*")):
        for png in cdir.glob("*.png"):
            stem_chunk[png.stem] = cdir
    if not stem_chunk:
        raise SystemExit(f"{session}: no SAM labels under {labels_cam}")
    objs = json.loads(OBJECTS.read_text())
    objmap = {**objs["default"], **objs["sessions"].get(session, {})}
    return {"name": session, "segments": segs, "frames": index["frames"],
            "fps": index["fps"], "instr": instr, "stem_chunk": stem_chunk,
            "objects": objmap, "chunk_cache": {}}


def frame_at(sess: dict, t: float) -> dict:
    i = int(round(t * sess["fps"]))
    return sess["frames"][max(0, min(i, len(sess["frames"]) - 1))]


def hand_centres(sess: dict, stem: str) -> list[tuple[float, float]]:
    cdir = sess["stem_chunk"].get(stem)
    if cdir is None:
        return []
    cache = sess["chunk_cache"]
    if cdir not in cache:
        frames = json.loads((cdir / "chunk.json").read_text())["frames"]
        cache[cdir] = {
            Path(fn).stem: [
                (b[0] + b[2] / 2, b[1] + b[3] / 2)
                for inst in rec.get("instances", [])
                if inst.get("concept") == "hand"
                for b in [inst["box_xywh"]]]
            for fn, rec in frames.items()}
    return cache[cdir].get(stem, [])


# --- rendering -----------------------------------------------------------------


def _font(size: int):
    try:
        return ImageFont.load_default(size=size)
    except TypeError:
        return ImageFont.load_default()


def _eroded(m: np.ndarray, k: int) -> np.ndarray:
    e = m.copy()
    for _ in range(k):
        e = e & np.roll(e, 1, 0) & np.roll(e, -1, 0) \
            & np.roll(e, 1, 1) & np.roll(e, -1, 1)
    return e


def render(sess: dict, t: float, label: str) -> Image.Image:
    """Frame nearest to t (session seconds), overlays, burned-in label."""
    f = frame_at(sess, t)
    stem = Path(f["file"]).stem
    img = Image.open(DATA / "frames" / sess["name"] / CAM / f["file"])
    base = np.asarray(img.convert("RGB")).astype(np.float32)
    src_h, src_w = base.shape[:2]
    cdir = sess["stem_chunk"].get(stem)
    if cdir is not None:
        sem = np.asarray(Image.open(cdir / f"{stem}.png"))
        if sem.shape[:2] == base.shape[:2]:
            for cid, rgb in SEM_COLORS.items():
                m = sem == cid
                if cid in OUTLINE_ONLY:
                    m = m & ~_eroded(m, 3)
                    a = 1.0
                else:
                    a = SEM_ALPHA[cid]
                base[m] = (1 - a) * base[m] + a * np.array(rgb, np.float32)
    out = Image.fromarray(base.astype(np.uint8)).resize((W, H), Image.BILINEAR)
    d = ImageDraw.Draw(out)
    sx, sy = W / src_w, H / src_h
    n = int(TRAIL_S * sess["fps"])
    for k in range(n, 0, -1):
        prev = frame_at(sess, t - k / sess["fps"])
        a = 1 - k / (n + 1)
        r = 1.5 + 2.5 * a
        for cx, cy in hand_centres(sess, Path(prev["file"]).stem):
            d.ellipse([cx * sx - r, cy * sy - r, cx * sx + r, cy * sy + r],
                      fill=(255, int(160 + 90 * a), 0))
    for cx, cy in hand_centres(sess, stem):
        r = 5
        d.ellipse([cx * sx - r, cy * sy - r, cx * sx + r, cy * sy + r],
                  outline=(255, 255, 255), width=2)
    d.rectangle([0, 0, 9 * len(label) + 12, 22], fill=(0, 0, 0))
    d.text((6, 3), label, fill=(255, 255, 255), font=_font(15))
    return out


def tile(imgs: list[Image.Image], cols: int) -> Image.Image:
    rows = -(-len(imgs) // cols)
    g = Image.new("RGB", (cols * W, rows * H))
    for i, im in enumerate(imgs):
        g.paste(im, ((i % cols) * W, (i // cols) * H))
    return g


def jpeg_b64(img: Image.Image, save: Path | None) -> str:
    buf = io.BytesIO()
    img.save(buf, "JPEG", quality=JPEG_Q)
    if save is not None:
        save.parent.mkdir(parents=True, exist_ok=True)
        save.write_bytes(buf.getvalue())
    return "data:image/jpeg;base64," + base64.b64encode(buf.getvalue()).decode()


# --- jobs ----------------------------------------------------------------------


def seg_span(seg: dict) -> tuple[float, float]:
    lo, hi = seg["start_s_cam0"], seg["end_s_cam0"]
    m0, m1 = seg.get("margin_s") or [0.0, 0.0]
    if hi - lo > m0 + m1 + 1.0:
        lo, hi = lo + m0, hi - m1
    return lo, hi


def timeline_jobs(sess: dict, out: Path, suffix: str) -> tuple[list, dict]:
    jobs, meta = [], {}
    for seg in sess["segments"]:
        if seg["block"] != "G":
            continue
        lo, hi = seg["start_s_cam0"], seg["end_s_cam0"]
        instr = sess["instr"].get(seg["id"], {}).get("instruction_de", "")
        k = 0
        w0 = lo
        while w0 < hi - 1.0:
            w1 = min(w0 + WINDOW_S, hi)
            jid = f"tl-{seg['id']}-w{k}"
            parts = [{"type": "text", "text": (
                f"Session {sess['name']}, freestyle segment {seg['id']}. "
                f"Instruction the person read (German): \"{instr}\". "
                f"Window {w0 - lo:.1f}-{w1 - lo:.1f} s of the segment; "
                f"times are seconds since segment start. Frames at 2 fps "
                f"follow as grids of {TL_TILE[0]}x{TL_TILE[1]}.")}]
            ts = []
            t = w0 + 0.25
            while t < w1:
                ts.append(round(t - lo, 2))
                t += 1 / STRIP_FPS
            per = TL_TILE[0] * TL_TILE[1]
            for g in range(0, len(ts), per):
                group = ts[g:g + per]
                imgs = [render(sess, lo + rel, f"#{g + i:02d}  t={rel:.2f}s")
                        for i, rel in enumerate(group)]
                url = jpeg_b64(tile(imgs, TL_TILE[0]),
                               out / "frames" / f"{jid}-g{g // per}.jpg")
                parts += [{"type": "text", "text": (
                    f"Grid of frames #{g:02d}-#{g + len(group) - 1:02d} "
                    f"(t = {group[0]:.2f}-{group[-1]:.2f} s), read left to "
                    "right, top to bottom:")},
                          {"type": "image_url", "image_url": {"url": url}}]
            parts.append({"type": "text", "text": (
                f"Write the timeline for t = {w0 - lo:.1f} to "
                f"{w1 - lo:.1f} s as JSON.{suffix}")})
            jobs.append(spark.Job(
                id=jid, est_out_tokens=EST_OUT_TOKENS,
                messages=[{"role": "system", "content": TIMELINE_SYS},
                          {"role": "user", "content": parts}]))
            meta[jid] = {"segment": seg["id"], "seg_start_s": round(lo, 3),
                         "w0": round(w0 - lo, 2),
                         "w1": round(w1 - lo, 2), "frame_t": ts,
                         "bad": seg.get("bad", False)}
            w0 = w1
            k += 1
    return jobs, meta


def qa_jobs(sess: dict, out: Path, suffix: str) -> tuple[list, dict]:
    scripted = [s for s in sess["segments"] if s["block"] != "G"]
    jobs, meta = [], {}
    for b in range(0, len(scripted), QA_PER_JOB):
        group = scripted[b:b + QA_PER_JOB]
        jid = f"qa-{b // QA_PER_JOB:02d}"
        objs = ", ".join(f"{k} = {v}" for k, v in sess["objects"].items())
        parts = [{"type": "text", "text": (
            f"Session {sess['name']}: {len(group)} segments, 3 frames "
            f"each. Objects actually used in this session (block E): "
            f"{objs}.")}]
        for seg in group:
            p = sess["instr"].get(seg["id"], {})
            lo, hi = seg_span(seg)
            parts.append({"type": "text", "text": (
                f"Segment {seg['id']} (block {seg['block']}). Instruction "
                f"(German): \"{p.get('instruction_de', '?')}\". Labels: "
                f"{json.dumps(seg.get('labels') or p.get('labels') or {})}. "
                "Its 3 frames, left to right:")})
            imgs = [render(sess, lo + q * (hi - lo), f"{seg['id']} f{i + 1}")
                    for i, q in enumerate(QA_POINTS)]
            url = jpeg_b64(tile(imgs, len(imgs)),
                           out / "frames" / f"qa-{seg['id']}.jpg")
            parts.append({"type": "image_url", "image_url": {"url": url}})
            meta[seg["id"]] = {"job": jid, "block": seg["block"],
                               "bad_marked": seg.get("bad", False),
                               "instruction_de": p.get("instruction_de", ""),
                               "labels": seg.get("labels")}
        ids = [s["id"] for s in group]
        parts.append({"type": "text", "text": (
            f"Check these segment ids in order: {', '.join(ids)}. "
            f"Reply as JSON.{suffix}")})
        jobs.append(spark.Job(
            id=jid, est_out_tokens=EST_OUT_TOKENS,
            messages=[{"role": "system", "content": QA_SYS},
                      {"role": "user", "content": parts}]))
        meta[jid] = {"ids": ids}
    return jobs, meta


# --- parsing -------------------------------------------------------------------


def _json_obj(text: str) -> dict:
    m = re.search(r"\{.*\}", text, re.S)
    if not m:
        raise spark.ParseError("no JSON object")
    try:
        return json.loads(m.group(0))
    except json.JSONDecodeError as e:
        raise spark.ParseError(f"bad JSON: {e}")


def parse_timeline(meta: dict):
    def parse(job: spark.Job, text: str):
        obj = _json_obj(text)
        evs = obj.get("events")
        if not isinstance(evs, list):
            raise spark.ParseError("events missing")
        w = meta[job.id]
        clean = []
        for e in evs:
            try:
                t0, t1 = float(e["t_start"]), float(e["t_end"])
            except (KeyError, TypeError, ValueError):
                raise spark.ParseError("event without times")
            prim = e.get("primitive")
            if prim not in PRIMITIVES:
                raise spark.ParseError(f"unknown primitive {prim!r}")
            if t1 < t0 or t0 < w["w0"] - 1 or t1 > w["w1"] + 1:
                raise spark.ParseError(f"times {t0}-{t1} outside window")
            clean.append({
                "t_start": round(t0, 2), "t_end": round(t1, 2),
                "hand": e.get("hand") if e.get("hand") in HANDS else "none",
                "primitive": prim, "params": e.get("params") or {},
                "name": str(e.get("name") or "")[:120],
                "confidence": float(e.get("confidence") or 0)})
        return {"events": clean, "summary": str(obj.get("summary") or "")}
    return parse


def parse_qa(meta: dict):
    def parse(job: spark.Job, text: str):
        obj = _json_obj(text)
        rows = obj.get("segments")
        if not isinstance(rows, list):
            raise spark.ParseError("segments missing")
        want = meta[job.id]["ids"]
        got = {r.get("id"): r for r in rows if isinstance(r, dict)}
        if set(want) - set(got):
            raise spark.ParseError(f"missing ids {sorted(set(want) - set(got))}")
        out = {}
        for sid in want:
            r = got[sid]
            match = r.get("match")
            if match not in ("yes", "no", "unsure"):
                raise spark.ParseError(f"{sid}: match {match!r}")
            out[sid] = {"match": match, "frames": r.get("frames") or [],
                        "issue": str(r.get("issue") or "")[:300]}
        return out
    return parse


# --- event agreement between passes ----------------------------------------------


def _same(a: dict, b: dict) -> bool:
    overlap = min(a["t_end"], b["t_end"]) - max(a["t_start"], b["t_start"])
    return a["primitive"] == b["primitive"] and overlap >= -0.5


def agree(ev_a: list, ev_b: list) -> tuple[list, int]:
    used, hits = set(), 0
    for e in ev_a:
        e["agreed_b"] = False
        for j, f in enumerate(ev_b):
            if j not in used and _same(e, f):
                used.add(j)
                e["agreed_b"] = True
                hits += 1
                break
    return ev_a, hits


# --- main ----------------------------------------------------------------------


def motion_spent() -> float:
    p = spark._ledger_path()
    if not p.exists():
        return 0.0
    total = 0.0
    for line in p.read_text().splitlines():
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        if str(r.get("run", "")).startswith("motion-"):
            total += float(r.get("cost_usd") or 0)
    return total


PASS_SUFFIX = {"a": "", "b": " (second independent reading)"}


def label_session(args) -> None:
    sess = load_session(args.session)
    out = OUT_ROOT / args.session
    out.mkdir(parents=True, exist_ok=True)
    passes = ["a", "b"][:args.passes]
    results = {"timeline": {}, "qa": {}}
    metas = {}
    for kind in ("timeline", "qa"):
        if args.only and args.only != kind:
            continue
        for p in passes:
            build = timeline_jobs if kind == "timeline" else qa_jobs
            jobs, meta = build(sess, out, PASS_SUFFIX[p])
            if args.limit_jobs:
                # evenly spaced, so a pilot covers every block
                step = max(1, -(-len(jobs) // args.limit_jobs))
                jobs = jobs[::step][:args.limit_jobs]
            metas[kind] = meta
            n_img = sum(1 for j in jobs for part in j.messages[1]["content"]
                        if part["type"] == "image_url")
            print(f"[{args.session}] {kind} pass {p}: {len(jobs)} requests, "
                  f"{n_img} images", flush=True)
            if args.dry_run:
                est = sum(spark._estimate_usd(j) for j in jobs)
                print(f"  dry-run: up-front estimate ${est:.3f}")
                continue
            left = args.max_usd - motion_spent()
            if left <= 0:
                raise spark.AbortError(
                    f"motion-* runs already spent ${motion_spent():.2f} "
                    f">= --max-usd {args.max_usd}")
            parse = (parse_timeline if kind == "timeline" else parse_qa)(meta)
            res = spark.run(
                f"motion-{args.session}-{kind}-{p}", jobs, parse,
                max_usd=left, workers=args.workers, pilot=args.pilot,
                reasoning_effort=args.reasoning_effort)
            results[kind][p] = res
    if args.dry_run:
        return
    if results["timeline"]:
        write_timeline(out, sess, metas["timeline"], results["timeline"])
    if results["qa"]:
        write_qa(out, metas["qa"], results["qa"])
    write_review(out)


def write_timeline(out: Path, sess: dict, meta: dict, res: dict) -> None:
    a, b = res.get("a", {}), res.get("b")
    windows, n_a, n_b, hits = [], 0, 0, 0
    for jid, w in meta.items():
        if jid not in a:
            continue
        evs = [dict(e) for e in a[jid]["events"]]
        row = {"job": jid, **w, "summary": a[jid]["summary"], "events": evs}
        n_a += len(evs)
        if b is not None and jid in b:
            evs, h = agree(evs, b[jid]["events"])
            hits += h
            n_b += len(b[jid]["events"])
            row["events_b"] = b[jid]["events"]
        windows.append(row)
    doc = {"session": sess["name"], "strip_fps": STRIP_FPS,
           "window_s": WINDOW_S, "size": [W, H], "model": spark.MODEL,
           "windows": windows, "events_a": n_a}
    if b is not None:
        doc["events_b"] = n_b
        doc["agreement"] = {"matched": hits,
                            "precision_a_vs_b": round(hits / n_a, 3) if n_a else None,
                            "recall_a_vs_b": round(hits / n_b, 3) if n_b else None}
    (out / "timeline.json").write_text(json.dumps(doc, indent=1, ensure_ascii=False))
    print(f"timeline: {len(windows)} windows, {n_a} events (pass a)"
          + (f", {hits} matched in pass b of {n_b}" if b is not None else ""))


def write_qa(out: Path, meta: dict, res: dict) -> None:
    a, b = res.get("a", {}), res.get("b")
    segs = {}
    for sid, m in meta.items():
        if not isinstance(m, dict) or "job" not in m or m["job"] not in a:
            continue
        ra = a[m["job"]][sid]
        row = {**m, "a": ra}
        verdicts = [ra["match"]]
        if b is not None and m["job"] in b:
            row["b"] = b[m["job"]][sid]
            verdicts.append(row["b"]["match"])
        wrong = max(r["frames"].count("wrong") for r in
                    [row["a"]] + ([row["b"]] if "b" in row else []))
        if "no" in verdicts:
            row["flag"] = "mismatch" if wrong >= 2 else "partial"
        else:
            row["flag"] = "unsure" if "unsure" in verdicts else "ok"
        row["passes_agree"] = len(set(verdicts)) == 1
        segs[sid] = row
    flags = {k: sum(1 for r in segs.values() if r["flag"] == k)
             for k in ("ok", "unsure", "partial", "mismatch")}
    (out / "qa.json").write_text(json.dumps(
        {"segments": segs, "counts": flags}, indent=1, ensure_ascii=False))
    print(f"qa: {len(segs)} segments, {flags}")


REVIEW_JS = """
function pick(el){el.parentNode.querySelectorAll('button').forEach(b=>b.classList.remove('on'));el.classList.add('on');}
function dump(){const r={};document.querySelectorAll('[data-k]').forEach(row=>{const on=row.querySelector('button.on');if(on)r[row.dataset.k]=on.textContent;});
const t=JSON.stringify(r);navigator.clipboard.writeText(t).then(()=>{document.getElementById('st').textContent='copied '+Object.keys(r).length+' verdicts';},()=>{document.getElementById('st').textContent=t;});}
"""


def write_review(out: Path) -> None:
    """Static spot-check page: every freestyle window with its events, and
    every QA segment that is not a clean ok, plus a random sample of oks."""
    import random
    h = ["<!doctype html><meta charset=utf-8><title>motion.labels review</title>",
         "<style>body{font:14px system-ui;background:#111;color:#ddd;margin:16px}"
         "img.g{width:49%;margin:1px}img.r{width:98%}h2{margin-top:32px}.ev{margin:2px 0}"
         "button{background:#333;color:#ddd;border:1px solid #555;margin-left:4px}"
         "button.on{background:#2a6;color:#fff}.flag{color:#f84}.u{color:#fc4}"
         ".strip{display:flex;flex-wrap:wrap}</style>",
         f"<script>{REVIEW_JS}</script>",
         f"<h1>{out.name}</h1><p>Mark each event or segment, then "
         "<button onclick=dump()>copy verdicts</button> <span id=st></span></p>"]
    tl = out / "timeline.json"
    if tl.exists():
        doc = json.loads(tl.read_text())
        h.append("<h2>Freestyle timelines</h2>")
        for w in doc["windows"]:
            h.append(f"<h3>{w['segment']} {w['w0']}-{w['w1']} s</h3>"
                     f"<p>{w['summary']}</p><div class=strip>")
            per = TL_TILE[0] * TL_TILE[1]
            for g in range(-(-len(w["frame_t"]) // per)):
                h.append(f"<img class=g src='frames/{w['job']}-g{g}.jpg'>")
            h.append("</div>")
            for k, e in enumerate(w["events"]):
                key = f"{w['job']}/e{k}"
                ag = "" if "agreed_b" not in e else (
                    " [b agrees]" if e["agreed_b"] else " <span class=u>[b differs]</span>")
                h.append(
                    f"<div class=ev data-k='{key}'>{e['t_start']}-{e['t_end']} s "
                    f"<b>{e['primitive']}</b> {e['hand']} {json.dumps(e['params'])} "
                    f"\"{e['name']}\" c={e['confidence']}{ag}"
                    "<button onclick=pick(this)>right</button>"
                    "<button onclick=pick(this)>partly</button>"
                    "<button onclick=pick(this)>wrong</button></div>")
            h.append(f"<div class=ev data-k='{w['job']}/missed'>events missing in this window?"
                     "<button onclick=pick(this)>none</button>"
                     "<button onclick=pick(this)>some</button>"
                     "<button onclick=pick(this)>many</button></div>")
    qa = out / "qa.json"
    if qa.exists():
        segs = json.loads(qa.read_text())["segments"]
        rank = {"mismatch": 0, "partial": 1, "unsure": 2}
        flagged = sorted((k for k, r in segs.items() if r["flag"] != "ok"),
                         key=lambda k: rank[segs[k]["flag"]])
        oks = [k for k, r in segs.items() if r["flag"] == "ok"]
        sample = sorted(random.Random(0).sample(oks, min(10, len(oks))))
        h.append(f"<h2>Segment QA: {len(flagged)} flagged, 10 random ok</h2>")
        for sid in flagged + sample:
            r = segs[sid]
            cls = "flag" if r["flag"] == "mismatch" else ("u" if r["flag"] == "unsure" else "")
            issue = r["a"]["issue"] + (f" | b: {r['b']['issue']}" if "b" in r else "")
            h.append(f"<h3 class={cls}>{sid}: {r['flag']}"
                     f"{' (marked bad by Noa)' if r['bad_marked'] else ''}</h3>"
                     f"<p>{r['instruction_de']} {json.dumps(r['labels'])}<br>{issue}</p>"
                     "<div class=strip>")
            h.append(f"<img class=r src='frames/qa-{sid}.jpg'>")
            h.append(f"</div><div class=ev data-k='qa/{sid}'>Spark verdict is"
                     "<button onclick=pick(this)>right</button>"
                     "<button onclick=pick(this)>wrong</button></div>")
    (out / "review.html").write_text("\n".join(h))
    print(f"review: {out / 'review.html'}")


def main() -> None:
    if len(sys.argv) > 1 and sys.argv[1] == "report":
        for s in sys.argv[2:]:
            write_review(OUT_ROOT / s)
        return
    ap = argparse.ArgumentParser()
    ap.add_argument("session")
    ap.add_argument("--max-usd", type=float, default=15.0,
                    help="cap on the sum of all motion-* runs in the ledger")
    ap.add_argument("--passes", type=int, choices=[1, 2], default=1)
    ap.add_argument("--only", choices=["qa", "timeline"])
    ap.add_argument("--limit-jobs", type=int, default=0)
    ap.add_argument("--reasoning-effort", default="medium")
    ap.add_argument("--workers", type=int, default=4)
    ap.add_argument("--pilot", type=int, default=2)
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()
    try:
        label_session(args)
    except spark.AbortError as e:
        print(f"refused: {e}", file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main()
