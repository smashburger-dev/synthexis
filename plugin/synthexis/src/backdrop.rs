//! Dither background (docs/design/ui-redesign.md section 4).
//!
//! Three soft colour fields drift over a dark base and cycle slowly along a
//! dusk hue path (navy, iris, aubergine, plum, ember) that never touches the
//! two hand colours. A domain-warped noise flow bends their edges; where a
//! field falls off into the base, cells switch between a dim and a lifted
//! level in an 8x8 ordered dither, gentler inside the text area.
//!
//! The maths follows the formulas of the spec, which describe the ideas of
//! Noa's portfolio; no code is copied from `noa-portfolio`.
//!
//! The grid has one cell per `CELL` points over the whole window. Each editor
//! frame renders one of 8 horizontal bands and uploads it with `set_partial`;
//! a size change renders everything. Editor thread only.

use std::ops::Range;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use nih_plug_egui::egui::{self, pos2, vec2, Color32, ColorImage, Rect, TextureHandle, TextureOptions};

/// Cell size in points.
pub const CELL: f32 = 3.0;
/// Bands per refresh cycle; one is rendered per frame.
const BANDS: usize = 8;
/// One flow node every this many cells, read bilinearly.
const FLOW_STEP: usize = 8;
/// Upper bound on the grid (a 6000 x 6000 pt window); beyond it only the
/// base colour is painted, so a bogus window size never allocates gigabytes.
const MAX_CELLS: usize = 1 << 22;
/// Time constant of the eased window height, seconds.
const EASE_S: f32 = 4.0;
/// The dark base, `#0a0a0c`.
const BASE: Color32 = Color32::from_rgb(0x0a, 0x0a, 0x0c);
/// Where the falloff into the base ends; above it the field is smooth.
const EDGE: f32 = 0.42;
/// Anchor chroma of all fields.
const CHROMA: f32 = 0.11;
/// Ramp stops above the base: (h, L, chroma as a share of `CHROMA`).
const STOPS: [(f32, f32, f32); 3] = [(0.35, 0.19, 0.5), (0.70, 0.245, 0.75), (1.00, 0.30, 0.9)];
/// Dither strength (klo, khi) inside the text area and elsewhere.
const GENTLE: (f32, f32) = (0.60, 0.25);
const FULL: (f32, f32) = (0.20, 0.55);

const BAYER8: [u8; 64] = [
    0, 32, 8, 40, 2, 34, 10, 42, //
    48, 16, 56, 24, 50, 18, 58, 26, //
    12, 44, 4, 36, 14, 46, 6, 38, //
    60, 28, 52, 20, 62, 30, 54, 22, //
    3, 35, 11, 43, 1, 33, 9, 41, //
    51, 19, 59, 27, 49, 17, 57, 25, //
    15, 47, 7, 39, 13, 45, 5, 37, //
    63, 31, 55, 23, 61, 29, 53, 21,
];

/// One moving colour field (spec 4.3). Homes are window fractions, amps in
/// S units, periods in seconds.
struct Field {
    home: [f32; 2],
    amp: [f32; 2],
    period: [f64; 2],
    phase: [f64; 2],
    r0: f32,
    pr: f64,
    g0: f32,
    pg: f64,
    ph: f64,
    phh: f64,
}

const FIELDS: [Field; 3] = [
    Field { home: [0.18, 0.30], amp: [0.22, 0.10], period: [97.0, 131.0], phase: [0.0, 1.3], r0: 0.50, pr: 29.0, g0: 0.85, pg: 23.0, ph: 173.0, phh: 0.0 },
    Field { home: [0.82, 0.62], amp: [0.22, 0.10], period: [113.0, 89.0], phase: [2.1, 0.4], r0: 0.50, pr: 37.0, g0: 0.85, pg: 31.0, ph: 229.0, phh: 2.1 },
    Field { home: [0.50, 1.02], amp: [0.30, 0.06], period: [157.0, 211.0], phase: [4.2, 2.9], r0: 0.60, pr: 43.0, g0: 0.70, pg: 41.0, ph: 283.0, phh: 4.2 },
];

/// A field at one instant, in S units.
#[derive(Clone, Copy, Debug)]
struct Live {
    c: [f32; 2],
    /// 1 / r^2.
    inv_r2: f32,
    g: f32,
}

fn tau(t: f64) -> f64 {
    std::f64::consts::TAU * t
}

/// Centre, inverse squared radius and gain of every field at `t` for a window `w`
/// wide with eased height `he`.
fn fields_at(w: f32, he: f32, t: f64) -> [Live; 3] {
    let s = 0.45 * w;
    let big_t = tau(t);
    FIELDS.map(|f| {
        let wob = |i: usize| (big_t / f.period[i] + f.phase[i]).sin() as f32;
        let c = [f.home[0] * w / s + f.amp[0] * wob(0), f.home[1] * he / s + f.amp[1] * wob(1)];
        let r = f.r0 * (1.0 + 0.12 * (big_t / f.pr).sin() as f32);
        Live { c, inv_r2: 1.0 / (r * r).max(1e-6), g: f.g0 * (1.0 + 0.08 * (big_t / f.pg).sin() as f32) }
    })
}

/// Position of field `i` on the hue path at `t`, 0..1.
fn field_u(f: &Field, t: f64) -> f32 {
    (0.5 - 0.5 * (tau(t) / f.ph + f.phh).cos()) as f32
}

