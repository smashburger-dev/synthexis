//! Noa's glass language (design-glass, chiasma docs/STYLE.md) as egui
//! visuals and a handful of widgets that replace the stock look.
//!
//! v2 of docs/design/ui-redesign.md (sections 2.1-2.4, 2.7, 2.9).

use std::hash::Hash;
use std::ops::RangeInclusive;
use std::sync::Arc;

use nih_plug_egui::egui::{
    self, epaint::RectShape, pos2, vec2, Align, Align2, Color32, ColorImage, Context, CornerRadius, FontId, Galley, Id, Layout, Margin, Order,
    Painter, Pos2, Rangef, Rect, Response, Sense, Shadow, Stroke, StrokeKind, TextStyle, TextureHandle, TextureOptions, Ui, UiBuilder, UiKind, Vec2,
};

pub fn white(a: f32) -> Color32 {
    Color32::from_white_alpha((a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// White at `a` for fills (hover, press, fields, insets), as Figma
/// draws it: premultiplied in gamma space, the space egui_glow blends
/// in (FRAMEBUFFER_SRGB off). egui 0.31's from_white_alpha premultiplies
/// in linear space, so `white(0.05)` lifts the 30 grey window to about
/// 90 (the white flash of Noa's FL test); `veil(0.05)` lifts it to 41.
/// Text keeps `white`, which reads brighter.
pub fn veil(a: f32) -> Color32 {
    let k = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_premultiplied(k, k, k, k)
}

/// Black at `a` (the dark line of a rule).
pub fn black(a: f32) -> Color32 {
    Color32::from_black_alpha((a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Tokens: alphas are of white unless named otherwise.
pub struct Tok;

impl Tok {
    pub const WINDOW: Color32 = Color32::from_rgb(0x1e, 0x1e, 0x1e);
    pub const EDGE: f32 = 0.20;
    /// The line under the camera.
    pub const CAM_EDGE: f32 = 0.20;
    pub const FIELD: f32 = 0.05;
    pub const FIELD_EDGE: f32 = 0.13;
    /// Inset groups (lanes, grids). `INSET` is the window inset.
    pub const INSET_FILL: f32 = 0.03;
    pub const SEP_SOFT: f32 = 0.10;
    /// A rule: a white line at `RULE` over a black line at `RULE_DARK`,
    /// so it reads over dark and over lit dither cells.
    pub const RULE: f32 = 0.10;
    pub const RULE_DARK: f32 = 0.35;
    /// Hover and press only lighten a little: no white fills.
    pub const HOVER: f32 = 0.05;
    pub const PRESSED: f32 = 0.07;
    /// The one accent hue for every selected and active state.
    pub const SEL: Color32 = Color32::from_rgb(74, 144, 217);
    /// The accent's light tone: edges and caps of active things.
    pub const ACCENT_HI: Color32 = Color32::from_rgb(122, 184, 245);
    /// The accent as a solid fill: slider fills, velocity bars, signal
    /// sliders. At least 3:1 against a field or the event lane, and the
    /// value text on it still reads (46/82/120 was under 2:1).
    pub const ACCENT_FILL: Color32 = Color32::from_rgb(64, 118, 176);
    pub const STRONG: f32 = 1.0;
    pub const VALUE: f32 = 0.80;
    pub const ROW: f32 = 0.56;
    /// Labels and hints read on the dither background (spec 4.2).
    pub const LABEL: f32 = 0.56;
    pub const HINT: f32 = 0.52;
    pub const AXIS: f32 = 0.31;
    pub const OFF: f32 = 0.25;
    pub const R_WINDOW: u8 = 16;
    pub const R_POP: u8 = 10;
    pub const R_TOGGLE: u8 = 6;
    pub const R_POP_ROW: u8 = 6;
    pub const R_FIELD: u8 = 4;
    pub const R_NOTE: u8 = 2;
    pub const R_BAR: u8 = 1;
    /// Window edge to content, left and right.
    pub const INSET: f32 = 12.0;
    /// Between controls in a row.
    pub const GAP: f32 = 8.0;
    /// Between rows.
    pub const ROW_GAP: f32 = 8.0;
    /// Every button, toggle, segmented control, dropdown and field.
    pub const CTRL_H: f32 = 24.0;
    /// Pong's secondary settings only.
    pub const CTRL_H_SM: f32 = 20.0;
    /// The top row: both pickers, the chat field, "+".
    pub const TOP_H: f32 = 30.0;
    /// A section's header row.
    pub const HEAD_H: f32 = 36.0;
    /// Every popup list row (v3: 22, design-glass's popover row, so the
    /// plugin picker's columns fit over the smaller camera).
    pub const POP_ROW_H: f32 = 22.0;
    /// Under a section's last row.
    pub const SEC_BOTTOM: f32 = 16.0;
    /// Under the last row of the window.
    pub const BOTTOM_ROOM: f32 = 24.0;
    pub const TOGGLE_MIN_W: f32 = 56.0;
    /// One knob for the whole ramp: the default font (Ubuntu-Light)
    /// reads thinner than chiasma's Inter.
    pub const TYPE_SCALE: f32 = 1.0;
    /// The four sizes (spec 2.2): section titles, everything read as a
    /// control, row, hint or chat text, secondary text in or beside a
    /// control, and the window's name and the Settings title.
    pub const TITLE: f32 = 15.0;
    pub const BODY: f32 = 13.0;
    pub const SMALL: f32 = 11.0;
    pub const HEAD: f32 = 18.0;
    pub const LIVE: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x5E);
    pub const TEST: Color32 = Color32::from_rgb(0xFF, 0xB2, 0x24);
}

pub fn pop_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(0x24, 0x24, 0x26, 245)
}

pub fn sel_edge() -> Stroke {
    Stroke::new(0.6_f32, Color32::from_rgba_unmultiplied(122, 184, 245, 77))
}

pub fn sel_bar() -> Color32 {
    Color32::from_rgba_unmultiplied(74, 144, 217, 204)
}

/// The accent hue at alpha `a`.
pub fn accent(a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(74, 144, 217, (a.clamp(0.0, 1.0) * 255.0) as u8)
}

pub fn font(size: f32) -> FontId {
    FontId::proportional(size * Tok::TYPE_SCALE)
}

/// The text style of section titles (`Tok::TITLE`), set by `style`.
pub fn title_style() -> TextStyle {
    TextStyle::Name("glass-title".into())
}

pub fn galley(ui: &Ui, text: &str, size: f32) -> Arc<Galley> {
    ui.painter().layout_no_wrap(text.to_owned(), font(size), Color32::WHITE)
}

/// `text` cut with "…" to fit `max_w` at `size`.
pub fn fit(ui: &Ui, text: &str, size: f32, max_w: f32) -> String {
    if galley(ui, text, size).size().x <= max_w {
        return text.to_owned();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let t: String = chars.iter().collect::<String>() + "…";
        if galley(ui, &t, size).size().x <= max_w {
            return t;
        }
    }
    "…".into()
}

/// Paints `g` (laid out white) in `color`.
pub fn text_at(p: &Painter, pos: Pos2, g: Arc<Galley>, color: Color32) {
    p.galley_with_override_text_color(pos, g, color);
}

/// Dark glass: the tokens mapped onto egui's visuals and spacing.
pub fn style(ctx: &Context) {
    // the arrows (→ ← ↑ ↓ ⇅) are only in Hack: the last fallback of the
    // proportional text, so they don't show as empty boxes
    let mut fonts = egui::FontDefinitions::default();
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.push("Hack".into());
    }
    ctx.set_fonts(fonts);
    let mut v = egui::Visuals::dark();
    v.override_text_color = None;
    v.panel_fill = Tok::WINDOW;
    v.window_fill = pop_fill();
    v.window_stroke = Stroke::new(0.7_f32, white(Tok::EDGE));
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.window_corner_radius = CornerRadius::same(Tok::R_WINDOW);
    v.menu_corner_radius = CornerRadius::same(Tok::R_POP);
    v.striped = false;
    v.indent_has_left_vline = false;
    v.collapsing_header_frame = false;
    v.slider_trailing_fill = true;
    v.extreme_bg_color = Color32::from_rgb(0x18, 0x18, 0x18);
    // one accent: text selection, the signal sliders' fill, a pressed
    // or open control; hover only lightens a little
    v.selection.bg_fill = Tok::ACCENT_FILL;
    v.selection.stroke = Stroke::new(1.0_f32, Tok::ACCENT_HI);
    v.text_cursor.stroke = Stroke::new(1.0_f32, Tok::ACCENT_HI);
    v.text_cursor.blink = true;
    v.text_cursor.on_duration = 0.5;
    v.text_cursor.off_duration = 0.5;
    for (w, fill, edge, fg) in [
        (&mut v.widgets.noninteractive, Color32::TRANSPARENT, white(0.10), Tok::VALUE),
        (&mut v.widgets.inactive, veil(Tok::FIELD), white(Tok::FIELD_EDGE), Tok::VALUE),
        (&mut v.widgets.hovered, veil(Tok::PRESSED), white(0.18), 0.92),
        (&mut v.widgets.active, accent(0.22), accent(0.55), 0.92),
        (&mut v.widgets.open, accent(0.16), accent(0.45), 0.92),
    ] {
        w.weak_bg_fill = fill;
        w.bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0_f32, edge);
        w.fg_stroke = Stroke::new(1.0_f32, white(fg));
        w.corner_radius = CornerRadius::same(Tok::R_FIELD);
        w.expansion = 0.0;
    }
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        let sp = &mut s.spacing;
        sp.item_spacing = vec2(Tok::GAP, Tok::ROW_GAP);
        sp.button_padding = vec2(8.0, 4.0);
        sp.interact_size = vec2(40.0, Tok::CTRL_H);
        sp.icon_width = 12.0;
        sp.icon_width_inner = 6.0;
        sp.icon_spacing = 4.0;
        sp.menu_margin = Margin::same(8);
        sp.window_margin = Margin::same(12);
        sp.combo_height = 260.0;
        sp.indent = 12.0;
        let mut sc = egui::style::ScrollStyle::floating();
        sc.bar_width = 6.0;
        sc.floating_width = 2.0;
        sc.handle_min_length = 24.0;
        sc.dormant_handle_opacity = 0.0;
        sc.active_handle_opacity = 0.35;
        sc.interact_handle_opacity = 0.6;
        sc.dormant_background_opacity = 0.0;
        sc.active_background_opacity = 0.0;
        sc.interact_background_opacity = 0.08;
        sp.scroll = sc;
        // monospace stays for code-like text only
        for (ts, size, mono) in [
            (TextStyle::Heading, Tok::HEAD, false),
            (TextStyle::Body, Tok::BODY, false),
            (TextStyle::Button, Tok::BODY, false),
            (TextStyle::Small, Tok::SMALL, false),
            (TextStyle::Monospace, Tok::BODY, true),
            (title_style(), Tok::TITLE, false),
        ] {
            let f = if mono { FontId::monospace(size * Tok::TYPE_SCALE) } else { font(size) };
            s.text_styles.insert(ts, f);
        }
    });
}

/// A horizontal gradient from `base` at `a_left` to `a_right`: a 2x1
/// white-to-clear texture sampled between its texel centres. egui_glow
/// samples it in linear space, so the middle reads a few levels
/// brighter than straight alpha (accepted at these alphas).
pub fn grad_rect(painter: &Painter, rect: Rect, r: impl Into<CornerRadius>, base: Color32, a_left: f32, a_right: f32) {
    if a_left <= 0.0 || rect.width() <= 0.0 {
        return;
    }
    let ctx = painter.ctx();
    let id = Id::new("glass-ramp");
    let tex = match ctx.data_mut(|d| d.get_temp::<TextureHandle>(id)) {
        Some(t) => t,
        None => {
            let mut img = ColorImage::new([2, 1], Color32::TRANSPARENT);
            img.pixels[0] = Color32::WHITE;
            let t = ctx.load_texture("glass-ramp", img, TextureOptions::LINEAR);
            ctx.data_mut(|d| d.insert_temp(id, t.clone()));
            t
        }
    };
    let fr = (a_right / a_left).clamp(0.0, 1.0);
    let uv = Rect::from_min_max(pos2(0.25, 0.0), pos2((1.5 - fr) / 2.0, 1.0));
    painter.add(RectShape::new(rect, r, base.gamma_multiply(a_left), Stroke::NONE, StrokeKind::Inside).with_texture(tex.id(), uv));
}

/// The selection look: blue gradient and its thin edge.
pub fn sel_rect(painter: &Painter, rect: Rect, r: impl Into<CornerRadius> + Copy) {
    grad_rect(painter, rect, r, Tok::SEL, 0.15, 0.06);
    painter.rect_stroke(rect, r, sel_edge(), StrokeKind::Inside);
}

/// A soft separator across the available width.
pub fn sep_soft(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    let y = ui.painter().round_to_pixel_center(r.center().y);
    ui.painter().hline(r.x_range(), y, Stroke::new(1.0_f32, white(Tok::SEP_SOFT)));
}

/// A small chevron centred on `c`, pointing right at `turn` 0 and down
/// at 1.
pub fn chevron_at(p: &Painter, c: Pos2, half: f32, turn: f32, color: Color32) {
    let a = turn * std::f32::consts::FRAC_PI_2;
    let (s, k) = a.sin_cos();
    let rot = |x: f32, y: f32| c + vec2(x * k - y * s, x * s + y * k);
    let pts = [rot(-half / 2.0, -half), rot(half / 2.0, 0.0), rot(-half / 2.0, half)];
    let st = Stroke::new(1.0_f32, color);
    p.line_segment([pts[0], pts[1]], st);
    p.line_segment([pts[1], pts[2]], st);
}

fn seg_radius(i: usize, n: usize) -> CornerRadius {
    let r = Tok::R_TOGGLE;
    CornerRadius { nw: if i == 0 { r } else { 0 }, sw: if i == 0 { r } else { 0 }, ne: if i + 1 == n { r } else { 0 }, se: if i + 1 == n { r } else { 0 } }
}

/// Side-by-side choices, the picked one in the selection look.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, opts: &[(T, &str)]) -> Response {
    segmented_with(ui, value, opts, &[], SEG_MIN_W, SEG_PAD)
}

