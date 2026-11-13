//! Pure geometry of the perception pipeline (spec section 5, feat.*):
//! resize and crops on BGRA frames, argmax and connected components on the
//! seg mask, hand features from 21 keypoints, face features from eye and
//! mouth shapes, the hand tracker (perc.track). No ObjC, all unit-tested.
//!
//! Coordinates: camera frames are unmirrored, so image-left is the
//! person's right. Every `x` feature is from the user's view (0 = the
//! user's left), which is `1 - x_image`.

use std::collections::VecDeque;
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
/// A second hand blob under this share of the first is dropped: real
/// second hands sit at a median 0.58 of the first on S1-S3, and the small
/// ones carry wrong fingers. Picked on the packer's dev segments of S1-S3
/// (present x finger accuracy 0.936 -> 0.938).
pub const HAND_PAIR_MIN: f32 = 0.1;

/// The hand blobs of a frame from `components` (largest first): at most
/// two, the second only when it is not a speck beside the first.
pub fn hand_blobs(blobs: Vec<Blob>) -> Vec<Blob> {
    let mut hands: Vec<Blob> = blobs.into_iter().filter(|b| b.area >= MIN_HAND_AREA).take(2).collect();
    if hands.len() == 2 && (hands[1].area as f32) < HAND_PAIR_MIN * hands[0].area as f32 {
        hands.truncate(1);
    }
    hands
}

/// Borrowed BGRA image.
#[derive(Clone, Copy)]
pub struct Bgra<'a> {
    pub data: &'a [u8],
    pub stride: usize,
    pub w: usize,
    pub h: usize,
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

/// Centroid and count of `other`-class pixels next to (4-neighbour) an
/// `inside`-class pixel, within the blob's box grown by one pixel: where
/// the blob touches e.g. the person class (the wrist for a hand).
pub fn border_centroid(mask: &[u8], w: usize, h: usize, b: &Blob, inside: u8, other: u8) -> ([f32; 2], usize) {
    let (x0, y0) = (b.x0.saturating_sub(1), b.y0.saturating_sub(1));
    let (x1, y1) = ((b.x1 + 1).min(w - 1), (b.y1 + 1).min(h - 1));
    let (mut sx, mut sy, mut n) = (0f32, 0f32, 0usize);
    for y in y0..=y1 {
        for x in x0..=x1 {
            if mask[y * w + x] != other {
                continue;
            }
            let touches = (x > 0 && mask[y * w + x - 1] == inside)
                || (x + 1 < w && mask[y * w + x + 1] == inside)
                || (y > 0 && mask[(y - 1) * w + x] == inside)
                || (y + 1 < h && mask[(y + 1) * w + x] == inside);
            if touches {
                sx += x as f32 + 0.5;
                sy += y as f32 + 0.5;
                n += 1;
            }
        }
    }
    if n == 0 {
        return (b.centre(), 0);
    }
    ([sx / n as f32, sy / n as f32], n)
}

/// Mean x and count of `other`-class pixels in the band below the blob:
/// one blob height deep, one blob width wider on each side.
pub fn below_centroid(mask: &[u8], w: usize, h: usize, b: &Blob, other: u8) -> (f32, usize) {
    let (bw, bh) = (b.x1 - b.x0 + 1, b.y1 - b.y0 + 1);
    let (x0, x1) = (b.x0.saturating_sub(bw), (b.x1 + bw).min(w - 1));
    let (mut sx, mut n) = (0f32, 0usize);
    for y in (b.y1 + 1).min(h)..(b.y1 + 1 + bh).min(h) {
        for x in x0..=x1 {
            if mask[y * w + x] == other {
                sx += x as f32 + 0.5;
                n += 1;
            }
        }
    }
    (if n == 0 { b.centre()[0] } else { sx / n as f32 }, n)
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

    /// The box around the ellipse [x0, y0, x1, y1].
    pub fn bounds(&self) -> [f32; 4] {
        let (a, b) = (self.major / 2.0, self.minor / 2.0);
        let (s, c) = self.angle.sin_cos();
        let ex = ((a * c).powi(2) + (b * s).powi(2)).sqrt();
        let ey = ((a * s).powi(2) + (b * c).powi(2)).sqrt();
        [self.centre[0] - ex, self.centre[1] - ey, self.centre[0] + ex, self.centre[1] + ey]
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
    /// Debounced finger mask for triggers as mask / 31 (`ShapeGate`,
    /// route::shape_mask reads it back).
    pub shape: f32,
    /// False while the hand's ShapeGate has no mask yet (the hand just
    /// came back): `shape` is then the plain mask and goes out as unseen.
    pub shape_settled: bool,
    /// The 21 joints in preview coordinates (0..1, mirrored like the
    /// preview, so x is the person's view), for the window's skeleton.
    pub joints: [[f32; 2]; 21],
    /// The student's finger probabilities thumb..pinky (the geometric
    /// finger values when it has none), for the bench.
    pub ext: [f32; 5],
    /// Palm centre in frame pixels (tracking, hands.distance).
    pub palm: [f32; 2],
    /// Openness after the perception's glide; `values` sends it when set.
    pub open_glide: Option<f32>,
}

/// Finger mean that reads 0 and 1 as openness: the p75 of fist frames and
/// the p25 of open-hand frames (block A, one labelled hand, S1-S3), so a
/// fist reaches 0 and an open hand 1 (Noa, FL tests 11-03), with one slope
/// in between.
const OPEN_LO: f32 = 0.079;
const OPEN_HI: f32 = 0.910;

/// Where `HandFeatures::values` has `shape`.
pub const SHAPE_AT: usize = 20;

impl HandFeatures {
    pub fn openness(&self) -> f32 {
        lin(self.finger.iter().sum::<f32>() / 5.0, OPEN_LO, OPEN_HI)
    }

    pub fn values(&self) -> [f32; 21] {
        let o = self.open_glide.unwrap_or_else(|| self.openness());
        let f = self.finger;
        let t = self.tilt;
        [
            o, 1.0 - o, f[0], f[1], f[2], f[3], f[4], self.spread, self.x, self.y, self.depth,
            self.pinch, self.speed, self.roll, t[0], t[1], t[2], t[3], t[4], self.finger_count, self.shape,
        ]
    }
}

/// Index to pinky, summed joint bend (rad) at extended and folded: the
/// 40th percentile of extended and the 60th of folded block A frames on
/// S1-S3 (non-dev segments).
const FINGER_BEND: [(f32, f32); 4] = [(0.61, 3.68), (0.60, 3.53), (0.62, 3.45), (0.79, 3.56)];
/// Index to pinky, tip reach from the wrist per palm length at folded
/// and extended, from the same frames. Reach shrinks when a finger curls
/// towards the camera, where the bend angle in the image barely changes.
const FINGER_REACH: [(f32, f32); 4] = [(0.96, 1.95), (0.77, 2.00), (0.67, 1.83), (0.60, 1.57)];

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
    for (j, p) in f.joints.iter_mut().zip(kp) {
        *j = [1.0 - (p[0] / fw).clamp(0.0, 1.0), (p[1] / fh).clamp(0.0, 1.0)];
    }
    for (n, ch) in FINGERS.iter().enumerate() {
        // Bend summed over the three joints of the chain; straight = 0.
        let segs = [sub(kp[ch[0]], wrist), sub(kp[ch[1]], kp[ch[0]]), sub(kp[ch[2]], kp[ch[1]]), sub(kp[ch[3]], kp[ch[2]])];
        let bend: f32 = (0..3).map(|j| angle_between(segs[j], segs[j + 1])).sum();
        f.finger[n] = if n == 0 {
            // Thumb: little bend range, so also its tip's reach from the index base.
            0.5 * (1.0 - lin(bend, 0.2, 1.4)) + 0.5 * lin(dist(kp[4], index_mcp) / palm_len, 0.35, 0.9)
        } else {
            let (b, r) = (FINGER_BEND[n - 1], FINGER_REACH[n - 1]);
            // Linear in the cues: a decile map fitted on the B ramps (fast
            // open-and-hold moves) held half-curled fingers at 0.5 to 0.6 and
            // kept fists off 0 (Noa, FL test 11-03).
            0.5 * (1.0 - lin(bend, b.0, b.1)) + 0.5 * lin(dist(kp[ch[3]], wrist) / palm_len, r.0, r.1)
        };
        let dir = sub(kp[ch[3]], kp[if n == 0 { ch[1] } else { ch[0] }]);
        f.tilt[n] = 0.5 - 0.5 * dir[1] / len(dir).max(1e-3);
    }
    let ray = |c: &[usize; 4]| sub(kp[c[3]], kp[c[0]]);
    f.spread = lin(angle_between(ray(&FINGERS[1]), ray(&FINGERS[4])), 0.15, 0.95);
    f.x = 1.0 - (palm[0] / fw).clamp(0.0, 1.0);
    f.y = 1.0 - (palm[1] / fh).clamp(0.0, 1.0);
    f.depth = lin(palm_len / fh, 0.06, 0.35);
    // Touching tips sit 0.1 to 0.3 palm apart (H_pinch_drag, S2 and S3).
    f.pinch = 1.0 - lin(dist(kp[4], kp[8]) / palm_len, 0.25, 0.93);
    // Roll around the forearm: the knuckle line's extent across the
    // wrist->middle axis, signed so the thumb side decides the direction.
    let up = sub(middle_mcp, wrist);
    let knuckles = sub(index_mcp, pinky_mcp);
    let across = (up[0] * knuckles[1] - up[1] * knuckles[0]) / (len(up).max(1e-3) * palm_len);
    let signed = if side == Side::R { -across } else { across };
    f.roll = 0.5 + 0.5 * (signed / 0.8).clamp(-1.0, 1.0);
    let ext = fingers_ext.unwrap_or(f.finger);
    f.ext = ext;
    f.finger_count = ext.iter().filter(|&&e| e > 0.5).count() as f32 / 5.0;
    // without a gate (bench, tests) the plain mask at 0.5
    let mask = ext.iter().enumerate().filter(|(_, &e)| e > 0.5).fold(0u8, |m, (i, _)| m | (1 << (4 - i)));
    f.shape = crate::route::shape_value(mask);
    f.shape_settled = true;
    f
}