/// Hue of field `i` at `t` in degrees, 0..360 (258 navy .. 18 ember).
pub fn field_hue(i: usize, t: f64) -> f32 {
    FIELDS.get(i).map_or(258.0, |f| (258.0 + 120.0 * field_u(f, t)).rem_euclid(360.0))
}

// ---- colour ---------------------------------------------------------------

type Lab = [f32; 3];

/// OKLab to linear sRGB (Ottosson).
fn lab_to_lin([l, a, b]: Lab) -> [f32; 3] {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
        -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    ]
}

/// Linear sRGB to OKLab (Ottosson).
fn lin_to_lab([r, g, b]: [f32; 3]) -> Lab {
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn decode(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn encode(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.003_130_8 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

/// Linear 0..1 to sRGB bytes, 4096 entries.
type Encode = [u8; 4096];

fn srgb_lut() -> &'static Encode {
    static LUT: OnceLock<Encode> = OnceLock::new();
    LUT.get_or_init(|| std::array::from_fn(|i| (encode(i as f32 / 4095.0) * 255.0).round() as u8))
}

fn srgb_u8(lut: &Encode, x: f32) -> u8 {
    let i = (x.clamp(0.0, 1.0) * 4095.0 + 0.5) as usize;
    lut.get(i).copied().unwrap_or(0)
}

/// `e^x` for `x <= 0`, within 2e-4 relative: `2^n` from the exponent bits
/// times a degree-5 series for `2^f`. Branch-free so the row loops
/// vectorise; inputs below -80 (and NaN) give `e^-80`, positive ones 1.
fn exp_neg(x: f32) -> f32 {
    let x = if x.is_nan() { -80.0 } else { x.clamp(-80.0, 0.0) };
    let y = x * std::f32::consts::LOG2_E;
    let n = y.floor();
    let f = y - n;
    let p = 1.0 + f * (std::f32::consts::LN_2 + f * (0.240_226_5 + f * (0.055_504_11 + f * (0.009_618_129 + f * 0.001_333_355))));
    f32::from_bits(((n as i32 + 127) as u32) << 23) * p
}

fn in_gamut(c: [f32; 3]) -> bool {
    c.iter().all(|v| (-1e-6..=1.0 + 1e-6).contains(v))
}

/// The OKLab colour at lightness `l`, hue `deg`, with chroma `c` folded into
/// the sRGB gamut by a 30-step binary search.
fn lch(l: f32, c: f32, deg: f32) -> Lab {
    let (sin, cos) = deg.to_radians().sin_cos();
    let at = |c: f32| [l, c * cos, c * sin];
    let (mut lo, mut hi) = (0.0_f32, c);
    for _ in 0..30 {
        let mid = 0.5 * (lo + hi);
        if in_gamut(lab_to_lin(at(mid))) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at(lo)
}

fn base_lab() -> Lab {
    lin_to_lab([BASE.r(), BASE.g(), BASE.b()].map(decode))
}

/// The ramp of one field at `deg`: the base and the three stops.
fn ramp(deg: f32) -> [(f32, Lab); 4] {
    let s = STOPS.map(|(h, l, k)| (h, lch(l, k * CHROMA, deg)));
    [(0.0, base_lab()), s[0], s[1], s[2]]
}

fn lerp3(a: Lab, b: Lab, f: f32) -> Lab {
    [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f]
}

/// 256-entry OKLab ramps at the fields' hues of one instant; entry `j`
/// holds the colour of all three fields at level `j / 255`.
struct Luts(Box<[[Lab; 3]; 256]>);

fn lut_index(h: f32) -> usize {
    ((h.clamp(0.0, 1.0) * 255.0 + 0.5) as usize).min(255)
}

impl Luts {
    fn at(t: f64) -> Self {
        let ramps = [0, 1, 2].map(|i| ramp(field_hue(i, t)));
        let mut lut = Box::new([[[0.0; 3]; 3]; 256]);
        for (j, entry) in lut.iter_mut().enumerate() {
            let h = j as f32 / 255.0;
            for (stops, out) in ramps.iter().zip(entry.iter_mut()) {
                let mut seg = (stops[2], stops[3]);
                for pair in stops.windows(2) {
                    if let [lo, hi] = pair {
                        if h <= hi.0 {
                            seg = (*lo, *hi);
                            break;
                        }
                    }
                }
                let ((h0, a), (h1, b)) = seg;
                *out = lerp3(a, b, ((h - h0) / (h1 - h0)).clamp(0.0, 1.0));
            }
        }
        Self(lut)
    }

    /// All three fields' colours at level `h`.
    fn row(&self, h: f32) -> [Lab; 3] {
        self.0.get(lut_index(h)).copied().unwrap_or([[0.0; 3]; 3])
    }

    #[cfg(test)]
    fn get(&self, i: usize, h: f32) -> Lab {
        self.row(h).get(i).copied().unwrap_or([0.0; 3])
    }
}

// ---- flow (spec 4.4) ------------------------------------------------------

fn pcg2d(v: [u32; 2]) -> [u32; 2] {
    const M: u32 = 1_664_525;
    let mut v = v.map(|x| x.wrapping_mul(M).wrapping_add(1_013_904_223));
    v[0] = v[0].wrapping_add(v[1].wrapping_mul(M));
    v[1] = v[1].wrapping_add(v[0].wrapping_mul(M));
    v = v.map(|x| x ^ (x >> 16));
    v[0] = v[0].wrapping_add(v[1].wrapping_mul(M));
    v[1] = v[1].wrapping_add(v[0].wrapping_mul(M));
    v.map(|x| x ^ (x >> 16))
}

fn grad(i: i32, j: i32) -> [f32; 2] {
    pcg2d([i as u32, j as u32]).map(|x| (f64::from(x) * (2.0 / 4_294_967_295.0) - 1.0) as f32)
}

fn gnoise([x, y]: [f32; 2]) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    let (i, j) = (fx as i32, fy as i32);
    let (dx, dy) = (x - fx, y - fy);
    let fade = |f: f32| f * f * f * (f * (6.0 * f - 15.0) + 10.0);
    let dot = |g: [f32; 2], a: f32, b: f32| g[0] * a + g[1] * b;
    let va = dot(grad(i, j), dx, dy);
    let vb = dot(grad(i.wrapping_add(1), j), dx - 1.0, dy);
    let vc = dot(grad(i, j.wrapping_add(1)), dx, dy - 1.0);
    let vd = dot(grad(i.wrapping_add(1), j.wrapping_add(1)), dx - 1.0, dy - 1.0);
    let (ux, uy) = (fade(dx), fade(dy));
    let lo = va + (vb - va) * ux;
    let hi = vc + (vd - vc) * ux;
    (lo + (hi - lo) * uy) * 1.414
}

fn fbm(mut p: [f32; 2]) -> f32 {
    let (mut v, mut a) = (0.0, 0.5);
    for _ in 0..4 {
        v += a * gnoise(p);
        p = [(0.8 * p[0] - 0.6 * p[1]) * 2.0 + 17.3, (0.6 * p[0] + 0.8 * p[1]) * 2.0 + 17.3];
        a *= 0.5;
    }
    v / 0.9375
}

/// Flow value (-1..1) and warp vector at `q` (S units).
fn flow(q: [f32; 2], t: f64) -> (f32, [f32; 2]) {
    let p = [q[0] / (1.0 + 7.0 * 0.5) * 1.3, q[1] * 1.3];
    let tf = (t * 0.03) as f32;
    let at = |o: [f32; 2]| fbm([p[0] + o[0], p[1] + o[1]]);
    let a = [at([tf * 0.7, tf * 0.2]), at([5.2 - tf * 0.3, 1.3 - tf * 0.6])];
    let (ax, ay) = (0.6 * a[0], 0.6 * a[1]);
    let b = [at([ax + 1.7 - tf * 0.5, ay + 9.2 + tf * 0.4]), at([ax + 8.3 + tf * 0.2, ay + 2.8 - tf * 0.7])];
    ((at([0.6 * b[0], 0.6 * b[1]]) * 1.8).clamp(-1.0, 1.0), b)
}

/// Flow nodes (flow, warp x, warp y) for the coarse rows `j0..j0 + n`, all
/// coarse columns; node (i, j) sits on cell (8 i, 8 j).
struct FlowRows {
    j0: usize,
    cols: usize,
    nodes: Vec<[f32; 3]>,
}

impl FlowRows {
    fn new(cols: usize, rows: &Range<usize>, s: f32, t: f64) -> Self {
        let ncols = cols / FLOW_STEP + 2;
        let j0 = rows.start / FLOW_STEP;
        let j1 = rows.end.saturating_sub(1) / FLOW_STEP + 1;
        let centre = |n: usize| ((n * FLOW_STEP) as f32 + 0.5) * CELL / s;
        let mut nodes = Vec::with_capacity(ncols * (j1 + 1 - j0));
        for j in j0..=j1 {
            for i in 0..ncols {
                let (f, w) = flow([centre(i), centre(j)], t);
                nodes.push([f, w[0], w[1]]);
            }
        }
        Self { j0, cols: ncols, nodes }
    }

    fn node(&self, i: usize, j: usize) -> [f32; 3] {
        let k = (j.saturating_sub(self.j0)) * self.cols + i.min(self.cols - 1);
        self.nodes.get(k).copied().unwrap_or([0.0; 3])
    }

    /// The nodes of cell row `y`, interpolated vertically, into `out`.
    fn row(&self, y: usize, out: &mut Vec<[f32; 3]>) {
        let j = y / FLOW_STEP;
        let fy = (y % FLOW_STEP) as f32 / FLOW_STEP as f32;
        out.clear();
        out.extend((0..self.cols).map(|i| {
            let (a, c) = (self.node(i, j), self.node(i, j + 1));
            std::array::from_fn(|k| a[k] + (c[k] - a[k]) * fy)
        }));
    }

    /// Bilinear flow at cell `x` from a row made by `row`.
    fn at_row(row: &[[f32; 3]], x: usize) -> [f32; 3] {
        let i = x / FLOW_STEP;
        let fx = (x % FLOW_STEP) as f32 / FLOW_STEP as f32;
        let a = row.get(i).copied().unwrap_or([0.0; 3]);
        let b = row.get(i + 1).copied().unwrap_or(a);
        std::array::from_fn(|k| a[k] + (b[k] - a[k]) * fx)
    }

    #[cfg(test)]
    fn at(&self, x: usize, y: usize) -> [f32; 3] {
        let mut row = Vec::new();
        self.row(y, &mut row);
        Self::at_row(&row, x)
    }
}

// ---- per cell (spec 4.5) --------------------------------------------------

/// Integer hash, copied from fingers.rs (`hash`), 0..1.
fn hash(x: usize, y: usize) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B9) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 13;
    (h & 0xFFFF) as f32 / 65535.0
}

