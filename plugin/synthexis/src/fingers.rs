//! The Hands section (ui-redesign v2, R13-R20; first drawn as "What
//! fingers play", Noa's pick of 11-04 from
//! docs/design/midi-editor-sketches.html). Two hands in dither art, turned
//! 45 degrees outward, white dots that dissolve at the wrist; a label node
//! beside each bound finger joined by a bent line, Fist and Open hand chips
//! under each hand. A click on a finger, its node or a chip picks what to
//! edit. Single edits that part (pattern, chord or held note, its FL
//! channel, when it starts), All lists every part as one row with a mini
//! clip, Routes draws where the hands go as a node view (fingers/graph.rs).
//! Patterns open as drum lanes, each lane's note picked by dragging or
//! scrolling its name, or as a chromatic piano roll that works like FL's:
//! click adds, drag moves, the right edge resizes, right-click or
//! right-drag deletes, Option/Alt moves off the grid. Chords show their
//! notes in the roll; editing one makes the chord a pattern ("Back to Am"
//! undoes it).
//!
//! After Noa's second FL test (11-04, ~02:30): the chromatic roll is a fixed
//! section of 15 rows that fits a part when it opens and then moves only by
//! the wheel over it (the page stays put) or a double-click on the keys; an
//! edit never moves its rows or its place on the page. It folds away from
//! the part's owner row. A note dragged past the loop end ends there, so
//! a chord's whole-bar notes move in time too (before, they were clamped
//! to step 0 and only the strum field moved them).

pub mod graph;
mod part_lib;

use nih_plug_egui::egui::{
    self, pos2, vec2, Align, Align2, Color32, ColorImage, CornerRadius, CursorIcon, Event, Id, InputState, Layout, Modifiers, MouseWheelUnit,
    Painter, PointerButton, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, UiBuilder,
    Vec2,
};

use nih_plug_egui::egui::emath::GuiRounding;

use crate::edges::{EdgeIn, NodeTarget, Routing};
use crate::glass::{self, black, white, Tok};
use crate::fl;
use crate::i18n::{fill, Text};
use crate::midi::{self, Channel, Key, Mode, Part, Play, Scale, Start, Step, Store};
use crate::param_find;
use crate::pong::arena::Side;
use crate::pong::setup::Action;
use crate::route::{finger_bit, Trigger};
use crate::writer::Writer;

/// One colour per hand, shared by the camera's skeleton, the dither
/// hands, nodes, contours, lines, Pong's hit marks and the routes view
/// (ux-plugin.md): the left hand a dark purple magenta from the dither
/// background's dusk hues (backdrop.rs, OKLCH 0.57 / 0.18 / 328), the right
/// a muted cyan (0.71 / 0.115 / 222).
pub const HAND: [Color32; 2] = [Color32::from_rgb(0xad, 0x47, 0xac), Color32::from_rgb(0x39, 0xb1, 0xd5)];
/// The hands' colours for text on the dark background: the left hand's
/// hue lifted to L 0.74 (5.6:1 on the brightest dither cell, 8:1 on the
/// base), the right hand's as it is (5.5:1 and 7.9:1).
pub const HAND_TEXT: [Color32; 2] = [Color32::from_rgb(0xd4, 0x8f, 0xd1), HAND[1]];

/// Named shapes of tools/mapping/ontology.yaml (mask thumb..pinky) with
/// their English and German names.
pub const SHAPES: [(u8, &str, &str); 12] = [
    (0b00000, "fist", "Faust"),
    (0b11111, "open", "offen"),
    (0b01000, "point", "Zeigen"),
    (0b10000, "thumbs up", "Daumen hoch"),
    (0b01100, "peace", "Peace"),
    (0b01110, "three", "drei"),
    (0b01111, "four", "vier"),
    (0b01001, "rock", "Rock"),
    (0b10001, "shaka", "Shaka"),
    (0b11000, "L", "L"),
    (0b00001, "pinky up", "kleiner Finger"),
    (0b11100, "thumb index middle", "Daumen Zeige Mittel"),
];

/// What a bind listens to: one finger shown (care = its bit) or a whole
/// shape (care = all five) on one hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub side: usize,
    pub mask: u8,
    pub care: u8,
}

impl Slot {
    pub fn finger(side: usize, f: usize) -> Slot {
        let b = finger_bit(f);
        Slot { side, mask: b, care: b }
    }

    pub fn shape(side: usize, mask: u8) -> Slot {
        Slot { side, mask, care: 31 }
    }

    pub fn signal(&self) -> String {
        format!("cam0.hand.{}.shape", ["L", "R"][self.side.min(1)])
    }

    pub fn trigger(&self) -> Trigger {
        Trigger::Shape { mask: self.mask, care: self.care }
    }

    /// The signal the Routes view shows for this bind:
    /// `cam0.hand.R.finger.index` for a finger, `cam0.hand.R.shape.17`
    /// for a shape.
    pub fn edge_signal(&self) -> String {
        let hand = ["L", "R"][self.side.min(1)];
        match self.finger_index() {
            Some(f) => format!("cam0.hand.{hand}.finger.{}", ["thumb", "index", "middle", "ring", "pinky"][f]),
            None => format!("cam0.hand.{hand}.shape.{}", self.mask),
        }
    }

    pub fn finger_index(&self) -> Option<usize> {
        (0..5).find(|f| self.care != 31 && finger_bit(*f) == self.care)
    }

    pub fn from_bind(b: &midi::Bind) -> Option<Slot> {
        let side = match b.signal.as_str() {
            "cam0.hand.L.shape" => 0,
            "cam0.hand.R.shape" => 1,
            _ => return None,
        };
        match b.trigger {
            Trigger::Shape { mask, care } => Some(Slot { side, mask, care }),
            Trigger::Above => None,
        }
    }

    pub fn label(&self, t: &Text, _de: bool) -> String {
        let hand = [t.hand_left, t.hand_right][self.side.min(1)];
        match self.finger_index() {
            Some(f) => format!("{hand} · {}", t.fingers()[f]),
            None => format!("{hand} · {}", lower_first(&mask_name(self.mask, t))),
        }
    }
}

// --- dither hands ---------------------------------------------------------

/// Hand space: the palm and finger SDFs and the wrist dissolve run here,
/// a right hand upright, palm to the viewer.
const HAND_W: usize = 120;
const HAND_H: usize = 104;
/// The turned hand is rasterised into this many dots square.
const CANVAS: usize = 160;
/// Logical points per dither dot.
const DOT: f32 = 1.5;
/// The palm centre on the canvas (a little low: the fingers reach further
/// up than the wrist down) and in hand space.
const CANVAS_C: [f32; 2] = [80.0, 92.0];
const HAND_C: [f32; 2] = [61.0, 62.0];
/// The right hand turns clockwise by this on screen (fingers to the upper
/// right); the left hand is its mirror.
const TURN: f32 = std::f32::consts::FRAC_PI_4;
/// A long finger's joints (fraction of its length) and the unlit seam
/// across the finger there, in dots.
const JOINTS: [f32; 2] = [0.49, 0.78];
const JOINT_SEAM: f32 = 0.8;
/// A finger's radius at the tip, of its radius at the base.
const TAPER: f32 = 0.85;
/// Thumb..pinky: a polyline (base, bend, tip) that gives the finger's
/// direction and curve, its length knuckle to tip and its radius at the
/// base. Index to pinky get three segments at 0.49 / 0.29 / 0.22 of the
/// length, the thumb two.
const FINGERS: [([[f32; 2]; 3], f32, f32); 5] = [
    ([[44.0, 70.0], [30.0, 52.0], [21.0, 39.0]], 30.0, 5.8),
    ([[48.0, 48.0], [45.0, 30.0], [43.0, 15.0]], 34.0, 5.2),
    ([[59.0, 46.0], [59.0, 27.0], [59.0, 9.0]], 38.0, 5.4),
    ([[70.0, 48.0], [72.0, 31.0], [74.0, 16.0]], 35.0, 5.1),
    ([[79.0, 54.0], [83.0, 41.0], [87.0, 29.0]], 27.0, 4.4),
];
/// Region ids in `Art::region`: 0 none, 1 palm, 2..=6 thumb..pinky, 7 trail.
const PALM: u8 = 1;
const TRAIL: u8 = 7;

/// A right hand turned 45 degrees clockwise, palm to the viewer, thumb
/// towards the body (the left hand is its mirror), as canvas dots: which
/// region each dot belongs to and whether it is lit.
struct Art {
    region: Vec<u8>,
    lit: Vec<bool>,
    /// Canvas dots of the fingertips, thumb..pinky.
    tips: [[f32; 2]; 5],
    /// Each finger's axis at its tip on the canvas (unit, outward).
    axis: [[f32; 2]; 5],
    /// The hand's contour (a hand-level route): 0 none, 1 a dot of a
    /// dash, 2 a dot of a gap (a pending route leaves the gaps out).
    outline: Vec<u8>,
}

fn rotate(v: [f32; 2], a: f32) -> [f32; 2] {
    let (s, c) = a.sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1]]
}

/// Canvas dot to hand space: R(-45 deg) (p - (80, 92)) + (61, 62).
fn to_hand(p: [f32; 2]) -> [f32; 2] {
    let q = rotate([p[0] - CANVAS_C[0], p[1] - CANVAS_C[1]], -TURN);
    [q[0] + HAND_C[0], q[1] + HAND_C[1]]
}

/// Hand space to canvas: R(+45 deg) (h - (61, 62)) + (80, 92).
fn to_canvas(h: [f32; 2]) -> [f32; 2] {
    let q = rotate([h[0] - HAND_C[0], h[1] - HAND_C[1]], TURN);
    [q[0] + CANVAS_C[0], q[1] + CANVAS_C[1]]
}

/// The distance from `p` to the polyline `chain` and the chain parameter
/// (0 at the base, 1 at the tip, by length) of the nearest point.
fn chain_dist(p: [f32; 2], chain: &[[f32; 2]]) -> (f32, f32) {
    let total: f32 = chain.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt()).sum::<f32>().max(1e-6);
    let mut best = (f32::MAX, 0.0);
    let mut before = 0.0f32;
    for w in chain.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (ab, ap) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
        let len2 = (ab[0] * ab[0] + ab[1] * ab[1]).max(1e-6);
        let t = ((ap[0] * ab[0] + ap[1] * ab[1]) / len2).clamp(0.0, 1.0);
        let (dx, dy) = (p[0] - a[0] - ab[0] * t, p[1] - a[1] - ab[1] * t);
        let d = (dx * dx + dy * dy).sqrt();
        if d < best.0 {
            best = (d, (before + t * len2.sqrt()) / total);
        }
        before += len2.sqrt();
    }
    best
}

/// Finger `i`'s chain: points at the joint fractions along its polyline,
/// scaled from the base to the finger's length.
fn finger_chain(i: usize) -> Vec<[f32; 2]> {
    let Some(&([p0, p1, p2], len, _)) = FINGERS.get(i) else { return Vec::new() };
    let d = |a: [f32; 2], b: [f32; 2]| ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    let (l1, l2) = (d(p0, p1), d(p1, p2));
    let old = (l1 + l2).max(1e-6);
    let at = |f: f32| {
        let s = f * old;
        let q = if s <= l1 {
            let k = s / l1.max(1e-6);
            [p0[0] + (p1[0] - p0[0]) * k, p0[1] + (p1[1] - p0[1]) * k]
        } else {
            let k = (s - l1) / l2.max(1e-6);
            [p1[0] + (p2[0] - p1[0]) * k, p1[1] + (p2[1] - p1[1]) * k]
        };
        let g = len / old;
        [p0[0] + (q[0] - p0[0]) * g, p0[1] + (q[1] - p0[1]) * g]
    };
    let fr: &[f32] = if i == 0 { &[0.0, 0.55, 1.0] } else { &[0.0, JOINTS[0], JOINTS[1], 1.0] };
    fr.iter().map(|f| at(*f)).collect()
}

fn hash(x: usize, y: usize) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B9) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 13;
    (h & 0xFFFF) as f32 / 65535.0
}

const BAYER: [f32; 16] = [0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0];

/// One canvas dot: the region it falls in (0 for none) and whether it is
/// lit. `p` is the dot's hand-space point, (x, y) its canvas dot for the
/// screen-aligned Bayer and the wrist's hash.
fn art_dot(p: [f32; 2], x: usize, y: usize, chains: &[Vec<[f32; 2]>]) -> (u8, bool) {
    let (pc, ph, pr) = (HAND_C, [18.5f32, 16.0f32], 8.0f32);
    let q = [(p[0] - pc[0]).abs() - ph[0] + pr, (p[1] - pc[1]).abs() - ph[1] + pr];
    let palm = (q[0].max(0.0).powi(2) + q[1].max(0.0).powi(2)).sqrt() + q[0].max(q[1]).min(0.0) - pr;
    // signed distance, region, shade and (for a finger) its joint seam
    let mut best = (palm, PALM, 0.45 + 0.45 * (-palm / 10.0).clamp(0.0, 1.0), false);
    let mut second = f32::MAX;
    for (i, chain) in chains.iter().enumerate() {
        let (axis, t) = chain_dist(p, chain);
        let (len, r0) = (FINGERS[i].1, FINGERS[i].2);
        let r = r0 * (1.0 - (1.0 - TAPER) * t);
        let d = axis - r;
        let shade = 0.3 + 0.7 * (1.0 - (axis / r).powi(2)).max(0.0);
        let joint = i > 0 && JOINTS.iter().any(|j| ((t - j) * len).abs() < JOINT_SEAM / 2.0);
        if d < best.0 {
            second = best.0;
            best = (d, i as u8 + 2, shade, joint);
        } else {
            second = second.min(d);
        }
    }
    if best.0 < 0.3 {
        let edge = best.0 > -0.9;
        // a thin unlit seam where a finger meets the palm or a neighbour,
        // and across a long finger at its two joints
        let seam = (second - best.0 < 0.7 || best.3) && !edge;
        let shade = best.2 - 0.12 * (p[0] - 40.0) / 60.0;
        return (best.1, edge || (!seam && shade > (BAYER[(y % 4) * 4 + x % 4] + 0.5) / 16.0));
    }
    // the wrist dissolves: dots thin out and drift away below the palm
    let fy = (p[1] - 74.0) / (HAND_H as f32 - 74.0);
    if fy > 0.0 {
        let spread = 15.0 + 10.0 * fy;
        let dx = (p[0] - pc[0]).abs();
        if dx < spread {
            let density = (1.0 - fy).powf(1.6) * (1.0 - (dx / spread).powi(2));
            if hash(x, y) < density * 0.85 {
                return (TRAIL, true);
            }
        }
    }
    (0, false)
}

fn build_art() -> Art {
    let chains: Vec<Vec<[f32; 2]>> = (0..5).map(finger_chain).collect();
    let mut art = Art { region: vec![0; CANVAS * CANVAS], lit: vec![false; CANVAS * CANVAS], tips: [[0.0; 2]; 5], axis: [[0.0; 2]; 5], outline: Vec::new() };
    for (i, chain) in chains.iter().enumerate() {
        let (Some(tip), Some(prev)) = (chain.last().copied(), chain.len().checked_sub(2).and_then(|k| chain.get(k)).copied()) else { continue };
        let (a, b) = (to_canvas(prev), to_canvas(tip));
        let l = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt().max(1e-6);
        art.tips[i] = b;
        art.axis[i] = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
    }
    for y in 0..CANVAS {
        for x in 0..CANVAS {
            let p = to_hand([x as f32 + 0.5, y as f32 + 0.5]);
            if p[0] < 0.0 || p[1] < 0.0 || p[0] >= HAND_W as f32 || p[1] >= HAND_H as f32 {
                continue;
            }
            let (region, lit) = art_dot(p, x, y, &chains);
            let i = y * CANVAS + x;
            art.region[i] = region;
            art.lit[i] = lit;
        }
    }
    art.outline = outline(&art.region);
    art
}

/// The contour's distance from the hand, in dots.
pub(crate) const OUTLINE_R: i32 = 3;
/// Dashes around the palm centre: this many dash-gap pairs.
const OUTLINE_DASHES: f32 = 32.0;
/// Below this hand-space y (the palm's bottom edge) the contour stops:
/// the wrist dissolves, its contour does too.
const OUTLINE_WRIST: f32 = 78.0;

/// The dots `OUTLINE_R` away from palm and fingers (1 on a dash, 2 in a
/// gap), open at the wrist.
fn outline(region: &[u8]) -> Vec<u8> {
    let body = |x: i32, y: i32| x >= 0 && y >= 0 && x < CANVAS as i32 && y < CANVAS as i32 && (PALM..=6).contains(&region.get(y as usize * CANVAS + x as usize).copied().unwrap_or(0));
    let (r2, inner2) = (OUTLINE_R * OUTLINE_R, (OUTLINE_R - 1) * (OUTLINE_R - 1));
    let mut out = vec![0u8; CANVAS * CANVAS];
    for y in 0..CANVAS as i32 {
        for x in 0..CANVAS as i32 {
            if body(x, y) || to_hand([x as f32 + 0.5, y as f32 + 0.5])[1] > OUTLINE_WRIST {
                continue;
            }
            let mut d2 = i32::MAX;
            for dy in -OUTLINE_R..=OUTLINE_R {
                for dx in -OUTLINE_R..=OUTLINE_R {
                    if body(x + dx, y + dy) {
                        d2 = d2.min(dx * dx + dy * dy);
                    }
                }
            }
            if d2 > inner2 && d2 <= r2 {
                let a = (y as f32 + 0.5 - CANVAS_C[1]).atan2(x as f32 + 0.5 - CANVAS_C[0]) / std::f32::consts::TAU + 0.5;
                out[y as usize * CANVAS + x as usize] = 1 + ((a * OUTLINE_DASHES * 2.0) as u8 % 2);
            }
        }
    }
    out
}

/// The art's dot under `p` (points inside `rect`), mirrored for the left
/// hand.
fn dot_at(rect: Rect, p: Pos2, mirror: bool) -> Option<(usize, usize)> {
    let x = ((p.x - rect.left()) / DOT).floor();
    let y = ((p.y - rect.top()) / DOT).floor();
    if x < 0.0 || y < 0.0 || x >= CANVAS as f32 || y >= CANVAS as f32 {
        return None;
    }
    let x = x as usize;
    Some((if mirror { CANVAS - 1 - x } else { x }, y as usize))
}

/// A point on the right hand's canvas (dots) in points on `rect`,
/// mirrored for the left hand.
fn on_canvas(rect: Rect, p: [f32; 2], mirror: bool) -> Pos2 {
    let x = if mirror { CANVAS as f32 - p[0] } else { p[0] };
    pos2(rect.left() + x * DOT, rect.top() + p[1] * DOT)
}

// --- label nodes ------------------------------------------------------------

/// The hands view: the canvas, 8 pt, the name and chips row.
const HANDS_H: f32 = CANVAS as f32 * DOT + 8.0 + Tok::CTRL_H;
/// Canvas centres from the content centre, at most; at least half a canvas.
const HAND_DX: f32 = 150.0;
/// Room an outer node wants beside a canvas (a node of "Hi-hat basic", its
/// distance from the canvas and the inset).
const NODE_ROOM: f32 = 120.0;
const NODE_H: f32 = 22.0;
/// The least distance between two outer nodes' centres.
const NODE_GAP: f32 = 26.0;
/// An outer line's first leg along the finger, and where it turns.
const LEG: f32 = 12.0;
const GUTTER: f32 = 8.0;
/// Outer nodes from the canvas edge; the thumb's from the canvas top.
const NODE_OUT: f32 = 16.0;
const THUMB_TOP: f32 = 8.0;
/// A node shows this many characters of its part, then "…".
const NODE_CHARS: usize = 18;

/// A finger's label node and the line from its tip to it.
#[derive(Clone, Debug, PartialEq)]
struct Label {
    f: usize,
    node: Rect,
    /// The polyline from the tip to the node's near edge.
    line: Vec<Pos2>,
    tip: Pos2,
}

/// Places `ys` (in angle order) at least `gap` apart: a forward pass
/// from `lo` pushes each down under the previous; if the last passes `hi`, all move
/// up by the excess and a backward pass keeps the gap upward; finally
/// nothing stays above `lo`.
fn spread(ys: &mut [f32], gap: f32, lo: f32, hi: f32) {
    if let Some(first) = ys.first_mut() {
        *first = first.max(lo);
    }
    for i in 1..ys.len() {
        ys[i] = ys[i].max(ys[i - 1] + gap);
    }
    if let Some(excess) = ys.last().map(|l| l - hi).filter(|e| *e > 0.0) {
        for y in ys.iter_mut() {
            *y -= excess;
        }
        for i in (0..ys.len().saturating_sub(1)).rev() {
            ys[i] = ys[i].min(ys[i + 1] - gap);
        }
    }
    for y in ys.iter_mut() {
        *y = y.max(lo);
    }
}

/// The label nodes of one hand on `canvas`: `widths[f]` is Some(node
/// width) for each finger that shows one. The thumb's node sits above its
/// tip, its top 8 pt under the canvas top; the other four go to the
/// outer side in the order of their tips' angle around the palm centre,
/// spread 26 apart, joined by a leg along the finger, a run to the gutter
/// and a last segment to the node. `clip` is the content's x range: no
/// node passes it.
#[cfg(test)]
fn label_layout(canvas: Rect, mirror: bool, art: &Art, widths: [Option<f32>; 5], clip: egui::Rangef) -> Vec<Label> {
    label_layout_above(canvas, mirror, art, widths, clip, canvas.bottom())
}

/// `label_layout` with the outer nodes' bottoms at most at `floor` (the
/// hand node's room under them).
fn label_layout_above(canvas: Rect, mirror: bool, art: &Art, widths: [Option<f32>; 5], clip: egui::Rangef, floor: f32) -> Vec<Label> {
    let out = if mirror { -1.0 } else { 1.0 };
    let tip = |f: usize| on_canvas(canvas, art.tips[f], mirror);
    let axis = |f: usize| vec2(art.axis[f][0] * out, art.axis[f][1]);
    let mut labels = Vec::new();
    if let Some(w) = widths[0] {
        let t = tip(0);
        let left = (t.x - w / 2.0).clamp(clip.min, (clip.max - w).max(clip.min));
        let node = Rect::from_min_size(pos2(left, canvas.top() + THUMB_TOP), vec2(w.min(clip.span()), NODE_H));
        labels.push(Label { f: 0, node, line: vec![t, pos2(t.x, node.bottom())], tip: t });
    }
    let c = on_canvas(canvas, CANVAS_C, mirror);
    let mut outer: Vec<(usize, f32, f32)> = (1..5)
        .filter_map(|f| {
            let w = widths[f]?;
            let d = tip(f) - c;
            Some((f, (d.x * out).atan2(-d.y), w))
        })
        .collect();
    outer.sort_by(|a, b| a.1.total_cmp(&b.1));
    let legs: Vec<Pos2> = outer.iter().map(|(f, _, _)| tip(*f) + axis(*f) * LEG).collect();
    let mut ys: Vec<f32> = legs.iter().map(|p| p.y).collect();
    spread(&mut ys, NODE_GAP, canvas.top() + NODE_H / 2.0, floor.min(canvas.bottom()) - NODE_H / 2.0);
    let gutter = if mirror { canvas.left() - GUTTER } else { canvas.right() + GUTTER };
    for (((f, _, w), leg), y) in outer.iter().zip(legs).zip(ys) {
        let node = if mirror {
            let right = canvas.left() - NODE_OUT;
            Rect::from_min_max(pos2((right - w).max(clip.min).min(right), y - NODE_H / 2.0), pos2(right, y + NODE_H / 2.0))
        } else {
            let left = canvas.right() + NODE_OUT;
            Rect::from_min_max(pos2(left, y - NODE_H / 2.0), pos2((left + w).min(clip.max).max(left), y + NODE_H / 2.0))
        };
        let near = if mirror { node.right() } else { node.left() };
        let t = tip(*f);
        labels.push(Label { f: *f, node, line: vec![t, leg, pos2(gutter, leg.y), pos2(near, y)], tip: t });
    }
    labels
}

/// The hand node reaches this far into the canvas from its outer edge;
/// its bottom sits this far above the canvas bottom.
const HAND_IN: f32 = 96.0;
const HAND_LOW: f32 = 4.0;
/// The least gap between the hand node and the finger nodes above it.
const HAND_GAP: f32 = 4.0;
/// A pending route's line: dash and gap.
pub(crate) const DASH: f32 = 4.0;
pub(crate) const DASH_GAP: f32 = 3.0;

/// The node of a hand-level route and the line from the hand's contour
/// to it.
#[derive(Clone, Debug, PartialEq)]
struct HandNode {
    node: Rect,
    line: Vec<Pos2>,
}

impl HandNode {
    /// Where the finger nodes above it must end.
    fn floor(&self) -> f32 {
        self.node.top() - HAND_GAP
    }
}

/// The hand node of `canvas`, `w` wide: in the canvas' empty corner beside
/// the wrist, lower right of the right hand and lower left of the left
/// (mirror), reaching out past the canvas up to `clip`. The line runs from
/// the contour dot nearest the node down, then across to the node's near
/// edge.
fn hand_node(canvas: Rect, mirror: bool, art: &Art, w: f32, clip: egui::Rangef) -> HandNode {
    let bottom = canvas.bottom() - HAND_LOW;
    let node = if mirror {
        let right = canvas.left() + HAND_IN;
        Rect::from_min_max(pos2((right - w).max(clip.min).min(right - NODE_H), bottom - NODE_H), pos2(right, bottom))
    } else {
        let left = canvas.right() - HAND_IN;
        Rect::from_min_max(pos2(left, bottom - NODE_H), pos2((left + w).min(clip.max).max(left + NODE_H), bottom))
    };
    let near = pos2(if mirror { node.right() } else { node.left() }, node.center().y);
    let anchor = (0..CANVAS * CANVAS)
        .filter(|i| art.outline.get(*i).copied().unwrap_or(0) != 0)
        .map(|i| on_canvas(canvas, [(i % CANVAS) as f32 + 0.5, (i / CANVAS) as f32 + 0.5], mirror))
        .min_by(|a, b| (*a - near).length_sq().total_cmp(&(*b - near).length_sq()))
        .unwrap_or(near);
    HandNode { node, line: vec![anchor, pos2(anchor.x, near.y), near] }
}

/// The colours of finger `f`'s dots, (lit, unlit): the calibration's
/// pose first, then a finger playing (white), the selected one (the hand
/// colour, its unlit dots at 0.45, never dimmed by the camera), a bound
/// one (the hand colour) and the rest (white 0.92). `seen`: the camera's
/// read of the finger (shown, folded), None without a camera; bound and
/// plain fingers dim to 0.28 when folded and 0.75 without a camera.
fn finger_colour(side: usize, f: usize, sel: bool, bound: bool, playing: bool, seen: Option<bool>, target: Option<u8>) -> (Color32, Color32) {
    let hand = HAND[side.min(1)];
    let none = Color32::TRANSPARENT;
    if let Some(m) = target {
        return (if m & finger_bit(f) != 0 { hand } else { white(0.2) }, none);
    }
    if playing {
        return (Color32::WHITE, none);
    }
    if sel {
        return (hand, hand.gamma_multiply(0.45));
    }
    let base = if bound { hand } else { white(0.92) };
    let dim = match seen {
        Some(true) => 1.0,
        Some(false) => 0.28,
        None => 0.75,
    };
    (base.gamma_multiply(dim), none)
}

/// "Thumb and pinky", "Index, middle and ring"; "Fist" and "Open hand"
/// for no and all fingers.
fn mask_name(mask: u8, t: &Text) -> String {
    match mask & 31 {
        0 => t.f_fist.to_string(),
        31 => t.f_open.to_string(),
        m => {
            let names: Vec<&str> = (0..5).filter(|f| m & finger_bit(*f) != 0).map(|f| t.fingers()[f]).collect();
            let s = match names.split_last() {
                Some((last, [])) => (*last).to_string(),
                Some((last, rest)) => format!("{} {} {last}", rest.join(", "), t.f_and),
                None => String::new(),
            };
            capital(&s)
        }
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().chain(c).collect(),
        None => String::new(),
    }
}

/// The features of a whole hand: a route from one gets the hand's node.
const HAND_LEVEL: [&str; 9] = ["openness", "fist", "x", "y", "pinch", "spread", "roll", "speed", "depth"];
const FINGER_IDS: [&str; 5] = ["thumb", "index", "middle", "ring", "pinky"];

/// Where on the hands a route's signal comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Src {
    Hand(usize),
    /// (side, finger): `…finger.index` and `…finger.index.tilt`.
    Finger(usize, usize),
}

fn src_of(signal: &str) -> Option<Src> {
    let (side, feat) = graph::split(signal)?;
    if HAND_LEVEL.contains(&feat) {
        return Some(Src::Hand(side));
    }
    let f = feat.strip_prefix("finger.")?;
    let f = f.strip_suffix(".tilt").unwrap_or(f);
    FINGER_IDS.iter().position(|n| *n == f).map(|i| Src::Finger(side, i))
}

/// A route from `signal` shows in a hand's or a finger's drives list,
/// where its `x` unmaps it.
pub(crate) fn in_drives_list(signal: &str) -> bool {
    src_of(signal).is_some()
}

/// The routes `src` drives: all of a hand's; a finger's parameters and
/// Signals (its MIDI part is its bind).
fn edges_of(edges: &[EdgeIn], src: Src) -> Vec<&EdgeIn> {
    edges.iter().filter(|e| src_of(&e.signal) == Some(src) && (matches!(src, Src::Hand(_)) || !matches!(e.target, NodeTarget::Part { .. }))).collect()
}

/// A node's text for `edges`: the one target ("Serum 2 · Cutoff") or how
/// many ("3 parameters", "3 routes" when a MIDI part is among them).
/// None without a route.
pub(crate) fn routes_text(edges: &[&EdgeIn], t: &Text) -> Option<String> {
    let mut targets: Vec<&NodeTarget> = Vec::new();
    for e in edges {
        if !targets.contains(&&e.target) {
            targets.push(&e.target);
        }
    }
    match targets.as_slice() {
        [] => None,
        [one] => Some(graph::target_label(one)),
        many => {
            let parts = many.iter().any(|t| matches!(t, NodeTarget::Part { .. }));
            Some(fill(if parts { t.f_n_routes } else { t.f_n_params }, &[&many.len()]))
        }
    }
}

/// True when one of `edges` still waits for Keep: its contour and line
/// go dashed until Keep or Discard.
pub(crate) fn any_pending(edges: &[&EdgeIn]) -> bool {
    edges.iter().any(|e| e.pending)
}

// --- state ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Single,
    Total,
    Routes,
}

/// When the view goes to Routes by itself and back (R19): once per new
/// pending batch while the chat field has a hand route in focus; back to
/// the view it left when the focus ends, unless the person picked a view
/// by hand in between. Keep leaves it on Routes (Noa, 11-04): the kept
/// routes are what he wants to see next; Discard goes back.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct RouteSwitch {
    /// The batch that switched last.
    fired: u64,
    /// The view the switch left.
    back_to: Option<View>,
    /// A view picked by hand since the switch.
    manual: bool,
    /// The pending batch and the number of kept routes the frame before:
    /// a batch that ends with more kept routes was kept.
    last_batch: u64,
    last_kept: usize,
}

impl RouteSwitch {
    /// The view after this frame's `r`.
    fn step(&mut self, view: View, r: &Routing) -> View {
        let kept = r.edges.iter().filter(|e| !e.pending).count();
        let was_kept = self.last_batch != 0 && r.batch == 0 && kept > self.last_kept;
        (self.last_batch, self.last_kept) = (r.batch, kept);
        if was_kept {
            self.back_to = None;
            return if self.manual { view } else { View::Routes };
        }
        if r.focus && r.batch != 0 && r.batch != self.fired {
            self.fired = r.batch;
            if view != View::Routes {
                self.back_to = Some(view);
                self.manual = false;
                return View::Routes;
            }
            return view;
        }
        if !r.focus {
            if let Some(v) = self.back_to.take() {
                if !self.manual && view == View::Routes {
                    return v;
                }
            }
        }
        view
    }
}

/// The three kinds a part can be, as the kind control names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind3 {
    Pattern,
    Chord,
    Held,
}

/// The kind control of a finger: a part's three kinds, or Parameter (what
/// the finger's routes drive; the part stays untouched).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind4 {
    Part(Kind3),
    Param,
}

/// The routes a part's owner can show under Parameter: a finger's.
fn param_src(owner: Owner) -> Option<Src> {
    match owner {
        Owner::Slot(s) => s.finger_index().map(|f| Src::Finger(s.side, f)),
        Owner::Wall(_) => None,
    }
}

/// One row per route: "Pinch → Serum 2 · Cutoff", a suggested one dimmer
/// with "· suggested" (its Keep and Discard sit in the top section), a
/// kept one with an `x` at the row's right end that unmaps it on the
/// second click (glass::arm_x); none: "Not routed yet. Type it in the
/// chat above." Returns the (signal, target) whose `x` confirmed.
pub(crate) fn param_list(ui: &mut Ui, edges: &[&EdgeIn], t: &Text) -> Option<(String, NodeTarget)> {
    if edges.is_empty() {
        hint(ui, t.f_param_none);
        return None;
    }
    let mut gone = None;
    for (i, e) in edges.iter().enumerate() {
        let mut line = format!("{} → {}", graph::signal_label(&e.signal, t), graph::target_label(&e.target));
        if e.pending {
            line = format!("{line} · {}", t.f_param_pending);
        }
        let a = if e.pending { Tok::HINT } else { Tok::VALUE };
        let h = glass::galley(ui, &line, Tok::BODY).size().y.max(Tok::CTRL_H_SM);
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
        let mut right = row.right();
        if !e.pending {
            let xr = Rect::from_center_size(pos2(row.right() - Tok::CTRL_H_SM / 2.0, row.center().y), vec2(Tok::CTRL_H_SM, Tok::CTRL_H_SM));
            let (confirmed, left) = glass::arm_x(ui, ui.id().with(("unmap", i, &e.signal)), xr, row, t.remove_hover, t.unmap_arm);
            if confirmed {
                gone = Some((e.signal.clone(), e.target.clone()));
            }
            right = left - Tok::GAP;
        }
        let g = glass::galley(ui, &glass::fit(ui, &line, Tok::BODY, (right - row.left()).max(0.0)), Tok::BODY);
        glass::text_at(ui.painter(), pos2(row.left(), row.center().y - g.size().y / 2.0), g, white(a));
    }
    gone
}

fn kind_of(p: &Play) -> Kind3 {
    match p {
        Play::Clip { .. } => Kind3::Pattern,
        Play::Chords { .. } => Kind3::Chord,
        Play::Note { .. } => Kind3::Held,
    }
}

/// The part as it was before a conversion, a kind switch or a template:
/// "Back to {name}" restores it. One slot, cleared by Back, by another
/// selection or by a newer undo.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Undo {
    part: u64,
    play: Play,
    start: Start,
    mode: Mode,
    name: String,
}

pub struct Fingers {
    calib: Option<crate::calib::Calib>,
    /// The lines after a calibration and when it ended.
    cal_msg: Option<(Vec<String>, std::time::Instant)>,
    sel: Option<Slot>,
    view: View,
    /// Patterns as a piano roll instead of drum lanes.
    piano: bool,
    art: Option<Art>,
    tex: [Option<TextureHandle>; 2],
    roll: Roll,
    undo: Option<Undo>,
    /// Where the roll was drawn last (the tests aim with it).
    geom: Option<Geom>,
    /// The roll is shown (its fold in the part's header row).
    roll_open: bool,
    /// (part id, height) of the tools above the roll: the tallest they
    /// were for this part.
    tools_h: Option<(u64, f32)>,
    /// The wall editor's roll, undo slot, tools height and view choices,
    /// apart from the finger's: both can be on screen at once.
    wall: Pane,
    /// writer.state_gen() as last seen: a change is a preset or project
    /// load, and what the editor holds about the old parts goes.
    seen_gen: Option<u64>,
    /// A wall row clicked in the total view, for the Pong section to open.
    wall_pick: Option<Side>,
    /// The Routes view's switch and the batch the window passed last.
    route: RouteSwitch,
    routes_batch: u64,
    /// A target node was clicked (take_unfold).
    unfold: bool,
    /// The next part editor scrolls its owner row to the top.
    scroll_owner: bool,
    /// Where the hands' canvases and the owner row were drawn last, and
    /// the finger colours (lit, unlit) the textures were built with.
    canvas: [Rect; 2],
    owner_row: Option<Rect>,
    cols: [[(Color32, Color32); 5]; 2],
    /// The saved parts' names (part_lib), read when the part picker opens
    /// and after a save or delete.
    saved: Option<Vec<String>>,
    /// The last save or delete error of a part.
    part_msg: Option<String>,
    /// The calibration's progress and since when it stood still.
    cal_still: Option<(f32, std::time::Instant)>,
    /// A hand picked by its node: the editor lists what it drives.
    hand_sel: Option<usize>,
    /// The part editor shows the Parameter kind (the selected finger's
    /// routes), its MIDI part stays as it is.
    params: bool,
    /// The chat field's routes from the hands this frame.
    edges: Vec<EdgeIn>,
    /// Where the hand nodes were drawn last, with their text.
    hand_nodes: [Option<(HandNode, String)>; 2],
    /// A kept route's `x` in a drives list confirmed (take_unmap).
    unmap: Option<(String, NodeTarget)>,
    /// The Routes view's open search and its retargets and removals.
    route_edit: graph::Edits,
}

