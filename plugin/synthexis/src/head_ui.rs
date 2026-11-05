//! The Head section (ui-redesign R5 sections; docs/session-2026-11-04-head.md):
//! a dither head drawn as outlines only, in the hands' dot grid. The mouth,
//! the left and right eye and the head itself are click targets; a click
//! selects one and lights its outline at once. The figure follows the
//! camera: it turns (yaw), nods (pitch) and tilts (roll) with the person,
//! the mouth opens and the eyes close. Mirror view, like the hands: the
//! person's left eye is on the screen's left.
//!
//! What a part drives is said in the chat field; the section shows it as
//! the Hands section shows a hand's routes: the part's contour lit (dashed
//! while a route waits for Keep), a node with the target or a count, and
//! under the figure "Mouth drives:" with a row per route. Over the camera
//! the face gets thin tracking boxes (`crt_boxes`).

use nih_plug_egui::egui::emath::GuiRounding;
use nih_plug_egui::egui::{self, pos2, vec2, Color32, ColorImage, CornerRadius, CursorIcon, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui};

use crate::edges::{EdgeIn, NodeTarget, Routing};
use crate::engine::geom::FaceGeom;
use crate::fingers::{any_pending, param_list, routes_text, DASH, DASH_GAP, OUTLINE_R};
use crate::glass::{self, black, white, Tok};
use crate::i18n::{fill, Text};
use crate::midi::Store;
use crate::settings::FaceOverlay;
use crate::writer::Writer;

/// The head's colour, beside the hands' purple magenta and cyan.
pub const HEAD_COL: Color32 = Color32::from_rgb(0xc3, 0x9b, 0xf2);

/// Dots per canvas side and their size (the hands' grid, fingers.rs).
const CANVAS: usize = 160;
const DOT: f32 = 1.5;
/// The face's centre on the canvas (between the eyes, a little low) and
/// its scale: one face unit in dots (the crown sits at -1.0, the chin at
/// 0.95).
const CENTRE: [f32; 2] = [80.0, 70.0];
const S: f32 = 52.0;
/// A stroke's weight (dots of half width at full coverage) grows this
/// much on the selected part.
const SEL_WEIGHT: f32 = 1.35;
/// Coverage above this is always lit; below it the Bayer matrix decides,
/// so thin and faint strokes break into dither like the hands' shading.
const SOLID: f32 = 0.62;
/// Full turn, nod and tilt on the figure (degrees at value 0 and 1).
const TURN_DEG: f32 = 45.0;
const NOD_DEG: f32 = 30.0;
const TILT_DEG: f32 = 35.0;
/// Of the tilt, the neck takes this share; the shoulders none.
const NECK_TILT: f32 = 0.3;
/// A face older than this counts as gone (ns).
const STALE_NS: u64 = 500_000_000;
/// Canvas, gap, the part chips, gap, the live line.
const VIEW_H: f32 = CANVAS as f32 * DOT + Tok::GAP + Tok::CTRL_H + 4.0 + 18.0;
const NODE_H: f32 = 22.0;
const NODE_OUT: f32 = 16.0;
/// Under this head-frame v (past the chin) the head's contour stops.
const CHIN_V: f32 = 0.97;

const BAYER: [f32; 16] = [0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0];

/// A click target of the head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Head,
    EyeL,
    EyeR,
    Mouth,
}

pub const PARTS: [Part; 4] = [Part::Head, Part::EyeL, Part::EyeR, Part::Mouth];

impl Part {
    pub fn name(self, t: &Text) -> &'static str {
        match self {
            Part::Head => t.h_head,
            Part::EyeL => t.h_eye_l,
            Part::EyeR => t.h_eye_r,
            Part::Mouth => t.h_mouth,
        }
    }

    /// A MIDI bind's signal belongs to this part.
    pub fn owns_signal(self, signal: &str) -> bool {
        let prefixes: &[&str] = match self {
            Part::Head => &["cam0.head."],
            Part::Mouth => &["cam0.mouth.", "cam0.face."],
            Part::EyeL => &["cam0.eye.L."],
            Part::EyeR => &["cam0.eye.R."],
        };
        prefixes.iter().any(|p| signal.starts_with(p))
    }

    fn examples(self, t: &Text) -> &'static str {
        match self {
            Part::Head => t.h_ex_head,
            Part::EyeL => t.h_ex_eye_l,
            Part::EyeR => t.h_ex_eye_r,
            Part::Mouth => t.h_ex_mouth,
        }
    }

    /// The click ellipse of an eye or the mouth in head units: half axes
    /// and its offset down from the part's centre (an eye's takes its
    /// brow); None for the head, whose area is inside the outline.
    fn ellipse(self) -> Option<([f32; 2], f32)> {
        match self {
            Part::Head => None,
            Part::EyeL | Part::EyeR => Some(([0.21, 0.17], -0.07)),
            Part::Mouth => Some(([0.27, 0.12], 0.0)),
        }
    }

    /// Dash-gap pairs around the part's contour.
    fn dashes(self) -> f32 {
        match self {
            Part::Head => 36.0,
            Part::Mouth => 12.0,
            Part::EyeL | Part::EyeR => 10.0,
        }
    }

    /// The region number on the canvas (0 is no line).
    fn region(self) -> u8 {
        match self {
            Part::Head => 1,
            Part::EyeL => 2,
            Part::EyeR => 3,
            Part::Mouth => 4,
        }
    }

    fn of_region(r: u8) -> Option<Part> {
        PARTS.into_iter().find(|p| p.region() == r)
    }
}

/// The face as the figure shows it, in signal values (0..1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub mouth: f32,
    pub smile: f32,
    /// [left, right] eye open.
    pub eye: [f32; 2],
}

impl Default for Pose {
    fn default() -> Self {
        Pose { yaw: 0.5, pitch: 0.5, roll: 0.5, mouth: 0.0, smile: 0.0, eye: [1.0; 2] }
    }
}

impl Pose {
    /// From the values in `signal::FACE_IDS` order.
    pub fn from_values(v: &[f32; 9]) -> Pose {
        Pose { mouth: v[0], smile: v[1], eye: [v[2], v[3]], yaw: v[6], pitch: v[7], roll: v[8] }
    }

    /// The live line under the figure: what the face does now, or "Facing
    /// ahead".
    pub fn caption(&self, t: &Text) -> String {
        let mut out: Vec<&str> = Vec::new();
        let off = |v: f32| (v - 0.5).abs() > 0.15;
        if off(self.yaw) {
            out.push(if self.yaw < 0.5 { t.h_turned_left } else { t.h_turned_right });
        }
        if off(self.pitch) {
            out.push(if self.pitch > 0.5 { t.h_looking_up } else { t.h_looking_down });
        }
        if off(self.roll) {
            out.push(if self.roll < 0.5 { t.h_tilted_left } else { t.h_tilted_right });
        }
        if self.mouth > 0.4 {
            out.push(t.h_mouth_open);
        }
        if self.eye[0] < 0.3 {
            out.push(t.h_eye_l_closed);
        }
        if self.eye[1] < 0.3 {
            out.push(t.h_eye_r_closed);
        }
        if out.is_empty() {
            t.h_ahead.to_string()
        } else {
            out.join(" · ")
        }
    }
}

// --- the figure -------------------------------------------------------------
//
// Strokes in face units (S dots; u across, screen left to right; v down; the
// face's centre between the eyes). Light comes from the upper left, so lines
// on the right and low on the face are heavier. Features sit on a sphere and
// turn and nod with the face; the outline, ears, neck and shoulders are the
// silhouette and move as a whole.

fn rotate(p: [f32; 2], a: f32) -> [f32; 2] {
    let (s, c) = a.sin_cos();
    [p[0] * c - p[1] * s, p[0] * s + p[1] * c]
}

fn turn(pose: &Pose) -> f32 {
    ((pose.yaw - 0.5) * 2.0 * TURN_DEG).to_radians()
}

fn nod(pose: &Pose) -> f32 {
    ((pose.pitch - 0.5) * 2.0 * NOD_DEG).to_radians()
}

/// The tilt on screen (radians, y down): the person's left shoulder is the
/// screen's left, so tilting to it turns the figure's top to the left.
fn tilt(pose: &Pose) -> f32 {
    ((pose.roll - 0.5) * 2.0 * TILT_DEG).to_radians()
}

/// The head tilts around the top of the neck, the neck around its base.
const HEAD_PIVOT: [f32; 2] = [0.0, 0.85];
const NECK_PIVOT: [f32; 2] = [0.0, 1.25];

/// A head-frame point (face units) in screen dots from the canvas centre.
fn head_to_screen(q: [f32; 2], pose: &Pose) -> [f32; 2] {
    let r = rotate([q[0] - HEAD_PIVOT[0], q[1] - HEAD_PIVOT[1]], tilt(pose));
    [(r[0] + HEAD_PIVOT[0]) * S, (r[1] + HEAD_PIVOT[1]) * S]
}

