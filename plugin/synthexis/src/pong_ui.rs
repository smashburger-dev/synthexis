//! The Pong section of the window (design pong-presets.md, sections 7, 8
//! and 11; ui-redesign.md R21 to R23): the header's On toggle, Live chip
//! and stats; the body's centred column with the shooting hand, the side
//! view (the 16:9 frame as a dotted outline in the hand art's style, a
//! chip per side naming what a hit does) and the compact settings; the
//! selected side's editor; plus what Pong draws on the camera: the walls,
//! hit flashes, projectiles (purple to blue) with a trail that narrows and
//! ends in dither, and the click bands that pick a wall.

use std::collections::VecDeque;

use nih_plug_egui::egui::{self, pos2, vec2, Align, Color32, CornerRadius, Id, Layout, Painter, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};

use crate::glass::{self, white, Tok};
use crate::fingers::{Fingers, HAND};
use crate::i18n::{fill, Text};
use crate::midi::{self, Play};
use crate::param_find::{self, Found};
use crate::pong::arena::Side;
use crate::pong::setup::{Action, Hands, How, Mode, Setup};
use crate::pong::View;
use crate::sentence::Sentence;
use crate::writer::Writer;

/// A hit's flash fades over this many seconds.
const FLASH: f32 = 0.3;
/// Preview positions kept per projectile for its trail.
const TRAIL: usize = 24;
/// The projectile's radius on the camera.
const BALL_R: f32 = 3.0;
/// A trail is at most this long on the camera (points), from the ball back.
const TRAIL_LEN: f32 = 110.0;
/// The share of a trail drawn solid; the rest is dither.
const SOLID: f32 = 0.5;
/// The dither's pixel: cell size and sampling step along the trail.
const CELL: f32 = 1.0;
/// 4x4 ordered dither thresholds (backdrop.rs uses the 8x8 one).
const BAYER4: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
/// A projectile this far (preview units) from a trail's last point
/// between two frames is another one.
const MATCH: f32 = 0.15;
/// The side view's widest.
const VIEW_W: f32 = 360.0;
/// The side view's click band, centred on each side.
const SIDE_BAND: f32 = 14.0;
/// The canvas' click band, inside each edge of the image.
const CANVAS_BAND: f32 = 12.0;
/// The window's top bar: the canvas' top band starts under it.
const BAR_H: f32 = 30.0;
/// Dither dots of the outline: spacing and radius (selected: 2 pt dots).
const DOT_STEP: f32 = 4.0;
const DOT_R: f32 = 0.8;
const DOT_R_SEL: f32 = 1.0;
/// "never" at the right end of the bounces slider.
const BOUNCES_NEVER: i32 = 33;
/// The compact sliders' width: Speed and Bounces stacked left of the side
/// view, Time limit and Max balls right of it.
const STACK_W: f32 = 140.0;
/// Between a slider stack and the side view (clear of its click band).
const STACK_GAP: f32 = 16.0;
/// The side view never narrower than this when the window is.
const VIEW_MIN: f32 = 160.0;
/// Test shot while Pong or Live is off: dim, still readable (R22).
const TEST_OFF: f32 = 0.40;

/// What the header asks of the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PongHead {
    /// The Live is off chip was clicked: the window turns Live on.
    pub live_on: bool,
}

/// Where the body drew its rows last (the tests measure them).
struct Rects {
    hand: Rect,
    sliders: [Rect; 4],
    toggles: Vec<Rect>,
    /// One per line of toggles.
    lines: Vec<Rect>,
}

impl Default for Rects {
    fn default() -> Self {
        Rects { hand: Rect::NOTHING, sliders: [Rect::NOTHING; 4], toggles: Vec::new(), lines: Vec::new() }
    }
}

/// What the selected side's editor shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Off,
    Midi,
    Param,
}

/// Lives in the editor's UiState as long as the plugin instance.
#[derive(Default)]
pub struct PongUi {
    /// The wall picked on the canvas or in the side view.
    pub sel: Option<Side>,
    /// The scope last handed to the sentence field.
    sent: Option<Side>,
    /// A kind chosen for a side that is still Off (MIDI before its part,
    /// Parameter before its target).
    tab: Option<(Side, Kind)>,
    /// The last preview positions of each projectile in `view.shots`
    /// order. View has no projectile ids, so a trail follows the nearest
    /// projectile from frame to frame.
    trails: Vec<VecDeque<[f32; 2]>>,
    view: View,
    state_gen: u64,
    /// The plugin whose parameter names were asked for last.
    wanted: Option<String>,
    /// Where the side view was drawn last (the tests aim with it).
    view_rect: Option<Rect>,
    /// Where the header's stats and the body's rows were drawn last.
    stats_rect: Option<Rect>,
    rects: Rects,
    /// The side whose editor was drawn last: another one hands the wall
    /// editor a fresh start (Fingers::wall_switched).
    drawn: Option<Side>,
    /// Stand-in projectile paths, oldest point first (the window
    /// captures, shot v).
    shot_balls: Option<Vec<Vec<[f32; 2]>>>,
    /// Opens the selected wall's target search once (shot w).
    shot_find: bool,
}

impl PongUi {
    /// Once per frame before anything is drawn: the runtime's snapshot,
    /// the trails, a reload after a preset or project load.
    pub fn frame(&mut self, ctx: &egui::Context, writer: &Writer) {
        let g = writer.state_gen();
        if g != self.state_gen {
            self.state_gen = g;
            (self.sel, self.tab) = (None, None);
            self.trails.clear();
        }
        self.view = writer.pong.view();
        if let Some(paths) = &self.shot_balls {
            self.view.shots = paths.iter().filter_map(|p| p.last().map(|q| (*q, 1.0))).collect();
            self.trails = paths.iter().map(|p| p.iter().copied().collect()).collect();
        } else {
            self.track();
        }
        if self.busy() {
            ctx.request_repaint();
        }
    }

    /// Hands the selected side to the sentence field as its scope; a scope
    /// the field dropped (after a Keep, Esc) clears the selection.
    pub fn sync_scope(&mut self, sentence: &mut Sentence) {
        let field = sentence.scope();
        if field != self.sent {
            self.sel = field;
        }
        if field != self.sel {
            sentence.set_scope(self.sel);
        }
        self.sent = self.sel;
    }

    /// The header's slot after the title (R22): the "On" toggle and, while
    /// `live` is false, the Live is off chip at the left; the stats at the
    /// right of `slot`. `live_on` is true when the chip was clicked.
    pub fn header(&mut self, ui: &mut Ui, slot: Rect, writer: &Writer, t: &Text, live: bool) -> PongHead {
        let old = writer.pong.setup();
        let mut s = (*old).clone();
        let mut head = PongHead::default();
        let mut left = ui.new_child(UiBuilder::new().max_rect(slot).layout(Layout::left_to_right(Align::Center)));
        glass::toggle(&mut left, &mut s.on, t.p_on, Tok::ACCENT_HI);
        if !live {
            head.live_on = live_chip(&mut left, t).clicked();
        }
        let left_end = left.min_rect().right();
        if s != *old {
            writer.pong.set_setup(s);
        }
        self.stats(ui, slot, left_end, old.on, t);
        head
    }

    /// "0 flying · 645 hits · 91 dropped" right-aligned in `slot`, each
    /// number in a slot as wide as its digits written as '8', so it never
    /// jitters. HINT while Pong is off. Left out when it would run into
    /// the left group, which ends at `left_end`.
    fn stats(&mut self, ui: &mut Ui, slot: Rect, left_end: f32, on: bool, t: &Text) {
        let v = &self.view;
        let nums = [v.shots.len(), v.hits as usize, (v.dropped_window + v.dropped_budget) as usize].map(|n| n.to_string());
        let pieces: Vec<&str> = t.p_live.split("{}").collect();
        let piece_w = |ui: &Ui, s: &str| glass::galley(ui, s, Tok::BODY).size().x;
        let total: f32 = pieces.iter().map(|p| piece_w(ui, p)).sum::<f32>() + nums.iter().map(|n| glass::num_slot_width(ui, n, Tok::BODY).max(piece_w(ui, n))).sum::<f32>();
        let x = slot.right() - total;
        if x < left_end + Tok::GAP {
            return;
        }
        let c = white(if on { Tok::ROW } else { Tok::HINT });
        let r = Rect::from_min_max(pos2(x, slot.top()), slot.max);
        let mut row = ui.new_child(UiBuilder::new().max_rect(r).layout(Layout::left_to_right(Align::Center)));
        row.spacing_mut().item_spacing.x = 0.0;
        for (i, p) in pieces.iter().enumerate() {
            if !p.is_empty() {
                let g = glass::galley(&row, p, Tok::BODY);
                let (pr, _) = row.allocate_exact_size(g.size(), Sense::hover());
                glass::text_at(row.painter(), pr.min, g, c);
            }
            if let Some(n) = nums.get(i) {
                glass::num_slot(&mut row, n, Tok::BODY, c);
            }
        }
        self.stats_rect = Some(row.min_rect());
    }

