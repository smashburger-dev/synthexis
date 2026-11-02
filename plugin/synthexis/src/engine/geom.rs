//! Pure geometry of the perception pipeline (spec section 5, feat.*):
//! resize and crops on BGRA frames, argmax and connected components on the
//! seg mask, hand features from 21 keypoints, face features from eye and
//! mouth shapes, the hand tracker (perc.track). No ObjC, all unit-tested.
//!
//! Coordinates: camera frames are unmirrored, so image-left is the
//! person's right. Every `x` feature is from the user's view (0 = the
//! user's left), which is `1 - x_image`.

use std::f32::consts::PI;

pub const SEG_W: usize = 384;
pub const SEG_H: usize = 216;
pub const CROP: usize = 128;
pub const HAND_PAD: f32 = 1.25;
pub const FACE_PAD: f32 = 1.3;

/// Seg classes (contract: background, person, face, eye, mouth, hand, object).
pub const C_FACE: u8 = 2;
pub const C_EYE: u8 = 3;
pub const C_MOUTH: u8 = 4;
pub const C_HAND: u8 = 5;
pub const C_OBJECT: u8 = 6;
pub const N_SEG: usize = 7;
/// Face student classes (other, eye, mouth).
pub const F_EYE: u8 = 1;
pub const F_MOUTH: u8 = 2;

/// Smallest blobs taken as a hand / object, in seg pixels (384x216). The
/// packer needs a 32 px hand box side at 1280 px, about 10 seg pixels.
pub const MIN_HAND_AREA: usize = 60;
pub const MIN_FACE_AREA: usize = 150;
pub const MIN_OBJECT_AREA: usize = 40;

/// Borrowed BGRA image.
#[derive(Clone, Copy)]
pub struct Bgra<'a> {
    pub data: &'a [u8],
    pub stride: usize,
    pub w: usize,
    pub h: usize,
}