/// The value per finger (thumb..pinky) the MIDI triggers threshold: the
/// mean of the student's finger probability and the geometric extension.
/// Of the four candidates in probes/finger-shapes/RESULT.md it read the
/// most exact masks on the dev frames (0.880) with the least flicker, and
/// 0.895 on S4; geometry judges each finger by its own joints, so it also
/// carries the combinations no recording has.
pub fn trigger_fingers(f: &HandFeatures) -> [f32; 5] {
    let mut v = [0.0; 5];
    for (i, x) in v.iter_mut().enumerate() {
        *x = (f.ext[i] + f.finger[i]) / 2.0;
    }
    v
}

/// Finger states for triggers (spec gest.finger_states, cond.hysteresis):
/// a finger counts as shown above its threshold + 0.1 and as folded below
/// its threshold - 0.1 (0.5 each, or the person's own from the finger
/// calibration, calib.rs), and a new mask holds for 60 ms before it
/// replaces the old one, so a finger near its threshold does not chatter
/// notes. Bits: thumb 0b10000 .. pinky 0b00001. A new gate has no mask
/// until the first one held its 60 ms; perception starts a hand's gate
/// over on every frame without that hand, so a hand coming back never
/// replays the mask it left with.
#[derive(Debug, Default, Clone, Copy)]
pub struct ShapeGate {
    lit: u8,
    /// The mask waiting out its dwell, and since when.
    cand: Option<(u8, u64)>,
    mask: Option<u8>,
}

pub const SHAPE_BAND: f32 = 0.1;
const SHAPE_DWELL_NS: u64 = 60_000_000;

impl ShapeGate {
    pub fn update(&mut self, ext: [f32; 5], thr: [f32; 5], t_ns: u64) -> Option<u8> {
        for (f, (&p, &th)) in ext.iter().zip(&thr).enumerate() {
            let bit = 1 << (4 - f);
            if p > th + SHAPE_BAND {
                self.lit |= bit;
            } else if p < th - SHAPE_BAND {
                self.lit &= !bit;
            }
        }
        let (cand, since) = match self.cand {
            Some((c, since)) if c == self.lit => (c, since),
            _ => (self.lit, t_ns),
        };
        self.cand = Some((cand, since));
        if self.mask != Some(cand) && t_ns.saturating_sub(since) >= SHAPE_DWELL_NS {
            self.mask = Some(cand);
        }
        self.mask
    }
}

/// A hand's skeleton (21 joints, x and y), wrist at 0, wrist to middle
/// base pointing up (0, -1), in that length.
pub type Bones = [f32; 42];

/// `j` (preview coordinates, y down) as `Bones`.
pub fn bones(j: &[[f32; 2]; 21]) -> Bones {
    let up = [j[9][0] - j[0][0], j[9][1] - j[0][1]];
    let len = (up[0] * up[0] + up[1] * up[1]).sqrt().max(1e-6);
    let (c, s) = (-up[1] / len, -up[0] / len);
    let mut out = [0.0; 42];
    for (k, p) in j.iter().enumerate() {
        let (x, y) = ((p[0] - j[0][0]) / len, (p[1] - j[0][1]) / len);
        out[2 * k] = c * x - s * y;
        out[2 * k + 1] = s * x + c * y;
    }
    out
}

pub fn bones_dist(a: &Bones, b: &Bones) -> f32 {
    a.iter().zip(b).map(|(a, b)| (a - b) * (a - b)).sum::<f32>().sqrt()
}

/// Per-person skeleton templates from the finger calibration (calib.rs)
/// for poses the hand student reads as one mask: middle alone and ring
/// alone read as the index (Noa, FL 11-04). The finger values cannot
/// tell them apart; the student's skeleton may still differ (where the
/// raised chain starts, how the folded fingers lie). A set holds, for a
/// read mask, the person's mean skeleton of each pose read as it; the
/// calibration only adds a pose whose skeletons it told apart from the
/// read pose's in at least `calib::MIN_APART` of held-out frames.
#[derive(Debug, Clone)]
pub struct MaskTemplates {
    pub sets: Vec<(u8, Vec<(u8, Bones)>)>,
}

impl MaskTemplates {
    pub const EMPTY: Self = Self { sets: Vec::new() };

    /// The mask for a hand the gate reads as `read` with skeleton `b`:
    /// the nearest template's pose when it is nearer than
    /// `TEMPLATE_MARGIN` times the next one, else `last` (the previous
    /// pick of this hand) when that is a pose of the same set, else
    /// `read`. Masks without a set pass through.
    pub fn pick(&self, read: u8, b: &Bones, last: Option<u8>) -> u8 {
        let Some((_, cands)) = self.sets.iter().find(|(r, _)| *r == read) else { return read };
        let mut d: Vec<(f32, u8)> = cands.iter().map(|(m, c)| (bones_dist(b, c), *m)).collect();
        d.sort_by(|a, b| a.0.total_cmp(&b.0));
        match d.as_slice() {
            [(d0, m0), (d1, _), ..] if *d0 <= TEMPLATE_MARGIN * d1 => *m0,
            _ => last.filter(|m| cands.iter().any(|c| c.0 == *m)).unwrap_or(read),
        }
    }
}