fn bayer(x: usize, y: usize) -> f32 {
    (f32::from(BAYER8[(y & 7) * 8 + (x & 7)]) + 0.5) / 64.0
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Weight of field `f` at the warped cell centre `(sx, sy)`.
fn weight(f: &Live, sx: f32, sy: f32) -> f32 {
    let (dx, dy) = (sx - f.c[0], sy - f.c[1]);
    f.g * exp_neg(-(dx * dx + dy * dy) * f.inv_r2)
}

/// Field weights at a cell centre `q` with warp `wv`.
#[cfg(test)]
fn weights(fields: &[Live; 3], q: [f32; 2], wv: [f32; 2]) -> [f32; 3] {
    fields.map(|f| weight(&f, q[0] + 0.10 * wv[0], q[1] + 0.10 * wv[1]))
}

/// Field level before the falloff dither.
fn level(w: [f32; 3], flow: f32) -> f32 {
    let u = 1.0 - w.iter().map(|w| 1.0 - w).product::<f32>();
    (-0.05 + u + 0.18 * flow * smoothstep(0.03, 0.3, u)).clamp(0.0, 1.0)
}

/// The OKLab colour of cell (x, y) given its weights, its `level` and the
/// dither strength.
fn shade(luts: &Luts, x: usize, y: usize, w: [f32; 3], mut h: f32, (klo, khi): (f32, f32)) -> Lab {
    let e = (h * (1.0 / EDGE)).min(1.0);
    if e < 1.0 {
        // The hash jitter may push the threshold below 0; clamped there so a
        // cell with no field at all stays the base colour.
        let thr = (bayer(x, y) + (hash(x, y) - 0.5) * 0.06).max(0.0);
        let on = ((e - thr) * (1.0 / 0.14)).clamp(0.0, 1.0);
        let lo = klo * h;
        let hi = h + (EDGE - h) * khi;
        h = lo + (hi - lo) * on;
    }
    let sq = w.map(|w| w * w);
    let sum: f32 = sq.iter().sum();
    let p = if sum < 1e-6 {
        [1.0 / 3.0; 3]
    } else {
        let inv = 1.0 / sum;
        sq.map(|s| s * inv)
    };
    let pick = bayer(x + 4, y + 2);
    let mut acc = 0.0;
    let k = p.iter().position(|pi| {
        acc += pi;
        acc > pick
    });
    let cols = luts.row(h);
    let own = cols.get(k.unwrap_or(2)).copied().unwrap_or(cols[2]);
    let mix = cols.iter().zip(p).fold([0.0; 3], |m, (c, pi)| [m[0] + pi * c[0], m[1] + pi * c[1], m[2] + pi * c[2]]);
    lerp3(own, mix, 0.35)
}

fn to_rgb(lut: &Encode, lab: Lab) -> Color32 {
    let [r, g, b] = lab_to_lin(lab).map(|v| srgb_u8(lut, v));
    Color32::from_rgb(r, g, b)
}

fn grid(w: f32, h: f32) -> [usize; 2] {
    let n = |v: f32| if v.is_finite() && v > 0.0 { (v / CELL).ceil() as usize } else { 0 };
    [n(w), n(h)]
}

/// Everything a render needs besides the rows: the LUTs of this refresh
/// cycle, the window size, the eased height, the text area in cells and `t`.
struct Scene<'a> {
    luts: &'a Luts,
    w: f32,
    h: f32,
    he: f32,
    ta: [usize; 4],
    t: f64,
}