/// A segment's least width and the room around its text.
const SEG_MIN_W: f32 = 44.0;
const SEG_PAD: f32 = 20.0;
/// The narrow segments of the profiles 1 2 3 in the top bar.
const NUM_SEG_W: f32 = 26.0;

/// `segmented` for one-character choices (the profiles 1 2 3): 26 pt
/// segments, one hover text each.
pub fn segmented_narrow<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, opts: &[(T, &str)], tips: &[String]) -> Response {
    segmented_with(ui, value, opts, tips, NUM_SEG_W, 0.0)
}

fn segmented_with<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, opts: &[(T, &str)], tips: &[String], min_w: f32, pad: f32) -> Response {
    let gs: Vec<Arc<Galley>> = opts.iter().map(|(_, s)| galley(ui, s, Tok::BODY)).collect();
    let ws: Vec<f32> = gs.iter().map(|g| (g.size().x + pad).max(min_w)).collect();
    let (rect, mut resp) = ui.allocate_exact_size(vec2(ws.iter().sum(), Tok::CTRL_H), Sense::hover());
    let mut x = rect.left();
    for (i, ((v, _), g)) in opts.iter().zip(gs).enumerate() {
        let w = ws.get(i).copied().unwrap_or(min_w);
        let seg = Rect::from_min_size(pos2(x, rect.top()), vec2(w, Tok::CTRL_H));
        x += w;
        let mut r = ui.interact(seg, resp.id.with(i), Sense::click());
        if let Some(tip) = tips.get(i).filter(|t| !t.is_empty()) {
            r = r.on_hover_text(tip);
        }
        let sel = *value == *v;
        if r.clicked() && !sel {
            *value = *v;
            resp.mark_changed();
        }
        let cr = seg_radius(i, opts.len());
        let p = ui.painter();
        let a = if sel {
            sel_rect(p, seg, cr);
            Tok::STRONG
        } else if r.hovered() {
            p.rect_filled(seg, cr, veil(Tok::HOVER));
            Tok::VALUE
        } else {
            Tok::LABEL
        };
        text_at(p, seg.center() - g.size() / 2.0, g, white(a));
    }
    ui.painter().rect_stroke(rect, CornerRadius::same(Tok::R_TOGGLE), Stroke::new(1.0_f32, white(Tok::FIELD_EDGE)), StrokeKind::Inside);
    resp
}

/// A small choice: shapes, degrees, templates. Selected in the accent;
/// `color` (a hand's) tints the text.
pub fn chip(ui: &mut Ui, selected: bool, text: &str, color: Option<Color32>) -> Response {
    let g = galley(ui, text, Tok::BODY);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 16.0, Tok::CTRL_H), Sense::click());
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_TOGGLE);
    let tc = if selected {
        sel_rect(p, rect, cr);
        color.map_or(white(Tok::STRONG), |c| c.lerp_to_gamma(Color32::WHITE, 0.45))
    } else {
        if resp.is_pointer_button_down_on() {
            p.rect_filled(rect, cr, veil(Tok::PRESSED));
        } else if resp.hovered() {
            p.rect_filled(rect, cr, veil(Tok::HOVER));
        }
        p.rect_stroke(rect, cr, Stroke::new(1.0_f32, white(Tok::FIELD_EDGE)), StrokeKind::Inside);
        color.unwrap_or(white(Tok::VALUE))
    };
    text_at(p, rect.center() - g.size() / 2.0, g, tc);
    resp
}

fn toggle_h(ui: &mut Ui, on: &mut bool, text: &str, color: Color32, h: f32, size: f32) -> Response {
    let g = galley(ui, text, size);
    let k = h / Tok::CTRL_H;
    let w = (g.size().x + 26.0 * k).max(Tok::TOGGLE_MIN_W * k);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_TOGGLE);
    // a short label stretched to the minimum width: dot and text centre as
    // one group, so the spare width splits evenly on both sides
    let x0 = rect.left() + (w - (g.size().x + 26.0 * k)) / 2.0;
    let dot = pos2(x0 + 10.0 * k, rect.center().y);
    let tc = if *on {
        sel_rect(p, rect, cr);
        p.circle_filled(dot, 3.0 * k, color);
        white(Tok::STRONG)
    } else {
        if resp.hovered() {
            p.rect_filled(rect, cr, veil(Tok::HOVER));
        }
        p.rect_stroke(rect, cr, Stroke::new(1.0_f32, white(Tok::FIELD_EDGE)), StrokeKind::Inside);
        p.circle_stroke(dot, 3.0 * k, Stroke::new(1.0_f32, white(Tok::HINT)));
        white(Tok::VALUE)
    };
    text_at(p, pos2(x0 + 18.0 * k, rect.center().y - g.size().y / 2.0), g, tc);
    resp
}

/// A switch (Live, Test signal): outline when off, the accent when on;
/// its own colour only in the dot.
pub fn toggle(ui: &mut Ui, on: &mut bool, text: &str, color: Color32) -> Response {
    toggle_h(ui, on, text, color, Tok::CTRL_H, Tok::BODY)
}

/// The compact toggle of Pong's secondary settings: `CTRL_H_SM` high,
/// 11 pt text.
pub fn toggle_sm(ui: &mut Ui, on: &mut bool, text: &str, color: Color32) -> Response {
    toggle_h(ui, on, text, color, Tok::CTRL_H_SM, Tok::SMALL)
}

fn quiet_a(ui: &mut Ui, text: &str, a: f32) -> Response {
    let g = galley(ui, text, Tok::BODY);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 16.0, Tok::CTRL_H), Sense::click());
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_TOGGLE);
    if resp.is_pointer_button_down_on() {
        p.rect_filled(rect, cr, veil(Tok::PRESSED));
    } else if resp.hovered() {
        p.rect_filled(rect, cr, veil(Tok::HOVER));
    }
    let a = if ui.is_enabled() { a } else { Tok::OFF };
    text_at(p, rect.center() - g.size() / 2.0, g, white(a));
    resp
}

/// A button without a frame: Calibrate, Reset, + shape, Settings, ×.
pub fn quiet(ui: &mut Ui, text: &str) -> Response {
    quiet_a(ui, text, Tok::VALUE)
}

/// A quieter one, for Remove.
pub fn quiet_dim(ui: &mut Ui, text: &str) -> Response {
    quiet_a(ui, text, Tok::LABEL)
}

/// A quiet button whose text starts at the left edge of its slot, so it
/// lines up with the edge of the column it leads (Save as under the
/// preset picker). Hover and press veils reach 4 pt to each side of the
/// text; the slot itself starts at the text.
pub fn quiet_lead(ui: &mut Ui, text: &str) -> Response {
    let g = galley(ui, text, Tok::BODY);
    let (slot, _) = ui.allocate_exact_size(vec2(g.size().x + 4.0, Tok::CTRL_H), Sense::hover());
    let rect = Rect::from_min_max(pos2(slot.left() - 4.0, slot.top()), slot.max);
    let resp = ui.interact(rect, ui.id().with(("quiet-lead", text)), Sense::click());
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_TOGGLE);
    if resp.is_pointer_button_down_on() {
        p.rect_filled(rect, cr, veil(Tok::PRESSED));
    } else if resp.hovered() {
        p.rect_filled(rect, cr, veil(Tok::HOVER));
    }
    text_at(p, pos2(slot.left(), slot.center().y - g.size().y / 2.0), g, white(Tok::VALUE));
    resp
}

/// A control that cannot act now: dim, still readable.
pub const DIM_OFF: f32 = 0.40;

/// A quiet button that stays readable while it cannot act (alpha
/// `DIM_OFF`) and explains itself on hover. The disabled look of this
/// style paints nothing (its fade colour is transparent), so this one
/// never disables its Ui.
pub fn dim_button(ui: &mut Ui, text: &str, enabled: bool, off_hover: &str) -> Response {
    let g = galley(ui, text, Tok::BODY);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 16.0, Tok::CTRL_H), sense);
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_TOGGLE);
    if enabled && resp.is_pointer_button_down_on() {
        p.rect_filled(rect, cr, veil(Tok::PRESSED));
    } else if enabled && resp.hovered() {
        p.rect_filled(rect, cr, veil(Tok::HOVER));
    }
    text_at(p, rect.center() - g.size() / 2.0, g, white(if enabled { Tok::VALUE } else { DIM_OFF }));
    if enabled { resp } else { resp.on_hover_text(off_hover) }
}

/// A `dropdown` that cannot open now: the closed field, `w` by
/// `CTRL_H`, with `shown` at `DIM_OFF`, no chevron, `hover` on hover.
pub fn dim_dropdown(ui: &mut Ui, w: f32, shown: &str, hover: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, Tok::CTRL_H), Sense::hover());
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_FIELD);
    p.rect_filled(rect, cr, veil(Tok::FIELD));
    p.rect_stroke(rect, cr, Stroke::new(1.0_f32, white(Tok::FIELD_EDGE)), StrokeKind::Inside);
    let pad = ui.spacing().button_padding.x;
    let g = galley(ui, &fit(ui, shown, Tok::BODY, w - 34.0), Tok::BODY);
    text_at(p, pos2(rect.left() + pad, rect.center().y - g.size().y / 2.0), g, white(DIM_OFF));
    resp.on_hover_text(hover)
}

/// One row of a dropdown or popover list.
pub fn pop_row(ui: &mut Ui, selected: bool, text: &str) -> Response {
    let g = galley(ui, text, Tok::BODY);
    let w = ui.available_width().max(g.size().x + 16.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, Tok::POP_ROW_H), Sense::click());
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_POP_ROW);
    let a = if selected {
        grad_rect(p, rect, cr, Tok::SEL, 0.15, 0.06);
        Tok::STRONG
    } else {
        if resp.hovered() {
            p.rect_filled(rect, cr, veil(Tok::HOVER));
        }
        Tok::VALUE
    };
    text_at(p, pos2(rect.left() + 8.0, rect.center().y - g.size().y / 2.0), g, white(a));
    resp
}

/// A field with a chevron that opens a list of `opts`; the picked one.
/// The list is a frosted popup (`popup`) as wide as the field or its
/// longest option, below the field or above it when there is more room
/// there, and it scrolls past the room it gets.
pub fn dropdown<T: PartialEq + Clone>(ui: &mut Ui, id: impl Hash, w: f32, shown: &str, value: &T, opts: impl IntoIterator<Item = (T, String)>) -> Option<T> {
    let id = ui.make_persistent_id(Id::new(id));
    let (rect, _) = ui.allocate_exact_size(vec2(w, Tok::CTRL_H), Sense::hover());
    let resp = ui.interact(rect, id, Sense::click());
    let open = ui.memory(|m| m.is_popup_open(id.with("popup")));
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_FIELD);
    let fill = if resp.is_pointer_button_down_on() || open {
        Tok::PRESSED
    } else if resp.hovered() {
        Tok::HOVER + 0.01
    } else {
        Tok::FIELD
    };
    p.rect_filled(rect, cr, veil(fill));
    p.rect_stroke(rect, cr, Stroke::new(1.0_f32, white(Tok::FIELD_EDGE)), StrokeKind::Inside);
    let pad = ui.spacing().button_padding.x;
    let g = galley(ui, &fit(ui, shown, Tok::BODY, w - 34.0), Tok::BODY);
    text_at(p, pos2(rect.left() + pad, rect.center().y - g.size().y / 2.0), g, white(Tok::VALUE));
    let c = pos2(rect.right() - pad - 4.0, rect.center().y);
    let (dx, dy) = (4.0, if open { -2.0 } else { 2.0 });
    let st = Stroke::new(1.0_f32, white(Tok::HINT));
    p.line_segment([c + vec2(-dx, -dy), c + vec2(0.0, dy)], st);
    p.line_segment([c + vec2(0.0, dy), c + vec2(dx, -dy)], st);
    let opts: Vec<(T, String)> = opts.into_iter().collect();
    let widest = opts.iter().map(|(_, s)| galley(ui, s, Tok::BODY).size().x + 2.0 * PICK_PAD + 8.0).fold(0.0_f32, f32::max);
    let pop_w = w.max(widest + 2.0 * f32::from(POP_PAD));
    let want_h = opts.len() as f32 * Tok::POP_ROW_H + 2.0 * f32::from(POP_PAD);
    let picked = popup(ui, id, &resp, Open::Auto, pop_w, want_h, f32::INFINITY, |ui, max_h| {
        egui::ScrollArea::vertical()
            .max_height(max_h)
            // the Area hands its content last frame's size; without this
            // the list never grows past it
            .min_scrolled_height(max_h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                    let mut picked = None;
                    for (v, s) in &opts {
                        if pop_row(ui, v == value, s).clicked() {
                            picked = Some(v.clone());
                        }
                    }
                    picked
                })
                .inner
            })
            .inner
    })
    .flatten();
    if picked.is_some() {
        ui.memory_mut(|m| m.close_popup());
    }
    picked
}

#[allow(clippy::too_many_arguments)]
fn slider_field_h(ui: &mut Ui, id: Id, v: &mut i32, range: RangeInclusive<i32>, default: i32, label: &str, w: f32, h: f32, size: f32, fmt: impl Fn(i32) -> String) -> Response {
    let (lo, hi) = (*range.start(), (*range.end()).max(*range.start()));
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let mut resp = ui.interact(rect, id, Sense::click_and_drag());
    let span = (hi - lo).max(1) as f32;
    if resp.drag_started() {
        ui.data_mut(|d| d.insert_temp(id, *v));
    }
    if resp.dragged() {
        let start = ui.data(|d| d.get_temp::<i32>(id)).unwrap_or(*v);
        let dx = ui.input(|i| match (i.pointer.interact_pos(), i.pointer.press_origin()) {
            (Some(p), Some(o)) => p.x - o.x,
            _ => 0.0,
        });
        let nv = (start as f32 + dx * span / 200.0).round().clamp(lo as f32, hi as f32) as i32;
        if nv != *v {
            *v = nv;
            resp.mark_changed();
        }
    }
    if resp.double_clicked() && *v != default {
        *v = default.clamp(lo, hi);
        resp.mark_changed();
    }
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_FIELD);
    p.rect_filled(rect, cr, veil(if resp.hovered() || resp.dragged() { Tok::PRESSED } else { Tok::FIELD }));
    let frac = (*v - lo) as f32 / span;
    let fill_w = rect.width() * frac.clamp(0.0, 1.0);
    if fill_w > 0.5 {
        // the value in the accent, a shade lighter while it is dragged
        let fr = Rect::from_min_size(rect.min, vec2(fill_w, rect.height()));
        let c = if resp.dragged() { Tok::ACCENT_FILL.lerp_to_gamma(Tok::SEL, 0.3) } else { Tok::ACCENT_FILL };
        p.rect_filled(fr, CornerRadius::same((fill_w / 2.0).min(4.0) as u8), c);
    }
    p.rect_stroke(rect, cr, Stroke::new(1.0_f32, white(Tok::FIELD_EDGE)), StrokeKind::Inside);
    let g = galley(ui, &fmt(*v), size);
    text_at(p, pos2(rect.left() + 6.0, rect.center().y - g.size().y / 2.0), g, white(if resp.dragged() { Tok::STRONG } else { Tok::VALUE }));
    if !label.is_empty() {
        p.text(pos2(rect.right() - 6.0, rect.center().y), Align2::RIGHT_CENTER, label, font(Tok::SMALL), white(Tok::LABEL));
    }
    resp
}