    /// The section's body (R22, after Noa's 720 pt test): one centred
    /// block with the shooting hand row; the side view between the slider
    /// stacks; the toggles in a line under it; then the inset rule and
    /// the selected side's editor or the hint.
    pub fn body(&mut self, ui: &mut Ui, writer: &Writer, fingers: &mut Fingers, t: &Text, de: bool) {
        let avail = ui.available_width();
        let (w, view_w) = block(avail);
        let mut col = ui.available_rect_before_wrap();
        col.min.x += ((avail - w) / 2.0).max(0.0);
        col.max.x = col.min.x + w;
        ui.scope_builder(UiBuilder::new().max_rect(col).layout(Layout::top_down(Align::Min)), |ui| {
            self.hand_row(ui, writer, t, w);
            self.compact(ui, writer, t, w, view_w);
        });
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 2.0), Sense::hover());
        glass::section_rule(ui.painter(), r.x_range(), r.top());
        self.follow(ui.ctx(), fingers);
        match self.sel {
            Some(side) => self.side_editor(ui, writer, fingers, side, t, de),
            None => {
                ui.label(RichText::new(t.p_sides_hint).size(Tok::BODY).color(white(Tok::HINT)));
            }
        }
    }

    fn track(&mut self) {
        let mut old = std::mem::take(&mut self.trails);
        for (p, _) in &self.view.shots {
            let near = old
                .iter()
                .enumerate()
                .filter_map(|(i, tr)| tr.back().map(|q| (i, (q[0] - p[0]).hypot(q[1] - p[1]))))
                .filter(|(_, d)| *d < MATCH)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            let mut tr = near.map_or_else(VecDeque::new, |i| old.swap_remove(i));
            if tr.back() != Some(p) {
                tr.push_back(*p);
            }
            while tr.len() > TRAIL {
                tr.pop_front();
            }
            self.trails.push(tr);
        }
    }

    /// 1 right after side `i` was hit, 0 after FLASH.
    fn flash(&self, i: usize) -> f32 {
        self.view.since_hit.get(i).copied().flatten().map_or(0.0, |s| (1.0 - s / FLASH).clamp(0.0, 1.0))
    }

    fn busy(&self) -> bool {
        !self.view.shots.is_empty() || (0..4).any(|i| self.flash(i) > 0.0)
    }

    // --- the column's rows --------------------------------------------------

    /// "Shooting hand" and Left, Right, Both at the column's left; Test
    /// shot at its right, readable but dim while Pong or Live is off.
    fn hand_row(&mut self, ui: &mut Ui, writer: &Writer, t: &Text, w: f32) {
        let old = writer.pong.setup();
        let mut s = (*old).clone();
        let (row, _) = ui.allocate_exact_size(vec2(w, Tok::CTRL_H), Sense::hover());
        let mut left = ui.new_child(UiBuilder::new().max_rect(row).layout(Layout::left_to_right(Align::Center)));
        label(&mut left, t.p_hand);
        glass::segmented(&mut left, &mut s.hands, &[(Hands::Left, t.p_left), (Hands::Right, t.p_right), (Hands::Both, t.p_both)]);
        let mut right = ui.new_child(UiBuilder::new().max_rect(row).layout(Layout::right_to_left(Align::Center)));
        if test_button(&mut right, t.p_test, old.on && writer.params.live.value(), t.p_test_off).clicked() {
            writer.pong.test_shots(1);
        }
        self.rects.hand = row;
        if s != *old {
            writer.pong.set_setup(s);
        }
    }

    /// The side view (`view_w` wide) in the middle of `w`, Speed over
    /// Bounces at its left, Time limit over Max balls at its right, both
    /// stacks centred on its height; the toggles in centred lines under
    /// it (one line at 720 pt).
    fn compact(&mut self, ui: &mut Ui, writer: &Writer, t: &Text, w: f32, view_w: f32) {
        let old = writer.pong.setup();
        let mut s = (*old).clone();
        let (row, _) = ui.allocate_exact_size(vec2(w, view_h(view_w)), Sense::hover());
        let mid = Rect::from_min_size(pos2(row.center().x - view_w / 2.0, row.top()), vec2(view_w, row.height()));
        let mut view_ui = ui.new_child(UiBuilder::new().max_rect(mid).layout(Layout::top_down(Align::Min)));
        self.side_view(&mut view_ui, writer, t);
        let stack_h = 2.0 * Tok::CTRL_H_SM + Tok::GAP;
        let top = row.center().y - stack_h / 2.0;
        for (first, x) in [(0, row.left()), (2, row.right() - STACK_W)] {
            let r = Rect::from_min_size(pos2(x, top), vec2(STACK_W, stack_h));
            let mut stack = ui.new_child(UiBuilder::new().max_rect(r).layout(Layout::top_down(Align::Min)));
            stack.spacing_mut().item_spacing.y = Tok::GAP;
            for i in first..first + 2 {
                if let Some(at) = self.rects.sliders.get_mut(i) {
                    *at = slider(&mut stack, i, &mut s, t);
                }
            }
        }
        self.toggle_lines(ui, &mut s, t, w);
        if s != *old {
            writer.pong.set_setup(s);
        }
    }

    /// The three compact toggles in lines of at most `w`, each centred.
    fn toggle_lines(&mut self, ui: &mut Ui, s: &mut Setup, t: &Text, w: f32) {
        let toggles: [(&str, &mut bool); 3] = [(t.p_vel, &mut s.vel_from_speed), (t.p_faster, &mut s.speed_from_flick), (t.p_pause, &mut s.pause_fingers)];
        let widths: Vec<f32> = toggles.iter().map(|(text, _)| toggle_sm_width(ui, text)).collect();
        let lines = pack(&widths, w, Tok::GAP);
        let mut toggles = toggles.into_iter();
        self.rects.toggles.clear();
        self.rects.lines.clear();
        ui.scope(|ui| {
            ui.spacing_mut().interact_size.y = Tok::CTRL_H_SM;
            for (i, n) in lines.iter().enumerate() {
                let line: Vec<_> = toggles.by_ref().take(*n).collect();
                let (rects, used) = glass::centered_row(ui, Id::new(("pong-toggles", i)), w, |ui| {
                    let rects: Vec<Rect> = line.into_iter().map(|(text, on)| glass::toggle_sm(ui, on, text, Tok::ACCENT_HI).rect).collect();
                    (rects, ui.min_rect())
                });
                self.rects.toggles.extend(rects);
                self.rects.lines.push(used);
            }
        });
    }

    // --- side view ----------------------------------------------------------

    /// The frame as a dotted outline, a clickable band and a chip per side,
    /// the projectiles as small dots, hit flashes in the hand's colour.
    fn side_view(&mut self, ui: &mut Ui, writer: &Writer, t: &Text) {
        let setup = writer.pong.setup();
        let store = writer.player.store();
        let avail = ui.available_width();
        let w = avail.min(VIEW_W) - 4.0;
        let (outer, _) = ui.allocate_exact_size(vec2(avail, view_h(avail)), Sense::hover());
        let rect = Rect::from_center_size(outer.center(), vec2(w, w * 9.0 / 16.0));
        self.view_rect = Some(rect);
        let hover_at = ui.input(|i| i.pointer.hover_pos()).and_then(|p| side_at(rect, p, SIDE_BAND / 2.0));
        // the bands overlap at the corners: a click on any goes to the
        // nearest side
        let mut hit = false;
        for side in Side::ALL {
            let r = band_rect(rect, side, SIDE_BAND / 2.0);
            hit |= ui.interact(r, Id::new(("pong-side-band", side.index())), Sense::click()).clicked();
        }
        let mut clicked = if hit { hover_at } else { None };
        if hover_at.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let hand = hit_color(&setup);
        let p = ui.painter().clone();
        for side in Side::ALL {
            let (a, b) = ends(rect, side);
            let sel = self.sel == Some(side);
            let base = if sel { Tok::SEL } else { white(if hover_at == Some(side) { 0.9 } else { 0.5 }) };
            let c = mix(base, hand, self.flash(side.index()));
            dotted(&p, a, b, if sel { DOT_R_SEL } else { DOT_R }, c);
        }
        let mut balls = egui::Mesh::default();
        for (k, (pos, _)) in self.view.shots.iter().enumerate() {
            let dir = self.trails.get(k).map_or(Vec2::X, |tr| heading(tr.iter().map(|q| to_rect(rect, *q))));
            ball(&mut balls, to_rect(rect, *pos), 2.0, dir);
        }
        if !balls.is_empty() {
            p.add(egui::Shape::mesh(balls));
        }
        for side in Side::ALL {
            let text = action_name(setup.wall(side), &store, t);
            let max_w = if matches!(side, Side::Top | Side::Bottom) { rect.width() * 0.6 } else { rect.width() * 0.42 };
            let place = |size: Vec2| match side {
                Side::Top => pos2(rect.center().x, rect.top()),
                Side::Bottom => pos2(rect.center().x, rect.bottom()),
                Side::Left => pos2(rect.left() + 8.0 + size.x / 2.0, rect.center().y),
                Side::Right => pos2(rect.right() - 8.0 - size.x / 2.0, rect.center().y),
            };
            if chip_at(ui, Id::new(("pong-side-chip", side.index())), &text, self.sel == Some(side), max_w, place).clicked() {
                clicked = Some(side);
            }
        }
        if let Some(side) = clicked {
            self.select(side);
        }
    }

    /// Picks `side` (the canvas, the side view, a wall row of the fingers'
    /// total view).
    pub fn select(&mut self, side: Side) {
        if self.sel != Some(side) {
            self.tab = None;
        }
        self.sel = Some(side);
    }

    /// Three stand-in projectiles with full trails (the window captures,
    /// shot v).
    pub fn shot_balls(&mut self) {
        let path = |from: [f32; 2], v: [f32; 2], bend: f32| -> Vec<[f32; 2]> {
            (0..TRAIL).map(|i| i as f32).map(|k| [from[0] + v[0] * k + bend * k * k, from[1] + v[1] * k]).collect()
        };
        self.shot_balls = Some(vec![
            path([0.30, 0.82], [0.010, -0.018], 0.0001),
            path([0.70, 0.75], [-0.012, -0.012], -0.0001),
            path([0.52, 0.55], [0.004, -0.014], 0.0002),
        ]);
    }

    /// Picks `side` with its target search open (the window captures,
    /// shot w).
    pub fn shot_find(&mut self, side: Side) {
        self.select(side);
        self.shot_find = true;
    }

    /// Another side selected than the one whose editor was drawn last, or
    /// none: the wall editor starts afresh for it.
    fn follow(&mut self, ctx: &egui::Context, fingers: &mut Fingers) {
        if self.drawn != self.sel {
            if self.drawn.is_some() {
                fingers.wall_switched(ctx);
            }
            self.drawn = self.sel;
        }
    }

    // --- the selected side's editor -----------------------------------------

    fn side_editor(&mut self, ui: &mut Ui, writer: &Writer, fingers: &mut Fingers, side: Side, t: &Text, de: bool) {
        let action = writer.pong.setup().wall(side).clone();
        let kind = match &action {
            Action::Off => self.tab.filter(|(s, _)| *s == side).map_or(Kind::Off, |(_, k)| k),
            Action::Part { .. } => Kind::Midi,
            Action::Param { .. } | Action::Macro { .. } => Kind::Param,
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(side_name(side, t)).size(Tok::TITLE).color(white(Tok::STRONG)));
            ui.add_space(6.0);
            let mut k = kind;
            if glass::segmented(ui, &mut k, &[(Kind::Off, t.p_off), (Kind::Midi, t.p_midi), (Kind::Param, t.p_param)]).changed() {
                self.tab = Some((side, k));
                if action != Action::Off {
                    set_off(writer, side);
                }
            }
        });
        match kind {
            Kind::Off => {}
            Kind::Midi => fingers.wall_editor(ui, writer, side, t, de),
            Kind::Param => self.param_editor(ui, writer, side, &action, t),
        }
    }

    /// Target (the picked plugin's parameters, then Signal 1..32), mode,
    /// rest and peak, depth from the shot, the glide back.
    fn param_editor(&mut self, ui: &mut Ui, writer: &Writer, side: Side, action: &Action, t: &Text) {
        // the picked plugin's parameters, then the Signals, found by typing
        let pin = crate::fl::Key::parse(writer.params.target_pin.read().trim());
        let key = param_find::plugin_key(None, pin);
        let (names, note) = match param_find::names(key.as_ref(), &mut self.wanted, t) {
            Ok(n) => (n, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        let (cur, shown, how) = match action {
            Action::Macro { index, how } => (Some(Found::Macro(*index)), fill(t.p_signal, &[&(index + 1)]), Some(*how)),
            Action::Param { key, index, name, how } => (Some(Found::Param { key: key.clone(), index: *index, name: name.clone() }), param_label(name, *index), Some(*how)),
            _ => (None, t.p_target.to_string(), None),
        };
        let items = param_find::targets(key.as_ref(), &names, t);
        ui.horizontal_wrapped(|ui| {
            if std::mem::take(&mut self.shot_find) {
                param_find::open(ui, ui.make_persistent_id(Id::new(("pong-target", side.index()))));
            }
            if let Some(p) = param_find::field(ui, ("pong-target", side.index()), 200.0, &shown, cur.as_ref(), &items, note.as_deref(), t) {
                let a = match p {
                    Found::Macro(index) => Some(Action::Macro { index, how: how.unwrap_or_default() }),
                    Found::Param { key, index, name } => Some(Action::Param { key, index, name, how: how.unwrap_or(How { lo: 0.5, hi: 1.0, ..How::default() }) }),
                    Found::Move(_) => None,
                };
                if let Some(a) = a {
                    set_wall(writer, side, a);
                }
            }
            if let Some(n) = &note {
                param_find::note_line(ui, n);
            }
        });
        let Some(how) = how else { return };
        let mut h = how;
        let rest_default = if matches!(action, Action::Param { .. }) { 50 } else { 0 };
        ui.horizontal_wrapped(|ui| {
            glass::segmented(ui, &mut h.mode, &[(Mode::Ramp, t.p_ramp), (Mode::Position, t.p_position), (Mode::Toggle, t.p_toggle)]);
            let pct = |v: i32| format!("{v} %");
            let mut lo = (h.lo * 100.0).round() as i32;
            if glass::slider_field(ui, Id::new(("pong-rest", side.index())), &mut lo, 0..=100, rest_default, t.p_rest, 110.0, pct).changed() {
                h.lo = lo as f32 / 100.0;
            }
            let mut hi = (h.hi * 100.0).round() as i32;
            if glass::slider_field(ui, Id::new(("pong-peak", side.index())), &mut hi, 0..=100, 100, t.p_peak, 110.0, pct).changed() {
                h.hi = hi as f32 / 100.0;
            }
            if h.mode == Mode::Ramp {
                glass::toggle(ui, &mut h.depth_from_speed, t.p_depth, Tok::ACCENT_HI);
                let mut ms = h.back_ms.clamp(20, 2000) as i32;
                let fmt = |v: i32| fill(t.p_ms, &[&v]);
                if glass::slider_field(ui, Id::new(("pong-glide", side.index())), &mut ms, 20..=2000, 400, t.p_glide, 140.0, fmt).changed() {
                    h.back_ms = ms as u32;
                }
            }
        });
        if let Action::Macro { index, .. } = action {
            glass::chip(ui, false, &fill(t.p_link, &[&(index + 1)]), None);
        }
        if h != how {
            let a = match action.clone() {
                Action::Macro { index, .. } => Action::Macro { index, how: h },
                Action::Param { key, index, name, .. } => Action::Param { key, index, name, how: h },
                other => other,
            };
            set_wall(writer, side, a);
        }
    }

    // --- canvas -------------------------------------------------------------

    /// On the camera with Pong on (`img`: the whole frame, contain fit):
    /// the walls, the selected one in the accent, hit flashes, the
    /// projectiles with their trails. A click in a band inside an edge
    /// picks that wall; the top band starts under the top bar. True after
    /// such a click.
    pub fn canvas(&mut self, ui: &mut Ui, painter: &Painter, cam: Rect, img: Rect, setup: &Setup, input: bool) -> bool {
        let top = img.top().max(cam.top() + BAR_H);
        let (mut hover, mut clicked) = (None, None);
        if input {
            hover = ui.input(|i| i.pointer.hover_pos()).and_then(|p| band_at(img, top, p, CANVAS_BAND));
            let mut hit = false;
            for side in Side::ALL {
                let r = canvas_band(img, top, side);
                hit |= ui.interact(r, Id::new(("pong-wall-band", side.index())), Sense::click()).clicked();
            }
            if hover.is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            clicked = if hit { hover } else { None };
        }
        let hand = hit_color(setup);
        let edge = img.shrink(1.0);
        for side in Side::ALL {
            let (a, b) = ends(edge, side);
            let (w, base): (f32, Color32) = if self.sel == Some(side) {
                (2.5, Tok::SEL)
            } else {
                (1.5, white(if hover == Some(side) { 0.7 } else { 0.35 }))
            };
            let f = self.flash(side.index());
            let w = if f > 0.0 { w.max(1.5 + f) } else { w };
            painter.line_segment([a, b], Stroke::new(w, mix(base, hand, f)));
        }
        // all trails in one mesh, then the balls (dark halo, gradient) in
        // one mesh on top
        let (mut trails, mut balls) = (egui::Mesh::default(), egui::Mesh::default());
        let mut pts: Vec<Pos2> = Vec::with_capacity(TRAIL + 1);
        for (k, (pos, _)) in self.view.shots.iter().enumerate() {
            let c = to_rect(img, *pos);
            pts.clear();
            if let Some(tr) = self.trails.get(k) {
                pts.extend(tr.iter().map(|q| to_rect(img, *q)));
            }
            if pts.last() != Some(&c) {
                pts.push(c);
            }
            paint_trail(&mut trails, &trail_shape(&pts, BALL_R));
            disc(&mut balls, c, BALL_R + 1.25, |_| HALO);
            ball(&mut balls, c, BALL_R, heading(pts.iter().copied()));
        }
        for m in [trails, balls] {
            if !m.is_empty() {
                painter.add(egui::Shape::mesh(m));
            }
        }
        if let Some(side) = clicked {
            self.select(side);
        }
        clicked.is_some()
    }
}

// --- helpers ------------------------------------------------------------------

fn label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(Tok::BODY).color(white(Tok::LABEL)));
}

