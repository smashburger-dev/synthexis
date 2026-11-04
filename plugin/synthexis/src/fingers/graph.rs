//! The Routes view of the Hands section (ui-redesign R19): where the
//! hands go, as three columns of nodes. Hands on the left, one source node
//! per (hand, signal) in the middle, the targets on the right grouped by
//! plugin ("Signals" and "Parts" for macros and MIDI parts). Edges from a
//! source to a target are cubic Beziers drawn as dither dots that switch
//! from the source's colour to the target's in the middle half; where an
//! edge enters a node, an 8 pt band of the node dithers in from the hand's
//! colour. A kept route's boxes change it (fingers.rs `routes`): the
//! target box opens a search over the plugin's parameters and Signals,
//! the source box one over the hand movements (param_find.rs), and the
//! `x` at the target box's right end removes it.

use nih_plug_egui::egui::emath::GuiRounding;
use nih_plug_egui::egui::{pos2, vec2, Color32, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Ui};

pub use crate::edges::*;
use crate::glass::{self, black, white, Tok};
use crate::i18n::{fill, Text};

use super::{mask_name, BAYER, HAND};

/// Row pitch and node height of the view.
pub const ROW: f32 = 30.0;
pub const NODE_H: f32 = 24.0;
/// Space above the first and below the last row.
const PAD: f32 = 8.0;
const HAND_NODE_W: f32 = 112.0;
const SRC_X: f32 = 136.0;
const SRC_W: f32 = 120.0;
/// Targets shown; the rest go into one "+N more" node.
pub const MAX_TARGETS: usize = 16;
/// Edge dots: square side and spacing along the arc, points.
pub const DOT: f32 = 1.5;
pub const DOT_STEP: f32 = 2.0;
/// A pending edge: dash and gap lengths along the arc, and its alpha.
const DASH: f32 = 6.0;
const DASH_GAP: f32 = 4.0;
const PENDING_A: f32 = 0.6;
/// The dither band where an edge enters a node.
const BAND_W: f32 = 8.0;
const BAND_A: f32 = 0.35;
const NODE_FILL: f32 = 0.55;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Hand(usize),
    Source(usize),
    Target,
    /// A group label row ("Serum 2", "Signals"): text only, no box.
    Group,
    More,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    /// Relative to the view's origin.
    pub rect: Rect,
    pub text: String,
    pub colour: Color32,
    pub kind: NodeKind,
    /// A target only pending edges reach: drawn hollow.
    pub pending: bool,
    /// The hand colours that dither in at the entry side: one, or the
    /// left and the right hand's for the top and bottom half.
    pub band: Vec<Color32>,
    pub target: Option<NodeTarget>,
    /// A source's signal.
    pub signal: Option<String>,
    /// Room kept free at the right end for a control (the remove `x`).
    pub tail: f32,
}

/// The box of the Routes view whose search is open.
#[derive(Clone, Debug, PartialEq)]
pub enum Finding {
    Target(NodeTarget),
    /// A source, by its signal.
    Source(String),
}