/// Area-average downscale of `src` into a BGRA `dst` of `dw` x `dh`
/// (stride `ds`). Alpha is set to 255.
pub fn resize_area(src: &Bgra, dst: &mut [u8], ds: usize, dw: usize, dh: usize) {
    for dy in 0..dh {
        let y0 = dy * src.h / dh;
        let y1 = ((dy + 1) * src.h / dh).max(y0 + 1).min(src.h);
        for dx in 0..dw {
            let x0 = dx * src.w / dw;
            let x1 = ((dx + 1) * src.w / dw).max(x0 + 1).min(src.w);
            let mut acc = [0u32; 3];
            for y in y0..y1 {
                let row = &src.data[y * src.stride..];
                for x in x0..x1 {
                    let p = &row[x * 4..x * 4 + 3];
                    acc[0] += p[0] as u32;
                    acc[1] += p[1] as u32;
                    acc[2] += p[2] as u32;
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let o = dy * ds + dx * 4;
            dst[o] = (acc[0] / n) as u8;
            dst[o + 1] = (acc[1] / n) as u8;
            dst[o + 2] = (acc[2] / n) as u8;
            dst[o + 3] = 255;
        }
    }
}

/// Fast downscale: each output pixel averages 2x2 samples at the quarter
/// points of its source block (an area filter for ratios around 3).
pub fn resize_quad(src: &Bgra, dst: &mut [u8], ds: usize, dw: usize, dh: usize) {
    let (kx, ky) = (src.w as f32 / dw as f32, src.h as f32 / dh as f32);
    let xs: Vec<[usize; 2]> = (0..dw)
        .map(|dx| [0.25f32, 0.75].map(|f| ((((dx as f32 + f) * kx) as usize).min(src.w - 1)) * 4))
        .collect();
    for dy in 0..dh {
        let [r0, r1] = [0.25f32, 0.75].map(|f| (((dy as f32 + f) * ky) as usize).min(src.h - 1) * src.stride);
        let out = &mut dst[dy * ds..dy * ds + dw * 4];
        for (dx, [x0, x1]) in xs.iter().enumerate() {
            for ch in 0..3 {
                let sum = src.data[r0 + x0 + ch] as u16
                    + src.data[r0 + x1 + ch] as u16
                    + src.data[r1 + x0 + ch] as u16
                    + src.data[r1 + x1 + ch] as u16;
                out[dx * 4 + ch] = ((sum + 2) / 4) as u8;
            }
            out[dx * 4 + 3] = 255;
        }
    }
}

/// Square crop: frame pixels `x0, y0` (top-left), `side` long, into a
/// BGRA `n` x `n` dst (stride `ds`). Zero outside the frame (the packer's
/// PIL crop pads with black), bilinear with up to 4x4 supersampling when
/// the crop shrinks.
pub fn crop_square(src: &Bgra, x0: f32, y0: f32, side: f32, dst: &mut [u8], ds: usize, n: usize) {
    let scale = side / n as f32;
    let k = (scale.ceil() as usize).clamp(1, 4);
    let sub = scale / k as f32;
    for v in 0..n {
        for u in 0..n {
            let mut acc = [0f32; 3];
            for j in 0..k {
                for i in 0..k {
                    let sx = x0 + u as f32 * scale + (i as f32 + 0.5) * sub - 0.5;
                    let sy = y0 + v as f32 * scale + (j as f32 + 0.5) * sub - 0.5;
                    let px = bilinear(src, sx, sy);
                    acc[0] += px[0];
                    acc[1] += px[1];
                    acc[2] += px[2];
                }
            }
            let inv = 1.0 / (k * k) as f32;
            let o = v * ds + u * 4;
            dst[o] = (acc[0] * inv) as u8;
            dst[o + 1] = (acc[1] * inv) as u8;
            dst[o + 2] = (acc[2] * inv) as u8;
            dst[o + 3] = 255;
        }
    }
}

fn texel(src: &Bgra, x: isize, y: isize) -> [f32; 3] {
    if x < 0 || y < 0 || x >= src.w as isize || y >= src.h as isize {
        return [0.0; 3];
    }
    let o = y as usize * src.stride + x as usize * 4;
    [src.data[o] as f32, src.data[o + 1] as f32, src.data[o + 2] as f32]
}

fn bilinear(src: &Bgra, x: f32, y: f32) -> [f32; 3] {
    let (fx, fy) = (x.floor(), y.floor());
    let (ax, ay) = (x - fx, y - fy);
    let (ix, iy) = (fx as isize, fy as isize);
    let a = texel(src, ix, iy);
    let b = texel(src, ix + 1, iy);
    let c = texel(src, ix, iy + 1);
    let d = texel(src, ix + 1, iy + 1);
    let mut out = [0.0; 3];
    for ch in 0..3 {
        let top = a[ch] + (b[ch] - a[ch]) * ax;
        let bot = c[ch] + (d[ch] - c[ch]) * ax;
        out[ch] = top + (bot - top) * ay;
    }
    out
}

/// IEEE fp16 bits -> a key that sorts like the float value.
#[inline]
fn f16_key(bits: u16) -> u16 {
    if bits & 0x8000 != 0 {
        !bits
    } else {
        bits | 0x8000
    }
}

/// Per-pixel argmax over `n_cls` fp16 planes (element strides `cs` per
/// class, `rs` per row) into `out` (`w * h`).
pub fn argmax_f16(logits: &[u16], n_cls: usize, w: usize, h: usize, cs: usize, rs: usize, out: &mut [u8]) {
    for y in 0..h {
        for x in 0..w {
            let base = y * rs + x;
            let mut best = 0u8;
            let mut best_k = f16_key(logits[base]);
            for c in 1..n_cls {
                let k = f16_key(logits[base + c * cs]);
                if k > best_k {
                    best_k = k;
                    best = c as u8;
                }
            }
            out[y * w + x] = best;
        }
    }
}

/// fp16 bits of `class / 6` for the seg prev_mask input.
pub fn prev_mask_bits(class: u8) -> u16 {
    f32_to_f16(class as f32 / 6.0)
}

/// f32 -> fp16 bits for values in 0..1 (normal range, round to nearest).
pub fn f32_to_f16(v: f32) -> u16 {
    if v <= 0.0 {
        return 0;
    }
    let bits = v.to_bits();
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    if exp <= 0 {
        return 0;
    }
    let mant = bits & 0x7f_ffff;
    let mut h = ((exp as u32) << 10) | (mant >> 13);
    if mant & 0x1000 != 0 {
        h += 1;
    }
    h as u16
}

/// fp16 bits -> f32 (finite values).
pub fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1f) as i32;
    let mant = (h & 0x3ff) as f32;
    let mag = if exp == 0 {
        mant * 2f32.powi(-24)
    } else {
        (1.0 + mant / 1024.0) * 2f32.powi(exp - 15)
    };
    sign * mag
}

/// A connected region with its moments (pixel units of its mask).
#[derive(Debug, Clone, Copy, Default)]
pub struct Blob {
    pub area: usize,
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
    sx: f64,
    sy: f64,
    sxx: f64,
    syy: f64,
    sxy: f64,
}

impl Blob {
    fn add(&mut self, x: usize, y: usize) {
        if self.area == 0 {
            (self.x0, self.y0, self.x1, self.y1) = (x, y, x, y);
        }
        self.area += 1;
        self.x0 = self.x0.min(x);
        self.y0 = self.y0.min(y);
        self.x1 = self.x1.max(x);
        self.y1 = self.y1.max(y);
        let (fx, fy) = (x as f64, y as f64);
        self.sx += fx;
        self.sy += fy;
        self.sxx += fx * fx;
        self.syy += fy * fy;
        self.sxy += fx * fy;
    }