fn render_with(sc: &Scene<'_>, rows: Range<usize>, out: &mut [Color32]) {
    let Scene { luts, w, h, he, ta, t } = *sc;
    let [cols, nrows] = grid(w, h);
    let rows = rows.start.min(nrows)..rows.end.min(nrows);
    if cols == 0 || rows.is_empty() {
        return;
    }
    let s = 0.45 * w;
    let fields = fields_at(w, he, t);
    let fl = FlowRows::new(cols, &rows, s, t);
    // A cell at level 0 is the base for every field (all ramps start there).
    let enc = srgb_lut();
    let base = to_rgb(enc, luts.row(0.0)[0]);
    // Per row, in passes over plain arrays so the weight and level passes
    // vectorise; only the dither and the LUT lookups run cell by cell.
    let qx: Vec<f32> = (0..cols).map(|x| (x as f32 + 0.5) * CELL / s).collect();
    let mut frow = Vec::with_capacity(fl.cols);
    let (mut sx, mut sy, mut flow) = (vec![0.0; cols], vec![0.0; cols], vec![0.0; cols]);
    let mut wt = [vec![0.0; cols], vec![0.0; cols], vec![0.0; cols]];
    for (y, line) in rows.zip(out.chunks_mut(cols)) {
        let qy = (y as f32 + 0.5) * CELL / s;
        let text_row = (ta[1]..ta[3]).contains(&y);
        fl.row(y, &mut frow);
        for (x, ((ox, oy), of)) in sx.iter_mut().zip(sy.iter_mut()).zip(flow.iter_mut()).enumerate() {
            let [f, wx, wy] = FlowRows::at_row(&frow, x);
            *ox = qx.get(x).copied().unwrap_or(0.0) + 0.10 * wx;
            *oy = qy + 0.10 * wy;
            *of = f;
        }
        for (field, out) in fields.iter().zip(wt.iter_mut()) {
            for ((o, a), b) in out.iter_mut().zip(&sx).zip(&sy) {
                *o = weight(field, *a, *b);
            }
        }
        let [w0, w1, w2] = &wt;
        let cells = line.iter_mut().zip(w0.iter().zip(w1).zip(w2)).zip(&flow).enumerate();
        for (x, ((px, ((a, b), c)), f)) in cells {
            let w = [*a, *b, *c];
            let h = level(w, *f);
            *px = if h <= 0.0 {
                base
            } else {
                let k = if text_row && (ta[0]..ta[2]).contains(&x) { GENTLE } else { FULL };
                to_rgb(enc, shade(luts, x, y, w, h, k))
            };
        }
    }
}

