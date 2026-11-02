//! Perception thread (spec sections 4 and 5): camera frame -> seg student
//! (prev mask fed back) -> hand boxes and face box from the mask -> 128 px
//! crops -> hand and face students -> feat.* signals. One thread per
//! process, fed by the camera callback through a one-slot channel, so a
//! slow frame drops the next one instead of queueing. Never on the audio
//! thread.
//!
//! Models: `$SYNTHEXIS_MODEL_DIR`, else
//! ~/Library/Application Support/synthexis/models, holding seg, hand and
//! face as `.mlmodelc` (install.sh compiles them) or `.mlpackage`.
//! Backend: `$SYNTHEXIS_PERCEPTION` or the first word of `<dir>/backend`:
//! `students` (default when seg loads), `vision` (Apple Vision at runtime,
//! the fallback the licence check allows) or `off`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};

use super::coreml::{F16Array, Frame, Input, Model, PixelBuf};
use super::geom::*;
use crate::log::log;
use crate::signal::{Sample, FACE_IDS, HAND_L, HAND_R, OBJECT_IDS, SIG_HANDS_DISTANCE};

const STATS_WINDOW: usize = 300;
const LOG_EVERY: Duration = Duration::from_secs(5);

struct Job {
    frame: Frame,
    t_capture_ns: u64,
    callback: Instant,
}

static TX: OnceLock<Option<Sender<Job>>> = OnceLock::new();