/// A template wins only this clearly; between, the hand keeps its last
/// pick, so a point held between the two does not flicker.
const TEMPLATE_MARGIN: f32 = 0.8;

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
    /// Pixel cues inside each eye outline [L, R] (`eye_pixels`).
    pub eye_px: [[f32; 3]; 2],
    /// Mouth cues: outer-lip width / eye distance, corner lift, outer and
    /// inner lip height across the eye line / eye distance.
    pub mouth_px: [f32; 4],
    /// Yaw, pitch and roll are this frame's (else they keep the last ones).
    pub seen_head: bool,
    /// The head pose's points (`head_points`, crop pixels) and its fit
    /// residual, for the bench.
    pub head_pts: [[f32; 2]; 7],
    pub head_fit: f32,
    /// Where the face, the eyes [L, R] and the mouth are, in the input's
    /// pixels from `face_features`; the producer maps it to the preview
    /// (`FaceGeom::map`) for the window's tracking boxes.
    pub geom: FaceGeom,
}

/// Axis-aligned boxes [x0, y0, x1, y1] of the face, the eyes [L, R] (the
/// person's) and the mouth; None for a part not seen.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FaceGeom {
    pub face: Option<[f32; 4]>,
    pub eyes: [Option<[f32; 4]>; 2],
    pub mouth: Option<[f32; 4]>,
}

impl FaceGeom {
    /// Every box through the point map `f` (corners mapped, then sorted
    /// again, so a mirror keeps x0 < x1).
    pub fn map(&self, f: impl Fn([f32; 2]) -> [f32; 2]) -> FaceGeom {
        let m = |b: Option<[f32; 4]>| {
            b.map(|[x0, y0, x1, y1]| {
                let (a, c) = (f([x0, y0]), f([x1, y1]));
                [a[0].min(c[0]), a[1].min(c[1]), a[0].max(c[0]), a[1].max(c[1])]
            })
        };
        FaceGeom { face: m(self.face), eyes: [m(self.eyes[0]), m(self.eyes[1])], mouth: m(self.mouth) }
    }
}

impl FaceFeatures {
    pub fn values(&self) -> [f32; 9] {
        let e = self.eye_open;
        [self.mouth_open, self.smile, e[0], e[1], 1.0 - e[0], 1.0 - e[1], self.yaw, self.pitch, self.roll]
    }
}