    pub fn centre(&self) -> [f32; 2] {
        let n = self.area.max(1) as f64;
        [(self.sx / n) as f32 + 0.5, (self.sy / n) as f32 + 0.5]
    }

    pub fn shape(&self) -> Shape {
        let n = self.area.max(1) as f64;
        let (mx, my) = (self.sx / n, self.sy / n);
        // +1/12: a pixel is a unit square, not a point.
        let cxx = self.sxx / n - mx * mx + 1.0 / 12.0;
        let cyy = self.syy / n - my * my + 1.0 / 12.0;
        let cxy = self.sxy / n - mx * my;
        Shape::from_cov([mx as f32 + 0.5, my as f32 + 0.5], cxx as f32, cyy as f32, cxy as f32)
    }
}

/// Connected regions (4-neighbour) of pixels where `pred(mask[i])`,
/// largest first. `labels` is scratch of `w * h`.
pub fn components(mask: &[u8], w: usize, h: usize, pred: impl Fn(u8) -> bool, labels: &mut Vec<u32>) -> Vec<Blob> {
    labels.clear();
    labels.resize(w * h, 0);
    let mut blobs = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if labels[start] != 0 || !pred(mask[start]) {
            continue;
        }
        let id = blobs.len() as u32 + 1;
        let mut blob = Blob::default();
        labels[start] = id;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            blob.add(x, y);
            let mut visit = |j: usize| {
                if labels[j] == 0 && pred(mask[j]) {
                    labels[j] = id;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        blobs.push(blob);
    }
    blobs.sort_by(|a, b| b.area.cmp(&a.area));
    blobs
}

/// Ellipse of a region or point set from its second moments.
#[derive(Debug, Clone, Copy, Default)]
pub struct Shape {
    pub centre: [f32; 2],
    /// Full axis lengths of the equivalent ellipse.
    pub major: f32,
    pub minor: f32,
    /// Major axis angle, radians in -pi/2..pi/2 (image y down).
    pub angle: f32,
}

impl Shape {
    fn from_cov(centre: [f32; 2], cxx: f32, cyy: f32, cxy: f32) -> Self {
        let tr = cxx + cyy;
        let det = cxx * cyy - cxy * cxy;
        let disc = (tr * tr / 4.0 - det).max(0.0).sqrt();
        let l1 = (tr / 2.0 + disc).max(0.0);
        let l2 = (tr / 2.0 - disc).max(0.0);
        // Filled ellipse: variance along an axis = (axis/2)^2 / 4.
        Self {
            centre,
            major: 4.0 * l1.sqrt(),
            minor: 4.0 * l2.sqrt(),
            angle: 0.5 * (2.0 * cxy).atan2(cxx - cyy),
        }
    }

    /// Ellipse through a contour of points (Vision landmarks). Points on an
    /// ellipse outline have twice the variance of the filled ellipse.
    pub fn from_outline(pts: &[[f32; 2]]) -> Option<Self> {
        if pts.len() < 3 {
            return None;
        }
        let n = pts.len() as f32;
        let mx = pts.iter().map(|p| p[0]).sum::<f32>() / n;
        let my = pts.iter().map(|p| p[1]).sum::<f32>() / n;
        let (mut cxx, mut cyy, mut cxy) = (0.0, 0.0, 0.0);
        for p in pts {
            let (dx, dy) = (p[0] - mx, p[1] - my);
            cxx += dx * dx;
            cyy += dy * dy;
            cxy += dx * dy;
        }
        Some(Self::from_cov([mx, my], cxx / n / 2.0, cyy / n / 2.0, cxy / n / 2.0))
    }

    pub fn ratio(&self) -> f32 {
        if self.major <= f32::EPSILON {
            0.0
        } else {
            self.minor / self.major
        }
    }
}

fn lin(v: f32, lo: f32, hi: f32) -> f32 {
    ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn len(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    len(sub(a, b))
}

/// Unsigned angle between two vectors, radians.
fn angle_between(a: [f32; 2], b: [f32; 2]) -> f32 {
    let d = a[0] * b[0] + a[1] * b[1];
    let c = a[0] * b[1] - a[1] * b[0];
    c.atan2(d).abs()
}

/// Side of the person: index 0 = L, 1 = R (signal tables use this order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    L = 0,
    R = 1,
}

/// MediaPipe joint order (`JOINTS` in tools/train/pack.py).
pub const WRIST: usize = 0;
/// Joint chains per finger, thumb..pinky: [base, j1, j2, tip].
pub const FINGERS: [[usize; 4]; 5] = [[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12], [13, 14, 15, 16], [17, 18, 19, 20]];

/// Geometric hand features, frame-normalised. `values()` follows
/// `signal::HAND_FEATS`.
#[derive(Debug, Clone, Copy, Default)]
pub struct HandFeatures {
    pub finger: [f32; 5],
    pub spread: f32,
    pub x: f32,
    pub y: f32,
    pub depth: f32,
    pub pinch: f32,
    pub speed: f32,
    pub roll: f32,
    pub tilt: [f32; 5],
    pub finger_count: f32,
    /// Palm centre in frame pixels (tracking, hands.distance).
    pub palm: [f32; 2],
}

impl HandFeatures {
    pub fn openness(&self) -> f32 {
        self.finger.iter().sum::<f32>() / 5.0
    }

    pub fn values(&self) -> [f32; 20] {
        let o = self.openness();
        let f = self.finger;
        let t = self.tilt;
        [
            o, 1.0 - o, f[0], f[1], f[2], f[3], f[4], self.spread, self.x, self.y, self.depth,
            self.pinch, self.speed, self.roll, t[0], t[1], t[2], t[3], t[4], self.finger_count,
        ]
    }
}

/// Features from 21 keypoints in frame pixels. `fingers_ext` are the
/// student's finger probabilities (thumb..pinky), when it has them.
pub fn hand_features(kp: &[[f32; 2]; 21], side: Side, fw: f32, fh: f32, fingers_ext: Option<[f32; 5]>) -> HandFeatures {
    let wrist = kp[WRIST];
    let (index_mcp, middle_mcp, pinky_mcp) = (kp[5], kp[9], kp[17]);
    let palm_len = dist(wrist, middle_mcp).max(1.2 * dist(index_mcp, pinky_mcp)).max(1.0);
    let palm = [
        (wrist[0] + index_mcp[0] + middle_mcp[0] + pinky_mcp[0]) / 4.0,
        (wrist[1] + index_mcp[1] + middle_mcp[1] + pinky_mcp[1]) / 4.0,
    ];
    let mut f = HandFeatures { palm, ..Default::default() };
    for (n, ch) in FINGERS.iter().enumerate() {
        // Bend summed over the three joints of the chain; straight = 0.
        let segs = [sub(kp[ch[0]], wrist), sub(kp[ch[1]], kp[ch[0]]), sub(kp[ch[2]], kp[ch[1]]), sub(kp[ch[3]], kp[ch[2]])];
        let bend: f32 = (0..3).map(|j| angle_between(segs[j], segs[j + 1])).sum();
        f.finger[n] = if n == 0 {
            // Thumb: little bend range, so also its tip's reach from the index base.
            0.5 * (1.0 - lin(bend, 0.2, 1.4)) + 0.5 * lin(dist(kp[4], index_mcp) / palm_len, 0.35, 0.9)
        } else {
            1.0 - lin(bend, 0.5, 3.0)
        };
        let dir = sub(kp[ch[3]], kp[if n == 0 { ch[1] } else { ch[0] }]);
        f.tilt[n] = 0.5 - 0.5 * dir[1] / len(dir).max(1e-3);
    }
    let ray = |c: &[usize; 4]| sub(kp[c[3]], kp[c[0]]);
    f.spread = lin(angle_between(ray(&FINGERS[1]), ray(&FINGERS[4])), 0.15, 0.95);
    f.x = 1.0 - (palm[0] / fw).clamp(0.0, 1.0);
    f.y = 1.0 - (palm[1] / fh).clamp(0.0, 1.0);
    f.depth = lin(palm_len / fh, 0.06, 0.35);
    f.pinch = 1.0 - lin(dist(kp[4], kp[8]) / palm_len, 0.12, 0.8);
    // Roll around the forearm: the knuckle line's extent across the
    // wrist->middle axis, signed so the thumb side decides the direction.
    let up = sub(middle_mcp, wrist);
    let knuckles = sub(index_mcp, pinky_mcp);
    let across = (up[0] * knuckles[1] - up[1] * knuckles[0]) / (len(up).max(1e-3) * palm_len);
    let signed = if side == Side::R { -across } else { across };
    f.roll = 0.5 + 0.5 * (signed / 0.8).clamp(-1.0, 1.0);
    let ext = fingers_ext.unwrap_or(f.finger);
    f.finger_count = ext.iter().filter(|&&e| e > 0.5).count() as f32 / 5.0;
    f
}

/// Face features. `values()` follows `signal::FACE_IDS`.
#[derive(Debug, Clone, Copy, Default)]
pub struct FaceFeatures {
    pub mouth_open: f32,
    pub smile: f32,
    pub eye_open: [f32; 2], // [L, R]
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    /// Raw shape ratios for calibration: eye minor/major [L, R], mouth
    /// minor/major, mouth width / face width.
    pub raw: [f32; 4],
    /// Which parts were seen: eyes [L, R], mouth.
    pub seen_eye: [bool; 2],
    pub seen_mouth: bool,
}

impl FaceFeatures {
    pub fn values(&self) -> [f32; 9] {
        let e = self.eye_open;
        [self.mouth_open, self.smile, e[0], e[1], 1.0 - e[0], 1.0 - e[1], self.yaw, self.pitch, self.roll]
    }
}

/// Calibration of the shape ratios per source of the shapes.
#[derive(Debug, Clone, Copy)]
pub struct FaceCal {
    pub eye: (f32, f32),
    pub mouth: (f32, f32),
    pub smile: (f32, f32),
}

/// Mask shapes (seg or face student): the mouth region includes the lips.
pub const CAL_MASK: FaceCal = FaceCal { eye: (0.18, 0.45), mouth: (0.3, 0.75), smile: (0.32, 0.5) };
/// Vision landmarks: eye outline and inner lips.
/// Calibrated on S4 block C (probes/plugin-perception/RESULT.md): eye
/// ratio neutral 0.30, closed 0.06; inner-lip ratio neutral 0.04, wide 0.57.
/// Smile from landmarks uses `smile_from_lips`, not the width.
pub const CAL_LANDMARKS: FaceCal = FaceCal { eye: (0.08, 0.28), mouth: (0.06, 0.5), smile: (0.32, 0.5) };

/// Smile from the outer-lip outline: how far the two mouth corners sit
/// above the lips' centre, per mouth width (image y down). Returns the raw
/// curve; `lin` it with the calibration.
pub fn lip_corner_lift(outer: &[[f32; 2]]) -> Option<f32> {
    let left = outer.iter().min_by(|a, b| a[0].total_cmp(&b[0]))?;
    let right = outer.iter().max_by(|a, b| a[0].total_cmp(&b[0]))?;
    let width = right[0] - left[0];
    if width <= 1.0 {
        return None;
    }
    let cy = outer.iter().map(|p| p[1]).sum::<f32>() / outer.len() as f32;
    Some((cy - (left[1] + right[1]) / 2.0) / width)
}

/// Face features from eye and mouth shapes in one coordinate space with
/// the face box `face` = [x0, y0, w, h]. Eyes in any order.
pub fn face_features(face: [f32; 4], eyes: &[Shape], mouth: Option<Shape>, cal: FaceCal) -> FaceFeatures {
    let mut f = FaceFeatures { yaw: 0.5, pitch: 0.5, roll: 0.5, ..Default::default() };
    let [fx, fy, fw, fh] = face;
    // Image-left eye is the person's right eye.
    let mut sorted: Vec<Shape> = eyes.iter().copied().take(2).collect();
    sorted.sort_by(|a, b| a.centre[0].total_cmp(&b.centre[0]));
    let mut eye_by_side: [Option<Shape>; 2] = [None, None];
    match sorted.as_slice() {
        [r, l] => {
            eye_by_side = [Some(*l), Some(*r)];
        }
        [one] => {
            let side = if one.centre[0] < fx + fw / 2.0 { Side::R } else { Side::L };
            eye_by_side[side as usize] = Some(*one);
        }
        _ => {}
    }
    for s in 0..2 {
        if let Some(e) = eye_by_side[s] {
            f.raw[s] = e.ratio();
            f.eye_open[s] = lin(e.ratio(), cal.eye.0, cal.eye.1);
            f.seen_eye[s] = true;
        }
    }
    if let (Some(l), Some(r)) = (eye_by_side[0], eye_by_side[1]) {
        let mid = [(l.centre[0] + r.centre[0]) / 2.0, (l.centre[1] + r.centre[1]) / 2.0];
        // Turning to the user's left moves the features to image-right.
        f.yaw = 0.5 - 0.5 * ((mid[0] - (fx + fw / 2.0)) / (0.2 * fw)).clamp(-1.0, 1.0);
        // Eyes sit at about 0.4 of the face box height when level.
        f.pitch = 0.5 + 0.5 * ((fy + 0.4 * fh - mid[1]) / (0.15 * fh)).clamp(-1.0, 1.0);
        // Tilt to the left shoulder lowers the person's left (image-right) eye.
        let a = (l.centre[1] - r.centre[1]).atan2(l.centre[0] - r.centre[0]);
        f.roll = 0.5 - 0.5 * (a / (PI / 6.0)).clamp(-1.0, 1.0);
    }
    if let Some(m) = mouth {
        f.raw[2] = m.ratio();
        f.raw[3] = m.major / fw.max(1.0);
        f.mouth_open = lin(m.ratio(), cal.mouth.0, cal.mouth.1);
        f.smile = lin(m.major / fw.max(1.0), cal.smile.0, cal.smile.1);
        f.seen_mouth = true;
    }
    f
}

/// Object features from its blob in seg pixels. `values()` order is
/// `signal::OBJECT_IDS` without speed, which the tracker adds.
pub fn object_features(b: &Blob, w: f32, h: f32) -> [f32; 4] {
    let c = b.centre();
    let s = b.shape();
    [
        1.0 - (c[0] / w).clamp(0.0, 1.0),
        1.0 - (c[1] / h).clamp(0.0, 1.0),
        lin((b.area as f32 / (w * h)).sqrt(), 0.02, 0.35),
        // Major axis angle, 0.5 = horizontal; the y axis points down.
        0.5 - s.angle / PI,
    ]
}

/// One side's hand track.
#[derive(Debug, Clone, Copy)]
struct Track {
    palm: [f32; 2],
    t_ns: u64,
    speed: f32,
}

/// Left/right assignment and speed per side (perc.track): two hands are
/// split by image side, one hand keeps the side of the nearest live
/// track, a new single hand takes its side from the face centre (else the
/// frame centre). Tracks expire after `EXPIRE_NS`.
#[derive(Debug, Default)]
pub struct HandTracker {
    tracks: [Option<Track>; 2],
}

const EXPIRE_NS: u64 = 500_000_000;
const MATCH_FRAC: f32 = 0.25;

impl HandTracker {
    /// Sides for the hands at palm centres `palms` (frame pixels).
    pub fn assign(&self, palms: &[[f32; 2]], face_cx: Option<f32>, fw: f32, t_ns: u64) -> Vec<Side> {
        match palms {
            [] => vec![],
            [a, b, ..] => {
                if a[0] < b[0] {
                    vec![Side::R, Side::L]
                } else {
                    vec![Side::L, Side::R]
                }
            }
            [p] => {
                let near = [Side::L, Side::R]
                    .into_iter()
                    .filter_map(|s| {
                        let t = self.tracks[s as usize]?;
                        (t_ns.saturating_sub(t.t_ns) < EXPIRE_NS).then(|| (s, dist(t.palm, *p)))
                    })
                    .filter(|(_, d)| *d < MATCH_FRAC * fw)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                match near {
                    Some((s, _)) => vec![s],
                    None => {
                        let reference = face_cx.unwrap_or(fw / 2.0);
                        vec![if p[0] < reference { Side::R } else { Side::L }]
                    }
                }
            }
        }
    }

    /// Record a hand seen at `palm`; returns its speed feature 0..1
    /// (frame heights per second, smoothed).
    pub fn update(&mut self, side: Side, palm: [f32; 2], fh: f32, t_ns: u64) -> f32 {
        let slot = &mut self.tracks[side as usize];
        let speed = match slot {
            Some(t) if t_ns > t.t_ns && t_ns - t.t_ns < EXPIRE_NS => {
                let dt = (t_ns - t.t_ns) as f32 / 1e9;
                let v = dist(palm, t.palm) / fh / dt;
                0.5 * t.speed + 0.5 * lin(v, 0.05, 2.5)
            }
            _ => 0.0,
        };
        *slot = Some(Track { palm, t_ns, speed });
        speed
    }
}

/// Speed of a tracked centroid, same scale as the hand speed.
#[derive(Debug, Default)]
pub struct PointTrack {
    last: Option<([f32; 2], u64, f32)>,
}

impl PointTrack {
    pub fn update(&mut self, p: [f32; 2], scale: f32, t_ns: u64) -> f32 {
        let speed = match self.last {
            Some((q, t, s)) if t_ns > t && t_ns - t < EXPIRE_NS => {
                let v = dist(p, q) / scale / ((t_ns - t) as f32 / 1e9);
                0.5 * s + 0.5 * lin(v, 0.05, 2.5)
            }
            _ => 0.0,
        };
        self.last = Some((p, t_ns, speed));
        speed
    }
}

/// Square crop box (x0, y0, side) around a box (x0, y0, w, h) in frame px.
pub fn square(x0: f32, y0: f32, w: f32, h: f32, pad: f32) -> (f32, f32, f32) {
    let side = pad * w.max(h);
    (x0 + w / 2.0 - side / 2.0, y0 + h / 2.0 - side / 2.0, side)
}

/// Box of a seg blob in frame pixels (x0, y0, w, h).
pub fn blob_box(b: &Blob, fw: f32, fh: f32) -> (f32, f32, f32, f32) {
    let (sx, sy) = (fw / SEG_W as f32, fh / SEG_H as f32);
    (b.x0 as f32 * sx, b.y0 as f32 * sy, (b.x1 - b.x0 + 1) as f32 * sx, (b.y1 - b.y0 + 1) as f32 * sy)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic right hand, palm facing the camera, fingers up, in a
    /// 1280x720 frame. `curl` 0 = straight fingers, 1 = curled.
    fn hand(curl: f32) -> [[f32; 2]; 21] {
        let mut kp = [[0.0; 2]; 21];
        kp[0] = [600.0, 500.0];
        // Image-left is the thumb side for a right hand facing the camera.
        let bases = [[560.0, 470.0], [570.0, 400.0], [600.0, 395.0], [630.0, 400.0], [655.0, 410.0]];
        for (n, ch) in FINGERS.iter().enumerate() {
            let mut p = bases[n];
            let mut dir: f32 = if n == 0 { -2.3 } else { -PI / 2.0 };
            for (j, &joint) in ch.iter().enumerate() {
                kp[joint] = p;
                let step = if n == 0 { 30.0 } else { 28.0 - 4.0 * j as f32 };
                dir += curl * 1.3;
                p = [p[0] + step * dir.cos(), p[1] + step * dir.sin()];
            }
        }
        kp
    }

    #[test]
    fn straight_hand_is_open_and_curled_hand_is_closed() {
        let open = hand_features(&hand(0.0), Side::R, 1280.0, 720.0, None);
        let fist = hand_features(&hand(1.0), Side::R, 1280.0, 720.0, None);
        assert!(open.openness() > 0.8, "open {}", open.openness());
        assert!(fist.openness() < 0.35, "fist {}", fist.openness());
        for n in 1..5 {
            assert!(open.finger[n] > 0.9 && fist.finger[n] < 0.3, "finger {n}: {} {}", open.finger[n], fist.finger[n]);
            assert!(open.tilt[n] > 0.9, "fingers point up");
        }
        assert_eq!(open.finger_count, 1.0);
    }

    #[test]
    fn position_is_from_the_users_view() {
        let f = hand_features(&hand(0.0), Side::R, 1280.0, 720.0, None);
        // Palm at image x 606 of 1280: just right of the user's centre.
        assert!((f.x - (1.0 - 606.25 / 1280.0)).abs() < 0.01, "x {}", f.x);
        assert!(f.y > 0.3 && f.y < 0.5, "y {}", f.y);
    }

    #[test]
    fn pinch_when_thumb_meets_index() {
        let mut kp = hand(0.0);
        kp[4] = kp[8];
        assert!(hand_features(&kp, Side::R, 1280.0, 720.0, None).pinch > 0.99);
        assert!(hand_features(&hand(0.0), Side::R, 1280.0, 720.0, None).pinch < 0.5);
    }

    #[test]
    fn finger_count_uses_the_student_logits_when_given() {
        let f = hand_features(&hand(0.0), Side::R, 1280.0, 720.0, Some([0.9, 0.9, 0.1, 0.1, 0.1]));
        assert!((f.finger_count - 0.4).abs() < 1e-6);
    }

    #[test]
    fn f16_round_trip_and_ordering() {
        for k in 0..7u8 {
            let v = k as f32 / 6.0;
            assert!((f16_to_f32(prev_mask_bits(k)) - v).abs() < 1e-3, "class {k}");
        }
        let vals = [-3.5f32, -0.25, 0.0, 0.1, 2.0, 7.5];
        let neg = |v: f32| f32_to_f16(-v) | 0x8000;
        let bits: Vec<u16> = vals.iter().map(|&v| if v < 0.0 { neg(v) } else { f32_to_f16(v) }).collect();
        for w in bits.windows(2) {
            assert!(f16_key(w[0]) < f16_key(w[1]));
        }
    }

    #[test]
    fn argmax_picks_the_largest_plane() {
        let (w, h) = (4, 2);
        let plane = w * h;
        let mut logits = vec![f32_to_f16(0.1); 3 * plane];
        logits[2 * plane + 5] = f32_to_f16(0.9);
        logits[plane + 1] = f32_to_f16(0.5);
        let mut out = vec![0u8; plane];
        argmax_f16(&logits, 3, w, h, plane, w, &mut out);
        assert_eq!(out, vec![0, 1, 0, 0, 0, 2, 0, 0]);
    }

    #[test]
    fn components_split_and_measure_blobs() {
        let (w, h) = (10, 6);
        let mut m = vec![0u8; w * h];
        for y in 1..4 {
            for x in 1..5 {
                m[y * w + x] = C_HAND;
            }
        }
        m[5 * w + 8] = C_HAND;
        let mut labels = Vec::new();
        let blobs = components(&m, w, h, |c| c == C_HAND, &mut labels);
        assert_eq!(blobs.len(), 2);
        assert_eq!(blobs[0].area, 12);
        assert_eq!((blobs[0].x0, blobs[0].y0, blobs[0].x1, blobs[0].y1), (1, 1, 4, 3));
        let s = blobs[0].shape();
        // A 4x3 rectangle: equal-variance ellipse with the same aspect.
        assert!((s.ratio() - 0.75).abs() < 0.02, "{s:?}");
        assert_eq!(blobs[0].centre(), [3.0, 2.5]);
    }

    #[test]
    fn outline_and_filled_ellipse_agree() {
        let pts: Vec<[f32; 2]> = (0..32).map(|i| {
            let t = i as f32 / 32.0 * 2.0 * PI;
            [10.0 * t.cos(), 3.0 * t.sin()]
        }).collect();
        let s = Shape::from_outline(&pts).unwrap_or_default();
        assert!((s.major - 20.0).abs() < 0.5 && (s.minor - 6.0).abs() < 0.3, "{s:?}");
    }

    #[test]
    fn face_sides_and_openness() {
        let face = [100.0, 100.0, 100.0, 120.0];
        let eye = |cx: f32, ratio: f32| Shape { centre: [cx, 148.0], major: 20.0, minor: 20.0 * ratio, angle: 0.0 };
        // Image-left eye open (person's right), image-right eye shut.
        let f = face_features(face, &[eye(130.0, 0.45), eye(170.0, 0.1)], None, CAL_MASK);
        assert!(f.eye_open[Side::R as usize] > 0.95 && f.eye_open[Side::L as usize] < 0.05);
        assert!((f.yaw - 0.5).abs() < 0.01 && (f.roll - 0.5).abs() < 0.01);
        let m = Shape { centre: [150.0, 190.0], major: 40.0, minor: 30.0, angle: 0.0 };
        let f = face_features(face, &[], Some(m), CAL_MASK);
        assert!(f.mouth_open > 0.9 && f.seen_mouth);
    }

    #[test]
    fn corners_above_the_centre_lift() {
        let flat = [[0.0, 10.0], [10.0, 8.0], [20.0, 10.0], [10.0, 12.0]];
        let smile = [[0.0, 6.0], [10.0, 8.0], [20.0, 6.0], [10.0, 12.0]];
        assert!(lip_corner_lift(&flat).unwrap_or(1.0).abs() < 1e-6);
        assert!((lip_corner_lift(&smile).unwrap_or(0.0) - 0.1).abs() < 1e-6);
    }

    #[test]
    fn tracker_sides() {
        let mut t = HandTracker::default();
        // Two hands: image-left is the person's right.
        assert_eq!(t.assign(&[[900.0, 300.0], [300.0, 300.0]], None, 1280.0, 0), vec![Side::L, Side::R]);
        // One new hand left of the face centre: person's right.
        assert_eq!(t.assign(&[[500.0, 300.0]], Some(640.0), 1280.0, 0), vec![Side::R]);
        // A live L track wins over geometry for a nearby single hand.
        t.update(Side::L, [520.0, 300.0], 720.0, 0);
        assert_eq!(t.assign(&[[500.0, 300.0]], Some(640.0), 1280.0, 33_000_000), vec![Side::L]);
        // Expired tracks fall back to geometry.
        assert_eq!(t.assign(&[[500.0, 300.0]], Some(640.0), 1280.0, 900_000_000), vec![Side::R]);
    }

    #[test]
    fn resize_and_crop_keep_colour_and_pad_black() {
        let (w, h) = (40, 20);
        let mut img = vec![0u8; w * h * 4];
        for p in img.chunks_mut(4) {
            p.copy_from_slice(&[10, 20, 30, 255]);
        }
        let src = Bgra { data: &img, stride: w * 4, w, h };
        let mut small = vec![0u8; 8 * 4 * 4];
        resize_area(&src, &mut small, 8 * 4, 8, 4);
        assert_eq!(&small[..4], &[10, 20, 30, 255]);
        let mut quad = vec![0u8; 8 * 4 * 4];
        resize_quad(&src, &mut quad, 8 * 4, 8, 4);
        assert_eq!(quad, small);
        let mut crop = vec![0u8; 8 * 8 * 4];
        crop_square(&src, -20.0, 0.0, 40.0, &mut crop, 8 * 4, 8);
        assert_eq!(&crop[..3], &[0, 0, 0]);
        assert_eq!(&crop[(2 * 8 + 6) * 4..(2 * 8 + 6) * 4 + 3], &[10, 20, 30]);
    }
}