impl Default for Fingers {
    fn default() -> Self {
        Fingers {
            calib: None,
            cal_msg: None,
            sel: Some(Slot::finger(1, 1)),
            view: View::Single,
            piano: false,
            art: None,
            tex: [None, None],
            roll: Roll::default(),
            undo: None,
            geom: None,
            roll_open: true,
            tools_h: None,
            wall: Pane::default(),
            seen_gen: None,
            wall_pick: None,
            route: RouteSwitch::default(),
            routes_batch: 0,
            unfold: false,
            scroll_owner: false,
            canvas: [Rect::NOTHING; 2],
            owner_row: None,
            cols: [[(Color32::TRANSPARENT, Color32::TRANSPARENT); 5]; 2],
            saved: None,
            part_msg: None,
            cal_still: None,
            hand_sel: None,
            params: false,
            edges: Vec::new(),
            hand_nodes: [None, None],
            unmap: None,
            route_edit: graph::Edits::default(),
        }
    }
}

/// What the part editor keeps between frames besides the part, for the
/// wall editor (the finger's are Fingers' own fields): a gesture, a
/// template's undo, the tools' height, "Hide roll" and lanes or piano roll
/// belong to the editor they were made in.
struct Pane {
    roll: Roll,
    undo: Option<Undo>,
    tools_h: Option<(u64, f32)>,
    piano: bool,
    roll_open: bool,
    scroll_owner: bool,
}

impl Default for Pane {
    fn default() -> Self {
        Pane { roll: Roll::default(), undo: None, tools_h: None, piano: false, roll_open: true, scroll_owner: false }
    }
}

/// What the window shows of the player and FL this frame.
pub struct Live<'a> {
    pub on: &'a [(String, Trigger)],
    pub playing: &'a [u64],
    pub player: &'a midi::Player,
    pub channels: &'a [fl::ChannelRow],
    pub linked: bool,
    pub focus_ch: Option<u32>,
}

// --- piano roll: geometry ------------------------------------------------------

/// Ticks per step: `nudge` counts 1/96 beat, 24 to a 1/16.
const TICKS: f32 = 24.0;
const LANE_LABEL_W: f32 = 124.0;
/// The lane popover's width (a GM name and its note).
const LANE_POP_W: f32 = 200.0;
const KEYS_W: f32 = 48.0;
/// The black part of a black key.
const BLACK_W: f32 = 30.0;
const LANE_ROW_H: f32 = 22.0;
const ROLL_ROW_H: f32 = 16.0;
/// Rows the chromatic roll shows; it scrolls inside itself for the rest.
const ROLL_VIS: u8 = 15;
const RULER_H: f32 = 18.0;
const EVENT_H: f32 = 48.0;
/// Height of a full velocity bar (127) or eight roll blocks.
const EVENT_SPAN: f32 = EVENT_H - 4.0;
const EVENT_GAP: f32 = 4.0;
/// Points a press moves before it counts as a drag: a click on a note
/// never moves it.
const DRAG_START: f32 = 3.0;
const LANE_PT_PER_SEMI: f32 = 6.0;
const WHEEL_PT: f32 = 14.0;
/// A note's right edge reaches this far past its end when no note is there.
const EDGE_OUT: f32 = 2.0;
/// GM drum notes in FPC's default kit.
const GM: [(u8, &str); 8] = [(36, "Kick"), (38, "Snare"), (39, "Clap"), (42, "Closed hat"), (44, "Pedal hat"), (46, "Open hat"), (49, "Crash"), (51, "Ride")];

fn gm_name(p: u8) -> Option<&'static str> {
    GM.iter().find(|g| g.0 == p).map(|g| g.1)
}

/// Where the roll's parts are this frame: label column (lane names or
/// keyboard), ruler, grid, event lane; rows highest first.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Geom {
    block: Rect,
    label: Rect,
    ruler: Rect,
    grid: Rect,
    event: Option<Rect>,
    cell_w: f32,
    row_h: f32,
    rows: Vec<u8>,
    n: u16,
}

impl Geom {
    fn new(origin: Pos2, width: f32, label_w: f32, rows: Vec<u8>, row_h: f32, n: u16, event: bool) -> Geom {
        let grid_w = (width - label_w).max(1.0);
        let ruler = Rect::from_min_size(origin + vec2(label_w, 0.0), vec2(grid_w, RULER_H));
        let grid = Rect::from_min_size(ruler.left_bottom(), vec2(grid_w, rows.len() as f32 * row_h));
        let label = Rect::from_min_max(pos2(origin.x, grid.top()), pos2(origin.x + label_w, grid.bottom()));
        let event = event.then(|| Rect::from_min_size(pos2(grid.left(), grid.bottom() + EVENT_GAP), vec2(grid_w, EVENT_H)));
        let bottom = event.map_or(grid.bottom(), |e| e.bottom());
        Geom { block: Rect::from_min_max(origin, pos2(origin.x + width, bottom)), label, ruler, grid, event, cell_w: grid_w / f32::from(n.max(1)), row_h, rows, n }
    }

    pub(crate) fn x_of_tick(&self, t: f32) -> f32 {
        self.grid.left() + t / TICKS * self.cell_w
    }

    pub(crate) fn tick_at(&self, x: f32) -> f32 {
        (x - self.grid.left()) / self.cell_w * TICKS
    }

    fn row_at(&self, y: f32) -> usize {
        let r = ((y - self.grid.top()) / self.row_h).floor().max(0.0) as usize;
        r.min(self.rows.len().saturating_sub(1))
    }

    fn row_top(&self, i: usize) -> f32 {
        self.grid.top() + i as f32 * self.row_h
    }

    #[cfg(test)]
    fn row_center(&self, i: usize) -> f32 {
        self.row_top(i) + self.row_h / 2.0
    }

    fn row_of(&self, pitch: u8) -> Option<usize> {
        self.rows.iter().position(|p| *p == pitch)
    }

    fn row_rect(&self, i: usize, x: egui::Rangef) -> Rect {
        Rect::from_x_y_ranges(x, self.row_top(i)..=self.row_top(i) + self.row_h)
    }

    /// The hit rect of a step in `row`, clipped at the loop end.
    fn note_rect(&self, s: &Step, row: usize) -> Rect {
        let t0 = start_tick(s);
        let x1 = self.x_of_tick(t0 + f32::from(s.len.max(1)) * TICKS).min(self.grid.right());
        self.row_rect(row, (self.x_of_tick(t0)..=x1).into())
    }

    fn shifted(&self, d: Vec2) -> Geom {
        let mut g = self.clone();
        for r in [&mut g.block, &mut g.label, &mut g.ruler, &mut g.grid] {
            *r = r.translate(d);
        }
        g.event = g.event.map(|e| e.translate(d));
        g
    }
}

fn start_tick(s: &Step) -> f32 {
    f32::from(s.at) * TICKS + f32::from(s.nudge.min(23))
}

fn steps_of(bars: u8) -> u16 {
    u16::from(bars.clamp(1, midi::MAX_BARS)) * midi::STEPS
}

/// A part's notes as the roll shows them: a clip as it is, a chord as the
/// clip it would become, a held note as one bar-long note.
struct ViewClip {
    bars: u8,
    steps: Vec<Step>,
    rows: Vec<u8>,
}

fn view_clip(part: &Part, key: Key) -> ViewClip {
    match &part.play {
        Play::Clip { bars, steps, rows } => ViewClip { bars: *bars, steps: steps.clone(), rows: rows.clone() },
        Play::Chords { .. } => match midi::chords_as_clip(key, &part.play) {
            Some(Play::Clip { bars, steps, rows }) => ViewClip { bars, steps, rows },
            _ => ViewClip { bars: 1, steps: Vec::new(), rows: Vec::new() },
        },
        Play::Note { pitch, vel } => ViewClip { bars: 1, steps: vec![Step { at: 0, len: midi::STEPS, pitch: *pitch, vel: *vel, ratchet: 1, nudge: 0 }], rows: vec![*pitch] },
    }
}

/// Drum lanes: the pitches the clip uses plus the kept rows, highest
/// first; C5 (a Sampler's root) when there are none.
fn lane_rows_of(steps: &[Step], kept: &[u8]) -> Vec<u8> {
    let mut rows: Vec<u8> = steps.iter().map(|s| s.pitch).chain(kept.iter().copied()).collect();
    rows.sort_unstable_by(|a, b| b.cmp(a));
    rows.dedup();
    if rows.is_empty() {
        rows.push(60);
    }
    rows
}

fn lane_rows(part: &Part) -> Vec<u8> {
    match &part.play {
        Play::Clip { steps, rows, .. } => lane_rows_of(steps, rows),
        Play::Note { pitch, .. } => vec![*pitch],
        Play::Chords { .. } => Vec::new(),
    }
}

/// The roll's top row when a part opens (or on a double-click on the
/// keyboard): the notes centred in the visible rows, the highest kept in
/// view when they span more; G5 down to F4 without notes (FL names, 60 =
/// C5). Edits never call it: the rows stay where they are.
fn auto_top(steps: &[Step], n: u16) -> u8 {
    let vis = || steps.iter().filter(|s| s.at < n).map(|s| s.pitch);
    let (Some(lo), Some(hi)) = (vis().min(), vis().max()) else { return 67 };
    let span = hi - lo + 1;
    let top = if span <= ROLL_VIS { u16::from(hi) + u16::from((ROLL_VIS - span) / 2) } else { u16::from(hi) + 2 };
    top.clamp(u16::from(ROLL_VIS - 1), 127) as u8
}

fn roll_rows(top: u8) -> Vec<u8> {
    let top = top.clamp(ROLL_VIS - 1, 127);
    (0..ROLL_VIS).map(|i| top - i).collect()
}

/// What the pointer is over in the grid. Inside a note beats the reach
/// past another note's end, so a back-to-back neighbour keeps its start.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Empty,
    Body(usize),
    Edge(usize),
}

/// Visible steps in draw order; `last` (the dragged one) is drawn last.
fn draw_order(steps: &[Step], n: u16, last: Option<usize>) -> Vec<usize> {
    let mut v: Vec<usize> = (0..steps.len()).filter(|&i| steps[i].at < n && Some(i) != last).collect();
    if let Some(d) = last.filter(|d| *d < steps.len() && steps[*d].at < n) {
        v.push(d);
    }
    v
}

fn hit(g: &Geom, steps: &[Step], p: Pos2, last: Option<usize>) -> Hit {
    if g.rows.is_empty() {
        return Hit::Empty;
    }
    let row = g.row_at(p.y);
    let pitch = g.rows[row];
    let order: Vec<usize> = draw_order(steps, g.n, last).into_iter().rev().filter(|&i| steps[i].pitch == pitch).collect();
    for &i in &order {
        let r = g.note_rect(&steps[i], row);
        if r.contains(p) {
            let e = (0.35 * r.width()).clamp(3.0, 6.0);
            return if p.x >= r.right() - e { Hit::Edge(i) } else { Hit::Body(i) };
        }
    }
    for &i in &order {
        let r = g.note_rect(&steps[i], row);
        if p.x >= r.right() && p.x <= r.right() + EDGE_OUT && r.y_range().contains(p.y) {
            return Hit::Edge(i);
        }
    }
    Hit::Empty
}

/// A touched note obeys the edit rules: len >= 1, inside the loop, nudge
/// <= 23, vel 1..127, ratchet 1..8.
fn clamp_step(mut s: Step, n: u16) -> Step {
    s.len = s.len.clamp(1, n.max(1));
    s.vel = s.vel.clamp(1, 127);
    s.ratchet = s.ratchet.clamp(1, 8);
    let max = u32::from(n.saturating_sub(s.len)) * 24;
    let start = (u32::from(s.at) * 24 + u32::from(s.nudge.min(23))).min(max);
    s.at = (start / 24) as u16;
    s.nudge = (start % 24) as u8;
    s
}

/// `orig` dragged from `origin` to `pos`: whole steps after half a cell
/// (nudge back to 0), ticks with Alt; the pitch is the row under `pos`.
/// The start follows the pointer up to the last step; a note that would
/// reach past the loop end ends there instead (its steps come back when
/// the same drag returns), and a nudge may hang over the end by less than
/// a step, as a strummed chord's notes do. A chord's notes fill their bar,
/// so without this rule they could not move in time at all.
fn moved_step(orig: &Step, g: &Geom, origin: Pos2, pos: Pos2, alt: bool) -> Step {
    let len = orig.len.clamp(1, g.n);
    let t0 = start_tick(orig);
    let dt = g.tick_at(pos.x) - g.tick_at(origin.x);
    let start = if alt {
        (t0 + dt).round()
    } else if (pos.x - origin.x).abs() < g.cell_w / 2.0 {
        t0
    } else {
        ((t0 + dt) / TICKS).round() * TICKS
    };
    let start = start.clamp(0.0, f32::from(g.n.max(1) - 1) * TICKS + TICKS - 1.0) as u32;
    let at = (start / 24) as u16;
    let pitch = g.rows.get(g.row_at(pos.y)).copied().unwrap_or(orig.pitch);
    Step { at, nudge: (start % 24) as u8, len: len.min(g.n - at).max(1), pitch, vel: orig.vel.clamp(1, 127), ratchet: orig.ratchet.clamp(1, 8) }
}

/// `orig`'s right edge dragged: whole steps, never past the loop end.
fn resized_step(orig: &Step, g: &Geom, origin: Pos2, pos: Pos2) -> Step {
    let s = clamp_step(*orig, g.n);
    let t0 = start_tick(&s);
    let t1 = t0 + f32::from(s.len) * TICKS;
    let dt = g.tick_at(pos.x) - g.tick_at(origin.x);
    let max = ((f32::from(g.n) * TICKS - t0) / TICKS).floor().max(1.0);
    let len = ((t1 + dt - t0) / TICKS).round().clamp(1.0, max) as u16;
    Step { len, ..s }
}

/// From `from`, |k| semitones in k's direction, skipping `others`,
/// stopping at 0 and 127.
fn step_pitch(from: u8, k: i32, others: &[u8]) -> u8 {
    let mut p = i32::from(from);
    let dir = k.signum();
    for _ in 0..k.unsigned_abs() {
        let mut q = p + dir;
        while (0..=127).contains(&q) && others.iter().any(|o| i32::from(*o) == q) {
            q += dir;
        }
        if !(0..=127).contains(&q) {
            break;
        }
        p = q;
    }
    u8::try_from(p).unwrap_or(from)
}

// --- piano roll: state and edits ----------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventLane {
    Velocity,
    Rolls,
}

/// The roll's state across frames and parts.
pub(crate) struct Roll {
    gesture: Gesture,
    /// Length of the last clicked or resized note: new notes take it (FL).
    last_len: u16,
    /// Velocity of the last clicked note.
    last_vel: u8,
    /// (pitch, at, nudge) of the last touched note.
    active: Option<(u8, u16, u8)>,
    /// The lane the event lane, Fill and the sel-bar act on (lanes mode).
    active_row: Option<u8>,
    lane: EventLane,
    /// (part id, top pitch) of the chromatic roll: fitted when a part
    /// opens, then moved only by the wheel or a double-click on the keys.
    top: Option<(u64, u8)>,
    /// Point wheel accumulator over the label column or the roll.
    wheel: f32,
    /// (part id, chord) the rows were last checked against: a chord whose
    /// fields changed brings the rows along when a note left them.
    chord_seen: Option<(u64, Play)>,
    /// The lane whose popover is open.
    pop_lane: Option<u8>,
}

impl Default for Roll {
    fn default() -> Self {
        Roll { gesture: Gesture::Idle, last_len: 1, last_vel: 100, active: None, active_row: None, lane: EventLane::Velocity, top: None, wheel: 0.0, chord_seen: None, pop_lane: None }
    }
}

impl Roll {
    /// Drops what it holds about parts (a gesture, fitted rows, the active
    /// lane, an open popover); note length, velocity and the event lane
    /// stay as the person set them.
    fn forget(&mut self) {
        *self = Roll { last_len: self.last_len, last_vel: self.last_vel, lane: self.lane, ..Roll::default() };
    }

    /// A gesture on part `id` that began while it was a chord.
    fn on_chord(&self, id: u64) -> bool {
        matches!(&self.gesture, Gesture::On(d) if d.part == id && d.chord)
    }
}

enum Gesture {
    Idle,
    On(Box<Drag>),
}

/// A press held on the roll. The geometry is frozen at the press, so rows
/// do not move under the pointer while notes change.
struct Drag {
    part: u64,
    geom: Geom,
    origin: Pos2,
    /// The pointer's last known position: a pointer that left the window
    /// (PointerGone) stays here instead of jumping back to `origin`.
    at: Pos2,
    button: PointerButton,
    /// The part was a chord at the press: until the release the window
    /// keeps the chord's rows above the roll, so the roll does not move
    /// under the pointer when the gesture turns the chord into a pattern.
    chord: bool,
    kind: Kind,
}

enum Kind {
    Move { idx: usize, orig: Step, last: Step, rows_at_press: Vec<u8>, moved: bool },
    Resize { idx: usize, orig: Step, last: Step },
    Erase { last: Pos2 },
    Lane { from: u8, now: u8, dragged: bool, others: Vec<u8> },
    /// `pending`: a chord's velocity, written on release (a written chord
    /// restarts).
    Event { last_x: f32, fresh: bool, pending: Option<u8>, shown: Option<(Pos2, String)> },
}

/// What the roll draws a part with this frame.
pub(crate) struct RollCx<'a> {
    pub key: Key,
    pub color: Color32,
    /// The part's own count in steps (Player::beats_of × 4) while it plays.
    pub playhead: Option<f32>,
    /// Drum lanes instead of the chromatic roll (patterns only).
    pub lanes: bool,
    /// FPC channel or none: lanes show GM drum names.
    pub drums: bool,
    pub t: &'a Text,
}

/// Makes the part a clip it can edit as notes: a chord becomes its notes
/// (starting on the next bar), a held note one bar-long note. Remembers
/// the old part for "Back". True when it converted.
pub(crate) fn ensure_clip(part: &mut Part, key: Key, undo: &mut Option<Undo>) -> bool {
    let (clip, bar) = match &part.play {
        Play::Clip { .. } => return false,
        Play::Chords { .. } => match midi::chords_as_clip(key, &part.play) {
            Some(c) => (c, true),
            None => return false,
        },
        Play::Note { pitch, vel } => {
            (Play::Clip { bars: 1, steps: vec![Step { at: 0, len: midi::STEPS, pitch: *pitch, vel: *vel, ratchet: 1, nudge: 0 }], rows: vec![*pitch] }, false)
        }
    };
    remember(part, undo);
    part.play = clip;
    if bar {
        part.start = Start::Bar;
    }
    true
}

fn remember(part: &Part, undo: &mut Option<Undo>) {
    *undo = Some(Undo { part: part.id, play: part.play.clone(), start: part.start, mode: part.mode, name: part.name.clone() });
}

/// "Back to {name}": the part as the undo slot holds it.
pub(crate) fn back(part: &mut Part, undo: &mut Option<Undo>) -> bool {
    match undo.take() {
        Some(u) if u.part == part.id => {
            part.play = u.play;
            part.start = u.start;
            part.mode = u.mode;
            part.name = u.name;
            true
        }
        other => {
            *undo = other;
            false
        }
    }
}

/// The kind control: every switch remembers the part for "Back".
pub(crate) fn set_kind(part: &mut Part, to: Kind3, key: Key, finger: Option<usize>, active_row: Option<u8>, undo: &mut Option<Undo>) {
    let from = kind_of(&part.play);
    if from == to {
        return;
    }
    let old = undo.take().filter(|u| u.part == part.id);
    let restore = old.as_ref().is_some_and(|u| match to {
        Kind3::Chord => matches!(u.play, Play::Chords { .. }),
        // a held note made from a pattern goes back to that pattern
        Kind3::Pattern => from == Kind3::Held && matches!(u.play, Play::Clip { .. }),
        Kind3::Held => false,
    });
    remember(part, undo);
    if let (true, Some(u)) = (restore, old) {
        part.play = u.play;
        part.start = u.start;
        part.name = u.name;
        return;
    }
    match to {
        Kind3::Pattern => {
            let mut scratch = None;
            ensure_clip(part, key, &mut scratch);
        }
        Kind3::Chord => {
            let d = finger.map_or(0, |f| [0, 5, 2, 6, 4][f]);
            part.play = Play::Chords { degrees: vec![d], size: 3, octave: 4, vel: 96, strum: 0 };
            part.name = midi::chord_name(key, d, 3);
        }
        Kind3::Held => {
            let pitch = match &part.play {
                Play::Clip { steps, .. } => {
                    let lane = active_row.filter(|r| steps.iter().any(|s| s.pitch == *r));
                    lane.or_else(|| steps.iter().map(|s| s.pitch).min()).unwrap_or(60)
                }
                Play::Chords { degrees, size, octave, .. } => degrees.first().and_then(|d| midi::chord(key, *d, *size, *octave).into_iter().min()).unwrap_or(60),
                Play::Note { pitch, .. } => *pitch,
            };
            part.play = Play::Note { pitch, vel: 100 };
            part.name = midi::note_name(pitch);
        }
    }
}

fn clip_mut(part: &mut Part) -> Option<(&mut u8, &mut Vec<Step>, &mut Vec<u8>)> {
    match &mut part.play {
        Play::Clip { bars, steps, rows } => Some((bars, steps, rows)),
        _ => None,
    }
}

/// Every note on lane `from` moves to `to`, and so does the lane itself
/// (also an empty one, like the default C5 lane).
fn lane_pitch(part: &mut Part, from: u8, to: u8, roll: &mut Roll) {
    match &mut part.play {
        Play::Clip { steps, rows, .. } => {
            for s in steps.iter_mut().filter(|s| s.pitch == from) {
                s.pitch = to;
            }
            rows.retain(|r| *r != from);
            if !rows.contains(&to) {
                rows.push(to);
            }
        }
        Play::Note { pitch, .. } => {
            if part.name == midi::note_name(*pitch) {
                part.name = midi::note_name(to);
            }
            *pitch = to;
        }
        Play::Chords { .. } => return,
    }
    roll.active_row = Some(to);
    if roll.pop_lane == Some(from) {
        roll.pop_lane = Some(to);
    }
}

/// Removes every note a point of the segment a..b touches (every 2 pt).
/// `lanes`: a lane keeps its row when its last note goes ("Remove row"
/// removes the row).
#[allow(clippy::too_many_arguments)]
fn erase_along(part: &mut Part, key: Key, undo: &mut Option<Undo>, g: &Geom, a: Pos2, b: Pos2, lanes: bool) {
    let d = b - a;
    let k = (d.length() / 2.0).ceil().max(1.0) as usize;
    let samples: Vec<Pos2> = (0..=k).map(|j| a + d * (j as f32 / k as f32)).collect();
    let view = view_clip(part, key);
    let hits: Vec<usize> = (0..view.steps.len())
        .filter(|&i| {
            let s = &view.steps[i];
            s.at < g.n && g.row_of(s.pitch).is_some_and(|r| samples.iter().any(|p| g.note_rect(s, r).contains(*p)))
        })
        .collect();
    if hits.is_empty() {
        return;
    }
    ensure_clip(part, key, undo);
    if let Some((_, steps, kept)) = clip_mut(part) {
        for i in hits.into_iter().rev() {
            if i < steps.len() {
                let gone = steps.remove(i);
                if lanes && !kept.contains(&gone.pitch) {
                    kept.push(gone.pitch);
                }
            }
        }
    }
}

/// The event lane's value at height `y`.
fn event_value(e: Rect, lane: EventLane, y: f32) -> u8 {
    let f = (e.bottom() - 2.0 - y) / EVENT_SPAN;
    match lane {
        EventLane::Velocity => (1.0 + 126.0 * f).round().clamp(1.0, 127.0) as u8,
        EventLane::Rolls => (1.0 + (8.0 * f).floor()).clamp(1.0, 8.0) as u8,
    }
}

/// The lane the event lane and Fill act on: the active one, else the top.
fn active_lane(roll: &Roll, rows: &[u8]) -> Option<u8> {
    roll.active_row.filter(|r| rows.contains(r)).or(rows.first().copied())
}

/// Steps the event lane edits: all visible ones, or the active lane's.
fn event_steps(steps: &[Step], n: u16, lane: Option<u8>) -> Vec<usize> {
    (0..steps.len()).filter(|&i| steps[i].at < n && lane.is_none_or(|l| steps[i].pitch == l)).collect()
}

/// One frame of the event lane at `pos`: on the press frame the notes at
/// the nearest start, later every start the pointer swept past.
#[allow(clippy::too_many_arguments)]
fn event_drag(part: &mut Part, cx: &RollCx, roll_lane: EventLane, lane: Option<u8>, undo: &mut Option<Undo>, g: &Geom, last_x: f32, fresh: bool, pos: Pos2) -> (Option<u8>, Option<u8>) {
    let Some(e) = g.event else { return (None, None) };
    let view = view_clip(part, cx.key);
    let cand = event_steps(&view.steps, g.n, lane);
    let tol = (g.cell_w / 2.0).max(3.0);
    let xs = |i: usize| g.x_of_tick(start_tick(&view.steps[i]));
    let targets: Vec<usize> = if fresh {
        let best = cand.iter().map(|&i| (i, (xs(i) - pos.x).abs())).filter(|(_, d)| *d < tol).min_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((b, _)) => {
                let t = start_tick(&view.steps[b]);
                cand.into_iter().filter(|&i| start_tick(&view.steps[i]) == t).collect()
            }
            None => Vec::new(),
        }
    } else {
        let (lo, hi) = (last_x.min(pos.x) - tol, last_x.max(pos.x) + tol);
        cand.into_iter().filter(|&i| xs(i) > lo && xs(i) < hi).collect()
    };
    let v = event_value(e, roll_lane, pos.y);
    if targets.is_empty() {
        return (None, Some(v));
    }
    if roll_lane == EventLane::Velocity && matches!(part.play, Play::Chords { .. }) {
        return (Some(v), Some(v));
    }
    ensure_clip(part, cx.key, undo);
    if let Some((_, steps, _)) = clip_mut(part) {
        for i in targets {
            if let Some(s) = steps.get_mut(i) {
                match roll_lane {
                    EventLane::Velocity => s.vel = v,
                    EventLane::Rolls => s.ratchet = v,
                }
            }
        }
    }
    (None, Some(v))
}

/// The pointer and buttons this frame. On macOS a Ctrl-click (baseview
/// sends Control as `command`) is a right press.
struct Inp {
    pos: Option<Pos2>,
    origin: Option<Pos2>,
    press: Option<(PointerButton, PointerButton)>,
    mods: Modifiers,
    down: [bool; 2],
    released: [bool; 2],
    wheel_lines: f32,
    wheel_points: f32,
}

impl Inp {
    fn read(i: &InputState) -> Inp {
        let mac_ctrl = cfg!(target_os = "macos") && (i.modifiers.command || i.modifiers.ctrl);
        let press = if i.pointer.button_pressed(PointerButton::Primary) {
            Some((PointerButton::Primary, if mac_ctrl { PointerButton::Secondary } else { PointerButton::Primary }))
        } else if i.pointer.button_pressed(PointerButton::Secondary) {
            Some((PointerButton::Secondary, PointerButton::Secondary))
        } else {
            None
        };
        let (mut lines, mut points) = (0.0f32, 0.0f32);
        for e in &i.events {
            if let Event::MouseWheel { unit, delta, .. } = e {
                if delta.y == 0.0 {
                    continue;
                }
                match unit {
                    MouseWheelUnit::Point => points += delta.y,
                    MouseWheelUnit::Line | MouseWheelUnit::Page => lines += delta.y,
                }
            }
        }
        // a press released in the same frame has no press origin left
        let tapped = i.events.iter().rev().find_map(|e| match e {
            Event::PointerButton { pos, pressed: true, .. } => Some(*pos),
            _ => None,
        });
        Inp {
            pos: i.pointer.latest_pos(),
            origin: i.pointer.press_origin().or(tapped),
            press,
            mods: i.modifiers,
            down: [i.pointer.button_down(PointerButton::Primary), i.pointer.button_down(PointerButton::Secondary)],
            released: [i.pointer.button_released(PointerButton::Primary), i.pointer.button_released(PointerButton::Secondary)],
            wheel_lines: lines,
            wheel_points: points,
        }
    }
}

/// One wheel step at most per frame: a line, or 14 points of a trackpad.
fn wheel_step(roll: &mut Roll, inp: &Inp) -> i32 {
    roll.wheel += inp.wheel_points;
    if inp.wheel_lines != 0.0 {
        return inp.wheel_lines.signum() as i32;
    }
    if roll.wheel.abs() >= WHEEL_PT {
        let s = roll.wheel.signum();
        roll.wheel -= s * WHEEL_PT;
        return s as i32;
    }
    0
}

/// The chromatic roll's wheel in semitones: two per wheel notch, one per
/// row height of a trackpad's points (fractions add up across frames).
fn roll_wheel(roll: &mut Roll, inp: &Inp) -> i32 {
    roll.wheel += inp.wheel_points + inp.wheel_lines * 2.0 * ROLL_ROW_H;
    let k = (roll.wheel / ROLL_ROW_H).trunc();
    roll.wheel -= k * ROLL_ROW_H;
    k as i32
}

/// The page's ScrollArea reads the scroll after its content: emptied here,
/// the page does not scroll while the wheel works the roll.
fn swallow_scroll(ui: &Ui) {
    ui.ctx().input_mut(|i| {
        i.smooth_scroll_delta = Vec2::ZERO;
        i.raw_scroll_delta = Vec2::ZERO;
    });
}

fn bidx(b: PointerButton) -> usize {
    usize::from(b != PointerButton::Primary)
}

/// What a lane popover row picked.
enum Pick {
    To(u8),
    Remove,
}

/// The rows of a lane's popover: GM drums (FPC or no channel), the
/// Sampler root, and "Remove row".
fn lane_menu(ui: &mut Ui, drums: bool, lane: u8, remove: bool, extra: Option<u8>, t: &Text) -> Option<Pick> {
    let mut pick = None;
    if let Some(p) = extra {
        if glass::pop_row(ui, false, &midi::note_name(p)).clicked() {
            pick = Some(Pick::To(p));
        }
    }
    if drums {
        for (p, name) in GM {
            if glass::pop_row(ui, p == lane, &format!("{name} {}", midi::note_name(p))).clicked() {
                pick = Some(Pick::To(p));
            }
        }
    }
    if glass::pop_row(ui, lane == 60, t.f_sampler_root).clicked() {
        pick = Some(Pick::To(60));
    }
    if remove {
        glass::sep_soft(ui);
        if glass::pop_row(ui, false, t.f_remove_row).clicked() {
            pick = Some(Pick::Remove);
        }
    }
    pick
}

// --- piano roll: the widget -----------------------------------------------------