/// Luma cues inside an eye outline's box (grown to at least half its
/// width in height, since a closed eye's outline is flat): spread
/// (p90 - p10) / 255, share of pixels darker than half the box median,
/// and the darkest decile / median. An open eye shows dark iris against
/// sclera and skin; a closed lid is skin with a lash line.
pub fn eye_pixels(img: &Bgra, outline: &[[f32; 2]]) -> Option<[f32; 3]> {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in outline {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    let (cy, half) = ((y0 + y1) / 2.0, ((y1 - y0) / 2.0).max((x1 - x0) / 4.0));
    let xa = x0.max(0.0) as usize;
    let xb = (x1.ceil().max(0.0) as usize).min(img.w);
    let ya = (cy - half).max(0.0) as usize;
    let yb = ((cy + half).ceil().max(0.0) as usize).min(img.h);
    if xb <= xa + 2 || yb <= ya + 2 {
        return None;
    }
    let mut luma: Vec<u8> = Vec::with_capacity((xb - xa) * (yb - ya));
    for y in ya..yb {
        let row = &img.data[y * img.stride..];
        for x in xa..xb {
            let px = &row[x * 4..x * 4 + 3];
            luma.push(((29 * px[0] as u32 + 150 * px[1] as u32 + 77 * px[2] as u32) >> 8) as u8);
        }
    }
    luma.sort_unstable();
    let q = |f: f32| luma[((luma.len() - 1) as f32 * f) as usize] as f32;
    let med = q(0.5).max(1.0);
    let dark = luma.iter().filter(|&&v| (v as f32) < 0.5 * med).count() as f32 / luma.len() as f32;
    Some([(q(0.9) - q(0.1)) / 255.0, dark, q(0.1) / med])
}

/// Quantile of the values pushed during the last `win_ns`.
#[derive(Debug)]
pub struct RollingQuantile {
    win_ns: u64,
    q: f32,
    buf: VecDeque<(u64, f32)>,
    scratch: Vec<f32>,
}

impl RollingQuantile {
    pub fn new(win_s: f32, q: f32) -> Self {
        Self { win_ns: (win_s * 1e9) as u64, q, buf: VecDeque::new(), scratch: Vec::new() }
    }

    /// Adds `v` at `t_ns` and returns the window's quantile.
    pub fn push(&mut self, t_ns: u64, v: f32) -> f32 {
        self.buf.push_back((t_ns, v));
        while self.buf.front().is_some_and(|f| t_ns.saturating_sub(f.0) > self.win_ns) {
            self.buf.pop_front();
        }
        self.scratch.clear();
        self.scratch.extend(self.buf.iter().map(|b| b.1));
        let k = ((self.scratch.len() - 1) as f32 * self.q) as usize;
        let (_, kth, _) = self.scratch.select_nth_unstable_by(k, |a, b| a.total_cmp(b));
        *kth
    }

    /// Seconds the window holds, oldest to newest sample.
    pub fn span_s(&self) -> f32 {
        match (self.buf.front(), self.buf.back()) {
            (Some(a), Some(b)) => (b.0 - a.0) as f32 / 1e9,
            _ => 0.0,
        }
    }
}

/// Eye geometry range (outline minor/major) for closed..open.
const EYE_GEO: (f32, f32) = (0.08, 0.2);
/// Gain of the pixel difference between the eyes, and the mean geometry
/// below which it may no longer open an eye (both eyes shut).
const EYE_PX_GAIN: f32 = 8.0;
const EYE_PX_GATE: f32 = 0.2;
/// Seconds of open levels before the pixel difference counts in full: a
/// near-empty window's low decile is its minimum, and the noisier eye
/// then reads as closed (after a cold start, or a hand held over the face
/// for longer than the window). Swept on S1-S3 block C from a cold start.
const EYE_PX_WARM: f32 = 8.0;
/// Mouth width over its running median for smile 0 and 1.
const SMILE_WIDTH: (f32, f32) = (1.05, 1.2);

/// Per-person face calibration of the `hybrid` backend, fitted on S1 to S3
/// block C (probes/plugin-perception). Eyes: the landmark outline gives
/// how open both eyes are together, the pixels inside each outline say
/// which one is more closed. Vision's landmarks move both outlines in a
/// wink; the pixel spread does not. Each eye's spread is taken against its
/// own open level (low decile of the closure over 10 s), which also
/// absorbs glasses and lighting. Smile: mouth width over its 20 s median.
#[derive(Debug)]
pub struct FaceAdapt {
    closure_open: [RollingQuantile; 2],
    mouth_width: RollingQuantile,
}

impl Default for FaceAdapt {
    fn default() -> Self {
        Self {
            closure_open: [RollingQuantile::new(10.0, 0.1), RollingQuantile::new(10.0, 0.1)],
            mouth_width: RollingQuantile::new(20.0, 0.5),
        }
    }
}

impl FaceAdapt {
    /// `hand_on_face`: a hand blob overlaps the face box; its skin and
    /// shadow then reach the eye boxes, so the eyes fall back to the
    /// outline and the open levels are left alone.
    pub fn apply(&mut self, f: &mut FaceFeatures, t_ns: u64, hand_on_face: bool) {
        if hand_on_face {
            for e in 0..2 {
                if f.seen_eye[e] {
                    f.eye_open[e] = lin(f.raw[e], EYE_GEO.0, EYE_GEO.1);
                }
            }
        } else if f.seen_eye[0] && f.seen_eye[1] && f.eye_px[0][0] > 0.0 && f.eye_px[1][0] > 0.0 {
            let g = [lin(f.raw[0], EYE_GEO.0, EYE_GEO.1), lin(f.raw[1], EYE_GEO.0, EYE_GEO.1)];
            let mut p = [0f32; 2];
            for (e, pe) in p.iter_mut().enumerate() {
                let closure = -f.eye_px[e][0];
                *pe = closure - self.closure_open[e].push(t_ns, closure);
            }
            // > 0: this eye looks more open than the other.
            let d = [p[1] - p[0], p[0] - p[1]];
            let gate = ((g[0] + g[1]) / 2.0 / EYE_PX_GATE).min(1.0);
            let span = self.closure_open[0].span_s().min(self.closure_open[1].span_s());
            let gain = EYE_PX_GAIN * (span / EYE_PX_WARM).min(1.0);
            for e in 0..2 {
                let shift = if d[e] < 0.0 { gain * d[e] } else { gain * d[e] * gate };
                f.eye_open[e] = (g[e] + shift).clamp(0.0, 1.0);
            }
        }
        if f.mouth_px[0] > 0.0 {
            let median = self.mouth_width.push(t_ns, f.mouth_px[0]).max(1e-3);
            f.smile = lin(f.mouth_px[0] / median, SMILE_WIDTH.0, SMILE_WIDTH.1);
        }
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

/// Extent of a lip outline across the eye line, per eye distance: a
/// height that does not shrink with the mouth's width, unlike minor/major.
pub fn height_across(p: &[[f32; 2]], eye_a: [f32; 2], eye_b: [f32; 2]) -> f32 {
    let d = dist(eye_a, eye_b).max(1.0);
    let n = [(eye_a[1] - eye_b[1]) / d, (eye_b[0] - eye_a[0]) / d];
    let (lo, hi) = p.iter().map(|q| q[0] * n[0] + q[1] * n[1]).fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
    (hi - lo).max(0.0) / d
}

/// Inner-lip gap (`height_across`) for mouth open 0 and 1, from S1-S3
/// block C: closed 0.02 to 0.04, mouth wide 0.52 to 0.62. The inner-lip
/// minor/major ratio it replaces jumps when the lips part and reads open
/// in a smile or a head turn.
pub const MOUTH_GAP: (f32, f32) = (0.06, 0.6);

pub fn mouth_from_gap(gap: f32) -> f32 {
    lin(gap, MOUTH_GAP.0, MOUTH_GAP.1)
}

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
    f.geom.face = Some([fx, fy, fx + fw, fy + fh]);
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
            f.geom.eyes[s] = Some(e.bounds());
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
        f.seen_head = true;
    }
    if let Some(m) = mouth {
        f.raw[2] = m.ratio();
        f.raw[3] = m.major / fw.max(1.0);
        f.mouth_open = lin(m.ratio(), cal.mouth.0, cal.mouth.1);
        f.smile = lin(m.major / fw.max(1.0), cal.smile.0, cal.smile.1);
        f.seen_mouth = true;
        f.geom.mouth = Some(m.bounds());
    }
    f
}

/// Rigid points of the head pose model in eye distances: x to the
/// person's left (image right when they face the camera), y down the face,
/// z into the head. Eyes (image-left, image-right), nose tip, bridge, nose
/// wings (image-left, image-right), subnasale. A generic face, not fitted;
/// no mouth or chin point, so speaking and smiling leave the pose alone.
const HEAD_MODEL: [[f32; 3]; 7] = [
    [-0.5, 0.0, 0.0],
    [0.5, 0.0, 0.0],
    [0.0, 0.62, -0.45],
    [0.0, 0.0, -0.20],
    [-0.28, 0.70, -0.12],
    [0.28, 0.70, -0.12],
    [0.0, 0.74, -0.25],
];
/// RMS fit residual (eye distances) above which a frame's pose does not
/// count: the landmarks have left the face's shape (a nod far down, a hand
/// over the nose). Neutral frames fit to 0.008; frames whose yaw jumps by
/// 40 to 60 degrees in the pitch sweeps sit at 0.1 to 0.5. Swept on S1-S3
/// block C (probes/head-signals/poselab.py): 0.05.
pub const HEAD_FIT_MAX: f32 = 0.05;
/// Degrees from the neutral pose for value 0 and 1: yaw, pitch, roll.
pub const HEAD_FULL_DEG: [f32; 3] = [45.0, 30.0, 35.0];
/// Seconds of poses the neutral pose is the median of: absorbs where the
/// camera sits (above, below, to the side); a pose held this long half
/// becomes the new neutral.
pub const HEAD_NEUTRAL_S: f32 = 30.0;

type Mat3 = [[f32; 3]; 3];

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit3(a: [f32; 3]) -> Option<[f32; 3]> {
    let n = dot3(a, a).sqrt();
    (n > 1e-9).then(|| [a[0] / n, a[1] / n, a[2] / n])
}

/// Rows of a rotation from its first two rows, made orthonormal.
fn orthonormal(r1: [f32; 3], r2: [f32; 3]) -> Option<Mat3> {
    let r1 = unit3(r1)?;
    let r3 = unit3(cross3(r1, r2))?;
    Some([r1, cross3(r3, r1), r3])
}

fn inv3(m: Mat3) -> Option<Mat3> {
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if det.abs() < 1e-9 {
        return None;
    }
    let k = 1.0 / det;
    Some([
        [(e * i - f * h) * k, (c * h - b * i) * k, (b * f - c * e) * k],
        [(f * g - d * i) * k, (a * i - c * g) * k, (c * d - a * f) * k],
        [(d * h - e * g) * k, (b * g - a * h) * k, (a * e - b * d) * k],
    ])
}

/// (A^T A)^-1 A^T of the model points: maps image offsets to a scaled row.
fn model_pinv() -> Option<[[f32; 7]; 3]> {
    let mut ata = [[0f32; 3]; 3];
    for p in &HEAD_MODEL {
        for r in 0..3 {
            for c in 0..3 {
                ata[r][c] += p[r] * p[c];
            }
        }
    }
    let k = inv3(ata)?;
    let mut out = [[0f32; 7]; 3];
    for (r, row) in out.iter_mut().enumerate() {
        for (n, p) in HEAD_MODEL.iter().enumerate() {
            row[n] = dot3(k[r], *p);
        }
    }
    Some(out)
}

/// The model's points from Vision's landmark regions, all in one image
/// space: eye centres (image-left, image-right), nose crest, nose outline,
/// median line. Each is picked in the eye-line frame, so a tilted head
/// picks the same points: the tip is the crest point on the median line
/// furthest down the face, the bridge the crest's top, the wings the
/// nose outline's outer points in its bottom quarter, the subnasale the
/// median line's next point below the tip.
pub fn head_points(eyes: [[f32; 2]; 2], crest: &[[f32; 2]], nose: &[[f32; 2]], median: &[[f32; 2]]) -> Option<[[f32; 2]; 7]> {
    let [a, b] = eyes;
    let d = dist(a, b);
    if d <= 1.0 || crest.is_empty() || nose.len() < 4 {
        return None;
    }
    let ex = [(b[0] - a[0]) / d, (b[1] - a[1]) / d];
    let along = |p: &[f32; 2]| p[0] * ex[0] + p[1] * ex[1];
    let down = |p: &[f32; 2]| -p[0] * ex[1] + p[1] * ex[0];
    let on_median: Vec<[f32; 2]> =
        crest.iter().copied().filter(|p| median.iter().any(|m| (p[0] - m[0]).abs() + (p[1] - m[1]).abs() < 0.5)).collect();
    let ridge = if on_median.is_empty() { crest } else { &on_median };
    let tip = *ridge.iter().max_by(|p, q| down(p).total_cmp(&down(q)))?;
    let bridge = *crest.iter().min_by(|p, q| down(p).total_cmp(&down(q)))?;
    let (lo, hi) = nose.iter().map(down).fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
    let low: Vec<[f32; 2]> = nose.iter().copied().filter(|p| down(p) >= lo + 0.75 * (hi - lo)).collect();
    let wing_l = *low.iter().min_by(|p, q| along(p).total_cmp(&along(q)))?;
    let wing_r = *low.iter().max_by(|p, q| along(p).total_cmp(&along(q)))?;
    let below = median.iter().filter(|m| down(m) > down(&tip) + 0.5);
    let sub = below.min_by(|p, q| down(p).total_cmp(&down(q))).copied().unwrap_or(tip);
    Some([a, b, tip, bridge, wing_l, wing_r, sub])
}

/// Weak-perspective head rotation (POS: least squares of the model's
/// scaled rows on the points' offsets from the eye midpoint) and the RMS
/// residual of the fit per eye distance. Rows map model to image (y down).
fn head_rotation(pinv: &[[f32; 7]; 3], pts: &[[f32; 2]; 7]) -> Option<(Mat3, f32)> {
    let o = [(pts[0][0] + pts[1][0]) / 2.0, (pts[0][1] + pts[1][1]) / 2.0];
    let d = dist(pts[0], pts[1]).max(1.0);
    let row = |k: usize| -> [f32; 3] { std::array::from_fn(|r| (0..7).map(|n| pinv[r][n] * (pts[n][k] - o[k])).sum()) };
    let (i, j) = (row(0), row(1));
    let err: f32 = HEAD_MODEL.iter().zip(pts).map(|(m, p)| (dot3(i, *m) - (p[0] - o[0])).powi(2) + (dot3(j, *m) - (p[1] - o[1])).powi(2)).sum();
    Some((orthonormal(i, j)?, (err / 7.0).sqrt() / d))
}

fn mat_mul_t(a: &Mat3, b: &Mat3) -> Mat3 {
    // a^T b
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[k][r] * b[k][c]).sum()))
}