/// What the Routes view keeps between frames and hands to the window.
#[derive(Debug, Default)]
pub struct Edits {
    pub finding: Option<Finding>,
    /// The plugin whose parameter names were asked for last.
    pub wanted: Option<String>,
    /// Kept routes to retarget and to remove (Sentence::retarget_kept,
    /// unmap_kept), as (signal, target).
    pub retarget: Vec<(String, NodeTarget, crate::param_find::Found)>,
    pub unmap: Vec<(String, NodeTarget)>,
    /// Opens the first kept target box's search once (the window
    /// captures, Fingers::shot_routes_find).
    pub shot_find: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub a: Pos2,
    pub b: Pos2,
    pub colour: Color32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dot {
    /// The dot's centre, relative to the view's origin.
    pub at: Pos2,
    pub colour: Color32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub lines: Vec<Line>,
    pub dots: Vec<Dot>,
    pub height: f32,
}

/// The side of a `cam0.hand.<L|R>.…` signal and what follows it
/// (`left`/`right` read the same).
pub(super) fn split(signal: &str) -> Option<(usize, &str)> {
    let rest = signal.strip_prefix("cam0.hand.")?;
    let (side, feat) = rest.split_once('.')?;
    match side {
        "L" | "left" => Some((0, feat)),
        "R" | "right" => Some((1, feat)),
        _ => None,
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// The source node's text: "Spread", "Index tilt", "Index shown" (a
/// finger bind, `…finger.index`), "Fist" or "Open hand" (a shape bind,
/// `…shape.<mask>`).
pub fn signal_label(signal: &str, t: &Text) -> String {
    if let Some(face) = face_label(signal, t) {
        return face.into();
    }
    let Some((_, feat)) = split(signal) else { return signal.to_string() };
    let finger = |name: &str| ["thumb", "index", "middle", "ring", "pinky"].iter().position(|f| *f == name).map(|i| capital(t.fingers()[i]));
    match feat {
        "openness" => t.g_openness.into(),
        "fist" => t.f_fist.into(),
        "spread" => t.g_spread.into(),
        "x" => t.g_x.into(),
        "y" => t.g_y.into(),
        "depth" => t.g_depth.into(),
        "pinch" => t.g_pinch.into(),
        "speed" => t.g_speed.into(),
        "roll" => t.g_roll.into(),
        "finger_count" => t.g_count.into(),
        _ => {
            if let Some(f) = feat.strip_prefix("finger.").and_then(|f| f.strip_suffix(".tilt")).and_then(finger) {
                return fill(t.g_tilt, &[&f]);
            }
            if let Some(f) = feat.strip_prefix("finger.").and_then(finger) {
                return fill(t.g_shown, &[&f]);
            }
            if let Some(m) = feat.strip_prefix("shape.").and_then(|m| m.parse::<u8>().ok()) {
                return mask_name(m & 31, t);
            }
            capital(feat)
        }
    }
}

/// A face or head signal's movement, without its part (the Head list
/// names the part above): "Open", "Smile", "Closed", "Turn".
fn face_label(signal: &str, t: &Text) -> Option<&'static str> {
    let rest = signal.strip_prefix("cam0.")?;
    let rest = rest.strip_prefix("eye.L.").or_else(|| rest.strip_prefix("eye.R.")).unwrap_or(rest);
    Some(match rest {
        "mouth.open" | "open" => t.g_open,
        "closed" => t.g_closed,
        "face.smile" => t.g_smile,
        "head.yaw" => t.g_turn,
        "head.pitch" => t.g_nod,
        "head.roll" => t.g_head_tilt,
        _ => return None,
    })
}

/// A plugin's own colour: OKLCH L 0.78, C 0.10, the hue from a hash of the
/// name, kept out of 36..86 and 197..247 degrees (the hands' hues).
pub fn plugin_colour(name: &str) -> Color32 {
    // FNV-1a
    let h = name.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
    let u = (h % 2600) as f32 / 10.0;
    let hue = if u < 36.0 {
        u
    } else if u < 147.0 {
        u + 50.0
    } else {
        u + 100.0
    };
    oklch(0.78, 0.10, hue)
}

/// OKLCH (L 0..1, C, hue in degrees) to sRGB.
fn oklch(l: f32, c: f32, hue: f32) -> Color32 {
    let (a, b) = (c * hue.to_radians().cos(), c * hue.to_radians().sin());
    let l_ = l + 0.396_337_8 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    let r = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
    let g = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3;
    let bl = -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;
    let enc = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        let v = if x <= 0.003_130_8 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
        (v * 255.0).round() as u8
    };
    Color32::from_rgb(enc(r), enc(g), enc(bl))
}

/// The group a target belongs to.
#[derive(Clone, Debug, PartialEq)]
enum Group {
    Plugin(String),
    Signals,
    Parts,
}

fn group_of(t: &NodeTarget) -> Group {
    match t {
        NodeTarget::Param { plugin, .. } => Group::Plugin(plugin.clone()),
        NodeTarget::Macro(_) => Group::Signals,
        NodeTarget::Part { .. } => Group::Parts,
    }
}

fn target_text(t: &NodeTarget) -> String {
    match t {
        NodeTarget::Param { name, .. } => name.clone(),
        NodeTarget::Macro(i) => format!("Signal {}", i + 1),
        NodeTarget::Part { name, .. } => name.clone(),
    }
}

/// A target as one line, plugin first: "Serum 2 · Cutoff", "Signal 3",
/// a part's name.
pub fn target_label(t: &NodeTarget) -> String {
    match t {
        NodeTarget::Param { plugin, name } if !plugin.is_empty() => format!("{plugin} · {name}"),
        _ => target_text(t),
    }
}

fn node_rect(x: f32, w: f32, row: f32) -> Rect {
    Rect::from_min_size(pos2(x, PAD + row * ROW + (ROW - NODE_H) / 2.0), vec2(w, NODE_H))
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The cubic Bezier from `a` to `b` with horizontal tangents (control
/// points at half the x distance), as dither dots every `DOT_STEP` of arc
/// length. Dot i takes `to` when smoothstep(0.25, 0.75, t_i) is above its
/// BAYER4 threshold, else `from`. Pending: dashes of `DASH` with `DASH_GAP`
/// gaps, at 0.6 alpha.
pub fn edge_dots(a: Pos2, b: Pos2, from: Color32, to: Color32, pending: bool) -> Vec<Dot> {
    let dx = (b.x - a.x) / 2.0;
    let (c1, c2) = (pos2(a.x + dx, a.y), pos2(b.x - dx, b.y));
    let at = |t: f32| {
        let u = 1.0 - t;
        let (k0, k1, k2, k3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        pos2(k0 * a.x + k1 * c1.x + k2 * c2.x + k3 * b.x, k0 * a.y + k1 * c1.y + k2 * c2.y + k3 * b.y)
    };
    const N: usize = 128;
    let pts: Vec<Pos2> = (0..=N).map(|i| at(i as f32 / N as f32)).collect();
    let mut acc = vec![0.0f32; pts.len()];
    for i in 1..pts.len() {
        acc[i] = acc[i - 1] + (pts[i] - pts[i - 1]).length();
    }
    let total = acc.last().copied().unwrap_or(0.0);
    if total < DOT_STEP {
        return Vec::new();
    }
    let n = (total / DOT_STEP).floor() as usize;
    let mut dots = Vec::with_capacity(n + 1);
    let mut seg = 1;
    for i in 0..=n {
        let s = i as f32 * DOT_STEP;
        if pending && s % (DASH + DASH_GAP) >= DASH {
            continue;
        }
        while seg + 1 < pts.len() && acc[seg] < s {
            seg += 1;
        }
        let (s0, s1) = (acc[seg - 1], acc[seg]);
        let k = if s1 > s0 { ((s - s0) / (s1 - s0)).clamp(0.0, 1.0) } else { 0.0 };
        let p = pts[seg - 1] + (pts[seg] - pts[seg - 1]) * k;
        let t = s / total;
        let c = if smoothstep(0.25, 0.75, t) > (BAYER[i % 16] + 0.5) / 16.0 { to } else { from };
        dots.push(Dot { at: p, colour: if pending { c.gamma_multiply(PENDING_A) } else { c } });
    }
    dots
}

/// The view for `edges` in `width` (the content width C): column 1 at
/// x 0 (hands, 112 wide), column 2 at 136 (sources, 120), column 3 at
/// 0.55 C (targets, min(240, 0.45 C - 16)); rows on a 30 pt pitch, group
/// labels as rows of their own. Only signals of the hands (`cam0.hand.`)
/// count. Height `rows * 30 + 16`.
pub fn layout(edges: &[EdgeIn], width: f32, t: &Text) -> Graph {
    let edges: Vec<(usize, &EdgeIn)> = edges.iter().filter_map(|e| split(&e.signal).map(|(s, _)| (s, e))).collect();
    if edges.is_empty() {
        return Graph { height: PAD * 2.0, ..Graph::default() };
    }
    // targets in order of first use; the first MAX_TARGETS get nodes
    let mut targets: Vec<&NodeTarget> = Vec::new();
    for (_, e) in &edges {
        if !targets.contains(&&e.target) {
            targets.push(&e.target);
        }
    }
    let more = targets.len().saturating_sub(MAX_TARGETS);
    let shown = &targets[..targets.len().min(MAX_TARGETS)];
    let mut groups: Vec<Group> = Vec::new();
    for tg in shown {
        let g = group_of(tg);
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    let tx = (0.55 * width).round();
    let tw = (0.45 * width - 16.0).clamp(40.0, 240.0);
    let mut g = Graph::default();
    // column 3: group label, then its targets
    let mut row = 0.0f32;
    let mut target_row: Vec<(&NodeTarget, f32, usize)> = Vec::new();
    for grp in &groups {
        let (label, colour) = match grp {
            Group::Plugin(p) if p.is_empty() => (t.plugin.to_string(), white(Tok::VALUE)),
            Group::Plugin(p) => (p.clone(), plugin_colour(p)),
            Group::Signals => (t.g_signals.to_string(), white(Tok::VALUE)),
            Group::Parts => (t.g_parts.to_string(), white(Tok::VALUE)),
        };
        g.nodes.push(Node { rect: node_rect(tx, tw, row), text: label, colour, kind: NodeKind::Group, pending: false, band: Vec::new(), target: None, signal: None, tail: 0.0 });
        row += 1.0;
        for tg in shown.iter().filter(|tg| group_of(tg) == *grp) {
            let into: Vec<&(usize, &EdgeIn)> = edges.iter().filter(|(_, e)| e.target == **tg).collect();
            let mut sides: Vec<usize> = into.iter().map(|(s, _)| *s).collect();
            sides.sort_unstable();
            sides.dedup();
            let colour = match tg {
                NodeTarget::Param { plugin, .. } if !plugin.is_empty() => plugin_colour(plugin),
                NodeTarget::Part { .. } => HAND[sides.first().copied().unwrap_or(0).min(1)],
                _ => white(Tok::VALUE),
            };
            let pending = into.iter().all(|(_, e)| e.pending);
            target_row.push((tg, row, g.nodes.len()));
            g.nodes.push(Node {
                rect: node_rect(tx, tw, row),
                text: target_text(tg),
                colour,
                kind: NodeKind::Target,
                pending,
                band: sides.iter().map(|s| HAND[(*s).min(1)]).collect(),
                target: Some((*tg).clone()),
                signal: None,
                tail: 0.0,
            });
            row += 1.0;
        }
    }
    let more_node = (more > 0).then(|| {
        g.nodes.push(Node { rect: node_rect(tx, tw, row), text: fill(t.g_more, &[&more]), colour: white(Tok::HINT), kind: NodeKind::More, pending: false, band: Vec::new(), target: None, signal: None, tail: 0.0 });
        row += 1.0;
        (row - 1.0, g.nodes.len() - 1)
    });
    let target_at = |tg: &NodeTarget| target_row.iter().find(|(x, _, _)| *x == tg).map(|(_, r, i)| (*r, *i)).or(more_node);
    // column 2: one source per (hand, signal), left hand first, each as
    // close to the row of its first target as the rows above allow
    let mut sources: Vec<(usize, &str, f32, usize)> = Vec::new();
    for (k, (side, e)) in edges.iter().enumerate() {
        if !sources.iter().any(|(s, sig, _, _)| s == side && *sig == e.signal) {
            sources.push((*side, &e.signal, target_at(&e.target).map_or(0.0, |(r, _)| r), k));
        }
    }
    sources.sort_by(|a, b| (a.0, a.2, a.3).partial_cmp(&(b.0, b.2, b.3)).unwrap_or(std::cmp::Ordering::Equal));
    let mut next = 0.0f32;
    let mut src_node: Vec<(usize, &str, usize)> = Vec::new();
    for (side, sig, want, _) in &sources {
        let r = want.max(next);
        next = r + 1.0;
        src_node.push((*side, sig, g.nodes.len()));
        g.nodes.push(Node { rect: node_rect(SRC_X, SRC_W, r), text: signal_label(sig, t), colour: HAND[*side], kind: NodeKind::Source(*side), pending: false, band: vec![HAND[*side]], target: None, signal: Some(sig.to_string()), tail: 0.0 });
    }
    // column 1: a hand at the mean height of its sources
    for side in 0..2 {
        let ys: Vec<f32> = src_node.iter().filter(|(s, _, _)| *s == side).map(|(_, _, i)| g.nodes[*i].rect.center().y).collect();
        if ys.is_empty() {
            continue;
        }
        let cy = ys.iter().sum::<f32>() / ys.len() as f32;
        let hand = Rect::from_center_size(pos2(HAND_NODE_W / 2.0, cy), vec2(HAND_NODE_W, NODE_H));
        for y in &ys {
            g.lines.push(Line { a: pos2(hand.right(), cy), b: pos2(SRC_X, *y), colour: HAND[side] });
        }
        g.nodes.push(Node { rect: hand, text: [t.hand_left, t.hand_right][side].to_string(), colour: HAND[side], kind: NodeKind::Hand(side), pending: false, band: Vec::new(), target: None, signal: None, tail: 0.0 });
    }
    // source -> target edges, once per pair
    let mut done: Vec<(usize, usize)> = Vec::new();
    for (side, e) in &edges {
        let Some(si) = src_node.iter().find(|(s, sig, _)| s == side && *sig == e.signal).map(|(_, _, i)| *i) else { continue };
        let Some((_, ti)) = target_at(&e.target) else { continue };
        if done.contains(&(si, ti)) {
            continue;
        }
        done.push((si, ti));
        let pending = edges.iter().filter(|(s, x)| s == side && x.signal == e.signal && target_at(&x.target).map(|(_, i)| i) == Some(ti)).all(|(_, x)| x.pending);
        let (a, b) = (g.nodes[si].rect.right_center(), g.nodes[ti].rect.left_center());
        g.dots.extend(edge_dots(a, b, g.nodes[si].colour, g.nodes[ti].colour, pending));
    }
    let rows = row.max(next);
    g.height = rows * ROW + 2.0 * PAD;
    g
}

/// Paints `g` with its origin at `origin`: edges first, then the nodes.
pub fn paint(ui: &mut Ui, g: &Graph, origin: Pos2) {
    let p = ui.painter();
    let o = origin.to_vec2();
    for l in &g.lines {
        let (a, b) = ((l.a + o).round_to_pixel_center(p.pixels_per_point()), (l.b + o).round_to_pixel_center(p.pixels_per_point()));
        p.line_segment([a, b], Stroke::new(1.0_f32, l.colour));
    }
    for d in &g.dots {
        let min = (d.at + o - vec2(DOT / 2.0, DOT / 2.0)).round_to_pixels(p.pixels_per_point());
        p.rect_filled(Rect::from_min_size(min, vec2(DOT, DOT)), 0.0, d.colour);
    }
    for n in &g.nodes {
        let r = n.rect.translate(o);
        if n.kind == NodeKind::Group {
            let gal = glass::galley(ui, &glass::fit(ui, &n.text, Tok::BODY, r.width()), Tok::BODY);
            glass::text_at(p, pos2(r.left(), r.center().y - gal.size().y / 2.0), gal, n.colour);
            continue;
        }
        let cr = CornerRadius::same(Tok::R_TOGGLE);
        let edge_a = if n.pending { PENDING_A } else { 1.0 };
        if !n.pending {
            p.rect_filled(r, cr, black(NODE_FILL));
            paint_band(p, r, &n.band);
        }
        p.rect_stroke(r, cr, Stroke::new(1.0_f32, n.colour.gamma_multiply(edge_a)), StrokeKind::Inside);
        let a = if n.kind == NodeKind::More { Tok::HINT } else { Tok::VALUE };
        let gal = glass::galley(ui, &glass::fit(ui, &n.text, Tok::BODY, (r.width() - 2.0 * BAND_W - 4.0 - n.tail).max(0.0)), Tok::BODY);
        glass::text_at(p, pos2(r.left() + BAND_W + 2.0, r.center().y - gal.size().y / 2.0), gal, white(a));
    }
}

/// The entry band: `DOT` cells over the node's left `BAND_W` that show the
/// band colour at 0.35 where 1 - x/BAND_W beats the BAYER4 threshold; with
/// two colours the top half takes the first, the bottom half the second.
/// Cells outside the rounded corners stay empty.
fn paint_band(p: &nih_plug_egui::egui::Painter, r: Rect, band: &[Color32]) {
    if band.is_empty() {
        return;
    }
    let rad = f32::from(Tok::R_TOGGLE);
    let cols = (BAND_W / DOT).round() as usize;
    let rows = (r.height() / DOT).floor() as usize;
    for j in 0..cols {
        for k in 0..rows {
            let c = pos2(r.left() + (j as f32 + 0.5) * DOT, r.top() + (k as f32 + 0.5) * DOT);
            // inside the rounded rect
            let qx = (r.left() + rad - c.x).max(0.0);
            let qy = ((r.top() + rad - c.y).max(0.0)).max((c.y - (r.bottom() - rad)).max(0.0));
            if qx * qx + qy * qy > rad * rad {
                continue;
            }
            let f = (j as f32 + 0.5) * DOT / BAND_W;
            if 1.0 - f <= (BAYER[(k % 4) * 4 + j % 4] + 0.5) / 16.0 {
                continue;
            }
            let col = if band.len() > 1 && c.y > r.center().y { band[1] } else { band[0] };
            let min = (c - vec2(DOT / 2.0, DOT / 2.0)).round_to_pixels(p.pixels_per_point());
            p.rect_filled(Rect::from_min_size(min, vec2(DOT, DOT)), 0.0, col.gamma_multiply(BAND_A));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::EN;

    fn param(plugin: &str, name: &str) -> NodeTarget {
        NodeTarget::Param { plugin: plugin.into(), name: name.into() }
    }

    fn e(signal: &str, target: NodeTarget, pending: bool) -> EdgeIn {
        EdgeIn { signal: signal.into(), target, pending }
    }

    #[test]
    fn routing_layout_has_no_overlaps_and_hand_colours() {
        let edges = vec![
            e("cam0.hand.L.spread", param("Serum 2", "Cutoff"), false),
            e("cam0.hand.R.openness", NodeTarget::Macro(10), false),
            e("cam0.hand.R.pinch", param("Serum 2", "Resonance"), true),
            e("cam0.hand.R.finger.index", NodeTarget::Part { id: 3, name: "Trap roll".into() }, false),
            e("cam0.hand.L.openness", param("Serum 2", "Cutoff"), false),
            e("cam0.face.smile", NodeTarget::Macro(1), false),
            e("cam0.hand.R.shape.0", param("Vital", "Drive"), false),
            e("cam0.hand.L.finger.ring.tilt", param("Serum 2", "Cutoff"), false),
        ];
        let width = 1176.0;
        let g = layout(&edges, width, &EN);
        for (i, a) in g.nodes.iter().enumerate() {
            for b in &g.nodes[i + 1..] {
                assert!(!a.rect.intersects(b.rect), "{} {:?} overlaps {} {:?}", a.text, a.rect, b.text, b.rect);
            }
        }
        let hands: Vec<&Node> = g.nodes.iter().filter(|n| matches!(n.kind, NodeKind::Hand(_))).collect();
        assert_eq!(hands.len(), 2);
        for h in hands {
            let NodeKind::Hand(s) = h.kind else { continue };
            assert_eq!(h.colour, HAND[s]);
            assert_eq!(h.text, [EN.hand_left, EN.hand_right][s]);
            assert_eq!((h.rect.left(), h.rect.width()), (0.0, 112.0));
        }
        let texts: Vec<&str> = g.nodes.iter().map(|n| n.text.as_str()).collect();
        for want in ["Spread", "Openness", "Pinch", "Index shown", "Fist", "Ring tilt", "Serum 2", "Signals", "Parts", "Vital", "Signal 11", "Cutoff"] {
            assert!(texts.contains(&want), "{want} in {texts:?}");
        }
        assert!(!texts.contains(&"Smile") && !texts.contains(&"Signal 2"), "only the hands' signals: {texts:?}");
        // the left hand's sources come first, every source in its hand colour
        let src: Vec<&Node> = g.nodes.iter().filter(|n| matches!(n.kind, NodeKind::Source(_))).collect();
        assert_eq!(src.len(), 7);
        let first_right = src.iter().position(|n| n.kind == NodeKind::Source(1)).unwrap_or(0);
        assert!(src[..first_right].iter().all(|n| n.kind == NodeKind::Source(0)) && first_right == 3, "{src:?}");
        assert!(src.iter().all(|n| (n.rect.left(), n.rect.width()) == (136.0, 120.0)));
        for n in &src {
            let NodeKind::Source(s) = n.kind else { continue };
            assert_eq!(n.colour, HAND[s]);
        }
        // targets: x 0.55 C, width min(240, 0.45 C - 16), one node per target
        let tg: Vec<&Node> = g.nodes.iter().filter(|n| n.kind == NodeKind::Target).collect();
        assert_eq!(tg.len(), 5);
        assert!(tg.iter().all(|n| (n.rect.left(), n.rect.width()) == ((0.55f32 * width).round(), 240.0)));
        let cutoff = tg.iter().find(|n| n.text == "Cutoff");
        assert_eq!(cutoff.map(|n| n.colour), Some(plugin_colour("Serum 2")));
        assert_eq!(cutoff.map(|n| n.band.len()), Some(1), "only the left hand feeds it");
        assert!(tg.iter().find(|n| n.text == "Resonance").is_some_and(|n| n.pending), "a pending target is hollow");
        assert_eq!(tg.iter().find(|n| n.text == "Trap roll").map(|n| n.colour), Some(HAND[1]));
        // rows: groups take a row each; the height counts them
        let rows = ((g.height - 16.0) / ROW).round() as usize;
        assert!(rows >= 5 + 4, "five targets and four group rows: {rows}");
        assert!(g.nodes.iter().all(|n| n.rect.bottom() <= g.height));
        // a target both hands feed splits its band
        let both = layout(&[e("cam0.hand.L.x", NodeTarget::Macro(0), false), e("cam0.hand.R.y", NodeTarget::Macro(0), false)], width, &EN);
        assert_eq!(both.nodes.iter().find(|n| n.kind == NodeKind::Target).map(|n| n.band.clone()), Some(vec![HAND[0], HAND[1]]));
        // more than 16 targets: one "+N more" node
        let many: Vec<EdgeIn> = (0..20).map(|i| e("cam0.hand.R.x", NodeTarget::Macro(i), false)).collect();
        let g = layout(&many, width, &EN);
        assert_eq!(g.nodes.iter().filter(|n| n.kind == NodeKind::Target).count(), 16);
        assert!(g.nodes.iter().any(|n| n.kind == NodeKind::More && n.text == "+4 more"));
        // plugin colours stay out of the hands' hue bands and are stable
        for name in ["Serum 2", "Vital", "Sampler", "FPC", "Diva", "Pro-Q 3", "Sytrus", "Harmor"] {
            assert_eq!(plugin_colour(name), plugin_colour(name));
            assert!(!HAND.contains(&plugin_colour(name)));
        }
        assert!(layout(&[], width, &EN).nodes.is_empty());
        // the source labels
        assert_eq!(signal_label("cam0.hand.R.finger.index.tilt", &EN), "Index tilt");
        assert_eq!(signal_label("cam0.hand.R.shape.31", &EN), "Open hand");
        assert_eq!(signal_label("cam0.hand.R.shape.17", &EN), "Thumb and pinky");
        assert_eq!(signal_label("cam0.hand.R.depth", &EN), "Distance");
        assert_eq!(signal_label("cam0.hand.R.finger_count", &EN), "Finger count");
    }

    #[test]
    fn a_dither_edge_switches_colour_in_the_middle_half() {
        let (from, to) = (HAND[0], Color32::from_rgb(10, 200, 30));
        let dots = edge_dots(pos2(256.0, 20.0), pos2(650.0, 140.0), from, to, false);
        assert!(dots.len() > 100, "{}", dots.len());
        let n = dots.len() as f32;
        for (i, d) in dots.iter().enumerate() {
            let t = i as f32 / n;
            if t < 0.24 {
                assert_eq!(d.colour, from, "dot {i} of {n} before the middle half");
            }
            if t > 0.76 {
                assert_eq!(d.colour, to, "dot {i} of {n} after the middle half");
            }
        }
        let mid: Vec<&Dot> = dots.iter().enumerate().filter(|(i, _)| (0.4..0.6).contains(&(*i as f32 / n))).map(|(_, d)| d).collect();
        assert!(mid.iter().any(|d| d.colour == from) && mid.iter().any(|d| d.colour == to), "both colours meet in the middle");
        // dots every 2 pt along the arc
        for w in dots.windows(2) {
            let d = (w[1].at - w[0].at).length();
            assert!(d > 1.6 && d < 2.05, "{d}");
        }
        // pending: dashes, quieter
        let pend = edge_dots(pos2(256.0, 20.0), pos2(650.0, 140.0), from, to, true);
        assert!(pend.len() < dots.len() * 7 / 10 && pend.len() > dots.len() / 2, "{} of {}", pend.len(), dots.len());
        assert!(pend.iter().all(|d| d.colour.a() < 255));
    }
}