/// Hands a camera frame to the perception thread (started on first use).
/// Drops the frame while the thread is still busy with the last one.
pub fn submit(frame: Frame, t_capture_ns: u64, callback: Instant) {
    let tx = TX.get_or_init(|| {
        let (tx, rx) = bounded(1);
        std::thread::Builder::new()
            .name("synthexis-perception".into())
            .spawn(move || run(&rx))
            .map_err(|e| log(&format!("perception: thread not started: {e}")))
            .ok()
            .map(|_| tx)
    });
    if let Some(tx) = tx {
        match tx.try_send(Job { frame, t_capture_ns, callback }) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

pub fn model_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SYNTHEXIS_MODEL_DIR") {
        return d.into();
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join("Library/Application Support/synthexis/models")
}

fn find_model(dir: &Path, name: &str) -> Option<PathBuf> {
    ["mlmodelc", "mlpackage"]
        .iter()
        .map(|ext| dir.join(format!("{name}.{ext}")))
        .find(|p| p.exists())
}

fn load(dir: &Path, name: &str) -> Option<Model> {
    let path = find_model(dir, name)?;
    let t0 = Instant::now();
    match Model::load(&path) {
        Ok(m) => {
            log(&format!(
                "perception: {name} loaded from {} in {:.0} ms, outputs {:?}",
                path.display(),
                t0.elapsed().as_secs_f64() * 1e3,
                m.outputs
            ));
            Some(m)
        }
        Err(e) => {
            log(&format!("perception: {name} not loaded from {}: {e}", path.display()));
            None
        }
    }
}

fn backend_choice(dir: &Path) -> String {
    std::env::var("SYNTHEXIS_PERCEPTION")
        .ok()
        .or_else(|| std::fs::read_to_string(dir.join("backend")).ok())
        .and_then(|s| s.split_whitespace().next().map(str::to_string))
        .unwrap_or_else(|| "students".into())
}

/// What one frame produced, before the hold logic.
#[derive(Default)]
pub struct Percept {
    pub hands: [Option<(HandFeatures, f32)>; 2],
    pub face: Option<(FaceFeatures, f32)>,
    pub object: Option<([f32; 5], f32)>,
}

pub trait Backend {
    fn name(&self) -> &'static str;
    /// Runs on one frame; `stages` gets named stage times in ms.
    fn process(&mut self, frame: &Frame, t_ns: u64, stages: &mut Vec<(&'static str, f64)>) -> Option<Percept>;
}

/// The backend `choice` asks for ("students", "vision", "off").
pub fn make_backend(dir: &Path, choice: &str) -> Option<Box<dyn Backend>> {
    match choice {
        "off" => None,
        "vision" => Some(Box::new(super::vision::VisionBackend::new())),
        _ => match Students::new(&dir) {
            Some(s) => Some(Box::new(s)),
            None => {
                log("perception: no seg student, falling back to Apple Vision");
                Some(Box::new(super::vision::VisionBackend::new()))
            }
        },
    }
}

fn run(rx: &Receiver<Job>) {
    let dir = model_dir();
    let choice = backend_choice(&dir);
    let mut backend = make_backend(&dir, &choice);
    log(&format!(
        "perception: backend {} (asked for {choice}, models in {})",
        backend.as_ref().map_or("off", |b| b.name()),
        dir.display()
    ));
    let mut emitter = Emitter::default();
    let mut stats = Stats::default();
    for job in rx {
        let Some(b) = backend.as_mut() else { continue };
        let start = Instant::now();
        let mut stages = Vec::new();
        let percept = b.process(&job.frame, job.t_capture_ns, &mut stages);
        drop(job.frame);
        let done = Instant::now();
        if let Some(p) = percept {
            emitter.emit(&p, job.t_capture_ns);
        }
        let emitted = Instant::now();
        stats.push("wait", start.duration_since(job.callback));
        stats.push("frame", done.duration_since(start));
        stats.push("emit", emitted.duration_since(done));
        stats.push("cb_to_emit", emitted.duration_since(job.callback));
        for (name, ms) in stages {
            stats.push_ms(name, ms);
        }
        stats.frames += 1;
        stats.maybe_log(b.name());
    }
}

#[derive(Default)]
pub struct Stats {
    series: Vec<(&'static str, VecDeque<f64>)>,
    /// Window length; 0 = STATS_WINDOW.
    pub window: usize,
    frames: u64,
    last_log: Option<Instant>,
}

impl Stats {
    pub fn push(&mut self, name: &'static str, d: Duration) {
        self.push_ms(name, d.as_secs_f64() * 1e3);
    }

    pub fn push_ms(&mut self, name: &'static str, ms: f64) {
        let i = match self.series.iter().position(|(n, _)| *n == name) {
            Some(i) => i,
            None => {
                self.series.push((name, VecDeque::new()));
                self.series.len() - 1
            }
        };
        let cap = if self.window == 0 { STATS_WINDOW } else { self.window };
        let v = &mut self.series[i].1;
        v.push_back(ms);
        while v.len() > cap {
            v.pop_front();
        }
    }

    /// "name=median/p95 ..." over the window.
    pub fn summary(&self) -> String {
        let parts: Vec<String> = self
            .series
            .iter()
            .map(|(n, v)| {
                let (m, p) = super::camera::median_p95(v);
                format!("{n}={m:.2}/{p:.2}")
            })
            .collect();
        parts.join(" ")
    }

    fn maybe_log(&mut self, backend: &str) {
        if self.last_log.is_some_and(|t| t.elapsed() < LOG_EVERY) {
            return;
        }
        self.last_log = Some(Instant::now());
        log(&format!("perception {backend} frames={} ms med/p95 {}", self.frames, self.summary()));
    }
}

/// Holds last values so an unseen source keeps reporting with
/// `present = false` (cond.occlusion then holds or glides).
#[derive(Default)]
struct Emitter {
    hand: [[f32; 20]; 2],
    face: [f32; 9],
    object: [f32; 5],
    distance: f32,
}

impl Emitter {
    fn emit(&mut self, p: &Percept, t_ns: u64) {
        let eng = super::engine();
        let send = |id: &str, value: f32, confidence: f32, present: bool| {
            eng.emit(id, &Sample { value, confidence, t_capture_ns: t_ns, present });
        };
        for (s, ids) in [HAND_L, HAND_R].iter().enumerate() {
            let seen = p.hands[s].as_ref();
            if let Some((f, _)) = seen {
                self.hand[s] = f.values();
            }
            let conf = seen.map_or(0.0, |h| h.1);
            for (id, v) in ids.iter().zip(self.hand[s]) {
                send(id, v, conf, seen.is_some());
            }
        }
        let both = match (&p.hands[0], &p.hands[1]) {
            (Some((l, cl)), Some((r, cr))) => {
                let dx = (l.x - r.x).abs();
                let dy = (l.y - r.y).abs();
                self.distance = ((dx * dx + dy * dy).sqrt() / 0.8).clamp(0.0, 1.0);
                Some(cl.min(*cr))
            }
            _ => None,
        };
        send(SIG_HANDS_DISTANCE, self.distance, both.unwrap_or(0.0), both.is_some());
        let face = p.face.as_ref();
        if let Some((f, _)) = face {
            let v = f.values();
            // Keep the last value of a part that was not seen this frame.
            let seen = [f.seen_mouth, f.seen_mouth, f.seen_eye[0], f.seen_eye[1], f.seen_eye[0], f.seen_eye[1]];
            for (i, s) in seen.iter().enumerate() {
                if *s {
                    self.face[i] = v[i];
                }
            }
            if f.seen_eye[0] && f.seen_eye[1] {
                self.face[6..9].copy_from_slice(&v[6..9]);
            }
        }
        for (i, id) in FACE_IDS.iter().enumerate() {
            let part_seen = face.is_some_and(|(f, _)| match i {
                0 | 1 => f.seen_mouth,
                2 | 4 => f.seen_eye[0],
                3 | 5 => f.seen_eye[1],
                _ => f.seen_eye[0] && f.seen_eye[1],
            });
            send(id, self.face[i], if part_seen { face.map_or(0.0, |f| f.1) } else { 0.0 }, part_seen);
        }
        let obj = p.object.as_ref();
        if let Some((v, _)) = obj {
            self.object = *v;
        }
        for (id, v) in OBJECT_IDS.iter().zip(self.object) {
            send(id, v, obj.map_or(0.0, |o| o.1), obj.is_some());
        }
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

fn lin(v: f32, lo: f32, hi: f32) -> f32 {
    ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
}

struct HandHead {
    model: Model,
    keypoints: String,
    fingers: String,
}

struct FaceHead {
    model: Model,
    logits: String,
}

/// The three CoreML students.
pub struct Students {
    seg: Model,
    seg_out: String,
    hand: Option<HandHead>,
    face: Option<FaceHead>,
    seg_in: PixelBuf,
    prev: F16Array,
    crop_in: PixelBuf,
    mask: Vec<u8>,
    face_mask: Vec<u8>,
    labels: Vec<u32>,
    lut: [u16; N_SEG],
    tracker: HandTracker,
    object_track: PointTrack,
    /// Hand crop and hand model time of the current frame, ms.
    hand_ms: [f64; 2],
}

impl Students {
    pub fn new(dir: &Path) -> Option<Self> {
        let seg = load(dir, "seg")?;
        let seg_out = seg.output_with(N_SEG * SEG_W * SEG_H)?;
        let hand = load(dir, "hand").and_then(|model| {
            Some(HandHead { keypoints: model.output_with(42)?, fingers: model.output_with(5)?, model })
        });
        let face = load(dir, "face")
            .and_then(|model| Some(FaceHead { logits: model.output_with(3 * CROP * CROP)?, model }));
        let buffers = (|| -> Result<_, String> {
            Ok((PixelBuf::new(SEG_W, SEG_H)?, F16Array::new(&[1, 1, SEG_H, SEG_W])?, PixelBuf::new(CROP, CROP)?))
        })();
        let (seg_in, prev, crop_in) = buffers.map_err(|e| log(&format!("perception: buffers: {e}"))).ok()?;
        let mut lut = [0u16; N_SEG];
        for (k, v) in lut.iter_mut().enumerate() {
            *v = prev_mask_bits(k as u8);
        }
        Some(Self {
            seg,
            seg_out,
            hand,
            face,
            seg_in,
            prev,
            crop_in,
            mask: vec![0; SEG_W * SEG_H],
            face_mask: vec![0; CROP * CROP],
            labels: Vec::new(),
            lut,
            tracker: HandTracker::default(),
            object_track: PointTrack::default(),
            hand_ms: [0.0; 2],
        })
    }

    fn hand(&mut self, src: &Bgra, b: &Blob, side: Side, t_ns: u64) -> Option<(HandFeatures, f32)> {
        let (fw, fh) = (src.w as f32, src.h as f32);
        let (bx, by, bw, bh) = blob_box(b, fw, fh);
        let (x0, y0, s) = square(bx, by, bw, bh, HAND_PAD);
        let conf = lin(b.area as f32, MIN_HAND_AREA as f32, 4.0 * MIN_HAND_AREA as f32);
        let head = self.hand.as_ref()?;
        let t0 = Instant::now();
        self.crop_in.write(|d, ds| super::vimage::crop_square(src, x0, y0, s, d, ds, CROP));
        let t1 = Instant::now();
        let out = head
            .model
            .predict(&[("image", Input::Pixels(&self.crop_in))])
            .map_err(|e| log(&format!("perception: hand predict: {e}")))
            .ok()?;
        let kp = out.with_f16(&head.keypoints, |d, st| {
            let mut kp = [[0f32; 2]; 21];
            for (j, p) in kp.iter_mut().enumerate() {
                let o = j * st[1];
                *p = [x0 + f16_to_f32(d[o]) * s, y0 + f16_to_f32(d[o + st[2]]) * s];
            }
            kp
        })?;
        let ext = out.with_f16(&head.fingers, |d, st| {
            let mut e = [0f32; 5];
            for (i, v) in e.iter_mut().enumerate() {
                *v = sigmoid(f16_to_f32(d[i * st[1]]));
            }
            e
        })?;
        self.hand_ms[0] += t1.duration_since(t0).as_secs_f64() * 1e3;
        self.hand_ms[1] += t1.elapsed().as_secs_f64() * 1e3;
        let mut f = hand_features(&kp, side, fw, fh, Some(ext));
        f.speed = self.tracker.update(side, f.palm, fh, t_ns);
        Some((f, conf))
    }

    /// Face from the face student on a 1.3x crop, else from the eye and
    /// mouth classes of the seg mask.
    fn face(&mut self, src: &Bgra, face: &Blob) -> Option<(FaceFeatures, f32)> {
        let conf = lin(face.area as f32, MIN_FACE_AREA as f32, 4.0 * MIN_FACE_AREA as f32);
        let Some(head) = self.face.as_ref() else {
            let inside = |b: &Blob| {
                let c = b.centre();
                c[0] >= face.x0 as f32 && c[0] <= face.x1 as f32 + 1.0 && c[1] >= face.y0 as f32 && c[1] <= face.y1 as f32 + 1.0
            };
            let eyes: Vec<Shape> = components(&self.mask, SEG_W, SEG_H, |c| c == C_EYE, &mut self.labels)
                .iter()
                .filter(|b| inside(b))
                .take(2)
                .map(Blob::shape)
                .collect();
            let mouth = components(&self.mask, SEG_W, SEG_H, |c| c == C_MOUTH, &mut self.labels)
                .iter()
                .find(|b| inside(b))
                .map(Blob::shape);
            let fbox = [face.x0 as f32, face.y0 as f32, (face.x1 - face.x0 + 1) as f32, (face.y1 - face.y0 + 1) as f32];
            return Some((face_features(fbox, &eyes, mouth, CAL_MASK), conf));
        };
        let (fw, fh) = (src.w as f32, src.h as f32);
        let (bx, by, bw, bh) = blob_box(face, fw, fh);
        let (x0, y0, s) = square(bx, by, bw, bh, FACE_PAD);
        self.crop_in.write(|d, ds| super::vimage::crop_square(src, x0, y0, s, d, ds, CROP));
        let out = head
            .model
            .predict(&[("image", Input::Pixels(&self.crop_in))])
            .map_err(|e| log(&format!("perception: face predict: {e}")))
            .ok()?;
        let mask = &mut self.face_mask;
        out.with_f16(&head.logits, |d, st| argmax_f16(d, 3, CROP, CROP, st[1], st[2], mask))?;
        let eyes: Vec<Shape> = components(&self.face_mask, CROP, CROP, |c| c == F_EYE, &mut self.labels)
            .iter()
            .take(2)
            .map(Blob::shape)
            .collect();
        let mouth = components(&self.face_mask, CROP, CROP, |c| c == F_MOUTH, &mut self.labels)
            .first()
            .map(Blob::shape);
        let k = CROP as f32 / s;
        let fbox = [(bx - x0) * k, (by - y0) * k, bw * k, bh * k];
        Some((face_features(fbox, &eyes, mouth, CAL_MASK), conf))
    }
}

impl Backend for Students {
    fn name(&self) -> &'static str {
        "students"
    }

    fn process(&mut self, frame: &Frame, t_ns: u64, stages: &mut Vec<(&'static str, f64)>) -> Option<Percept> {
        frame.read(|data, stride, w, h| {
            let src = Bgra { data, stride, w, h };
            let mut lap = Instant::now();
            let mut stage = |stages: &mut Vec<(&'static str, f64)>, name: &'static str| {
                let now = Instant::now();
                stages.push((name, now.duration_since(lap).as_secs_f64() * 1e3));
                lap = now;
            };
            self.seg_in.write(|d, ds| resize_quad(&src, d, ds, SEG_W, SEG_H));
            stage(stages, "resize");
            let out = self
                .seg
                .predict(&[("image", Input::Pixels(&self.seg_in)), ("prev_mask", Input::Array(&self.prev))])
                .map_err(|e| log(&format!("perception: seg predict: {e}")))
                .ok()?;
            stage(stages, "seg");
            let mask = &mut self.mask;
            out.with_f16(&self.seg_out, |d, st| argmax_f16(d, N_SEG, SEG_W, SEG_H, st[1], st[2], mask))?;
            drop(out);
            let lut = self.lut;
            self.prev.write(|p| p.iter_mut().zip(self.mask.iter()).for_each(|(p, &c)| *p = lut[c as usize]));
            let hands: Vec<Blob> = components(&self.mask, SEG_W, SEG_H, |c| c == C_HAND, &mut self.labels)
                .into_iter()
                .filter(|b| b.area >= MIN_HAND_AREA)
                .take(2)
                .collect();
            let face = components(&self.mask, SEG_W, SEG_H, |c| c == C_FACE || c == C_EYE || c == C_MOUTH, &mut self.labels)
                .into_iter()
                .find(|b| b.area >= MIN_FACE_AREA);
            let object = components(&self.mask, SEG_W, SEG_H, |c| c == C_OBJECT, &mut self.labels)
                .into_iter()
                .find(|b| b.area >= MIN_OBJECT_AREA);
            stage(stages, "mask");
            let (fw, fh) = (w as f32, h as f32);
            let (kx, ky) = (fw / SEG_W as f32, fh / SEG_H as f32);
            let centres: Vec<[f32; 2]> = hands.iter().map(|b| {
                let c = b.centre();
                [c[0] * kx, c[1] * ky]
            }).collect();
            let face_cx = face.as_ref().map(|f| f.centre()[0] * kx);
            let sides = self.tracker.assign(&centres, face_cx, fw, t_ns);
            let mut p = Percept::default();
            self.hand_ms = [0.0; 2];
            for (b, side) in hands.iter().zip(sides) {
                p.hands[side as usize] = self.hand(&src, b, side, t_ns);
            }
            stage(stages, "hands");
            stages.push(("hand_crop", self.hand_ms[0]));
            stages.push(("hand_model", self.hand_ms[1]));
            if let Some(f) = face.as_ref() {
                p.face = self.face(&src, f);
            }
            stage(stages, "face");
            if let Some(o) = object.as_ref() {
                let v = object_features(o, SEG_W as f32, SEG_H as f32);
                let c = o.centre();
                let speed = self.object_track.update(c, SEG_H as f32, t_ns);
                let conf = lin(o.area as f32, MIN_OBJECT_AREA as f32, 4.0 * MIN_OBJECT_AREA as f32);
                p.object = Some(([v[0], v[1], v[2], v[3], speed], conf));
            }
            Some(p)
        })
        .flatten()
    }
}