fn neck_to_screen(q: [f32; 2], pose: &Pose) -> [f32; 2] {
    let r = rotate([q[0] - NECK_PIVOT[0], q[1] - NECK_PIVOT[1]], tilt(pose) * NECK_TILT);
    [(r[0] + NECK_PIVOT[0]) * S, (r[1] + NECK_PIVOT[1]) * S]
}

/// Screen dots back to the head frame (face units).
fn screen_to_head(p: [f32; 2], pose: &Pose) -> [f32; 2] {
    let r = rotate([p[0] / S - HEAD_PIVOT[0], p[1] / S - HEAD_PIVOT[1]], -tilt(pose));
    [r[0] + HEAD_PIVOT[0], r[1] + HEAD_PIVOT[1]]
}

/// A point of the face's front at (u, v) after the turn and nod, in head
/// units, and how much it faces the camera (the cosine of its longitude).
/// `r` is its distance from the head's vertical axis: the nose stands out
/// further than the eyes, so it swings further.
fn proj(pose: &Pose, u: f32, v: f32, r: f32) -> ([f32; 2], f32) {
    let lon = (u / r).clamp(-1.0, 1.0).asin() + turn(pose);
    let lat = (v / 1.1).clamp(-1.0, 1.0).asin() - nod(pose) * (r / 0.85);
    ([r * lon.sin(), 1.1 * lat.sin()], lon.cos())
}

/// A line of the figure: points in screen dots with their weight (half width in dots
/// at full coverage), its strength (0..1: how dense its dither is) and its
/// part.
struct Line {
    part: Part,
    pts: Vec<([f32; 2], f32)>,
    k: f32,
}

/// The right half of the head's outline, crown to chin (face units).
/// A narrower crown, the temple, the cheekbone at -0.08, the jaw's angle
/// at 0.50 and a soft square chin.
const OUTLINE: [[f32; 2]; 19] = [
    [0.0, -1.0], [0.19, -0.985], [0.37, -0.94], [0.52, -0.85], [0.64, -0.72], [0.72, -0.56], [0.76, -0.40], [0.78, -0.24],
    [0.79, -0.08], [0.77, 0.08], [0.73, 0.24], [0.69, 0.38], [0.64, 0.50], [0.55, 0.62], [0.44, 0.73], [0.31, 0.83],
    [0.17, 0.90], [0.07, 0.935], [0.0, 0.945],
];

/// The whole outline, crown over the left side to the chin and back up the
/// right (head frame, face units): the jaw and chin follow the turn and
/// nod a little.
fn outline(pose: &Pose) -> Vec<[f32; 2]> {
    let (tu, nd) = (turn(pose).sin(), nod(pose).sin());
    let bend = |p: [f32; 2]| {
        let low = ((p[1] - 0.2) / 0.75).clamp(0.0, 1.0);
        [p[0] + 0.14 * tu * low, p[1] * (1.0 - 0.06 * nd * low)]
    };
    let left = OUTLINE.iter().map(|p| bend([-p[0], p[1]]));
    let right = OUTLINE.iter().rev().skip(1).map(|p| bend(*p));
    left.chain(right).collect()
}

/// The outline's weight at a point: heavier low on the face, on the
/// shadow side and along the jaw, light at the crown.
fn outline_weight(p: [f32; 2]) -> f32 {
    let low = (p[1] + 1.0) / 2.0;
    let jaw = (-((p[1] - 0.55) / 0.2).powi(2)).exp();
    0.45 + 0.35 * low + 0.3 * (p[0] / 0.8).max(0.0) + 0.3 * jaw
}

/// Even-odd point in polygon.
fn inside(p: [f32; 2], poly: &[[f32; 2]]) -> bool {
    let mut odd = false;
    let mut j = poly.len().saturating_sub(1);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1] + 1e-9) + a[0] {
            odd = !odd;
        }
        j = i;
    }
    odd
}

/// Eye geometry for side `s` (-1: the screen's left, the person's left
/// eye): centre, half width.
fn eye_centre(s: f32) -> [f32; 2] {
    [0.33 * s, -0.10]
}
const EYE_HW: f32 = 0.155;

/// The upper and lower lid's v at `t` (-1..1 across the eye) for an eye
/// open `o`: the upper lid sinks onto the lower as the eye closes; the
/// outer corner sits a little higher.
fn lids(s: f32, t: f32, o: f32) -> (f32, f32) {
    let o = o.clamp(0.0, 1.0);
    let t0 = -0.15 * s;
    let f = (1.0 - ((t - t0) / (1.0 + t0.abs())).powi(2)).max(0.0);
    let corner = -0.012 * t * s;
    let v = eye_centre(s)[1] + corner;
    (v - (0.012 + 0.068 * o) * f.powf(0.7), v + (0.008 + 0.03 * o) * (1.0 - t * t))
}

/// Where a brow sits (v of its base): it lifts a little with an open
/// mouth and comes down over an eye that closes.
fn brow_base(pose: &Pose, side: usize) -> f32 {
    -0.30 - 0.035 * pose.mouth.clamp(0.0, 1.0) + 0.03 * (1.0 - pose.eye[side].clamp(0.0, 1.0)) + 0.01 * pose.smile.clamp(0.0, 1.0)
}

/// Samples of `f` over t in `a..b` (n points).
fn curve(n: usize, a: f32, b: f32, mut f: impl FnMut(f32) -> Option<([f32; 2], f32)>) -> Vec<([f32; 2], f32)> {
    (0..n).filter_map(|i| f(a + (b - a) * i as f32 / (n - 1).max(1) as f32)).collect()
}