/// Yaw to the person's left, pitch up, roll to the left shoulder (radians)
/// of a head rotation: yaw and pitch from where the face points, roll as
/// the head's x axis turned around that direction, against the same axis
/// without roll.
fn head_angles(r: &Mat3) -> Option<[f32; 3]> {
    let col = |c: usize| [r[0][c], r[1][c], r[2][c]];
    let z = col(2);
    let f = [-z[0], -z[1], -z[2]];
    let u = col(0);
    let yaw = f[0].atan2(-f[2]);
    let pitch = (-f[1]).atan2(f[0].hypot(f[2]));
    let h0 = unit3(cross3([0.0, -1.0, 0.0], f))?;
    let roll = -dot3(cross3(h0, u), f).atan2(dot3(h0, u));
    Some([yaw, pitch, roll])
}

/// Head yaw, pitch and roll from the rigid landmarks against the running
/// neutral pose (the elementwise median of the poses of the last
/// `HEAD_NEUTRAL_S`, made a rotation again). Values follow the ontology: yaw 0
/// turned to the person's left, pitch 0 looking down, roll 0 tilted to the
/// left shoulder, 0.5 neutral.
#[derive(Debug, Default)]
pub struct HeadPose {
    hist: VecDeque<(u64, Mat3)>,
    n: u64,
    scratch: Vec<f32>,
    last: Option<[f32; 3]>,
}

impl HeadPose {
    /// The last values `update` gave, neutral before the first.
    pub fn last(&self) -> [f32; 3] {
        self.last.unwrap_or([0.5; 3])
    }

    /// The values for `pts` (`head_points`) and the fit residual; None for
    /// the values when the fit is past `HEAD_FIT_MAX`.
    pub fn update(&mut self, pts: &[[f32; 2]; 7], t_ns: u64) -> (Option<[f32; 3]>, f32) {
        let Some((r, fit)) = model_pinv().and_then(|p| head_rotation(&p, pts)) else { return (None, f32::MAX) };
        if fit > HEAD_FIT_MAX {
            return (None, fit);
        }
        // every third frame: 300 poses over 30 s at 30 fps
        if self.n.is_multiple_of(3) || self.hist.is_empty() {
            self.hist.push_back((t_ns, r));
        }
        self.n += 1;
        let win = (HEAD_NEUTRAL_S * 1e9) as u64;
        while self.hist.front().is_some_and(|h| t_ns.saturating_sub(h.0) > win) {
            self.hist.pop_front();
        }
        let (Some(neutral), Some(_)) = (self.neutral(), self.hist.back()) else { return (None, fit) };
        let Some(a) = head_angles(&mat_mul_t(&neutral, &r)) else { return (None, fit) };
        let v = |k: usize, sign: f32| 0.5 + 0.5 * sign * (a[k].to_degrees() / HEAD_FULL_DEG[k]).clamp(-1.0, 1.0);
        self.last = Some([v(0, -1.0), v(1, 1.0), v(2, -1.0)]);
        (self.last, fit)
    }

    fn neutral(&mut self) -> Option<Mat3> {
        let mut m = [[0f32; 3]; 3];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, x) in row.iter_mut().enumerate() {
                self.scratch.clear();
                self.scratch.extend(self.hist.iter().map(|h| h.1[i][j]));
                let k = self.scratch.len().checked_sub(1)? / 2;
                *x = *self.scratch.select_nth_unstable_by(k, |a, b| a.total_cmp(b)).1;
            }
        }
        orthonormal(m[0], m[1])
    }
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

/// Which way the forearm leaves the hand: the wrist keypoint's x offset
/// from the hand blob's centre per palm size, -1..1. The forearm comes
/// from the hand's own body side, so > 0 (wrist to the image right) means
/// the person's left hand, also for a hand held at the image centre. On
/// S1 to S3 block A its sign alone gets 87 to 94% of one-hand frames right.
pub fn wrist_lean(kp: &[[f32; 2]; 21], centre: [f32; 2]) -> f32 {
    let palm = dist(kp[WRIST], kp[9]).max(1.2 * dist(kp[5], kp[17])).max(1.0);
    ((kp[WRIST][0] - centre[0]) / palm).clamp(-1.0, 1.0)
}

/// Side vote of one hand blob for `SideTracker`, > 0 = the person's left:
/// `wrist_lean` plus the hand student's handedness logit (> 0 = right),
/// scaled to -1..1 at |logit| 2, as an equal second vote. On S1 to S3
/// block A the logit's sign alone gets 99% of one-hand frames right
/// (in-sample for the model), the lean 87 to 95%.
pub fn side_vote(lean: f32, handed: Option<f32>) -> f32 {
    lean + handed.map_or(0.0, |h| (-h / 2.0).clamp(-1.0, 1.0))
}

/// Side assignment for the seg student's hand blobs. Each blob belongs to
/// an identity track (nearest track within a quarter frame width that was
/// seen in the last `SIDE_GAP_NS`); a track sums the `side_vote` of its
/// frames, capped at `SIDE_CAP`, starting from a small prior from its
/// position against the face (else frame) centre. Two hands split by
/// image side; one hand takes its track's sign. Tracks expire after
/// `EXPIRE_NS` unseen.
#[derive(Debug, Default)]
pub struct SideTracker {
    tracks: Vec<SideTrack>,
}

#[derive(Debug, Clone, Copy)]
struct SideTrack {
    pos: [f32; 2],
    t_ns: u64,
    lean: f32,
    /// The side this track read last as a single hand.
    side: Option<Side>,
}