/// Pure: cells of `rows` at time `t` for a window of `w` x `h` pt (eased height `he`).
///
/// `out` holds `rows.len()` rows of `ceil(w / CELL)` cells; `text_area` is
/// `[x0, y0, x1, y1]` in cells (half-open) and gets the gentle dither.
#[cfg(test)]
pub fn render_rows(w: f32, h: f32, he: f32, rows: Range<usize>, text_area: [usize; 4], t: f64, out: &mut [Color32]) {
    render_with(&Scene { luts: &Luts::at(t), w, h, he, ta: text_area, t }, rows, out);
}

// ---- the texture ----------------------------------------------------------

/// The background of the window. Lives in the editor's UI state, so a
/// reopened window keeps the motion where it was.
#[derive(Default)]
pub struct Backdrop {
    start: Option<Instant>,
    last_t: f64,
    tex: Option<TextureHandle>,
    buf: Vec<Color32>,
    /// Window size in points the texture was rendered for.
    size: [f32; 2],
    luts: Option<Luts>,
    next: usize,
    /// Window height eased toward the real one.
    he: Option<f32>,
}

impl Backdrop {
    /// Drops the texture (it belongs to a closed window's context); the
    /// next `paint` renders everything again.
    pub fn reset_texture(&mut self) {
        self.tex = None;
    }

    /// Eases the field height toward `h` and returns it.
    fn ease(&mut self, h: f32, dt: f64) -> f32 {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let he = match self.he {
            Some(he) if he.is_finite() => he + (h - he) * (1.0 - (-dt / f64::from(EASE_S)).exp()) as f32,
            _ => h,
        };
        self.he = Some(he);
        he
    }