/// Every stroke of the figure for `pose`, in screen dots.
fn strokes(pose: &Pose) -> Vec<Line> {
    let mut out = Vec::new();
    let head = |q: [f32; 2]| head_to_screen(q, pose);
    // a face point after the turn and nod, None when it faces away
    let face = |u: f32, v: f32, r: f32| {
        let (q, front) = proj(pose, u, v, r);
        (front > 0.12).then(|| head(q))
    };
    // the outline
    let line = outline(pose);
    out.push(Line { part: Part::Head, pts: line.iter().map(|p| (head(*p), outline_weight(*p))).collect(), k: 1.0 });
    // the ears, on the silhouette: the one the face turns away from shows
    for s in [-1.0f32, 1.0] {
        let away = -s * turn(pose).sin();
        if away < -0.35 {
            continue;
        }
        let pull = 0.55 * away.max(0.0);
        let squeeze = (1.0 + away.min(0.0) / 0.35).clamp(0.0, 1.0);
        let ear = |p: [f32; 2]| head([s * (0.775 + (p[0] - 0.775) * squeeze - pull * 0.3), p[1]]);
        let outer = [[0.78, -0.24], [0.84, -0.28], [0.89, -0.23], [0.905, -0.11], [0.88, 0.02], [0.83, 0.10], [0.775, 0.12]];
        let inner = [[0.825, -0.19], [0.86, -0.15], [0.855, -0.05], [0.82, 0.03]];
        let w = if s > 0.0 { 0.7 } else { 0.55 };
        out.push(Line { part: Part::Head, pts: outer.iter().map(|p| (ear(*p), w)).collect(), k: 0.95 });
        out.push(Line { part: Part::Head, pts: inner.iter().map(|p| (ear(*p), 0.45)).collect(), k: 0.55 });
    }
    // the eyes and brows
    for (i, part) in [(0usize, Part::EyeL), (1, Part::EyeR)] {
        let s = if i == 0 { -1.0 } else { 1.0 };
        let o = pose.eye[i].clamp(0.0, 1.0);
        let c = eye_centre(s);
        let u = |t: f32| c[0] + t * EYE_HW;
        out.push(Line { part, pts: curve(15, -1.0, 1.0, |t| face(u(t), lids(s, t, o).0, 0.8).map(|p| (p, 0.5 + 0.55 * (1.0 - t * t)))), k: 1.0 });
        out.push(Line { part, pts: curve(13, -0.95, 0.95, |t| face(u(t), lids(s, t, o).1, 0.8).map(|p| (p, 0.42))), k: 0.6 });
        // the lid's crease, faint
        out.push(Line { part, pts: curve(11, -0.75, 0.9, |t| face(u(t), lids(s, t, 1.0).0 - 0.035 - 0.012 * (1.0 - t * t), 0.8).map(|p| (p, 0.35))), k: 0.45 });
        // the iris between the lids, a ring with a sparse fill
        if o > 0.3 {
            let r = 0.048 * o.min(1.0);
            let (up, lo) = lids(s, 0.0, o);
            let ring = curve(17, 0.0, std::f32::consts::TAU, |a| {
                let (x, y) = (c[0] + r * a.cos(), c[1] + r * a.sin());
                let t = (x - c[0]) / EYE_HW;
                let (up, lo) = lids(s, t, o);
                (y > up + 0.004 && y < lo - 0.002).then(|| face(x, y, 0.8)).flatten().map(|p| (p, 0.4))
            });
            out.push(Line { part, pts: ring, k: 0.8 });
            let mid = (up + lo) / 2.0;
            if let Some(p) = face(c[0], mid.max(c[1] - r * 0.6), 0.8) {
                out.push(Line { part, pts: vec![(p, r * S * 0.7)], k: 0.4 });
            }
        }
        // the brow: thick at the inner end, an arch that peaks towards the outer
        let base = brow_base(pose, i);
        let brow = curve(12, 0.0, 1.0, |f| {
            let x = s * (0.12 + 0.38 * f);
            let y = base + 0.012 * (1.0 - f) - 0.045 * (std::f32::consts::PI * f.powf(0.8)).sin();
            face(x, y, 0.82).map(|p| (p, 1.25 - 0.8 * f))
        });
        out.push(Line { part, pts: brow, k: 0.9 });
    }
    // the nose: the ridge on the shadow side, the tip, nostrils and wings
    let nose = |pts: &[[f32; 2]], r: f32, w: f32| -> Vec<([f32; 2], f32)> { pts.iter().filter_map(|p| face(p[0], p[1], r).map(|q| (q, w))).collect() };
    out.push(Line { part: Part::Head, pts: nose(&[[0.065, -0.20], [0.072, -0.08], [0.08, 0.05], [0.085, 0.12]], 0.86, 0.4), k: 0.55 });
    out.push(Line { part: Part::Head, pts: nose(&[[-0.05, 0.195], [-0.02, 0.222], [0.02, 0.225], [0.055, 0.205]], 0.95, 0.6), k: 0.9 });
    out.push(Line { part: Part::Head, pts: nose(&[[-0.115, 0.165], [-0.105, 0.205], [-0.065, 0.215]], 0.9, 0.5), k: 0.85 });
    out.push(Line { part: Part::Head, pts: nose(&[[0.115, 0.165], [0.105, 0.205], [0.065, 0.215]], 0.9, 0.75), k: 1.0 });
    out.push(Line { part: Part::Head, pts: nose(&[[-0.115, 0.115], [-0.125, 0.16]], 0.9, 0.4), k: 0.5 });
    out.push(Line { part: Part::Head, pts: nose(&[[0.115, 0.115], [0.125, 0.16]], 0.9, 0.5), k: 0.65 });
    // the mouth: the line between the lips (thin at the corners), the
    // lower lip's inner edge when open, the cupid's bow and the shadow
    // under the lower lip, both faint
    let (m, smile) = (pose.mouth.clamp(0.0, 1.0), pose.smile.clamp(0.0, 1.0));
    let (mv, hw, gap) = (0.44, 0.21 + 0.03 * smile, 0.11 * m);
    let mouth = |t: f32, v: f32, w: f32| face(t * hw, v - 0.03 * smile * t * t, 0.84).map(|p| (p, w));
    out.push(Line { part: Part::Mouth, pts: curve(15, -1.0, 1.0, |t| mouth(t, mv - (0.35 * gap + 0.008) * (1.0 - t * t), 0.85 * (1.0 - 0.55 * t * t))), k: 1.0 });
    if gap > 0.012 {
        out.push(Line { part: Part::Mouth, pts: curve(13, -0.92, 0.92, |t| mouth(t, mv + 0.65 * gap * (1.0 - t * t), 0.5)), k: 0.8 });
    }
    out.push(Line {
        part: Part::Mouth,
        pts: curve(13, -0.72, 0.72, |t| mouth(t, mv - 0.35 * gap - 0.008 - 0.034 * (1.0 - (t / 0.72).powi(2)) + 0.011 * (-(t / 0.09).powi(2)).exp(), 0.35)),
        k: 0.5,
    });
    out.push(Line { part: Part::Mouth, pts: curve(9, -0.5, 0.5, |t| mouth(t, mv + 0.65 * gap + 0.06 * (1.0 - (t / 0.5).powi(2)).sqrt(), 0.45)), k: 0.5 });
    // the neck: its sides (heavier on the shadow side), the two muscles
    // from behind the ears to the notch, the notch
    let neck = |pts: &[[f32; 2]], w: f32| -> Vec<([f32; 2], f32)> { pts.iter().map(|p| (neck_to_screen(*p, pose), w)).collect() };
    out.push(Line { part: Part::Head, pts: neck(&[[-0.40, 0.70], [-0.40, 0.86], [-0.41, 1.02], [-0.45, 1.18]], 0.5), k: 0.95 });
    out.push(Line { part: Part::Head, pts: neck(&[[0.40, 0.70], [0.40, 0.86], [0.41, 1.02], [0.45, 1.18]], 0.75), k: 1.0 });
    out.push(Line { part: Part::Head, pts: neck(&[[-0.50, 0.82], [-0.32, 1.0], [-0.11, 1.18]], 0.35), k: 0.35 });
    out.push(Line { part: Part::Head, pts: neck(&[[0.50, 0.82], [0.32, 1.0], [0.11, 1.18]], 0.4), k: 0.5 });
    out.push(Line { part: Part::Head, pts: neck(&[[-0.07, 1.21], [0.0, 1.245], [0.07, 1.21]], 0.4), k: 0.45 });
    // the shoulders and collarbones stay level
    let level = |pts: &[[f32; 2]], w: f32| -> Vec<([f32; 2], f32)> { pts.iter().map(|p| ([p[0] * S, p[1] * S], w)).collect() };
    out.push(Line { part: Part::Head, pts: level(&[[-0.45, 1.18], [-0.66, 1.27], [-0.92, 1.33], [-1.18, 1.37]], 0.5), k: 0.8 });
    out.push(Line { part: Part::Head, pts: level(&[[0.45, 1.18], [0.66, 1.27], [0.92, 1.33], [1.18, 1.37]], 0.6), k: 0.9 });
    out.push(Line { part: Part::Head, pts: level(&[[-0.09, 1.28], [-0.36, 1.31], [-0.62, 1.33]], 0.35), k: 0.35 });
    out.push(Line { part: Part::Head, pts: level(&[[0.09, 1.28], [0.36, 1.31], [0.62, 1.33]], 0.35), k: 0.4 });
    out
}

/// Stamps `s` into the coverage buffer: a soft disc along every segment,
/// keeping per dot the strongest stroke and its part. `sel` strokes are
/// heavier.
fn stamp(s: &Line, wmul: f32, cov: &mut [f32], region: &mut [u8]) {
    let n = CANVAS as i32;
    let mut put = |p: [f32; 2], w: f32| {
        let w = w * wmul;
        let (cx, cy) = (p[0] + CENTRE[0], p[1] + CENTRE[1]);
        let r = (w + 1.0).ceil() as i32;
        for y in (cy as i32 - r)..=(cy as i32 + r) {
            for x in (cx as i32 - r)..=(cx as i32 + r) {
                if x < 0 || y < 0 || x >= n || y >= n {
                    continue;
                }
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                let c = ((w + 0.5 - d).clamp(0.0, 1.0)) * s.k;
                let i = (y * n + x) as usize;
                if c > cov[i] {
                    cov[i] = c;
                    region[i] = s.part.region();
                }
            }
        }
    };
    match s.pts.as_slice() {
        [] => {}
        [(p, w)] => put(*p, *w),
        pts => {
            for seg in pts.windows(2) {
                let ((a, wa), (b, wb)) = (seg[0], seg[1]);
                let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                let steps = (len / 0.4).ceil().max(1.0) as usize;
                for k in 0..=steps {
                    let f = k as f32 / steps as f32;
                    put([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f], wa + (wb - wa) * f);
                }
            }
        }
    }
}

/// The canvas regions and lit dots for `pose`: a dot is lit where the
/// coverage is solid, else where the Bayer matrix lets its coverage
/// through.
fn raster(pose: &Pose, sel: Option<Part>) -> (Vec<u8>, Vec<bool>) {
    let mut cov = vec![0f32; CANVAS * CANVAS];
    let mut region = vec![0u8; CANVAS * CANVAS];
    for s in strokes(pose) {
        stamp(&s, if sel == Some(s.part) { SEL_WEIGHT } else { 1.0 }, &mut cov, &mut region);
    }
    let lit = cov
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (x, y) = (i % CANVAS, i / CANVAS);
            *c > SOLID || *c > (BAYER[(y % 4) * 4 + x % 4] + 0.5) / 16.0
        })
        .collect();
    (region, lit)
}

/// The centre of an eye or the mouth on screen (dots from the canvas
/// centre), None when it faces away; the head's is its middle.
fn centre_of(part: Part, pose: &Pose) -> Option<[f32; 2]> {
    let at = |u: f32, v: f32| {
        let (q, front) = proj(pose, u, v, 0.8);
        (front > 0.12).then(|| head_to_screen(q, pose))
    };
    match part {
        Part::Head => Some(head_to_screen([0.0, -0.55], pose)),
        Part::EyeL => at(eye_centre(-1.0)[0], eye_centre(-1.0)[1]),
        Part::EyeR => at(eye_centre(1.0)[0], eye_centre(1.0)[1]),
        Part::Mouth => at(0.0, 0.44),
    }
}