const SIDE_PRIOR: f32 = 0.3;
/// A track's summed vote stays within +-3, about two frames of a typical
/// vote (median |vote| 1.15 to 1.43 on S1 to S4 block A). At the old cap
/// of 40 a hand that took the other hand's place inherited its side for
/// a second (hand switch at the same spot, probes/finger-shapes/sides.md,
/// S1 to S3: median 30 wrong frames, max 50); at 3 and with `SIDE_GAP_NS`
/// the new hand reads right after a median of 1 frame (S4, scored once:
/// p90 2, max 6), and one-hand frames keep their wrong-side share (S1 to
/// S3 0.25%, was 0.20%; S4 0.28%, was 0.37%).
const SIDE_CAP: f32 = 3.0;
/// A track unseen for longer than this is not taken over by a new blob:
/// a hand that went out and one that came in are two hands.
const SIDE_GAP_NS: u64 = 150_000_000;
/// A single hand keeps the side its track had until the summed vote
/// passes this against it: a fist's handedness vote wavers around 0 and
/// flipped the hand to the other side for a few frames (Noa, FL 11-04;
/// probes/fist-glide/RESULT.md).
const SIDE_FLIP: f32 = 1.0;

impl SideTracker {
    /// `hands`: blob centre in frame px and its `side_vote` (0 without
    /// keypoints), at most two, largest first.
    pub fn assign(&mut self, hands: &[([f32; 2], f32)], face_cx: Option<f32>, fw: f32, t_ns: u64) -> Vec<Side> {
        self.tracks.retain(|t| t_ns.saturating_sub(t.t_ns) < EXPIRE_NS);
        let mut pairs: Vec<(f32, usize, usize)> = hands
            .iter()
            .enumerate()
            .flat_map(|(i, h)| {
                self.tracks
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t_ns.saturating_sub(t.t_ns) <= SIDE_GAP_NS)
                    .map(move |(j, t)| (dist(h.0, t.pos), i, j))
            })
            .collect();
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut track_of: Vec<Option<usize>> = vec![None; hands.len()];
        let mut taken = vec![false; self.tracks.len()];
        for (d, i, j) in pairs {
            if d < MATCH_FRAC * fw && track_of[i].is_none() && !taken[j] {
                track_of[i] = Some(j);
                taken[j] = true;
            }
        }
        let reference = face_cx.unwrap_or(fw / 2.0);
        let idx: Vec<usize> = hands
            .iter()
            .zip(track_of)
            .map(|(h, m)| {
                m.unwrap_or_else(|| {
                    let prior = SIDE_PRIOR * ((h.0[0] - reference) / (0.25 * fw)).clamp(-1.0, 1.0);
                    self.tracks.push(SideTrack { pos: h.0, t_ns, lean: prior, side: None });
                    self.tracks.len() - 1
                })
            })
            .collect();
        for (h, &j) in hands.iter().zip(&idx) {
            let t = &mut self.tracks[j];
            t.lean = (t.lean + h.1).clamp(-SIDE_CAP, SIDE_CAP);
            t.pos = h.0;
            t.t_ns = t_ns;
        }
        let sides = match hands {
            [a, b] => {
                if a.0[0] < b.0[0] {
                    vec![Side::R, Side::L]
                } else {
                    vec![Side::L, Side::R]
                }
            }
            [_] => {
                let t = &mut self.tracks[idx[0]];
                let side = match t.side {
                    Some(Side::L) if t.lean > -SIDE_FLIP => Side::L,
                    Some(Side::R) if t.lean < SIDE_FLIP => Side::R,
                    _ if t.lean > 0.0 => Side::L,
                    _ => Side::R,
                };
                t.side = Some(side);
                vec![side]
            }
            _ => vec![],
        };
        // Keep the two newest tracks.
        let extra = self.tracks.len().saturating_sub(2);
        self.tracks.drain(..extra);
        sides
    }
}