/// A number field: the field fills up to the value, a horizontal drag
/// sets it (span / 200 per point), a double-click resets it.
#[allow(clippy::too_many_arguments)]
pub fn slider_field(ui: &mut Ui, id: Id, v: &mut i32, range: RangeInclusive<i32>, default: i32, label: &str, w: f32, fmt: impl Fn(i32) -> String) -> Response {
    slider_field_h(ui, id, v, range, default, label, w, Tok::CTRL_H, Tok::BODY, fmt)
}

/// The compact `slider_field` of Pong's secondary settings: `CTRL_H_SM`
/// high, value and label at 11 pt.
#[allow(clippy::too_many_arguments)]
pub fn slider_field_sm(ui: &mut Ui, id: Id, v: &mut i32, range: RangeInclusive<i32>, default: i32, label: &str, w: f32, fmt: impl Fn(i32) -> String) -> Response {
    slider_field_h(ui, id, v, range, default, label, w, Tok::CTRL_H_SM, Tok::SMALL, fmt)
}

/// A thin progress track (the calibration).
pub fn progress(ui: &mut Ui, frac: f32, w: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(w, 4.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same(2), white(0.10));
    let f = Rect::from_min_size(r.min, vec2(r.width() * frac.clamp(0.0, 1.0), r.height()));
    p.rect_filled(f, CornerRadius::same(2), Tok::SEL);
}

// ---- v2 widgets (spec 2.4, 2.7, 2.9) ----

/// Text left inset of picker buttons and popup rows.
const PICK_PAD: f32 = 8.0;
/// The picker triangle's centre from the button's right edge.
const PICK_ARROW: f32 = 12.0;
/// A popup is at least this wide.
pub const POP_MIN_W: f32 = 200.0;
/// Padding inside a popup.
const POP_PAD: i8 = 8;
/// `POP_PAD` in points, with the edge: what a popup's frame takes on each side.
pub const POP_PAD_PT: f32 = POP_PAD as f32 + POP_EDGE;
/// A popup's edge width (the frame counts it inside its size).
const POP_EDGE: f32 = 0.7;
/// A section's close arm lasts this long (s).
const ARM_S: f64 = 4.0;
/// An info toast stays this long (s), then fades over `TOAST_FADE`.
const TOAST_SHOW: f32 = 4.0;
const TOAST_FADE: f32 = 0.3;
/// The toast's distance from the window's left and bottom edges.
const TOAST_MARGIN: f32 = 12.0;

/// The one picker style (plugin picker, preset picker, part picker): the
/// field fill, a 1 px edge, radius 6, 13 pt text at VALUE (HINT for a
/// placeholder) cut with "…", and a filled triangle 12 pt from the right
/// edge. Opens nothing by itself: pass the response to `picker_popup`.
pub fn picker_button(ui: &mut Ui, id: Id, size: Vec2, text: &str, placeholder: bool, hover: &str) -> Response {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let resp = ui.interact(rect, id, Sense::click());
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_TOGGLE);
    let open = ui.memory(|m| m.is_popup_open(id.with("popup")));
    let fill = if resp.is_pointer_button_down_on() || open {
        Tok::PRESSED
    } else if resp.hovered() {
        Tok::HOVER + 0.01
    } else {
        Tok::FIELD
    };
    p.rect_filled(rect, cr, veil(fill));
    p.rect_stroke(rect, cr, Stroke::new(1.0_f32, white(Tok::FIELD_EDGE)), StrokeKind::Inside);
    let max_w = (rect.width() - PICK_PAD - PICK_ARROW - 8.0).max(0.0);
    let g = galley(ui, &fit(ui, text, Tok::BODY, max_w), Tok::BODY);
    let a = if placeholder { Tok::HINT } else { Tok::VALUE };
    text_at(p, pos2(rect.left() + PICK_PAD, rect.center().y - g.size().y / 2.0), g, white(a));
    let c = pos2(rect.right() - PICK_ARROW, rect.center().y);
    p.add(egui::Shape::convex_polygon(vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)], white(Tok::VALUE), Stroke::NONE));
    if hover.is_empty() { resp } else { resp.on_hover_text(hover) }
}

/// Where a popup opens from its anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Open {
    /// Above the anchor (the top section's pickers, over the camera).
    Up,
    /// Below when the popup fits there or there is more room below, else above.
    Auto,
}

/// A popup keeps this far from the window's edges.
pub const POP_MARGIN: f32 = 4.0;
/// Between a popup and its anchor.
const POP_GAP: f32 = 2.0;

/// Where a popup sits: its anchor point, the pivot there, its width and
/// the room it has on that side (the most height it may take).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub pos: Pos2,
    pub pivot: Align2,
    pub w: f32,
    pub max_h: f32,
}

impl Placed {
    /// The popup's rect at height `h` (at most `max_h`).
    #[cfg(test)]
    pub fn rect(&self, h: f32) -> Rect {
        let size = vec2(self.w, h.min(self.max_h).max(0.0));
        self.pivot.anchor_size(self.pos, size)
    }
}

/// Places a popup `w` wide whose content is `want_h` high next to
/// `anchor` inside `win` (pure, spec v3 2.7): never past `POP_MARGIN` from
/// the window's edges. Left-aligned to the anchor, shifted left when it
/// would pass the right edge; never wider than the window. `Up` opens
/// above unless less than two rows fit there and below has more room;
/// `Auto` below when `want_h` fits there or
/// below has more room. The caller scrolls what `max_h` cuts off.
pub fn place(anchor: Rect, w: f32, want_h: f32, win: Rect, open: Open) -> Placed {
    let w = w.min(win.width() - 2.0 * POP_MARGIN).max(0.0);
    let x_max = (win.right() - POP_MARGIN - w).max(win.left() + POP_MARGIN);
    let x = if anchor.left() + w > win.right() - POP_MARGIN { anchor.right() - w } else { anchor.left() };
    let x = x.clamp(win.left() + POP_MARGIN, x_max);
    let up_room = (anchor.top() - POP_GAP - (win.top() + POP_MARGIN)).max(0.0);
    let down_room = (win.bottom() - POP_MARGIN - (anchor.bottom() + POP_GAP)).max(0.0);
    let small = (2.0 * Tok::POP_ROW_H + 2.0 * f32::from(POP_PAD)).min(want_h);
    let up = match open {
        Open::Up => up_room >= small || up_room >= down_room,
        Open::Auto => !(down_room >= want_h || down_room >= up_room),
    };
    // an anchor scrolled out of the window still opens inside it
    let (y, room) = if up {
        ((anchor.top() - POP_GAP).clamp(win.top() + POP_MARGIN, win.bottom() - POP_MARGIN), up_room)
    } else {
        ((anchor.bottom() + POP_GAP).clamp(win.top() + POP_MARGIN, win.bottom() - POP_MARGIN), down_room)
    };
    let room = room.min(win.height() - 2.0 * POP_MARGIN).max(0.0);
    let pivot = if up { Align2::LEFT_BOTTOM } else { Align2::LEFT_TOP };
    Placed { pos: pos2(x, y), pivot, w, max_h: room }
}

/// The window rect popups stay inside: the one the editor stored this
/// frame (`set_frost`), else the screen.
pub fn popup_window(ctx: &Context) -> Rect {
    ctx.data(|d| d.get_temp::<FrostSrc>(frost_id())).map_or_else(|| ctx.screen_rect(), |f| f.win)
}

/// A frosted popup (spec v3 2.7) for `button`: opens and closes on a click
/// on it, closes on Esc or a click outside both. `w` wide (cut to the
/// window), placed by `place` for content `want_h` high, at most `cap_h`
/// high; `add` gets the inner height it may use and scrolls beyond it
/// itself. Its id is `id.with("popup")`. The caller closes it after a
/// pick (`ui.memory_mut(|m| m.close_popup())`).
#[allow(clippy::too_many_arguments)]
pub fn popup<R>(ui: &Ui, id: Id, button: &Response, open: Open, w: f32, want_h: f32, cap_h: f32, add: impl FnOnce(&mut Ui, f32) -> R) -> Option<R> {
    let pid = id.with("popup");
    if button.clicked() {
        ui.memory_mut(|m| m.toggle_popup(pid));
    }
    popup_shown(ui, pid, button.rect, button, open, w, want_h, cap_h, add)
}

/// The popup `pid` when it is open (opened elsewhere): placed at `anchor`
/// like `popup`, closed on Esc or a click outside it and outside `owner`.
#[allow(clippy::too_many_arguments)]
pub fn popup_shown<R>(ui: &Ui, pid: Id, anchor: Rect, owner: &Response, open: Open, w: f32, want_h: f32, cap_h: f32, add: impl FnOnce(&mut Ui, f32) -> R) -> Option<R> {
    if !ui.memory(|m| m.is_popup_open(pid)) {
        return None;
    }
    let mut b = anchor;
    if let Some(t) = ui.ctx().layer_transform_to_global(ui.layer_id()) {
        b = t * b;
    }
    // padding and the edge on each side
    let pad = f32::from(POP_PAD) + POP_EDGE;
    let at = place(b, w, want_h, popup_window(ui.ctx()), open);
    let inner_w = (at.w - 2.0 * pad).max(0.0);
    let inner_h = (at.max_h.min(cap_h) - 2.0 * pad).max(0.0);
    let area = egui::Area::new(pid)
        .kind(UiKind::Popup)
        .order(Order::Foreground)
        .fixed_pos(at.pos)
        .pivot(at.pivot)
        .constrain(false)
        .show(ui.ctx(), |ui| {
            frosted(ui, Margin::same(POP_PAD), |ui| {
                ui.set_width(inner_w);
                ui.spacing_mut().item_spacing.y = 0.0;
                add(ui, inner_h)
            })
            .inner
        });
    let outside = owner.clicked_elsewhere() && area.response.clicked_elsewhere();
    if outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        ui.memory_mut(|m| m.close_popup());
    }
    Some(area.inner)
}

/// A frosted popup list that scrolls (`popup_shown` with a vertical
/// scroll area), `w` wide; its height last frame decides the side.
pub fn popup_list<R>(ui: &Ui, pid: Id, anchor: Rect, owner: &Response, w: f32, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let want = ui.ctx().memory(|m| m.area_rect(pid)).map_or(300.0, |r| r.height());
    popup_shown(ui, pid, anchor, owner, Open::Auto, w, want, f32::INFINITY, |ui, inner_h| {
        egui::ScrollArea::vertical()
            .max_height(inner_h)
            .min_scrolled_height(inner_h)
            .auto_shrink([false, true])
            .show(ui, |ui| ui.with_layout(Layout::top_down_justified(Align::LEFT), add).inner)
            .inner
    })
}

/// The popup of a picker (`popup`, `Open::Auto`): as wide as the button
/// (at least 200); the list grows to `max_h` and scrolls beyond.
pub fn picker_popup<R>(ui: &Ui, id: Id, button: &Response, max_h: f32, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    picker_popup_to(ui, id, button, Open::Auto, max_h, add)
}

/// `picker_popup` opening above the button (the top section's preset
/// picker and "+", over the camera).
pub fn picker_popup_up<R>(ui: &Ui, id: Id, button: &Response, max_h: f32, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    picker_popup_to(ui, id, button, Open::Up, max_h, add)
}

fn picker_popup_to<R>(ui: &Ui, id: Id, button: &Response, open: Open, max_h: f32, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let w = button.rect.width().max(POP_MIN_W);
    // which side has room is decided by the list's height last frame
    let want = ui.ctx().memory(|m| m.area_rect(id.with("popup"))).map_or(max_h, |r| r.height()).min(max_h);
    popup(ui, id, button, open, w, want, max_h, |ui, inner_h| {
        egui::ScrollArea::vertical()
            .max_height(inner_h)
            .min_scrolled_height(inner_h)
            .auto_shrink([false, true])
            .show(ui, |ui| ui.with_layout(Layout::top_down_justified(Align::LEFT), add).inner)
            .inner
    })
}

// ---- frost (spec v3 2.7) ----

/// The dark tint over the blurred camera behind a popup. 0.65 keeps
/// VALUE text at 4.5:1 or better over a blurred white wall.
pub const FROST_CAM_TINT: f32 = 0.65;
/// The tint over the dither background: strong, so text reads, with a
/// little of the dither still showing (glass). 0.84 let labels under a
/// popup show through (Noa, 11-04); 0.94 hides them.
pub const FROST_TINT: f32 = 0.94;

/// What a popup's frost needs from the window, stored each frame by the
/// editor (`set_frost`): the window, the camera rect, where the camera
/// image is drawn with which uv, and the blurred camera texture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrostSrc {
    pub win: Rect,
    pub cam: Rect,
    pub img: Rect,
    pub uv: Rect,
    pub blur: Option<egui::TextureId>,
}

fn frost_id() -> Id {
    Id::new("glass-frost")
}

/// Stores this frame's frost source for every popup.
pub fn set_frost(ctx: &Context, src: FrostSrc) {
    ctx.data_mut(|d| d.insert_temp(frost_id(), src));
}