/// Where a part's node line starts: an eye's outer corner, the mouth's
/// right corner, the head's left temple (screen dots).
fn anchor_of(part: Part, pose: &Pose) -> [f32; 2] {
    let at = |u: f32, v: f32| head_to_screen(proj(pose, u, v, 0.8).0, pose);
    match part {
        Part::Head => head_to_screen([-0.78, -0.45], pose),
        Part::EyeL => at(-0.33 - EYE_HW, -0.10),
        Part::EyeR => at(0.33 + EYE_HW, -0.10),
        Part::Mouth => at(0.20, 0.44),
    }
}

/// The part under the canvas point `p` (dots from the canvas centre): an
/// eye with its brow, the mouth, else the head or neck.
fn part_at(p: [f32; 2], pose: &Pose) -> Option<Part> {
    let q = screen_to_head(p, pose);
    let near = |part: Part, half: [f32; 2], dv: f32| {
        centre_of(part, pose).is_some_and(|c| {
            let c = screen_to_head(c, pose);
            ((q[0] - c[0]) / half[0]).powi(2) + ((q[1] - c[1] - dv) / half[1]).powi(2) < 1.0
        })
    };
    for part in [Part::EyeL, Part::EyeR, Part::Mouth] {
        if part.ellipse().is_some_and(|(half, dv)| near(part, half, dv)) {
            return Some(part);
        }
    }
    let neck = (q[0].abs() < 0.45 && q[1] > 0.6 && q[1] < 1.2).then_some(Part::Head);
    if inside(q, &outline(pose)) { Some(Part::Head) } else { neck }
}

/// A part's colour: the selected one in the head colour, hovered or
/// bound ones lighter, the rest white; all dim without a face.
fn part_colour(sel: bool, hover: bool, bound: bool, seen: bool) -> Color32 {
    let c = if sel {
        HEAD_COL
    } else if hover {
        Color32::WHITE
    } else if bound {
        HEAD_COL.gamma_multiply(0.85)
    } else {
        white(0.85)
    };
    if seen || sel {
        c
    } else {
        c.gamma_multiply(0.55)
    }
}

/// The contour of `part` for `pose`, as the hands' (fingers.rs `outline`):
/// per dot 1 on a dash, 2 in a gap, 0 none. It runs `OUTLINE_R` dots off
/// the part's strokes and its area (inside the outline for the head, the
/// click ellipse for an eye or the mouth); the head's opens under the
/// chin as the hand's does at the wrist. Empty for a part facing away.
fn contour(part: Part, pose: &Pose, region: &[u8]) -> Vec<u8> {
    let n = CANVAS as i32;
    let mut body: Vec<bool> = region.iter().map(|r| *r == part.region()).collect();
    body.resize(CANVAS * CANVAS, false);
    let centre = match part.ellipse() {
        Some((half, dv)) => {
            let Some(c) = centre_of(part, pose) else { return vec![0; CANVAS * CANVAS] };
            let ch = screen_to_head(c, pose);
            // the click ellipse, tested in the head frame around its centre only
            let r = ((half[0].max(half[1]) + dv.abs()) * S).ceil() as i32 + 2;
            let (cx, cy) = ((c[0] + CENTRE[0]) as i32, (c[1] + CENTRE[1]) as i32);
            for y in (cy - r).max(0)..(cy + r).min(n) {
                for x in (cx - r).max(0)..(cx + r).min(n) {
                    let q = screen_to_head([x as f32 + 0.5 - CENTRE[0], y as f32 + 0.5 - CENTRE[1]], pose);
                    if ((q[0] - ch[0]) / half[0]).powi(2) + ((q[1] - ch[1] - dv) / half[1]).powi(2) < 1.0 {
                        if let Some(b) = body.get_mut((y * n + x) as usize) {
                            *b = true;
                        }
                    }
                }
            }
            c
        }
        None => {
            // inside the outline: what a fill from the border over empty dots misses
            let outside = reach_from_border(region);
            for (i, b) in body.iter_mut().enumerate() {
                *b |= region.get(i).copied() == Some(0) && !outside.get(i).copied().unwrap_or(true);
            }
            head_to_screen([0.0, -0.05], pose)
        }
    };
    let is_body = |x: i32, y: i32| x >= 0 && y >= 0 && x < n && y < n && body.get((y * n + x) as usize).copied().unwrap_or(false);
    // the least squared distance to the body, stamped from its edge dots
    let mut near = vec![i32::MAX; CANVAS * CANVAS];
    for y in 0..n {
        for x in 0..n {
            if !is_body(x, y) || (is_body(x - 1, y) && is_body(x + 1, y) && is_body(x, y - 1) && is_body(x, y + 1)) {
                continue;
            }
            for dy in -OUTLINE_R..=OUTLINE_R {
                for dx in -OUTLINE_R..=OUTLINE_R {
                    let (tx, ty) = (x + dx, y + dy);
                    if tx >= 0 && ty >= 0 && tx < n && ty < n && !is_body(tx, ty) {
                        if let Some(d) = near.get_mut((ty * n + tx) as usize) {
                            *d = (*d).min(dx * dx + dy * dy);
                        }
                    }
                }
            }
        }
    }
    let (r2, inner2) = (OUTLINE_R * OUTLINE_R, (OUTLINE_R - 1) * (OUTLINE_R - 1));
    near.iter()
        .enumerate()
        .map(|(i, d2)| {
            let p = [(i % CANVAS) as f32 + 0.5 - CENTRE[0], (i / CANVAS) as f32 + 0.5 - CENTRE[1]];
            if *d2 <= inner2 || *d2 > r2 || (part == Part::Head && screen_to_head(p, pose)[1] > CHIN_V) {
                return 0;
            }
            let a = (p[1] - centre[1]).atan2(p[0] - centre[0]) / std::f32::consts::TAU + 0.5;
            1 + ((a * part.dashes() * 2.0) as u8 % 2)
        })
        .collect()
}

/// The dots a 4-connected fill from the canvas border reaches over dots
/// no stroke covers: the outline is closed, so the inside stays out.
fn reach_from_border(region: &[u8]) -> Vec<bool> {
    let n = CANVAS;
    let empty = |i: usize| region.get(i).copied().unwrap_or(1) == 0;
    let mut seen = vec![false; n * n];
    let mut stack: Vec<usize> = (0..n).flat_map(|k| [k, (n - 1) * n + k, k * n, k * n + n - 1]).filter(|i| empty(*i)).collect();
    while let Some(i) = stack.pop() {
        if seen.get(i).copied().unwrap_or(true) {
            continue;
        }
        if let Some(s) = seen.get_mut(i) {
            *s = true;
        }
        let (x, y) = (i % n, i / n);
        let nb = [(x > 0).then(|| i - 1), (x + 1 < n).then(|| i + 1), (y > 0).then(|| i - n), (y + 1 < n).then(|| i + n)];
        stack.extend(nb.into_iter().flatten().filter(|j| empty(*j) && !seen.get(*j).copied().unwrap_or(true)));
    }
    seen
}

/// The Head section's edges: the face and head routes of the chat field
/// (`Sentence::face_routing`) and the MIDI binds on a face or head
/// signal that no line shows yet (a part the mouth plays).
fn head_edges(routing: &Routing, store: &Store) -> Vec<EdgeIn> {
    let mut edges: Vec<EdgeIn> = routing.edges.iter().filter(|e| crate::preset::head_signal(&e.signal)).cloned().collect();
    for b in store.binds.iter().filter(|b| crate::preset::head_signal(&b.signal)) {
        if let Some(p) = store.get(b.part) {
            let target = NodeTarget::Part { id: p.id, name: p.name.clone() };
            if !edges.iter().any(|e| e.signal == b.signal && e.target == target) {
                edges.push(EdgeIn { signal: b.signal.clone(), target, pending: false });
            }
        }
    }
    edges
}

/// The edges `part` drives.
/// A route from `signal` shows in a part's drives list ("Mouth drives:"),
/// where its `x` unmaps it.
pub(crate) fn in_drives_list(signal: &str) -> bool {
    PARTS.iter().any(|p| p.owns_signal(signal))
}

fn part_edges(edges: &[EdgeIn], part: Part) -> Vec<&EdgeIn> {
    edges.iter().filter(|e| part.owns_signal(&e.signal)).collect()
}

/// A part's node text, as a hand's (fingers.rs `routes_text`): the one
/// target ("Serum 2 · Dist Drive") or how many ("3 parameters"), with
/// true while one waits for Keep; "Empty" (pending false) for the
/// selected part without routes; None for the rest.
fn node_label(edges: &[EdgeIn], part: Part, sel: bool, t: &Text) -> Option<(String, bool)> {
    let mine = part_edges(edges, part);
    match routes_text(&mine, t) {
        Some(text) => Some((text, any_pending(&mine))),
        None => sel.then(|| (t.f_empty.to_string(), false)),
    }
}