/// Whether two blobs' bounding boxes overlap.
pub fn boxes_overlap(a: &Blob, b: &Blob) -> bool {
    a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
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

    #[test]
    fn shape_gate_debounces_finger_states() {
        let mut g = ShapeGate::default();
        let ms = |m: u64| m * 1_000_000;
        // index shown: the mask follows after 60 ms, not on the first frame
        assert_eq!(g.update([0.1, 0.9, 0.1, 0.1, 0.1], [0.5; 5], ms(0)), None);
        assert_eq!(g.update([0.1, 0.9, 0.1, 0.1, 0.1], [0.5; 5], ms(33)), None);
        assert_eq!(g.update([0.1, 0.9, 0.1, 0.1, 0.1], [0.5; 5], ms(66)), Some(0b01000));
        // a probability wandering around 0.5 changes nothing
        for (i, p) in [0.45, 0.55, 0.42, 0.58].iter().enumerate() {
            assert_eq!(g.update([0.1, *p, 0.1, 0.1, 0.1], [0.5; 5], ms(100 + 33 * i as u64)), Some(0b01000));
        }
        // a one-frame flicker of the middle finger is no new shape
        assert_eq!(g.update([0.1, 0.9, 0.9, 0.1, 0.1], [0.5; 5], ms(300)), Some(0b01000));
        assert_eq!(g.update([0.1, 0.9, 0.1, 0.1, 0.1], [0.5; 5], ms(333)), Some(0b01000));
        assert_eq!(g.update([0.1, 0.9, 0.1, 0.1, 0.1], [0.5; 5], ms(400)), Some(0b01000));
        // thumb up held: 0b10000 + the index still up
        assert_eq!(g.update([0.9, 0.9, 0.1, 0.1, 0.1], [0.5; 5], ms(500)), Some(0b01000));
        assert_eq!(g.update([0.9, 0.9, 0.1, 0.1, 0.1], [0.5; 5], ms(566)), Some(0b11000));
    }

    #[test]
    fn a_hand_that_comes_back_waits_out_its_first_mask() {
        let ms = |m: u64| m * 1_000_000;
        let (index, fist) = ([0.1, 0.9, 0.1, 0.1, 0.1], [0.1; 5]);
        let mut g = ShapeGate::default();
        for t in [0, 33, 66, 100] {
            g.update(index, [0.5; 5], ms(t));
        }
        assert_eq!(g.update(index, [0.5; 5], ms(133)), Some(0b01000));
        // the hand leaves; perception starts its gate over. It comes back
        // as a fist: neither the old index nor a fist before 60 ms
        let mut g = ShapeGate::default();
        assert_eq!(g.update(fist, [0.5; 5], ms(2000)), None);
        assert_eq!(g.update(fist, [0.5; 5], ms(2033)), None);
        assert_eq!(g.update(fist, [0.5; 5], ms(2066)), Some(0));
    }

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
                // Fingers about one palm long (wrist to middle base, 105 px).
                let step = if n == 0 { 30.0 } else { 44.0 - 6.0 * j as f32 };
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
            // The synthetic curl stays in the image plane, so the tips end
            // about a palm from the wrist (real fists: 0.6); folded, not 0.
            assert!(open.finger[n] > 0.9 && fist.finger[n] < 0.5, "finger {n}: {} {}", open.finger[n], fist.finger[n]);
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
        assert_eq!(f.geom.mouth, Some([130.0, 175.0, 170.0, 205.0]));
        assert_eq!(f.geom.face, Some([100.0, 100.0, 200.0, 220.0]));
    }

    /// The tracking boxes: an ellipse's bounds, the person's eyes by side,
    /// and a mirrored map keeps every box ordered.
    #[test]
    fn face_boxes_follow_the_parts() {
        let tilted = Shape { centre: [0.0, 0.0], major: 20.0, minor: 10.0, angle: PI / 2.0 };
        let b = tilted.bounds();
        assert!((b[0] + 5.0).abs() < 1e-3 && (b[1] + 10.0).abs() < 1e-3, "{b:?}");
        let face = [100.0, 100.0, 100.0, 120.0];
        let eye = |cx: f32| Shape { centre: [cx, 148.0], major: 20.0, minor: 8.0, angle: 0.0 };
        let f = face_features(face, &[eye(130.0), eye(170.0)], None, CAL_MASK);
        // the image-right eye is the person's left
        assert_eq!(f.geom.eyes[Side::L as usize], Some([160.0, 144.0, 180.0, 152.0]));
        assert_eq!(f.geom.eyes[Side::R as usize], Some([120.0, 144.0, 140.0, 152.0]));
        assert_eq!(f.geom.mouth, None);
        let m = f.geom.map(|p| [1.0 - p[0] / 400.0, p[1] / 300.0]);
        let l = m.eyes[Side::L as usize].unwrap_or_default();
        assert!(l[0] < l[2] && (l[0] - 0.55).abs() < 1e-4 && (l[2] - 0.6).abs() < 1e-4, "{l:?}");
    }

    /// The model's points seen through rotation `r` (rows, model to
    /// image), scaled to an eye distance of 100 px.
    fn head_image(r: [[f32; 3]; 3]) -> [[f32; 2]; 7] {
        HEAD_MODEL.map(|p| [300.0 + 100.0 * dot3(r[0], p), 200.0 + 100.0 * dot3(r[1], p)])
    }

    fn rot(axis: usize, deg: f32) -> [[f32; 3]; 3] {
        let (s, c) = deg.to_radians().sin_cos();
        match axis {
            // yaw: the face turns to the person's left (towards image right)
            0 => [[c, 0.0, -s], [0.0, 1.0, 0.0], [s, 0.0, c]],
            // pitch: the face turns up
            1 => [[1.0, 0.0, 0.0], [0.0, c, s], [0.0, -s, c]],
            // roll: the person's left eye (image right) goes down
            _ => [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    fn settled(pose: &mut HeadPose, r: [[f32; 3]; 3]) {
        for i in 0..30 {
            pose.update(&head_image(r), i * 33_000_000);
        }
    }

    #[test]
    fn head_pose_follows_the_ontology_directions() {
        let id = rot(0, 0.0);
        for (axis, deg, want_low) in [(0, 30.0, true), (0, -30.0, false), (1, 20.0, false), (1, -20.0, true), (2, 25.0, true), (2, -25.0, false)] {
            let mut pose = HeadPose::default();
            settled(&mut pose, id);
            let (v, fit) = pose.update(&head_image(rot(axis, deg)), 1_000_000_000);
            assert!(fit < 1e-4, "axis {axis}: fit {fit}");
            let v = v.unwrap_or([0.5; 3]);
            assert_eq!(v[axis] < 0.5, want_low, "axis {axis} {deg} deg: {v:?}");
            for other in (0..3).filter(|o| *o != axis) {
                assert!((v[other] - 0.5).abs() < 0.02, "axis {axis} leaks into {other}: {v:?}");
            }
            let full = 0.5 * deg.abs() / HEAD_FULL_DEG[axis];
            assert!(((v[axis] - 0.5).abs() - full).abs() < 0.02, "axis {axis}: {v:?}");
        }
    }

    #[test]
    fn head_pose_takes_the_camera_out() {
        // camera above and to the side: every pose seen through it
        let mul = |a: [[f32; 3]; 3], b: [[f32; 3]; 3]| -> [[f32; 3]; 3] {
            std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
        };
        let cam = mul(rot(1, -20.0), rot(0, 15.0));
        let mut pose = HeadPose::default();
        settled(&mut pose, cam);
        let (v, _) = pose.update(&head_image(cam), 1_000_000_000);
        let v = v.unwrap_or([0.0; 3]);
        assert!(v.iter().all(|x| (x - 0.5).abs() < 0.01), "neutral through the camera: {v:?}");
        // a pure tilt through the camera reads as a tilt
        let (v, _) = pose.update(&head_image(mul(cam, rot(2, 20.0))), 1_033_000_000);
        let v = v.unwrap_or([0.5; 3]);
        assert!(v[2] < 0.3 && (v[0] - 0.5).abs() < 0.03 && (v[1] - 0.5).abs() < 0.03, "{v:?}");
    }

    #[test]
    fn a_misfit_holds_the_head() {
        let mut pose = HeadPose::default();
        settled(&mut pose, rot(0, 0.0));
        let mut pts = head_image(rot(0, 0.0));
        // the tip lands on an eye: no face has that shape
        pts[2] = pts[0];
        let (v, fit) = pose.update(&pts, 1_000_000_000);
        assert!(v.is_none() && fit > HEAD_FIT_MAX, "fit {fit}");
    }

    #[test]
    fn head_points_pick_in_the_eye_line_frame() {
        // Vision's regions of a face tilted 40 degrees: crest (ridge and
        // two side points), nose outline, median line
        let (s, c) = 40f32.to_radians().sin_cos();
        let t = |x: f32, y: f32| [300.0 + c * x - s * y, 200.0 + s * x + c * y];
        let eyes = [t(-50.0, 0.0), t(50.0, 0.0)];
        let crest = [t(0.0, 0.0), t(0.0, 20.0), t(0.0, 40.0), t(0.0, 60.0), t(-20.0, 61.0), t(20.0, 63.0)];
        let nose = [t(0.0, 20.0), t(-28.0, 46.0), t(-29.0, 71.0), t(-14.0, 72.0), t(0.0, 75.0), t(13.0, 73.0), t(26.0, 73.0), t(27.0, 48.0)];
        let median = [t(0.0, 0.0), t(0.0, 20.0), t(0.0, 40.0), t(0.0, 60.0), t(-1.0, 75.0), t(-1.0, 94.0)];
        let p = head_points(eyes, &crest, &nose, &median).unwrap_or([[0.0; 2]; 7]);
        let near = |a: [f32; 2], b: [f32; 2]| dist(a, b) < 0.01;
        assert!(near(p[2], t(0.0, 60.0)), "tip {:?}", p[2]);
        assert!(near(p[3], t(0.0, 0.0)), "bridge {:?}", p[3]);
        assert!(near(p[4], t(-29.0, 71.0)) && near(p[5], t(26.0, 73.0)), "wings {:?} {:?}", p[4], p[5]);
        assert!(near(p[6], t(-1.0, 75.0)), "subnasale {:?}", p[6]);
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
    fn side_tracker_follows_the_wrist_for_a_centred_hand() {
        // A hand at the image centre, right of the face, wrist to the image
        // left: the person's right hand coming in from its own side.
        let mut kp = [[640.0f32, 300.0]; 21];
        kp[WRIST] = [600.0, 400.0];
        kp[9] = [640.0, 300.0];
        kp[5] = [620.0, 300.0];
        kp[17] = [660.0, 300.0];
        let lean = wrist_lean(&kp, [640.0, 330.0]);
        assert!(lean < -0.3, "{lean}");
        let mut t = SideTracker::default();
        let face = Some(600.0);
        // The position prior alone says L (hand right of the face) ...
        assert_eq!(t.assign(&[([640.0, 330.0], 0.0)], face, 1280.0, 0), vec![Side::L]);
        // ... the wrist turns it within a frame and the track keeps it.
        let mut t = SideTracker::default();
        for k in 0..10u64 {
            assert_eq!(t.assign(&[([640.0, 330.0], lean)], face, 1280.0, k * 33_000_000), vec![Side::R]);
        }
        // A few contrary frames do not flip a settled track.
        for k in 10..13u64 {
            assert_eq!(t.assign(&[([645.0, 330.0], 0.5)], face, 1280.0, k * 33_000_000), vec![Side::R]);
        }
        // Two hands split by image side regardless of lean.
        assert_eq!(
            t.assign(&[([900.0, 300.0], -1.0), ([300.0, 300.0], 1.0)], face, 1280.0, 14 * 33_000_000),
            vec![Side::L, Side::R]
        );
        // After the track expires a new hand starts from its own evidence.
        assert_eq!(t.assign(&[([640.0, 330.0], 0.6)], face, 1280.0, 2_000_000_000), vec![Side::L]);
    }

    #[test]
    fn a_hand_taking_the_other_hands_place_gets_its_own_side() {
        // Noa, FL 11-04: the right hand read as left until the left hand
        // came back. The left hand is held 3 s (typical vote +1.3), goes
        // out, and the right hand (vote -1.3) comes in at the same spot.
        const FRAME: u64 = 33_000_000;
        let face = Some(640.0);
        for gap_frames in [3u64, 9] {
            let mut t = SideTracker::default();
            for k in 0..90u64 {
                assert_eq!(t.assign(&[([600.0, 500.0], 1.3)], face, 1280.0, k * FRAME), vec![Side::L]);
            }
            let start = 90 + gap_frames;
            let sides: Vec<Side> = (start..start + 10)
                .map(|k| t.assign(&[([600.0, 500.0], -1.3)], face, 1280.0, k * FRAME)[0])
                .collect();
            // at most the first three frames keep the old side
            assert!(sides[3..].iter().all(|s| *s == Side::R), "gap {gap_frames}: {sides:?}");
            if gap_frames * FRAME > SIDE_GAP_NS {
                assert_eq!(sides[0], Side::R, "a hand after a gap is a new hand");
            }
        }
    }

    #[test]
    fn side_vote_adds_handedness() {
        assert_eq!(side_vote(0.3, None), 0.3);
        // A confident right hand outvotes a small lean to the left.
        assert!(side_vote(0.3, Some(4.0)) < 0.0);
        assert!((side_vote(-0.2, Some(-1.0)) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn face_adapt_splits_a_wink_by_pixels() {
        let mut a = FaceAdapt::default();
        let open = |f: &mut FaceFeatures| {
            f.seen_eye = [true, true];
            f.raw = [0.3, 0.3, 0.05, 0.0];
            f.eye_px = [[0.4, 0.1, 0.4], [0.4, 0.1, 0.4]];
            f.mouth_px = [0.8, 0.0, 0.0, 0.0];
        };
        // Nine seconds of open eyes set the open levels and the mouth width.
        for k in 0..270u64 {
            let mut f = FaceFeatures::default();
            open(&mut f);
            a.apply(&mut f, k * 33_000_000, false);
            assert!(f.eye_open[0] > 0.99 && f.eye_open[1] > 0.99 && f.smile < 0.01);
        }
        // Wink L: both outlines half shut, only the left eye loses spread.
        let mut f = FaceFeatures::default();
        open(&mut f);
        f.raw = [0.14, 0.14, 0.05, 0.0];
        f.eye_px[0][0] = 0.25;
        f.mouth_px = [0.96, 0.0, 0.0, 0.0];
        a.apply(&mut f, 271 * 33_000_000, false);
        assert!(f.eye_open[0] < 0.1 && f.eye_open[1] > 0.9, "{:?}", f.eye_open);
        assert!(f.smile > 0.9, "{}", f.smile);
        // Both shut: the pixel difference may not reopen an eye.
        let mut f = FaceFeatures::default();
        open(&mut f);
        f.raw = [0.06, 0.06, 0.05, 0.0];
        f.eye_px[0][0] = 0.3;
        a.apply(&mut f, 272 * 33_000_000, false);
        assert!(f.eye_open[0] < 0.01 && f.eye_open[1] < 0.01, "{:?}", f.eye_open);
        // A hand on the face: outline only, even with lopsided pixels.
        let mut f = FaceFeatures::default();
        open(&mut f);
        f.eye_px[1][0] = 0.1;
        a.apply(&mut f, 273 * 33_000_000, true);
        assert!(f.eye_open[0] > 0.99 && f.eye_open[1] > 0.99, "{:?}", f.eye_open);
    }

    #[test]
    fn face_adapt_warms_up_before_trusting_pixels() {
        // A cold start with one noisy eye: its low decile is its widest
        // spread, so the other frames would read as closing.
        let mut a = FaceAdapt::default();
        for k in 0..30u64 {
            let mut f = FaceFeatures { seen_eye: [true, true], raw: [0.3, 0.3, 0.05, 0.0], ..Default::default() };
            f.eye_px = [[if k % 3 == 0 { 0.55 } else { 0.4 }, 0.1, 0.4], [0.4, 0.1, 0.4]];
            a.apply(&mut f, k * 33_000_000, false);
            assert!(f.eye_open[0] > 0.6 && f.eye_open[1] > 0.6, "{k}: {:?}", f.eye_open);
        }
    }

    #[test]
    fn openness_has_one_slope_from_fist_to_open() {
        let at = |m: f32| HandFeatures { finger: [m; 5], ..Default::default() }.openness();
        assert_eq!(at(OPEN_LO), 0.0);
        assert_eq!(at(0.0), 0.0);
        assert!((at(OPEN_HI) - 1.0).abs() < 1e-6);
        assert!((at((OPEN_LO + OPEN_HI) / 2.0) - 0.5).abs() < 1e-5);
        // equal finger travel, equal change, anywhere in the range
        let steps: Vec<f32> = (0..8).map(|i| at(0.1 + 0.1 * (i + 1) as f32) - at(0.1 + 0.1 * i as f32)).collect();
        assert!(steps.iter().all(|d| (d - steps[0]).abs() < 1e-5), "{steps:?}");
    }

    #[test]
    fn hand_blobs_drop_a_speck_beside_a_hand() {
        let blob = |area| Blob { area, ..Default::default() };
        let areas = |v: Vec<Blob>| v.iter().map(|b| b.area).collect::<Vec<_>>();
        assert_eq!(areas(hand_blobs(vec![blob(5000), blob(90), blob(70)])), vec![5000]);
        assert_eq!(areas(hand_blobs(vec![blob(5000), blob(2000), blob(900)])), vec![5000, 2000]);
        assert_eq!(areas(hand_blobs(vec![blob(400), blob(61)])), vec![400, 61]);
        assert_eq!(areas(hand_blobs(vec![blob(59)])), Vec::<usize>::new());
    }

    #[test]
    fn blob_overlap_of_boxes() {
        let mut mask = vec![0u8; 40 * 20];
        for y in 2..8 {
            for x in 2..8 {
                mask[y * 40 + x] = 1;
            }
            for x in 6..12 {
                mask[(y + 4) * 40 + x] = 2;
            }
            for x in 30..35 {
                mask[y * 40 + x] = 2;
            }
        }
        let mut labels = Vec::new();
        let a = components(&mask, 40, 20, |c| c == 1, &mut labels).remove(0);
        let bs = components(&mask, 40, 20, |c| c == 2, &mut labels);
        assert!(boxes_overlap(&a, &bs[0]) != boxes_overlap(&a, &bs[1]));
    }

    #[test]
    fn resize_and_crop_keep_colour_and_pad_black() {
        let (w, h) = (40, 20);
        let mut img = vec![0u8; w * h * 4];
        for p in img.chunks_mut(4) {
            p.copy_from_slice(&[10, 20, 30, 255]);
        }
        let src = Bgra { data: &img, stride: w * 4, w, h };
        let mut quad = vec![0u8; 8 * 4 * 4];
        resize_quad(&src, &mut quad, 8 * 4, 8, 4);
        assert!(quad.chunks(4).all(|p| p == [10, 20, 30, 255]));
        let mut crop = vec![0u8; 8 * 8 * 4];
        crop_square(&src, -20.0, 0.0, 40.0, &mut crop, 8 * 4, 8);
        assert_eq!(&crop[..3], &[0, 0, 0]);
        assert_eq!(&crop[(2 * 8 + 6) * 4..(2 * 8 + 6) * 4 + 3], &[10, 20, 30]);
    }
}