/// The roll of one part: lanes or keyboard, ruler, grid, event lane.
/// Returns the geometry it used (the tests aim with it).
pub(crate) fn roll_ui(ui: &mut Ui, part: &mut Part, cx: &RollCx, roll: &mut Roll, undo: &mut Option<Undo>) -> Geom {
    let id = part.id;
    let held = matches!(part.play, Play::Note { .. });
    let lanes = cx.lanes || held;
    if matches!(&roll.gesture, Gesture::On(d) if d.part != id) {
        roll.gesture = Gesture::Idle;
    }
    let view = view_clip(part, cx.key);
    let frozen = match &roll.gesture {
        Gesture::On(d) => Some(d.geom.clone()),
        Gesture::Idle => None,
    };
    let (width, label_w, rows, row_h, n) = match &frozen {
        Some(g) => (g.block.width(), g.label.width(), g.rows.clone(), g.row_h, g.n),
        None => {
            let n = steps_of(view.bars);
            if lanes {
                (ui.available_width(), LANE_LABEL_W, lane_rows_of(&view.steps, &view.rows), LANE_ROW_H, n)
            } else {
                let mut top = match roll.top {
                    Some((p, t)) if p == id => t,
                    _ => auto_top(&view.steps, n),
                };
                // a chord's fields move all its notes at once (octave, size,
                // another degree): then the rows follow if a note left them.
                // Notes edited one by one and the wheel never move them.
                if matches!(part.play, Play::Chords { .. }) && roll.chord_seen.as_ref() != Some(&(id, part.play.clone())) {
                    let rows = roll_rows(top);
                    if view.steps.iter().any(|s| s.at < n && !rows.contains(&s.pitch)) {
                        top = auto_top(&view.steps, n);
                    }
                    roll.chord_seen = Some((id, part.play.clone()));
                }
                roll.top = Some((id, top));
                (ui.available_width(), KEYS_W, roll_rows(top), ROLL_ROW_H, n)
            }
        }
    };
    let height = RULER_H + rows.len() as f32 * row_h + if held { 0.0 } else { EVENT_GAP + EVENT_H };
    let (block, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let mut geom = Geom::new(block.min, width, label_w, rows, row_h, n, !held);
    if let Gesture::On(d) = &mut roll.gesture {
        let shift = block.min - d.geom.block.min;
        if shift != Vec2::ZERO {
            d.origin += shift;
            d.at += shift;
            d.geom = d.geom.shifted(shift);
            match &mut d.kind {
                Kind::Erase { last } => *last += shift,
                Kind::Event { last_x, .. } => *last_x += shift.x,
                _ => {}
            }
        }
    }

    let grid_resp = (!held).then(|| ui.interact(geom.grid, Id::new(("roll-grid", id)), Sense::click_and_drag()));
    let label_resp = ui.interact(geom.label, Id::new((if lanes { "roll-label" } else { "roll-keys" }, id)), Sense::click_and_drag());
    let event_resp = geom.event.map(|r| ui.interact(r, Id::new(("roll-event", id)), Sense::click_and_drag()));
    let pop_id = Id::new(("lane-pop", id));
    let inp = ui.input(Inp::read);

    // a press starts a gesture on the zone it landed in; a press released
    // in the same frame (a trackpad tap) leaves no button down, only a click
    let pressed = |r: &egui::Response| r.is_pointer_button_down_on() || r.clicked() || r.secondary_clicked();
    if let (Some((phys, btn)), Some(origin), Gesture::Idle) = (inp.press, inp.origin, &roll.gesture) {
        let any_pop = ui.memory(|m| m.any_popup_open());
        let own_pop = ui.memory(|m| m.is_popup_open(pop_id));
        let chord = matches!(part.play, Play::Chords { .. });
        let drag = |kind: Kind| Gesture::On(Box::new(Drag { part: id, geom: geom.clone(), origin, at: origin, button: phys, chord, kind }));
        if grid_resp.as_ref().is_some_and(pressed) && !any_pop {
            roll.gesture = drag(start_grid(part, cx, roll, undo, &geom, origin, btn, inp.mods.alt));
        } else if pressed(&label_resp) && lanes && (!any_pop || own_pop) && !geom.rows.is_empty() {
            let lane = geom.rows[geom.row_at(origin.y)];
            roll.active_row = Some(lane);
            if btn == PointerButton::Secondary {
                roll.pop_lane = Some(lane);
                ui.memory_mut(|m| m.open_popup(pop_id));
            } else {
                let others = geom.rows.iter().copied().filter(|r| *r != lane).collect();
                roll.gesture = drag(Kind::Lane { from: lane, now: lane, dragged: false, others });
            }
        } else if event_resp.as_ref().is_some_and(pressed) && !any_pop && btn == PointerButton::Primary {
            roll.gesture = drag(Kind::Event { last_x: origin.x, fresh: true, pending: None, shown: None });
        }
    }

    // a held gesture follows the pointer until its button comes up
    if let Gesture::On(mut d) = std::mem::replace(&mut roll.gesture, Gesture::Idle) {
        let b = bidx(d.button);
        let pos = inp.pos.unwrap_or(d.at);
        d.at = pos;
        let alive = drag_frame(&mut d, part, cx, roll, undo, pos, inp.mods);
        // the frozen rows show a dragged lane at the note it reached
        if let (true, Kind::Lane { from, now, .. }) = (alive, &d.kind) {
            for r in geom.rows.iter_mut().filter(|r| **r == *from) {
                *r = *now;
            }
        }
        if alive {
            if inp.released[b] || !inp.down[b] {
                finish(*d, part, cx, roll, ui, pop_id);
            } else {
                roll.gesture = Gesture::On(d);
            }
        }
    }

    // the wheel: over a lane's name it moves the lane's note; over the
    // chromatic roll it scrolls the rows (and the page stays put)
    let hover = ui.input(|i| i.pointer.hover_pos());
    let idle = matches!(roll.gesture, Gesture::Idle);
    let over = |r: Rect| hover.is_some_and(|p| r.contains(p)) && ui.rect_contains_pointer(r);
    if lanes && over(geom.label) && !geom.rows.is_empty() {
        swallow_scroll(ui);
        let step = wheel_step(roll, &inp);
        if let (true, Some(h)) = (idle && step != 0, hover) {
            let lane = geom.rows[geom.row_at(h.y)];
            let others: Vec<u8> = geom.rows.iter().copied().filter(|r| *r != lane).collect();
            let to = step_pitch(lane, step, &others);
            if to != lane {
                lane_pitch(part, lane, to, roll);
            }
        }
    } else if !lanes && over(geom.block) && !geom.rows.is_empty() {
        swallow_scroll(ui);
        let semis = roll_wheel(roll, &inp);
        if idle && semis != 0 {
            let top = (i32::from(geom.rows[0]) + semis).clamp(i32::from(ROLL_VIS) - 1, 127) as u8;
            roll.top = Some((id, top));
        }
    } else {
        roll.wheel = 0.0;
    }
    if !lanes && label_resp.double_clicked() {
        roll.top = Some((id, auto_top(&view_clip(part, cx.key).steps, n)));
    }

    // the lane popover (closed when the lanes went away, else it would
    // stay open unseen and swallow every press)
    if !lanes && ui.memory(|m| m.is_popup_open(pop_id)) {
        ui.memory_mut(|m| m.close_popup());
    }
    if lanes && ui.memory(|m| m.is_popup_open(pop_id)) {
        if let Some(lane) = roll.pop_lane.filter(|l| geom.rows.contains(l)).or(geom.rows.first().copied()) {
            let r = geom.row_rect(geom.row_of(lane).unwrap_or(0), geom.label.x_range());
            let mut anchor = label_resp.clone();
            anchor.rect = r;
            anchor.interact_rect = r;
            let is_clip = matches!(part.play, Play::Clip { .. });
            let pick = glass::popup_list(ui, pop_id, r, &anchor, LANE_POP_W, |ui| lane_menu(ui, cx.drums, lane, is_clip, None, cx.t)).flatten();
            match pick {
                Some(Pick::To(p)) => {
                    if p != lane {
                        lane_pitch(part, lane, p, roll);
                    }
                    ui.memory_mut(|m| m.close_popup());
                }
                Some(Pick::Remove) => {
                    if let Some((_, steps, kept)) = clip_mut(part) {
                        steps.retain(|s| s.pitch != lane);
                        kept.retain(|k| *k != lane);
                    }
                    ui.memory_mut(|m| m.close_popup());
                }
                None => {}
            }
        }
    }

    paint_roll(ui, part, cx, roll, &geom, hover, inp.mods.alt, lanes, held);
    geom
}

/// A press in the grid: add a note (and drag it), grab one, its right
/// edge, or start erasing.
#[allow(clippy::too_many_arguments)]
fn start_grid(part: &mut Part, cx: &RollCx, roll: &mut Roll, undo: &mut Option<Undo>, g: &Geom, origin: Pos2, btn: PointerButton, alt: bool) -> Kind {
    if btn != PointerButton::Primary {
        return Kind::Erase { last: origin };
    }
    let view = view_clip(part, cx.key);
    // only drum lanes stay when their last note moves away
    let rows_at_press = if cx.lanes { g.rows.clone() } else { Vec::new() };
    match hit(g, &view.steps, origin, None) {
        Hit::Empty => {
            let pitch = g.rows.get(g.row_at(origin.y)).copied().unwrap_or(60);
            let t = g.tick_at(origin.x).max(0.0);
            let start = if alt { t.floor() as u32 } else { (t / TICKS).floor() as u32 * 24 };
            let len = roll.last_len.clamp(1, g.n);
            let start = start.min(u32::from(g.n - len) * 24);
            let s = Step { at: (start / 24) as u16, len: len.min(g.n - (start / 24) as u16).max(1), pitch, vel: roll.last_vel.clamp(1, 127), ratchet: 1, nudge: (start % 24) as u8 };
            ensure_clip(part, cx.key, undo);
            let idx = match clip_mut(part) {
                Some((_, steps, _)) => {
                    steps.push(s);
                    steps.len() - 1
                }
                None => return Kind::Erase { last: origin },
            };
            roll.active = Some((s.pitch, s.at, s.nudge));
            Kind::Move { idx, orig: s, last: s, rows_at_press, moved: false }
        }
        Hit::Body(i) => {
            let s = view.steps[i];
            roll.active = Some((s.pitch, s.at, s.nudge));
            roll.last_len = s.len.max(1);
            roll.last_vel = s.vel.clamp(1, 127);
            Kind::Move { idx: i, orig: s, last: s, rows_at_press, moved: false }
        }
        Hit::Edge(i) => {
            let s = view.steps[i];
            roll.active = Some((s.pitch, s.at, s.nudge));
            Kind::Resize { idx: i, orig: s, last: s }
        }
    }
}

/// One frame of a held gesture; false when the part changed under it
/// (the gesture ends without writing).
fn drag_frame(d: &mut Drag, part: &mut Part, cx: &RollCx, roll: &mut Roll, undo: &mut Option<Undo>, pos: Pos2, mods: Modifiers) -> bool {
    if d.part != part.id {
        return false;
    }
    let far = (pos - d.origin).length() >= DRAG_START;
    match &mut d.kind {
        Kind::Move { idx, orig, last, moved, .. } => {
            if view_clip(part, cx.key).steps.get(*idx) != Some(last) {
                return false;
            }
            *moved |= far;
            if *moved {
                let s = moved_step(orig, &d.geom, d.origin, pos, mods.alt);
                if s != *last {
                    ensure_clip(part, cx.key, undo);
                    if let Some(x) = clip_mut(part).and_then(|(_, steps, _)| steps.get_mut(*idx)) {
                        *x = s;
                        *last = s;
                    }
                }
            }
        }
        Kind::Resize { idx, orig, last } => {
            if view_clip(part, cx.key).steps.get(*idx) != Some(last) {
                return false;
            }
            if far {
                let s = resized_step(orig, &d.geom, d.origin, pos);
                if s != *last {
                    ensure_clip(part, cx.key, undo);
                    if let Some(x) = clip_mut(part).and_then(|(_, steps, _)| steps.get_mut(*idx)) {
                        *x = s;
                        *last = s;
                    }
                }
            }
        }
        Kind::Erase { last } => {
            erase_along(part, cx.key, undo, &d.geom, *last, pos, cx.lanes);
            *last = pos;
        }
        Kind::Lane { now, dragged, others, from } => {
            *dragged |= (pos.y - d.origin.y).abs() >= DRAG_START;
            if *dragged {
                let k = ((d.origin.y - pos.y) / LANE_PT_PER_SEMI).round() as i32 * if mods.shift { 12 } else { 1 };
                let to = step_pitch(*from, k, others);
                if to != *now {
                    lane_pitch(part, *now, to, roll);
                    *now = to;
                }
            }
        }
        Kind::Event { last_x, fresh, pending, shown } => {
            let lane = if cx.lanes { active_lane(roll, &d.geom.rows) } else { None };
            let (p, v) = event_drag(part, cx, roll.lane, lane, undo, &d.geom, *last_x, *fresh, pos);
            if p.is_some() {
                *pending = p;
            }
            *shown = v.map(|v| (pos, if roll.lane == EventLane::Velocity { v.to_string() } else { fill(cx.t.f_hits, &[&v]) }));
            *last_x = pos.x;
            *fresh = false;
        }
    }
    true
}

/// The button came up: settle what the gesture leaves behind.
fn finish(d: Drag, part: &mut Part, cx: &RollCx, roll: &mut Roll, ui: &Ui, pop_id: Id) {
    match d.kind {
        Kind::Move { idx, last, rows_at_press, moved, .. } => {
            if let Some((_, steps, kept)) = clip_mut(part) {
                if steps.get(idx) == Some(&last) {
                    // a note dropped on another replaces it
                    let key = (last.pitch, last.at, last.nudge);
                    let mut j = 0;
                    steps.retain(|s| {
                        let keep = j == idx || (s.pitch, s.at, s.nudge) != key;
                        j += 1;
                        keep
                    });
                    // a lane emptied by the move stays
                    if moved && cx.lanes {
                        for r in rows_at_press {
                            if !kept.contains(&r) {
                                kept.push(r);
                            }
                        }
                    }
                }
            }
            roll.active = Some((last.pitch, last.at, last.nudge));
            if cx.lanes && moved {
                roll.active_row = Some(last.pitch);
            }
        }
        Kind::Resize { idx, last, .. } => {
            if view_clip(part, cx.key).steps.get(idx) == Some(&last) {
                roll.last_len = last.len.max(1);
            }
        }
        Kind::Lane { from, dragged: false, .. } => {
            let open = ui.memory(|m| m.is_popup_open(pop_id));
            if open && roll.pop_lane == Some(from) {
                ui.memory_mut(|m| m.close_popup());
            } else {
                roll.pop_lane = Some(from);
                ui.memory_mut(|m| m.open_popup(pop_id));
            }
        }
        Kind::Event { pending: Some(v), .. } => {
            if let Play::Chords { vel, .. } = &mut part.play {
                *vel = v;
            }
        }
        _ => {}
    }
}

/// The note colour: grey towards the hand colour with the velocity.
fn note_fill(color: Color32, vel: u8) -> Color32 {
    Color32::from_gray(0x30).lerp_to_gamma(color, 0.30 + 0.70 * f32::from(vel.min(127)) / 127.0)
}

fn is_black(p: u8) -> bool {
    matches!(p % 12, 1 | 3 | 6 | 8 | 10)
}

fn in_key(key: Key, p: u8) -> bool {
    key.scale.steps().contains(&((p % 12 + 12 - key.root % 12) % 12))
}

/// The alpha of the grid line at `step` (ui-redesign R18): bars, beats
/// and steps, the lanes brighter than the piano roll; None: no line.
fn grid_alpha(step: u16, cell_w: f32, lanes: bool) -> Option<f32> {
    match (lanes, step.is_multiple_of(16), step.is_multiple_of(4)) {
        (true, true, _) => Some(0.32),
        (true, false, true) => Some(0.16),
        (true, false, false) => (cell_w >= 6.0).then_some(0.07),
        (false, true, _) => Some(0.20),
        (false, false, true) => Some(if cell_w < 5.0 { 0.06 } else { 0.09 }),
        (false, false, false) => (cell_w >= 8.0).then_some(0.04),
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_roll(ui: &mut Ui, part: &Part, cx: &RollCx, roll: &Roll, g: &Geom, hover: Option<Pos2>, alt: bool, lanes: bool, held: bool) {
    let view = view_clip(part, cx.key);
    let mut steps = view.steps;
    let drag = match &roll.gesture {
        Gesture::On(d) => Some(d.as_ref()),
        Gesture::Idle => None,
    };
    if let (Some(Drag { kind: Kind::Event { pending: Some(v), .. }, .. }), Play::Chords { .. }) = (drag, &part.play) {
        for s in &mut steps {
            s.vel = *v;
        }
    }
    let dragged = drag.and_then(|d| match d.kind {
        Kind::Move { idx, moved: true, .. } | Kind::Resize { idx, .. } => Some(idx),
        _ => None,
    });
    let n = g.n;
    let lane_on = if lanes { active_lane(roll, &g.rows) } else { None };

    // ruler: bar numbers (chord names), beat ticks
    let rp = ui.painter_at(g.ruler);
    for b in 0..n / midi::STEPS {
        let x = g.x_of_tick(f32::from(b * midi::STEPS) * TICKS);
        let label = match &part.play {
            Play::Chords { degrees, size, .. } => format!("{} {}", b + 1, degrees.get(usize::from(b)).map(|d| midi::chord_name(cx.key, *d, *size)).unwrap_or_default()),
            _ => (b + 1).to_string(),
        };
        rp.text(pos2(x + 3.0, g.ruler.top() + 1.0), Align2::LEFT_TOP, label, glass::font(Tok::SMALL), white(Tok::AXIS));
    }
    for s in (0..n).step_by(4) {
        let x = rp.round_to_pixel_center(g.x_of_tick(f32::from(s) * TICKS));
        // lanes: bar ticks the ruler's full height, beat ticks 4 pt
        let (h, a) = match (lanes, s % 16 == 0) {
            (true, true) => (g.ruler.height(), 0.32),
            (true, false) => (4.0, 0.28),
            (false, _) => (3.0, 0.20),
        };
        rp.vline(x, g.ruler.bottom() - h..=g.ruler.bottom(), Stroke::new(1.0_f32, white(a)));
    }

    // grid
    let gp = ui.painter_at(g.grid);
    gp.rect_filled(g.grid, CornerRadius::same(4), glass::veil(Tok::INSET_FILL));
    let bars = n / midi::STEPS;
    for b in (1..bars).step_by(2) {
        let (x0, x1) = (g.x_of_tick(f32::from(b * 16) * TICKS), g.x_of_tick(f32::from((b + 1) * 16) * TICKS));
        gp.rect_filled(Rect::from_x_y_ranges(x0..=x1, g.grid.y_range()), 0.0, glass::veil(if lanes { 0.035 } else { 0.025 }));
    }
    if lanes {
        if let Some(i) = lane_on.and_then(|l| g.row_of(l)) {
            gp.rect_filled(g.row_rect(i, g.grid.x_range()), 0.0, glass::veil(Tok::INSET_FILL));
        }
        for i in 1..g.rows.len() {
            let y = gp.round_to_pixel_center(g.row_top(i));
            gp.hline(g.grid.x_range(), y, Stroke::new(1.0_f32, white(0.10)));
        }
    } else {
        for (i, &p) in g.rows.iter().enumerate() {
            if !in_key(cx.key, p) {
                gp.rect_filled(g.row_rect(i, g.grid.x_range()), 0.0, Color32::from_black_alpha(46));
            }
        }
        for i in 1..g.rows.len() {
            let y = gp.round_to_pixel_center(g.row_top(i));
            let a = if g.rows[i - 1].is_multiple_of(12) { 0.10 } else if g.row_h >= 10.0 { 0.03 } else { continue };
            gp.hline(g.grid.x_range(), y, Stroke::new(1.0_f32, white(a)));
        }
    }
    for s in 0..=n {
        let Some(a) = grid_alpha(s, g.cell_w, lanes) else { continue };
        let x = gp.round_to_pixel_center(g.x_of_tick(f32::from(s) * TICKS));
        gp.vline(x, g.grid.y_range(), Stroke::new(1.0_f32, white(a)));
    }

    // notes
    let idle = drag.is_none();
    let hover_hit = hover.filter(|p| idle && !held && g.grid.contains(*p)).map(|p| (p, hit(g, &steps, p, None)));
    for i in draw_order(&steps, n, dragged) {
        let s = &steps[i];
        let Some(row) = g.row_of(s.pitch) else { continue };
        if held {
            let r = g.row_rect(row, g.grid.x_range()).shrink2(vec2(0.5, 1.0));
            gp.rect_filled(r, CornerRadius::same(Tok::R_NOTE), note_fill(cx.color, s.vel));
            glass::chevron_at(&gp, pos2(g.grid.right() - 6.0, r.center().y), 3.0, 0.0, white(0.5));
            continue;
        }
        let hit_r = g.note_rect(s, row);
        let dx = if g.cell_w < 5.0 { 0.25 } else { 0.5 };
        let r = Rect::from_min_max(hit_r.min + vec2(dx, 1.0), hit_r.max - vec2(dx, 1.0));
        let fill = note_fill(cx.color, s.vel);
        let rat = s.ratchet.clamp(1, 8);
        if rat > 1 {
            let first = g.x_of_tick(start_tick(s) + TICKS).min(r.right());
            let k = f32::from(rat);
            let bw = ((first - r.left() - (k - 1.0)) / k).max(0.5);
            for j in 0..rat {
                let x0 = r.left() + f32::from(j) * (bw + 1.0);
                gp.rect_filled(Rect::from_x_y_ranges(x0..=x0 + bw, r.y_range()), CornerRadius::same(Tok::R_BAR), fill);
            }
            if r.right() > first + 1.0 {
                gp.rect_filled(Rect::from_x_y_ranges(first + 1.0..=r.right(), r.y_range()), CornerRadius::same(Tok::R_BAR), fill.gamma_multiply(0.25));
            }
        } else {
            gp.rect_filled(r, CornerRadius::same(Tok::R_NOTE), fill);
        }
        if !lanes && r.width() >= 30.0 && g.row_h >= 14.0 {
            gp.text(pos2(r.left() + 3.0, r.center().y), Align2::LEFT_CENTER, midi::note_name(s.pitch), glass::font(Tok::SMALL), Color32::from_black_alpha(190));
        }
        let hovered = matches!(hover_hit, Some((_, Hit::Body(h))) if h == i);
        // the dragged and the last touched note in the accent, a hovered one
        // a quiet edge: no white flashes
        let outline = if dragged == Some(i) || roll.active == Some((s.pitch, s.at, s.nudge)) {
            Some(Tok::ACCENT_HI)
        } else if hovered {
            Some(white(0.35))
        } else {
            None
        };
        if let Some(c) = outline {
            gp.rect_stroke(r, CornerRadius::same(Tok::R_NOTE), Stroke::new(1.0_f32, c), StrokeKind::Inside);
        }
        if matches!(hover_hit, Some((_, Hit::Edge(h))) if h == i) {
            gp.rect_filled(Rect::from_x_y_ranges(r.right() - 2.0..=r.right(), r.y_range()), 0.0, Tok::ACCENT_HI);
        }
    }
    // cues and cursor (macOS draws no cursor for plug-ins: the cue is it)
    match hover_hit {
        Some((p, Hit::Empty)) => {
            let row = g.row_at(p.y);
            let t = g.tick_at(p.x).max(0.0);
            let len = roll.last_len.clamp(1, n);
            let start = if alt { t.floor() } else { (t / TICKS).floor() * TICKS }.min(f32::from(n - len) * TICKS);
            let r = g.row_rect(row, (g.x_of_tick(start)..=g.x_of_tick(start + f32::from(len) * TICKS)).into()).shrink2(vec2(0.5, 1.0));
            gp.rect_stroke(r, CornerRadius::same(Tok::R_NOTE), Stroke::new(1.0_f32, white(0.25)), StrokeKind::Inside);
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        Some((_, Hit::Body(_))) => ui.ctx().set_cursor_icon(CursorIcon::Grab),
        Some((_, Hit::Edge(_))) => ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal),
        None => {}
    }
    if let Some(d) = drag {
        match d.kind {
            Kind::Move { moved: true, .. } => ui.ctx().set_cursor_icon(CursorIcon::Grabbing),
            Kind::Resize { .. } => ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal),
            Kind::Lane { .. } | Kind::Event { .. } => ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical),
            _ => {}
        }
    }

    // label column
    let lp = ui.painter_at(g.label);
    let in_label = hover.filter(|p| idle && g.label.contains(*p)).map(|p| g.row_at(p.y));
    if lanes {
        let lane_drag = drag.and_then(|d| match d.kind {
            Kind::Lane { now, dragged: true, .. } => Some(now),
            _ => None,
        });
        for (i, &p) in g.rows.iter().enumerate() {
            let r = g.row_rect(i, g.label.x_range());
            let dragging = lane_drag == Some(p);
            let hov = in_label == Some(i);
            if dragging || hov {
                lp.rect_filled(r.shrink2(vec2(0.0, 1.0)), CornerRadius::same(Tok::R_FIELD), if dragging { glass::accent(0.22) } else { glass::veil(Tok::HOVER) });
            }
            if lane_on == Some(p) && !held {
                lp.rect_filled(Rect::from_min_size(r.min + vec2(0.0, 2.0), vec2(2.0, r.height() - 4.0)), 0.0, glass::sel_bar());
            }
            let name_a = if dragging { Tok::STRONG } else { Tok::VALUE };
            match gm_name(p).filter(|_| cx.drums) {
                Some(gm) => {
                    lp.text(pos2(r.left() + 6.0, r.center().y), Align2::LEFT_CENTER, gm, glass::font(Tok::BODY), white(name_a));
                    lp.text(pos2(r.right() - 14.0, r.center().y), Align2::RIGHT_CENTER, midi::note_name(p), glass::font(Tok::SMALL), white(Tok::LABEL));
                }
                None => {
                    lp.text(pos2(r.left() + 6.0, r.center().y), Align2::LEFT_CENTER, midi::note_name(p), glass::font(Tok::BODY), white(name_a));
                }
            }
            // the up/down glyph: this name drags
            let c = pos2(r.right() - 7.0, r.center().y);
            let st = Stroke::new(1.0_f32, if dragging { Tok::ACCENT_HI } else { white(if hov { Tok::VALUE } else { Tok::HINT }) });
            for dir in [-1.0f32, 1.0] {
                let tip = c + vec2(0.0, dir * 4.0);
                let base = c + vec2(0.0, dir * 1.0);
                lp.line_segment([pos2(c.x - 3.0, base.y), tip], st);
                lp.line_segment([tip, pos2(c.x + 3.0, base.y)], st);
            }
        }
        if in_label.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
        }
    } else {
        for (i, &p) in g.rows.iter().enumerate() {
            let r = g.row_rect(i, g.label.x_range());
            if is_black(p) {
                lp.rect_filled(Rect::from_x_y_ranges(r.left()..=r.left() + BLACK_W, r.y_range()), 0.0, Color32::from_gray(0x14));
                lp.rect_filled(Rect::from_x_y_ranges(r.left() + BLACK_W..=r.right(), r.y_range()), 0.0, white(0.10));
            } else {
                lp.rect_filled(r, 0.0, white(0.10));
            }
            if in_label == Some(i) {
                lp.rect_filled(r, 0.0, glass::veil(Tok::HOVER));
            }
            if matches!(p % 12, 0 | 5) {
                let y = lp.round_to_pixel_center(r.bottom() - 0.5);
                lp.hline(r.x_range(), y, Stroke::new(1.0_f32, Color32::from_black_alpha(102)));
            }
            let root = p % 12 == cx.key.root % 12;
            if p % 12 == 0 || root {
                let a = if root { Tok::LABEL } else { Tok::AXIS };
                lp.text(pos2(r.right() - 3.0, r.center().y), Align2::RIGHT_CENTER, midi::note_name(p), glass::font(Tok::SMALL), white(a));
            }
        }
        if in_label.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
    }

    // event lane: velocity bars or roll blocks
    if let Some(e) = g.event {
        let ep = ui.painter_at(e);
        ep.rect_filled(e, CornerRadius::same(4), glass::veil(Tok::INSET_FILL));
        let w = (g.cell_w * 0.5).clamp(3.0, 7.0);
        let bottom = e.bottom() - 2.0;
        // the bars in the accent: a solid fill with a light cap reads on
        // the dark lane at any height
        for i in event_steps(&steps, n, lane_on) {
            let s = &steps[i];
            let x = g.x_of_tick(start_tick(s));
            match roll.lane {
                EventLane::Velocity => {
                    let h = (EVENT_SPAN * f32::from(s.vel.min(127)) / 127.0).max(2.0);
                    let bar = Rect::from_min_max(pos2(x, bottom - h), pos2(x + w, bottom));
                    ep.rect_filled(bar, CornerRadius::same(Tok::R_BAR), Tok::ACCENT_FILL);
                    ep.rect_filled(Rect::from_min_max(bar.min, pos2(bar.right(), bar.top() + 2.0)), CornerRadius::same(Tok::R_BAR), Tok::ACCENT_HI);
                }
                EventLane::Rolls => {
                    let bh = (EVENT_SPAN - 7.0) / 8.0;
                    let k = s.ratchet.clamp(1, 8);
                    for j in 0..k {
                        let y1 = bottom - f32::from(j) * (bh + 1.0);
                        ep.rect_filled(Rect::from_min_max(pos2(x, y1 - bh), pos2(x + w, y1)), 0.0, if j + 1 == k { Tok::ACCENT_HI } else { Tok::ACCENT_FILL });
                    }
                }
            }
        }
        if let Some(Drag { kind: Kind::Event { shown: Some((p, text)), .. }, .. }) = drag {
            ui.painter().text(*p + vec2(8.0, -8.0), Align2::LEFT_BOTTOM, text, glass::font(Tok::SMALL), white(Tok::STRONG));
        }
        if idle && hover.is_some_and(|p| e.contains(p)) {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
        }
    }

    // playhead
    if let Some(ph) = cx.playhead {
        let s = ph.rem_euclid(f32::from(n));
        let x = ui.painter().round_to_pixel_center(g.x_of_tick(s * TICKS));
        let bottom = g.event.map_or(g.grid.bottom(), |e| e.bottom());
        let p = ui.painter_at(g.block);
        p.vline(x, g.ruler.top()..=bottom, Stroke::new(1.0_f32, white(0.70)));
        let yb = g.ruler.bottom();
        p.add(egui::Shape::convex_polygon(vec![pos2(x - 2.5, yb - 4.0), pos2(x + 2.5, yb - 4.0), pos2(x, yb)], white(0.80), Stroke::NONE));
    }
}

// --- the window section -----------------------------------------------------------

/// A field whose value is written only when the drag ends (or on a
/// double-click): a chord or held note restarts whenever it changes.
/// (shown value, value to write).
#[allow(clippy::too_many_arguments)]
fn deferred_field(ui: &mut Ui, id: Id, now: u8, range: std::ops::RangeInclusive<i32>, default: i32, label: &str, w: f32, fmt: impl Fn(i32) -> String) -> (u8, Option<u8>) {
    let key = id.with("pending");
    let mut v = ui.data(|d| d.get_temp::<i32>(key)).unwrap_or(i32::from(now));
    let r = glass::slider_field(ui, id, &mut v, range, default, label, w, fmt);
    let v8 = u8::try_from(v).unwrap_or(now);
    if r.dragged() {
        ui.data_mut(|d| d.insert_temp(key, v));
        (v8, None)
    } else {
        ui.data_mut(|d| d.remove::<i32>(key));
        (v8, (v8 != now).then_some(v8))
    }
}

/// A hint under what it explains: 13 at HINT (R17).
fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(Tok::BODY).color(white(Tok::HINT)));
}

/// The inset two-line rule between the hands and what is under them.
fn rule(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 2.0), Sense::hover());
    glass::section_rule(ui.painter(), r.x_range(), r.top());
}

/// The width `glass::segmented` takes for `opts`.
fn seg_width<T>(ui: &Ui, opts: &[(T, &str)]) -> f32 {
    opts.iter().map(|(_, s)| (glass::galley(ui, s, Tok::BODY).size().x + 20.0).max(44.0)).sum()
}

/// The channel and start dropdowns of the editor row.
const CHANNEL_W: f32 = 180.0;
const START_W: f32 = 124.0;
/// The part picker, the reserved Back slot and the part name field.
const PICKER_W: f32 = 168.0;
const BACK_W: f32 = 160.0;
const NAME_W: f32 = 150.0;

/// "Middle alone", "Fist".
fn pose_name(mask: u8, t: &Text) -> String {
    match (0..5).find(|f| finger_bit(*f) == mask) {
        _ if mask == 0 => capital(t.f_pose_fist),
        _ if mask == 31 => capital(t.f_pose_open),
        Some(f) => capital(&fill(t.f_pose_alone, &[&t.fingers()[f]])),
        None => mask_name(mask, t),
    }
}

/// The lines after a calibration: done, one line per pose the camera
/// reads as another (both hands on one line), then the fingers that stay
/// hard to tell apart and are not said in a misread line already.
fn cal_lines(hard: &[(usize, usize)], misread: &[(usize, u8, u8)], t: &Text) -> Vec<String> {
    let hand = |s: usize| ["L", "R"][s.min(1)];
    let mut lines = vec![t.f_cal_done.to_string()];
    let mut poses: Vec<(u8, u8, Vec<usize>)> = Vec::new();
    for (side, pose, read) in misread {
        match poses.iter_mut().find(|(p, r, _)| p == pose && r == read) {
            Some(e) => e.2.push(*side),
            None => poses.push((*pose, *read, vec![*side])),
        }
    }
    // whole finger names here: the line is a sentence, not a chip
    let full = t.fingers_full();
    for (pose, read, sides) in &poses {
        let read = match read & 31 {
            0 => t.f_pose_fist.to_string(),
            31 => t.f_pose_open.to_string(),
            m => (0..5).filter(|f| m & finger_bit(*f) != 0).filter_map(|f| full.get(f).copied()).collect::<Vec<_>>().join(", "),
        };
        let shown = match (0..5).find(|f| finger_bit(*f) == *pose) {
            Some(f) => capital(&fill(t.f_cal_alone, &[&full.get(f).copied().unwrap_or_default()])),
            None => pose_name(*pose, t),
        };
        let sides: Vec<&str> = sides.iter().map(|s| hand(*s)).collect();
        lines.push(format!("{} ({})", fill(t.f_cal_misread, &[&shown, &read]), sides.join(", ")));
    }
    let rest: Vec<String> = hard
        .iter()
        .filter(|(s, f)| !misread.iter().any(|(ms, pose, _)| ms == s && *pose == finger_bit(*f)))
        .map(|(s, f)| format!("{} {}", hand(*s), t.fingers()[(*f).min(4)]))
        .collect();
    if !rest.is_empty() {
        lines.push(capital(&fill(t.f_cal_hard, &[&rest.join(", ")])));
    }
    lines
}

/// The part an owner without one picked: its name and play, and for a
/// saved part its mode and start.
struct Seed {
    name: String,
    play: Play,
    saved: Option<(Mode, Start)>,
}

impl Seed {
    /// Adds the part on `channel` (drum notes fitted to it); its id.
    fn add(self, store: &mut Store, channel: Option<Channel>) -> u64 {
        let mut play = self.play;
        if let Some(c) = &channel {
            midi::fit_to_channel(&mut play, &c.name);
        }
        let id = store.add(&self.name, play, channel);
        if let (Some((mode, start)), Some(p)) = (self.saved, store.get_mut(id)) {
            p.mode = mode;
            p.start = start;
        }
        id
    }
}

/// What the part picker picked.
enum PartPick {
    Template(&'static str),
    Empty,
    Saved(String),
    Delete(String),
}

impl Fingers {
    /// Called when the window opens again: the egui context is new.
    pub fn reset_textures(&mut self) {
        self.tex = [None, None];
        self.roll.gesture = Gesture::Idle;
    }

    /// Selects `slot` (the window captures, editor.rs `apply_shot`).
    pub fn shot_select(&mut self, slot: Slot) {
        self.sel = Some(slot);
    }

    /// Selects `slot` with the Parameter kind shown (the window captures).
    pub fn shot_params(&mut self, slot: Slot) {
        self.sel = Some(slot);
        self.params = true;
    }

    /// Picks hand `side` (the window captures): the editor lists what it
    /// drives.
    pub fn shot_hand(&mut self, side: usize) {
        self.hand_sel = Some(side);
        self.sel = None;
    }

    /// The Routes view with the first kept target box's search open (the
    /// window captures, shot u).
    pub fn shot_routes_find(&mut self) {
        self.view = View::Routes;
        self.route.manual = true;
        self.route_edit.shot_find = true;
    }

    /// While a calibration runs the section header stays open.
    pub fn calibrating(&self) -> bool {
        self.calib.is_some()
    }

    /// True once after a click on a target node of the Routes view.
    pub fn take_unfold(&mut self) -> bool {
        std::mem::take(&mut self.unfold)
    }

    /// The kept route whose `x` in a drives list confirmed, once: the
    /// window unmaps it (Sentence::unmap_kept).
    pub fn take_unmap(&mut self) -> Option<(String, NodeTarget)> {
        self.unmap.take().or_else(|| self.route_edit.unmap.pop())
    }

    /// Kept routes a box of the Routes view retargeted, once: the window
    /// changes them (Sentence::retarget_kept).
    pub fn take_retarget(&mut self) -> Vec<(String, NodeTarget, crate::param_find::Found)> {
        std::mem::take(&mut self.route_edit.retarget)
    }

    fn select(&mut self, s: Slot, ctx: &egui::Context) {
        if self.sel != Some(s) {
            self.params = false;
            self.undo = None;
            self.roll.active_row = None;
            self.roll.pop_lane = None;
            // another part opens: its rows fit it, its tools start afresh
            self.roll.top = None;
            self.tools_h = None;
            // the old part's lane or row popover would stay open unseen
            ctx.memory_mut(|m| m.close_popup());
        }
        self.sel = Some(s);
        self.hand_sel = None;
        if self.view != View::Single {
            self.route.manual = true;
        }
        self.view = View::Single;
        // the editor starts below the fold at these heights (2.8)
        self.scroll_owner = true;
    }

    /// Picks hand `side` (its node): the editor lists what it drives.
    fn select_hand(&mut self, side: usize, ctx: &egui::Context) {
        if self.hand_sel != Some(side) {
            self.undo = None;
            self.roll.active_row = None;
            self.roll.pop_lane = None;
            ctx.memory_mut(|m| m.close_popup());
        }
        self.hand_sel = Some(side);
        self.sel = None;
        if self.view != View::Single {
            self.route.manual = true;
        }
        self.view = View::Single;
        self.scroll_owner = true;
    }

    /// Reset: stops a running calibration; true when the `stored`
    /// thresholds go too.
    fn reset(&mut self, stored: bool) -> bool {
        self.cancel_calibration();
        stored
    }

    /// Stops a running calibration: its progress line and the dimmed
    /// Calibrate button go at once, the thresholds stay as they were.
    fn cancel_calibration(&mut self) {
        if let Some(c) = self.calib.take() {
            c.cancel();
        }
        self.cal_still = None;
    }