/// A label node beside the figure and the line from the part's contour
/// to its near edge.
struct Node {
    part: Part,
    rect: Rect,
    line: Vec<Pos2>,
    text: String,
    pending: bool,
}

#[derive(Default)]
pub struct HeadUi {
    sel: Option<Part>,
    tex: Option<TextureHandle>,
    /// A fixed face instead of the camera's (the window captures).
    fixed: Option<Pose>,
    /// A kept route's `x` in "Mouth drives:" confirmed (take_unmap).
    unmap: Option<(String, NodeTarget)>,
}

impl HeadUi {
    /// The window captures (`SYNTHEXIS_UI_SHOT` g, m, n, o): `part`
    /// selected and the figure held at `pose`.
    pub fn shot(&mut self, part: Part, pose: Pose) {
        self.sel = Some(part);
        self.fixed = Some(pose);
    }

    /// The kept route whose `x` in the drives list confirmed, once: the
    /// window unmaps it (Sentence::unmap_kept).
    pub fn take_unmap(&mut self) -> Option<(String, NodeTarget)> {
        self.unmap.take()
    }

    /// Called when the window opens again: the egui context is new.
    pub fn reset_textures(&mut self) {
        self.tex = None;
    }

    /// The live face, or None when no face came for `STALE_NS`.
    pub fn live() -> Option<Pose> {
        let face = *crate::engine::FACE.lock();
        let now = crate::midi::now_ns();
        face.filter(|(_, at)| now.saturating_sub(*at) < STALE_NS).map(|((v, _), _)| Pose::from_values(&v))
    }

    /// The figure with a node per routed part (its contour lit, dashed
    /// while a route waits for Keep), the part chips and the live line;
    /// under them what the selected part drives, as a hand's list.
    pub fn section(&mut self, ui: &mut Ui, writer: &Writer, t: &Text, routing: &Routing) {
        let live = self.fixed.or_else(Self::live);
        let pose = live.unwrap_or_default();
        let edges = head_edges(routing, &writer.player.store());
        let bound = |p: Part| edges.iter().any(|e| p.owns_signal(&e.signal));
        let size = CANVAS as f32 * DOT;
        let (view, _) = ui.allocate_exact_size(vec2(ui.available_width(), VIEW_H), Sense::hover());
        let canvas = Rect::from_center_size(pos2(view.center().x, view.top() + size / 2.0), vec2(size, size));
        let to_dots = |p: Pos2| [(p.x - canvas.left()) / DOT - CENTRE[0], (p.y - canvas.top()) / DOT - CENTRE[1]];
        let to_screen = |d: [f32; 2]| pos2(canvas.left() + (d[0] + CENTRE[0]) * DOT, canvas.top() + (d[1] + CENTRE[1]) * DOT);
        // the click first, so the selection shows in the frame of its click
        let resp = ui.interact(canvas, ui.id().with("head-canvas"), Sense::click());
        let hover = resp.hover_pos().and_then(|p| part_at(to_dots(p), &pose));
        if hover.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            if let Some(p) = hover {
                self.sel = Some(p);
            }
        }
        let (region, _) = raster(&pose, self.sel);
        // the contour of each part that drives something
        let contours: Vec<(Part, Vec<u8>, bool)> = PARTS
            .iter()
            .filter_map(|p| {
                let mine = part_edges(&edges, *p);
                (!mine.is_empty()).then(|| (*p, contour(*p, &pose, &region), any_pending(&mine)))
            })
            .collect();
        let mut nodes = self.nodes(ui, canvas, view, &pose, &edges, &contours, t, to_screen);
        let mut clicked = None;
        for n in &nodes {
            let r = ui.interact(n.rect, ui.id().with(("head-node", n.part as u8)), Sense::click());
            if r.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if r.clicked() {
                clicked = Some(n.part);
            }
        }
        // the part chips under the canvas, centred
        let names: Vec<&str> = PARTS.iter().map(|p| p.name(t)).collect();
        let chips_w: f32 = names.iter().map(|n| glass::galley(ui, n, Tok::BODY).size().x + 16.0).sum::<f32>() + Tok::GAP * (PARTS.len() - 1) as f32;
        let row = Rect::from_min_size(pos2(view.center().x - chips_w / 2.0, canvas.bottom() + Tok::GAP), vec2(chips_w + 1.0, Tok::CTRL_H));
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::left_to_right(egui::Align::Center)));
        c.spacing_mut().item_spacing.x = Tok::GAP;
        for p in PARTS {
            if glass::chip(&mut c, self.sel == Some(p), p.name(t), bound(p).then_some(HEAD_COL)).clicked() {
                clicked = Some(p);
            }
        }
        if let Some(p) = clicked.filter(|p| self.sel != Some(*p)) {
            // a part without routes gets its "Empty" node now
            self.sel = Some(p);
            nodes = self.nodes(ui, canvas, view, &pose, &edges, &contours, t, to_screen);
        }
        // the figure, and under it each routed part's contour
        let (region, lit) = raster(&pose, self.sel);
        let none = Color32::TRANSPARENT;
        let mut img = ColorImage::new([CANVAS, CANVAS], none);
        for (i, px) in img.pixels.iter_mut().enumerate() {
            if lit.get(i).copied().unwrap_or(false) {
                if let Some(p) = region.get(i).copied().and_then(Part::of_region) {
                    *px = part_colour(self.sel == Some(p), hover == Some(p), bound(p), live.is_some());
                }
                continue;
            }
            for (part, dots, pending) in &contours {
                let o = dots.get(i).copied().unwrap_or(0);
                if o == 1 || (o == 2 && !pending) {
                    *px = HEAD_COL.gamma_multiply(if self.sel == Some(*part) { 0.9 } else { 0.6 });
                }
            }
        }
        // the lines under the figure, so its dots cover them; a route that
        // waits for Keep draws dashed
        let p = ui.painter();
        let stroke = Stroke::new(1.0_f32, HEAD_COL.gamma_multiply(0.55));
        let ppp = p.pixels_per_point();
        for n in &nodes {
            let pts: Vec<Pos2> = n.line.iter().map(|q| q.round_to_pixel_center(ppp)).collect();
            if n.pending {
                p.extend(egui::Shape::dashed_line(&pts, stroke, DASH, DASH_GAP));
            } else {
                for w in pts.windows(2) {
                    p.line_segment([w[0], w[1]], stroke);
                }
            }
        }
        let tex = self.tex.get_or_insert_with(|| ui.ctx().load_texture("head", img.clone(), TextureOptions::NEAREST));
        tex.set(img, TextureOptions::NEAREST);
        p.image(tex.id(), canvas, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        for n in &nodes {
            let sel = self.sel == Some(n.part);
            let cr = CornerRadius::same(Tok::R_TOGGLE);
            p.rect_filled(n.rect, cr, if sel { HEAD_COL.gamma_multiply(0.25) } else { black(0.60) });
            p.rect_stroke(n.rect, cr, Stroke::new(1.0_f32, HEAD_COL.gamma_multiply(if n.pending { 0.6 } else { 1.0 })), StrokeKind::Inside);
            let a = if n.text == t.f_empty {
                Tok::HINT
            } else if sel {
                Tok::STRONG
            } else {
                Tok::VALUE
            };
            let g = glass::galley(ui, &glass::fit(ui, &n.text, Tok::BODY, (n.rect.width() - 16.0).max(0.0)), Tok::BODY);
            glass::text_at(p, n.rect.center() - g.size() / 2.0, g, white(a));
        }
        // the live line under the chips
        let caption = live.map_or_else(|| t.h_no_face.to_string(), |pose| pose.caption(t));
        let g = glass::galley(ui, &caption, Tok::BODY);
        glass::text_at(p, pos2(view.center().x - g.size().x / 2.0, row.bottom() + 4.0), g, white(Tok::HINT));
        // the selected part: "Mouth drives:" and a row per route, as a hand's
        ui.add_space(Tok::ROW_GAP);
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 2.0), Sense::hover());
        glass::section_rule(ui.painter(), r.x_range(), r.top());
        ui.add_space(Tok::ROW_GAP);
        let Some(part) = self.sel else {
            ui.label(RichText::new(t.h_pick).size(Tok::BODY).color(white(Tok::HINT)));
            return;
        };
        let (owner, _) = ui.allocate_exact_size(vec2(ui.available_width(), Tok::CTRL_H), Sense::hover());
        let g = glass::galley(ui, &fill(t.f_hand_drives, &[&part.name(t)]), Tok::BODY);
        glass::text_at(ui.painter(), pos2(owner.left(), owner.center().y - g.size().y / 2.0), g, HEAD_COL);
        let mine = part_edges(&edges, part);
        self.unmap = param_list(ui, &mine, t).or(self.unmap.take());
        if mine.is_empty() {
            for ex in part.examples(t).split(" · ") {
                ui.label(RichText::new(format!("\u{201c}{ex}\u{201d}")).size(Tok::BODY).color(white(Tok::HINT)));
            }
        }
    }

    /// The label nodes: one per part that drives something, plus the
    /// selected one ("Empty" without routes). Eyes and the head on their
    /// side of the canvas, the mouth on the right under the right eye,
    /// each at most as wide as the room beside the canvas in `view`. The
    /// line runs from the contour dot nearest the node down or up, then
    /// across to the node's near edge (the hands' hand node).
    #[allow(clippy::too_many_arguments)]
    fn nodes(&self, ui: &Ui, canvas: Rect, view: Rect, pose: &Pose, edges: &[EdgeIn], contours: &[(Part, Vec<u8>, bool)], t: &Text, to_screen: impl Fn([f32; 2]) -> Pos2) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::new();
        let mut right_y: Vec<f32> = Vec::new();
        for part in PARTS {
            let Some((text, pending)) = node_label(edges, part, self.sel == Some(part), t) else { continue };
            let left = matches!(part, Part::Head | Part::EyeL);
            let room = if left { canvas.left() - NODE_OUT - view.left() } else { view.right() - canvas.right() - NODE_OUT };
            let w = (glass::galley(ui, &text, Tok::BODY).size().x + 16.0).min(room.max(NODE_H));
            let mut y = to_screen(anchor_of(part, pose)).y;
            if !left {
                // the right column: the mouth under the right eye's node
                for prev in &right_y {
                    if (y - prev).abs() < NODE_H + 4.0 {
                        y = prev + NODE_H + 4.0;
                    }
                }
                right_y.push(y);
            }
            let x0 = if left { canvas.left() - NODE_OUT - w } else { canvas.right() + NODE_OUT };
            let rect = Rect::from_min_size(pos2(x0, y - NODE_H / 2.0), vec2(w, NODE_H));
            out.push(Node { part, rect, line: Vec::new(), text, pending });
        }
        // the head's node above the left eye's when both show
        if let (Some(h), Some(e)) = (out.iter().position(|n| n.part == Part::Head), out.iter().position(|n| n.part == Part::EyeL)) {
            if (out[h].rect.center().y - out[e].rect.center().y).abs() < NODE_H + 4.0 {
                let up = out[e].rect.top() - 4.0 - NODE_H;
                out[h].rect = Rect::from_min_size(pos2(out[h].rect.left(), up), out[h].rect.size());
            }
        }
        for n in &mut out {
            let left = n.rect.center().x < canvas.center().x;
            let near = pos2(if left { n.rect.right() } else { n.rect.left() }, n.rect.center().y);
            let dots = contours.iter().find(|c| c.0 == n.part).map(|c| c.1.as_slice()).unwrap_or(&[]);
            let anchor = (0..dots.len())
                .filter(|i| dots.get(*i).copied().unwrap_or(0) != 0)
                .map(|i| to_screen([(i % CANVAS) as f32 + 0.5 - CENTRE[0], (i / CANVAS) as f32 + 0.5 - CENTRE[1]]))
                .min_by(|a, b| (*a - near).length_sq().total_cmp(&(*b - near).length_sq()))
                .unwrap_or_else(|| to_screen(anchor_of(n.part, pose)));
            n.line = vec![anchor, pos2(anchor.x, near.y), near];
        }
        out
    }
}