/// The frost tint colour at `a`: the window grey, a shade darker.
fn frost_tint(a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(18, 18, 20, (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// `r` on the corners of `inner` that are corners of `outer`, square
/// elsewhere (the camera part of a popup that also hangs over the dither).
fn shared_corners(inner: Rect, outer: Rect, r: u8) -> CornerRadius {
    let same = |a: f32, b: f32| (a - b).abs() < 0.01;
    let (l, rt, t, b) = (same(inner.left(), outer.left()), same(inner.right(), outer.right()), same(inner.top(), outer.top()), same(inner.bottom(), outer.bottom()));
    let k = |on: bool| if on { r } else { 0 };
    CornerRadius { nw: k(l && t), ne: k(rt && t), sw: k(l && b), se: k(rt && b) }
}

/// The frost behind a popup at `rect` with corner radius `r` (pure): the
/// strong tint over the whole rect; where the rect lies on the camera
/// image, the blurred camera at the same place (its uv mapped from the
/// image's) under the dark camera tint.
pub fn frost_shapes(src: Option<&FrostSrc>, rect: Rect, r: u8) -> Vec<egui::Shape> {
    let mut out = vec![egui::Shape::Rect(RectShape::filled(rect, CornerRadius::same(r), frost_tint(FROST_TINT)))];
    let Some(f) = src else { return out };
    let Some(tex) = f.blur else { return out };
    let on = rect.intersect(f.cam).intersect(f.img);
    if !on.is_positive() || !f.img.is_positive() {
        return out;
    }
    let u = |x: f32| f.uv.left() + (x - f.img.left()) / f.img.width() * f.uv.width();
    let v = |y: f32| f.uv.top() + (y - f.img.top()) / f.img.height() * f.uv.height();
    let uv = Rect::from_min_max(pos2(u(on.left()), v(on.top())), pos2(u(on.right()), v(on.bottom())));
    let cr = shared_corners(on, rect, r);
    out.push(egui::Shape::Rect(RectShape::filled(on, cr, Color32::WHITE).with_texture(tex, uv)));
    out.push(egui::Shape::Rect(RectShape::filled(on, cr, frost_tint(FROST_CAM_TINT))));
    out
}

/// A popup's frame with frost behind it: no fill of its own, the 0.7 pt
/// edge, radius `R_POP`, `margin` inside. The frost goes into a slot taken
/// before the content, so it lies under the frame's edge and the text.
pub fn frosted<R>(ui: &mut Ui, margin: Margin, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let slot = ui.painter().add(egui::Shape::Noop);
    let out = egui::Frame::NONE
        .stroke(Stroke::new(POP_EDGE, white(Tok::EDGE)))
        .corner_radius(CornerRadius::same(Tok::R_POP))
        .inner_margin(margin)
        .show(ui, add);
    let src = ui.ctx().data(|d| d.get_temp::<FrostSrc>(frost_id()));
    ui.painter().set(slot, egui::Shape::Vec(frost_shapes(src.as_ref(), out.response.rect, Tok::R_POP)));
    let pass = ui.ctx().cumulative_pass_nr();
    let rect = out.response.rect;
    ui.ctx().data_mut(|d| d.get_temp_mut_or_default::<Cover>(cover_id()).add(pass, rect));
    out
}

/// The rects the frosted popups took, this pass and the one before.
#[derive(Clone, Copy, Debug)]
struct Cover {
    pass: u64,
    now: Rect,
    before: Rect,
}

impl Default for Cover {
    fn default() -> Self {
        Self { pass: 0, now: Rect::NOTHING, before: Rect::NOTHING }
    }
}

impl Cover {
    fn add(&mut self, pass: u64, r: Rect) {
        if self.pass != pass {
            self.before = if self.pass + 1 == pass { self.now } else { Rect::NOTHING };
            self.now = Rect::NOTHING;
            self.pass = pass;
        }
        self.now = self.now.union(r);
    }

    /// What the popups covered in the pass before `pass`.
    fn last(&self, pass: u64) -> Rect {
        match pass.checked_sub(self.pass) {
            Some(0) => self.before,
            Some(1) => self.now,
            _ => Rect::NOTHING,
        }
    }
}

fn cover_id() -> Id {
    Id::new("glass-popup-cover")
}

/// What the open frosted popups covered last pass (`Rect::NOTHING` when
/// none was open): chrome drawn before them hides there, so it does not
/// show through their glass as a ghost.
pub fn popup_cover(ctx: &Context) -> Rect {
    if !ctx.memory(|m| m.any_popup_open()) {
        return Rect::NOTHING;
    }
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<Cover>(cover_id())).map_or(Rect::NOTHING, |c| c.last(pass))
}

/// The camera frame averaged over `block` x `block` pixels (one pixel per
/// block, every `step`th pixel sampled) for the frost behind popups: drawn
/// back with linear filtering it reads as a blur. Pure; a short buffer
/// gives black blocks.
pub fn block_average(rgba: &[u8], w: usize, h: usize, block: usize, step: usize) -> ColorImage {
    let block = block.max(1);
    let step = step.clamp(1, block);
    let (bw, bh) = (w.div_ceil(block).max(1), h.div_ceil(block).max(1));
    let mut px = Vec::with_capacity(bw * bh);
    for by in 0..bh {
        for bx in 0..bw {
            let (mut sum, mut n) = ([0u32; 3], 0u32);
            for y in (by * block..((by + 1) * block).min(h)).step_by(step) {
                for x in (bx * block..((bx + 1) * block).min(w)).step_by(step) {
                    if let Some(p) = rgba.get((y * w + x) * 4..(y * w + x) * 4 + 3) {
                        for (s, c) in sum.iter_mut().zip(p) {
                            *s += u32::from(*c);
                        }
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            let c = |i: usize| (sum[i] / n).min(255) as u8;
            px.push(Color32::from_rgb(c(0), c(1), c(2)));
        }
    }
    ColorImage { size: [bw, bh], pixels: px }
}

// ---- column layout (the plugin picker, spec v3 2.7) ----

/// One cell of a column layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColCell {
    /// A group's header; `cont` when the group carries on from the column before.
    Head { group: usize, cont: bool },
    /// Entry `item` of `group`.
    Item { group: usize, item: usize },
}

/// Lays groups of entries into columns of at most `cap` rows (pure). Each
/// group starts with its header row when it has one (`(head, entries)`).
/// A group goes into the current column when it fits in what is left;
/// else it starts a new column, so a group stays together where it can.
/// A group taller than a column fills the current one and carries on in
/// the next, its header repeated there (`cont`). `cap` below 2 counts as 2.
pub fn columns(groups: &[(bool, usize)], cap: usize) -> Vec<Vec<ColCell>> {
    let cap = cap.max(2);
    let mut cols: Vec<Vec<ColCell>> = Vec::new();
    let mut col: Vec<ColCell> = Vec::new();
    for (g, &(head, n)) in groups.iter().enumerate() {
        if n == 0 {
            continue;
        }
        let rows = n + usize::from(head);
        let left = cap - col.len();
        // a group that fits a column of its own never splits; a longer one
        // starts where its header and first entry still fit
        if !col.is_empty() && rows > left && (rows <= cap || left < usize::from(head) + 1) {
            cols.push(std::mem::take(&mut col));
        }
        if head {
            col.push(ColCell::Head { group: g, cont: false });
        }
        for item in 0..n {
            if col.len() == cap {
                cols.push(std::mem::take(&mut col));
                if head {
                    col.push(ColCell::Head { group: g, cont: true });
                }
            }
            col.push(ColCell::Item { group: g, item });
        }
    }
    if !col.is_empty() {
        cols.push(col);
    }
    cols
}

/// The group a cell belongs to.
fn cell_group(c: &ColCell) -> usize {
    match *c {
        ColCell::Head { group, .. } | ColCell::Item { group, .. } => group,
    }
}

/// The separators between the columns of `columns` (pure), one per gap,
/// left to right: true where a new group starts in the next column (a
/// stronger line), false where a group carries on there.
pub fn col_seps(cols: &[Vec<ColCell>]) -> Vec<bool> {
    cols.windows(2).map(|w| w[0].last().map(cell_group) != w[1].first().map(cell_group)).collect()
}

/// The alpha of a column separator: `EDGE` before a new group (like the
/// lines between a bar's sections), `SEP_SOFT` where a group goes on.
pub fn col_sep_alpha(new_group: bool) -> f32 {
    if new_group {
        Tok::EDGE
    } else {
        Tok::SEP_SOFT
    }
}

/// Where Left (`dir` < 0) or Right (`dir` > 0) moves a row of columns
/// `pitch` apart that is scrolled to `offset` of at most `max` (pure):
/// to the start of the column before or after the one at the left edge,
/// never past either end. `dir` 0 or no room to scroll keeps it.
pub fn col_step(offset: f32, dir: i32, pitch: f32, max: f32) -> f32 {
    let max = max.max(0.0);
    if dir == 0 || pitch <= 0.0 || max <= 0.0 {
        return offset.clamp(0.0, max);
    }
    let k = if dir > 0 { ((offset + 0.5) / pitch).floor() + 1.0 } else { ((offset - 0.5) / pitch).ceil() - 1.0 };
    (k * pitch).clamp(0.0, max)
}

/// What a `pick_row` click did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PickRow {
    pub clicked: bool,
    pub delete: bool,
}

/// One popup row, `POP_ROW_H` high over the list width: `name` left,
/// `place` right (13, HINT); selected in the selection fill. A disabled
/// row draws at OFF and takes no click (its hover text still shows).
/// `delete`: Some((label, armed)) shows `label` at the right end on hover;
/// the first click turns it into `armed`, the second sets `delete`. The
/// arm drops when the pointer leaves the row.
#[allow(clippy::too_many_arguments)]
pub fn pick_row(ui: &mut Ui, id: Id, selected: bool, enabled: bool, name: &str, place: &str, hover: &str, delete: Option<(&str, &str)>) -> PickRow {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), Tok::POP_ROW_H), Sense::hover());
    let row = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    let inside = ui.rect_contains_pointer(rect);
    let arm_id = id.with("arm");
    let mut armed = ui.data(|d| d.get_temp::<bool>(arm_id)).unwrap_or(false) && inside;
    let mut out = PickRow { clicked: enabled && row.clicked(), delete: false };
    // the Delete slot is registered after the row, so it takes the click
    let del = match (enabled, delete) {
        (true, Some((label, arm_text))) => {
            let text = if armed { arm_text } else { label };
            let g = galley(ui, text, Tok::BODY);
            let dr = Rect::from_min_max(pos2(rect.right() - g.size().x - 2.0 * PICK_PAD, rect.top()), rect.max);
            let d = ui.interact(dr, id.with("delete"), Sense::click());
            if d.clicked() {
                out.clicked = false;
                if armed {
                    out.delete = true;
                    armed = false;
                } else {
                    armed = true;
                }
            }
            Some((dr, d, label, arm_text))
        }
        _ => None,
    };
    ui.data_mut(|d| d.insert_temp(arm_id, armed));
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_POP_ROW);
    if selected {
        grad_rect(p, rect, cr, Tok::SEL, 0.15, 0.06);
    } else if enabled && inside {
        p.rect_filled(rect, cr, veil(Tok::HOVER));
    }
    let name_a = if !enabled {
        Tok::OFF
    } else if selected {
        Tok::STRONG
    } else {
        Tok::VALUE
    };
    let show_del = del.as_ref().is_some_and(|_| inside || armed);
    let right_text = match (&del, show_del) {
        (Some((_, _, label, arm_text)), true) => Some((if armed { *arm_text } else { *label }, Tok::LABEL)),
        _ if !place.is_empty() => Some((place, if enabled { Tok::HINT } else { Tok::OFF })),
        _ => None,
    };
    let mut name_right = rect.right() - PICK_PAD;
    if let Some((s, a)) = right_text {
        let g = galley(ui, s, Tok::BODY);
        let x = rect.right() - PICK_PAD - g.size().x;
        name_right = x - PICK_PAD;
        let a = match &del {
            Some((_, d, _, _)) if show_del && d.hovered() => Tok::VALUE,
            _ => a,
        };
        text_at(p, pos2(x, rect.center().y - g.size().y / 2.0), g, white(a));
    }
    let g = galley(ui, &fit(ui, name, Tok::BODY, (name_right - rect.left() - PICK_PAD).max(0.0)), Tok::BODY);
    text_at(p, pos2(rect.left() + PICK_PAD, rect.center().y - g.size().y / 2.0), g, white(name_a));
    if !hover.is_empty() {
        row.on_hover_text(hover);
    }
    out
}

/// A group header inside a popup list (HINT, not clickable).
pub fn pick_header(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), Tok::POP_ROW_H), Sense::hover());
    let g = galley(ui, text, Tok::BODY);
    text_at(ui.painter(), pos2(rect.left() + PICK_PAD, rect.center().y - g.size().y / 2.0), g, white(Tok::HINT));
}

/// A one-line text field, `CTRL_H` high and `width` wide, 13 pt, the
/// field look; `hint` shows while it is empty. The response's rect is the
/// whole field (egui's TextEdit reports the text area inside its margin).
pub fn text_field(ui: &mut Ui, id: Id, text: &mut String, width: f32, hint: &str) -> Response {
    let edit = egui::TextEdit::singleline(text)
        .id(id)
        .font(font(Tok::BODY))
        .hint_text(egui::RichText::new(hint).color(white(Tok::HINT)))
        .background_color(veil(Tok::FIELD))
        .margin(Margin::symmetric(6, 4))
        .vertical_align(Align::Center)
        .min_size(vec2(width, Tok::CTRL_H))
        .desired_width(width - 12.0);
    let layout = Layout::centered_and_justified(egui::Direction::LeftToRight);
    let inner = ui.allocate_ui_with_layout(vec2(width, Tok::CTRL_H), layout, |ui| ui.add(edit));
    let mut resp = inner.inner;
    resp.rect = inner.response.rect;
    resp
}