/// The "Live is off" chip: the glass chip with an edge in Live's colour.
fn live_chip(ui: &mut Ui, t: &Text) -> Response {
    let r = glass::chip(ui, false, t.p_live_off, None);
    ui.painter().rect_stroke(r.rect, CornerRadius::same(Tok::R_TOGGLE), Stroke::new(1.0_f32, Tok::LIVE), StrokeKind::Inside);
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

/// A quiet button that stays readable while disabled (`TEST_OFF`, not
/// `Tok::OFF`) and then explains itself on hover with `off_hover`.
fn test_button(ui: &mut Ui, text: &str, enabled: bool, off_hover: &str) -> Response {
    let g = glass::galley(ui, text, Tok::BODY);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 16.0, Tok::CTRL_H), sense);
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_TOGGLE);
    if enabled && resp.is_pointer_button_down_on() {
        p.rect_filled(rect, cr, glass::veil(Tok::PRESSED));
    } else if enabled && resp.hovered() {
        p.rect_filled(rect, cr, glass::veil(Tok::HOVER));
    }
    glass::text_at(p, rect.center() - g.size() / 2.0, g, white(if enabled { Tok::VALUE } else { TEST_OFF }));
    if enabled { resp } else { resp.on_hover_text(off_hover) }
}

/// `glass::toggle_sm`'s width for `text` (its sizing rule, scaled from
/// `CTRL_H` to `CTRL_H_SM`), so the toggles can be packed into lines
/// before they are drawn.
fn toggle_sm_width(ui: &Ui, text: &str) -> f32 {
    let k = Tok::CTRL_H_SM / Tok::CTRL_H;
    (glass::galley(ui, text, Tok::SMALL).size().x + 26.0 * k).max(Tok::TOGGLE_MIN_W * k)
}