/// The tracking boxes' rects over the camera, in `PARTS` order (head,
/// left eye, right eye, mouth): each preview box of `g` through the crop
/// (`img` shows `uv` of the preview), an eye or the mouth padded to a
/// readable height and 2 pt round, the face 4 % round. None for a part
/// not seen.
pub fn crt_rects(g: &FaceGeom, img: Rect, uv: Rect) -> [Option<Rect>; 4] {
    let map = |b: [f32; 4]| {
        let to = |x: f32, y: f32| img.min + ((pos2(x, y) - uv.min) / uv.size()) * img.size();
        Rect::from_two_pos(to(b[0], b[1]), to(b[2], b[3]))
    };
    let face = g.face.map(|b| {
        let r = map(b);
        r.expand2(r.size() * 0.04)
    });
    let part = |b: Option<[f32; 4]>, tall: f32| {
        b.map(|b| {
            let r = map(b);
            let h = r.height().max(r.width() * tall);
            Rect::from_center_size(r.center(), vec2(r.width(), h)).expand(2.0)
        })
    };
    [face, part(g.eyes[0], 0.6), part(g.eyes[1], 0.6), part(g.mouth, 0.45)]
}

/// The tracking boxes (v2 glass, a CRT tracker's look): over the camera
/// and under the top bar, a 1 px frame at low alpha with brighter corner
/// ticks around the face, each eye and the mouth, faint scanlines inside
/// the face, and an 11 pt label with the part's value: the head's above
/// its box, the eyes' outside the face at their height with a leader,
/// the mouth's under the face. Only while a face is seen. `show` (the
/// Face overlay setting) picks the boxes (head, eyes, mouth) and whether
/// a label carries its value or only the part's name.
pub fn crt_boxes(p: &egui::Painter, img: Rect, uv: Rect, g: &FaceGeom, pose: &Pose, t: &Text, show: FaceOverlay) {
    let rects = crt_rects(g, img, uv);
    // veil: true low alphas (white() lifts a lit camera far more)
    let frame = Stroke::new(1.0_f32, glass::veil(0.28));
    let tick = Stroke::new(1.0_f32, glass::veil(0.85));
    let lead = Stroke::new(1.0_f32, glass::veil(0.4));
    let ppp = p.pixels_per_point();
    let font = glass::font(Tok::SMALL);
    // the label on a small dark plate, so it reads over a lit room too
    let label = |at: Pos2, align: egui::Align2, text: String| {
        let g = p.layout_no_wrap(text, font.clone(), white(Tok::VALUE));
        let r = align.anchor_size(at, g.size());
        p.rect_filled(r.expand2(vec2(3.0, 0.0)), CornerRadius::same(2), black(0.45));
        p.galley(r.min, g, white(Tok::VALUE));
    };
    let face = rects[0];
    let on = [show.head, show.eyes, show.eyes, show.mouth];
    for (i, r) in rects.iter().enumerate() {
        let Some(r) = r.filter(|_| on.get(i).copied().unwrap_or(false)) else { continue };
        let r = Rect::from_min_max(r.min.round_to_pixel_center(ppp), r.max.round_to_pixel_center(ppp));
        p.rect_stroke(r, CornerRadius::ZERO, frame, StrokeKind::Middle);
        let k = (r.width().min(r.height()) / 4.0).clamp(2.0, 8.0);
        for (c, dx, dy) in [(r.left_top(), 1.0, 1.0), (r.right_top(), -1.0, 1.0), (r.left_bottom(), 1.0, -1.0), (r.right_bottom(), -1.0, -1.0)] {
            p.line_segment([c, c + vec2(k * dx, 0.0)], tick);
            p.line_segment([c, c + vec2(0.0, k * dy)], tick);
        }
        if i == 0 {
            // the scanlines, every 3 pt
            let mut y = r.top() + 2.0;
            while y < r.bottom() - 1.0 {
                p.hline(egui::Rangef::new(r.left() + 1.0, r.right() - 1.0), p.round_to_pixel_center(y), Stroke::new(1.0_f32, glass::veil(0.05)));
                y += 3.0;
            }
        }
        let n = |name: &str, v: f32| if show.values { format!("{name} {v:.2}") } else { name.to_string() };
        let (fl, fr, fb) = face.map_or((r.left(), r.right(), r.bottom()), |f| (f.left(), f.right(), f.bottom()));
        match i {
            0 => label(pos2(r.left(), r.top() - 3.0), egui::Align2::LEFT_BOTTOM, n(t.crt_head, pose.yaw)),
            1 => {
                let x = fl.min(r.left()) - 6.0;
                p.line_segment([pos2(x + 3.0, r.center().y.round()), pos2(r.left(), r.center().y.round())], lead);
                label(pos2(x, r.center().y), egui::Align2::RIGHT_CENTER, n(t.crt_eye_l, pose.eye[0]));
            }
            2 => {
                let x = fr.max(r.right()) + 6.0;
                p.line_segment([pos2(r.right(), r.center().y.round()), pos2(x - 3.0, r.center().y.round())], lead);
                label(pos2(x, r.center().y), egui::Align2::LEFT_CENTER, n(t.crt_eye_r, pose.eye[1]));
            }
            _ => label(pos2(fl, fb.max(r.bottom()) + 3.0), egui::Align2::LEFT_TOP, n(t.crt_mouth, pose.mouth)),
        }
    }
}