/// A section's header row (`HEAD_H`): the title (15, STRONG) at the left;
/// with `close` = Some((hover, armed_text)) a close group at the right
/// inset: a 1 px rule and an `x`. The first click on the `x` arms it
/// (`armed_text` shows left of the rule, the `x` lit) for 4 s or until
/// the pointer leaves the header; the second click confirms. Draws the
/// inset rule under the row. Returns the free slot between title and
/// close group (16 pt clear of both), and true on the confirming click.
/// The rule only with `rule`: a folded section has none, its section
/// separator follows right after.
pub fn section_head(ui: &mut Ui, id: Id, title: &str, close: Option<(&str, &str)>, rule: bool) -> (Rect, bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), Tok::HEAD_H), Sense::hover());
    let row = Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - 2.0));
    let g = galley(ui, title, Tok::TITLE);
    let title_right = row.left() + g.size().x;
    text_at(ui.painter(), pos2(row.left(), row.center().y - g.size().y / 2.0), g, white(Tok::STRONG));
    let mut group_left = row.right();
    let mut confirmed = false;
    if let Some((hover, arm_text)) = close {
        let arm_id = id.with("close-arm");
        let now = ui.input(|i| i.time);
        let inside = ui.rect_contains_pointer(rect);
        let mut armed = ui.data(|d| d.get_temp::<f64>(arm_id)).filter(|t| inside && now - t < ARM_S);
        let xr = Rect::from_center_size(pos2(row.right() - Tok::CTRL_H / 2.0, row.center().y), vec2(Tok::CTRL_H, Tok::CTRL_H));
        let x = ui.interact(xr, id.with("close"), Sense::click());
        if x.clicked() {
            if armed.is_some() {
                confirmed = true;
                armed = None;
            } else {
                armed = Some(now);
            }
        }
        match armed {
            Some(t) => {
                ui.data_mut(|d| d.insert_temp(arm_id, t));
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64((ARM_S - (now - t)).max(0.05)));
            }
            None => ui.data_mut(|d| d.remove::<f64>(arm_id)),
        }
        let p = ui.painter();
        let cr = CornerRadius::same(Tok::R_TOGGLE);
        if armed.is_some() {
            sel_rect(p, xr, cr);
        } else if x.is_pointer_button_down_on() {
            p.rect_filled(xr, cr, veil(Tok::PRESSED));
        } else if x.hovered() {
            p.rect_filled(xr, cr, veil(Tok::HOVER));
        }
        let xa = if armed.is_some() { Tok::STRONG } else { Tok::VALUE };
        let xg = galley(ui, "×", Tok::BODY);
        text_at(p, xr.center() - xg.size() / 2.0, xg, white(xa));
        let rule_x = p.round_to_pixel_center(xr.left() - Tok::GAP);
        let half = Tok::CTRL_H / 2.0 - 4.0;
        p.vline(rule_x, Rangef::new(row.center().y - half, row.center().y + half), Stroke::new(1.0_f32, white(Tok::EDGE)));
        group_left = rule_x;
        if armed.is_some() {
            let ag = galley(ui, arm_text, Tok::BODY);
            let ax = rule_x - Tok::GAP - ag.size().x;
            text_at(p, pos2(ax, row.center().y - ag.size().y / 2.0), ag, white(Tok::VALUE));
            group_left = ax;
        }
        if !hover.is_empty() {
            x.on_hover_text(hover);
        }
    }
    if rule {
        section_rule_in(ui.painter(), rect.x_range(), rect.bottom() - 2.0);
    }
    let slot_l = (title_right + 16.0).min(group_left);
    let slot_r = (group_left - 16.0).max(slot_l);
    (Rect::from_min_max(pos2(slot_l, row.top()), pos2(slot_r, row.bottom())), confirmed)
}

/// A list row's remove `x` in `xr` (a square at the row's right end):
/// the first click arms it (the selection look, the `x` lit, `armed_text`
/// left of it at LABEL) for `ARM_S` or until the pointer leaves `row`;
/// the second click confirms. Returns true on the confirming click and
/// the left edge of what it drew, where the row's own text has to end.
pub fn arm_x(ui: &mut Ui, id: Id, xr: Rect, row: Rect, hover: &str, armed_text: &str) -> (bool, f32) {
    let arm_id = id.with("x-arm");
    let now = ui.input(|i| i.time);
    let inside = ui.rect_contains_pointer(row);
    let mut armed = ui.data(|d| d.get_temp::<f64>(arm_id)).filter(|t| inside && now - t < ARM_S);
    let x = ui.interact(xr, id.with("x"), Sense::click());
    let mut confirmed = false;
    if x.clicked() {
        if armed.is_some() {
            confirmed = true;
            armed = None;
        } else {
            armed = Some(now);
        }
    }
    match armed {
        Some(t) => {
            ui.data_mut(|d| d.insert_temp(arm_id, t));
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64((ARM_S - (now - t)).max(0.05)));
        }
        None => ui.data_mut(|d| d.remove::<f64>(arm_id)),
    }
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_TOGGLE);
    if armed.is_some() {
        sel_rect(p, xr, cr);
    } else if x.is_pointer_button_down_on() {
        p.rect_filled(xr, cr, veil(Tok::PRESSED));
    } else if x.hovered() {
        p.rect_filled(xr, cr, veil(Tok::HOVER));
    }
    let xa = if armed.is_some() || x.hovered() { Tok::STRONG } else { Tok::LABEL };
    let xg = galley(ui, "×", Tok::BODY);
    text_at(p, xr.center() - xg.size() / 2.0, xg, white(xa));
    let mut left = xr.left();
    if armed.is_some() {
        let ag = galley(ui, armed_text, Tok::BODY);
        left = xr.left() - Tok::GAP - ag.size().x;
        text_at(p, pos2(left, xr.center().y - ag.size().y / 2.0), ag, white(Tok::LABEL));
    }
    if !hover.is_empty() && armed.is_none() {
        x.on_hover_text(hover);
    }
    (confirmed, left)
}

/// The two lines of a rule at `y` over `x`, in the painter's clip.
fn section_rule_in(painter: &Painter, x: Rangef, y: f32) {
    let y = painter.round_to_pixel_center(y);
    painter.hline(x, y, Stroke::new(1.0_f32, white(Tok::RULE)));
    painter.hline(x, y + 1.0, Stroke::new(1.0_f32, black(Tok::RULE_DARK)));
}

/// Full-width two-line rule at `y` over `x` (window x range): a white
/// line at 0.10 over a black line at 0.35. The clip widens to `x` (the
/// section's own Ui is inset) and keeps the painter's vertical clip, so a
/// rule in the scrolling panel area never draws over the camera.
pub fn section_rule(painter: &egui::Painter, x: egui::Rangef, y: f32) {
    let clip = painter.clip_rect();
    let p = painter.with_clip_rect(Rect::from_x_y_ranges(x, clip.y_range()));
    section_rule_in(&p, x, y);
}

/// A row that folds content (Advanced, Signals, Diagnostics): chevron,
/// title (15, STRONG), the summary (13, LABEL) while closed. A click
/// toggles `open`.
pub fn disclosure(ui: &mut Ui, id: Id, open: &mut bool, title: &str, summary: Option<&str>) -> Response {
    disclosure_h(ui, id, open, title, summary, Tok::HEAD_H, Tok::TITLE)
}

/// The compact `disclosure` of the Settings panel: `CTRL_H` high, the
/// title at `BODY`.
pub fn disclosure_sm(ui: &mut Ui, id: Id, open: &mut bool, title: &str) -> Response {
    disclosure_h(ui, id, open, title, None, Tok::CTRL_H, Tok::BODY)
}

fn disclosure_h(ui: &mut Ui, id: Id, open: &mut bool, title: &str, summary: Option<&str>, h: f32, size: f32) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let mut resp = ui.interact(rect, id, Sense::click());
    if resp.clicked() {
        *open = !*open;
        resp.mark_changed();
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let turn = ui.ctx().animate_bool_with_time(id.with("turn"), *open, 0.15);
    let p = ui.painter();
    chevron_at(p, pos2(rect.left() + 4.0, rect.center().y), 4.0, turn, white(if resp.hovered() { Tok::VALUE } else { Tok::HINT }));
    let g = galley(ui, title, size);
    let tx = rect.left() + 16.0;
    let right = tx + g.size().x;
    text_at(p, pos2(tx, rect.center().y - g.size().y / 2.0), g, white(Tok::STRONG));
    if let (false, Some(s)) = (*open, summary) {
        let sx = right + Tok::INSET;
        let g = galley(ui, &fit(ui, s, Tok::BODY, (rect.right() - sx).max(0.0)), Tok::BODY);
        text_at(p, pos2(sx, rect.center().y - g.size().y / 2.0), g, white(Tok::LABEL));
    }
    resp
}

/// The "+" next to the preset picker (`TOP_H` square): HINT while not
/// `lit`, VALUE with the selection edge when lit; clickable in both.
pub fn plus_button(ui: &mut Ui, lit: bool, hover: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(Tok::TOP_H, Tok::TOP_H), Sense::click());
    let p = ui.painter();
    let cr = CornerRadius::same(Tok::R_TOGGLE);
    if resp.is_pointer_button_down_on() {
        p.rect_filled(rect, cr, veil(Tok::PRESSED));
    } else if resp.hovered() {
        p.rect_filled(rect, cr, veil(Tok::HOVER));
    }
    if lit {
        p.rect_stroke(rect, cr, sel_edge(), StrokeKind::Inside);
    }
    let g = galley(ui, "+", Tok::TITLE);
    text_at(p, rect.center() - g.size() / 2.0, g, white(if lit { Tok::VALUE } else { Tok::HINT }));
    if hover.is_empty() { resp } else { resp.on_hover_text(hover) }
}

/// Lays `add` out left to right, centred in `width` from the current
/// left edge, using the width it took last frame. When that width is
/// missing or changed, the pass is discarded (`request_discard`), so the
/// row never shows left-aligned or off-centre for a frame.
pub fn centered_row<R>(ui: &mut Ui, id: Id, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let key = id.with("centered-row");
    let prev = ui.data(|d| d.get_temp::<f32>(key));
    let left = ui.available_rect_before_wrap().left();
    let x = left + ((width - prev.unwrap_or(0.0)) / 2.0).max(0.0);
    let max = Rect::from_min_size(pos2(x, ui.cursor().top()), vec2((left + width - x).max(0.0), ui.spacing().interact_size.y));
    let inner = ui.scope_builder(UiBuilder::new().max_rect(max).layout(Layout::left_to_right(Align::Center)), add);
    let used = inner.response.rect.width();
    if prev.is_none_or(|p| (p - used).abs() > 0.5) {
        ui.data_mut(|d| d.insert_temp(key, used));
        ui.ctx().request_discard("glass::centered_row");
    }
    inner.inner
}

/// The width of `text` at `size` with every ASCII digit replaced by '8'.
pub fn num_slot_width(ui: &Ui, text: &str, size: f32) -> f32 {
    let wide: String = text.chars().map(|c| if c.is_ascii_digit() { '8' } else { c }).collect();
    galley(ui, &wide, size).size().x
}

/// Right-aligned text in a slot as wide as `text` with every digit
/// replaced by '8', so a changing number never jitters.
pub fn num_slot(ui: &mut Ui, text: &str, size: f32, color: Color32) -> Response {
    let g = galley(ui, text, size);
    let w = num_slot_width(ui, text, size).max(g.size().x);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
    text_at(ui.painter(), pos2(rect.right() - g.size().x, rect.top()), g, color);
    resp
}

/// The toast's alpha at `age` seconds: info stays 4 s, then fades.
fn toast_alpha(error: bool, age: f32) -> f32 {
    if error {
        1.0
    } else {
        (1.0 - (age - TOAST_SHOW) / TOAST_FADE).clamp(0.0, 1.0)
    }
}