/// How many of `widths` go on each line of at most `max`, `gap` apart;
/// at least one per line.
fn pack(widths: &[f32], max: f32, gap: f32) -> Vec<usize> {
    let mut lines = Vec::new();
    let (mut n, mut used) = (0usize, 0.0f32);
    for w in widths {
        let next = if n == 0 { *w } else { used + gap + w };
        if n > 0 && next > max {
            lines.push(n);
            (n, used) = (1, *w);
        } else {
            (n, used) = (n + 1, next);
        }
    }
    if n > 0 {
        lines.push(n);
    }
    lines
}

/// The body's block in `avail`: its width (the side view between the two
/// slider stacks) and the side view's width.
fn block(avail: f32) -> (f32, f32) {
    let stacks = 2.0 * (STACK_W + STACK_GAP);
    let view = (avail - stacks).clamp(VIEW_MIN, VIEW_W);
    (view + stacks, view)
}

/// The side view's height in a column `avail` wide: the 16:9 frame and
/// room for the top and bottom chips.
fn view_h(avail: f32) -> f32 {
    (avail.min(VIEW_W) - 4.0) * 9.0 / 16.0 + Tok::CTRL_H + 6.0
}

/// Compact slider `i` (Speed, Bounces, Time limit, Max balls), `STACK_W`
/// wide, on `s`; its rect.
fn slider(ui: &mut Ui, i: usize, s: &mut Setup, t: &Text) -> Rect {
    match i {
        0 => {
            let mut speed = (s.speed * 100.0).round() as i32;
            let fmt = |v: i32| fill(t.p_speed_unit, &[&format!("{:.2}", v as f32 / 100.0)]);
            let r = glass::slider_field_sm(ui, Id::new("pong-speed"), &mut speed, 25..=400, 100, t.p_speed, STACK_W, fmt);
            if r.changed() {
                s.speed = speed as f32 / 100.0;
            }
            r.rect
        }
        1 => {
            let mut b = s.life.bounces.map_or(BOUNCES_NEVER, |b| b.min(32) as i32);
            let fmt = |v: i32| if v >= BOUNCES_NEVER { t.p_never.to_string() } else { v.to_string() };
            let r = glass::slider_field_sm(ui, Id::new("pong-bounces"), &mut b, 1..=BOUNCES_NEVER, 3, t.p_bounces, STACK_W, fmt);
            if r.changed() {
                s.life.bounces = (b < BOUNCES_NEVER).then_some(b as u32);
            }
            r.rect
        }
        2 => {
            let mut secs = s.life.seconds.map_or(0, |x| x.round().clamp(1.0, 60.0) as i32);
            let fmt = |v: i32| if v == 0 { t.p_never.to_string() } else { fill(t.p_secs, &[&v]) };
            let r = glass::slider_field_sm(ui, Id::new("pong-time"), &mut secs, 0..=60, 0, t.p_time, STACK_W, fmt);
            if r.changed() {
                s.life.seconds = (secs > 0).then_some(secs as f32);
            }
            r.rect
        }
        _ => {
            let mut max = s.max as i32;
            let r = glass::slider_field_sm(ui, Id::new("pong-max"), &mut max, 1..=64, 16, t.p_max, STACK_W, |v| v.to_string());
            if r.changed() {
                s.max = max as u32;
            }
            r.rect
        }
    }
}

// --- the projectile -------------------------------------------------------------

/// Under each ball: black at 0.55 (premultiplied), so it reads on any camera.
const HALO: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 140);
/// Segments of a ball's disc.
const DISC_N: u32 = 16;
/// A disc's soft rim (points).
const FEATHER: f32 = 0.6;

/// A projectile's trail on screen.
#[derive(Debug, Default)]
struct TrailShape {
    /// The solid part's cross-sections from the ball back: centre, unit
    /// normal, half width, alpha.
    ribbon: Vec<(Pos2, Vec2, f32, f32)>,
    /// The dithered end: each kept pixel's top-left on the `CELL` grid
    /// and its alpha.
    dots: Vec<(Pos2, f32)>,
    /// From the ball to the end, points (the tests measure it).
    #[cfg_attr(not(test), allow(dead_code))]
    len: f32,
}

/// The trail along `pts` (oldest first, the ball last) for a ball of
/// radius `r`: it starts as wide as the ball and narrows to nothing; the
/// first `SOLID` of its length is solid and fades from 0.9 to 0.5, the
/// rest is pixels on the screen grid kept by a 4x4 ordered dither, ever
/// fewer and fainter. At most `TRAIL_LEN` long, so at most
/// `TRAIL_LEN / CELL` samples whatever the input.
fn trail_shape(pts: &[Pos2], r: f32) -> TrailShape {
    let mut samples: Vec<(Pos2, Vec2, f32)> = Vec::new();
    let (mut acc, mut next) = (0.0f32, 0.0f32);
    for (b, a) in pts.iter().rev().zip(pts.iter().rev().skip(1)) {
        let d = *b - *a;
        let l = d.length();
        if !l.is_finite() || l <= 1e-4 {
            continue;
        }
        let dir = d / l;
        while next <= acc + l && next <= TRAIL_LEN {
            samples.push((*b - dir * (next - acc), dir, next));
            next += CELL;
        }
        acc += l;
        if acc >= TRAIL_LEN {
            break;
        }
    }
    let len = acc.min(TRAIL_LEN);
    let mut shape = TrailShape { len, ..TrailShape::default() };
    if len < CELL {
        return shape;
    }
    let mut cells: Vec<(i32, i32, f32)> = Vec::new();
    for (p, dir, s) in samples {
        let u = s / len;
        let hw = r * (1.0 - u).max(0.0);
        let n = vec2(-dir.y, dir.x);
        // one sample past SOLID, so the ribbon meets the dither
        if u <= SOLID + CELL / len {
            shape.ribbon.push((p, n, hw, 0.9 - 0.4 * (u / SOLID).min(1.0)));
        }
        if u >= SOLID {
            let v = ((u - SOLID) / (1.0 - SOLID)).clamp(0.0, 1.0);
            let (density, alpha) = (0.85 * (1.0 - v), 0.6 * (1.0 - 0.5 * v));
            let m = (hw / CELL).floor() as i32;
            for k in -m..=m {
                let q = p + n * (k as f32 * CELL);
                let (ix, iy) = ((q.x / CELL).floor() as i32, (q.y / CELL).floor() as i32);
                if bayer4(ix, iy) < density {
                    cells.push((ix, iy, alpha));
                }
            }
        }
    }
    // a pixel two samples reach is drawn once, with the nearer alpha
    cells.sort_by_key(|c| (c.0, c.1));
    cells.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    shape.dots = cells.into_iter().map(|(x, y, a)| (pos2(x as f32 * CELL, y as f32 * CELL), a)).collect();
    shape
}

fn bayer4(x: i32, y: i32) -> f32 {
    let i = (y.rem_euclid(4) * 4 + x.rem_euclid(4)) as usize;
    (f32::from(BAYER4.get(i).copied().unwrap_or(0)) + 0.5) / 16.0
}