    /// The controls in the section header's slot, all `CTRL_H` high on one
    /// centre line and `GAP` apart: Single | All | Routes on the left;
    /// [root] [scale], Calibrate fingers and Reset (only while a
    /// calibration exists) on the right. The scale field takes what its
    /// value needs, at most 150, and shrinks (its text cut with "…") so the
    /// two groups never meet.
    pub fn header(&mut self, ui: &mut Ui, slot: Rect, writer: &Writer, t: &Text) {
        let stored = crate::settings::store().get().fingers.is_some();
        let reset = stored || self.calib.is_some();
        let before = writer.player.store().key;
        let widths = HeadWidths {
            seg: [t.f_single, t.f_total, t.f_routes].iter().map(|s| (glass::galley(ui, s, Tok::BODY).size().x + 20.0).max(44.0)).sum(),
            buttons: [Some(t.f_calibrate), reset.then_some(t.f_cal_reset)].into_iter().flatten().map(|s| glass::galley(ui, s, Tok::BODY).size().x + 16.0 + Tok::GAP).sum(),
            scale: glass::galley(ui, &capital(before.scale.name()), Tok::BODY).size().x,
        };
        let scale_w = widths.scale_w(slot.width());
        let mut c = ui.new_child(UiBuilder::new().max_rect(slot).layout(Layout::left_to_right(Align::Center)));
        c.spacing_mut().item_spacing.x = Tok::GAP;
        let opts = [(View::Single, t.f_single), (View::Total, t.f_total), (View::Routes, t.f_routes)];
        let r = glass::segmented(&mut c, &mut self.view, &opts);
        if r.changed() {
            self.route.manual = true;
        }
        if let Some(p) = r.hover_pos() {
            let mut x = r.rect.left();
            for ((_, s), h) in opts.iter().zip([t.f_single_hover, t.f_all_hover, t.f_routes_hover]) {
                x += (glass::galley(&c, s, Tok::BODY).size().x + 20.0).max(44.0);
                if p.x < x {
                    r.on_hover_text(h);
                    break;
                }
            }
        }
        let mut key = before;
        c.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Reset also stops a running calibration
            if reset && glass::quiet_dim(ui, t.f_cal_reset).clicked() && self.reset(stored) {
                crate::settings::store().update(|s| s.fingers = None);
                crate::engine::set_finger_thresholds(None);
            }
            // dim while a run is going, so the header keeps its place
            if glass::dim_button(ui, t.f_calibrate, self.calib.is_none(), t.f_cal_running).clicked() {
                self.calib = Some(crate::calib::Calib::default());
            }
            key_picker(ui, &mut key, scale_w);
        });
        if key != before {
            let mut st = writer.player.store();
            st.key = key;
            writer.update_midi(st);
        }
    }

    /// The hands view and, under it, the part editor (Single), all parts
    /// (All) or the node view of `routing` (Routes).
    pub fn section(&mut self, ui: &mut Ui, writer: &Writer, t: &Text, de: bool, routing: &Routing) {
        let (channels, linked, focus_ch) = fl_view();
        self.body(ui, writer, t, de, routing, &channels, linked, focus_ch);
    }

    /// The part editor for a Pong wall: what a hit on `side` plays. A wall
    /// without a part shows the kinds to start one; the part binds no
    /// finger, it is the wall's (Store.walls, Action::Part).
    pub fn wall_editor(&mut self, ui: &mut Ui, writer: &Writer, side: Side, t: &Text, de: bool) {
        let (channels, linked, focus_ch) = fl_view();
        self.wall_body(ui, writer, side, t, de, &channels, linked, focus_ch);
    }

    /// The wall's pane in place of the finger's, or back.
    fn swap_wall(&mut self) {
        let w = &mut self.wall;
        std::mem::swap(&mut self.roll, &mut w.roll);
        std::mem::swap(&mut self.undo, &mut w.undo);
        std::mem::swap(&mut self.tools_h, &mut w.tools_h);
        std::mem::swap(&mut self.piano, &mut w.piano);
        std::mem::swap(&mut self.roll_open, &mut w.roll_open);
        std::mem::swap(&mut self.scroll_owner, &mut w.scroll_owner);
    }

    /// Another wall opens in the wall editor (the Pong section calls it):
    /// its rows fit it, its tools start afresh, the old part's lane
    /// popover closes (else it stays open unseen and both rolls ignore
    /// every press on their grids), and its owner row scrolls to the top.
    pub fn wall_switched(&mut self, ctx: &egui::Context) {
        let w = &mut self.wall;
        (w.undo, w.tools_h) = (None, None);
        (w.roll.active_row, w.roll.pop_lane, w.roll.top) = (None, None, None);
        w.scroll_owner = true;
        ctx.memory_mut(|m| m.close_popup());
    }

    /// A wall row the total view had clicked since the last call.
    pub fn take_wall_pick(&mut self) -> Option<Side> {
        self.wall_pick.take()
    }

    /// The wall editor with FL's channels given (the tests run it without FL).
    #[allow(clippy::too_many_arguments)]
    fn wall_body(&mut self, ui: &mut Ui, writer: &Writer, side: Side, t: &Text, de: bool, channels: &[fl::ChannelRow], linked: bool, focus_ch: Option<u32>) {
        let mut store = writer.player.store();
        self.sync_state(writer, &store);
        let before = store.clone();
        let playing = writer.player.playing();
        let live = Live { on: &[], playing: &playing, player: &writer.player, channels, linked, focus_ch };
        let setup = writer.pong.setup();
        let id = match setup.wall(side) {
            Action::Part { id } if store.get(*id).is_some() => *id,
            _ => {
                let Some(seed) = self.empty_owner(ui, store.key, Owner::Wall(side), 0, t.fw_note, t, de) else { return };
                let ch = focus_channel(&live);
                let from_file = seed.saved.is_some();
                let id = seed.add(&mut store, ch.clone());
                if let Some(p) = store.get_mut(id).filter(|_| !from_file) {
                    // a hit plays at once unless the part says otherwise
                    p.start = Start::Now;
                }
                store.walls.push(id);
                writer.update_midi(store);
                let mut s = (*setup).clone();
                s.walls[side.index()] = Action::Part { id };
                if ch.is_none() {
                    // no channel list yet: the part gets one when it comes
                    s.auto_channel.push(id);
                }
                writer.pong.set_setup(s);
                return;
            }
        };
        self.swap_wall();
        let (remove, geom) = self.part_editor(ui, &mut store, id, Owner::Wall(side), &live, t, de);
        self.swap_wall();
        self.geom = geom;
        if remove {
            let mut s = (*setup).clone();
            s.walls[side.index()] = Action::Off;
            s.auto_channel.retain(|p| *p != id);
            writer.pong.set_setup(s);
            // another wall or a finger may play the same part
            let others = writer.pong.setup().wall_parts();
            if !others.contains(&id) && !store.binds.iter().any(|b| b.part == id) {
                store.parts.retain(|p| p.id != id);
            }
            store.walls.retain(|w| *w != id);
            writer.update_midi(store);
            return;
        }
        if store != before {
            writer.update_midi(store);
        }
        if !playing.is_empty() || !matches!(self.wall.roll.gesture, Gesture::Idle) {
            ui.ctx().request_repaint();
        }
    }

    /// A preset or a project load changed the parts under the editor: the
    /// undo slot, a gesture, the fitted rows and the tools' height belong
    /// to the old ones, and so does a selected finger whose part is gone.
    fn sync_state(&mut self, writer: &Writer, store: &Store) {
        let g = writer.state_gen();
        if self.seen_gen.replace(g).is_none_or(|old| old == g) {
            return;
        }
        self.undo = None;
        self.roll.forget();
        self.wall.roll.forget();
        self.tools_h = None;
        self.wall.undo = None;
        self.wall.tools_h = None;
        if self.sel.is_some_and(|s| store.bound(&s.signal(), s.trigger()).is_none()) {
            self.sel = None;
        }
    }

    /// The body with FL's channels given (the tests run it without FL).
    #[allow(clippy::too_many_arguments)]
    fn body(&mut self, ui: &mut Ui, writer: &Writer, t: &Text, de: bool, routing: &Routing, channels: &[fl::ChannelRow], linked: bool, focus_ch: Option<u32>) {
        let mut store = writer.player.store();
        self.sync_state(writer, &store);
        let before = store.clone();
        let on = writer.binds_on();
        let playing = writer.player.playing();
        let live = Live { on: &on, playing: &playing, player: &writer.player, channels, linked, focus_ch };
        self.view = self.route.step(self.view, routing);
        self.routes_batch = routing.batch;
        self.edges.clone_from(&routing.edges);

        let target = self.calibrate(ui, t);
        self.hands(ui, &store, &live, target, t);
        rule(ui);
        match self.view {
            View::Single => match (self.hand_sel, self.sel) {
                (Some(side), _) => self.hand_params(ui, side, t),
                (None, Some(slot)) => self.single(ui, &mut store, slot, &live, t, de),
                (None, None) => hint(ui, t.f_pick_part),
            },
            View::Total => {
                let walls: Vec<(Side, u64)> = writer.pong.setup().walls.iter().zip(Side::ALL).filter_map(|(a, s)| if let Action::Part { id } = a { Some((s, *id)) } else { None }).collect();
                self.total(ui, &store, &walls, &live, t, de)
            }
            View::Routes => self.routes(ui, &store, routing, writer, t),
        }
        if store != before {
            writer.update_midi(store);
        }
        if !playing.is_empty() || !matches!(self.roll.gesture, Gesture::Idle) {
            ui.ctx().request_repaint();
        }
    }

    /// Runs the finger calibration while one is going; the pose to show.
    fn calibrate(&mut self, ui: &mut Ui, t: &Text) -> Option<u8> {
        let values = *crate::engine::FINGER_VALUES.lock();
        let old = *crate::engine::FINGER_THR.lock();
        let tick = self.calib.as_mut().map(|c| c.tick(values, old).ok_or_else(|| c.now()));
        match tick {
            Some(Ok(out)) => {
                self.calib = None;
                self.cal_still = None;
                let lines = if out.seen.iter().any(|s| *s) {
                    crate::settings::store().update(|s| s.fingers = Some(out.thresholds));
                    crate::engine::set_finger_thresholds(Some(out.thresholds));
                    cal_lines(&out.hard, &out.misread, t)
                } else {
                    vec![t.f_cal_none.to_string()]
                };
                self.cal_msg = Some((lines, std::time::Instant::now()));
            }
            Some(Err((pose, p))) => {
                let mask = crate::calib::POSES.get(pose).copied().unwrap_or(0);
                let name = match pose {
                    0 => t.f_pose_fist.to_string(),
                    1 => t.f_pose_open.to_string(),
                    k => fill(t.f_pose_alone, &[&t.fingers()[(k - 2).min(4)]]),
                };
                let frac = (pose as f32 + p) / crate::calib::POSES.len() as f32;
                // the bar stands still while no hand gives frames
                let now = std::time::Instant::now();
                let since = match self.cal_still {
                    Some((f, at)) if (f - frac).abs() < 1e-4 => at,
                    _ => {
                        self.cal_still = Some((frac, now));
                        now
                    }
                };
                let fresh = values.iter().flatten().map(|v| v.1).max().is_some_and(|ts| midi::now_ns().saturating_sub(ts) < 1_000_000_000);
                let no_hand = now.duration_since(since).as_secs_f32() > 1.0 && !fresh;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(fill(t.f_cal_show, &[&name])).size(Tok::TITLE).color(white(Tok::STRONG)));
                    glass::progress(ui, frac, 200.0);
                    if no_hand {
                        ui.label(RichText::new(t.f_cal_no_hand).size(Tok::BODY).color(white(Tok::HINT)));
                    }
                });
                ui.ctx().request_repaint();
                return Some(mask);
            }
            None => {}
        }
        if let Some((lines, at)) = &self.cal_msg {
            // a misread pose matters in FL: its line stays longer
            let keep = if lines.len() > 1 { 20 } else { 8 };
            if at.elapsed().as_secs() < keep {
                for l in lines {
                    ui.label(RichText::new(l.as_str()).size(Tok::BODY).color(white(Tok::VALUE)));
                }
            }
        }
        None
    }

    fn is_on(live: &Live, slot: Slot) -> bool {
        live.on.iter().any(|(s, tr)| *s == slot.signal() && *tr == slot.trigger())
    }

    /// The two turned dither hands, a label node beside each bound finger
    /// with a line to its tip, and under each hand its name with the
    /// Fist and Open hand chips. `target`: during the calibration, the
    /// pose to show (lit fingers).
    fn hands(&mut self, ui: &mut Ui, store: &Store, live: &Live, target: Option<u8>, t: &Text) {
        let art = self.art.take().unwrap_or_else(build_art);
        let size = CANVAS as f32 * DOT;
        let (view, _) = ui.allocate_exact_size(vec2(ui.available_width(), HANDS_H), Sense::hover());
        let cx = view.center().x;
        // a narrow window moves the hands together so the outer nodes keep room
        let dx = (view.width() / 2.0 - size / 2.0 - NODE_ROOM).clamp(size / 2.0 + 2.0, HAND_DX);
        let canvases = [-dx, dx].map(|dx| Rect::from_center_size(pos2(cx + dx, view.top() + size / 2.0), vec2(size, size)));
        self.canvas = canvases;
        // a hand-level route: the node in the corner beside the wrist, the
        // finger nodes above it
        let hand_nodes: [Option<(HandNode, String, bool)>; 2] = [0, 1].map(|side| {
            let routes = edges_of(&self.edges, Src::Hand(side));
            let text = routes_text(&routes, t)?;
            let w = glass::galley(ui, &text, Tok::BODY).size().x + 16.0;
            Some((hand_node(canvases[side], side == 0, &art, w, view.x_range()), text, any_pending(&routes)))
        });
        self.hand_nodes = [0, 1].map(|s| hand_nodes[s].as_ref().map(|(n, text, _)| (n.clone(), text.clone())));
        let floors = [0, 1].map(|s| hand_nodes[s].as_ref().map_or(canvases[s].bottom(), |(n, _, _)| n.floor()));
        let mut labels = self.hand_labels(ui, store, canvases, &art, view.x_range(), floors, t);
        // the clicks first, so a selection shows in the frame of its click
        let mut clicked = None;
        let mut clicked_hand = None;
        for side in 0..2 {
            let rect = canvases[side];
            let resp = ui.interact(rect, ui.id().with(("hand", side)), Sense::click());
            if let Some((x, y)) = resp.hover_pos().and_then(|p| dot_at(rect, p, side == 0)) {
                let r = art.region.get(y * CANVAS + x).copied().unwrap_or(0);
                if (2..=6).contains(&r) {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    if resp.clicked() {
                        clicked = Some(Slot::finger(side, usize::from(r - 2)));
                    }
                }
            }
            for (l, _, _, _) in &labels[side] {
                let r = ui.interact(l.node, ui.id().with(("node", side, l.f)), Sense::click());
                if r.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if r.clicked() {
                    clicked = Some(Slot::finger(side, l.f));
                }
            }
            if let Some((h, _, _)) = &hand_nodes[side] {
                let r = ui.interact(h.node, ui.id().with(("hand-node", side)), Sense::click());
                if r.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if r.clicked() {
                    clicked_hand = Some(side);
                }
            }
            if let Some(s) = self.shape_chips(ui, store, canvases[side], view, side, t) {
                clicked = Some(s);
            }
        }
        if let Some(s) = clicked {
            let was = self.sel;
            self.select(s, ui.ctx());
            // a finger that only drives parameters opens on them
            if was != Some(s) {
                let src = s.finger_index().map(|f| Src::Finger(s.side, f));
                self.params = store.bound(&s.signal(), s.trigger()).is_none() && src.is_some_and(|src| !edges_of(&self.edges, src).is_empty());
            }
        } else if let Some(side) = clicked_hand {
            self.select_hand(side, ui.ctx());
        }
        if clicked.is_some() || clicked_hand.is_some() {
            // a finger without a part gets its "Empty" node now
            labels = self.hand_labels(ui, store, canvases, &art, view.x_range(), floors, t);
        }
        let now = midi::now_ns();
        let masks = *crate::engine::MASKS.lock();
        for side in 0..2 {
            let rect = canvases[side];
            let mirror = side == 0;
            // the fingers the camera counts as shown light up, the folded
            // ones go dim, so what fires is visible (finger_colour)
            let seen = masks[side].filter(|(_, at)| now.saturating_sub(*at) < 400_000_000).map(|m| m.0);
            let none = Color32::TRANSPARENT;
            let mut col = [(none, none); 8];
            col[PALM as usize] = (white(if seen.is_some() { 0.62 } else { 0.4 }), none);
            col[TRAIL as usize] = (white(0.42), none);
            // the fingers of a whole shape that plays now (care = all five)
            let sig = Slot::finger(side, 0).signal();
            let shape_bits = live.on.iter().fold(0u8, |m, (s, tr)| match tr {
                Trigger::Shape { mask, care: 31 } if *s == sig => m | mask,
                _ => m,
            });
            let shape_on = live.on.iter().any(|(s, tr)| *s == sig && matches!(tr, Trigger::Shape { care: 31, .. }));
            for f in 0..5 {
                let slot = Slot::finger(side, f);
                let bound = store.bound(&slot.signal(), slot.trigger()).is_some();
                let playing = Self::is_on(live, slot) || shape_bits & finger_bit(f) != 0;
                col[f + 2] = finger_colour(side, f, self.sel == Some(slot), bound, playing, seen.map(|m| m & finger_bit(f) != 0), target);
                self.cols[side][f] = col[f + 2];
            }
            if shape_on {
                col[PALM as usize].0 = HAND[side];
            }
            // the lines under the art, so its dots cover them; a route
            // that waits for Keep draws dashed
            let p = ui.painter();
            let line = Stroke::new(1.0_f32, HAND[side].gamma_multiply(0.55));
            let ppp = p.pixels_per_point();
            let poly = |pts: &[Pos2], dashed: bool| {
                let pts: Vec<Pos2> = pts.iter().map(|q| q.round_to_pixel_center(ppp)).collect();
                if dashed {
                    p.extend(egui::Shape::dashed_line(&pts, line, DASH, DASH_GAP));
                } else {
                    for w in pts.windows(2) {
                        p.line_segment([w[0], w[1]], line);
                    }
                }
            };
            for (l, _, _, dashed) in &labels[side] {
                poly(&l.line, *dashed);
            }
            if let Some((h, _, pending)) = &hand_nodes[side] {
                poly(&h.line, *pending);
            }
            // the contour of a hand that drives something
            let contour = hand_nodes[side].as_ref().map(|(_, _, pending)| {
                let a = if self.hand_sel == Some(side) { 0.9 } else { 0.6 };
                (HAND[side].gamma_multiply(a), *pending)
            });
            let mut img = ColorImage::new([CANVAS, CANVAS], none);
            for y in 0..CANVAS {
                for x in 0..CANVAS {
                    let i = y * CANVAS + if mirror { CANVAS - 1 - x } else { x };
                    let region = art.region.get(i).copied().unwrap_or(0);
                    let (lit, unlit) = col.get(usize::from(region)).copied().unwrap_or((none, none));
                    if art.lit.get(i).copied().unwrap_or(false) {
                        img.pixels[y * CANVAS + x] = lit;
                    } else if (2..=6).contains(&region) {
                        img.pixels[y * CANVAS + x] = unlit;
                    } else if let Some((c, pending)) = contour {
                        let o = art.outline.get(i).copied().unwrap_or(0);
                        if o == 1 || (o == 2 && !pending) {
                            img.pixels[y * CANVAS + x] = c;
                        }
                    }
                }
            }
            let tex = self.tex[side].get_or_insert_with(|| ui.ctx().load_texture(format!("hand{side}"), img.clone(), TextureOptions::NEAREST));
            tex.set(img, TextureOptions::NEAREST);
            p.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            // the nodes, and a dot on each labelled tip (white when selected)
            if let Some((h, text, pending)) = &hand_nodes[side] {
                let sel = self.hand_sel == Some(side);
                let cr = CornerRadius::same(Tok::R_TOGGLE);
                p.rect_filled(h.node, cr, if sel { HAND[side].gamma_multiply(0.25) } else { black(0.60) });
                p.rect_stroke(h.node, cr, Stroke::new(1.0_f32, HAND[side].gamma_multiply(if *pending { 0.6 } else { 1.0 })), StrokeKind::Inside);
                let g = glass::galley(ui, &glass::fit(ui, text, Tok::BODY, (h.node.width() - 16.0).max(0.0)), Tok::BODY);
                glass::text_at(p, h.node.center() - g.size() / 2.0, g, white(if sel { Tok::STRONG } else { Tok::VALUE }));
            }
            for (l, text, empty, _) in &labels[side] {
                let sel = self.sel == Some(Slot::finger(side, l.f));
                let cr = CornerRadius::same(Tok::R_TOGGLE);
                p.rect_filled(l.node, cr, if sel { HAND[side].gamma_multiply(0.25) } else { black(0.60) });
                p.rect_stroke(l.node, cr, Stroke::new(1.0_f32, HAND[side]), StrokeKind::Inside);
                let a = if *empty {
                    Tok::HINT
                } else if sel {
                    Tok::STRONG
                } else {
                    Tok::VALUE
                };
                let g = glass::galley(ui, &glass::fit(ui, text, Tok::BODY, (l.node.width() - 16.0).max(0.0)), Tok::BODY);
                glass::text_at(p, l.node.center() - g.size() / 2.0, g, white(a));
                p.circle_filled(l.tip, 1.0, if sel { Color32::WHITE } else { HAND[side] });
            }
        }
        self.art = Some(art);
    }

    /// Each hand's label nodes with their text: a bound finger's part,
    /// else what its routes drive ("Serum 2 · Cutoff", "2 parameters";
    /// dashed while one waits for Keep), else "Empty" (true) for the
    /// selected finger. The outer nodes end above `floors` (the hand
    /// nodes).
    #[allow(clippy::too_many_arguments)]
    fn hand_labels(&self, ui: &Ui, store: &Store, canvases: [Rect; 2], art: &Art, clip: egui::Rangef, floors: [f32; 2], t: &Text) -> [Vec<(Label, String, bool, bool)>; 2] {
        let mut labels: [Vec<(Label, String, bool, bool)>; 2] = [Vec::new(), Vec::new()];
        for (side, out) in labels.iter_mut().enumerate() {
            let mut texts: [Option<(String, bool, bool)>; 5] = Default::default();
            let mut widths = [None; 5];
            for f in 0..5 {
                let slot = Slot::finger(side, f);
                let routes = edges_of(&self.edges, Src::Finger(side, f));
                texts[f] = match (store.bound(&slot.signal(), slot.trigger()), routes_text(&routes, t)) {
                    (Some(p), _) => Some((summary(p, store.key), false, false)),
                    (None, Some(s)) => Some((s, false, any_pending(&routes))),
                    (None, None) if self.sel == Some(slot) => Some((t.f_empty.to_string(), true, false)),
                    (None, None) => None,
                };
                widths[f] = texts[f].as_ref().map(|(s, _, _)| glass::galley(ui, s, Tok::BODY).size().x + 16.0);
            }
            *out = label_layout_above(canvases[side], side == 0, art, widths, clip, floors[side]).into_iter().filter_map(|l| texts[l.f].clone().map(|(s, e, d)| (l, s, e, d))).collect();
        }
        labels
    }

    /// The row under one hand's canvas, centred on it: the hand's name,
    /// Fist, Open hand and, when an old project binds other shapes on this
    /// hand, "+N older shapes" with a list of them. The shape clicked.
    fn shape_chips(&self, ui: &mut Ui, store: &Store, canvas: Rect, view: Rect, side: usize, t: &Text) -> Option<Slot> {
        // text in the hand's colour: its text tone
        let hand = HAND_TEXT[side.min(1)];
        let mut older: Vec<u8> = store.binds.iter().filter_map(Slot::from_bind).filter(|s| s.side == side && s.care == 31 && s.mask != 0 && s.mask != 31).map(|s| s.mask).collect();
        older.sort_unstable();
        older.dedup();
        let more = (!older.is_empty()).then(|| fill(t.f_older_shapes, &[&older.len()]));
        let name = [t.hand_left, t.hand_right][side.min(1)];
        let chip_w = |s: &str| glass::galley(ui, s, Tok::BODY).size().x + 16.0;
        let name_w = glass::galley(ui, name, Tok::BODY).size().x;
        let mut w = name_w + 2.0 * Tok::GAP + chip_w(t.f_fist) + chip_w(t.f_open);
        if let Some(m) = &more {
            w += Tok::GAP + chip_w(m);
        }
        let row = Rect::from_min_size(pos2(canvas.center().x - w / 2.0, view.bottom() - Tok::CTRL_H), vec2(w + 1.0, Tok::CTRL_H));
        let mut c = ui.new_child(UiBuilder::new().max_rect(row).layout(Layout::left_to_right(Align::Center)));
        c.spacing_mut().item_spacing.x = Tok::GAP;
        let (r, _) = c.allocate_exact_size(vec2(name_w, Tok::CTRL_H), Sense::hover());
        let g = glass::galley(&c, name, Tok::BODY);
        glass::text_at(c.painter(), pos2(r.left(), r.center().y - g.size().y / 2.0), g, hand.gamma_multiply(0.8));
        let mut pick = None;
        for (mask, label) in [(0b00000, t.f_fist), (0b11111, t.f_open)] {
            let s = Slot::shape(side, mask);
            let bound = store.bound(&s.signal(), s.trigger()).is_some();
            if glass::chip(&mut c, self.sel == Some(s), label, bound.then_some(hand)).on_hover_text(t.f_shape_hover).clicked() {
                pick = Some(s);
            }
        }
        if let Some(m) = more {
            let sel = self.sel.is_some_and(|s| s.side == side && s.care == 31 && older.contains(&s.mask));
            let r = glass::chip(&mut c, sel, &m, Some(hand));
            let id = Id::new(("older-shapes", side));
            let chosen = glass::picker_popup(&c, id, &r, 420.0, |ui| {
                let mut chosen = None;
                for mask in &older {
                    let s = Slot::shape(side, *mask);
                    if glass::pick_row(ui, id.with(*mask), self.sel == Some(s), true, &mask_name(*mask, t), "", "", None).clicked {
                        chosen = Some(s);
                    }
                }
                chosen
            })
            .flatten();
            if let Some(s) = chosen {
                pick = Some(s);
                c.memory_mut(|m| m.close_popup());
            }
        }
        pick
    }

    /// The node view: where the hands go (fingers/graph.rs). The chat
    /// field's routes from hand signals and every bind's part. A kept
    /// route's target box opens a search over its plugin's parameters and
    /// the Signals, its source box one over the hand movements
    /// (param_find.rs); a pick retargets the route and the `x` at the
    /// target box's right end removes it, both through the window
    /// (take_retarget, take_unmap). Any other target box unfolds the routes
    /// in the top section.
    fn routes(&mut self, ui: &mut Ui, store: &Store, routing: &Routing, writer: &Writer, t: &Text) {
        let mut edges = routing.edges.clone();
        for b in &store.binds {
            let (Some(s), Some(p)) = (Slot::from_bind(b), store.get(b.part)) else { continue };
            edges.push(EdgeIn { signal: s.edge_signal(), target: NodeTarget::Part { id: p.id, name: p.name.clone() }, pending: false });
        }
        let w = ui.available_width();
        let mut g = graph::layout(&edges, w, t);
        if g.nodes.is_empty() {
            self.route_edit.finding = None;
            hint(ui, t.f_routes_empty);
            return;
        }
        let (rect, _) = ui.allocate_exact_size(vec2(w, g.height), Sense::hover());
        let o = rect.min.to_vec2();
        // what the boxes change: the chat field's kept routes from a hand
        // to a parameter or a Signal
        let kept: Vec<&EdgeIn> = routing.edges.iter().filter(|e| !e.pending && graph::split(&e.signal).is_some() && !matches!(e.target, NodeTarget::Part { .. })).collect();
        let into = |tg: &NodeTarget| kept.iter().filter(|e| e.target == *tg).map(|e| (*e).clone()).collect::<Vec<EdgeIn>>();
        let from = |sig: &str| kept.iter().filter(|e| e.signal == sig).map(|e| (*e).clone()).collect::<Vec<EdgeIn>>();
        let fid = ui.id().with("route-find");
        let xr = |r: Rect| Rect::from_center_size(pos2(r.right() - 2.0 - Tok::CTRL_H_SM / 2.0, r.center().y), vec2(Tok::CTRL_H_SM, Tok::CTRL_H_SM));
        let base = ui.id();
        let x_id = |i: usize| base.with(("route-x", i));
        // the boxes before the x, so the x takes its own clicks
        let mut owner: Option<(Rect, egui::Response)> = None;
        let mut xs: Vec<(usize, Rect, Vec<EdgeIn>)> = Vec::new();
        for (i, n) in g.nodes.iter_mut().enumerate() {
            let find = match (n.kind, &n.target, &n.signal) {
                (graph::NodeKind::Target, Some(tg), _) if !into(tg).is_empty() => Some(graph::Finding::Target(tg.clone())),
                (graph::NodeKind::Source(_), _, Some(sig)) if !from(sig).is_empty() => Some(graph::Finding::Source(sig.clone())),
                (graph::NodeKind::Target, ..) => None,
                _ => continue,
            };
            let r = n.rect.translate(o);
            let mut resp = ui.interact(r, base.with(("route-node", i)), Sense::click());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let Some(f) = find else {
                if resp.clicked() {
                    self.unfold = true;
                }
                continue;
            };
            if let graph::Finding::Target(tg) = &f {
                n.tail = Tok::CTRL_H_SM + 2.0;
                // armed: "Remove? Click again" takes the box's text place
                if ui.data(|d| d.get_temp::<f64>(x_id(i).with("x-arm"))).is_some() {
                    n.text.clear();
                }
                xs.push((i, r, into(tg)));
            }
            resp = resp.on_hover_text(if matches!(f, graph::Finding::Target(_)) { t.pf_target_hover } else { t.pf_source_hover });
            let mine = self.route_edit.finding.as_ref() == Some(&f);
            if resp.clicked() {
                if mine && param_find::is_open(ui, fid) {
                    ui.memory_mut(|m| m.close_popup());
                } else {
                    self.route_edit.finding = Some(f.clone());
                    param_find::open(ui, fid);
                }
            }
            if self.route_edit.shot_find && matches!(f, graph::Finding::Target(_)) {
                self.route_edit.shot_find = false;
                self.route_edit.finding = Some(f.clone());
                param_find::open(ui, fid);
            }
            if self.route_edit.finding.as_ref() == Some(&f) {
                owner = Some((r, resp));
            }
        }
        graph::paint(ui, &g, rect.min);
        for (i, r, routes) in xs {
            let (confirmed, _) = glass::arm_x(ui, x_id(i), xr(r), r, t.remove_hover, t.unmap_arm);
            if confirmed {
                self.route_edit.unmap.extend(routes.into_iter().map(|e| (e.signal, e.target)));
            }
        }
        let (Some(f), Some((anchor, resp))) = (self.route_edit.finding.clone(), owner) else {
            self.route_edit.finding = None;
            return;
        };
        let (items, current, note) = match &f {
            graph::Finding::Target(tg) => {
                let plugin = match tg {
                    NodeTarget::Param { plugin, .. } if !plugin.is_empty() => Some(plugin.as_str()),
                    _ => None,
                };
                let key = param_find::plugin_key(plugin, fl::Key::parse(writer.params.target_pin.read().trim()));
                let (names, note) = match param_find::names(key.as_ref(), &mut self.route_edit.wanted, t) {
                    Ok(n) => (n, None),
                    Err(e) => (Vec::new(), Some(e)),
                };
                let items = param_find::targets(key.as_ref(), &names, t);
                let label = match tg {
                    NodeTarget::Param { name, .. } => name.clone(),
                    NodeTarget::Macro(i) => fill(t.p_signal, &[&(i + 1)]),
                    NodeTarget::Part { name, .. } => name.clone(),
                };
                let current = items.iter().find(|(_, l)| *l == label).map(|(v, _)| v.clone());
                (items, current, note)
            }
            graph::Finding::Source(sig) => (param_find::moves(t), graph::split(sig).map(|(_, feat)| param_find::Found::Move(feat.to_string())), None),
        };
        let picked = param_find::popup(ui, fid, anchor, &resp, current.as_ref(), &items, note.as_deref(), t);
        if let Some(p) = picked.filter(|p| Some(p) != current.as_ref()) {
            let routes = match &f {
                graph::Finding::Target(tg) => into(tg),
                graph::Finding::Source(sig) => from(sig),
            };
            self.route_edit.retarget.extend(routes.into_iter().map(|e| (e.signal, e.target, p.clone())));
        }
        if !param_find::is_open(ui, fid) {
            self.route_edit.finding = None;
        }
    }

    /// A finger with no part yet: the owner row, the kinds and the part
    /// picker (`empty_owner`); a pick makes the part on the hand's channel.
    fn nothing_yet(&mut self, ui: &mut Ui, store: &mut Store, slot: Slot, live: &Live, t: &Text, de: bool) {
        let d = slot.finger_index().map_or(0, |f| [0, 5, 2, 6, 4][f]);
        let Some(seed) = self.empty_owner(ui, store.key, Owner::Slot(slot), d, t.f_held, t, de) else { return };
        let ch = default_channel(store, slot, live);
        let id = seed.add(store, ch);
        store.bind(&slot.signal(), slot.trigger(), id);
    }

    /// An owner with no part yet, in the rows of R17: the owner row
    /// ("{owner} plays nothing yet:" in its colour), then the three kinds
    /// with none picked and the part picker. A click on a kind or a pick
    /// in the list makes the part: an empty pattern, a chord on `degree`,
    /// a note (`note` names that kind), a template or a saved part.
    #[allow(clippy::too_many_arguments)]
    fn empty_owner(&mut self, ui: &mut Ui, key: Key, owner: Owner, degree: u8, note: &str, t: &Text, de: bool) -> Option<Seed> {
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), Tok::CTRL_H), Sense::hover());
        let label = owner.label(t, de);
        let g = glass::galley(ui, &fill(t.f_nothing, &[&label]), Tok::BODY);
        glass::text_at(ui.painter(), pos2(row.left(), row.center().y - g.size().y / 2.0), g, owner.text_color());
        // a finger can also drive parameters (the chat field's routes)
        let src = param_src(owner);
        let mut kinds = vec![(Some(Kind4::Part(Kind3::Pattern)), t.f_pattern), (Some(Kind4::Part(Kind3::Chord)), t.f_chord), (Some(Kind4::Part(Kind3::Held)), note)];
        if src.is_some() {
            kinds.push((Some(Kind4::Param), t.f_param));
        }
        let empty = || Seed { name: t.f_pattern.to_string(), play: Play::Clip { bars: 1, steps: Vec::new(), rows: Vec::new() }, saved: None };
        let mut seed = None;
        let shown = src.is_some() && self.params;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = Tok::GAP;
            let mut kind = shown.then_some(Kind4::Param);
            glass::segmented(ui, &mut kind, &kinds);
            seed = match kind {
                Some(Kind4::Part(Kind3::Pattern)) => Some(empty()),
                Some(Kind4::Part(Kind3::Chord)) => Some(Seed { name: midi::chord_name(key, degree, 3), play: Play::Chords { degrees: vec![degree], size: 3, octave: 4, vel: 96, strum: 0 }, saved: None }),
                Some(Kind4::Part(Kind3::Held)) => Some(Seed { name: "C5".into(), play: Play::Note { pitch: 60, vel: 100 }, saved: None }),
                Some(Kind4::Param) | None => None,
            };
            if src.is_some() {
                self.params = kind == Some(Kind4::Param);
            }
            let id = Id::new(("part-picker-new", &label));
            match self.pick_list(ui, id, Kind3::Pattern, "", t) {
                Some(PartPick::Template(name)) => seed = midi::template(name).map(|play| Seed { name: name.to_string(), play, saved: None }),
                Some(PartPick::Empty) => seed = Some(empty()),
                Some(PartPick::Saved(name)) => match part_lib::load(&name) {
                    Ok(p) => seed = Some(Seed { name: p.name, play: p.play, saved: Some((p.mode, p.start)) }),
                    Err(e) => self.part_msg = Some(e),
                },
                Some(PartPick::Delete(_)) | None => {}
            }
        });
        if let Some(m) = &self.part_msg {
            hint(ui, m);
        }
        if let Some(src) = src.filter(|_| self.params) {
            self.unmap = param_list(ui, &edges_of(&self.edges, src), t).or(self.unmap.take());
        }
        if seed.is_some() {
            self.part_msg = None;
        }
        seed
    }

    /// A hand picked by its node: the owner row and what it drives.
    fn hand_params(&mut self, ui: &mut Ui, side: usize, t: &Text) {
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), Tok::CTRL_H), Sense::hover());
        let name = [t.hand_left, t.hand_right][side.min(1)];
        let g = glass::galley(ui, &fill(t.f_hand_drives, &[&name]), Tok::BODY);
        glass::text_at(ui.painter(), pos2(row.left(), row.center().y - g.size().y / 2.0), g, HAND_TEXT[side.min(1)]);
        if std::mem::take(&mut self.scroll_owner) {
            ui.scroll_to_rect(row, Some(Align::Min));
        }
        self.owner_row = Some(row);
        self.unmap = param_list(ui, &edges_of(&self.edges, Src::Hand(side)), t).or(self.unmap.take());
    }

    /// The Back slot: reserved at its full width, so nothing in its row
    /// moves when "Back to {name}" comes and goes.
    fn back_slot(&mut self, ui: &mut Ui, part: &mut Part, t: &Text) {
        let name = self.undo.as_ref().filter(|u| u.part == part.id).map(|u| u.name.clone());
        let undo = &mut self.undo;
        ui.allocate_ui_with_layout(vec2(BACK_W, Tok::CTRL_H), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_size(vec2(BACK_W, Tok::CTRL_H));
            if let Some(name) = name {
                let text = glass::fit(ui, &fill(t.f_back, &[&name]), Tok::BODY, BACK_W - 16.0);
                if glass::quiet(ui, &text).clicked() {
                    back(part, undo);
                }
            }
        });
    }

    /// The part picker of a part's tools row, applied to `part`: a
    /// template or Empty with the undo slot, a saved part replaces play,
    /// mode, start and name and fits drum notes to the channel.
    fn part_picker(&mut self, ui: &mut Ui, part: &mut Part, t: &Text) {
        match self.pick_list(ui, Id::new(("part-picker", part.id)), kind_of(&part.play), &part.name, t) {
            Some(PartPick::Template(name)) => {
                if let Some(mut p) = midi::template(name) {
                    if let Some(c) = &part.channel {
                        midi::fit_to_channel(&mut p, &c.name);
                    }
                    remember(part, &mut self.undo);
                    part.play = p;
                    part.name = name.into();
                }
            }
            Some(PartPick::Empty) => {
                remember(part, &mut self.undo);
                if let Play::Clip { steps, .. } = &mut part.play {
                    steps.clear();
                }
            }
            Some(PartPick::Saved(name)) => match part_lib::load(&name) {
                Ok(p) => {
                    remember(part, &mut self.undo);
                    part.play = p.play;
                    part.mode = p.mode;
                    part.start = p.start;
                    part.name = p.name;
                    if let Some(c) = &part.channel {
                        midi::fit_to_channel(&mut part.play, &c.name);
                    }
                    self.part_msg = None;
                }
                Err(e) => self.part_msg = Some(e),
            },
            Some(PartPick::Delete(_)) | None => {}
        }
    }

    /// The part picker (2.7) for a part of `kind`: the button reads
    /// "Template"; the built-in templates first (the row named `current`
    /// selected), then the saved parts (part_lib) with a two-click Delete.
    /// A Delete runs here; any other pick closes the list and is returned.
    fn pick_list(&mut self, ui: &mut Ui, id: Id, kind: Kind3, current: &str, t: &Text) -> Option<PartPick> {
        let b = glass::picker_button(ui, id, vec2(PICKER_W, Tok::CTRL_H), t.f_template, false, "");
        if b.clicked() {
            self.saved = None;
        }
        let saved = self.saved.get_or_insert_with(part_lib::list).clone();
        let builtin: &[&'static str] = match kind {
            Kind3::Pattern => midi::TEMPLATES.get(..5).unwrap_or(&[]),
            Kind3::Chord => midi::TEMPLATES.get(5..).unwrap_or(&[]),
            Kind3::Held => &[],
        };
        let pick = glass::picker_popup(ui, id, &b, 420.0, |ui| {
            let mut pick = None;
            for (i, name) in builtin.iter().enumerate() {
                if glass::pick_row(ui, id.with(("tpl", i)), current == *name, true, name, t.preset_builtin, "", None).clicked {
                    pick = Some(PartPick::Template(name));
                }
            }
            if kind == Kind3::Pattern && glass::pick_row(ui, id.with("empty"), false, true, t.f_empty, t.preset_builtin, "", None).clicked {
                pick = Some(PartPick::Empty);
            }
            glass::pick_header(ui, t.f_saved_parts);
            if saved.is_empty() {
                glass::pick_header(ui, t.f_no_parts);
            }
            for (i, name) in saved.iter().enumerate() {
                let r = glass::pick_row(ui, id.with(("saved", i)), false, true, name, "", "", Some((t.delete, t.delete_arm)));
                if r.clicked {
                    pick = Some(PartPick::Saved(name.clone()));
                }
                if r.delete {
                    pick = Some(PartPick::Delete(name.clone()));
                }
            }
            pick
        })
        .flatten();
        match &pick {
            Some(PartPick::Delete(name)) => {
                self.part_msg = part_lib::delete(name).err();
                self.saved = None;
            }
            Some(_) => ui.memory_mut(|m| m.close_popup()),
            None => {}
        }
        pick
    }

    /// The right group of the tools row: the part's name (edits rename
    /// it) and Save, "Overwrite" when a saved part has that name.
    fn name_save(&mut self, ui: &mut Ui, part: &mut Part, t: &Text) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = Tok::GAP;
            let exists = self.saved.get_or_insert_with(part_lib::list).iter().any(|n| n.trim().eq_ignore_ascii_case(part.name.trim()));
            if glass::quiet(ui, if exists { t.overwrite } else { t.save }).clicked() {
                self.part_msg = part_lib::save(&part.name, part).err();
                self.saved = None;
            }
            glass::text_field(ui, Id::new(("part-name", part.id)), &mut part.name, NAME_W, t.f_part_name);
        });
    }

    /// The tools row's tail: the part picker, Back's slot, the name and
    /// Save. It stays in the row when the row has room for all of it and
    /// takes a line of its own under the row otherwise (a chord's fields
    /// at 720 pt), so the picker never runs under the name field.
    fn tools_tail(&mut self, ui: &mut Ui, part: &mut Part, t: &Text) -> bool {
        let save_w = [t.save, t.overwrite].iter().map(|s| glass::galley(ui, s, Tok::BODY).size().x + 16.0).fold(0.0, f32::max);
        if ui.available_width() < PICKER_W + BACK_W + NAME_W + save_w + 3.0 * Tok::GAP {
            return false;
        }
        self.tail(ui, part, t);
        true
    }

    /// `tools_tail` on a line of its own when the row had no room for it.
    fn tools_tail_line(&mut self, ui: &mut Ui, part: &mut Part, t: &Text, placed: bool) {
        if !placed {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = Tok::GAP;
                self.tail(ui, part, t);
            });
        }
    }

    fn tail(&mut self, ui: &mut Ui, part: &mut Part, t: &Text) {
        self.part_picker(ui, part, t);
        self.back_slot(ui, part, t);
        self.name_save(ui, part, t);
    }

    fn single(&mut self, ui: &mut Ui, store: &mut Store, slot: Slot, live: &Live, t: &Text, de: bool) {
        let (sig, trig) = (slot.signal(), slot.trigger());
        let Some(id) = store.bound(&sig, trig).map(|p| p.id) else {
            self.nothing_yet(ui, store, slot, live, t, de);
            return;
        };
        let (remove, geom) = self.part_editor(ui, store, id, Owner::Slot(slot), live, t, de);
        self.geom = geom;
        if remove {
            store.unbind(&sig, trig);
        }
    }

    /// Part `id` as its owner edits it, in the rows of R17: the owner row
    /// (Hide roll, Remove), the editor row (kind, channel, start and for a
    /// finger how it plays), the tools row, the roll and its footer, the
    /// hints. Returns whether Remove was clicked and where the roll was
    /// drawn.
    #[allow(clippy::too_many_arguments)]
    fn part_editor(&mut self, ui: &mut Ui, store: &mut Store, id: u64, owner: Owner, live: &Live, t: &Text, de: bool) -> (bool, Option<Geom>) {
        let key = store.key;
        let mut remove = false;
        let Some(part) = store.get_mut(id) else { return (false, None) };
        let color = owner.color();
        let wall = matches!(owner, Owner::Wall(_));
        let playing = live.playing.contains(&id);
        let start_was = part.start;
        let mut start_picked = false;
        // 1. owner row: who plays it; Hide roll, then Remove at the right
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), Tok::CTRL_H), Sense::hover());
        let dot = if playing { "●" } else { "○" };
        let g = glass::galley(ui, &format!("{dot} {}", owner.label(t, de)), Tok::BODY);
        glass::text_at(ui.painter(), pos2(row.left(), row.center().y - g.size().y / 2.0), g, owner.text_color());
        let mut right = ui.new_child(UiBuilder::new().max_rect(row).layout(Layout::right_to_left(Align::Center)));
        right.spacing_mut().item_spacing.x = Tok::GAP;
        if glass::quiet_dim(&mut right, t.f_remove).clicked() {
            remove = true;
        }
        // folds the roll away, so the wheel scrolls the page again
        if glass::quiet(&mut right, if self.roll_open { t.f_roll_hide } else { t.f_roll_show }).clicked() {
            self.roll_open = !self.roll_open;
            self.roll.gesture = Gesture::Idle;
        }
        if std::mem::take(&mut self.scroll_owner) {
            ui.scroll_to_rect(row, Some(Align::Min));
        }
        self.owner_row = Some(row);
        // 2. editor row; the gate mode takes a row of its own when the
        // row would not fit
        let held = if wall { t.fw_note } else { t.f_held };
        let src = param_src(owner);
        let mut kinds = vec![(Kind4::Part(Kind3::Pattern), t.f_pattern), (Kind4::Part(Kind3::Chord), t.f_chord), (Kind4::Part(Kind3::Held), held)];
        if src.is_some() {
            kinds.push((Kind4::Param, t.f_param));
        }
        let modes = [(Mode::Gate, t.f_while_held), (Mode::Latch, t.f_toggle), (Mode::Once, t.f_once)];
        let need = seg_width(ui, &kinds) + CHANNEL_W + START_W + seg_width(ui, &modes) + 3.0 * Tok::GAP + 24.0;
        let wrap = !wall && need > ui.available_width();
        // Parameter shows the finger's routes; the part stays as it is
        let mut params = src.is_some() && self.params;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = Tok::GAP;
            let mut kind = if params { Kind4::Param } else { Kind4::Part(kind_of(&part.play)) };
            if glass::segmented(ui, &mut kind, &kinds).changed() {
                params = kind == Kind4::Param;
                match kind {
                    Kind4::Part(k) if k != kind_of(&part.play) => set_kind(part, k, key, owner.finger(), self.roll.active_row, &mut self.undo),
                    _ => {}
                }
            }
            if params {
                return;
            }
            channel_picker(ui, part, live, t);
            match owner {
                Owner::Slot(_) => {
                    start_picked = start_picker(ui, "midi-start", &mut part.start, t);
                    if !wrap {
                        glass::segmented(ui, &mut part.mode, &modes);
                    }
                }
                // a hit has no "while shown": only when it plays
                Owner::Wall(_) => {
                    ui.label(RichText::new(t.fw_hit_plays).size(Tok::BODY).color(white(Tok::LABEL)));
                    start_picked = start_picker(ui, "wall-start", &mut part.start, t);
                }
            }
        });
        if !wall {
            self.params = params;
        }
        if let Some(src) = src.filter(|_| params) {
            self.unmap = param_list(ui, &edges_of(&self.edges, src), t).or(self.unmap.take());
            return (remove, None);
        }
        if wrap {
            ui.horizontal(|ui| glass::segmented(ui, &mut part.mode, &modes));
        }
        let playhead = playing.then(|| (live.player.beats_of(id) * 4.0) as f32);
        let drums = part.channel.as_ref().is_none_or(|c| c.name.to_lowercase().contains("fpc"));
        // a gesture that turned a chord into a pattern keeps the chord's
        // tools above the roll until the release (drawn from the undo slot)
        let ghost = match (&self.undo, &part.play) {
            (Some(u), Play::Clip { .. }) if self.roll.on_chord(id) && u.part == id && matches!(u.play, Play::Chords { .. }) => Some(Part { play: u.play.clone(), ..part.clone() }),
            _ => None,
        };
        let kind = if ghost.is_some() { Kind3::Chord } else { kind_of(&part.play) };
        // 3. tools row; the tools keep the tallest height they had for
        // this part, so the roll under them stays where it is when a
        // chord becomes a pattern
        let min_h = self.tools_h.filter(|(p, _)| *p == id).map_or(0.0, |(_, h)| h);
        let mut pending = None;
        let tools = ui.vertical(|ui| {
            ui.set_min_height(min_h);
            match kind {
                Kind3::Pattern => self.clip_tools(ui, part, t),
                Kind3::Chord => {
                    pending = match ghost {
                        Some(mut g) => {
                            self.chord_tools(ui, &mut g, key, color, t);
                            None
                        }
                        None => self.chord_tools(ui, part, key, color, t),
                    };
                }
                Kind3::Held => {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = Tok::GAP;
                        if let Play::Note { vel, .. } = &mut part.play {
                            if let (_, Some(v)) = deferred_field(ui, Id::new(("held-vel", id)), *vel, 1..=127, 100, t.f_vel, 104.0, |v| v.to_string()) {
                                *vel = v;
                            }
                        }
                        self.back_slot(ui, part, t);
                    });
                }
            }
        });
        self.tools_h = Some((id, tools.response.rect.height().max(min_h)));
        let lanes = match kind {
            Kind3::Pattern => !self.piano,
            Kind3::Chord => false,
            Kind3::Held => true,
        };
        // 4. the roll: the chromatic roll always overflows its 15 rows and
        // takes the wheel; the lanes show all their rows, so the wheel over
        // the grid scrolls the panel area (over a lane's name it moves the
        // lane's note)
        let cx = RollCx { key, color, playhead, lanes, drums, t };
        let mut geom = None;
        if self.roll_open {
            let g = match pending {
                Some(play) => {
                    let mut preview = part.clone();
                    preview.play = play;
                    roll_ui(ui, &mut preview, &cx, &mut self.roll, &mut self.undo)
                }
                None => roll_ui(ui, part, &cx, &mut self.roll, &mut self.undo),
            };
            geom = Some(g);
            // 5. footer
            if kind != Kind3::Held {
                self.footer(ui, part, &cx);
            }
        }
        // a chord edited as notes stays in the roll, not drum lanes
        if kind == Kind3::Chord && !matches!(part.play, Play::Chords { .. }) {
            self.piano = true;
        }
        // 6. hints
        match kind {
            Kind3::Pattern => {
                if self.undo.as_ref().is_some_and(|u| u.part == id && !matches!(u.play, Play::Clip { .. })) {
                    hint(ui, if wall { t.fw_now_pattern } else { t.f_now_pattern });
                }
                if self.roll_open {
                    hint(ui, t.f_roll_hint);
                    hint(ui, if lanes { t.f_lane_hint } else { t.f_scroll_hint });
                }
            }
            Kind3::Chord if self.roll_open => {
                hint(ui, if wall { t.fw_chord_hint } else { t.f_chord_hint });
                hint(ui, t.f_scroll_hint);
            }
            Kind3::Chord => {}
            Kind3::Held => hint(ui, if wall { t.fw_note_hint } else { t.f_held_hint }),
        }
        if let Some(m) = &self.part_msg {
            hint(ui, m);
        }
        // a chord made a pattern starts on the next bar for a finger; a
        // wall's hits keep the start its picker shows
        if wall && !start_picked && part.start != start_was {
            part.start = start_was;
        }
        (remove, geom)
    }

    /// Bars, Lanes | Piano roll, the part picker, Back; the name and Save
    /// at the right (the tail on a line of its own when it does not fit).
    fn clip_tools(&mut self, ui: &mut Ui, part: &mut Part, t: &Text) {
        let placed = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = Tok::GAP;
            if let Play::Clip { bars, .. } = &mut part.play {
                let mut b = i32::from(*bars);
                if glass::slider_field(ui, Id::new(("bars", part.id)), &mut b, 1..=i32::from(midi::MAX_BARS), 1, t.f_bars, 88.0, |v| v.to_string()).changed() {
                    *bars = u8::try_from(b).unwrap_or(1);
                }
            }
            let mut piano = self.piano;
            glass::segmented(ui, &mut piano, &[(false, t.f_lanes), (true, t.f_roll)]);
            self.piano = piano;
            self.tools_tail(ui, part, t)
        });
        self.tools_tail_line(ui, part, t, placed.inner);
    }

    /// Velocity | Repeats; in lanes mode Fill (the active lane) and + row.
    fn footer(&mut self, ui: &mut Ui, part: &mut Part, cx: &RollCx) {
        let t = cx.t;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = Tok::GAP;
            glass::segmented(ui, &mut self.roll.lane, &[(EventLane::Velocity, t.f_lane_vel), (EventLane::Rolls, t.f_repeats)]);
            if !cx.lanes {
                return;
            }
            let rows = lane_rows(part);
            let lane = active_lane(&self.roll, &rows);
            let Play::Clip { bars, steps, rows: kept } = &mut part.play else { return };
            let n = steps_of(*bars);
            if let Some(lane) = lane {
                // only the bars shown count; hits in bars hidden by a lower
                // bar count stay where they are
                let mut k = (steps.iter().filter(|s| s.pitch == lane && s.at < n).count() as i32).min(i32::from(n));
                let was = k;
                glass::slider_field(ui, Id::new(("fill", part.id)), &mut k, 0..=i32::from(n), was, t.f_fill, 108.0, |k| format!("{k} / {n}"));
                if k != was {
                    steps.retain(|s| s.pitch != lane || s.at >= n);
                    for (i, on) in midi::euclid(k as u16, n).into_iter().enumerate() {
                        if on {
                            steps.push(Step { at: i as u16, len: 1, pitch: lane, vel: if i % 4 == 0 { 110 } else { 85 }, ratchet: 1, nudge: 0 });
                        }
                    }
                    if !kept.contains(&lane) {
                        kept.push(lane);
                    }
                }
            }
            // + row: another note as a lane
            let r = glass::quiet(ui, t.f_add_row);
            let pid = Id::new(("row-pop", part.id));
            if r.clicked() {
                ui.memory_mut(|m| m.toggle_popup(pid));
            }
            let top = rows.first().copied().unwrap_or(60);
            let next = (top..=127).find(|p| !rows.contains(p));
            let pick = glass::popup_list(ui, pid, r.rect, &r, LANE_POP_W, |ui| lane_menu(ui, cx.drums, 255, false, next, t)).flatten();
            if let Some(Pick::To(p)) = pick {
                for r in rows.iter().copied().filter(|r| !kept.contains(r)).collect::<Vec<_>>() {
                    kept.push(r);
                }
                if !kept.contains(&p) {
                    kept.push(p);
                }
                self.roll.active_row = Some(p);
                ui.memory_mut(|m| m.close_popup());
            }
        });
    }

    /// Degree chips and the chord's fields. While a field is dragged the
    /// chord as it would be (written on release).
    fn chord_tools(&mut self, ui: &mut Ui, part: &mut Part, key: Key, color: Color32, t: &Text) -> Option<Play> {
        let id = part.id;
        let shift = ui.input(|i| i.modifiers.shift);
        let n = key.scale.steps().len() as u8;
        let mut pending = None;
        if let Play::Chords { degrees, size, .. } = &mut part.play {
            ui.horizontal_wrapped(|ui| {
                for d in 0..n {
                    let label = format!("{} {}", midi::degree_name(key, d), midi::chord_name(key, d, *size));
                    if glass::chip(ui, degrees.contains(&d), &label, Some(color)).clicked() {
                        if shift && degrees.len() < usize::from(midi::MAX_BARS) {
                            degrees.push(d);
                        } else {
                            *degrees = vec![d];
                        }
                    }
                }
            });
        }
        // Back sits in the fields row in a slot of its own: its coming and
        // going moves nothing; with the picker and the name it takes a
        // line under the fields when they leave no room (720 pt)
        let placed = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = Tok::GAP;
            if let Play::Chords { degrees, size, octave, vel, strum } = &mut part.play {
                let (s, cs) = deferred_field(ui, Id::new(("ch-size", id)), *size, 1..=5, 3, t.f_size, 84.0, |v| v.to_string());
                let (o, co) = deferred_field(ui, Id::new(("ch-oct", id)), *octave, 1..=8, 4, t.f_octave, 92.0, |v| v.to_string());
                let (st, cst) = deferred_field(ui, Id::new(("ch-strum", id)), *strum, 0..=24, 0, t.f_strum, 84.0, |v| v.to_string());
                let (v, cv) = deferred_field(ui, Id::new(("ch-vel", id)), *vel, 1..=127, 96, t.f_vel, 112.0, |v| v.to_string());
                if (s, o, st, v) != (*size, *octave, *strum, *vel) {
                    pending = Some(Play::Chords { degrees: degrees.clone(), size: s, octave: o, vel: v, strum: st });
                }
                *size = cs.unwrap_or(*size);
                *octave = co.unwrap_or(*octave);
                *strum = cst.unwrap_or(*strum);
                *vel = cv.unwrap_or(*vel);
            }
            self.tools_tail(ui, part, t)
        });
        self.tools_tail_line(ui, part, t, placed.inner);
        pending.filter(|p| *p != part.play)
    }

    /// One row per bound finger or shape, then one per wall with a part.
    fn total(&mut self, ui: &mut Ui, store: &Store, walls: &[(Side, u64)], live: &Live, t: &Text, de: bool) {
        let mut binds: Vec<(Slot, u64)> = store.binds.iter().filter_map(|b| Slot::from_bind(b).map(|s| (s, b.part))).collect();
        binds.sort_by_key(|(s, _)| (std::cmp::Reverse(s.side), s.finger_index().unwrap_or(9), s.mask));
        if binds.is_empty() {
            hint(ui, t.f_pick_part);
        }
        let mut open = None;
        for (slot, id) in binds {
            let Some(part) = store.get(id) else { continue };
            if total_row(ui, &slot.label(t, de), HAND[slot.side.min(1)], part, store.key, live, t) {
                open = Some(slot);
            }
        }
        for (side, id) in walls {
            let Some(part) = store.get(*id) else { continue };
            if total_row(ui, wall_name(*side, t), Owner::Wall(*side).color(), part, store.key, live, t) {
                self.wall_pick = Some(*side);
            }
        }
        if let Some(s) = open {
            self.select(s, ui.ctx());
        }
    }
}