    /// Renders one band and paints the whole texture over `window`; `text_area` gets the gentle dither.
    pub fn paint(&mut self, ctx: &egui::Context, painter: &egui::Painter, window: Rect, text_area: Rect) {
        let painter = painter.with_clip_rect(window);
        let (w, h) = (window.width(), window.height());
        let [cols, rows] = grid(w, h);
        if cols == 0 || rows == 0 || cols.saturating_mul(rows) > MAX_CELLS {
            painter.rect_filled(window, 0.0, BASE);
            return;
        }
        let now = Instant::now();
        let t = now.duration_since(*self.start.get_or_insert(now)).as_secs_f64();
        let he = self.ease(h, t - self.last_t);
        self.last_t = t;
        let ta = cells_of(window, text_area, cols, rows);
        let resized = self.size != [w, h] || self.buf.len() != cols * rows;
        if resized || self.tex.is_none() {
            self.size = [w, h];
            self.buf.resize(cols * rows, BASE);
            let luts = Luts::at(t);
            render_with(&Scene { luts: &luts, w, h, he, ta, t }, 0..rows, &mut self.buf);
            self.luts = Some(luts);
            self.next = 0;
            let img = ColorImage { size: [cols, rows], pixels: self.buf.clone() };
            match &mut self.tex {
                Some(tex) => tex.set(img, TextureOptions::NEAREST),
                None => self.tex = Some(ctx.load_texture("backdrop", img, TextureOptions::NEAREST)),
            }
        } else {
            if self.next == 0 || self.luts.is_none() {
                self.luts = Some(Luts::at(t));
            }
            let per = rows.div_ceil(BANDS);
            let band = (self.next * per).min(rows)..((self.next + 1) * per).min(rows);
            self.next = (self.next + 1) % BANDS;
            if let (Some(luts), Some(tex), false) = (&self.luts, &mut self.tex, band.is_empty()) {
                let slice = self.buf.get_mut(band.start * cols..band.end * cols).unwrap_or_default();
                render_with(&Scene { luts, w, h, he, ta, t }, band.clone(), slice);
                let img = ColorImage { size: [cols, band.len()], pixels: slice.to_vec() };
                tex.set_partial([0, band.start], img, TextureOptions::NEAREST);
            }
        }
        if let Some(tex) = &self.tex {
            let rect = Rect::from_min_size(window.min, vec2(cols as f32 * CELL, rows as f32 * CELL));
            painter.image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

/// `text_area` in cells of the grid over `window`, half-open, clamped.
fn cells_of(window: Rect, text_area: Rect, cols: usize, rows: usize) -> [usize; 4] {
    if !(text_area.is_positive() && text_area.is_finite()) {
        return [0; 4];
    }
    let lo = |v: f32, n: usize| (((v / CELL).floor()).max(0.0) as usize).min(n);
    let hi = |v: f32, n: usize| (((v / CELL).ceil()).max(0.0) as usize).min(n);
    let (a, b) = (text_area.min - window.min, text_area.max - window.min);
    [lo(a.x, cols), lo(a.y, rows), hi(b.x, cols), hi(b.y, rows)]
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 1200.0;
    const H: f32 = 820.0;

    fn render(w: f32, h: f32, ta: [usize; 4], t: f64) -> (usize, Vec<Color32>) {
        let [cols, rows] = grid(w, h);
        let mut out = vec![Color32::TRANSPARENT; cols * rows];
        render_rows(w, h, h, 0..rows, ta, t, &mut out);
        (cols, out)
    }

    fn lab_of(c: Color32) -> Lab {
        lin_to_lab([c.r(), c.g(), c.b()].map(decode))
    }

    fn hex(c: Lab) -> [u8; 3] {
        lab_to_lin(c).map(|v| (encode(v) * 255.0).round() as u8)
    }

    fn parse(s: &str) -> [u8; 3] {
        let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0);
        [(v >> 16) as u8, (v >> 8) as u8, v as u8]
    }

    fn close(a: [u8; 3], b: [u8; 3], tol: i32) -> bool {
        a.iter().zip(b).all(|(x, y)| (i32::from(*x) - i32::from(y)).abs() <= tol)
    }

    fn hue_of(l: Lab) -> f32 {
        l[2].atan2(l[1]).to_degrees().rem_euclid(360.0)
    }

    fn hue_gap(a: f32, b: f32) -> f32 {
        let d = (a - b).rem_euclid(360.0);
        d.min(360.0 - d)
    }

    /// Raw level and weights of a cell, as the renderer sees them.
    fn probe(w: f32, he: f32, t: f64, x: usize, y: usize) -> ([f32; 3], f32) {
        let s = 0.45 * w;
        let [cols, _] = grid(w, he);
        let fl = FlowRows::new(cols, &(y..y + 1), s, t);
        let [f, wx, wy] = fl.at(x, y);
        let q = [(x as f32 + 0.5) * CELL / s, (y as f32 + 0.5) * CELL / s];
        (weights(&fields_at(w, he, t), q, [wx, wy]), f)
    }

    #[test]
    fn palette_matches_the_reference_table() {
        let table = [
            (258.0, ["#04132c", "#031f47", "#072c5e"]),
            (285.0, ["#120f2b", "#1e1845", "#2b235c"]),
            (312.0, ["#1c0b25", "#2d113c", "#3e1b51"]),
            (340.0, ["#23081b", "#380c2c", "#4c143d"]),
            (18.0, ["#28070a", "#3f0911", "#55101a"]),
        ];
        for (deg, row) in table {
            let r = ramp(deg);
            for (k, want) in row.iter().enumerate() {
                let got = r.get(k + 1).map_or([0; 3], |s| hex(s.1));
                assert!(close(got, parse(want), 1), "hue {deg} stop {k}: {got:?} vs {want}");
            }
        }
        // the LUT itself: its ends are the base and the top stop of the current hue
        let luts = Luts::at(0.0);
        for i in 0..3 {
            let top = ramp(field_hue(i, 0.0))[3].1;
            assert!(close(hex(luts.get(i, 1.0)), hex(top), 0));
            assert!(close(hex(luts.get(i, 0.0)), [0x0a, 0x0a, 0x0c], 1));
        }
    }

    #[test]
    fn exp_neg_matches_exp() {
        for k in 0..=20_000 {
            let x = -f64::from(k) * 0.001;
            let (got, want) = (f64::from(exp_neg(x as f32)), x.exp());
            assert!((got - want).abs() <= 2e-4 * want + 1e-9, "x {x}: {got} vs {want}");
        }
        assert!(exp_neg(f32::NAN) < 1e-30);
        assert!(exp_neg(-1e9) < 1e-30);
        assert!((exp_neg(0.5) - 1.0).abs() < 1e-6, "positive input clamps to 0");
    }

    #[test]
    fn render_is_deterministic() {
        let ta = [10, 100, 390, 200];
        assert_eq!(render(W, H, ta, 12.5), render(W, H, ta, 12.5));
        assert_ne!(render(W, H, ta, 12.5).1, render(W, H, ta, 40.0).1);
    }

    #[test]
    fn bands_match_a_full_render() {
        let (ta, t) = ([10, 100, 390, 200], 77.0);
        let (cols, full) = render(W, H, ta, t);
        let rows = full.len() / cols;
        let per = rows.div_ceil(BANDS);
        let mut banded = vec![Color32::TRANSPARENT; full.len()];
        for b in 0..BANDS {
            let r = (b * per).min(rows)..((b + 1) * per).min(rows);
            let (s, e) = (r.start * cols, r.end * cols);
            render_rows(W, H, H, r, ta, t, banded.get_mut(s..e).unwrap_or_default());
        }
        assert!(full == banded, "a band render differs from the full render");
    }

    #[test]
    fn far_from_the_fields_is_the_base_colour() {
        // A very tall window: the fields sit near the top, the far bottom is empty.
        let (w, h) = (1200.0, 6000.0);
        let [cols, rows] = grid(w, h);
        let r = rows - 40..rows;
        let mut out = vec![Color32::TRANSPARENT; cols * r.len()];
        render_rows(w, h, 820.0, r, [0; 4], 0.0, &mut out);
        for c in out {
            assert!(close([c.r(), c.g(), c.b()], [0x0a, 0x0a, 0x0c], 1), "{c:?}");
        }
    }

    #[test]
    fn the_core_is_smooth() {
        let t = 0.0;
        let (cols, img) = render(W, H, [0; 4], t);
        let rows = img.len() / cols;
        let mut seen = 0;
        for y in 1..rows - 1 {
            for x in 1..cols - 1 {
                let core = |x: usize, y: usize| {
                    let (w, f) = probe(W, H, t, x, y);
                    level(w, f) >= EDGE
                };
                if !(core(x, y) && core(x + 1, y) && core(x, y + 1)) {
                    continue;
                }
                seen += 1;
                let l = |x: usize, y: usize| img.get(y * cols + x).map_or(0.0, |c| lab_of(*c)[0]);
                let (here, right, down) = (l(x, y), l(x + 1, y), l(x, y + 1));
                assert!((here - right).abs() < 0.01 && (here - down).abs() < 0.01, "step at {x},{y}");
            }
        }
        assert!(seen > 1000, "only {seen} core cells");
    }

    #[test]
    fn the_seam_dithers_instead_of_greying() {
        let t = 0.0;
        let (ha, hb) = (field_hue(0, t), field_hue(1, t));
        assert!(hue_gap(ha, hb) > 45.0, "fields A and B share a hue at t 0");
        let luts = Luts::at(t);
        let [cols, rows] = grid(W, H);
        // find an 8x8 tile where A and B share the weight
        let tile = (0..rows / 8)
            .flat_map(|j| (0..cols / 8).map(move |i| (i * 8, j * 8)))
            .find(|&(x, y)| {
                let (w, f) = probe(W, H, t, x + 4, y + 4);
                let (a, b) = (w[0] * w[0], w[1] * w[1]);
                level(w, f) > 0.15 && a > 1e-6 && (0.3..0.7).contains(&(a / (a + b + w[2] * w[2])))
            });
        assert!(tile.is_some(), "no seam tile between fields A and B");
        let (x0, y0) = tile.unwrap_or_default();
        let (mut near_a, mut near_b) = (0, 0);
        for y in y0..y0 + 8 {
            for x in x0..x0 + 8 {
                let (w, f) = probe(W, H, t, x, y);
                // the hue of the field's share, without the base's own tint
                let (c, base) = (shade(&luts, x, y, w, level(w, f), FULL), base_lab());
                let hue = hue_of([c[0], c[1] - base[1], c[2] - base[2]]);
                if hue_gap(hue, ha) < hue_gap(hue, hb) {
                    near_a += 1;
                } else {
                    near_b += 1;
                }
            }
        }
        assert!(near_a >= 8 && near_b >= 8, "tile {x0},{y0}: {near_a} cells near A, {near_b} near B");
    }

    /// Largest OKLab L step between horizontal or vertical neighbours inside `ta`.
    fn max_step(cols: usize, img: &[Color32], ta: [usize; 4]) -> f32 {
        let l = |x: usize, y: usize| img.get(y * cols + x).map_or(0.0, |c| lab_of(*c)[0]);
        let mut m: f32 = 0.0;
        for y in ta[1]..ta[3] - 1 {
            for x in ta[0]..ta[2] - 1 {
                m = m.max((l(x, y) - l(x + 1, y)).abs()).max((l(x, y) - l(x, y + 1)).abs());
            }
        }
        m
    }

    #[test]
    fn the_text_area_dither_is_gentle() {
        let ta = [4, 70, 396, 250];
        let mut gentle_max: f32 = 0.0;
        for t in [0.0, 60.0, 150.0, 400.0] {
            let (cols, img) = render(W, H, ta, t);
            let (_, full) = render(W, H, [0; 4], t);
            let (g, f) = (max_step(cols, &img, ta), max_step(cols, &full, ta));
            assert!(g <= 0.07, "t {t}: text-area step {g}");
            assert!(g < f, "t {t}: text area {g} not gentler than elsewhere {f}");
            gentle_max = gentle_max.max(g);
        }
        assert!(gentle_max > 0.0);
    }

    #[test]
    fn text_contrast_at_the_brightest_cell() {
        let lum = |c: [u8; 3]| {
            let [r, g, b] = c.map(decode);
            0.2126 * r + 0.7152 * g + 0.0722 * b
        };
        let ratio = |a: f32, bg: [u8; 3]| {
            let fg = bg.map(|c| (a * 255.0 + (1.0 - a) * f32::from(c)).round() as u8);
            let (x, y) = (lum(fg), lum(bg));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        };
        for deg in 0..360 {
            let top = hex(ramp(deg as f32)[3].1);
            assert!(ratio(0.52, top) >= 4.5, "hue {deg}: {}", ratio(0.52, top));
            assert!(ratio(0.56, top) >= 5.0, "hue {deg}: {}", ratio(0.56, top));
        }
    }

    /// The fields sweep and stay off the right hand's cyan (and the old
    /// orange). The left hand's purple magenta comes from this dusk family
    /// on purpose (fingers.rs HAND); it stands out by lightness instead,
    /// see `the_left_hand_is_far_lighter_than_its_dusk_hue`.
    #[test]
    fn hues_sweep_and_never_rest_on_a_hand_colour() {
        for (i, f) in FIELDS.iter().enumerate() {
            for start in (0..3000).step_by(10) {
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                for k in 0..=600 {
                    let t = f64::from(start) + f64::from(k) * 0.5;
                    let hue = field_hue(i, t);
                    assert!(!(36.0..=86.0).contains(&hue) && !(197.0..=247.0).contains(&hue), "field {i} at {t}: {hue}");
                    let u = field_u(f, t);
                    lo = lo.min(u);
                    hi = hi.max(u);
                }
                assert!(120.0 * (hi - lo) >= 60.0, "field {i} from {start}: {}", 120.0 * (hi - lo));
            }
        }
    }

    /// The left hand's colour sits in the dither's hue family and still
    /// reads on it: far lighter than the brightest stop at its hue.
    #[test]
    fn the_left_hand_is_far_lighter_than_its_dusk_hue() {
        let hand = lab_of(crate::fingers::HAND[0]);
        let top = ramp(hue_of(hand))[3].1;
        assert!(hand[0] - top[0] >= 0.25, "hand L {} vs dither {}", hand[0], top[0]);
        assert!((258.0..=378.0).contains(&(hue_of(hand) + if hue_of(hand) < 180.0 { 360.0 } else { 0.0 })), "on the dusk path: {}", hue_of(hand));
    }

    #[test]
    fn no_field_moves_faster_than_0_02_s_per_second() {
        let dt = 0.01;
        let mut fastest: f32 = 0.0;
        for k in 0..20_000 {
            let t = f64::from(k) * 0.05;
            let (a, b) = (fields_at(W, H, t), fields_at(W, H, t + dt));
            for (p, q) in a.iter().zip(&b) {
                let v = (q.c[0] - p.c[0]).hypot(q.c[1] - p.c[1]) / dt as f32;
                fastest = fastest.max(v);
            }
        }
        assert!(fastest < 0.02, "{fastest} S/s");
        assert!(fastest > 0.01, "{fastest} S/s: fields barely move");
    }

    #[test]
    fn opening_a_section_does_not_move_the_fields() {
        let mut b = Backdrop::default();
        let (t0, dt) = (30.0, 1.0 / 60.0);
        let he0 = b.ease(720.0, 0.0);
        let before = fields_at(W, he0, t0);
        let he1 = b.ease(900.0, dt);
        let after = fields_at(W, he1, t0);
        assert!(he1 > he0 && he1 < 900.0);
        let s = 0.45 * W;
        for (p, q) in before.iter().zip(&after) {
            let moved = (q.c[0] - p.c[0]).hypot(q.c[1] - p.c[1]) * s;
            assert!(moved < 1.0, "a field moved {moved} pt");
        }
        // and it does get there: after 20 s the height is within 1 %
        let mut he = he1;
        for _ in 0..1200 {
            he = b.ease(900.0, dt);
        }
        assert!((he - 900.0).abs() < 9.0, "{he}");
    }

    #[test]
    fn text_area_maps_to_cells() {
        let win = Rect::from_min_size(pos2(0.0, 0.0), vec2(W, H));
        let [cols, rows] = grid(W, H);
        let ta = Rect::from_min_max(pos2(12.0, 300.0), pos2(1188.0, 700.0));
        assert_eq!(cells_of(win, ta, cols, rows), [4, 100, 396, 234]);
        assert_eq!(cells_of(win, Rect::NOTHING, cols, rows), [0; 4]);
        let wide = Rect::from_min_max(pos2(-50.0, -50.0), pos2(5000.0, 5000.0));
        assert_eq!(cells_of(win, wide, cols, rows), [0, 0, cols, rows]);
    }

    #[test]
    fn odd_sizes_render_without_panicking() {
        for (w, h) in [(1.0, 1.0), (2.9, 700.0), (0.0, 0.0), (f32::NAN, 10.0), (-5.0, 10.0), (1201.5, 3.1)] {
            let [cols, rows] = grid(w, h);
            let mut out = vec![Color32::TRANSPARENT; cols * rows];
            render_rows(w, h, h, 0..rows + 5, [0, 0, 1000, 1000], 5.0, &mut out);
            assert!(out.iter().all(|c| c.a() == 255));
            // a short buffer is filled as far as it goes
            let mut short = vec![Color32::TRANSPARENT; cols / 2];
            render_rows(w, h, h, 0..rows, [0; 4], 5.0, &mut short);
        }
    }

    #[test]
    #[ignore = "release bench: ./guarded-cargo.sh test -p synthexis --lib --release -- --ignored backdrop_bench"]
    fn backdrop_bench_1200x900() {
        let (w, h) = (1200.0, 900.0);
        let [cols, rows] = grid(w, h);
        let mut buf = vec![Color32::TRANSPARENT; cols * rows];
        let ta = [4, 100, 396, 280];
        let per = rows.div_ceil(BANDS);
        // The gate is on the median; the fastest run shows the cost on a
        // quiet machine (agents often build alongside).
        let stats = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            (v.first().copied().unwrap_or(f64::MAX), v.get(v.len() / 2).copied().unwrap_or(f64::MAX))
        };
        let mut band_ms = Vec::new();
        let mut full_ms = Vec::new();
        for k in 0..80 {
            let t = 10.0 + f64::from(k) * 0.016;
            let luts = Luts::at(t);
            let b = k as usize % BANDS;
            let r = (b * per).min(rows)..((b + 1) * per).min(rows);
            let s = Instant::now();
            let sc = Scene { luts: &luts, w, h, he: h, ta, t };
            render_with(&sc, r.clone(), buf.get_mut(r.start * cols..r.end * cols).unwrap_or_default());
            band_ms.push(s.elapsed().as_secs_f64() * 1e3);
            if k % 4 == 0 {
                let s = Instant::now();
                render_rows(w, h, h, 0..rows, ta, t, &mut buf);
                full_ms.push(s.elapsed().as_secs_f64() * 1e3);
            }
        }
        let s = Instant::now();
        let fl = FlowRows::new(cols, &(0..rows), 0.45 * w, 10.0);
        let flow_ms = s.elapsed().as_secs_f64() * 1e3;
        let ((band, band_med), (full, full_med)) = (stats(band_ms), stats(full_ms));
        println!(
            "backdrop 1200x900 ({cols}x{rows} cells): band {band:.3} ms (median {band_med:.3}), full {full:.3} ms (median {full_med:.3}), flow {flow_ms:.3} ms for {} nodes",
            fl.nodes.len()
        );
        assert!(band_med <= 0.8, "band {band_med} ms");
        assert!(full_med <= 6.0, "full {full_med} ms");
    }
}