/// `s` into `mesh`: the ribbon over a dark underlay 0.75 wider, then the
/// dither pixels, all in the left hand's purple (the ball's back).
fn paint_trail(mesh: &mut egui::Mesh, s: &TrailShape) {
    if s.ribbon.len() >= 2 {
        for (grow, under) in [(0.75, true), (0.0, false)] {
            let i0 = mesh.vertices.len() as u32;
            for (p, n, hw, a) in &s.ribbon {
                let c = if under { glass::black(0.4 * a) } else { HAND[0].gamma_multiply(*a) };
                mesh.colored_vertex(*p + *n * (hw + grow), c);
                mesh.colored_vertex(*p - *n * (hw + grow), c);
            }
            for i in 0..s.ribbon.len() as u32 - 1 {
                let k = i0 + 2 * i;
                mesh.add_triangle(k, k + 1, k + 2);
                mesh.add_triangle(k + 1, k + 3, k + 2);
            }
        }
    }
    for (p, a) in &s.dots {
        mesh.add_colored_rect(Rect::from_min_size(*p, vec2(CELL, CELL)), HAND[0].gamma_multiply(*a));
    }
}

/// The unit direction from the second-to-last distinct point to the last;
/// +x without one.
fn heading(pts: impl Iterator<Item = Pos2>) -> Vec2 {
    let (mut prev, mut last) = (None, None);
    for p in pts {
        if last != Some(p) {
            (prev, last) = (last, Some(p));
        }
    }
    match (prev, last) {
        (Some(a), Some(b)) if (b - a).length() > 1e-4 => (b - a) / (b - a).length(),
        _ => Vec2::X,
    }
}

/// A ball of radius `r` at `c`: a gradient from the left hand's purple at
/// its back to the right hand's blue at its front (`dir`).
fn ball(mesh: &mut egui::Mesh, c: Pos2, r: f32, dir: Vec2) {
    disc(mesh, c, r, |u| HAND[0].lerp_to_gamma(HAND[1], (u.dot(dir) + 1.0) / 2.0));
}

/// A filled disc as a fan; `color` takes the unit offset from the centre
/// (zero at the centre). The rim fades out over `FEATHER`.
fn disc(mesh: &mut egui::Mesh, c: Pos2, r: f32, color: impl Fn(Vec2) -> Color32) {
    let i0 = mesh.vertices.len() as u32;
    mesh.colored_vertex(c, color(Vec2::ZERO));
    for k in 0..DISC_N {
        let u = Vec2::angled(k as f32 / DISC_N as f32 * std::f32::consts::TAU);
        let col = color(u);
        mesh.colored_vertex(c + u * (r - FEATHER / 2.0), col);
        mesh.colored_vertex(c + u * (r + FEATHER / 2.0), Color32::TRANSPARENT);
    }
    for k in 0..DISC_N {
        let (a, b) = (i0 + 1 + 2 * k, i0 + 1 + 2 * ((k + 1) % DISC_N));
        mesh.add_triangle(i0, a, b);
        mesh.add_triangle(a, a + 1, b);
        mesh.add_triangle(b, a + 1, b + 1);
    }
}

fn param_label(name: &str, index: u32) -> String {
    if name.trim().is_empty() { format!("#{index}") } else { name.to_string() }
}

fn mode_name(m: Mode, t: &Text) -> &'static str {
    match m {
        Mode::Ramp => t.p_ramp,
        Mode::Position => t.p_position,
        Mode::Toggle => t.p_toggle,
    }
}

fn side_name(side: Side, t: &Text) -> &'static str {
    match side {
        Side::Top => t.fw_top,
        Side::Right => t.fw_right,
        Side::Bottom => t.fw_bottom,
        Side::Left => t.fw_left,
    }
}

/// What a side's chip says: "Off", "Signal 11 · Ramp", "Cutoff ·
/// Position", "Pong top · Note".
pub(crate) fn action_name(a: &Action, store: &midi::Store, t: &Text) -> String {
    match a {
        Action::Off => t.p_off.into(),
        Action::Macro { index, how } => format!("{} · {}", fill(t.p_signal, &[&(index + 1)]), mode_name(how.mode, t)),
        Action::Param { name, index, how, .. } => format!("{} · {}", param_label(name, *index), mode_name(how.mode, t)),
        Action::Part { id } => match store.get(*id) {
            Some(p) => {
                let kind = match p.play {
                    Play::Clip { .. } => t.f_pattern,
                    Play::Chords { .. } => t.f_chord,
                    Play::Note { .. } => t.p_note,
                };
                format!("{} · {kind}", p.name)
            }
            None => t.p_part_gone.into(),
        },
    }
}

/// The colour a hit flashes in: the shooting hand's, the right one's with
/// both.
fn hit_color(s: &Setup) -> Color32 {
    match s.hands {
        Hands::Left => HAND[0],
        Hands::Right | Hands::Both => HAND[1],
    }
}

fn mix(base: Color32, hand: Color32, f: f32) -> Color32 {
    if f <= 0.0 { base } else { base.lerp_to_gamma(hand, f) }
}

/// A preview point (0..1) in `r`.
fn to_rect(r: Rect, p: [f32; 2]) -> Pos2 {
    r.min + vec2(p[0] * r.width(), p[1] * r.height())
}

/// A side's ends, clockwise, so each corner belongs to one side.
fn ends(r: Rect, side: Side) -> (Pos2, Pos2) {
    match side {
        Side::Top => (r.left_top(), r.right_top()),
        Side::Right => (r.right_top(), r.right_bottom()),
        Side::Bottom => (r.right_bottom(), r.left_bottom()),
        Side::Left => (r.left_bottom(), r.left_top()),
    }
}

/// Dots every ~DOT_STEP from `a` up to (not on) `b`.
fn dotted(p: &Painter, a: Pos2, b: Pos2, r: f32, c: Color32) {
    let n = ((b - a).length() / DOT_STEP).round().max(1.0) as usize;
    for i in 0..n {
        p.circle_filled(a + (b - a) * (i as f32 / n as f32), r, c);
    }
}

/// The side nearest `p` within `half` of `r`'s outline.
fn side_at(r: Rect, p: Pos2, half: f32) -> Option<Side> {
    if !r.expand(half).contains(p) {
        return None;
    }
    let d = [(p.y - r.top()).abs(), (p.x - r.right()).abs(), (p.y - r.bottom()).abs(), (p.x - r.left()).abs()];
    nearest(d, half)
}

/// The side whose band inside `img` holds `p`; the top band starts at `top`.
fn band_at(img: Rect, top: f32, p: Pos2, band: f32) -> Option<Side> {
    if !img.contains(p) || p.y < top {
        return None;
    }
    nearest([p.y - top, img.right() - p.x, img.bottom() - p.y, p.x - img.left()], band)
}

fn nearest(d: [f32; 4], max: f32) -> Option<Side> {
    let (i, m) = d.iter().copied().enumerate().min_by(|a, b| a.1.total_cmp(&b.1))?;
    (m <= max).then_some(Side::ALL[i])
}

/// The side view's band along `side`, `half` to each side of the line.
fn band_rect(r: Rect, side: Side, half: f32) -> Rect {
    let (a, b) = ends(r, side);
    Rect::from_two_pos(a, b).expand(half)
}

/// The canvas band inside `img` along `side`, all under `top`.
fn canvas_band(img: Rect, top: f32, side: Side) -> Rect {
    let b = CANVAS_BAND;
    let (l, r, bot) = (img.left(), img.right(), img.bottom());
    match side {
        Side::Top => Rect::from_min_max(pos2(l, top), pos2(r, top + b)),
        Side::Right => Rect::from_min_max(pos2(r - b, top), pos2(r, bot)),
        Side::Bottom => Rect::from_min_max(pos2(l, bot - b), pos2(r, bot)),
        Side::Left => Rect::from_min_max(pos2(l, top), pos2(l + b, bot)),
    }
}

/// A chip painted at `place(size)` (its centre) over the outline, in the
/// glass chip's look on a solid backing so the dots do not show through.
fn chip_at(ui: &mut Ui, id: Id, text: &str, selected: bool, max_w: f32, place: impl Fn(Vec2) -> Pos2) -> Response {
    let text = glass::fit(ui, text, Tok::BODY, (max_w - 16.0).max(20.0));
    let g = glass::galley(ui, &text, Tok::BODY);
    let size = vec2(g.size().x + 16.0, Tok::CTRL_H);
    let rect = Rect::from_center_size(place(size), size);
    let resp = ui.interact(rect, id, Sense::click());
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_TOGGLE);
    p.rect_filled(rect, cr, glass::pop_fill());
    if selected {
        glass::sel_rect(p, rect, cr);
    } else {
        if resp.hovered() {
            p.rect_filled(rect, cr, glass::veil(Tok::HOVER));
        }
        p.rect_stroke(rect, cr, Stroke::new(1.0_f32, white(Tok::FIELD_EDGE)), StrokeKind::Inside);
    }
    glass::text_at(p, rect.center() - g.size() / 2.0, g, white(if selected { Tok::STRONG } else { Tok::VALUE }));
    resp
}