/// A row of the total view: owner, channel › name, the mini clip. True
/// when clicked.
fn total_row(ui: &mut Ui, label: &str, color: Color32, part: &Part, key: Key, live: &Live, t: &Text) -> bool {
    let id = part.id;
    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(row, CornerRadius::same(Tok::R_FIELD), glass::veil(Tok::HOVER));
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let lit = live.playing.contains(&id);
    let y = row.center().y;
    p.circle_filled(pos2(row.left() + 6.0, y), 3.0, if lit { Color32::WHITE } else { color });
    p.text(pos2(row.left() + 14.0, y), Align2::LEFT_CENTER, label, glass::font(Tok::BODY), if lit { white(Tok::STRONG) } else { white(Tok::ROW) });
    let what = format!("{} › {}", part.channel.as_ref().map_or(t.f_no_channel, |c| c.name.as_str()), part.name);
    let mut job = egui::text::LayoutJob::simple_singleline(what, glass::font(Tok::BODY), white(Tok::LABEL));
    job.wrap = egui::text::TextWrapping { max_width: 200.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let g = p.layout_job(job);
    p.galley(pos2(row.left() + 190.0, y - g.size().y / 2.0), g, white(Tok::LABEL));
    let clip = Rect::from_min_max(pos2(row.left() + 400.0, y - 12.0), pos2(row.right() - 4.0, y + 12.0));
    let playhead = lit.then(|| (live.player.beats_of(id) * 4.0) as f32);
    mini_clip(p, clip, part, key, color, playhead);
    resp.clicked()
}

/// Who a part belongs to: a finger or shape (a bind) or a Pong wall (its
/// hits play it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Slot(Slot),
    Wall(Side),
}

impl Owner {
    fn label(&self, t: &Text, de: bool) -> String {
        match self {
            Owner::Slot(s) => s.label(t, de),
            Owner::Wall(side) => wall_name(*side, t).to_string(),
        }
    }

    /// The hand's colour for a finger; a wall is the selection's accent
    /// (its editor shows the selected wall).
    fn color(&self) -> Color32 {
        match self {
            Owner::Slot(s) => HAND[s.side.min(1)],
            Owner::Wall(_) => Tok::ACCENT_HI,
        }
    }

    /// `color` for text on the dark background (HAND_TEXT).
    fn text_color(&self) -> Color32 {
        match self {
            Owner::Slot(s) => HAND_TEXT[s.side.min(1)],
            Owner::Wall(_) => Tok::ACCENT_HI,
        }
    }

    fn finger(&self) -> Option<usize> {
        match self {
            Owner::Slot(s) => s.finger_index(),
            Owner::Wall(_) => None,
        }
    }
}

/// "Top wall", "Obere Wand".
pub fn wall_name(side: Side, t: &Text) -> &'static str {
    match side {
        Side::Top => t.fw_top,
        Side::Right => t.fw_right,
        Side::Bottom => t.fw_bottom,
        Side::Left => t.fw_left,
    }
}

/// FL's channels, whether the link is up with a project loaded, and the
/// generator channel in focus.
fn fl_view() -> (Vec<fl::ChannelRow>, bool, Option<u32>) {
    let v = fl::bridge().view();
    let focus = v.focus.as_ref().and_then(|k| match k.at {
        fl::At::Gen { ch } => Some(ch),
        fl::At::Fx { .. } => None,
    });
    (v.channels.clone(), v.link == fl::Link::Up && v.loaded, focus)
}

/// A part's notes in 24 points: x over the loop, y over its pitch span.
fn mini_clip(p: &Painter, r: Rect, part: &Part, key: Key, color: Color32, playhead: Option<f32>) {
    if r.width() < 20.0 {
        return;
    }
    p.rect_filled(r, CornerRadius::same(Tok::R_FIELD), glass::veil(Tok::INSET_FILL));
    let v = view_clip(part, key);
    let n = steps_of(v.bars);
    let vis: Vec<&Step> = v.steps.iter().filter(|s| s.at < n).collect();
    let (Some(lo), Some(hi)) = (vis.iter().map(|s| s.pitch).min(), vis.iter().map(|s| s.pitch).max()) else { return };
    let span = f32::from(hi - lo + 1);
    let bar_h = (24.0 / span).clamp(2.0, 6.0);
    let x = |t: f32| r.left() + t / (f32::from(n) * TICKS) * r.width();
    for s in vis {
        let cy = if hi == lo { r.center().y } else { r.bottom() - 2.0 - (f32::from(s.pitch - lo) / f32::from(hi - lo)) * (r.height() - 4.0) };
        let t0 = start_tick(s);
        let x1 = x(t0 + f32::from(s.len.max(1)) * TICKS).min(r.right());
        let nr = Rect::from_min_max(pos2(x(t0) + 0.5, cy - bar_h / 2.0), pos2((x1 - 0.5).max(x(t0) + 1.5), cy + bar_h / 2.0));
        p.rect_filled(nr, CornerRadius::same(Tok::R_BAR), note_fill(color, s.vel));
    }
    if let Some(ph) = playhead {
        let px = p.round_to_pixel_center(x(ph.rem_euclid(f32::from(n)) * TICKS));
        p.vline(px, r.y_range(), Stroke::new(1.0_f32, white(0.70)));
    }
}

/// "Am", "F → G", "Trap roll", "C5"; a name cut at 18 characters with "…".
fn summary(p: &Part, key: Key) -> String {
    match &p.play {
        Play::Chords { degrees, .. } => {
            let n: Vec<String> = degrees.iter().take(3).map(|d| midi::chord_name(key, *d, 3)).collect();
            n.join("·")
        }
        Play::Note { pitch, .. } => midi::note_name(*pitch),
        Play::Clip { .. } if p.name.chars().count() > NODE_CHARS => p.name.chars().take(NODE_CHARS).chain(['…']).collect(),
        Play::Clip { .. } => p.name.clone(),
    }
}

/// The channel of the hand's other parts, else the generator FL has in
/// focus, else the first channel.
fn default_channel(store: &Store, slot: Slot, live: &Live) -> Option<Channel> {
    let same_hand = store.binds.iter().filter(|b| b.signal == slot.signal()).find_map(|b| store.get(b.part).and_then(|p| p.channel.clone()));
    same_hand.or_else(|| focus_channel(live))
}

/// The generator FL has in focus, else the first channel (a wall's part).
fn focus_channel(live: &Live) -> Option<Channel> {
    let pick = live.focus_ch.and_then(|ch| live.channels.iter().find(|c| c.index == ch)).or_else(|| live.channels.first())?;
    Some(Channel { index: pick.index, name: pick.label() })
}

const ROOTS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// "Key", the root, then the scale (R10); laid out right to left, so
/// the scale goes in first.
/// The Hands header's key: [root] [scale], laid out right to left (the
/// scale field `scale_w` wide). No "Key" label: the two fields read as
/// one key ("A Minor").
fn key_picker(ui: &mut Ui, key: &mut Key, scale_w: f32) {
    let root = key.root % 12;
    if let Some(s) = glass::dropdown(ui, "midi-scale", scale_w, &capital(key.scale.name()), &key.scale, Scale::ALL.map(|s| (s, capital(s.name())))) {
        key.scale = s;
    }
    if let Some(r) = glass::dropdown(ui, "midi-root", ROOT_W, ROOTS[usize::from(root)], &root, (0..12u8).map(|i| (i, ROOTS[usize::from(i)].to_string()))) {
        key.root = r;
    }
}

/// The root field's width: "C#" and the chevron.
const ROOT_W: f32 = 52.0;
/// The scale field: its value's width plus the chevron's room, between
/// these two; under the room it gets, down to `SCALE_MIN`.
const SCALE_MIN: f32 = 76.0;
const SCALE_MAX: f32 = 150.0;
/// What a dropdown needs beside its text (dropdown fits the text to w - 34).
const SCALE_PAD: f32 = 36.0;

/// The widths the Hands header lays out with (pure, so a test can check
/// the groups never meet): Single | All | Routes, the buttons with their
/// gaps, and the scale's current text.
#[derive(Clone, Copy, Debug)]
struct HeadWidths {
    seg: f32,
    buttons: f32,
    scale: f32,
}

impl HeadWidths {
    /// The scale field's width in a header slot `slot_w` wide.
    fn scale_w(&self, slot_w: f32) -> f32 {
        let room = slot_w - self.total(0.0);
        (self.scale + SCALE_PAD).clamp(SCALE_MIN, SCALE_MAX).min(room).max(SCALE_MIN)
    }

    /// Everything in a row at `scale_w`, gaps included.
    fn total(&self, scale_w: f32) -> f32 {
        self.seg + Tok::GAP + self.buttons + ROOT_W + Tok::GAP + scale_w
    }
}

fn channel_picker(ui: &mut Ui, part: &mut Part, live: &Live, t: &Text) {
    let shown = match (&part.channel, live.linked) {
        (_, false) => t.f_not_linked.to_string(),
        (Some(c), true) => format!("{} · {}", c.index + 1, c.name),
        (None, true) => t.f_no_channel.to_string(),
    };
    // without FL the field stays readable and dim (a disabled Ui paints
    // nothing in this style)
    if !live.linked {
        glass::dim_dropdown(ui, CHANNEL_W, &shown, t.f_not_linked_hover);
        return;
    }
    let cur = part.channel.as_ref().map(|c| c.index);
    let opts = live.channels.iter().map(|c| (Some(c.index), format!("{} · {}", c.index + 1, c.label())));
    if let Some(Some(ix)) = glass::dropdown(ui, ("midi-ch", part.id), CHANNEL_W, &shown, &cur, opts) {
        if let Some(c) = live.channels.iter().find(|c| c.index == ix) {
            part.channel = Some(Channel { index: c.index, name: c.label() });
            midi::fit_to_channel(&mut part.play, &c.label());
        }
    }
}