/// The toast of 2.9 at the bottom left of `window`: a glass pill 12 pt
/// from the left and bottom edges, `Order::Foreground`, text 13 VALUE.
/// Info (`error` false) fades out after 4 s of `age` and never takes a
/// click; an error stays and returns true when it is clicked.
pub fn toast(ctx: &egui::Context, window: Rect, text: &str, error: bool, age: f32) -> bool {
    let a = toast_alpha(error, age);
    if text.is_empty() || a <= 0.0 {
        return false;
    }
    if !error {
        let wait = if age < TOAST_SHOW { TOAST_SHOW - age } else { 0.0 };
        ctx.request_repaint_after(std::time::Duration::from_secs_f32(wait.max(1.0 / 60.0)));
    }
    let max_w = (window.width() - 2.0 * TOAST_MARGIN - 16.0).max(40.0);
    let area = egui::Area::new(Id::new("glass-toast"))
        .order(Order::Foreground)
        .fixed_pos(pos2(window.left() + TOAST_MARGIN, window.bottom() - TOAST_MARGIN))
        .pivot(Align2::LEFT_BOTTOM)
        .constrain(false)
        .interactable(error)
        .sense(if error { Sense::click() } else { Sense::hover() })
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(pop_fill().gamma_multiply(a))
                .stroke(Stroke::new(0.7_f32, white(Tok::EDGE * a)))
                .corner_radius(CornerRadius::same(Tok::R_POP))
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    let g = ui.painter().layout(text.to_owned(), font(Tok::BODY), Color32::WHITE, max_w);
                    let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                    text_at(ui.painter(), r.min, g, white(Tok::VALUE * a));
                });
        });
    if error && area.response.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    error && area.response.clicked()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use nih_plug_egui::egui::{Event, Modifiers, RawInput};

    use super::*;

    fn walk_texts(s: &egui::Shape, out: &mut Vec<(String, Pos2, f32, Color32)>) {
        match s {
            egui::Shape::Text(t) => {
                let size = t.galley.job.sections.first().map_or(0.0, |s| s.format.font_id.size);
                let color = t.override_text_color.unwrap_or(t.fallback_color);
                out.push((t.galley.text().to_string(), t.pos + t.galley.rect.center().to_vec2(), size, color));
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk_texts(s, out)),
            _ => {}
        }
    }

    /// (text, centre, font size, colour) of every text drawn.
    fn all_texts(out: Option<&egui::FullOutput>) -> Vec<(String, Pos2, f32, Color32)> {
        let mut v = Vec::new();
        for c in out.iter().flat_map(|o| &o.shapes) {
            walk_texts(&c.shape, &mut v);
        }
        v
    }

    fn near(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    /// A headless window for the kit: `f` runs inside a CentralPanel.
    struct Kit {
        ctx: egui::Context,
        time: f64,
        out: Option<egui::FullOutput>,
    }

    impl Kit {
        fn new() -> Self {
            let ctx = egui::Context::default();
            style(&ctx);
            Kit { ctx, time: 0.0, out: None }
        }

        fn frame(&mut self, events: Vec<Event>, f: &mut dyn FnMut(&mut egui::Ui)) {
            self.time += 1.0 / 60.0;
            let raw = RawInput { events, time: Some(self.time), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() };
            self.out = Some(self.ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| f(ui));
            }));
        }

        fn button(at: Pos2, pressed: bool) -> Event {
            Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
        }

        fn click(&mut self, at: Pos2, f: &mut dyn FnMut(&mut egui::Ui)) {
            self.frame(vec![Event::PointerMoved(at), Self::button(at, true)], f);
            self.frame(vec![Self::button(at, false)], f);
            self.frame(vec![], f);
        }

        /// Hover first (a real pointer gets there before it clicks), then click.
        fn hover_click(&mut self, at: Pos2, f: &mut dyn FnMut(&mut egui::Ui)) {
            self.frame(vec![Event::PointerMoved(at)], f);
            self.click(at, f);
        }

        fn texts(&self) -> Vec<(String, Pos2, f32, Color32)> {
            all_texts(self.out.as_ref())
        }

        fn find(&self, needle: &str) -> Option<Pos2> {
            self.texts().into_iter().find(|(s, ..)| s == needle).map(|(_, p, ..)| p)
        }
    }

    /// Where `needle` is drawn, else far outside the window (a click there
    /// hits nothing and the next assertion fails).
    fn spot(k: &Kit, needle: &str) -> Pos2 {
        k.find(needle).unwrap_or(Pos2::new(-500.0, -500.0))
    }

    #[test]
    fn glass_style_sets_tokens() {
        let ctx = egui::Context::default();
        style(&ctx);
        let s = ctx.style();
        assert_eq!(s.spacing.interact_size.y, 24.0);
        assert_eq!(s.spacing.button_padding, vec2(8.0, 4.0));
        assert_eq!(s.spacing.item_spacing, vec2(Tok::GAP, Tok::ROW_GAP));
        let w = &s.visuals.widgets;
        for v in [&w.noninteractive, &w.inactive, &w.hovered, &w.active, &w.open] {
            assert_eq!(v.expansion, 0.0);
        }
        assert_eq!(s.visuals.window_shadow, egui::Shadow::NONE);
        let c = &s.visuals.text_cursor;
        assert_eq!(c.stroke, Stroke::new(1.0_f32, Tok::ACCENT_HI));
        assert!(c.blink && c.on_duration == 0.5 && c.off_duration == 0.5);
        let size = |ts: egui::TextStyle| s.text_styles.get(&ts).map(|f| f.size);
        assert_eq!(size(egui::TextStyle::Body), Some(13.0));
        assert_eq!(size(egui::TextStyle::Button), Some(13.0));
        assert_eq!(size(egui::TextStyle::Small), Some(11.0));
        assert_eq!(size(title_style()), Some(15.0));
        assert_eq!(size(egui::TextStyle::Heading), Some(18.0));
        assert!(size(egui::TextStyle::Monospace).is_some_and(|m| m >= 13.0));
    }

    #[test]
    fn a_short_toggle_splits_its_spare_width_evenly() {
        for (text, small) in [("On", false), ("Live", false), ("An", true), ("Suggestions while typing", false)] {
            let mut k = Kit::new();
            let (mut on, mut rect, mut gw) = (false, Rect::NOTHING, 0.0);
            let size = if small { Tok::SMALL } else { Tok::BODY };
            k.frame(vec![], &mut |ui| {
                gw = galley(ui, text, size).size().x;
                rect = if small { toggle_sm(ui, &mut on, text, Tok::LIVE) } else { toggle(ui, &mut on, text, Tok::LIVE) }.rect;
            });
            let Some(c) = k.find(text) else { return assert!(k.find(text).is_some(), "{text}") };
            let kk = rect.height() / Tok::CTRL_H;
            // the dot (radius 3k) sits 8k left of the text
            let left = c.x - gw / 2.0 - 8.0 * kk - 3.0 * kk - rect.left();
            let right = rect.right() - (c.x + gw / 2.0);
            assert!(near(left + kk, right, 0.6), "{text}: {left} left, {right} right in {rect:?}");
        }
    }

    fn kit_widgets(ui: &mut egui::Ui, seg: &mut u8, on: &mut bool, v: &mut i32, field: &mut Rect) {
        segmented(ui, seg, &[(0u8, "Lanes"), (1u8, "Piano roll")]);
        toggle(ui, on, "Live", Tok::LIVE);
        *field = slider_field(ui, Id::new("test-field"), v, 1..=127, 100, "velocity", 96.0, |x| x.to_string()).rect;
    }

    #[test]
    fn segmented_toggle_and_slider_field() {
        let mut k = Kit::new();
        let (mut seg, mut on, mut v) = (0u8, false, 64i32);
        let mut field = Rect::NOTHING;
        k.frame(vec![], &mut |ui| kit_widgets(ui, &mut seg, &mut on, &mut v, &mut field));
        let p = spot(&k, "Piano roll");
        k.click(p, &mut |ui| kit_widgets(ui, &mut seg, &mut on, &mut v, &mut field));
        assert_eq!(seg, 1);
        let p = spot(&k, "Live");
        k.click(p, &mut |ui| kit_widgets(ui, &mut seg, &mut on, &mut v, &mut field));
        assert!(on);
        // a +40 pt drag on 1..=127: 40 * 126 / 200 = 25.2
        let a = field.center();
        k.frame(vec![Event::PointerMoved(a), Kit::button(a, true)], &mut |ui| kit_widgets(ui, &mut seg, &mut on, &mut v, &mut field));
        for i in 1..=8 {
            let p = a + egui::vec2(5.0 * i as f32, 0.0);
            k.frame(vec![Event::PointerMoved(p)], &mut |ui| kit_widgets(ui, &mut seg, &mut on, &mut v, &mut field));
        }
        let end = a + egui::vec2(40.0, 0.0);
        k.frame(vec![Kit::button(end, false)], &mut |ui| kit_widgets(ui, &mut seg, &mut on, &mut v, &mut field));
        assert!((v - (64 + 25)).abs() <= 1, "{v}");
        // a double-click resets (after a pause: three clicks in 0.6 s are a triple-click)
        let c = field.center();
        for _ in 0..40 {
            k.frame(vec![Event::PointerMoved(c)], &mut |ui| kit_widgets(ui, &mut seg, &mut on, &mut v, &mut field));
        }
        for _ in 0..2 {
            k.frame(vec![Kit::button(c, true)], &mut |ui| kit_widgets(ui, &mut seg, &mut on, &mut v, &mut field));
            k.frame(vec![Kit::button(c, false)], &mut |ui| kit_widgets(ui, &mut seg, &mut on, &mut v, &mut field));
        }
        assert_eq!(v, 100);
    }

    fn fills(s: &egui::Shape, out: &mut Vec<Color32>) {
        match s {
            egui::Shape::Rect(r) => out.push(r.fill),
            egui::Shape::Circle(c) => out.push(c.fill),
            egui::Shape::Vec(v) => v.iter().for_each(|s| fills(s, out)),
            _ => {}
        }
    }

    /// `c` over `bg` as egui_glow blends: premultiplied, in gamma space.
    fn over(c: Color32, bg: Color32) -> Color32 {
        let k = 1.0 - f32::from(c.a()) / 255.0;
        let ch = |s: u8, d: u8| (f32::from(s) + f32::from(d) * k).round().min(255.0) as u8;
        Color32::from_rgb(ch(c.r(), bg.r()), ch(c.g(), bg.g()), ch(c.b(), bg.b()))
    }

    fn pressables(ui: &mut egui::Ui, seg: &mut u8, on: &mut bool, v: &mut i32, check: &mut bool) {
        ui.horizontal_wrapped(|ui| {
            quiet(ui, "Quiet");
            chip(ui, false, "Chip", None);
            toggle(ui, on, "Toggle", Tok::LIVE);
            segmented(ui, seg, &[(0u8, "SegA"), (1u8, "SegB")]);
            let _ = ui.button("Button");
            ui.checkbox(check, "Check");
            dropdown(ui, "test-drop", 120.0, "Drop", &0u8, [(0u8, "Zero".to_string())]);
            slider_field(ui, Id::new("test-press-field"), v, 0..=10, 5, "Field", 96.0, |x| x.to_string());
            picker_button(ui, Id::new("test-press-picker"), vec2(120.0, Tok::TOP_H), "Picker", false, "");
            plus_button(ui, true, "");
            toggle_sm(ui, on, "Small", Tok::LIVE);
        });
    }

    #[test]
    fn a_pressed_control_never_fills_white() {
        // a grey fill may lift the window by a few levels at most; white(0.05)
        // through egui 0.31's linear premultiply lifted it from 30 to 89
        let lift = |c: &Color32| {
            let (lo, hi) = (c.r().min(c.g()).min(c.b()), c.r().max(c.g()).max(c.b()));
            let grey = hi - lo < 24;
            if grey { i32::from(over(*c, Tok::WINDOW).r()) - i32::from(Tok::WINDOW.r()) } else { 0 }
        };
        for name in ["Quiet", "Chip", "Toggle", "SegA", "SegB", "Button", "Check", "Drop", "Field", "Picker", "+", "Small"] {
            let mut k = Kit::new();
            let (mut seg, mut on, mut v, mut check) = (0u8, false, 5i32, false);
            let mut ui_fn = |ui: &mut egui::Ui| pressables(ui, &mut seg, &mut on, &mut v, &mut check);
            k.frame(vec![], &mut ui_fn);
            let p = k.find(name);
            assert!(p.is_some(), "{name} drawn");
            let Some(p) = p else { return };
            // hover, press, hold, release
            let mut frames = vec![vec![Event::PointerMoved(p)], vec![Kit::button(p, true)], vec![], vec![], vec![Kit::button(p, false)], vec![], vec![]];
            for (i, events) in frames.drain(..).enumerate() {
                k.frame(events, &mut ui_fn);
                let mut all = Vec::new();
                for c in k.out.iter().flat_map(|o| &o.shapes) {
                    fills(&c.shape, &mut all);
                }
                let white: Vec<(Color32, i32)> = all.iter().map(|c| (*c, lift(c))).filter(|(_, l)| *l > 20).collect();
                assert!(white.is_empty(), "{name}, frame {i}: {white:?}");
            }
        }
        // the regression itself: the old fill flashed
        assert!(lift(&white(Tok::HOVER)) > 40 && lift(&veil(Tok::HOVER)) < 15);
    }

    #[test]
    fn velocity_bars_and_slider_fills_read_on_the_dark_panel() {
        let lin = |c: u8| {
            let s = f32::from(c) / 255.0;
            if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
        };
        let lum = |c: Color32| 0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b());
        let ratio = |a: Color32, b: Color32| (lum(a).max(lum(b)) + 0.05) / (lum(a).min(lum(b)) + 0.05);
        // the event lane and a field as rendered over the window colour,
        // which is lighter than the frosted camera under the panels
        let lane = over(veil(Tok::INSET_FILL), Tok::WINDOW);
        let field = over(veil(Tok::FIELD), Tok::WINDOW);
        assert!(ratio(Tok::ACCENT_FILL, lane) >= 3.0, "{}", ratio(Tok::ACCENT_FILL, lane));
        assert!(ratio(Tok::ACCENT_FILL, field) >= 3.0, "{}", ratio(Tok::ACCENT_FILL, field));
        assert!(ratio(Tok::ACCENT_HI, lane) >= 4.5);
        // the value text on a slider fill stays readable
        let text = over(white(Tok::VALUE), Tok::ACCENT_FILL);
        assert!(ratio(text, Tok::ACCENT_FILL) >= 4.5, "{}", ratio(text, Tok::ACCENT_FILL));
        let ctx = egui::Context::default();
        style(&ctx);
        assert_eq!(ctx.style().visuals.selection.bg_fill, Tok::ACCENT_FILL, "the signal sliders fill with it");
    }

    #[test]
    fn picker_button_has_the_given_size_and_13_pt_text() {
        let mut k = Kit::new();
        let mut rect = Rect::NOTHING;
        let mut f = |ui: &mut egui::Ui| {
            rect = picker_button(ui, Id::new("pb"), vec2(204.0, Tok::TOP_H), "Serum 2", false, "Plugin picker").rect;
            picker_button(ui, Id::new("pb2"), vec2(204.0, Tok::TOP_H), "Pick a preset", true, "");
            picker_button(ui, Id::new("pb3"), vec2(120.0, Tok::TOP_H), "A very long plugin name that cannot fit", false, "");
        };
        k.frame(vec![], &mut f);
        assert!(near(rect.width(), 204.0, 0.01) && near(rect.height(), 30.0, 0.01), "{rect:?}");
        let texts = k.texts();
        let value = texts.iter().find(|t| t.0 == "Serum 2");
        assert!(value.is_some_and(|t| t.2 == 13.0 && t.3 == white(Tok::VALUE)), "{value:?}");
        let ph = texts.iter().find(|t| t.0 == "Pick a preset");
        assert!(ph.is_some_and(|t| t.2 == 13.0 && t.3 == white(Tok::HINT)), "placeholder at HINT: {ph:?}");
        assert!(texts.iter().any(|t| t.0.ends_with('…')), "a long name is cut: {texts:?}");
    }

    #[test]
    fn picker_popup_right_aligns_at_the_window_edge() {
        for (pad, right) in [(10.0f32, false), (640.0, true)] {
            let mut k = Kit::new();
            let button = Cell::new(Rect::NOTHING);
            let id = Id::new(("pp", right));
            let mut f = |ui: &mut egui::Ui| {
                ui.horizontal(|ui| {
                    ui.add_space(pad);
                    let b = picker_button(ui, id, vec2(140.0, Tok::TOP_H), "Pick", false, "");
                    button.set(b.rect);
                    picker_popup(ui, id, &b, 420.0, |ui| {
                        pick_row(ui, Id::new("row"), false, true, "Hands", "Built-in", "", None);
                    });
                });
            };
            k.frame(vec![], &mut f);
            let c = button.get().center();
            k.click(c, &mut f);
            k.frame(vec![], &mut f);
            let button = button.get();
            let pop = k.ctx.memory(|m| m.area_rect(id.with("popup")));
            assert!(pop.is_some(), "the popup is open");
            let Some(pop) = pop else { return };
            assert!(near(pop.width(), 200.0, 0.5), "at least 200 wide: {pop:?}");
            assert!(near(pop.top(), button.bottom() + 2.0, 0.5), "under the button: {pop:?} {button:?}");
            if right {
                assert!(near(pop.right(), button.right(), 0.5), "right-aligned: {pop:?} {button:?}");
                assert!(pop.right() <= 800.0);
            } else {
                assert!(near(pop.left(), button.left(), 0.5), "left-aligned: {pop:?} {button:?}");
            }
            assert!(k.find("Hands").is_some() && k.find("Built-in").is_some());
            // Esc closes it
            k.frame(vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], &mut f);
            k.frame(vec![], &mut f);
            assert!(k.find("Hands").is_none(), "Esc closes the popup");
        }
    }

    /// Upward placement (spec v3 2.7): above the anchor when two rows fit,
    /// never past the window's margin, shifted left at the right edge.
    #[test]
    fn placement_never_leaves_the_window() {
        let win = Rect::from_min_size(pos2(0.0, 0.0), vec2(720.0, 520.0));
        let inside = |r: Rect| win.shrink(POP_MARGIN - 0.01).contains_rect(r);
        // the plugin picker under a 145 pt camera opens upward over it
        let button = Rect::from_min_size(pos2(12.0, 157.0), vec2(137.0, 30.0));
        let at = place(button, 1200.0, 160.0, win, Open::Up);
        assert_eq!(at.pivot, Align2::LEFT_BOTTOM);
        assert!(near(at.pos.y, 155.0, 0.01) && near(at.max_h, 151.0, 0.01), "{at:?}");
        assert!(near(at.w, 712.0, 0.01), "never wider than the window: {at:?}");
        assert!(inside(at.rect(at.max_h)) && inside(at.rect(1000.0)), "{:?}", at.rect(1000.0));
        // "+" at the right edge: right-aligned to it
        let plus = Rect::from_min_size(pos2(678.0, 157.0), vec2(30.0, 30.0));
        let at = place(plus, 200.0, 80.0, win, Open::Up);
        assert!(near(at.rect(80.0).right(), 708.0, 0.01), "{:?}", at.rect(80.0));
        // no room above: Up falls back to below
        let top = Rect::from_min_size(pos2(100.0, 10.0), vec2(100.0, 24.0));
        assert_eq!(place(top, 200.0, 300.0, win, Open::Up).pivot, Align2::LEFT_TOP);
        // Auto: below when it fits, else the side with more room
        let low = Rect::from_min_size(pos2(100.0, 400.0), vec2(100.0, 24.0));
        assert_eq!(place(low, 200.0, 60.0, win, Open::Auto).pivot, Align2::LEFT_TOP);
        assert_eq!(place(low, 200.0, 300.0, win, Open::Auto).pivot, Align2::LEFT_BOTTOM);
        // any anchor, any size, any direction: inside the window
        for ax in [-50.0f32, 0.0, 300.0, 690.0, 800.0] {
            for ay in [-40.0f32, 0.0, 150.0, 300.0, 500.0, 600.0] {
                for (w, h) in [(30.0f32, 22.0f32), (200.0, 100.0), (900.0, 2000.0)] {
                    for open in [Open::Up, Open::Auto] {
                        let anchor = Rect::from_min_size(pos2(ax, ay), vec2(140.0, 30.0));
                        let at = place(anchor, w, h, win, open);
                        let r = at.rect(h);
                        assert!(inside(r), "{anchor:?} {w}x{h} {open:?}: {r:?}");
                    }
                }
            }
        }
    }

    /// The plugin picker's columns: a track stays in one column where it
    /// fits, a long one splits with its header repeated, nothing is lost.
    #[test]
    fn columns_wrap_and_keep_groups_together() {
        let item = |group, item| ColCell::Item { group, item };
        let head = |group, cont| ColCell::Head { group, cont };
        // Follow FL (no header), then tracks of 4, 3 and 2 plugins
        let cols = columns(&[(false, 1), (true, 4), (true, 3), (true, 2)], 6);
        assert_eq!(cols[0], vec![item(0, 0), head(1, false), item(1, 0), item(1, 1), item(1, 2), item(1, 3)]);
        assert_eq!(cols[1], vec![head(2, false), item(2, 0), item(2, 1), item(2, 2)]);
        assert_eq!(cols[2], vec![head(3, false), item(3, 0), item(3, 1)], "a group that does not fit starts a column");
        // Master with 6 effects: 7 rows split into 6 + a continued header
        let cols = columns(&[(true, 6)], 6);
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[1], vec![head(0, true), item(0, 5)]);
        // a tiny cap still makes progress
        let cols = columns(&[(true, 3)], 0);
        assert!(cols.iter().all(|c| c.len() <= 2) && cols.len() == 3, "{cols:?}");
        // the stress project: Master 6 FX, ten tracks of 3 to 5 plugins
        let mut groups = vec![(false, 1), (true, 6)];
        groups.extend([4, 4, 3, 5, 4, 3, 4, 4, 3, 5].map(|n| (true, n)));
        for cap in [2usize, 3, 5, 6, 9] {
            let cols = columns(&groups, cap);
            assert!(cols.iter().all(|c| !c.is_empty() && c.len() <= cap.max(2)), "{cap}");
            let items: Vec<(usize, usize)> = cols.iter().flatten().filter_map(|c| match *c {
                ColCell::Item { group, item } => Some((group, item)),
                ColCell::Head { .. } => None,
            }).collect();
            let want: Vec<(usize, usize)> = groups.iter().enumerate().flat_map(|(g, (_, n))| (0..*n).map(move |i| (g, i))).collect();
            assert_eq!(items, want, "{cap}: every plugin once, in order");
            // a group never starts at the bottom of a column without its header
            for c in &cols {
                if let Some(ColCell::Item { group, item: 0 }) = c.first() {
                    assert!(!groups[*group].0, "{cap}: a headed group starts with its header");
                }
            }
            if cap >= 6 {
                let split = |g: usize| cols.iter().filter(|c| c.iter().any(|x| matches!(x, ColCell::Item { group, .. } if *group == g))).count();
                assert!((2..groups.len()).all(|g| split(g) == 1), "{cap}: every track in one column");
            }
        }
    }

    /// Noa (v3 follow-up): thin lines between the picker's columns, the
    /// stronger one where a new track starts.
    #[test]
    fn column_separators_mark_new_tracks_and_continued_ones() {
        // Follow + Master (6 FX, carries on), Track 1 (3), Track 2 (2)
        let cols = columns(&[(false, 1), (true, 6), (true, 3), (true, 2)], 6);
        // Master carries on in column 1; Tracks 1 and 2 start columns 2 and 3
        assert_eq!(cols.len(), 4, "{cols:?}");
        assert_eq!(col_seps(&cols), vec![false, true, true], "{cols:?}");
        assert!(col_sep_alpha(true) > col_sep_alpha(false));
        assert_eq!(col_sep_alpha(true), Tok::EDGE);
        assert_eq!(col_sep_alpha(false), Tok::SEP_SOFT);
        // one column: no line; every gap gets exactly one
        assert!(col_seps(&columns(&[(true, 2)], 6)).is_empty());
        let mut groups = vec![(false, 1), (true, 6)];
        groups.extend([4, 4, 3, 5, 4, 3, 4, 4, 3, 5].map(|n| (true, n)));
        let cols = columns(&groups, 6);
        let seps = col_seps(&cols);
        assert_eq!(seps.len(), cols.len() - 1);
        // a continued header always sits after a soft line
        for (i, c) in cols.iter().enumerate().skip(1) {
            if let Some(ColCell::Head { cont, .. }) = c.first() {
                assert_eq!(seps[i - 1], !cont, "column {i}");
            }
        }
    }

    /// Left/Right page the columns one pitch at a time and stop at the ends.
    #[test]
    fn arrow_keys_step_one_column_and_stop_at_the_ends() {
        let (pitch, max) = (192.0, 1000.0);
        assert_eq!(col_step(0.0, 1, pitch, max), 192.0);
        assert_eq!(col_step(192.0, 1, pitch, max), 384.0);
        // from between two columns: to the next start, or back to this one's
        assert_eq!(col_step(250.0, 1, pitch, max), 384.0);
        assert_eq!(col_step(250.0, -1, pitch, max), 192.0);
        assert_eq!(col_step(192.0, -1, pitch, max), 0.0);
        assert_eq!(col_step(0.0, -1, pitch, max), 0.0);
        // the last step lands on the end, not past it
        assert_eq!(col_step(960.0, 1, pitch, max), 1000.0);
        assert_eq!(col_step(1000.0, 1, pitch, max), 1000.0);
        assert_eq!(col_step(1000.0, -1, pitch, max), 960.0);
        // nothing to scroll, no key or a broken pitch: stays in range
        assert_eq!(col_step(0.0, 1, pitch, 0.0), 0.0);
        assert_eq!(col_step(300.0, 0, pitch, max), 300.0);
        assert_eq!(col_step(300.0, 1, 0.0, max), 300.0);
        assert_eq!(col_step(5000.0, 0, pitch, max), max);
    }

    /// The top bar hides under a popup that covered it last pass.
    #[test]
    fn popup_cover_is_last_passes_union() {
        let a = Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 10.0));
        let b = Rect::from_min_size(pos2(20.0, 0.0), vec2(10.0, 10.0));
        let mut c = Cover::default();
        c.add(5, a);
        c.add(5, b);
        assert_eq!(c.last(5), Rect::NOTHING, "nothing known before pass 5");
        assert_eq!(c.last(6), a.union(b));
        c.add(6, b);
        assert_eq!(c.last(6), a.union(b), "pass 6 still reads pass 5");
        assert_eq!(c.last(7), b);
        assert_eq!(c.last(9), Rect::NOTHING, "a closed popup covers nothing");
    }

    #[test]
    fn frost_shows_the_blurred_camera_only_over_the_camera() {
        let tex = egui::TextureId::Managed(7);
        let src = FrostSrc {
            win: Rect::from_min_size(pos2(0.0, 0.0), vec2(720.0, 520.0)),
            cam: Rect::from_min_size(pos2(0.0, 0.0), vec2(720.0, 145.0)),
            img: Rect::from_min_size(pos2(174.5, 0.0), vec2(371.0, 145.0)),
            uv: Rect::from_min_max(pos2(0.0, 0.105), pos2(1.0, 0.805)),
            blur: Some(tex),
        };
        let rect_of = |s: &egui::Shape| match s {
            egui::Shape::Rect(r) => Some(r.clone()),
            _ => None,
        };
        // over the dither only: the strong tint
        let shapes = frost_shapes(Some(&src), Rect::from_min_size(pos2(20.0, 300.0), vec2(200.0, 100.0)), Tok::R_POP);
        assert_eq!(shapes.len(), 1);
        assert_eq!(rect_of(&shapes[0]).map(|r| r.fill), Some(frost_tint(FROST_TINT)));
        // half over the camera image: tint, the blur with mapped uv, the dark tint
        let pop = Rect::from_min_max(pos2(200.0, 100.0), pos2(400.0, 200.0));
        let shapes = frost_shapes(Some(&src), pop, Tok::R_POP);
        assert_eq!(shapes.len(), 3);
        let blur = rect_of(&shapes[1]);
        assert!(blur.as_ref().is_some_and(|r| r.rect == Rect::from_min_max(pos2(200.0, 100.0), pos2(400.0, 145.0))), "{blur:?}");
        let brush = blur.as_ref().and_then(|r| r.brush.clone());
        assert!(brush.as_ref().is_some_and(|b| b.fill_texture_id == tex), "textured");
        let uv = brush.map(|b| b.uv).unwrap_or(Rect::NOTHING);
        assert!(near(uv.left(), (200.0 - 174.5) / 371.0, 1e-4) && near(uv.top(), 0.105 + 100.0 / 145.0 * 0.7, 1e-4) && near(uv.bottom(), 0.805, 1e-4), "{uv:?}");
        // rounded only where the popup's own corners are (its top ones)
        let cr = blur.map(|r| r.corner_radius).unwrap_or_default();
        assert_eq!((cr.nw, cr.ne, cr.sw, cr.se), (Tok::R_POP, Tok::R_POP, 0, 0));
        assert_eq!(rect_of(&shapes[2]).map(|r| r.fill), Some(frost_tint(FROST_CAM_TINT)));
        // no blur yet, or no source: the tint alone
        assert_eq!(frost_shapes(Some(&FrostSrc { blur: None, ..src }), pop, Tok::R_POP).len(), 1);
        assert_eq!(frost_shapes(None, pop, Tok::R_POP).len(), 1);
    }

    #[test]
    fn block_average_blurs_into_one_pixel_per_block() {
        // 4 x 2 pixels, blocks of 2: left block white, right block black/red
        let mut rgba = vec![0u8; 4 * 2 * 4];
        for y in 0..2 {
            for x in 0..2 {
                rgba[(y * 4 + x) * 4..(y * 4 + x) * 4 + 3].copy_from_slice(&[255, 255, 255]);
            }
            rgba[(y * 4 + 3) * 4] = 200;
        }
        let img = block_average(&rgba, 4, 2, 2, 1);
        assert_eq!(img.size, [2, 1]);
        assert_eq!(img.pixels[0], Color32::from_rgb(255, 255, 255));
        assert_eq!(img.pixels[1], Color32::from_rgb(100, 0, 0));
        // the camera frame: 40 x 23, a short buffer gives black, no panic
        let img = block_average(&[], 1280, 720, 32, 4);
        assert_eq!(img.size, [40, 23]);
        assert!(img.pixels.iter().all(|p| *p == Color32::BLACK));
    }

    #[test]
    fn a_dropdown_opens_a_frosted_list_and_picks() {
        let mut k = Kit::new();
        let picked = Cell::new(None);
        let mut f = |ui: &mut egui::Ui| {
            if let Some(v) = dropdown(ui, "dd", 120.0, "Minor", &1u8, [(0u8, "Major".to_string()), (1, "Minor".to_string())]) {
                picked.set(Some(v));
            }
        };
        k.frame(vec![], &mut f);
        assert!(k.find("Major").is_none(), "closed");
        let at = spot(&k, "Minor");
        k.click(at, &mut f);
        k.frame(vec![], &mut f);
        assert!(k.find("Major").is_some(), "open");
        let major = spot(&k, "Major");
        k.hover_click(major, &mut f);
        assert_eq!(picked.get(), Some(0));
        k.frame(vec![], &mut f);
        assert!(k.find("Major").is_none(), "a pick closes it");
    }

    #[test]
    fn pick_row_is_22_high_and_a_disabled_row_ignores_clicks() {
        let mut k = Kit::new();
        let (rows, clicks) = (Cell::new([Rect::NOTHING; 2]), Cell::new([0u32; 2]));
        let mut f = |ui: &mut egui::Ui| {
            ui.set_width(220.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, (name, enabled)) in [("Hands", true), ("Pong", false)].into_iter().enumerate() {
                let before = ui.cursor().top();
                let r = pick_row(ui, Id::new(("pr", i)), false, enabled, name, "Built-in", "Already open", None);
                let (mut rs, mut cs) = (rows.get(), clicks.get());
                rs[i] = Rect::from_min_max(pos2(0.0, before), pos2(220.0, ui.cursor().top()));
                cs[i] += u32::from(r.clicked);
                rows.set(rs);
                clicks.set(cs);
            }
        };
        k.frame(vec![], &mut f);
        let rows = rows.get();
        assert!(near(rows[0].height(), Tok::POP_ROW_H, 0.01) && near(rows[1].height(), Tok::POP_ROW_H, 0.01), "{rows:?}");
        let texts = k.texts();
        let pong = texts.iter().find(|t| t.0 == "Pong");
        assert!(pong.is_some_and(|t| t.2 == 13.0 && t.3 == white(Tok::OFF)), "disabled at OFF: {pong:?}");
        let hands = texts.iter().find(|t| t.0 == "Hands");
        assert!(hands.is_some_and(|t| t.2 == 13.0), "{hands:?}");
        let (h, p) = (spot(&k, "Hands"), spot(&k, "Pong"));
        k.hover_click(h, &mut f);
        k.hover_click(p, &mut f);
        assert_eq!(clicks.get(), [1, 0]);
    }

    #[test]
    fn pick_row_delete_needs_two_clicks() {
        let mut k = Kit::new();
        let (picked, deleted) = (Cell::new(0u32), Cell::new(0u32));
        let mut f = |ui: &mut egui::Ui| {
            ui.set_width(240.0);
            let r = pick_row(ui, Id::new("saved"), false, true, "Live set", "", "", Some(("Delete", "Delete? Click again")));
            picked.set(picked.get() + u32::from(r.clicked));
            deleted.set(deleted.get() + u32::from(r.delete));
        };
        let counts = || (picked.get(), deleted.get());
        k.frame(vec![], &mut f);
        assert!(k.find("Delete").is_none(), "Delete shows only on hover");
        let name = spot(&k, "Live set");
        k.frame(vec![Event::PointerMoved(name)], &mut f);
        let del = spot(&k, "Delete");
        k.hover_click(del, &mut f);
        assert_eq!(counts(), (0, 0), "the first click only arms");
        assert!(k.find("Delete? Click again").is_some(), "{:?}", k.texts());
        let armed = spot(&k, "Delete? Click again");
        k.hover_click(armed, &mut f);
        assert_eq!(counts(), (0, 1), "the second click deletes");
        // leaving the row drops the arm
        k.hover_click(del, &mut f);
        k.frame(vec![Event::PointerMoved(pos2(700.0, 500.0))], &mut f);
        k.frame(vec![Event::PointerMoved(name)], &mut f);
        assert!(k.find("Delete").is_some() && k.find("Delete? Click again").is_none());
        k.hover_click(name, &mut f);
        assert_eq!(counts(), (1, 1), "the name picks");
    }

    #[test]
    fn section_head_returns_the_slot_and_closes_on_the_second_click() {
        let mut k = Kit::new();
        let (slot, closed) = (Cell::new(Rect::NOTHING), Cell::new(0u32));
        let mut f = |ui: &mut egui::Ui| {
            let (s, c) = section_head(ui, Id::new("hands"), "Hands", Some(("Close Hands", "Close Hands? 3 parts go")), true);
            slot.set(s);
            closed.set(closed.get() + u32::from(c));
        };
        k.frame(vec![], &mut f);
        let full = slot.get();
        let title = k.texts().into_iter().find(|t| t.0 == "Hands");
        assert!(title.as_ref().is_some_and(|t| t.2 == 15.0 && t.3 == white(Tok::STRONG)), "{title:?}");
        let x = spot(&k, "×");
        assert!(full.left() >= title.map_or(0.0, |t| t.1.x) && full.right() + 16.0 <= x.x - 8.0, "{full:?} {x:?}");
        assert!(near(full.height(), Tok::HEAD_H - 2.0, 0.01), "{full:?}");
        k.hover_click(x, &mut f);
        assert_eq!(closed.get(), 0, "the first click arms");
        assert!(k.find("Close Hands? 3 parts go").is_some(), "{:?}", k.texts());
        assert!(slot.get().right() < full.right(), "the arm text takes room from the slot");
        k.hover_click(x, &mut f);
        assert_eq!(closed.get(), 1, "the second click closes");
        assert!(k.find("Close Hands? 3 parts go").is_none());
        assert_eq!(slot.get(), full);
        // the arm lasts 4 s
        k.hover_click(x, &mut f);
        for _ in 0..(4.2 * 60.0) as usize {
            k.frame(vec![], &mut f);
        }
        assert!(k.find("Close Hands? 3 parts go").is_none(), "the arm ran out");
        k.hover_click(x, &mut f);
        assert_eq!(closed.get(), 1, "a click after the timeout arms again");
        // or until the pointer leaves the header
        k.frame(vec![Event::PointerMoved(pos2(400.0, 500.0))], &mut f);
        k.frame(vec![Event::PointerMoved(x)], &mut f);
        assert!(k.find("Close Hands? 3 parts go").is_none(), "leaving the header drops the arm");
        k.hover_click(x, &mut f);
        assert_eq!(closed.get(), 1);
    }

    /// A folded section's header draws no title rule: the section
    /// separator under it would read as a double line.
    #[test]
    fn a_folded_section_head_has_no_title_rule() {
        let rules = |k: &Kit| {
            k.out.as_ref().map_or(0, |o| {
                o.shapes.iter().filter(|c| matches!(&c.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == white(Tok::RULE))).count()
            })
        };
        let mut k = Kit::new();
        k.frame(vec![], &mut |ui| {
            section_head(ui, Id::new("open"), "Hands", None, true);
        });
        assert_eq!(rules(&k), 1, "an open section has its rule");
        k.frame(vec![], &mut |ui| {
            section_head(ui, Id::new("folded"), "Hands", None, false);
        });
        assert_eq!(rules(&k), 0, "a folded one none");
    }

    #[test]
    fn section_head_without_close_has_no_x() {
        let mut k = Kit::new();
        let mut slot = Rect::NOTHING;
        k.frame(vec![], &mut |ui| slot = section_head(ui, Id::new("plain"), "Pong", None, true).0);
        assert!(k.find("×").is_none());
        assert!(slot.width() > 600.0, "{slot:?}");
    }

    #[test]
    fn centered_row_never_draws_left_aligned() {
        let mut k = Kit::new();
        let mut label = "Shooting hand";
        let mut area = Rect::NOTHING;
        for frame in 0..6 {
            if frame == 3 {
                label = "Shooting hand, a much longer line";
            }
            let text = label;
            k.frame(vec![], &mut |ui| {
                area = ui.available_rect_before_wrap();
                let w = area.width();
                centered_row(ui, Id::new("cr"), w, |ui| {
                    ui.label(text);
                    quiet(ui, "Test shot");
                });
            });
            let (a, b) = (k.texts().into_iter().find(|t| t.0 == text), k.texts().into_iter().find(|t| t.0 == "Test shot"));
            assert!(a.is_some() && b.is_some(), "frame {frame}: {:?}", k.texts());
            let (Some(a), Some(b)) = (a, b) else { return };
            // the group's outer edges sit symmetric around the area's centre
            let left = a.1.x - galley_w(&k, text) / 2.0;
            let right = b.1.x + galley_w(&k, "Test shot") / 2.0 + 8.0;
            assert!(near((left + right) / 2.0, area.center().x, 1.0), "frame {frame}: {left}..{right} in {area:?}");
        }
    }

    fn galley_w(k: &Kit, text: &str) -> f32 {
        k.ctx.fonts(|f| f.layout_no_wrap(text.to_owned(), font(Tok::BODY), Color32::WHITE).size().x)
    }

    #[test]
    fn num_slot_width_ignores_the_digits() {
        let mut k = Kit::new();
        let mut ws = Vec::new();
        k.frame(vec![], &mut |ui| {
            ws.clear();
            for s in ["1 flying", "9 flying", "0 flying", "8 flying"] {
                ws.push(num_slot(ui, s, Tok::BODY, white(Tok::ROW)).rect.width());
            }
        });
        assert!(ws.windows(2).all(|w| near(w[0], w[1], 0.01)), "{ws:?}");
        // right-aligned: the narrow text ends at the slot's right edge
        let mut slot = Rect::NOTHING;
        k.frame(vec![], &mut |ui| slot = num_slot(ui, "1", Tok::BODY, white(Tok::ROW)).rect);
        let one = k.texts().into_iter().find(|t| t.0 == "1");
        assert!(one.is_some_and(|t| near(t.1.x + galley_w(&k, "1") / 2.0, slot.right(), 0.5)), "{slot:?}");
    }

    #[test]
    fn compact_controls_are_20_high() {
        let mut k = Kit::new();
        let (mut on, mut v) = (false, 100i32);
        let (mut t, mut s, mut big) = (Rect::NOTHING, Rect::NOTHING, Rect::NOTHING);
        k.frame(vec![], &mut |ui| {
            t = toggle_sm(ui, &mut on, "Harder shot plays louder", Tok::ACCENT_HI).rect;
            s = slider_field_sm(ui, Id::new("sm"), &mut v, 25..=400, 100, "Speed", 136.0, |x| x.to_string()).rect;
            big = toggle(ui, &mut on, "Live", Tok::LIVE).rect;
        });
        assert!(near(t.height(), 20.0, 0.01) && near(s.height(), 20.0, 0.01), "{t:?} {s:?}");
        assert!(near(s.width(), 136.0, 0.01) && near(big.height(), 24.0, 0.01));
        let texts = k.texts();
        for name in ["Harder shot plays louder", "Speed", "100"] {
            let f = texts.iter().find(|x| x.0 == name);
            assert!(f.is_some_and(|x| x.2 == 11.0), "{name}: {f:?}");
        }
        assert!(texts.iter().find(|x| x.0 == "Live").is_some_and(|x| x.2 == 13.0));
    }

    #[test]
    fn text_field_is_24_high() {
        let mut k = Kit::new();
        let mut text = String::new();
        let mut r = Rect::NOTHING;
        k.frame(vec![], &mut |ui| r = text_field(ui, Id::new("tf"), &mut text, 150.0, "Part name").rect);
        assert!(near(r.height(), 24.0, 0.01) && near(r.width(), 150.0, 0.01), "{r:?}");
        assert!(k.find("Part name").is_some(), "the hint shows while empty");
    }

    #[test]
    fn disclosure_toggles_and_shows_the_summary_when_closed() {
        let mut k = Kit::new();
        let mut open = false;
        let mut f = |ui: &mut egui::Ui| {
            disclosure(ui, Id::new("adv"), &mut open, "Advanced", Some("Camera on"));
        };
        k.frame(vec![], &mut f);
        assert!(k.find("Camera on").is_some());
        let p = spot(&k, "Advanced");
        k.click(p, &mut f);
        assert!(open && k.find("Camera on").is_none());
    }

    #[test]
    fn plus_button_is_hint_until_lit() {
        let mut k = Kit::new();
        for lit in [false, true] {
            let mut r = Rect::NOTHING;
            k.frame(vec![], &mut |ui| r = plus_button(ui, lit, "Add a section").rect);
            assert!(near(r.width(), 30.0, 0.01) && near(r.height(), 30.0, 0.01));
            let plus = k.texts().into_iter().find(|t| t.0 == "+");
            let want = white(if lit { Tok::VALUE } else { Tok::HINT });
            assert!(plus.as_ref().is_some_and(|t| t.3 == want), "{lit}: {plus:?}");
        }
    }

    #[test]
    fn toast_sits_bottom_left_info_fades_and_errors_take_a_click() {
        let window = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let ctx = egui::Context::default();
        style(&ctx);
        let run = |ctx: &egui::Context, t: f64, events: Vec<Event>, text: &str, error: bool, age: f32| {
            let mut hit = false;
            let raw = RawInput { events, time: Some(t), screen_rect: Some(window), ..Default::default() };
            let out = ctx.run(raw, |ctx| hit = toast(ctx, window, text, error, age));
            (out, hit)
        };
        let mut t = 0.0;
        let mut step = |events: Vec<Event>, text: &str, error: bool, age: f32| {
            t += 1.0 / 60.0;
            run(&ctx, t, events, text, error, age)
        };
        for _ in 0..3 {
            step(vec![], "Loaded Pong", false, 0.5);
        }
        let rect = ctx.memory(|m| m.area_rect(Id::new("glass-toast")));
        assert!(rect.is_some_and(|r| near(r.left(), 12.0, 0.5) && near(r.bottom(), 588.0, 0.5)), "{rect:?}");
        let (out, _) = step(vec![], "Loaded Pong", false, 3.9);
        assert!(all_texts(Some(&out)).iter().any(|t| t.0 == "Loaded Pong"));
        let (out, _) = step(vec![], "Loaded Pong", false, 4.5);
        assert!(all_texts(Some(&out)).is_empty(), "info fades after 4 s");
        // an info toast takes no click, an error toast does
        let at = pos2(30.0, 576.0);
        let press = |p| vec![Event::PointerMoved(at), Kit::button(at, p)];
        for error in [false, true] {
            for _ in 0..3 {
                step(vec![Event::PointerMoved(at)], "Not saved", error, 1.0);
            }
            step(press(true), "Not saved", error, 1.0);
            let (out, hit) = step(press(false), "Not saved", error, 1.0);
            assert_eq!(hit, error, "error {error}");
            if error {
                let (out, _) = step(vec![], "Not saved", true, 60.0);
                assert!(all_texts(Some(&out)).iter().any(|t| t.0 == "Not saved"), "errors stay");
            }
            drop(out);
        }
    }
}