fn set_wall(writer: &Writer, side: Side, a: Action) {
    let mut s = (*writer.pong.setup()).clone();
    s.walls[side.index()] = a;
    writer.pong.set_setup(s);
}

/// The side Off; a part it played leaves the store (unless another wall
/// plays it too).
fn set_off(writer: &Writer, side: Side) {
    let mut s = (*writer.pong.setup()).clone();
    let old = std::mem::take(&mut s.walls[side.index()]);
    let still = s.wall_parts();
    writer.pong.set_setup(s);
    if let Action::Part { id } = old {
        if !still.contains(&id) {
            let mut store = writer.player.store();
            store.parts.retain(|p| p.id != id);
            store.binds.retain(|b| b.part != id);
            writer.update_midi(store);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nih_plug_egui::egui::{Event, Modifiers, RawInput};

    use super::*;
    use crate::i18n::EN;

    fn writer() -> Arc<Writer> {
        Writer::new(Arc::new(crate::params::SynthexisParams::default()))
    }

    struct Kit {
        ctx: egui::Context,
        time: f64,
        out: Option<egui::FullOutput>,
        /// Some: a window this wide with the panel area's `Tok::INSET` on
        /// each side, as the editor gives the section.
        win: Option<f32>,
    }

    impl Kit {
        fn new() -> Self {
            let ctx = egui::Context::default();
            glass::style(&ctx);
            Kit { ctx, time: 0.0, out: None, win: None }
        }

        /// The real window: `w` wide, the section `w - 2 INSET`.
        fn window(w: f32) -> Self {
            Kit { win: Some(w), ..Kit::new() }
        }

        fn frame(&mut self, events: Vec<Event>, f: &mut dyn FnMut(&mut Ui)) {
            self.time += 1.0 / 60.0;
            let size = vec2(self.win.unwrap_or(900.0), 700.0);
            let raw = RawInput { events, time: Some(self.time), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), ..Default::default() };
            let inset = self.win.is_some();
            self.out = Some(self.ctx.run(raw, |ctx| {
                let panel = egui::CentralPanel::default();
                let panel = if inset { panel.frame(egui::Frame::NONE.inner_margin(egui::Margin::same(Tok::INSET as i8))) } else { panel };
                panel.show(ctx, |ui| f(ui));
            }));
        }

        fn click(&mut self, at: Pos2, f: &mut dyn FnMut(&mut Ui)) {
            let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            self.frame(vec![Event::PointerMoved(at), button(true)], f);
            self.frame(vec![button(false)], f);
            self.frame(vec![], f);
        }

        fn texts(&self) -> Vec<(String, Pos2)> {
            let mut v = Vec::new();
            for c in self.out.iter().flat_map(|o| &o.shapes) {
                walk(&c.shape, &mut v);
            }
            v
        }

        fn find(&self, needle: &str) -> Option<Pos2> {
            self.texts().into_iter().find(|(s, _)| s == needle).map(|(_, p)| p)
        }

        fn count(&self, needle: &str) -> usize {
            self.texts().iter().filter(|(s, _)| s == needle).count()
        }

        /// Every text drawn: its string, rect, colour and font size.
        fn shapes(&self) -> Vec<Drawn> {
            let mut v = Vec::new();
            for c in self.out.iter().flat_map(|o| &o.shapes) {
                walk_drawn(&c.shape, &mut v);
            }
            v
        }

        fn drawn(&self, needle: &str) -> Option<Drawn> {
            self.shapes().into_iter().find(|d| d.text == needle)
        }
    }

    #[derive(Clone, Debug)]
    struct Drawn {
        text: String,
        rect: Rect,
        color: Color32,
        size: f32,
    }

    fn walk_drawn(s: &egui::Shape, out: &mut Vec<Drawn>) {
        match s {
            egui::Shape::Text(t) => out.push(Drawn {
                text: t.galley.text().to_string(),
                rect: t.galley.rect.translate(t.pos.to_vec2()),
                color: t.override_text_color.unwrap_or(t.fallback_color),
                size: t.galley.job.sections.first().map_or(0.0, |x| x.format.font_id.size),
            }),
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk_drawn(s, out)),
            _ => {}
        }
    }

    /// The section as the new window draws it: a header row (its slot is
    /// the row right of a 60 pt title), then the body.
    fn section(p: &mut PongUi, w: &Writer, f: &mut Fingers, ui: &mut Ui, live: bool) -> PongHead {
        section_in(p, w, f, ui, live, &EN, false)
    }

    fn section_in(p: &mut PongUi, w: &Writer, f: &mut Fingers, ui: &mut Ui, live: bool, t: &Text, de: bool) -> PongHead {
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), Tok::HEAD_H), Sense::hover());
        let slot = Rect::from_min_max(pos2(row.left() + 60.0, row.top()), row.max);
        let head = p.header(ui, slot, w, t, live);
        p.body(ui, w, f, t, de);
        head
    }

    fn walk(s: &egui::Shape, out: &mut Vec<(String, Pos2)>) {
        match s {
            egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos + t.galley.rect.center().to_vec2())),
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }

    #[test]
    fn a_click_on_a_side_of_the_view_selects_that_side() {
        let w = writer();
        let mut p = PongUi::default();
        let mut k = Kit::new();
        let mut f = |ui: &mut Ui| p.side_view(ui, &w, &EN);
        k.frame(vec![], &mut f);
        assert!(p.view_rect.is_some(), "side view drawn");
        let r = p.view_rect.unwrap_or(Rect::NOTHING);
        for (at, side) in [
            (pos2(r.left() + 2.0, r.top() + r.height() * 0.2), Side::Left),
            (pos2(r.right() - 3.0, r.bottom() - r.height() * 0.2), Side::Right),
            (pos2(r.left() + r.width() * 0.2, r.bottom() + 4.0), Side::Bottom),
            (pos2(r.right() - r.width() * 0.2, r.top() - 3.0), Side::Top),
        ] {
            k.click(at, &mut |ui: &mut Ui| p.side_view(ui, &w, &EN));
            assert_eq!(p.sel, Some(side), "{at:?}");
        }
        // the middle of the frame is no side
        k.click(r.center() + vec2(0.0, 30.0), &mut |ui: &mut Ui| p.side_view(ui, &w, &EN));
        assert_eq!(p.sel, Some(Side::Top));
    }

    #[test]
    fn a_click_in_a_canvas_band_selects_that_wall_and_the_top_bar_keeps_its_own() {
        let mut p = PongUi::default();
        let setup = Setup::default();
        let cam = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 300.0));
        let img = Rect::from_center_size(cam.center(), vec2(300.0 * 16.0 / 9.0, 300.0));
        let mut k = Kit::new();
        let mut clicks = 0;
        for (at, want) in [
            (pos2(img.center().x, cam.top() + 10.0), None),
            (pos2(img.center().x, cam.top() + BAR_H + 6.0), Some(Side::Top)),
            (pos2(img.left() + 5.0, img.center().y), Some(Side::Left)),
            (pos2(img.right() - 5.0, img.center().y), Some(Side::Right)),
            (pos2(img.center().x, img.bottom() - 4.0), Some(Side::Bottom)),
            (img.center(), Some(Side::Bottom)),
            (pos2(img.left() - 20.0, img.center().y), Some(Side::Bottom)),
        ] {
            k.click(at, &mut |ui: &mut Ui| {
                let painter = ui.painter().clone();
                if p.canvas(ui, &painter, cam, img, &setup, true) {
                    clicks += 1;
                }
            });
            assert_eq!(p.sel, want, "{at:?}");
        }
        assert_eq!(clicks, 4);
    }

    #[test]
    fn the_side_chips_name_the_actions() {
        let w = writer();
        let mut store = w.player.store();
        let id = store.add("Pong bottom", Play::Chords { degrees: vec![0, 4], size: 3, octave: 4, vel: 96, strum: 0 }, None);
        w.set_midi(store);
        let mut s = Setup::default();
        s.walls[Side::Top.index()] = Action::Macro { index: 10, how: How::default() };
        s.walls[Side::Right.index()] = Action::Param { key: "fx:1:0:Portal".into(), index: 3, name: "Cutoff".into(), how: How { mode: Mode::Position, ..How::default() } };
        s.walls[Side::Bottom.index()] = Action::Part { id };
        w.pong.set_setup(s);
        let mut p = PongUi::default();
        let mut k = Kit::new();
        k.frame(vec![], &mut |ui: &mut Ui| p.side_view(ui, &w, &EN));
        let texts: Vec<String> = k.texts().into_iter().map(|(s, _)| s).collect();
        for want in ["Signal 11 · Ramp", "Cutoff · Position", "Pong bottom · Chord", "Off"] {
            assert!(texts.iter().any(|x| x == want), "{want:?} missing in {texts:?}");
        }
    }

    #[test]
    fn trails_follow_the_nearest_projectile_and_keep_trail_points() {
        let mut p = PongUi::default();
        for i in 0..30 {
            let x = 0.1 + 0.02 * i as f32;
            p.view.shots = vec![([x, 0.5], 0.8), ([0.9 - x, 0.2], 0.8)];
            p.track();
        }
        assert_eq!(p.trails.len(), 2);
        assert!(p.trails.iter().all(|t| t.len() == TRAIL));
        assert!(p.trails[0].iter().all(|q| q[1] == 0.5) && p.trails[1].iter().all(|q| q[1] == 0.2));
        p.view.shots = vec![([0.5, 0.9], 0.8)];
        p.track();
        assert_eq!(p.trails.len(), 1);
        assert_eq!(p.trails[0].len(), 1, "a new projectile starts a new trail");
    }

    /// A ball flying right along y = 100 with 24 points 5 pt apart behind it.
    fn straight() -> (Vec<Pos2>, Pos2) {
        let pts: Vec<Pos2> = (0..24).map(|i| pos2(100.0 + 5.0 * i as f32, 100.0)).collect();
        let ball = pts.last().copied().unwrap_or(Pos2::ZERO);
        (pts, ball)
    }

    #[test]
    fn a_trail_starts_as_wide_as_the_ball_narrows_and_runs_long() {
        let (pts, ball) = straight();
        let s = trail_shape(&pts, BALL_R);
        assert!((s.len - TRAIL_LEN).abs() < 0.01, "115 pt of path, capped at {TRAIL_LEN}: {}", s.len);
        let first = s.ribbon.first().copied().unwrap_or((Pos2::ZERO, Vec2::ZERO, 0.0, 0.0));
        assert_eq!(first.0, ball, "the ribbon starts at the ball");
        assert!((first.2 - BALL_R).abs() < 0.01, "as wide as the ball: half width {}", first.2);
        for (a, b) in s.ribbon.iter().zip(s.ribbon.iter().skip(1)) {
            assert!(b.2 <= a.2 && b.3 <= a.3, "narrows and fades: {a:?} then {b:?}");
            assert!(b.0.x < a.0.x, "runs back from the ball");
        }
        let end = s.ribbon.last().map_or(0.0, |r| ball.x - r.0.x);
        assert!((end - SOLID * s.len).abs() <= CELL + 0.01, "solid up to {SOLID} of the length: {end}");
    }

    #[test]
    fn a_trail_ends_in_pixels_that_thin_out_and_fade() {
        let (pts, ball) = straight();
        let s = trail_shape(&pts, BALL_R);
        assert!(!s.dots.is_empty(), "dither drawn");
        let from = SOLID * s.len;
        let mid = from + (s.len - from) / 2.0;
        let (mut near, mut far) = (Vec::new(), Vec::new());
        for (p, a) in &s.dots {
            let d = ball.x - (p.x + CELL / 2.0);
            assert!(d >= from - CELL && d <= s.len + CELL, "a pixel only in the dithered end: {d}");
            assert!((p.y + CELL / 2.0 - 100.0).abs() <= BALL_R * (1.0 - SOLID) + CELL, "no wider than the trail: {p:?}");
            assert_eq!((p.x % CELL, p.y % CELL), (0.0, 0.0), "on the screen grid: {p:?}");
            if d < mid { near.push(*a) } else { far.push(*a) }
        }
        assert!(near.len() > 2 * far.len(), "thins out: {} then {}", near.len(), far.len());
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
        assert!(!far.is_empty() && mean(&far) < mean(&near), "fades: {} then {}", mean(&near), mean(&far));
        let solid_end = s.ribbon.last().map_or(0.0, |r| r.3);
        assert!(near.iter().all(|a| *a <= solid_end + 0.15), "the dither starts about as strong as the ribbon ends: {solid_end}");
    }

    #[test]
    fn a_trail_stays_bounded_on_any_path() {
        let long: Vec<Pos2> = (0..1000).map(|i| pos2(50.0 * (i % 2) as f32, 30.0 * i as f32)).collect();
        let s = trail_shape(&long, BALL_R);
        let samples = TRAIL_LEN / CELL + 2.0;
        assert!(s.len <= TRAIL_LEN && s.ribbon.len() as f32 <= samples, "{} {}", s.len, s.ribbon.len());
        assert!(s.dots.len() as f32 <= samples * (2.0 * BALL_R / CELL + 1.0), "{}", s.dots.len());
        for pts in [vec![], vec![pos2(1.0, 1.0)], vec![pos2(1.0, 1.0); 5], vec![pos2(f32::NAN, 0.0), pos2(1.0, 1.0)], vec![pos2(0.0, 0.0), pos2(f32::INFINITY, 0.0)]] {
            let s = trail_shape(&pts, BALL_R);
            assert!(s.ribbon.is_empty() && s.dots.is_empty(), "{pts:?}: {s:?}");
        }
    }

    #[test]
    fn the_ball_runs_from_purple_at_its_back_to_blue_at_its_front() {
        let mut m = egui::Mesh::default();
        ball(&mut m, pos2(50.0, 50.0), BALL_R, Vec2::X);
        let at = |x: f32| m.vertices.iter().filter(|v| v.color != Color32::TRANSPARENT).min_by(|a, b| (a.pos.x - x).abs().total_cmp(&(b.pos.x - x).abs())).map(|v| v.color);
        assert_eq!(at(40.0), Some(HAND[0]), "back");
        assert_eq!(at(60.0), Some(HAND[1]), "front");
        assert_eq!(heading([pos2(0.0, 0.0), pos2(0.0, 2.0), pos2(0.0, 2.0)].into_iter()), Vec2::Y);
        assert_eq!(heading(std::iter::once(pos2(3.0, 3.0))), Vec2::X);
    }

    /// Another side, or none, after a side's editor: the wall editor starts
    /// afresh and the old part's lane popover closes.
    #[test]
    fn another_side_closes_the_wall_editors_popover() {
        let mut p = PongUi::default();
        let mut f = Fingers::default();
        let ctx = egui::Context::default();
        let pop = || Id::new(("lane-pop", 7u64));
        p.select(Side::Top);
        p.follow(&ctx, &mut f);
        ctx.memory_mut(|m| m.open_popup(pop()));
        p.follow(&ctx, &mut f);
        assert!(ctx.memory(|m| m.is_popup_open(pop())), "the same side keeps it");
        p.select(Side::Bottom);
        p.follow(&ctx, &mut f);
        assert!(!ctx.memory(|m| m.any_popup_open()));
        ctx.memory_mut(|m| m.open_popup(pop()));
        p.sel = None;
        p.follow(&ctx, &mut f);
        assert!(!ctx.memory(|m| m.any_popup_open()), "no side: the editor went");
    }

    #[test]
    fn there_is_no_shot_choice() {
        let w = writer();
        let (mut p, mut f) = (PongUi::default(), Fingers::default());
        let mut k = Kit::new();
        for _ in 0..3 {
            k.frame(vec![], &mut |ui: &mut Ui| {
                section(&mut p, &w, &mut f, ui, true);
            });
        }
        assert!(k.find(EN.p_hand).is_some(), "the body drew: {:?}", k.texts());
        for gone in ["Shot", "Point", "Flick"] {
            assert_eq!(k.count(gone), 0, "{gone:?} drawn");
        }
    }

    #[test]
    fn the_sliders_flank_the_side_view_and_the_toggles_sit_in_one_line_under_it() {
        let w = writer();
        let (mut p, mut f) = (PongUi::default(), Fingers::default());
        let mut k = Kit::window(720.0);
        for _ in 0..3 {
            k.frame(vec![], &mut |ui: &mut Ui| {
                section(&mut p, &w, &mut f, ui, true);
            });
        }
        let view = p.view_rect.unwrap_or(Rect::NOTHING);
        let hand = k.drawn(EN.p_hand).map_or(Rect::NOTHING, |d| d.rect);
        let test = k.drawn(EN.p_test).map_or(Rect::NOTHING, |d| d.rect);
        assert!(hand.bottom() < view.top() - Tok::CTRL_H / 2.0 && test.bottom() < view.top() - Tok::CTRL_H / 2.0, "{hand:?} {test:?} over {view:?}");
        assert!((hand.center().y - test.center().y).abs() < 1.0, "one row: {hand:?} {test:?}");
        let s = p.rects.sliders;
        for (i, left) in [(0, true), (1, true), (2, false), (3, false)] {
            let r = s[i];
            assert!(if left { r.right() + STACK_GAP <= view.left() } else { r.left() - STACK_GAP >= view.right() }, "slider {i} at {r:?} beside {view:?}");
            assert!(r.top() >= view.top() && r.bottom() <= view.bottom(), "slider {i} at {r:?} within the view's height {view:?}");
        }
        for (a, b) in [(0, 1), (2, 3)] {
            assert!(s[a].bottom() + Tok::GAP <= s[b].top() + 0.01 && (s[a].left() - s[b].left()).abs() < 0.01, "stacked: {:?} {:?}", s[a], s[b]);
            let mid = (s[a].top() + s[b].bottom()) / 2.0;
            assert!((mid - view.center().y).abs() < 0.5, "centred on the view: {mid} {:?}", view.center());
        }
        for text in [EN.p_speed, EN.p_bounces] {
            let r = k.drawn(text).map_or(Rect::NOTHING, |d| d.rect);
            assert!(r.right() < view.left(), "{text:?} at {r:?} left of {view:?}");
        }
        for text in [EN.p_time, EN.p_max] {
            let r = k.drawn(text).map_or(Rect::NOTHING, |d| d.rect);
            assert!(r.left() > view.right(), "{text:?} at {r:?} right of {view:?}");
        }
        assert_eq!(p.rects.lines.len(), 1, "one line of toggles: {:?}", p.rects.lines);
        for under in [EN.p_vel, EN.p_faster, EN.p_pause] {
            let r = k.drawn(under).map_or(Rect::NOTHING, |d| d.rect);
            assert!(r.top() > view.bottom() + Tok::CTRL_H / 2.0, "{under:?} at {r:?} under {view:?} and its chip");
        }
        let hint = k.drawn(EN.p_sides_hint).map_or(Rect::NOTHING, |d| d.rect);
        assert!(hint.top() > p.rects.lines.last().map_or(f32::INFINITY, |l| l.bottom()), "the hint comes last: {hint:?}");
    }

    /// The window at 720 pt in English and German, the widest values in
    /// the sliders: the block is centred and inside the section, the
    /// toggles take one line, and no slider's value runs into its label.
    #[test]
    fn the_body_fits_720_in_english_and_german() {
        for (t, de) in [(&EN, false), (&crate::i18n::DE, true)] {
            let w = writer();
            let mut setup = (*w.pong.setup()).clone();
            (setup.speed, setup.life.bounces, setup.life.seconds, setup.max) = (4.0, None, Some(60.0), 64);
            w.pong.set_setup(setup);
            let (mut p, mut f) = (PongUi::default(), Fingers::default());
            let mut k = Kit::window(720.0);
            let mut content = Rect::NOTHING;
            for _ in 0..3 {
                k.frame(vec![], &mut |ui: &mut Ui| {
                    content = ui.max_rect();
                    section_in(&mut p, &w, &mut f, ui, true, t, de);
                });
            }
            assert!((content.width() - (720.0 - 2.0 * Tok::INSET)).abs() < 0.01, "{content:?}");
            let r = &p.rects;
            let view = p.view_rect.unwrap_or(Rect::NOTHING);
            let block = Rect::from_min_max(pos2(r.sliders[0].left(), r.hand.top()), pos2(r.sliders[3].right(), r.hand.bottom()));
            assert!((block.width() - r.hand.width()).abs() < 0.01, "{de}: the hand row spans the block: {:?} {block:?}", r.hand);
            assert!(content.x_range().contains(block.left()) && content.x_range().contains(block.right()), "{de}: {block:?} in {content:?}");
            assert!((view.width() - (VIEW_W - 4.0)).abs() < 0.01, "{de}: the side view keeps its size at 720: {view:?}");
            for (what, x) in [("block", block.center().x), ("side view", view.center().x)].into_iter().chain(r.lines.iter().map(|l| ("toggle line", l.center().x))) {
                assert!((x - content.center().x).abs() < 0.5, "{de}: {what} centred at {x}, the section at {}", content.center().x);
            }
            assert_eq!(r.lines.len(), 1, "{de}: one line of toggles: {:?}", r.lines);
            assert_eq!(r.toggles.len(), 3);
            assert!(r.lines.iter().all(|l| l.width() <= block.width() + 0.01), "{de}: {:?}", r.lines);
            for (a, b) in r.toggles.iter().zip(r.toggles.iter().skip(1)) {
                assert!(a.right() <= b.left(), "{de}: toggles overlap: {a:?} {b:?}");
            }
            let values = [fill(t.p_speed_unit, &[&"4.00"]), t.p_never.to_string(), fill(t.p_secs, &[&60]), "64".to_string()];
            let labels = [t.p_speed, t.p_bounces, t.p_time, t.p_max];
            for (i, (value, label)) in values.iter().zip(labels).enumerate() {
                let v = k.drawn(value).map_or(Rect::NOTHING, |d| d.rect);
                let l = k.drawn(label).map_or(Rect::NOTHING, |d| d.rect);
                let field = r.sliders[i];
                assert!(v.right() + 4.0 <= l.left(), "{de}: {value:?} {v:?} runs into {label:?} {l:?}");
                assert!(field.expand(0.5).contains_rect(v) && field.expand(0.5).contains_rect(l), "{de}: {value:?} {label:?} in {field:?}");
            }
            for d in k.shapes() {
                assert!(d.rect.right() <= content.right() + 0.5 && d.rect.left() >= content.left() - 0.5, "{de}: {:?} at {:?} leaves {content:?}", d.text, d.rect);
            }
        }
    }

    #[test]
    fn compact_controls_are_20_high() {
        let w = writer();
        let (mut p, mut f) = (PongUi::default(), Fingers::default());
        let mut k = Kit::new();
        for _ in 0..3 {
            k.frame(vec![], &mut |ui: &mut Ui| {
                section(&mut p, &w, &mut f, ui, true);
            });
        }
        for r in p.rects.sliders.iter().chain(&p.rects.toggles) {
            assert!((r.height() - Tok::CTRL_H_SM).abs() < 0.01, "{r:?}");
        }
        for text in [EN.p_speed, EN.p_max, EN.p_vel, EN.p_pause] {
            let size = k.drawn(text).map_or(0.0, |d| d.size);
            assert!((size - Tok::SMALL).abs() < 0.01, "{text:?} at {size} pt");
        }
        assert!(k.find(&fill(EN.p_speed_unit, &[&"1.00"])).is_some(), "the speed reads in screens/s: {:?}", k.texts());
    }

    #[test]
    fn the_stats_sit_in_the_header_slot_and_do_not_jitter() {
        let w = writer();
        let mut p = PongUi::default();
        let mut k = Kit::new();
        let slot = Rect::from_min_size(pos2(120.0, 20.0), vec2(700.0, Tok::HEAD_H - 2.0));
        let mut at = Vec::new();
        for n in [1, 9] {
            p.view.shots = vec![([0.5, 0.5], 0.8); n];
            k.frame(vec![], &mut |ui: &mut Ui| {
                p.header(ui, slot, &w, &EN, true);
            });
            let stats = p.stats_rect.unwrap_or(Rect::NOTHING);
            assert!(slot.contains_rect(stats), "{stats:?} in {slot:?}");
            assert!((stats.right() - slot.right()).abs() < 0.5, "right-aligned: {stats:?}");
            let num = k.drawn(&n.to_string()).map_or(Rect::NOTHING, |d| d.rect);
            let word = k.shapes().into_iter().find(|d| d.text.contains("flying")).map_or(Rect::NOTHING, |d| d.rect);
            assert!(num.right() <= word.left() + 0.5, "the number before the word: {num:?} {word:?}");
            at.push((stats.width(), word.left(), num.right()));
        }
        assert_eq!(at[0], at[1], "1 flying and 9 flying take the same slot");
    }

    #[test]
    fn the_live_chip_shows_only_while_live_is_off() {
        let w = writer();
        let mut p = PongUi::default();
        let mut k = Kit::new();
        let slot = Rect::from_min_size(pos2(120.0, 20.0), vec2(700.0, Tok::HEAD_H - 2.0));
        k.frame(vec![], &mut |ui: &mut Ui| {
            p.header(ui, slot, &w, &EN, true);
        });
        assert_eq!(k.count(EN.p_live_off), 0, "Live on: no chip");
        let mut heads = Vec::new();
        let mut f = |ui: &mut Ui| heads.push(p.header(ui, slot, &w, &EN, false));
        k.frame(vec![], &mut f);
        let chip = k.find(EN.p_live_off);
        assert!(chip.is_some(), "Live off: the chip: {:?}", k.texts());
        k.click(chip.unwrap_or(Pos2::ZERO), &mut f);
        assert!(heads.iter().any(|h| h.live_on), "a click asks for Live on");
        assert!(slot.contains_rect(k.drawn(EN.p_live_off).map_or(Rect::NOTHING, |d| d.rect)), "the chip sits in the slot");
    }

    #[test]
    fn test_shot_reads_disabled_not_missing() {
        let w = writer();
        let (mut p, mut f) = (PongUi::default(), Fingers::default());
        let mut k = Kit::new();
        let mut draw = |ui: &mut Ui| {
            section(&mut p, &w, &mut f, ui, true);
        };
        for _ in 0..3 {
            k.frame(vec![], &mut draw);
        }
        assert!(!w.pong.setup().on, "Pong starts off");
        let test = k.drawn(EN.p_test);
        let a = test.as_ref().map_or(0, |d| d.color.a());
        assert!(a as f32 >= TEST_OFF * 255.0 - 1.0 && (a as f32) < Tok::VALUE * 255.0, "alpha {a}");
        let at = test.map_or(Pos2::ZERO, |d| d.rect.center());
        k.frame(vec![Event::PointerMoved(at)], &mut draw);
        for _ in 0..45 {
            k.frame(vec![], &mut draw);
        }
        assert!(k.find(EN.p_test_off).is_some(), "the hover says why: {:?}", k.texts());
    }
}