/// When the part starts; true when the person picked one.
fn start_picker(ui: &mut Ui, id: &str, start: &mut Start, t: &Text) -> bool {
    let name = |s: Start| match s {
        Start::Now => t.f_start_now,
        Start::Sixteenth => t.f_start_16,
        Start::Beat => t.f_start_beat,
        Start::Bar => t.f_start_bar,
    };
    let all = [Start::Now, Start::Sixteenth, Start::Beat, Start::Bar];
    let picked = glass::dropdown(ui, id, START_W, name(*start), start, all.map(|s| (s, name(s).to_string())));
    if let Some(s) = picked {
        *start = s;
    }
    picked.is_some()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nih_plug_egui::egui::{Context, FullOutput, RawInput};

    use super::*;
    use crate::i18n::EN;

    #[test]
    fn slots_round_trip_through_binds() {
        let s = Slot::finger(1, 1);
        assert_eq!(s.signal(), "cam0.hand.R.shape");
        assert_eq!(s.trigger(), Trigger::Shape { mask: 0b01000, care: 0b01000 });
        let b = midi::Bind { signal: s.signal(), trigger: s.trigger(), part: 3 };
        assert_eq!(Slot::from_bind(&b), Some(s));
        assert_eq!(s.finger_index(), Some(1));
        let peace = Slot::shape(0, 0b01100);
        assert_eq!(peace.finger_index(), None);
        assert_eq!(mask_name(0b01100, &EN), "Index and middle");
        assert_eq!(mask_name(0b10101, &EN), "Thumb, middle and pinky");
    }

    #[test]
    fn the_art_has_five_clickable_fingers_and_a_dissolving_wrist() {
        let art = build_art();
        assert_eq!((art.region.len(), art.lit.len()), (CANVAS * CANVAS, CANVAS * CANVAS));
        // the turned hand fits the canvas: nothing lit on its border
        for k in 0..CANVAS {
            for i in [k, (CANVAS - 1) * CANVAS + k, k * CANVAS, k * CANVAS + CANVAS - 1] {
                assert!(!art.lit[i], "lit dot on the border at {i}");
            }
        }
        for r in 2..=6u8 {
            let n = art.region.iter().filter(|x| **x == r).count();
            assert!(n >= 150, "finger {r} has {n} dots");
        }
        // each tip sits on its own finger
        for (f, [x, y]) in art.tips.iter().enumerate() {
            assert_eq!(art.region.get(*y as usize * CANVAS + *x as usize), Some(&(f as u8 + 2)), "tip {f} at {x} {y}");
        }
        // the right hand's middle finger points to the upper right, the
        // left hand's to the upper left
        let [mx, my] = art.tips[2];
        assert!(mx > CANVAS_C[0] && my < CANVAS_C[1], "{mx} {my}");
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(CANVAS as f32 * DOT));
        let left = on_canvas(rect, art.tips[2], true);
        assert!(left.x < CANVAS_C[0] * DOT && left.y < CANVAS_C[1] * DOT, "{left:?}");
        assert_eq!(dot_at(rect, left, true).map(|(x, y)| art.region[y * CANVAS + x]), Some(4));
        // the wrist trail thins out downwards and reaches as far as before
        let trail: Vec<[f32; 2]> = (0..CANVAS * CANVAS).filter(|i| art.region[*i] == TRAIL).map(|i| to_hand([(i % CANVAS) as f32 + 0.5, (i / CANVAS) as f32 + 0.5])).collect();
        let top = trail.iter().filter(|p| p[1] < 88.0).count();
        let bottom = trail.len() - top;
        assert!(top > bottom * 2, "the trail thins out downwards: {top} vs {bottom}");
        assert!(trail.iter().any(|p| p[1] > 98.0), "the trail reaches the wrist's end");
        assert_eq!(dot_at(Rect::from_min_size(Pos2::ZERO, Vec2::splat(240.0)), Pos2::new(1.0, 1.0), true), Some((CANVAS - 1, 0)));
        // the forward and inverse maps agree, and the spec's numbers hold
        let h = to_hand(to_canvas([43.0, 15.0]));
        assert!((h[0] - 43.0).abs() < 1e-3 && (h[1] - 15.0).abs() < 1e-3);
        let c = to_canvas([43.0, 15.0]);
        assert!((c[0] - 100.0).abs() < 1.0 && (c[1] - 46.0).abs() < 1.0, "{c:?}");
    }

    #[test]
    fn each_long_finger_has_two_joint_seams() {
        let art = build_art();
        let chains: Vec<Vec<[f32; 2]>> = (0..5).map(finger_chain).collect();
        for f in 1..5usize {
            let (len, r0) = (FINGERS[f].1, FINGERS[f].2);
            for j in JOINTS {
                let mut seam = 0;
                let mut lit_near = 0;
                for i in 0..CANVAS * CANVAS {
                    if art.region[i] != f as u8 + 2 {
                        continue;
                    }
                    let p = to_hand([(i % CANVAS) as f32 + 0.5, (i / CANVAS) as f32 + 0.5]);
                    let (axis, t) = chain_dist(p, &chains[f]);
                    let interior = axis - r0 * (1.0 - (1.0 - TAPER) * t) <= -0.9;
                    let off = ((t - j) * len).abs();
                    if interior && off < JOINT_SEAM / 2.0 {
                        assert!(!art.lit[i], "finger {f} joint {j}: a lit dot in the seam");
                        seam += 1;
                    }
                    if interior && (1.0..3.0).contains(&off) && art.lit[i] {
                        lit_near += 1;
                    }
                }
                assert!(seam >= 3, "finger {f} joint {j}: {seam} seam dots");
                assert!(lit_near >= 3, "finger {f} joint {j}: the finger stays lit beside the seam");
            }
            // the three segments: 0.49, 0.29, 0.22 of the length
            let c = &chains[f];
            let d = |a: [f32; 2], b: [f32; 2]| ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let total: f32 = c.windows(2).map(|w| d(w[0], w[1])).sum();
            assert!((total - len).abs() < 0.5, "finger {f}: {total} long");
        }
        assert_eq!(finger_chain(0).len(), 3, "the thumb has two segments");
    }

    fn all_widths() -> [Option<f32>; 5] {
        [Some(80.0); 5]
    }

    #[test]
    fn labels_never_overlap() {
        let (lo, hi) = (11.0, 229.0);
        for base in [0.0f32, 100.0, 225.0, 260.0] {
            let mut ys = vec![base, base + 3.0, base + 6.0, base + 10.0];
            spread(&mut ys, NODE_GAP, lo, hi);
            for w in ys.windows(2) {
                assert!(w[1] - w[0] >= NODE_GAP - 1e-3, "{base}: {ys:?}");
            }
            assert!(ys.iter().all(|y| (lo..=hi).contains(y)), "{base}: {ys:?}");
        }
        // the real nodes of both hands, every finger bound
        let art = build_art();
        let clip = egui::Rangef::new(12.0, 1188.0);
        let nodes: Vec<Rect> = [(Rect::from_center_size(pos2(450.0, 120.0), Vec2::splat(240.0)), true), (Rect::from_center_size(pos2(750.0, 120.0), Vec2::splat(240.0)), false)]
            .into_iter()
            .flat_map(|(c, m)| label_layout(c, m, &art, all_widths(), clip).into_iter().map(|l| l.node))
            .collect();
        assert_eq!(nodes.len(), 10);
        for (i, a) in nodes.iter().enumerate() {
            for b in &nodes[i + 1..] {
                assert!(!a.intersects(*b), "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn no_label_line_crosses_another_finger() {
        let art = build_art();
        let canvas = Rect::from_min_size(pos2(300.0, 0.0), Vec2::splat(240.0));
        for mirror in [false, true] {
            let labels = label_layout(canvas, mirror, &art, all_widths(), egui::Rangef::new(0.0, 1200.0));
            assert_eq!(labels.len(), 5);
            for l in &labels {
                for w in l.line.windows(2) {
                    let n = ((w[1] - w[0]).length() / 0.25).ceil().max(1.0) as usize;
                    for k in 0..=n {
                        let p = w[0] + (w[1] - w[0]) * (k as f32 / n as f32);
                        let Some((x, y)) = dot_at(canvas, p, mirror) else { continue };
                        let i = y * CANVAS + x;
                        let r = art.region[i];
                        if art.lit[i] && (2..=6).contains(&r) {
                            assert_eq!(r, l.f as u8 + 2, "mirror {mirror}: the line of finger {} passes finger {} at {p:?}", l.f, r - 2);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn nodes_follow_the_finger_angle_and_the_thumb_sits_above() {
        let art = build_art();
        let canvas = Rect::from_min_size(pos2(300.0, 40.0), Vec2::splat(240.0));
        for mirror in [false, true] {
            let ls = label_layout(canvas, mirror, &art, all_widths(), egui::Rangef::new(0.0, 1200.0));
            let by = |f: usize| ls.iter().find(|l| l.f == f).cloned();
            let Some(thumb) = by(0) else { return assert!(by(0).is_some()) };
            assert_eq!(thumb.node.top(), canvas.top() + THUMB_TOP);
            assert!((thumb.node.center().x - thumb.tip.x).abs() < 0.5 && thumb.node.bottom() < thumb.tip.y, "{thumb:?}");
            assert_eq!(thumb.line, vec![thumb.tip, pos2(thumb.tip.x, thumb.node.bottom())], "straight up");
            let ys: Vec<f32> = (1..5).filter_map(by).map(|l| l.node.center().y).collect();
            assert_eq!(ys.len(), 4);
            assert!(ys.windows(2).all(|w| w[1] > w[0]), "index on top, pinky at the bottom: {ys:?}");
            for f in 1..5 {
                let Some(l) = by(f) else { continue };
                assert!((l.node.height() - NODE_H).abs() < 1e-3);
                let gutter = if mirror { canvas.left() - GUTTER } else { canvas.right() + GUTTER };
                assert_eq!(l.line.len(), 4);
                assert!(((l.line[1] - l.line[0]).length() - LEG).abs() < 0.01, "a 12 pt leg");
                assert_eq!((l.line[2].x, l.line[2].y), (gutter, l.line[1].y), "then horizontal to the gutter");
                if mirror {
                    assert_eq!(l.node.right(), canvas.left() - NODE_OUT);
                    assert!((l.line[3] - pos2(l.node.right(), l.node.center().y)).length() < 1e-3, "the last segment ends at the node's near edge");
                } else {
                    assert_eq!(l.node.left(), canvas.right() + NODE_OUT);
                    assert!((l.line[3] - pos2(l.node.left(), l.node.center().y)).length() < 1e-3, "the last segment ends at the node's near edge");
                }
            }
        }
        // only the fingers given a width get a node; a narrow content cuts it
        let mut w = [None; 5];
        w[2] = Some(200.0);
        let ls = label_layout(canvas, false, &art, w, egui::Rangef::new(0.0, 600.0));
        assert_eq!(ls.len(), 1);
        assert_eq!(ls[0].node.right(), 600.0);
    }

    #[test]
    fn a_selected_finger_stays_lit_when_the_camera_sees_it_folded() {
        let t = Color32::TRANSPARENT;
        assert_eq!(finger_colour(1, 1, true, true, false, Some(false), None), (HAND[1], HAND[1].gamma_multiply(0.45)));
        assert_eq!(finger_colour(0, 3, true, false, false, None, None).0, HAND[0], "no camera: full too");
        assert_eq!(finger_colour(1, 1, false, true, false, Some(false), None), (HAND[1].gamma_multiply(0.28), t));
        assert_eq!(finger_colour(1, 1, false, true, false, Some(true), None), (HAND[1], t));
        assert_eq!(finger_colour(1, 1, false, true, false, None, None), (HAND[1].gamma_multiply(0.75), t));
        assert_eq!(finger_colour(1, 2, false, false, false, Some(true), None), (white(0.92), t));
        assert_eq!(finger_colour(1, 1, true, true, true, Some(false), None), (Color32::WHITE, t), "playing wins");
        assert_eq!(finger_colour(1, 1, true, true, true, None, Some(0b01000)), (HAND[1], t), "the calibration's pose first");
        assert_eq!(finger_colour(1, 2, true, true, true, None, Some(0b01000)), (white(0.2), t));
    }

    #[test]
    fn lanes_grid_alphas() {
        for w in [3.0, 10.0] {
            assert_eq!(grid_alpha(0, w, true), Some(0.32));
            assert_eq!(grid_alpha(16, w, true), Some(0.32));
            assert_eq!(grid_alpha(4, w, true), Some(0.16), "beats at any width");
        }
        assert_eq!(grid_alpha(1, 6.0, true), Some(0.07));
        assert_eq!(grid_alpha(1, 5.9, true), None);
        // the piano roll keeps today's values
        assert_eq!(grid_alpha(16, 10.0, false), Some(0.20));
        assert_eq!(grid_alpha(4, 4.0, false), Some(0.06));
        assert_eq!(grid_alpha(4, 10.0, false), Some(0.09));
        assert_eq!(grid_alpha(1, 8.0, false), Some(0.04));
        assert_eq!(grid_alpha(1, 7.0, false), None);
    }

    const A_MINOR: Key = Key { root: 9, scale: Scale::Minor };
    const P: PointerButton = PointerButton::Primary;
    const S: PointerButton = PointerButton::Secondary;
    const NO: Modifiers = Modifiers::NONE;

    fn st(at: u16, len: u16, pitch: u8) -> Step {
        Step { at, len, pitch, vel: 80, ratchet: 1, nudge: 0 }
    }

    fn clip(bars: u8, steps: Vec<Step>, rows: Vec<u8>) -> Part {
        Part { id: 7, name: "Pattern".into(), channel: None, play: Play::Clip { bars, steps, rows }, mode: Mode::Gate, start: Start::Now }
    }

    fn am() -> Part {
        Part { id: 7, name: "Am".into(), channel: None, play: Play::Chords { degrees: vec![0], size: 3, octave: 4, vel: 96, strum: 0 }, mode: Mode::Gate, start: Start::Now }
    }

    fn walk_texts(s: &egui::Shape, out: &mut Vec<(String, Pos2)>) {
        match s {
            egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos + t.galley.rect.center().to_vec2())),
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk_texts(s, out)),
            _ => {}
        }
    }

    /// A headless window with one roll in a 1000x600 CentralPanel.
    struct H {
        ctx: Context,
        time: f64,
        part: Part,
        roll: Roll,
        undo: Option<Undo>,
        lanes: bool,
        geom: Option<Geom>,
        mods: Modifiers,
        last: Pos2,
        out: Option<FullOutput>,
    }

    impl H {
        fn new(part: Part, lanes: bool) -> H {
            let ctx = Context::default();
            crate::glass::style(&ctx);
            let mut h = H { ctx, time: 0.0, part, roll: Roll::default(), undo: None, lanes, geom: None, mods: NO, last: Pos2::ZERO, out: None };
            h.idle();
            h
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 1.0 / 60.0;
            let raw = RawInput { events, time: Some(self.time), modifiers: self.mods, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0))), ..Default::default() };
            let cx = RollCx { key: A_MINOR, color: HAND[1], playhead: None, lanes: self.lanes, drums: true, t: &EN };
            let (part, roll, undo) = (&mut self.part, &mut self.roll, &mut self.undo);
            let mut g = None;
            self.out = Some(self.ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    g = Some(roll_ui(ui, part, &cx, roll, undo));
                });
            }));
            if g.is_some() {
                self.geom = g;
            }
        }

        fn idle(&mut self) {
            self.frame(vec![]);
        }

        fn g(&self) -> Geom {
            self.geom.clone().unwrap_or_else(|| Geom::new(Pos2::ZERO, 100.0, 10.0, vec![60], 10.0, 16, true))
        }

        fn press(&mut self, p: Pos2, b: PointerButton, mods: Modifiers) {
            self.mods = mods;
            self.last = p;
            self.frame(vec![Event::PointerMoved(p), Event::PointerButton { pos: p, button: b, pressed: true, modifiers: mods }]);
        }

        fn move_to(&mut self, p: Pos2) {
            let a = self.last;
            for i in 1..=6 {
                self.frame(vec![Event::PointerMoved(a + (p - a) * (i as f32 / 6.0))]);
            }
            self.last = p;
        }

        fn release(&mut self, p: Pos2, b: PointerButton) {
            let mods = self.mods;
            self.frame(vec![Event::PointerMoved(p), Event::PointerButton { pos: p, button: b, pressed: false, modifiers: mods }]);
            self.mods = NO;
            self.last = p;
            self.idle();
        }

        fn click(&mut self, p: Pos2, b: PointerButton, mods: Modifiers) {
            self.press(p, b, mods);
            self.release(p, b);
        }

        /// A trackpad tap: press and release in one frame.
        fn tap(&mut self, p: Pos2, b: PointerButton) {
            let (down, up) = (Event::PointerButton { pos: p, button: b, pressed: true, modifiers: NO }, Event::PointerButton { pos: p, button: b, pressed: false, modifiers: NO });
            self.frame(vec![Event::PointerMoved(p), down, up]);
            self.last = p;
            self.idle();
        }

        fn drag(&mut self, a: Pos2, b: Pos2, button: PointerButton, mods: Modifiers) {
            self.press(a, button, mods);
            self.move_to(b);
            self.release(b, button);
        }

        /// The point at `step` (fractional) on the row of `pitch`.
        fn at(&self, step: f32, pitch: u8) -> Pos2 {
            let g = self.g();
            pos2(g.x_of_tick(step * TICKS), g.row_center(g.row_of(pitch).unwrap_or(0)))
        }

        fn steps(&self) -> Vec<Step> {
            match &self.part.play {
                Play::Clip { steps, .. } => steps.clone(),
                _ => Vec::new(),
            }
        }

        fn kept(&self) -> Vec<u8> {
            match &self.part.play {
                Play::Clip { rows, .. } => rows.clone(),
                _ => Vec::new(),
            }
        }

        fn texts(&self) -> Vec<(String, Pos2)> {
            let mut out = Vec::new();
            for c in self.out.iter().flat_map(|o| &o.shapes) {
                walk_texts(&c.shape, &mut out);
            }
            out
        }

        fn find(&self, needle: &str) -> Option<Pos2> {
            self.texts().into_iter().find(|(s, _)| s == needle).map(|(_, p)| p)
        }
    }

    #[test]
    fn click_on_an_empty_cell_adds_a_note_with_the_last_length() {
        let mut h = H::new(clip(1, vec![], vec![]), false);
        let top = h.g().rows[0];
        h.click(h.at(3.5, top), P, NO);
        let s = h.steps();
        assert_eq!(s.len(), 1, "{s:?}");
        assert!(s[0].at == 3 && s[0].len == 1 && s[0].vel == 100 && s[0].pitch == top, "{s:?}");
        // resize it to 4 steps, then a new note takes that length
        let p = h.at(4.0, top) - vec2(2.0, 0.0);
        let cw = h.g().cell_w;
        h.drag(p, p + vec2(3.0 * cw, 0.0), P, NO);
        assert_eq!(h.steps()[0].len, 4, "{:?}", h.steps());
        let row0 = h.g().rows[0];
        h.click(h.at(9.5, row0), P, NO);
        let s = h.steps();
        assert!(s.len() == 2 && s[1].at == 9 && s[1].len == 4, "{s:?}");
    }

    #[test]
    fn dragging_a_note_snaps_to_steps() {
        let base = clip(1, vec![st(4, 1, 60)], vec![]);
        let mut h = H::new(base.clone(), false);
        let cw = h.g().cell_w;
        let a = h.at(4.5, 60);
        h.drag(a, a + vec2(0.4 * cw, 0.0), P, NO);
        assert_eq!(h.steps(), vec![st(4, 1, 60)], "0.4 cell stays");
        let a = h.at(4.5, 60);
        h.drag(a, a + vec2(1.6 * cw, 0.0), P, NO);
        assert!(h.steps()[0].at == 6 && h.steps()[0].nudge == 0, "{:?}", h.steps());
        let a = h.at(6.5, 60);
        let rh = h.g().row_h;
        h.drag(a, a + vec2(0.0, rh), P, NO);
        assert_eq!(h.steps()[0].pitch, 59, "one row down");
        let mut off = st(4, 1, 60);
        off.nudge = 12;
        let mut h = H::new(clip(1, vec![off], vec![]), false);
        let a = h.at(4.9, 60);
        h.drag(a, a + vec2(0.0, -2.0 * rh), P, NO);
        let s = h.steps()[0];
        assert!(s.nudge == 12 && s.at == 4 && s.pitch == 62, "{s:?}");
    }

    #[test]
    fn alt_drag_moves_in_ticks_and_never_wraps() {
        let base = clip(1, vec![st(4, 1, 60)], vec![]);
        for (dx, want) in [(0.5f32, Some((4u16, 12u8))), (1.25, Some((5, 6))), (-20.0, Some((0, 0))), (40.0, None)] {
            let mut h = H::new(base.clone(), false);
            let cw = h.g().cell_w;
            let a = h.at(4.3, 60);
            h.drag(a, a + vec2(dx * cw, 0.0), P, Modifiers::ALT);
            let s = h.steps()[0];
            // inside the loop by whole steps; a nudge may hang over the
            // end by less than a step, as a strummed chord's notes do
            assert!(s.nudge <= 23 && s.at + s.len <= 16, "{s:?}");
            if let Some((at, nudge)) = want {
                assert!(s.at == at && s.nudge == nudge, "dx {dx}: {s:?}");
            }
        }
    }

    #[test]
    fn the_right_edge_resizes_in_whole_steps() {
        for mods in [NO, Modifiers::ALT] {
            let mut h = H::new(clip(1, vec![st(2, 1, 60)], vec![]), false);
            let cw = h.g().cell_w;
            let p = h.at(3.0, 60) - vec2(2.0, 0.0);
            h.drag(p, p + vec2(2.6 * cw, 0.0), P, mods);
            assert_eq!(h.steps()[0].len, 4, "{:?}", h.steps());
            let p = h.at(6.0, 60) - vec2(2.0, 0.0);
            h.drag(p, p + vec2(40.0 * cw, 0.0), P, mods);
            assert!(h.steps()[0].len == 14 && h.steps()[0].at == 2, "clamped at the loop end: {:?}", h.steps());
        }
        // 1 bar: 7 pt inside the end moves, 5 pt resizes
        let mut h = H::new(clip(1, vec![st(2, 1, 60)], vec![]), true);
        let cw = h.g().cell_w;
        let p = h.at(3.0, 60) - vec2(7.0, 0.0);
        h.drag(p, p + vec2(1.6 * cw, 0.0), P, NO);
        assert!(h.steps()[0].at == 4 && h.steps()[0].len == 1, "moved: {:?}", h.steps());
        let p = h.at(5.0, 60) - vec2(5.0, 0.0);
        h.drag(p, p + vec2(1.6 * cw, 0.0), P, NO);
        assert!(h.steps()[0].at == 4 && h.steps()[0].len == 3, "resized: {:?}", h.steps());
        // 8 bars: just past the end with no neighbour still resizes
        let mut h = H::new(clip(8, vec![st(10, 1, 60)], vec![]), true);
        let cw = h.g().cell_w;
        let p = h.at(11.0, 60) + vec2(1.5, 0.0);
        h.drag(p, p + vec2(2.0 * cw, 0.0), P, NO);
        assert!(h.steps()[0].at == 10 && h.steps()[0].len == 3, "{:?}", h.steps());
        // back to back: the neighbour's start belongs to the neighbour
        let mut h = H::new(clip(8, vec![st(2, 1, 60), st(3, 1, 60)], vec![]), true);
        let cw = h.g().cell_w;
        let p = h.at(3.0, 60) + vec2(1.0, 0.0);
        h.drag(p, p + vec2(2.0 * cw, 0.0), P, NO);
        let s = h.steps();
        assert!(s[0] == st(2, 1, 60) && s[1].at == 5 && s[1].len == 1, "{s:?}");
    }

    #[test]
    fn right_click_deletes_and_right_drag_erases_a_run() {
        let run = clip(1, vec![st(2, 1, 60), st(3, 1, 60), st(4, 1, 60)], vec![]);
        let mut h = H::new(run.clone(), false);
        h.drag(h.at(2.5, 60), h.at(4.5, 60), S, NO);
        assert!(h.steps().is_empty(), "{:?}", h.steps());
        let mut h = H::new(run.clone(), false);
        h.click(h.at(9.5, 60), S, NO);
        assert_eq!(h.steps().len(), 3, "a right click on empty changes nothing");
        h.click(h.at(3.5, 60), S, NO);
        assert_eq!(h.steps().len(), 2, "a right click on a note deletes it");
        #[cfg(target_os = "macos")]
        for mods in [Modifiers { command: true, ..Default::default() }, Modifiers::CTRL] {
            let mut h = H::new(run.clone(), false);
            h.click(h.at(2.5, 60), P, mods);
            assert_eq!(h.steps().len(), 2, "ctrl-click deletes ({mods:?})");
        }
    }

    #[test]
    fn a_long_right_press_does_not_paint() {
        let mut h = H::new(clip(1, vec![], vec![]), false);
        let a = h.at(5.5, 64);
        h.press(a, S, NO);
        for i in 0..60 {
            h.frame(vec![Event::PointerMoved(a + vec2(8.0 * i as f32 / 60.0, 0.0))]);
        }
        h.release(a + vec2(8.0, 0.0), S);
        assert!(h.steps().is_empty(), "{:?}", h.steps());
    }

    #[test]
    fn a_note_dragged_against_the_top_row_stays_there() {
        let mut h = H::new(clip(1, vec![st(4, 1, 60)], vec![]), false);
        let g = h.g();
        let top = g.rows[0];
        let a = h.at(4.5, 60);
        h.press(a, P, NO);
        let up = pos2(a.x, g.grid.top() - 40.0);
        h.move_to(up);
        for _ in 0..30 {
            h.frame(vec![Event::PointerMoved(up)]);
        }
        assert_eq!(h.steps()[0].pitch, top, "the frozen top row");
        assert_eq!(h.g().rows[0], top, "rows stay while the button is down");
        h.release(up, P);
        assert_eq!(h.steps()[0].pitch, top);
        assert_eq!(h.g().rows, g.rows, "and after the release: an edit never moves the rows");
    }

    #[test]
    fn the_lane_label_drags_and_scrolls_the_lane_note() {
        let hats = clip(1, vec![st(0, 1, 42), st(2, 1, 42), st(4, 1, 42)], vec![42]);
        let mut h = H::new(hats, true);
        let lab = |h: &H, pitch: u8| {
            let g = h.g();
            pos2(g.label.center().x, g.row_center(g.row_of(pitch).unwrap_or(0)))
        };
        let a = lab(&h, 42);
        h.drag(a, a - vec2(0.0, 18.0), P, NO);
        assert!(h.steps().iter().all(|s| s.pitch == 45), "{:?}", h.steps());
        assert!(h.kept().contains(&45) && !h.kept().contains(&42), "{:?}", h.kept());
        // a taken note is skipped
        let mut h = H::new(clip(1, vec![st(0, 1, 42), st(1, 1, 44)], vec![]), true);
        let a = lab(&h, 42);
        h.drag(a, a - vec2(0.0, 12.0), P, NO);
        let s = h.steps();
        assert!(s[0].pitch == 45 && s[1].pitch == 44, "{s:?}");
        // the wheel: a line is one semitone, 14 points of a trackpad too
        let mut h = H::new(clip(1, vec![st(0, 1, 42)], vec![]), true);
        let a = lab(&h, 42);
        h.frame(vec![Event::PointerMoved(a)]);
        let wheel = |unit, y: f32| Event::MouseWheel { unit, delta: vec2(0.0, y), modifiers: NO };
        h.frame(vec![wheel(MouseWheelUnit::Line, 1.0)]);
        assert_eq!(h.steps()[0].pitch, 43);
        assert_eq!(h.ctx.input(|i| i.smooth_scroll_delta), Vec2::ZERO, "the wheel stays in the label");
        h.idle();
        h.frame(vec![wheel(MouseWheelUnit::Point, 10.0)]);
        assert_eq!(h.steps()[0].pitch, 43, "10 pt is not a step yet");
        h.frame(vec![wheel(MouseWheelUnit::Point, 4.0)]);
        assert_eq!(h.steps()[0].pitch, 44);
        // the default C5 lane of an empty clip moves too
        let mut h = H::new(clip(1, vec![], vec![]), true);
        assert_eq!(h.g().rows, vec![60]);
        let a = lab(&h, 60);
        h.drag(a, a - vec2(0.0, 6.0), P, NO);
        assert_eq!(h.kept(), vec![61]);
        assert_eq!(h.g().rows, vec![61]);
    }

    #[test]
    fn a_click_on_the_lane_label_opens_its_popover() {
        let mut h = H::new(clip(1, vec![st(0, 1, 42), st(4, 1, 42)], vec![42]), true);
        let g = h.g();
        let a = pos2(g.label.center().x, g.row_center(0));
        h.click(a, P, NO);
        assert!(h.ctx.memory(|m| m.is_popup_open(Id::new(("lane-pop", 7u64)))));
        h.idle();
        assert!(h.find(EN.f_remove_row).is_some(), "{:?}", h.texts());
        assert!(h.find("Closed hat F#3").is_some(), "GM names on a drum lane");
        let Some(root) = h.find(EN.f_sampler_root) else { return assert!(h.find(EN.f_sampler_root).is_some()) };
        h.click(root, P, NO);
        assert!(h.steps().iter().all(|s| s.pitch == 60), "{:?}", h.steps());
        assert!(!h.ctx.memory(|m| m.any_popup_open()));
        // with the popover open, a press on the grid only closes it
        let a = pos2(h.g().label.center().x, h.g().row_center(0));
        h.click(a, P, NO);
        h.idle();
        assert!(h.ctx.memory(|m| m.any_popup_open()));
        let n = h.steps().len();
        h.click(h.at(9.5, 60), P, NO);
        assert_eq!(h.steps().len(), n, "no note added");
        assert!(!h.ctx.memory(|m| m.any_popup_open()));
    }

    #[test]
    fn event_lane_sets_velocity_and_rolls() {
        let four = clip(1, vec![st(0, 1, 60), st(4, 1, 60), st(8, 1, 60), st(12, 1, 60)], vec![]);
        let mut h = H::new(four.clone(), false);
        let Some(e) = h.g().event else { return assert!(h.g().event.is_some()) };
        let y = e.top() + 1.0;
        let g = h.g();
        h.drag(pos2(g.x_of_tick(0.0), y), pos2(g.x_of_tick(12.0 * TICKS), y), P, NO);
        assert!(h.steps().iter().all(|s| s.vel == 127), "{:?}", h.steps());
        // lanes mode: only the active lane (the top one)
        let mut h = H::new(clip(1, vec![st(0, 1, 42), st(0, 1, 38)], vec![]), true);
        let Some(e) = h.g().event else { return assert!(h.g().event.is_some()) };
        let g = h.g();
        h.click(pos2(g.x_of_tick(0.0), e.top() + 1.0), P, NO);
        let s = h.steps();
        assert!(s[0].vel == 127 && s[1].vel == 80, "{s:?}");
        // rolls: the bottom is one hit, the top eight
        let mut h = H::new(four, false);
        h.roll.lane = EventLane::Rolls;
        let Some(e) = h.g().event else { return assert!(h.g().event.is_some()) };
        let g = h.g();
        h.click(pos2(g.x_of_tick(4.0 * TICKS), e.bottom() - 1.0), P, NO);
        h.click(pos2(g.x_of_tick(8.0 * TICKS), e.top() + 1.0), P, NO);
        let s = h.steps();
        assert!(s[1].ratchet == 1 && s[2].ratchet == 8 && s[0].ratchet == 1, "{s:?}");
    }

    #[test]
    fn moving_a_lanes_last_note_keeps_the_lane() {
        let mut h = H::new(clip(1, vec![st(0, 1, 42), st(4, 1, 38)], vec![]), true);
        h.drag(h.at(0.5, 42), h.at(8.5, 38), P, NO);
        let s = h.steps();
        assert!(s.iter().all(|s| s.pitch == 38) && s.iter().any(|s| s.at == 8), "{s:?}");
        assert!(h.kept().contains(&42), "{:?}", h.kept());
        assert!(h.g().rows.contains(&42));
    }

    #[test]
    fn editing_a_chord_note_makes_a_pattern_and_back_restores_it() {
        let mut h = H::new(am(), false);
        let notes = view_clip(&h.part, A_MINOR).steps.len();
        h.click(h.at(5.5, 59), P, NO);
        assert!(matches!(h.part.play, Play::Clip { .. }), "{:?}", h.part.play);
        assert!(h.part.name == "Am" && h.part.start == Start::Bar);
        assert_eq!(h.steps().len(), notes + 1);
        assert!(back(&mut h.part, &mut h.undo));
        assert_eq!(h.part, am());
        assert!(h.undo.is_none());
        // the velocity lane on a chord: still a chord, written on release
        let mut h = H::new(am(), false);
        h.idle();
        let Some(e) = h.g().event else { return assert!(h.g().event.is_some()) };
        let g = h.g();
        let a = pos2(g.x_of_tick(0.0), e.top() + 1.0);
        h.press(a, P, NO);
        h.move_to(a + vec2(4.0, 0.0));
        assert!(matches!(h.part.play, Play::Chords { vel: 96, .. }), "{:?}", h.part.play);
        h.release(a + vec2(4.0, 0.0), P);
        assert!(matches!(h.part.play, Play::Chords { vel: 127, .. }), "{:?}", h.part.play);
    }

    #[test]
    fn a_chord_moved_by_its_fields_brings_the_rows_the_wheel_does_not_snap_back() {
        let mut h = H::new(am(), false);
        let rows = h.g().rows.clone();
        // the wheel: five notches up, the chord's low notes leave the view
        let over = pos2(h.g().grid.center().x, h.g().grid.top() + 10.0);
        h.frame(vec![Event::PointerMoved(over)]);
        for _ in 0..5 {
            h.frame(vec![Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, 1.0), modifiers: NO }]);
        }
        h.idle();
        assert_eq!(h.g().rows[0], rows[0] + 10, "scrolled, not snapped back");
        // the octave field: the chord moves an octave up, the rows follow
        if let Play::Chords { octave, .. } = &mut h.part.play {
            *octave = 6;
        }
        h.frame(vec![Event::PointerMoved(pos2(5.0, 5.0))]);
        let notes = view_clip(&h.part, A_MINOR).steps;
        assert!(notes.iter().all(|s| h.g().rows.contains(&s.pitch)), "{:?} in {:?}", notes, h.g().rows);
    }

    #[test]
    fn kind_switches_keep_notes() {
        let mut p = am();
        let mut undo = None;
        let notes = view_clip(&p, A_MINOR).steps;
        set_kind(&mut p, Kind3::Pattern, A_MINOR, None, None, &mut undo);
        assert!(matches!(&p.play, Play::Clip { steps, .. } if *steps == notes));
        let pat = clip(1, vec![st(0, 1, 42), st(4, 1, 38)], vec![42, 38]);
        let mut p = pat.clone();
        set_kind(&mut p, Kind3::Held, A_MINOR, None, Some(42), &mut undo);
        assert_eq!(p.play, Play::Note { pitch: 42, vel: 100 });
        assert_eq!(p.name, "F#3");
        set_kind(&mut p, Kind3::Pattern, A_MINOR, None, None, &mut undo);
        assert_eq!(p, pat, "pattern, held note, pattern");
        set_kind(&mut p, Kind3::Held, A_MINOR, None, None, &mut undo);
        assert_eq!(p.play, Play::Note { pitch: 38, vel: 100 }, "the lowest note without an active lane");
        assert!(back(&mut p, &mut undo));
        assert_eq!(p, pat, "Back too");
        // a lane drag on a held note changes the note, not the kind
        let held = Part { id: 7, name: "C5".into(), channel: None, play: Play::Note { pitch: 60, vel: 100 }, mode: Mode::Gate, start: Start::Now };
        let mut h = H::new(held, false);
        let g = h.g();
        let a = pos2(g.label.center().x, g.row_center(0));
        h.drag(a, a - vec2(0.0, 6.0), P, NO);
        assert_eq!(h.part.play, Play::Note { pitch: 61, vel: 100 });
        assert_eq!(h.part.name, "C#5");
    }

    #[test]
    fn ticks_and_points_round_trip() {
        for w in [4.0f32, 7.3, 12.0, 40.0, 55.5, 60.0] {
            let g = Geom::new(pos2(10.0, 20.0), 96.0 + w * 16.0, 96.0, vec![60], 18.0, 16, true);
            assert!((g.cell_w - w).abs() < 1e-3);
            for t in [0.0f32, 1.0, 23.0, 24.0, 100.5, 383.0] {
                assert!((g.tick_at(g.x_of_tick(t)) - t).abs() < 1e-3, "{w} {t}");
            }
        }
    }

    #[test]
    fn roll_rows_auto_fit() {
        assert_eq!(auto_top(&[], 16), 67);
        assert_eq!(roll_rows(67).last(), Some(&53), "G5 down to F4 around C5");
        let span: Vec<Step> = (57..=64).map(|p| st(0, 1, p)).collect();
        assert_eq!(auto_top(&span, 16), 67);
        let wide = vec![st(0, 1, 40), st(1, 1, 69)];
        let top = auto_top(&wide, 16);
        assert!(roll_rows(top).contains(&69), "{top}");
        for top in [0u8, 14, 71, 127] {
            assert_eq!(roll_rows(top).len(), usize::from(ROLL_VIS));
        }
        // a step past the loop end does not count
        assert_eq!(auto_top(&[st(20, 1, 100)], 16), 67);
    }

    #[test]
    fn a_tap_in_one_frame_clicks_too() {
        let mut h = H::new(clip(1, vec![st(2, 1, 60)], vec![]), false);
        h.tap(h.at(9.5, 60), P);
        assert_eq!(h.steps().len(), 2, "a tap adds: {:?}", h.steps());
        h.tap(h.at(2.5, 60), S);
        assert!(h.steps().len() == 1 && h.steps()[0].at == 9, "a right tap deletes: {:?}", h.steps());
        let mut h = H::new(clip(1, vec![st(0, 1, 42)], vec![]), true);
        let g = h.g();
        h.tap(pos2(g.label.center().x, g.row_center(0)), P);
        assert!(h.ctx.memory(|m| m.is_popup_open(Id::new(("lane-pop", 7u64)))), "a tap on a lane name opens its list");
    }

    #[test]
    fn a_lane_drag_shows_the_note_it_reached() {
        let mut h = H::new(clip(1, vec![st(0, 1, 42), st(2, 1, 42)], vec![42]), true);
        let g = h.g();
        let a = pos2(g.label.center().x, g.row_center(0));
        h.press(a, P, NO);
        h.move_to(a - vec2(0.0, 18.0));
        assert!(h.find("A3").is_some() && h.find("Closed hat").is_none(), "{:?}", h.texts());
        assert_eq!(h.g().rows, vec![45], "the frozen row carries the reached note");
        h.release(a - vec2(0.0, 18.0), P);
        assert!(h.steps().iter().all(|s| s.pitch == 45), "{:?}", h.steps());
    }

    #[test]
    fn a_pointer_leaving_the_window_keeps_the_drag() {
        let mut h = H::new(clip(1, vec![st(4, 1, 60)], vec![]), false);
        let (a, b) = (h.at(4.5, 60), h.at(8.5, 60));
        h.press(a, P, NO);
        h.move_to(b);
        assert_eq!(h.steps()[0].at, 8);
        h.frame(vec![Event::PointerGone]);
        assert_eq!(h.steps()[0].at, 8, "no jump back to the press");
        h.release(b, P);
        assert_eq!(h.steps()[0].at, 8);
    }

    #[test]
    fn erasing_a_lanes_last_note_keeps_the_lane() {
        let mut h = H::new(clip(1, vec![st(0, 1, 42), st(4, 1, 38)], vec![]), true);
        h.click(h.at(0.5, 42), S, NO);
        assert_eq!(h.steps(), vec![st(4, 1, 38)]);
        assert!(h.kept().contains(&42) && h.g().rows.contains(&42), "{:?}", h.kept());
        // the chromatic roll keeps no rows
        let mut h = H::new(clip(1, vec![st(0, 1, 60)], vec![]), false);
        h.click(h.at(0.5, 60), S, NO);
        assert!(h.steps().is_empty() && h.kept().is_empty(), "{:?}", h.kept());
    }

    /// The whole section as the plugin window runs it: a ScrollArea over a
    /// glass panel in a 1000x720 window, Fingers::ui inside, the store in a
    /// real Writer. Input arrives as egui-baseview sends it: the move that
    /// brings the pointer in on one frame, the press alone on the next,
    /// moves while the button is held, the release alone; the modifiers
    /// stay in RawInput from the mouse events.
    struct Win {
        ctx: Context,
        time: f64,
        f: Fingers,
        w: Arc<Writer>,
        id: u64,
        mods: Modifiers,
        at: Pos2,
        out: Option<FullOutput>,
        /// The page's scroll offset and content height after the frame.
        page: (f32, f32),
        /// The window's height (the page's viewport).
        height: f32,
        /// Some: the window runs the wall editor for this side with these
        /// FL channels instead of the section.
        wall: Option<(Side, Vec<fl::ChannelRow>)>,
        /// What the chat field passes the section.
        routing: Routing,
        /// The window's width.
        width: f32,
    }

    impl Win {
        fn new(play: Play, piano: bool) -> Win {
            let ctx = Context::default();
            crate::glass::style(&ctx);
            let w = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
            let mut store = w.player.store();
            store.key = A_MINOR;
            let slot = Slot::finger(1, 1);
            let id = store.add("Part", play, None);
            store.bind(&slot.signal(), slot.trigger(), id);
            w.update_midi(store);
            let f = Fingers { piano, ..Fingers::default() };
            let mut win = Win { ctx, time: 0.0, f, w, id, mods: NO, at: pos2(2.0, 2.0), out: None, page: (0.0, 0.0), height: 720.0, wall: None, routing: Routing::default(), width: 1000.0 };
            for _ in 0..4 {
                win.frame(vec![]);
            }
            win
        }

        /// The wall editor for `side` with nothing on any wall yet.
        fn wall(side: Side, channels: Vec<fl::ChannelRow>) -> Win {
            let ctx = Context::default();
            crate::glass::style(&ctx);
            let w = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
            let mut win = Win { ctx, time: 0.0, f: Fingers::default(), w, id: 0, mods: NO, at: pos2(2.0, 2.0), out: None, page: (0.0, 0.0), height: 720.0, wall: Some((side, channels)), routing: Routing::default(), width: 1000.0 };
            for _ in 0..4 {
                win.frame(vec![]);
            }
            win
        }

        /// The part on the wall now, if any.
        fn wall_part(&self, side: Side) -> Option<u64> {
            match self.w.pong.setup().wall(side) {
                Action::Part { id } => Some(*id),
                _ => None,
            }
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 1.0 / 60.0;
            let raw = RawInput { events, time: Some(self.time), modifiers: self.mods, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(self.width, self.height))), ..Default::default() };
            let (f, w, wall, routing) = (&mut self.f, &self.w, &self.wall, &self.routing);
            let mut page = self.page;
            self.out = Some(self.ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let o = egui::ScrollArea::vertical().auto_shrink([false, false]).drag_to_scroll(false).show(ui, |ui| {
                        // the old glass panel's 1 pt edge and margins, so the
                        // measured positions stay as they were
                        let edge = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
                        let margin = egui::Margin { left: 10, right: 10, top: 0, bottom: 10 };
                        egui::Frame::NONE.stroke(edge).inner_margin(margin).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            match wall {
                                Some((side, rows)) => f.wall_body(ui, w, *side, &EN, false, rows, !rows.is_empty(), None),
                                None => f.body(ui, w, &EN, false, routing, &[], false, None),
                            }
                        });
                    });
                    page = (o.state.offset.y, o.content_size.y);
                });
            }));
            self.page = page;
        }

        fn wheel(&mut self, p: Pos2, lines: f32) {
            self.hover(p);
            let e = Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, lines), modifiers: NO };
            self.frame(vec![e]);
            for _ in 0..20 {
                self.frame(vec![]);
            }
        }

        fn texts(&self) -> Vec<(String, Pos2)> {
            let mut out = Vec::new();
            for c in self.out.iter().flat_map(|o| &o.shapes) {
                walk_texts(&c.shape, &mut out);
            }
            out
        }

        fn click_text(&mut self, needle: &str) -> bool {
            let Some(p) = self.texts().into_iter().find(|(s, _)| s == needle).map(|(_, p)| p) else { return false };
            self.press(p, NO);
            self.release();
            true
        }

        fn part(&self) -> Option<Part> {
            self.w.player.store().get(self.id).cloned()
        }

        fn steps(&self) -> Vec<Step> {
            match self.part().map(|p| p.play) {
                Some(Play::Clip { steps, .. }) => steps,
                _ => Vec::new(),
            }
        }

        fn g(&self) -> Geom {
            self.f.geom.clone().unwrap_or_else(|| Geom::new(Pos2::ZERO, 100.0, 10.0, vec![60], 10.0, 16, true))
        }

        fn at(&self, step: f32, pitch: u8) -> Pos2 {
            let g = self.g();
            pos2(g.x_of_tick(step * TICKS), g.row_center(g.row_of(pitch).unwrap_or(0)))
        }

        fn hover(&mut self, p: Pos2) {
            self.at = p;
            self.frame(vec![Event::PointerMoved(p)]);
        }

        fn press(&mut self, p: Pos2, mods: Modifiers) {
            self.mods = mods;
            self.hover(p);
            self.frame(vec![Event::PointerButton { pos: p, button: P, pressed: true, modifiers: mods }]);
        }

        /// 3 pt per frame, as a hand moves a mouse.
        fn move_to(&mut self, p: Pos2) {
            let a = self.at;
            let n = ((p - a).length() / 3.0).ceil().max(1.0) as usize;
            for i in 1..=n {
                self.at = a + (p - a) * (i as f32 / n as f32);
                self.frame(vec![Event::PointerMoved(self.at)]);
            }
        }

        fn release(&mut self) {
            let mods = self.mods;
            self.frame(vec![Event::PointerButton { pos: self.at, button: P, pressed: false, modifiers: mods }]);
            self.mods = NO;
            self.frame(vec![]);
        }

        fn drag(&mut self, a: Pos2, b: Pos2, mods: Modifiers) {
            self.press(a, mods);
            self.move_to(b);
            self.release();
        }
    }

    #[test]
    fn in_the_window_a_pattern_note_drags_in_time_and_pitch() {
        for piano in [true, false] {
            let mut w = Win::new(Play::Clip { bars: 1, steps: vec![st(4, 1, 60), st(0, 1, 64)], rows: vec![] }, piano);
            let rows = w.g().rows.clone();
            let cw = w.g().cell_w;
            let a = w.at(4.5, 60);
            w.drag(a, a + vec2(3.0 * cw, 0.0), NO);
            assert!(w.steps().iter().any(|s| s.at == 7 && s.pitch == 60), "piano {piano}: {:?}", w.steps());
            if piano {
                let a = w.at(7.5, 60);
                let rh = w.g().row_h;
                w.drag(a, a - vec2(0.0, 2.0 * rh), NO);
                assert!(w.steps().iter().any(|s| s.at == 7 && s.pitch == 62), "{:?}", w.steps());
                assert_eq!(w.g().rows, rows, "the rows stay after the edits");
            }
        }
    }

    #[test]
    fn in_the_window_a_drag_works_with_the_page_scrolled() {
        let mut w = Win::new(Play::Clip { bars: 1, steps: vec![st(4, 1, 60)], rows: vec![] }, true);
        w.height = 480.0;
        // the page scrolled down by the wheel over the hands
        w.wheel(pos2(500.0, 60.0), -20.0);
        assert!(w.page.0 > 0.0, "{:?}", w.page);
        let (cw, rh) = (w.g().cell_w, w.g().row_h);
        let a = w.at(4.5, 60);
        assert!(a.y > 0.0 && a.y < w.height, "the note shows: {a:?}");
        w.drag(a, a + vec2(2.0 * cw, -rh), NO);
        assert_eq!(w.steps(), vec![Step { at: 6, pitch: 61, ..st(4, 1, 60) }]);
        let a = w.at(6.5, 61);
        w.drag(a, a + vec2(0.5 * cw, 0.0), Modifiers::ALT);
        assert!(w.steps()[0].at == 6 && w.steps()[0].nudge == 12, "{:?}", w.steps());
    }

    #[test]
    fn in_the_window_a_chord_note_drags_and_the_drag_survives_the_conversion() {
        let mut w = Win::new(Play::Chords { degrees: vec![0], size: 3, octave: 4, vel: 96, strum: 0 }, false);
        let notes = view_clip(&w.part().unwrap_or_else(am), A_MINOR).steps;
        let Some(s) = notes.first().copied() else { return assert!(!notes.is_empty()) };
        let rows = w.g().rows.clone();
        let (cw, rh) = (w.g().cell_w, w.g().row_h);
        let a = w.at(s.at as f32 + 2.5, s.pitch);
        w.press(a, NO);
        // up two rows: the first change turns the chord into a pattern
        w.move_to(a - vec2(0.0, 2.0 * rh));
        assert!(matches!(w.part().map(|p| p.play), Some(Play::Clip { .. })), "{:?}", w.part());
        // the same press goes on: three steps right; the whole-bar note
        // ends at the loop end, and gets its steps back on the way back
        w.move_to(a + vec2(3.0 * cw, -2.0 * rh));
        let moved = |w: &Win| w.steps().iter().find(|x| x.pitch == s.pitch + 2).copied();
        assert!(moved(&w).is_some_and(|m| m.at == s.at + 3 && m.len == 13), "{:?}", w.steps());
        w.move_to(a - vec2(0.0, 2.0 * rh));
        assert!(moved(&w).is_some_and(|m| m.at == s.at && m.len == 16), "{:?}", w.steps());
        w.move_to(a + vec2(3.0 * cw, -2.0 * rh));
        w.release();
        let moved = w.steps().iter().find(|x| x.pitch == s.pitch + 2).copied();
        assert!(moved.is_some_and(|m| m.at == s.at + 3), "{:?}", w.steps());
        assert_eq!(w.steps().len(), notes.len());
        assert_eq!(w.g().rows, rows, "the rows stay");
    }

    #[test]
    fn in_the_window_an_option_drag_nudges_a_chord_note() {
        let mut w = Win::new(Play::Chords { degrees: vec![0], size: 3, octave: 4, vel: 96, strum: 0 }, false);
        let notes = view_clip(&w.part().unwrap_or_else(am), A_MINOR).steps;
        let Some(s) = notes.first().copied() else { return assert!(!notes.is_empty()) };
        let cw = w.g().cell_w;
        let a = w.at(6.5, s.pitch);
        // a quarter step later, continuously with the mouse
        w.drag(a, a + vec2(0.25 * cw, 0.0), Modifiers::ALT);
        let m = w.steps().into_iter().find(|x| x.pitch == s.pitch);
        assert!(m.is_some_and(|m| m.at == 0 && m.nudge == 6 && m.len == 16), "{:?}", w.steps());
        assert_eq!(w.steps().len(), notes.len(), "the other chord notes stay");
    }

    #[test]
    fn in_the_window_an_option_drag_moves_in_ticks() {
        let mut w = Win::new(Play::Clip { bars: 1, steps: vec![st(4, 1, 60)], rows: vec![] }, true);
        let cw = w.g().cell_w;
        let a = w.at(4.4, 60);
        w.drag(a, a + vec2(0.5 * cw, 0.0), Modifiers::ALT);
        let s = w.steps();
        assert!(s.len() == 1 && s[0].at == 4 && s[0].nudge == 12, "{s:?}");
        // and back, continuously: a quarter step left
        let a = w.at(4.9, 60);
        w.drag(a, a - vec2(0.25 * cw, 0.0), Modifiers::ALT);
        let s = w.steps();
        assert!(s.len() == 1 && s[0].at == 4 && s[0].nudge == 6, "{s:?}");
    }

    #[test]
    fn adding_notes_never_moves_the_roll() {
        // a chord: the click above it makes it a pattern; rows and the
        // roll's place on the page stay, so the next note goes right above
        let mut w = Win::new(Play::Chords { degrees: vec![0], size: 3, octave: 4, vel: 96, strum: 0 }, false);
        let before = w.g();
        let top = before.rows[0];
        let p = w.at(8.5, top);
        w.press(p, NO);
        w.release();
        assert!(matches!(w.part().map(|p| p.play), Some(Play::Clip { .. })), "{:?}", w.part());
        assert!(w.steps().iter().any(|s| s.pitch == top && s.at == 8), "{:?}", w.steps());
        for _ in 0..10 {
            w.frame(vec![]);
        }
        let after = w.g();
        assert_eq!(after.rows, before.rows, "rows stay");
        assert_eq!(after.block.min, before.block.min, "the roll stays on the page");
        // a pattern in the roll: notes at the top and bottom rows
        let mut w = Win::new(Play::Clip { bars: 1, steps: vec![st(0, 1, 60)], rows: vec![] }, true);
        let before = w.g();
        for (step, row) in [(2.5, 0), (4.5, before.rows.len() - 1), (6.5, 1)] {
            let p = w.at(step, before.rows[row]);
            w.press(p, NO);
            w.release();
        }
        assert_eq!(w.steps().len(), 4, "{:?}", w.steps());
        assert_eq!(w.g().rows, before.rows);
        assert_eq!(w.g().block.min, before.block.min);
        // a double-click on the keys fits the notes, opening another part
        // fits too
        let k = pos2(w.g().label.center().x, w.g().row_center(3));
        w.hover(k);
        for _ in 0..2 {
            w.frame(vec![Event::PointerButton { pos: k, button: P, pressed: true, modifiers: NO }]);
            w.frame(vec![Event::PointerButton { pos: k, button: P, pressed: false, modifiers: NO }]);
        }
        w.frame(vec![]);
        assert_eq!(w.g().rows[0], auto_top(&w.steps(), 16));
    }

    #[test]
    fn the_wheel_scrolls_the_roll_not_the_page_and_the_fold_gives_it_back() {
        let mut w = Win::new(Play::Clip { bars: 1, steps: vec![st(0, 1, 60)], rows: vec![] }, true);
        // the plugin's sections get about this much under the camera
        w.height = 480.0;
        for _ in 0..3 {
            w.frame(vec![]);
        }
        let g = w.g();
        let top = g.rows[0];
        assert!(w.page.1 > w.height && g.grid.top() + 30.0 < w.height, "the page scrolls and the roll shows: {:?} {:?}", w.page, g.grid);
        let over = pos2(g.grid.center().x, g.grid.top() + 20.0);
        w.wheel(over, 1.0);
        assert_eq!(w.g().rows[0], top + 2, "a notch up shows two higher notes");
        assert_eq!(w.page.0, 0.0, "the page stayed");
        w.wheel(over, -3.0);
        assert_eq!(w.g().rows[0], top - 4);
        // over the keys too
        let keys = pos2(g.label.center().x, g.grid.top() + 20.0);
        w.wheel(keys, 1.0);
        assert_eq!(w.g().rows[0], top - 2);
        assert_eq!(w.page.0, 0.0);
        // folded: no roll, the same wheel scrolls the page
        assert!(w.click_text(EN.f_roll_hide), "{:?}", w.texts());
        assert!(w.f.geom.is_none() && !w.f.roll_open);
        w.wheel(over, -3.0);
        assert!(w.page.0 > 0.0 || w.page.1 <= w.height, "{:?}", w.page);
        w.hover(over);
        w.frame(vec![Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, -1.0), modifiers: NO }]);
        assert_ne!(w.ctx.input(|i| i.raw_scroll_delta), Vec2::ZERO, "the page gets the wheel");
        assert!(w.click_text(EN.f_roll_show), "{:?}", w.texts());
        assert!(w.f.geom.is_some());
    }

    #[test]
    fn a_chord_note_dragged_into_a_pattern_adds_no_lanes() {
        let mut h = H::new(am(), false);
        let Some(s) = view_clip(&h.part, A_MINOR).steps.first().copied() else { return assert!(!view_clip(&h.part, A_MINOR).steps.is_empty()) };
        let rh = h.g().row_h;
        let a = h.at(4.5, s.pitch);
        h.press(a, P, NO);
        h.move_to(a - vec2(0.0, 2.0 * rh));
        assert!(matches!(h.part.play, Play::Clip { .. }), "{:?}", h.part.play);
        assert!(h.roll.on_chord(7), "the window keeps the chord's rows until the release");
        // the pattern view showing lanes under the held button
        h.lanes = true;
        h.move_to(a - vec2(0.0, 3.0 * rh));
        h.release(a - vec2(0.0, 3.0 * rh), P);
        assert!(!h.roll.on_chord(7));
        assert!(h.kept().is_empty(), "no chromatic row becomes a lane: {:?}", h.kept());
    }

    #[test]
    fn in_the_window_a_chord_drag_survives_batched_and_idle_frames() {
        let mut w = Win::new(Play::Chords { degrees: vec![0], size: 3, octave: 4, vel: 96, strum: 0 }, false);
        let notes = view_clip(&w.part().unwrap_or_else(am), A_MINOR).steps;
        let Some(s) = notes.first().copied() else { return assert!(!notes.is_empty()) };
        let (cw, rh) = (w.g().cell_w, w.g().row_h);
        let a = w.at(f32::from(s.at) + 4.5, s.pitch);
        // baseview batches what came between two frames: the move that
        // brought the pointer and the press arrive together
        w.frame(vec![Event::PointerMoved(a), Event::PointerButton { pos: a, button: P, pressed: true, modifiers: NO }]);
        let end = a + vec2(3.0 * cw, -rh);
        for i in 1..=10 {
            let f = i as f32 / 10.0;
            // two moves in one frame, then a frame without events
            w.frame(vec![Event::PointerMoved(a + (end - a) * (f - 0.05)), Event::PointerMoved(a + (end - a) * f)]);
            w.frame(vec![]);
            assert!(!matches!(w.f.roll.gesture, Gesture::Idle), "the drag lives on frame {i}");
        }
        // the last move and the release in one frame
        w.frame(vec![Event::PointerMoved(end), Event::PointerButton { pos: end, button: P, pressed: false, modifiers: NO }]);
        w.frame(vec![]);
        let m = w.steps().into_iter().find(|x| x.pitch == s.pitch + 1);
        assert!(m.is_some_and(|m| m.at == s.at + 3), "{:?}", w.steps());
        assert_eq!(w.steps().len(), notes.len());
        assert!(matches!(w.f.roll.gesture, Gesture::Idle));
    }

    #[test]
    fn in_the_window_an_option_drag_follows_the_mouse_frame_by_frame() {
        let mut w = Win::new(Play::Clip { bars: 1, steps: vec![st(4, 1, 60)], rows: vec![] }, true);
        let g = w.g();
        let a = w.at(4.3, 60);
        w.press(a, Modifiers::ALT);
        let mut seen = Vec::new();
        for i in 1..=10 {
            let p = a + vec2(g.cell_w * 0.05 * i as f32, 0.0);
            w.at = p;
            w.frame(vec![Event::PointerMoved(p)]);
            let t = w.steps().first().map(|s| u32::from(s.at) * 24 + u32::from(s.nudge));
            let want = if (p - a).length() >= DRAG_START { (96.0 + (g.tick_at(p.x) - g.tick_at(a.x))).round() as u32 } else { 96 };
            assert_eq!(t, Some(want), "frame {i}");
            seen.push(want);
        }
        seen.dedup();
        assert!(seen.len() >= 8, "a tick or so per frame, no jumps to the grid: {seen:?}");
        w.release();
        let s = w.steps();
        assert!(s.len() == 1 && s[0].at == 4 && s[0].nudge == 12, "{s:?}");
    }

    #[test]
    fn three_notes_added_above_a_chord_leave_rows_and_page_alone() {
        let mut w = Win::new(Play::Chords { degrees: vec![0], size: 3, octave: 4, vel: 96, strum: 0 }, false);
        let notes = view_clip(&w.part().unwrap_or_else(am), A_MINOR).steps;
        let hi = notes.iter().map(|s| s.pitch).max().unwrap_or(0);
        // two notches up over the roll: rows above the chord come in
        let g = w.g();
        w.wheel(g.grid.center(), 2.0);
        let before = w.g();
        assert!(before.rows[2] > hi, "rows above the chord: {:?} over {hi}", before.rows);
        let page = w.page.0;
        for (k, step) in [(0usize, 2.5f32), (1, 5.5), (2, 9.5)] {
            let p = w.at(step, before.rows[k]);
            w.press(p, NO);
            w.release();
            for _ in 0..10 {
                w.frame(vec![]);
            }
            assert_eq!(w.g().rows, before.rows, "rows stay after note {k}");
            assert_eq!(w.g().block.min, before.block.min, "the roll stays on the page after note {k}");
            assert_eq!(w.page.0, page);
        }
        assert_eq!(w.steps().iter().filter(|s| s.pitch > hi).count(), 3, "{:?}", w.steps());
        assert_eq!(w.steps().len(), notes.len() + 3);
    }

    #[test]
    fn a_wall_without_a_part_makes_one_that_binds_no_finger() {
        let rows = vec![fl::ChannelRow { index: 0, name: "Kick".into(), plugin: String::new() }, fl::ChannelRow { index: 3, name: "Pad".into(), plugin: String::new() }];
        let mut w = Win::wall(Side::Top, rows.clone());
        let binds = w.w.player.store().binds;
        assert!(w.texts().iter().any(|(s, _)| *s == fill(EN.f_nothing, &[&EN.fw_top])), "{:?}", w.texts());
        assert!(w.click_text(EN.fw_note), "{:?}", w.texts());
        let store = w.w.player.store();
        assert_eq!(store.binds, binds, "no finger bound");
        let Some(id) = w.wall_part(Side::Top) else { return assert!(w.wall_part(Side::Top).is_some()) };
        assert_eq!(store.walls, vec![id]);
        let part = store.get(id).cloned();
        assert_eq!(part.as_ref().map(|p| (&p.play, p.start)), Some((&Play::Note { pitch: 60, vel: 100 }, Start::Now)));
        assert_eq!(part.and_then(|p| p.channel).map(|c| c.index), Some(0), "the first channel without a focus");
        assert!(w.w.pong.setup().auto_channel.is_empty(), "it has a channel");
        // the editor now: when a hit plays, no "while shown"
        w.frame(vec![]);
        let texts: Vec<String> = w.texts().into_iter().map(|(s, _)| s).collect();
        assert!(texts.contains(&format!("○ {}", EN.fw_top)) && texts.contains(&EN.fw_hit_plays.to_string()), "{texts:?}");
        assert!(!texts.contains(&EN.f_while_held.to_string()));
        // an empty pattern on another wall starts at once too (a finger's
        // pattern waits for the next 1/16)
        let mut w = Win::wall(Side::Bottom, rows);
        assert!(w.click_text(EN.f_pattern), "{:?}", w.texts());
        let start = w.wall_part(Side::Bottom).and_then(|id| w.w.player.store().get(id).map(|p| p.start));
        assert_eq!(start, Some(Start::Now));
        assert!(w.w.player.store().binds.is_empty());
    }

    /// Every text drawn with its rect.
    fn text_rects(w: &Win) -> Vec<(String, Rect)> {
        fn walk(s: &egui::Shape, out: &mut Vec<(String, Rect)>) {
            match s {
                egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2()))),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for c in w.out.iter().flat_map(|o| &o.shapes) {
            walk(&c.shape, &mut out);
        }
        out
    }

    /// Noa's 720 pt test: the template picker ran under the part's name
    /// ("Template" under "Pong bottom"). At 720 the picker, Back's slot,
    /// the name and Save take a line under the fields; with room they
    /// stay in the row. Nothing overlaps either way.
    #[test]
    fn the_template_picker_never_runs_under_the_part_name() {
        let chord = Play::Chords { degrees: vec![0], size: 3, octave: 4, vel: 96, strum: 0 };
        let clip = midi::template("Kick").unwrap_or(Play::Clip { bars: 1, steps: Vec::new(), rows: Vec::new() });
        for (play, lead, width, wraps) in [(chord.clone(), EN.f_strum, 720.0, true), (clip, EN.f_bars, 720.0, true), (chord, EN.f_strum, 1100.0, false)] {
            let mut w = Win::new(play, false);
            w.width = width;
            let mut store = w.w.player.store();
            if let Some(p) = store.get_mut(w.id) {
                p.name = "Pong bottom".into();
            }
            w.w.update_midi(store);
            for _ in 0..4 {
                w.frame(vec![]);
            }
            let texts = text_rects(&w);
            let find = |s: &str| texts.iter().find(|(x, _)| x == s).map_or(Rect::NOTHING, |(_, r)| *r);
            let (tpl, lead, save) = (find(EN.f_template), find(lead), find(EN.save));
            assert!(tpl.is_positive() && lead.is_positive() && save.is_positive(), "{width}: {texts:?}");
            // the picker's text sits 8 in from the button's left
            let button_right = tpl.left() - 8.0 + PICKER_W;
            let names: Vec<Rect> = texts.iter().filter(|(x, _)| x == "Pong bottom").map(|(_, r)| *r).collect();
            assert!(!names.is_empty(), "{width}: the name drawn");
            for n in names.iter().filter(|n| n.y_range().intersects(tpl.y_range())) {
                assert!(n.left() >= button_right, "{width}: the name {n:?} under the picker ending at {button_right}");
            }
            for n in names.iter().filter(|n| n.y_range().intersects(save.y_range())) {
                assert!(save.left() >= n.right(), "{width}: Save {save:?} over the name {n:?}");
            }
            assert!(save.right() <= width - 18.0, "{width}: Save {save:?} inside the window");
            assert_eq!(tpl.top() > lead.bottom(), wraps, "{width}: the tail on its own line: {tpl:?} {lead:?}");
        }
    }

    /// The finger of `Win::new` with its part taken away.
    fn finger_without_a_part() -> Win {
        let mut w = Win::new(Play::Note { pitch: 60, vel: 100 }, false);
        let mut store = w.w.player.store();
        store.binds.clear();
        store.parts.clear();
        w.w.update_midi(store);
        w.f.sel = Some(Slot::finger(1, 1));
        w.frame(vec![]);
        w
    }

    #[test]
    fn a_finger_without_a_part_shows_the_kinds_and_the_part_picker_not_template_chips() {
        let mut w = finger_without_a_part();
        let slot = Slot::finger(1, 1);
        let texts: Vec<String> = w.texts().into_iter().map(|(s, _)| s).collect();
        assert!(texts.contains(&fill(EN.f_nothing, &[&slot.label(&EN, false)])), "{texts:?}");
        for want in [EN.f_pattern, EN.f_chord, EN.f_held, EN.f_template] {
            assert!(texts.iter().any(|s| s == want), "{want}: {texts:?}");
        }
        let templates = ["Hi-hat basic", "Trap roll", "Snare fill", "Kick", "Clap", "I-V-vi-IV"];
        assert!(!texts.iter().any(|s| templates.contains(&s.as_str())), "no template chips: {texts:?}");
        // the templates live in the picker; the chord's only in a chord's
        assert!(w.click_text(EN.f_template), "{:?}", w.texts());
        let texts: Vec<String> = w.texts().into_iter().map(|(s, _)| s).collect();
        assert!(texts.iter().any(|s| s == "Kick") && !texts.iter().any(|s| s == "I-V-vi-IV"), "{texts:?}");
        assert!(w.click_text("Kick"));
        let store = w.w.player.store();
        let part = store.bound(&slot.signal(), slot.trigger()).cloned();
        assert_eq!(part.as_ref().map(|p| p.name.as_str()), Some("Kick"));
        assert_eq!(part.map(|p| p.play), midi::template("Kick"));
        // the part editor now
        w.frame(vec![]);
        assert!(text_pos(&w, &format!("○ {}", slot.label(&EN, false))).is_some(), "{:?}", w.texts());
        // a kind makes the part too
        let mut w = finger_without_a_part();
        assert!(w.click_text(EN.f_chord), "{:?}", w.texts());
        let play = w.w.player.store().bound(&slot.signal(), slot.trigger()).map(|p| p.play.clone());
        assert!(matches!(play, Some(Play::Chords { .. })), "{play:?}");
    }

    #[test]
    fn without_fl_the_channel_field_reads_not_linked() {
        // the window runs without FL: a disabled Ui would paint nothing
        let w = Win::new(Play::Note { pitch: 60, vel: 100 }, false);
        let start = [EN.f_start_now, EN.f_start_16, EN.f_start_beat, EN.f_start_bar].iter().find_map(|s| text_pos(&w, s));
        let (Some(ch), Some(start)) = (text_pos(&w, EN.f_not_linked), start) else { return assert!(w.texts().is_empty(), "{:?}", w.texts()) };
        assert!(start.x > ch.x, "the start dropdown after the channel field");
        let kind = text_pos(&w, EN.f_held).map_or(f32::NAN, |p| p.y);
        assert!((ch.y - kind).abs() < 1.0, "on the editor row");
    }

    #[test]
    fn calibrate_fingers_stays_readable_while_it_runs() {
        let ctx = Context::default();
        crate::glass::style(&ctx);
        let w = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut f = Fingers::default();
        let frame = |f: &mut Fingers, i: u32| {
            let raw = RawInput { time: Some(f64::from(i) / 60.0), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 200.0))), ..Default::default() };
            let out = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| f.header(ui, Rect::from_min_size(pos2(80.0, 10.0), vec2(1000.0, Tok::HEAD_H)), &w, &EN));
            });
            let mut texts = Vec::new();
            for c in &out.shapes {
                walk_texts(&c.shape, &mut texts);
            }
            let colour = out.shapes.iter().find_map(|c| text_colour(&c.shape, EN.f_calibrate));
            (texts.into_iter().find(|(s, _)| s == EN.f_calibrate).map(|(_, p)| p), colour)
        };
        let idle = (0..3).map(|i| frame(&mut f, i)).last().unwrap_or_default();
        assert_eq!(idle.1, Some(white(Tok::VALUE)), "{idle:?}");
        f.calib = Some(crate::calib::Calib::default());
        let running = frame(&mut f, 3);
        assert_eq!(running.0, idle.0, "same place, still drawn");
        assert_eq!(running.1, Some(white(glass::DIM_OFF)), "dim while it runs");
    }

    fn text_colour(s: &egui::Shape, needle: &str) -> Option<Color32> {
        match s {
            egui::Shape::Text(t) if t.galley.text() == needle => Some(t.override_text_color.unwrap_or(t.fallback_color)),
            egui::Shape::Vec(v) => v.iter().find_map(|s| text_colour(s, needle)),
            _ => None,
        }
    }

    #[test]
    fn the_misread_line_names_whole_fingers_with_the_camera_reading() {
        use crate::i18n::DE;
        let misread = [(0, finger_bit(2), finger_bit(1)), (1, finger_bit(2), finger_bit(1))];
        let de = cal_lines(&[], &misread, &DE);
        assert!(de.iter().any(|l| l == "Mittelfinger allein: die Kamera liest Zeigefinger (L, R)"), "{de:?}");
        let en = cal_lines(&[], &misread, &EN);
        assert!(en.iter().any(|l| l == "Middle alone reads as index (L, R)"), "{en:?}");
        let fist = cal_lines(&[], &[(1, 0, finger_bit(1) | finger_bit(2))], &DE);
        assert!(fist.iter().any(|l| l == "Faust: die Kamera liest Zeigefinger, Mittelfinger (R)"), "{fist:?}");
    }

    #[test]
    fn remove_sets_the_wall_off_and_deletes_its_part() {
        let mut w = Win::wall(Side::Left, vec![]);
        assert!(w.click_text(EN.f_chord));
        let Some(id) = w.wall_part(Side::Left) else { return assert!(w.wall_part(Side::Left).is_some()) };
        assert_eq!(w.w.pong.setup().auto_channel, vec![id], "no channel list: it waits for one");
        assert!(matches!(w.w.player.store().get(id).map(|p| &p.play), Some(Play::Chords { degrees, .. }) if *degrees == vec![0]));
        w.frame(vec![]);
        assert!(w.click_text(EN.f_remove), "{:?}", w.texts());
        let setup = w.w.pong.setup();
        assert_eq!(setup.wall(Side::Left), &Action::Off);
        assert!(setup.auto_channel.is_empty());
        let store = w.w.player.store();
        assert!(store.get(id).is_none() && store.walls.is_empty(), "{store:?}");
        // the kinds again
        w.frame(vec![]);
        assert!(w.texts().iter().any(|(s, _)| *s == fill(EN.f_nothing, &[&EN.fw_left])));
    }

    #[test]
    fn a_state_reload_clears_the_undo_slot_and_a_selection_whose_part_went() {
        let mut w = Win::new(Play::Chords { degrees: vec![0], size: 3, octave: 4, vel: 96, strum: 0 }, false);
        assert!(w.click_text(EN.f_pattern), "{:?}", w.texts());
        assert!(w.f.undo.is_some());
        w.frame(vec![]);
        assert!(w.f.undo.is_some(), "frames alone keep it");
        w.w.bump_state();
        w.frame(vec![]);
        assert!(w.f.undo.is_none());
        assert_eq!(w.f.sel, Some(Slot::finger(1, 1)), "its part is still there");
        // a load without the finger's part
        let mut store = w.w.player.store();
        store.binds.clear();
        store.parts.clear();
        w.w.set_midi(store);
        w.w.bump_state();
        w.frame(vec![]);
        assert_eq!(w.f.sel, None);
        assert!(w.f.tools_h.is_none() && matches!(w.f.roll.gesture, Gesture::Idle));
    }

    #[test]
    fn the_total_view_lists_wall_parts_after_the_fingers() {
        let mut w = Win::new(Play::Note { pitch: 60, vel: 100 }, false);
        let mut store = w.w.player.store();
        let id = store.add("Pong top", Play::Note { pitch: 72, vel: 100 }, None);
        w.w.update_midi(store);
        let mut s = (*w.w.pong.setup()).clone();
        s.walls[Side::Top.index()] = Action::Part { id };
        w.w.pong.set_setup(s);
        w.f.view = View::Total;
        w.frame(vec![]);
        let y = |w: &Win, needle: &str| w.texts().into_iter().find(|(s, _)| s == needle).map(|(_, p)| p.y);
        let finger = y(&w, &Slot::finger(1, 1).label(&EN, false));
        let wall = y(&w, EN.fw_top);
        assert!(matches!((finger, wall), (Some(a), Some(b)) if a < b), "{finger:?} {wall:?}");
        assert!(w.click_text(EN.fw_top));
        assert_eq!(w.f.take_wall_pick(), Some(Side::Top));
        assert_eq!(w.f.take_wall_pick(), None);
    }

    /// The wall editor keeps its own fold, tools height and undo slot: the
    /// finger's editor may be on screen in the same frame.
    #[test]
    fn the_wall_editor_keeps_its_own_roll_fold_and_tools_height() {
        let mut w = Win::wall(Side::Left, vec![]);
        assert!(w.click_text(EN.f_chord));
        w.frame(vec![]);
        assert!(w.f.wall.tools_h.is_some(), "the wall's tools measured");
        assert!(w.f.tools_h.is_none(), "the finger's height is not the wall's");
        assert!(w.click_text(EN.f_pattern), "{:?}", w.texts());
        assert!(w.f.wall.undo.is_some() && w.f.undo.is_none(), "the wall's template undo is its own");
        w.frame(vec![]);
        assert!(w.click_text(EN.f_roll_hide), "{:?}", w.texts());
        assert!(!w.f.wall.roll_open);
        assert!(w.f.roll_open, "the finger's roll stays shown");
    }

    /// A lane popover opened on one wall's roll closes when another wall
    /// opens; that wall's roll then takes presses again.
    #[test]
    fn another_wall_closes_the_old_parts_lane_popover() {
        let mut w = Win::wall(Side::Top, vec![]);
        let mut store = w.w.player.store();
        store.key = A_MINOR;
        let top = store.add("Top", Play::Clip { bars: 1, steps: vec![st(0, 1, 42), st(4, 1, 42)], rows: vec![42] }, None);
        let chord = Play::Chords { degrees: vec![0], size: 3, octave: 4, vel: 96, strum: 0 };
        let bottom = store.add("Bottom", chord, None);
        w.w.update_midi(store);
        let mut s = (*w.w.pong.setup()).clone();
        s.walls[Side::Top.index()] = Action::Part { id: top };
        s.walls[Side::Bottom.index()] = Action::Part { id: bottom };
        w.w.pong.set_setup(s);
        for _ in 0..4 {
            w.frame(vec![]);
        }
        let g = w.g();
        let a = pos2(g.label.center().x, g.row_center(0));
        w.press(a, NO);
        w.release();
        assert!(w.ctx.memory(|m| m.is_popup_open(Id::new(("lane-pop", top)))));
        w.wall = Some((Side::Bottom, vec![]));
        w.f.wall_switched(&w.ctx);
        for _ in 0..4 {
            w.frame(vec![]);
        }
        assert!(!w.ctx.memory(|m| m.any_popup_open()));
        let notes = w.w.player.store().get(bottom).map(|p| view_clip(p, A_MINOR).steps).unwrap_or_default();
        let Some(n) = notes.first().copied() else { return assert!(!notes.is_empty()) };
        let rh = w.g().row_h;
        let a = w.at(n.at as f32 + 2.5, n.pitch);
        w.drag(a, a - vec2(0.0, 2.0 * rh), NO);
        let play = w.w.player.store().get(bottom).map(|p| p.play.clone());
        assert!(matches!(play, Some(Play::Clip { .. })), "the drag on the chord made a pattern: {play:?}");
    }

    /// A lit dot of finger `f` on hand `side` with its four neighbours in
    /// the same region, in window points.
    fn finger_point(w: &Win, side: usize, f: usize) -> Option<Pos2> {
        let art = build_art();
        let r = f as u8 + 2;
        let at = |x: usize, y: usize| art.region.get(y * CANVAS + x).copied();
        let (x, y) = (1..CANVAS - 1).flat_map(|y| (1..CANVAS - 1).map(move |x| (x, y))).find(|&(x, y)| {
            art.lit[y * CANVAS + x] && [at(x, y), at(x - 1, y), at(x + 1, y), at(x, y - 1), at(x, y + 1)].iter().all(|v| *v == Some(r))
        })?;
        Some(on_canvas(w.f.canvas[side], [x as f32 + 0.5, y as f32 + 0.5], side == 0))
    }

    fn text_pos(w: &Win, needle: &str) -> Option<Pos2> {
        w.texts().into_iter().find(|(s, _)| s == needle).map(|(_, p)| p)
    }

    #[test]
    fn a_click_selects_in_the_same_frame() {
        let mut w = Win::new(Play::Clip { bars: 1, steps: vec![st(0, 1, 42)], rows: vec![] }, false);
        let Some(p) = finger_point(&w, 1, 3) else { return assert!(finger_point(&w, 1, 3).is_some()) };
        assert_ne!(w.f.cols[1][3].0, HAND[1], "the ring finger is plain before");
        w.hover(p);
        w.frame(vec![Event::PointerButton { pos: p, button: P, pressed: true, modifiers: NO }]);
        w.frame(vec![Event::PointerButton { pos: p, button: P, pressed: false, modifiers: NO }]);
        // the frame of the click built the texture with the new selection
        assert_eq!(w.f.sel, Some(Slot::finger(1, 3)));
        assert_eq!(w.f.cols[1][3], (HAND[1], HAND[1].gamma_multiply(0.45)));
        assert_ne!(w.f.cols[1][1].0, HAND[1], "the index is no longer selected: {:?}", w.f.cols[1][1]);
        // a selected finger without a part shows an "Empty" node
        assert!(w.texts().iter().any(|(s, _)| s == EN.f_empty), "{:?}", w.texts());
    }

    #[test]
    fn shapes_offer_fist_and_open_and_keep_old_bound_ones_behind_one_chip() {
        let mut w = Win::new(Play::Note { pitch: 60, vel: 100 }, false);
        let mut store = w.w.player.store();
        for mask in [0b10001, 0b11000] {
            let s = Slot::shape(1, mask);
            let id = store.add("Old", Play::Note { pitch: 62, vel: 100 }, None);
            store.bind(&s.signal(), s.trigger(), id);
        }
        w.w.update_midi(store);
        w.frame(vec![]);
        let texts: Vec<String> = w.texts().into_iter().map(|(s, _)| s).collect();
        assert_eq!(texts.iter().filter(|s| *s == EN.f_fist).count(), 2, "{texts:?}");
        assert_eq!(texts.iter().filter(|s| *s == EN.f_open).count(), 2);
        let more = fill(EN.f_older_shapes, &[&2]);
        assert_eq!(texts.iter().filter(|s| **s == more).count(), 1, "only the right hand has older shapes");
        assert_eq!(HANDS_H, 272.0, "the chip row keeps the view 272 high");
        // the list names the fingers; a pick opens the part editor
        assert!(w.click_text(&more), "{:?}", w.texts());
        assert!(text_pos(&w, "Thumb and index").is_some(), "{:?}", w.texts());
        assert!(w.click_text("Thumb and pinky"), "{:?}", w.texts());
        assert_eq!((w.f.sel, w.f.view), (Some(Slot::shape(1, 0b10001)), View::Single));
        assert!(text_pos(&w, &format!("○ {} · thumb and pinky", EN.hand_right)).is_some(), "{:?}", w.texts());
        // Fist on the left hand
        assert!(w.click_text(EN.f_fist));
        assert_eq!(w.f.sel, Some(Slot::shape(0, 0b00000)));
        assert_eq!(mask_name(0b01110, &EN), "Index, middle and ring");
        assert_eq!(mask_name(0b11000, &EN), "Thumb and index");
        assert_eq!(mask_name(0b01000, &EN), "Index");
        // nothing selected: one hint
        w.f.sel = None;
        w.frame(vec![]);
        assert!(text_pos(&w, EN.f_pick_part).is_some());
    }

    #[test]
    fn part_editor_rows_in_order() {
        let mut w = Win::new(Play::Clip { bars: 1, steps: vec![st(0, 1, 42)], rows: vec![] }, false);
        let y = |w: &Win, s: &str| text_pos(w, s).map(|p| p.y);
        let owner = format!("○ {}", Slot::finger(1, 1).label(&EN, false));
        let (Some(o), Some(k), Some(b)) = (y(&w, &owner), y(&w, EN.f_pattern), y(&w, EN.f_bars)) else { return assert!(w.texts().is_empty(), "{:?}", w.texts()) };
        assert!(o < k && k < b, "owner {o}, kind {k}, bars {b}");
        let (Some(hide), Some(remove)) = (text_pos(&w, EN.f_roll_hide), text_pos(&w, EN.f_remove)) else { return assert!(w.texts().is_empty(), "{:?}", w.texts()) };
        assert!((hide.y - o).abs() < 1.0 && (remove.y - o).abs() < 1.0, "Hide roll and Remove on the owner's row");
        assert!(remove.x > hide.x, "Remove right of Hide roll");
        assert!(text_pos(&w, EN.f_repeats).is_some(), "the footer says Repeats");
        // a template brings Back; the bars field and the name stay
        let bars = text_pos(&w, EN.f_bars);
        let save = text_pos(&w, EN.save);
        assert!(w.click_text(EN.f_template), "{:?}", w.texts());
        assert!(w.click_text("Kick"), "{:?}", w.texts());
        assert!(w.f.undo.is_some());
        w.frame(vec![]);
        assert!(text_pos(&w, &fill(EN.f_back, &[&"Part"])).is_some(), "{:?}", w.texts());
        assert_eq!(text_pos(&w, EN.f_bars), bars, "the bars field did not move");
        assert_eq!(text_pos(&w, EN.save).map(|p| p.x), save.map(|p| p.x), "Save did not move");
    }

    #[test]
    fn the_part_picker_lists_templates_then_saved_parts_and_empty_clears() {
        let saved = Part { id: 99, name: "Zz picker test".into(), channel: None, play: Play::Clip { bars: 2, steps: vec![st(2, 1, 38)], rows: vec![38] }, mode: Mode::Once, start: Start::Bar };
        assert_eq!(part_lib::save(&saved.name, &saved), Ok(()));
        let mut w = Win::new(Play::Clip { bars: 1, steps: vec![st(0, 1, 42), st(4, 1, 42)], rows: vec![] }, false);
        assert!(w.click_text(EN.f_template), "{:?}", w.texts());
        let y = |w: &Win, s: &str| text_pos(w, s).map(|p| p.y).unwrap_or(f32::NAN);
        let order = ["Hi-hat basic", "Trap roll", "Snare fill", "Kick", "Clap", EN.f_empty, EN.f_saved_parts, "Zz picker test"];
        let ys: Vec<f32> = order.iter().map(|s| y(&w, s)).collect();
        assert!(ys.windows(2).all(|p| p[1] > p[0]), "{ys:?} {:?}", w.texts());
        assert!(w.texts().iter().filter(|(s, _)| s == EN.preset_builtin).count() >= 6);
        assert!(!w.texts().iter().any(|(s, _)| s == "I-V-vi-IV"), "a pattern lists the pattern templates");
        // Empty clears the steps with the undo slot
        assert!(w.click_text(EN.f_empty));
        assert!(w.steps().is_empty() && w.f.undo.is_some(), "{:?}", w.steps());
        // a saved part replaces play, mode, start and name
        assert!(w.click_text(EN.f_template));
        assert!(w.click_text("Zz picker test"), "{:?}", w.texts());
        let p = w.part();
        assert_eq!(p.as_ref().map(|p| (&p.play, p.mode, p.start, p.name.as_str())), Some((&saved.play, Mode::Once, Start::Bar, "Zz picker test")));
        assert_eq!(p.map(|p| p.id), Some(w.id), "the part keeps its id");
        // a saved name reads Overwrite
        w.frame(vec![]);
        assert!(text_pos(&w, EN.overwrite).is_some(), "{:?}", w.texts());
        assert_eq!(part_lib::delete(&saved.name), Ok(()));
    }

    #[test]
    fn routes_view_switches_once_per_batch_and_returns() {
        let r = |batch: u64, focus: bool| Routing { edges: vec![EdgeIn { signal: "cam0.hand.R.pinch".into(), target: NodeTarget::Macro(2), pending: true }], focus, batch };
        let mut w = Win::new(Play::Note { pitch: 60, vel: 100 }, false);
        w.routing = r(5, false);
        w.frame(vec![]);
        assert_eq!(w.f.view, View::Single, "no focus: no switch");
        w.routing = r(5, true);
        w.frame(vec![]);
        assert_eq!(w.f.view, View::Routes);
        assert!(text_pos(&w, "Pinch").is_some() && text_pos(&w, "Signal 3").is_some(), "{:?}", w.texts());
        w.frame(vec![]);
        assert_eq!(w.f.view, View::Routes);
        w.routing = r(5, false);
        w.frame(vec![]);
        assert_eq!(w.f.view, View::Single, "back where it was");
        w.routing = r(5, true);
        w.frame(vec![]);
        assert_eq!(w.f.view, View::Single, "the same batch switches once");
        // a click on a finger in between: the view stays where the person put it
        w.f.view = View::Total;
        w.routing = r(6, true);
        w.frame(vec![]);
        assert_eq!(w.f.view, View::Routes);
        let ctx = w.ctx.clone();
        w.f.select(Slot::finger(1, 2), &ctx);
        w.routing = r(6, false);
        w.frame(vec![]);
        assert_eq!(w.f.view, View::Single);
        // a manual pick of Routes stays
        let mut sw = RouteSwitch::default();
        assert_eq!(sw.step(View::Routes, &r(7, false)), View::Routes);
        assert_eq!(sw.step(View::Routes, &r(7, true)), View::Routes);
        assert_eq!(sw.step(View::Routes, &r(7, false)), View::Routes);
        // a click on a target node asks the window once to unfold the routes
        w.routing = r(8, true);
        w.frame(vec![]);
        // the hand node reads "Signal 3" too: the Routes view's is the lower one
        let node = w.texts().into_iter().filter(|(s, _)| s == "Signal 3").map(|(_, p)| p).max_by(|a, b| a.y.total_cmp(&b.y));
        assert!(node.is_some_and(|p| p.y > w.f.canvas[1].bottom()), "{:?}", w.texts());
        if let Some(p) = node {
            w.press(p, NO);
            w.release();
        }
        assert!(w.f.take_unfold());
        assert!(!w.f.take_unfold());
        // the focus ends: back to the view it left
        w.routing = Routing::default();
        w.frame(vec![]);
        assert_eq!(w.f.view, View::Single);
        // no hand route: the hint
        let mut store = w.w.player.store();
        store.binds.clear();
        w.w.update_midi(store);
        w.f.view = View::Routes;
        w.frame(vec![]);
        assert!(text_pos(&w, EN.f_routes_empty).is_some(), "{:?}", w.texts());
    }

    /// Keep leaves the view on Routes (Noa, 11-04); the person can still
    /// pick another view; Discard goes back where it was.
    #[test]
    fn keep_leaves_the_view_on_routes() {
        let freq = |pending| EdgeIn { signal: "cam0.hand.L.openness".into(), target: NodeTarget::Param { plugin: "Serum 2".into(), name: "Filter 1 Freq".into() }, pending };
        let res = EdgeIn { signal: "cam0.hand.R.y".into(), target: NodeTarget::Param { plugin: "Serum 2".into(), name: "Filter 1 Res".into() }, pending: true };
        let mut w = Win::new(Play::Note { pitch: 60, vel: 100 }, false);
        w.routing = Routing { edges: vec![freq(true)], focus: true, batch: 3 };
        w.frame(vec![]);
        assert_eq!(w.f.view, View::Routes);
        // Keep: the route is kept, nothing pends, the field lost the focus
        w.routing = Routing { edges: vec![freq(false)], focus: false, batch: 0 };
        for _ in 0..3 {
            w.frame(vec![]);
            assert_eq!(w.f.view, View::Routes, "Keep stays on Routes");
        }
        // a view picked by hand stays
        w.f.view = View::Single;
        w.f.route.manual = true;
        w.frame(vec![]);
        assert_eq!(w.f.view, View::Single);
        // Discard: back to the view the switch left
        w.routing = Routing { edges: vec![freq(false), res], focus: true, batch: 4 };
        w.frame(vec![]);
        assert_eq!(w.f.view, View::Routes);
        w.routing = Routing { edges: vec![freq(false)], focus: false, batch: 0 };
        w.frame(vec![]);
        assert_eq!(w.f.view, View::Single, "Discard goes back");
        // a Keep while the person had picked Single after the switch stays there
        w.routing = Routing { edges: vec![freq(false), EdgeIn { pending: true, ..freq(false) }], focus: true, batch: 5 };
        w.frame(vec![]);
        w.f.view = View::Single;
        w.f.route.manual = true;
        w.routing = Routing { edges: vec![freq(false), EdgeIn { signal: "cam0.hand.R.x".into(), ..freq(false) }], focus: false, batch: 0 };
        w.frame(vec![]);
        assert_eq!(w.f.view, View::Single);
    }

    /// In the Routes view a kept route's target box opens the search over
    /// the parameters and Signals, its source box the one over the hand
    /// movements; a pick asks the window to retarget, the x to remove.
    #[test]
    fn routes_boxes_retarget_and_the_x_removes() {
        let target = NodeTarget::Param { plugin: "Serum 2".into(), name: "Filter 1 Freq".into() };
        let sig = "cam0.hand.L.openness";
        let mut w = Win::new(Play::Note { pitch: 60, vel: 100 }, false);
        w.routing = Routing { edges: vec![EdgeIn { signal: sig.into(), target: target.clone(), pending: false }], focus: false, batch: 0 };
        w.f.view = View::Routes;
        w.frame(vec![]);
        let below = |w: &Win, s: &str| w.texts().into_iter().filter(|(t, _)| t == s).map(|(_, p)| p).find(|p| p.y > w.f.canvas[1].bottom());
        let enter = |w: &mut Win| w.frame(vec![Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: NO }]);
        // the target box: Signals by typing
        let Some(box_at) = below(&w, "Filter 1 Freq") else { return assert!(w.texts().is_empty(), "{:?}", w.texts()) };
        w.press(box_at, NO);
        w.release();
        assert!(text_pos(&w, EN.pf_hint).is_some(), "the search opened: {:?}", w.texts());
        w.frame(vec![Event::Text("signal 12".into())]);
        enter(&mut w);
        assert_eq!(w.f.take_retarget(), vec![(sig.to_string(), target.clone(), param_find::Found::Macro(11))]);
        w.frame(vec![]);
        assert!(text_pos(&w, EN.pf_hint).is_none(), "a pick closes it");
        // the source box: the hand movements
        let Some(src) = below(&w, "Openness") else { return assert!(w.texts().is_empty(), "{:?}", w.texts()) };
        w.press(src, NO);
        w.release();
        assert!(text_pos(&w, "Pinch").is_some(), "{:?}", w.texts());
        w.frame(vec![Event::Text("pinch".into())]);
        enter(&mut w);
        assert_eq!(w.f.take_retarget(), vec![(sig.to_string(), target.clone(), param_find::Found::Move("pinch".into()))]);
        assert!(w.f.take_retarget().is_empty(), "once");
        // the x: the first click arms, the second removes
        w.frame(vec![]);
        let Some(x) = below(&w, "×") else { return assert!(w.texts().is_empty(), "{:?}", w.texts()) };
        w.press(x, NO);
        w.release();
        assert_eq!(w.f.take_unmap(), None);
        assert!(text_pos(&w, EN.unmap_arm).is_some(), "{:?}", w.texts());
        w.press(x, NO);
        w.release();
        assert_eq!(w.f.take_unmap(), Some((sig.to_string(), target)));
        // a pending route's box has neither: it unfolds the routes
        w.routing.edges[0].pending = true;
        w.frame(vec![]);
        assert!(below(&w, "×").is_none());
        if let Some(p) = below(&w, "Filter 1 Freq") {
            w.press(p, NO);
            w.release();
        }
        assert!(w.f.take_unfold() && w.f.take_retarget().is_empty());
    }

    #[test]
    fn selecting_scrolls_the_owner_row_to_the_top() {
        let mut w = Win::new(Play::Clip { bars: 1, steps: vec![st(0, 1, 60)], rows: vec![] }, true);
        w.height = 360.0;
        w.frame(vec![]);
        let before = w.f.owner_row.map_or(0.0, |r| r.top());
        assert!(before > 200.0 && w.page.0 == 0.0, "{before} {:?}", w.page);
        // the index's node
        assert!(w.click_text("Part"), "{:?}", w.texts());
        for _ in 0..40 {
            w.frame(vec![]);
        }
        let top = w.f.owner_row.map_or(f32::MAX, |r| r.top());
        assert!(w.page.0 > 0.0 && top < 24.0, "owner row at {top}, page {:?}", w.page);
        // frames alone do not scroll again: the wheel over the owner row
        // moves the page
        let page = w.page.0;
        w.wheel(pos2(500.0, top + 12.0), 2.0);
        assert!(w.page.0 < page, "the wheel still scrolls the page: {} {page}", w.page.0);
    }

    /// A kept row's x unmaps on the second click (the first arms it); a
    /// suggested row has no x (Keep and Discard are in the top section).
    #[test]
    fn a_kept_rows_x_unmaps_on_the_second_click() {
        let ctx = Context::default();
        crate::glass::style(&ctx);
        let edges = [edge("cam0.hand.L.openness", param("Serum 2", "Cutoff"), false), edge("cam0.hand.L.y", param("Serum 2", "Res"), true)];
        let refs: Vec<&EdgeIn> = edges.iter().collect();
        let mut time = 0.0;
        let mut run = |events: Vec<Event>| {
            time += 1.0 / 60.0;
            let raw = RawInput { events, time: Some(time), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 200.0))), ..Default::default() };
            let mut gone = None;
            let out = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| gone = param_list(ui, &refs, &EN));
            });
            let mut texts = Vec::new();
            for c in &out.shapes {
                walk_texts(&c.shape, &mut texts);
            }
            (gone, texts)
        };
        let (_, texts) = run(vec![]);
        let xs: Vec<Pos2> = texts.iter().filter(|(s, _)| s == "×").map(|(_, p)| *p).collect();
        assert_eq!(xs.len(), 1, "only the kept row: {texts:?}");
        let Some(&at) = xs.first() else { return };
        let click = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        let mut got = Vec::new();
        for events in [vec![Event::PointerMoved(at), click(true)], vec![click(false)], vec![], vec![click(true)], vec![click(false)]] {
            let (gone, texts) = run(events);
            got.push((gone, texts.iter().any(|(s, _)| s == EN.unmap_arm)));
        }
        assert_eq!(got.get(1), Some(&(None, true)), "the first click arms: {got:?}");
        assert_eq!(got.get(4).and_then(|g| g.0.clone()), Some(("cam0.hand.L.openness".to_string(), param("Serum 2", "Cutoff"))), "{got:?}");
    }

    #[test]
    fn key_shows_root_before_mode() {
        let ctx = Context::default();
        crate::glass::style(&ctx);
        let w = Writer::new(Arc::new(crate::params::SynthexisParams::default()));
        let mut store = w.player.store();
        store.key = A_MINOR;
        w.update_midi(store);
        let mut f = Fingers::default();
        let mut out = None;
        for i in 0..3 {
            let raw = RawInput { time: Some(f64::from(i) / 60.0), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 200.0))), ..Default::default() };
            out = Some(ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    f.header(ui, Rect::from_min_size(pos2(80.0, 10.0), vec2(1000.0, Tok::HEAD_H)), &w, &EN);
                });
            }));
        }
        let mut texts = Vec::new();
        for c in out.iter().flat_map(|o| &o.shapes) {
            walk_texts(&c.shape, &mut texts);
        }
        let x = |s: &str| texts.iter().find(|(t, _)| t == s).map(|(_, p)| p.x);
        let (Some(root), Some(mode), Some(cal)) = (x("A"), x("Minor"), x(EN.f_calibrate)) else { return assert!(texts.is_empty(), "{texts:?}") };
        assert!(root < mode && mode < cal, "root {root}, mode {mode}, Calibrate {cal}");
        assert!(!texts.iter().any(|(s, _)| s == "Key" || s == "Tonart"), "no Key label: {texts:?}");
        let (Some(single), Some(routes)) = (x(EN.f_single), x(EN.f_routes)) else { return assert!(texts.is_empty(), "{texts:?}") };
        assert!(single < routes && routes < root);
    }

    /// Noa (screenshot 6): "Key" ran into Single | All | Routes. In the
    /// 720 pt window, in both languages, with Reset and the longest scale,
    /// the two groups keep at least `GAP` between them, and the scale field
    /// keeps its full width for "Minor".
    #[test]
    fn the_header_fits_the_720_window_in_both_languages() {
        let ctx = Context::default();
        crate::glass::style(&ctx);
        let _ = ctx.run(RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for t in [&EN, &crate::i18n::DE] {
                    let w = |s: &str, size: f32| glass::galley(ui, s, size).size().x;
                    // the section header: 720 minus the insets and the fold column;
                    // the slot sits 16 right of the title and 16 left of the close group
                    let head = 720.0 - 2.0 * Tok::INSET - 16.0;
                    let slot = head - (w(t.f_title, Tok::TITLE) + 16.0) - (Tok::CTRL_H + Tok::GAP + 16.0);
                    for reset in [false, true] {
                        for scale in Scale::ALL {
                            let hw = HeadWidths {
                                seg: [t.f_single, t.f_total, t.f_routes].iter().map(|s| (w(s, Tok::BODY) + 20.0).max(44.0)).sum(),
                                buttons: [Some(t.f_calibrate), reset.then_some(t.f_cal_reset)].into_iter().flatten().map(|s| w(s, Tok::BODY) + 16.0 + Tok::GAP).sum(),
                                scale: w(&capital(scale.name()), Tok::BODY),
                            };
                            let sw = hw.scale_w(slot);
                            assert!(hw.total(sw) <= slot + 0.01, "{} reset {reset} {scale:?}: {} in {slot}", t.f_title, hw.total(sw));
                            if scale == Scale::Minor {
                                assert!(sw >= hw.scale + SCALE_PAD - 0.01, "{} reset {reset}: Minor cut at {sw}", t.f_title);
                            }
                        }
                    }
                }
            });
        });
    }

    fn edge(signal: &str, target: NodeTarget, pending: bool) -> EdgeIn {
        EdgeIn { signal: signal.into(), target, pending }
    }

    fn param(plugin: &str, name: &str) -> NodeTarget {
        NodeTarget::Param { plugin: plugin.into(), name: name.into() }
    }

    /// The hands' canvases as `hands` places them in a content `width` wide.
    fn canvases_in(view: Rect) -> [Rect; 2] {
        let size = CANVAS as f32 * DOT;
        let dx = (view.width() / 2.0 - size / 2.0 - NODE_ROOM).clamp(size / 2.0 + 2.0, HAND_DX);
        [-dx, dx].map(|d| Rect::from_center_size(pos2(view.center().x + d, view.top() + size / 2.0), vec2(size, size)))
    }

    #[test]
    fn hand_nodes_keep_clear_of_the_fingers_and_their_nodes_at_720() {
        let art = build_art();
        // the content of the 720 pt window (insets, maybe a scroll bar)
        for width in [680.0f32, 696.0, 1176.0] {
            let view = Rect::from_min_size(pos2(12.0, 30.0), vec2(width, HANDS_H));
            let canvases = canvases_in(view);
            let clip = view.x_range();
            for (hand_w, finger_w) in [(60.0, 80.0), (130.0, 120.0), (400.0, 200.0)] {
                let mut nodes: Vec<(String, Rect)> = Vec::new();
                for (side, &canvas) in canvases.iter().enumerate() {
                    let mirror = side == 0;
                    let h = hand_node(canvas, mirror, &art, hand_w, clip);
                    let n = h.node;
                    assert!(n.left() >= clip.min - 1e-3 && n.right() <= clip.max + 1e-3, "{width} {side}: {n:?} inside the content");
                    assert!((n.height() - NODE_H).abs() < 1e-3 && n.width() >= NODE_H);
                    assert!(n.top() > canvas.center().y && n.bottom() <= canvas.bottom(), "{width} {side}: low in the canvas, above the chip row: {n:?}");
                    let outer = if mirror { n.center().x < canvas.center().x } else { n.center().x > canvas.center().x };
                    assert!(outer, "{width} {side}: lower left for the left hand, lower right for the right: {n:?}");
                    // nothing of the hand, its contour included, under the node
                    for i in 0..CANVAS * CANVAS {
                        if art.region[i] == 0 && art.outline[i] == 0 {
                            continue;
                        }
                        let p = on_canvas(canvas, [(i % CANVAS) as f32 + 0.5, (i / CANVAS) as f32 + 0.5], mirror);
                        assert!(!n.expand(1.0).contains(p), "{width} {side}: dot {i} under the hand node");
                    }
                    // the line leaves the contour and crosses no finger or palm
                    let start = dot_at(canvas, h.line[0], mirror).map(|(x, y)| art.outline[y * CANVAS + x]);
                    assert!(start.is_some_and(|o| o != 0), "{width} {side}: the line starts on the contour");
                    assert_eq!(h.line.last().map(|p| (p.y - n.center().y).abs() < 1e-3 && (p.x == n.left() || p.x == n.right())), Some(true));
                    for w in h.line.windows(2) {
                        let k = ((w[1] - w[0]).length() / 0.25).ceil().max(1.0) as usize;
                        for j in 0..=k {
                            let p = w[0] + (w[1] - w[0]) * (j as f32 / k as f32);
                            let Some((x, y)) = dot_at(canvas, p, mirror) else { continue };
                            let i = y * CANVAS + x;
                            assert!(!(art.lit[i] && (PALM..=6).contains(&art.region[i])), "{width} {side}: the hand line crosses the hand at {p:?}");
                        }
                    }
                    // every finger bound: its nodes end above the hand node
                    let ls = label_layout_above(canvas, mirror, &art, [Some(finger_w); 5], clip, h.floor());
                    assert_eq!(ls.len(), 5);
                    for l in &ls {
                        assert!(!l.node.intersects(n), "{width} {side}: finger {} node {:?} on the hand node {n:?}", l.f, l.node);
                        for p in &l.line {
                            assert!(!n.contains(*p), "{width} {side}: finger {}'s line through the hand node", l.f);
                        }
                        nodes.push((format!("{side} finger {}", l.f), l.node));
                    }
                    nodes.push((format!("{side} hand"), n));
                }
                for (i, (a, ra)) in nodes.iter().enumerate() {
                    for (b, rb) in &nodes[i + 1..] {
                        if a.contains("hand") || b.contains("hand") {
                            assert!(!ra.intersects(*rb), "{width} {hand_w}: {a} {ra:?} overlaps {b} {rb:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_contour_rings_the_hand_and_opens_at_the_wrist() {
        let art = build_art();
        let (mut dash, mut gap) = (0, 0);
        for i in 0..CANVAS * CANVAS {
            match art.outline[i] {
                0 => {}
                o => {
                    assert!(!(PALM..=6).contains(&art.region[i]), "the contour lies outside the hand");
                    let p = to_hand([(i % CANVAS) as f32 + 0.5, (i / CANVAS) as f32 + 0.5]);
                    assert!(p[1] <= OUTLINE_WRIST, "no contour below the palm");
                    if o == 1 {
                        dash += 1;
                    } else {
                        gap += 1;
                    }
                }
            }
        }
        assert!(dash > 150 && gap > 150, "dashes {dash}, gaps {gap}");
    }

    #[test]
    fn route_nodes_name_one_target_or_count_several() {
        assert_eq!(src_of("cam0.hand.R.openness"), Some(Src::Hand(1)));
        assert_eq!(src_of("cam0.hand.left.pinch"), Some(Src::Hand(0)));
        assert_eq!(src_of("cam0.hand.L.finger.index.tilt"), Some(Src::Finger(0, 1)));
        assert_eq!(src_of("cam0.hand.R.finger.pinky"), Some(Src::Finger(1, 4)));
        for none in ["cam0.hand.R.shape", "cam0.hand.R.finger_count", "cam0.arm.L.raise", "cam0.head.yaw"] {
            assert_eq!(src_of(none), None, "{none}");
        }
        let cutoff = param("Serum 2", "Cutoff");
        let one = vec![edge("cam0.hand.R.openness", cutoff.clone(), true), edge("cam0.hand.R.fist", cutoff.clone(), false)];
        let r = edges_of(&one, Src::Hand(1));
        assert_eq!(routes_text(&r, &EN).as_deref(), Some("Serum 2 · Cutoff"), "one target, however many signals");
        assert!(any_pending(&r));
        assert_eq!(routes_text(&edges_of(&one, Src::Hand(0)), &EN), None);
        let three = vec![edge("cam0.hand.L.x", cutoff.clone(), false), edge("cam0.hand.L.y", param("Serum 2", "Res"), false), edge("cam0.hand.L.depth", NodeTarget::Macro(4), false)];
        let r = edges_of(&three, Src::Hand(0));
        assert_eq!(routes_text(&r, &EN).as_deref(), Some("3 parameters"));
        assert_eq!(routes_text(&r, &crate::i18n::DE).as_deref(), Some("3 Parameter"));
        assert!(!any_pending(&r));
        let with_part = vec![edge("cam0.hand.R.pinch", NodeTarget::Part { id: 3, name: "Kick".into() }, false), edge("cam0.hand.R.x", cutoff.clone(), false)];
        assert_eq!(routes_text(&edges_of(&with_part, Src::Hand(1)), &EN).as_deref(), Some("2 routes"));
        // a finger's MIDI part is its bind, not one of its parameters
        let finger = vec![edge("cam0.hand.R.finger.index", NodeTarget::Part { id: 3, name: "Kick".into() }, false), edge("cam0.hand.R.finger.index.tilt", NodeTarget::Macro(2), true)];
        let r = edges_of(&finger, Src::Finger(1, 1));
        assert_eq!(r.len(), 1);
        assert_eq!(routes_text(&r, &EN).as_deref(), Some("Signal 3"));
        assert_eq!(graph::target_label(&param("", "Cutoff")), "Cutoff");
    }

    #[test]
    fn a_hand_route_gets_a_node_and_a_click_lists_what_the_hand_drives() {
        let mut w = Win::new(Play::Note { pitch: 60, vel: 100 }, false);
        w.width = 720.0;
        w.routing = Routing {
            edges: vec![edge("cam0.hand.R.openness", param("Serum 2", "Cutoff"), true), edge("cam0.hand.L.finger.middle", param("Vital", "Mix"), false)],
            focus: false,
            batch: 0,
        };
        w.frame(vec![]);
        w.frame(vec![]);
        assert!(text_pos(&w, "Serum 2 · Cutoff").is_some(), "{:?}", w.texts());
        assert!(text_pos(&w, "Vital · Mix").is_some(), "a finger's parameter on its label: {:?}", w.texts());
        assert!(w.f.hand_nodes[0].is_none());
        let Some((h, text)) = w.f.hand_nodes[1].clone() else { return assert!(w.f.hand_nodes[1].is_some()) };
        assert_eq!(text, "Serum 2 · Cutoff");
        assert!(h.node.right() <= 720.0 && h.node.center().x > w.f.canvas[1].center().x);
        w.press(h.node.center(), NO);
        w.release();
        assert_eq!((w.f.hand_sel, w.f.sel), (Some(1), None));
        let line = format!("{} → Serum 2 · Cutoff · {}", graph::signal_label("cam0.hand.R.openness", &EN), EN.f_param_pending);
        assert!(text_pos(&w, &fill(EN.f_hand_drives, &[&EN.hand_right])).is_some(), "{:?}", w.texts());
        assert!(text_pos(&w, &line).is_some(), "{line}: {:?}", w.texts());
        // kept: the line loses "suggested"
        w.routing.edges[0].pending = false;
        w.frame(vec![]);
        assert!(text_pos(&w, &format!("{} → Serum 2 · Cutoff", graph::signal_label("cam0.hand.R.openness", &EN))).is_some(), "{:?}", w.texts());
        // the finger that only drives a parameter opens on Parameter
        assert!(w.click_text("Vital · Mix"), "{:?}", w.texts());
        assert_eq!((w.f.hand_sel, w.f.sel, w.f.params), (None, Some(Slot::finger(0, 2)), true));
        let line = format!("{} → Vital · Mix", graph::signal_label("cam0.hand.L.finger.middle", &EN));
        assert!(text_pos(&w, &line).is_some(), "{line}: {:?}", w.texts());
        assert!(w.w.player.store().bound(&Slot::finger(0, 2).signal(), Slot::finger(0, 2).trigger()).is_none(), "no part made");
    }

    #[test]
    fn parameter_lists_the_fingers_routes_and_never_touches_its_part() {
        let mut w = Win::new(Play::Clip { bars: 1, steps: vec![st(0, 1, 60), st(4, 1, 62)], rows: vec![] }, true);
        let before = w.part();
        assert!(before.is_some());
        assert!(w.click_text(EN.f_param), "{:?}", w.texts());
        assert!(w.f.params);
        assert!(text_pos(&w, EN.f_param_none).is_some(), "{:?}", w.texts());
        assert_eq!(w.part(), before, "the pattern stays");
        let slot = Slot::finger(1, 1);
        assert_eq!(w.w.player.store().bound(&slot.signal(), slot.trigger()).map(|p| p.id), Some(w.id), "and stays bound");
        // a route from the finger shows up in the list
        w.routing = Routing { edges: vec![edge("cam0.hand.R.finger.index.tilt", param("Vital", "Mix"), false)], focus: false, batch: 0 };
        w.frame(vec![]);
        let line = format!("{} → Vital · Mix", graph::signal_label("cam0.hand.R.finger.index.tilt", &EN));
        assert!(text_pos(&w, &line).is_some(), "{line}: {:?}", w.texts());
        assert!(text_pos(&w, EN.f_param_none).is_none());
        // the part's kind brings the part back as it was
        assert!(w.click_text(EN.f_pattern), "{:?}", w.texts());
        assert!(!w.f.params);
        assert_eq!(w.part(), before);
        assert!(w.f.geom.is_some(), "the roll is back");
        // a finger without a part: Parameter makes none
        let mut w = finger_without_a_part();
        assert!(w.click_text(EN.f_param), "{:?}", w.texts());
        w.frame(vec![]);
        assert!(w.f.params && text_pos(&w, EN.f_param_none).is_some(), "{:?}", w.texts());
        assert!(w.w.player.store().parts.is_empty());
        // a wall has no Parameter
        let w = Win::wall(Side::Top, Vec::new());
        assert!(text_pos(&w, EN.f_param).is_none());
    }

    /// The header for `f` in one frame with `events`; its texts.
    fn header_frame(ctx: &Context, f: &mut Fingers, w: &Writer, time: f64, events: Vec<Event>) -> Vec<(String, Pos2)> {
        let raw = RawInput { events, time: Some(time), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 200.0))), ..Default::default() };
        let out = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                f.header(ui, Rect::from_min_size(pos2(80.0, 10.0), vec2(1000.0, Tok::HEAD_H)), w, &EN);
            });
        });
        let mut texts = Vec::new();
        for c in &out.shapes {
            walk_texts(&c.shape, &mut texts);
        }
        texts
    }

    #[test]
    fn reset_stops_a_running_calibration_at_once() {
        let mut w = Win::new(Play::Note { pitch: 60, vel: 100 }, false);
        w.f.calib = Some(crate::calib::Calib::default());
        w.frame(vec![]);
        let showing = fill(EN.f_cal_show, &[&EN.f_pose_fist]);
        assert!(text_pos(&w, &showing).is_some(), "{:?}", w.texts());
        assert!(w.f.calibrating());
        // Reset shows while a run goes, stored thresholds or not
        let ctx = Context::default();
        crate::glass::style(&ctx);
        let texts = (0..3).map(|i| header_frame(&ctx, &mut w.f, &w.w, f64::from(i) / 60.0, vec![])).last().unwrap_or_default();
        let reset = texts.iter().find(|(s, _)| s == EN.f_cal_reset).map(|(_, p)| *p);
        assert!(reset.is_some(), "{texts:?}");
        if crate::settings::store().get().fingers.is_none() {
            // no stored thresholds: the click writes no settings
            if let Some(p) = reset {
                let p = p + vec2(4.0, 4.0);
                header_frame(&ctx, &mut w.f, &w.w, 0.1, vec![Event::PointerMoved(p)]);
                header_frame(&ctx, &mut w.f, &w.w, 0.12, vec![Event::PointerButton { pos: p, button: P, pressed: true, modifiers: NO }]);
                header_frame(&ctx, &mut w.f, &w.w, 0.14, vec![Event::PointerButton { pos: p, button: P, pressed: false, modifiers: NO }]);
            }
        } else {
            assert!(!w.f.reset(false));
        }
        assert!(!w.f.calibrating() && w.f.cal_still.is_none(), "the run stopped");
        w.frame(vec![]);
        assert!(text_pos(&w, &showing).is_none(), "the progress line went: {:?}", w.texts());
        // a stored calibration goes on Reset too
        w.f.calib = Some(crate::calib::Calib::default());
        assert!(w.f.reset(true));
        assert!(!w.f.calibrating());
    }
}