/// The live face's boxes and values for `crt_boxes`, None when no face
/// came for `STALE_NS`.
pub fn live_boxes() -> Option<(FaceGeom, Pose)> {
    let g = *crate::engine::FACE_GEOM.lock();
    let now = crate::midi::now_ns();
    let g = g.filter(|(_, at)| now.saturating_sub(*at) < STALE_NS)?.0;
    Some((g, HeadUi::live()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{DE, EN};

    fn lit_of(pose: &Pose, part: Part) -> usize {
        let (region, lit) = raster(pose, None);
        region.iter().zip(&lit).filter(|(r, l)| **r == part.region() && **l).count()
    }

    /// The mean x (dots from the centre) of a part's lit dots.
    fn mean_x(pose: &Pose, part: Part) -> f32 {
        let (region, lit) = raster(pose, None);
        let xs: Vec<f32> = (0..CANVAS * CANVAS).filter(|i| region[*i] == part.region() && lit[*i]).map(|i| (i % CANVAS) as f32 - CENTRE[0]).collect();
        xs.iter().sum::<f32>() / xs.len().max(1) as f32
    }

    /// Rows (dots) a part's lit dots span.
    fn span_y(pose: &Pose, part: Part) -> usize {
        let (region, lit) = raster(pose, None);
        let ys: Vec<usize> = (0..CANVAS * CANVAS).filter(|i| region[*i] == part.region() && lit[*i]).map(|i| i / CANVAS).collect();
        ys.iter().max().unwrap_or(&0) - ys.iter().min().unwrap_or(&0)
    }

    #[test]
    fn the_figure_is_outlines_of_four_parts() {
        let pose = Pose::default();
        for p in PARTS {
            assert!(lit_of(&pose, p) > 20, "{p:?} draws");
        }
        // outline only: the cheek and the forehead stay dark
        let (region, lit) = raster(&pose, None);
        let at = |x: f32, y: f32| {
            let i = ((y * S + CENTRE[1]) as usize) * CANVAS + (x * S + CENTRE[0]) as usize;
            region[i] != 0 && lit[i]
        };
        assert!(!at(0.55, 0.15), "the cheek is not filled");
        assert!(!at(0.0, -0.6), "the forehead is not filled");
        let all = lit.iter().filter(|l| **l).count();
        assert!(all < CANVAS * CANVAS / 7, "{all} lit dots: lines, not a fill");
        // the brows and the nose draw: dots above each eye and between them
        let feats = |x0: f32, x1: f32, y0: f32, y1: f32| (0..CANVAS * CANVAS).filter(|i| {
            let (x, y) = (((i % CANVAS) as f32 - CENTRE[0]) / S, ((i / CANVAS) as f32 - CENTRE[1]) / S);
            lit[*i] && x > x0 && x < x1 && y > y0 && y < y1
        }).count();
        assert!(feats(0.15, 0.5, -0.42, -0.24) > 10, "a brow");
        assert!(feats(-0.13, 0.13, 0.1, 0.25) > 10, "a nose");
        assert!(feats(-0.45, 0.45, 1.0, 1.3) > 15, "a neck");
    }

    #[test]
    fn the_figure_follows_the_face_in_mirror_view() {
        let ahead = Pose::default();
        // turned to the person's left (yaw 0): the face moves to the screen's left
        let left = Pose { yaw: 0.0, ..ahead };
        assert!(mean_x(&left, Part::Mouth) < mean_x(&ahead, Part::Mouth) - 10.0);
        assert!(mean_x(&Pose { yaw: 1.0, ..ahead }, Part::Mouth) > mean_x(&ahead, Part::Mouth) + 10.0);
        // tilted to the left shoulder (roll 0): the top goes left, the left eye sits lower
        let tilted = Pose { roll: 0.0, ..ahead };
        let y = |part: Part, p: &Pose| centre_of(part, p).map_or(0.0, |c| c[1]);
        let x = |part: Part, p: &Pose| centre_of(part, p).map_or(0.0, |c| c[0]);
        assert!(y(Part::EyeL, &tilted) > y(Part::EyeR, &tilted) + 5.0);
        assert!(x(Part::Head, &tilted) < x(Part::Head, &ahead) - 5.0, "the head leans over the neck");
        // looking up moves the features up
        assert!(y(Part::Mouth, &Pose { pitch: 1.0, ..ahead }) < y(Part::Mouth, &ahead) - 5.0);
        // the mouth opens, a closed eye loses its iris and opening
        assert!(span_y(&Pose { mouth: 1.0, ..ahead }, Part::Mouth) > span_y(&ahead, Part::Mouth) + 4);
        assert!(lit_of(&Pose { eye: [0.0, 1.0], ..ahead }, Part::EyeL) + 15 < lit_of(&ahead, Part::EyeL));
        // in a full turn to the right the far eye (the screen's right) squeezes
        let turned = Pose { yaw: 1.0, ..ahead };
        let width = |s: f32| {
            let c = eye_centre(s);
            let a = proj(&turned, c[0] - EYE_HW, c[1], 0.8).0[0];
            let b = proj(&turned, c[0] + EYE_HW, c[1], 0.8).0[0];
            (b - a).abs()
        };
        assert!(width(1.0) < 0.6 * width(-1.0), "{} {}", width(1.0), width(-1.0));
    }

    #[test]
    fn the_brows_move_with_the_face() {
        let ahead = Pose::default();
        assert!(brow_base(&Pose { mouth: 1.0, ..ahead }, 0) < brow_base(&ahead, 0) - 0.02, "an open mouth lifts them");
        assert!(brow_base(&Pose { eye: [0.0, 1.0], ..ahead }, 0) > brow_base(&ahead, 0) + 0.02, "a closing eye pulls its brow down");
        assert_eq!(brow_base(&Pose { eye: [0.0, 1.0], ..ahead }, 1), brow_base(&ahead, 1), "the other brow stays");
    }

    #[test]
    fn a_click_finds_the_part_under_it() {
        for pose in [Pose::default(), Pose { yaw: 0.2, roll: 0.3, pitch: 0.6, ..Pose::default() }] {
            for part in [Part::EyeL, Part::EyeR, Part::Mouth] {
                let c = centre_of(part, &pose).unwrap_or([0.0; 2]);
                assert_eq!(part_at(c, &pose), Some(part), "{pose:?}");
            }
            assert_eq!(part_at(head_to_screen([0.0, -0.75], &pose), &pose), Some(Part::Head));
            assert_eq!(part_at(head_to_screen([0.0, -1.3], &pose), &pose), None);
        }
        assert_eq!(part_at([0.0, 1.05 * S], &Pose::default()), Some(Part::Head), "the neck is the head's");
    }

    #[test]
    fn the_selected_part_lights_wider() {
        let pose = Pose::default();
        let (region, lit) = raster(&pose, Some(Part::Mouth));
        let n = region.iter().zip(&lit).filter(|(r, l)| **r == Part::Mouth.region() && **l).count();
        assert!(n > lit_of(&pose, Part::Mouth), "{n}");
        assert_eq!(part_colour(true, false, false, false), HEAD_COL, "the selection never dims");
        assert_ne!(part_colour(false, false, false, false), part_colour(false, false, false, true));
    }

    #[test]
    fn binds_belong_to_their_part() {
        assert!(Part::EyeR.owns_signal("cam0.eye.R.closed") && !Part::EyeR.owns_signal("cam0.eye.L.closed"));
        assert!(Part::Mouth.owns_signal("cam0.mouth.open") && Part::Head.owns_signal("cam0.head.yaw"));
    }

    #[test]
    fn captions_in_both_languages() {
        let p = Pose { yaw: 0.1, roll: 0.9, mouth: 0.8, ..Pose::default() };
        assert_eq!(p.caption(&EN), "Turned left · Tilted right · Mouth open");
        assert_eq!(p.caption(&DE), "Nach links gedreht · Nach rechts geneigt · Mund offen");
        assert_eq!(Pose::default().caption(&EN), EN.h_ahead);
        let v = [0.7, 0.1, 1.0, 0.2, 0.0, 0.8, 0.3, 0.6, 0.4];
        assert_eq!(Pose::from_values(&v), Pose { mouth: 0.7, smile: 0.1, eye: [1.0, 0.2], yaw: 0.3, pitch: 0.6, roll: 0.4 });
    }

    fn param(plugin: &str, name: &str) -> NodeTarget {
        NodeTarget::Param { plugin: plugin.into(), name: name.into(), inst: format!("gen:0:{plugin}") }
    }

    fn edge(signal: &str, target: NodeTarget, pending: bool) -> EdgeIn {
        EdgeIn { signal: signal.into(), target, pending }
    }

    /// A node names its one target or counts them, as a hand's; pending
    /// while one waits for Keep, "Empty" only for the selected part.
    #[test]
    fn a_node_names_one_target_or_counts_them() {
        let drive = edge("cam0.mouth.open", param("Serum 2", "Dist Drive"), false);
        assert_eq!(node_label(std::slice::from_ref(&drive), Part::Mouth, false, &EN), Some(("Serum 2 · Dist Drive".to_string(), false)));
        let three = [
            edge("cam0.head.yaw", param("Serum 2", "Pan"), false),
            edge("cam0.head.yaw", param("Serum 2", "Filter 1 Cutoff"), true),
            edge("cam0.head.roll", NodeTarget::Macro(2), false),
        ];
        assert_eq!(node_label(&three, Part::Head, false, &EN), Some(("3 parameters".to_string(), true)));
        assert_eq!(node_label(&three, Part::Head, true, &DE), Some(("3 Parameter".to_string(), true)));
        let snare = [edge("cam0.mouth.open", NodeTarget::Part { id: 4, name: "Snare".into() }, false), drive.clone()];
        assert_eq!(node_label(&snare, Part::Mouth, false, &EN), Some(("2 routes".to_string(), false)));
        assert_eq!(node_label(&three, Part::Mouth, false, &EN), None, "the mouth drives nothing");
        assert_eq!(node_label(&three, Part::Mouth, true, &EN), Some((EN.f_empty.to_string(), false)));
        // an eye's route with both sides belongs to both eyes
        let wink = [edge("cam0.eye.L.closed", NodeTarget::Macro(4), false), edge("cam0.eye.R.closed", NodeTarget::Macro(4), false)];
        assert_eq!(part_edges(&wink, Part::EyeL).len(), 1);
        assert_eq!(part_edges(&wink, Part::EyeR).len(), 1);
        assert!(part_edges(&wink, Part::Head).is_empty());
        // the list rows name the movement without the part
        assert_eq!(crate::fingers::graph::signal_label("cam0.mouth.open", &EN), "Open");
        assert_eq!(crate::fingers::graph::signal_label("cam0.eye.R.closed", &DE), "Geschlossen");
        assert_eq!(crate::fingers::graph::signal_label("cam0.head.yaw", &EN), "Turn");
    }

    /// The Head section's edges: the face routing's plus the MIDI binds on
    /// a face signal, each once; never a hand's.
    #[test]
    fn head_edges_add_the_face_binds_once() {
        let mut store = Store::default();
        let id = store.add("Snare", crate::midi::Play::Note { pitch: 38, vel: 100 }, None);
        store.binds.push(crate::midi::Bind { signal: "cam0.mouth.open".into(), trigger: crate::route::Trigger::Above, part: id });
        store.binds.push(crate::midi::Bind { signal: "cam0.hand.L.finger.index".into(), trigger: crate::route::Trigger::Above, part: id });
        let routing = Routing { edges: vec![edge("cam0.hand.R.y", NodeTarget::Macro(0), false), edge("cam0.head.roll", NodeTarget::Macro(1), true)], ..Default::default() };
        let edges = head_edges(&routing, &store);
        let sigs: Vec<&str> = edges.iter().map(|e| e.signal.as_str()).collect();
        assert_eq!(sigs, ["cam0.head.roll", "cam0.mouth.open"]);
        let again = Routing { edges: edges.clone(), ..Default::default() };
        assert_eq!(head_edges(&again, &store).len(), 2, "a bind the routing shows comes once");
    }

    /// A routed part's contour rings it OUTLINE_R dots off its strokes,
    /// in dashes and gaps; the head's opens under the chin.
    #[test]
    fn a_routed_part_gets_a_contour_ring() {
        let pose = Pose::default();
        let (region, _) = raster(&pose, None);
        for part in PARTS {
            let c = contour(part, &pose, &region);
            let dash = c.iter().filter(|o| **o == 1).count();
            let gap = c.iter().filter(|o| **o == 2).count();
            assert!(dash > 20 && gap > 20, "{part:?}: {dash} {gap}");
            assert!(c.iter().zip(&region).all(|(o, r)| *o == 0 || *r != part.region()), "{part:?}: never on its own strokes");
        }
        let head = contour(Part::Head, &pose, &region);
        let below = (0..CANVAS * CANVAS).filter(|i| head[*i] != 0 && screen_to_head([(i % CANVAS) as f32 + 0.5 - CENTRE[0], (i / CANVAS) as f32 + 0.5 - CENTRE[1]], &pose)[1] > CHIN_V).count();
        assert_eq!(below, 0, "open under the chin");
        // the mouth's ring sits around the mouth, inside the face
        let mouth = contour(Part::Mouth, &pose, &region);
        let c = centre_of(Part::Mouth, &pose).unwrap_or([0.0; 2]);
        assert!((0..CANVAS * CANVAS).filter(|i| mouth[*i] != 0).all(|i| {
            let (dx, dy) = ((i % CANVAS) as f32 + 0.5 - CENTRE[0] - c[0], (i / CANVAS) as f32 + 0.5 - CENTRE[1] - c[1]);
            dx.abs() < 0.27 * S + 6.0 && dy.abs() < 0.2 * S + 6.0
        }));
    }

    /// The fill from the border stops at the closed outline: the forehead
    /// is inside, the corner outside, for a turned and tilted head too.
    #[test]
    fn the_outline_closes_the_head() {
        for pose in [Pose::default(), Pose { yaw: 0.2, roll: 0.3, pitch: 0.6, ..Pose::default() }] {
            let (region, _) = raster(&pose, None);
            let out = reach_from_border(&region);
            let dot = |p: [f32; 2]| ((p[1] + CENTRE[1]) as usize) * CANVAS + (p[0] + CENTRE[0]) as usize;
            assert!(!out[dot(head_to_screen([0.0, -0.6], &pose))], "{pose:?}");
            assert!(out[0] && out[CANVAS * CANVAS - 1]);
        }
    }

    /// The tracking boxes: through the crop into window points, the eyes
    /// and the mouth padded to a readable height, nothing for a part not
    /// seen.
    #[test]
    fn crt_boxes_map_the_face_parts_through_the_crop() {
        let g = FaceGeom { face: Some([0.4, 0.2, 0.5, 0.45]), eyes: [Some([0.42, 0.29, 0.44, 0.30]), None], mouth: Some([0.43, 0.38, 0.47, 0.39]) };
        let img = Rect::from_min_size(pos2(100.0, 0.0), vec2(400.0, 200.0));
        let uv = Rect::from_min_max(pos2(0.0, 0.1), pos2(1.0, 0.8));
        let [face, eye_l, eye_r, mouth] = crt_rects(&g, img, uv);
        let face = face.unwrap_or(Rect::NOTHING);
        // x 0.4..0.5 of 400 from 100, y (0.2-0.1)/0.7 of 200, 4 % round
        assert!((face.left() - (260.0 - 1.6)).abs() < 0.01 && (face.right() - (300.0 + 1.6)).abs() < 0.01, "{face:?}");
        assert!((face.top() - (200.0 * 0.1 / 0.7 - 0.04 * 200.0 * 0.25 / 0.7)).abs() < 0.01, "{face:?}");
        let eye = eye_l.unwrap_or(Rect::NOTHING);
        assert!((eye.width() - (8.0 + 4.0)).abs() < 0.01 && (eye.height() - (8.0 * 0.6 + 4.0)).abs() < 0.01, "{eye:?}");
        assert!(eye_r.is_none());
        let m = mouth.unwrap_or(Rect::NOTHING);
        assert!((m.width() - (16.0 + 4.0)).abs() < 0.01 && (m.height() - (16.0 * 0.45 + 4.0)).abs() < 0.01, "{m:?}");
        assert!(face.contains_rect(eye) && face.contains_rect(m), "the parts sit in the face");
        assert_eq!(crt_rects(&FaceGeom::default(), img, uv), [None; 4]);
    }

    /// The labels `crt_boxes` paints with `show`.
    fn crt_labels(show: FaceOverlay) -> Vec<String> {
        let ctx = egui::Context::default();
        crate::glass::style(&ctx);
        let g = FaceGeom { face: Some([0.4, 0.2, 0.5, 0.45]), eyes: [Some([0.42, 0.29, 0.44, 0.30]), Some([0.46, 0.29, 0.48, 0.30])], mouth: Some([0.43, 0.38, 0.47, 0.39]) };
        let pose = Pose { yaw: 0.25, mouth: 0.5, eye: [0.75, 1.0], ..Default::default() };
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            let p = ctx.layer_painter(egui::LayerId::background());
            crt_boxes(&p, Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 450.0)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), &g, &pose, &EN, show);
        });
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }

    /// Settings > Face overlay: each box goes with its toggle, and without
    /// values a label is the part's name alone.
    #[test]
    fn the_face_overlay_setting_picks_boxes_and_values() {
        let all = crt_labels(FaceOverlay::default());
        assert_eq!(all, vec!["HEAD 0.25", "EYE L 0.75", "EYE R 1.00", "MOUTH 0.50"]);
        let names = crt_labels(FaceOverlay { values: false, ..FaceOverlay::default() });
        assert_eq!(names, vec!["HEAD", "EYE L", "EYE R", "MOUTH"]);
        let no_eyes = crt_labels(FaceOverlay { eyes: false, ..FaceOverlay::default() });
        assert_eq!(no_eyes, vec!["HEAD 0.25", "MOUTH 0.50"]);
        let mouth = crt_labels(FaceOverlay { head: false, eyes: false, ..FaceOverlay::default() });
        assert_eq!(mouth, vec!["MOUTH 0.50"]);
        assert!(crt_labels(FaceOverlay { head: false, eyes: false, mouth: false, values: true }).is_empty());
    }
}
