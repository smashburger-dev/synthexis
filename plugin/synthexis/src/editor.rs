//! The window of docs/design/ui-redesign.md v2 (glass language,
//! design-glass), top to bottom: the camera edge to edge with the hand
//! skeletons on it and a top bar floating over it (name, Live, key, host
//! position, the profiles 1 2 3, Settings) that fades after 2.5 s except
//! Live, the profiles and the gear; a
//! 1 px line; the top section (plugin picker, chat field, preset picker
//! and "+", and under them the plugin note, the routes strip and Save as,
//! then the chat's suggestions with Keep and Discard, which grow the
//! section and push the others down); the open feature sections (Hands,
//! Pong, Head) in the preset view's order, each with a fold chevron, a
//! closing `x` and a full-width rule; Advanced; 24 pt of bottom room. Everything under the camera scrolls and sits on the dither
//! background (backdrop.rs). The window has one fixed size (v3 2.11,
//! 720 x 520) and never asks the host for another; the camera and the
//! panel area split the real height by a share per state. Every popup
//! has frost behind it: the camera blurred where it lies over the
//! picture (glass::frosted). The
//! Settings overlay holds language, completion, the calibration status,
//! the face overlay's boxes, the 32 signals and the diagnostics. Every
//! string comes from i18n.rs in
//! the per-user language (settings.rs). Textures reset on every open
//! (probe v0.2.1 fix).

use std::any::Any;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nih_plug::prelude::*;
use nih_plug_egui::egui::{self, pos2, vec2, Align, Color32, ColorImage, Id, Key, Layout, Margin, Order, Pos2, Rangef, Rect, RichText, Sense, TextureHandle, TextureOptions, UiBuilder};
use nih_plug_egui::{create_egui_editor, widgets};
use synthexis_complete::Lang;

pub(crate) use crate::glass;

use crate::backdrop::Backdrop;
use crate::engine::camera::{median_p95, EngineShared, Preview};
use crate::i18n::{self, fill, Text};
use crate::params::SynthexisParams;
use crate::preset::top::{Ctx, Presets, View};
use crate::preset::{self, Section};
use crate::sentence::Sentence;
use crate::settings;
use crate::writer::Writer;
use glass::{white, Tok};

/// Open plugin windows over all instances; the camera and perception run
/// while one is open (engine::demand).
pub static OPEN_EDITORS: AtomicUsize = AtomicUsize::new(0);

const FADE_SECS: f32 = 0.15;
/// No camera frame for this long: the camera sends no picture.
const STALE: Duration = Duration::from_secs(2);
const LOG_LINES: usize = 40;
/// The top bar stays this long after the window opens or the pointer left it.
const BAR_SHOW: Duration = Duration::from_millis(2500);
/// The top bar's height over the camera.
const BAR_H: f32 = 30.0;
/// After the fade the Live toggle and the Settings gear keep this alpha.
const BAR_KEEP: f32 = 0.55;
/// The top bar's flat tint (section 1): rgba(30,30,30,0.62).
const BAR_TINT: f32 = 0.62;
/// The top section: 12 + row 1 (30) + 6 + row 2 (24) + 12.
const TOP_SECTION_H: f32 = 84.0;
const TOP_PAD: f32 = 12.0;
const TOP_ROW_GAP: f32 = 6.0;
/// A rule's height (two 1 px lines, 2.4).
const RULE_H: f32 = 2.0;
/// The chat's suggestions in the top section take at most this share of
/// the panel area under rows 1 and 2; taller, they scroll inside.
const INLINE_SHARE: f32 = 0.5;
/// The fold chevron's column left of a section's title (as Advanced's).
const FOLD_W: f32 = 16.0;
/// The Settings panel (v3): 62 % of the window wide, between these two;
/// at most 78 % of the window high; its padding, its rows' spacing and
/// the diagnostics log's height.
const SETTINGS_MIN_W: f32 = 320.0;
const SETTINGS_W: f32 = 460.0;
const SETTINGS_SHARE_W: f32 = 0.62;
const SETTINGS_SHARE_H: f32 = 0.78;
const SETTINGS_MARGIN: f32 = 12.0;
const SETTINGS_ROW_GAP: f32 = 4.0;
/// Extra room between two blocks of Settings (a row and its hints).
const SETTINGS_BLOCK_GAP: f32 = 6.0;
/// The disclosures' content starts under their title (chevron column).
const SETTINGS_INDENT: f32 = 16.0;
const SETTINGS_LOG_H: f32 = 120.0;
/// What a signal slider in Settings leaves its cell for the value box
/// ("0.50" with padding and the gap before it).
const SIGNAL_VALUE_W: f32 = 52.0;

// --- window size and split (spec v3 2.8) -------------------------------------

/// The pickers' width in the top grid, at least and at most.
const PICKER_MIN_W: f32 = 136.0;
const PICKER_MAX_W: f32 = 220.0;

/// The window's one fixed size (pt). FL does not honour a plug-in's
/// resize, so the window never asks; the layout fits the real size.
pub const WIN_W: u32 = 720;
pub const WIN_H: u32 = 520;
/// The share of the window's height the panel area (top section and the
/// sections under it) takes with no section open, and with one open.
const PANEL_EMPTY: f32 = 0.30;
const PANEL_OPEN: f32 = 0.72;
/// The panel area never gets less: the top section, its rule, Advanced
/// and the bottom room.
const PANEL_MIN: f32 = TOP_SECTION_H + RULE_H + Tok::HEAD_H + Tok::BOTTOM_ROOM;
/// The camera never gets less while the window has room for it: the
/// pickers above the top section open into it.
const CAM_FLOOR: f32 = 120.0;

/// The full camera height at `width`: the 16:9 frame at full width.
fn cam_full(width: f32) -> f32 {
    (width * 9.0 / 16.0).floor()
}

/// Whether the panel area takes `PANEL_OPEN` (pure): a section shows its
/// body (open and not folded) or the chat's suggestions show (`inline`).
/// A folded section is only its header, so with every open section
/// folded, or none open, the camera gets its big layout back at once.
fn panel_open(view: &View, inline: bool) -> bool {
    inline || view.sections.iter().any(|sec| !view.is_folded(*sec))
}

/// The camera's height in a window `width` x `h` (pure): the panel area
/// takes its share (`PANEL_OPEN` with a section open, else
/// `PANEL_EMPTY`, at least `PANEL_MIN`), the camera what is left, at
/// least `CAM_FLOOR` when the window has room for both, at most the full
/// frame. Content inside the sections never moves it; it scrolls.
#[cfg(test)]
fn cam_height(width: f32, h: f32, open: bool) -> f32 {
    cam_height_with(width, h, open, 0)
}

/// What a folded section takes in the panel area: its header, the room
/// under it and its rule (panel_area).
const FOLDED_H: f32 = Tok::HEAD_H + Tok::ROW_GAP + (Tok::SEC_BOTTOM - Tok::ROW_GAP) + RULE_H;

/// `cam_height` with `folded` sections showing only their headers: the
/// panel area keeps room for them too, so Advanced stays in view.
fn cam_height_with(width: f32, h: f32, open: bool, folded: usize) -> f32 {
    let share = if open { PANEL_OPEN } else { PANEL_EMPTY };
    let min = PANEL_MIN + folded as f32 * FOLDED_H;
    let panel = (h * share).max(min);
    let floor = CAM_FLOOR.min(h - min).max(0.0);
    (h - panel).floor().max(floor).min(cam_full(width)).max(0.0)
}

/// The top section's row-1 columns at `width` (2.5, v3), y from 0:
/// plugin, chat, preset, plus. The pickers take 19 % of the width (136 to
/// 220), so the chat field keeps 372 of the default 720.
fn top_columns(width: f32) -> [Rect; 4] {
    let c = width - 2.0 * Tok::INSET;
    let p = (width * 0.19).round().clamp(PICKER_MIN_W, PICKER_MAX_W);
    let chat = (c - 2.0 * p - 2.0 * Tok::GAP - (Tok::TOP_H + 4.0)).max(0.0);
    let plugin_x = Tok::INSET;
    let chat_x = plugin_x + p + Tok::GAP;
    let preset_x = chat_x + chat + Tok::GAP;
    let plus_x = preset_x + p + 4.0;
    let r = |x: f32, w: f32| Rect::from_min_size(pos2(x, 0.0), vec2(w, Tok::TOP_H));
    [r(plugin_x, p), r(chat_x, chat), r(preset_x, p), r(plus_x, Tok::TOP_H)]
}

/// The host side of the window on macOS: the size of the NSView the host
/// gave the editor.
#[allow(unsafe_code)]
mod mac {
    use nih_plug_egui::egui::{vec2, Vec2};
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_foundation::NSRect;

    /// The size in points of `view` (an `NSView *`, 0 when unknown).
    /// Runs on the main thread inside the editor's frame.
    pub fn view_size(view: usize) -> Option<Vec2> {
        if view == 0 {
            return None;
        }
        let view = view as *mut AnyObject;
        // SAFETY: `view` is the NSView the host passed to `Editor::spawn`;
        // EditorHandle clears it before the editor closes, and this runs on
        // the main thread inside the editor's frame callback. `frame` is a
        // plain getter.
        let frame: NSRect = unsafe { msg_send![view, frame] };
        Some(vec2(frame.size.width as f32, frame.size.height as f32))
    }
}

// --- camera preview (section 3) ----------------------------------------------

/// True when `now` is a frame the texture does not show yet.
fn needs_upload(last: Option<&Arc<Preview>>, now: &Arc<Preview>) -> bool {
    last.is_none_or(|l| !Arc::ptr_eq(l, now))
}

/// The camera texture and the frame it shows.
#[derive(Default)]
struct Cam {
    tex: Option<TextureHandle>,
    last: Option<Arc<Preview>>,
    /// Copy time into the ColorImage (ms) since the last log line.
    copy_ms: f64,
    copies: u32,
    /// The shown frame block-averaged for the frost behind popups (v3 2.7).
    blur: Option<TextureHandle>,
    /// The frame `blur` holds.
    blurred: Option<Arc<Preview>>,
}

/// The frost's blur: one pixel per 32 x 32 block of the 1280x720 frame
/// (40 x 23), every 4th pixel sampled; linear filtering smooths it.
const FROST_BLOCK: usize = 32;
const FROST_STEP: usize = 4;

/// Uploads between two log lines of the copy time.
const COPY_LOG_EVERY: u32 = 300;

impl Cam {
    /// Converts and uploads `frame` when it is new: `set` on the first
    /// upload or a size change, `set_partial` after. True when it uploaded.
    fn upload(&mut self, ctx: &egui::Context, frame: Option<Arc<Preview>>) -> bool {
        let Some(frame) = frame else { return false };
        if !needs_upload(self.last.as_ref(), &frame) {
            return false;
        }
        let size = [frame.w, frame.h];
        if frame.w == 0 || frame.h == 0 || frame.rgba.len() != frame.w * frame.h * 4 {
            return false;
        }
        let started = Instant::now();
        let img = ColorImage::from_rgba_unmultiplied(size, &frame.rgba);
        self.copy_ms += started.elapsed().as_secs_f64() * 1000.0;
        self.copies += 1;
        if self.copies >= COPY_LOG_EVERY {
            crate::log::log(&format!("preview {}x{}: copy into the texture image {:.2} ms per frame", frame.w, frame.h, self.copy_ms / f64::from(self.copies)));
            (self.copy_ms, self.copies) = (0.0, 0);
        }
        match &mut self.tex {
            Some(tex) if tex.size() == size => tex.set_partial([0, 0], img, TextureOptions::LINEAR),
            Some(tex) => tex.set(img, TextureOptions::LINEAR),
            None => self.tex = Some(ctx.load_texture("cam", img, TextureOptions::LINEAR)),
        }
        self.last = Some(frame);
        true
    }

    /// The blur texture's id, made (black) on first use so a popup can
    /// point at it in the frame it opens; `refresh_blur` fills it.
    fn blur_id(&mut self, ctx: &egui::Context) -> egui::TextureId {
        self.blur.get_or_insert_with(|| ctx.load_texture("cam-frost", ColorImage::new([1, 1], Color32::BLACK), TextureOptions::LINEAR)).id()
    }

    /// Blurs the shown frame into the blur texture when it holds an older
    /// one (called while a popup is open; the texture update lands before
    /// this frame paints).
    fn refresh_blur(&mut self, ctx: &egui::Context) {
        let Some(frame) = self.last.clone() else { return };
        if !needs_upload(self.blurred.as_ref(), &frame) {
            return;
        }
        let img = glass::block_average(&frame.rgba, frame.w, frame.h, FROST_BLOCK, FROST_STEP);
        match &mut self.blur {
            Some(tex) => tex.set(img, TextureOptions::LINEAR),
            None => self.blur = Some(ctx.load_texture("cam-frost", img, TextureOptions::LINEAR)),
        }
        self.blurred = Some(frame);
    }

    /// A reopened window has a new context: the textures go, and the
    /// current frame uploads at once even while the camera stalls.
    fn clear(&mut self) {
        self.tex = None;
        self.last = None;
        self.blur = None;
        self.blurred = None;
    }
}

/// The frame's aspect (1280x720).
const ASPECT: f32 = 16.0 / 9.0;
/// Below this share of the frame's height a wide, low camera strip shows
/// wings instead of cutting more off the top and bottom.
const V_MIN: f32 = 0.70;
/// Of the rows cut off, this share comes off the top: raised hands stay.
const TOP_SHARE: f32 = 0.35;

/// Where the frame goes in the camera rect: `img` its rect, `uv` the part
/// of the preview shown, `wings` when the dither shows beside it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Crop {
    img: Rect,
    uv: Rect,
    wings: bool,
}

/// The cover crop of the preview over `cam`; past V_MIN the image keeps
/// 70 % of the frame's height and narrows, the dither wings beside it.
fn crop(cam: Rect) -> Crop {
    let want = cam.width() / cam.height().max(1.0);
    if want <= ASPECT {
        let w = want / ASPECT;
        return Crop { img: cam, uv: Rect::from_min_max(Pos2::new((1.0 - w) / 2.0, 0.0), Pos2::new((1.0 + w) / 2.0, 1.0)), wings: false };
    }
    let v = ASPECT / want;
    let (v, img, wings) = if v >= V_MIN {
        (v, cam, false)
    } else {
        let w = cam.height() * ASPECT / V_MIN;
        (V_MIN, Rect::from_center_size(cam.center(), egui::vec2(w, cam.height())), true)
    };
    let v0 = (1.0 - v) * TOP_SHARE;
    Crop { img, uv: Rect::from_min_max(Pos2::new(0.0, v0), Pos2::new(1.0, v0 + v)), wings }
}

/// Pong's fit: the whole frame (the arena) in the largest 16:9 rect
/// centred in `cam`, the dither wings beside it.
fn contain(cam: Rect) -> Crop {
    let w = cam.width().min(cam.height() * ASPECT);
    let img = Rect::from_center_size(cam.center(), egui::vec2(w, w / ASPECT));
    let wings = img.width() < cam.width() - 0.5 || img.height() < cam.height() - 0.5;
    Crop { img, uv: Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), wings }
}

/// How the frame fits the camera rect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fit {
    /// `crop`: the big camera, cut to cover the rect.
    Cover,
    /// `contain`: the whole 16:9 frame, the dither wings beside it.
    Contain,
}

impl Fit {
    /// The frame's place in `cam` under this fit.
    fn place(self, cam: Rect) -> Crop {
        match self {
            Fit::Cover => crop(cam),
            Fit::Contain => contain(cam),
        }
    }
}

/// The fit for `view` (pure): the whole frame while a section shows its
/// body (Hands, Head or Pong open and not folded) or Pong plays, so Hands
/// and Head look like Pong without its walls; the cover crop with no
/// section open or every open one folded. The chat's suggestions alone
/// keep the cover crop.
fn fit_for(view: &View, pong_on: bool) -> Fit {
    if pong_on || panel_open(view, false) {
        Fit::Contain
    } else {
        Fit::Cover
    }
}

/// A point of the preview (0..1) in window points.
fn map_joint(img: Rect, uv: Rect, p: [f32; 2]) -> Pos2 {
    img.min + ((Pos2::new(p[0], p[1]) - uv.min) / uv.size()) * img.size()
}

/// The skeletons perception saw last, in each hand's colour; a finger
/// whose part plays now gets a white tip.
fn skeletons(painter: &egui::Painter, img: Rect, uv: Rect, lit: &[(String, crate::route::Trigger)]) {
    const BONES: [(usize, usize); 20] = [
        (0, 1), (1, 2), (2, 3), (3, 4), (0, 5), (5, 6), (6, 7), (7, 8), (5, 9), (9, 10),
        (10, 11), (11, 12), (9, 13), (13, 14), (14, 15), (15, 16), (13, 17), (0, 17), (17, 18), (18, 19),
    ];
    let now = crate::midi::now_ns();
    let hands = *crate::engine::SKELETON.lock();
    for (side, h) in hands.iter().enumerate() {
        let Some((joints, at)) = h else { continue };
        if now.saturating_sub(*at) > 400_000_000 {
            continue;
        }
        let to = |p: [f32; 2]| map_joint(img, uv, p);
        let col = crate::fingers::HAND[side];
        let stroke = egui::Stroke::new(1.5_f32, col.gamma_multiply(0.85));
        for (a, b) in BONES.iter().chain(std::iter::once(&(19, 20))) {
            painter.line_segment([to(joints[*a]), to(joints[*b])], stroke);
        }
        let signal = crate::fingers::Slot::finger(side, 0).signal();
        for f in 0..5 {
            let tip = to(joints[4 + 4 * f]);
            let on = lit.iter().any(|(s, t)| {
                *s == signal && matches!(t, crate::route::Trigger::Shape { mask, care } if care & crate::route::finger_bit(f) != 0 && (*care != 31 || mask & crate::route::finger_bit(f) != 0))
            });
            painter.circle_filled(tip, if on { 5.0 } else { 2.5 }, if on { Color32::WHITE } else { col });
        }
    }
}

// --- the editor --------------------------------------------------------------

struct EditorHandle {
    host: Arc<AtomicUsize>,
    _inner: Box<dyn Any + Send>,
}

impl Drop for EditorHandle {
    fn drop(&mut self) {
        // before the inner editor (and the host's view) goes
        self.host.store(0, Ordering::Release);
        OPEN_EDITORS.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Wrap {
    inner: Box<dyn Editor>,
    /// The host's NSView while the editor is open, else 0.
    host: Arc<AtomicUsize>,
}

impl Editor for Wrap {
    fn spawn(&self, p: ParentWindowHandle, c: Arc<dyn GuiContext>) -> Box<dyn Any + Send> {
        OPEN_EDITORS.fetch_add(1, Ordering::AcqRel);
        let view = match p {
            ParentWindowHandle::AppKitNsView(v) => v as usize,
            _ => 0,
        };
        let inner = self.inner.spawn(p, c);
        self.host.store(view, Ordering::Release);
        Box::new(EditorHandle { host: self.host.clone(), _inner: inner })
    }
    fn size(&self) -> (u32, u32) { self.inner.size() }
    fn set_scale_factor(&self, f: f32) -> bool { self.inner.set_scale_factor(f) }
    fn param_value_changed(&self, id: &str, v: f32) { self.inner.param_value_changed(id, v) }
    fn param_modulation_changed(&self, id: &str, v: f32) { self.inner.param_modulation_changed(id, v) }
    fn param_values_changed(&self) { self.inner.param_values_changed() }
}

fn set_bool(setter: &ParamSetter, param: &BoolParam, v: bool) {
    setter.begin_set_parameter(param);
    setter.set_parameter(param, v);
    setter.end_set_parameter(param);
}

/// The camera's state in words, and whether it is a problem.
fn camera_status(t: &Text, shared: &EngineShared) -> (String, bool) {
    let auth = shared.auth_status.lock().clone();
    let last = *shared.last_cb.lock();
    match auth.as_str() {
        "denied" | "restricted" => (t.cam_denied.to_string(), true),
        "notDetermined" => (t.cam_asking.to_string(), false),
        "authorized" => match last {
            Some(at) if at.elapsed() < STALE => {
                let (iv, _) = median_p95(&shared.intervals_ms.lock());
                (fill(t.cam_on, &[&format!("{:.0}", 1000.0 / iv.max(1.0))]), false)
            }
            Some(_) => (t.cam_no_picture.to_string(), true),
            None => (t.cam_starting.to_string(), false),
        },
        _ => (t.cam_starting.to_string(), false),
    }
}

/// The numbers behind the camera status, for Settings > Diagnostics.
fn diagnostics(t: &Text, shared: &EngineShared) -> Vec<String> {
    let (iv, _) = median_p95(&shared.intervals_ms.lock());
    let (d_med, d_p95) = median_p95(&shared.pts_to_cb_ms.lock());
    let fps = if iv > 0.0 { format!("{:.1}", 1000.0 / iv) } else { "0".into() };
    vec![
        fill(t.diag_access, &[&*shared.auth_status.lock()]),
        fill(
            t.diag_camera,
            &[&fps, &shared.frame_count.load(Ordering::Relaxed), &shared.drop_count.load(Ordering::Relaxed)],
        ),
        fill(t.diag_delay, &[&format!("{d_med:.0}"), &format!("{d_p95:.0}")]),
    ]
}

/// Which of Settings' project disclosures are open (closed on every new
/// instance).
#[derive(Clone, Copy, Debug, Default)]
struct SetFolds {
    signals: bool,
    diag: bool,
    face: bool,
}

/// What Settings needs besides the per-user store.
struct SetIn<'a> {
    params: &'a SynthexisParams,
    setter: &'a ParamSetter<'a>,
    writer: &'a Writer,
    diag: &'a [String],
}

/// The Settings panel's size in a `win` window (pure, v3): its width and
/// the most height it takes before it scrolls. Never closer than 16 to
/// the window's sides or 24 to its top and bottom.
fn settings_size(win: egui::Vec2) -> egui::Vec2 {
    let w = (win.x * SETTINGS_SHARE_W).clamp(SETTINGS_MIN_W, SETTINGS_W).min(win.x - 32.0).max(0.0);
    let h = (win.y * SETTINGS_SHARE_H).min(win.y - 48.0).max(80.0);
    vec2(w, h)
}

/// The Settings overlay (R8): a dimmed layer over the whole window with a
/// panel in the middle (`settings_size`) that scrolls when taller.
/// Returns false once the user closed it (Esc, the `x`, a click outside
/// the panel).
fn settings_overlay(ctx: &egui::Context, store: &settings::Store, t: &Text, inp: &SetIn, folds: &mut SetFolds) -> bool {
    let id = Id::new("synthexis-settings");
    let fade = ctx.animate_bool_with_time(id.with("fade"), true, FADE_SECS);
    let screen = ctx.screen_rect();
    let mut keep = !ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
    egui::Area::new(id).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        ui.multiply_opacity(fade);
        let bg = Tok::WINDOW;
        ui.painter().rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), 200));
        // takes every click outside the panel
        let scrim = ui.interact(screen, id.with("scrim"), Sense::click());
        let egui::Vec2 { x: width, y: max_h } = settings_size(screen.size());
        // centred by last frame's height; the room below lets it grow
        let last_h = ui.data(|d| d.get_temp::<f32>(id.with("h"))).unwrap_or(max_h).min(max_h);
        let top = (screen.center().y - last_h / 2.0).max(screen.top() + 24.0);
        let room = (screen.bottom() - 24.0 - top).min(max_h).max(80.0);
        let panel = Rect::from_min_size(pos2(screen.center().x - width / 2.0, top), vec2(width, room));
        let frame = egui::Frame::NONE
            .fill(glass::pop_fill())
            .stroke(egui::Stroke::new(0.7_f32, white(Tok::EDGE)))
            .corner_radius(egui::CornerRadius::same(Tok::R_WINDOW))
            .inner_margin(Margin::same(SETTINGS_MARGIN as i8));
        let inner_w = width - 2.0 * SETTINGS_MARGIN;
        let mut drawn = panel;
        ui.scope_builder(UiBuilder::new().max_rect(panel), |ui| {
            let out = frame.show(ui, |ui| {
                ui.set_width(inner_w);
                egui::ScrollArea::vertical()
                    .id_salt(id.with("scroll"))
                    .max_height(room - 2.0 * SETTINGS_MARGIN)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_width(inner_w);
                        if settings_body(ui, store, t, inp, folds, inner_w) {
                            keep = false;
                        }
                    });
            });
            drawn = out.response.rect;
        });
        if (drawn.height() - last_h).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(id.with("h"), drawn.height()));
            ui.ctx().request_repaint();
        }
        let outside = ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| !drawn.contains(p));
        if scrim.clicked() && outside {
            keep = false;
        }
    });
    keep
}

/// Settings' rows, top to bottom (R8): the title and its `x`; the account
/// settings as a form (labels 13 at LABEL in one column, controls beside
/// them, hints 11 at HINT under the control), the Face overlay disclosure
/// and the account note; then what is saved with the project (Signals,
/// Diagnostics). Every control row is `CTRL_H` high; a hint sits
/// `SETTINGS_ROW_GAP` under its control, a block `SETTINGS_BLOCK_GAP`
/// further. True when the `x` was clicked.
fn settings_body(ui: &mut egui::Ui, store: &settings::Store, t: &Text, inp: &SetIn, folds: &mut SetFolds, width: f32) -> bool {
    let mut close = false;
    ui.spacing_mut().item_spacing.y = SETTINGS_ROW_GAP;
    ui.horizontal(|ui| {
        ui.set_min_height(Tok::CTRL_H);
        ui.label(RichText::new(t.settings_title).size(Tok::TITLE).color(white(Tok::STRONG)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if glass::quiet(ui, "×").on_hover_text(t.close).clicked() {
                close = true;
            }
        });
    });
    set_sep(ui);
    let mut s = store.get();
    let before = s;
    // the label column: the widest label and a gap
    let label_w = [t.language, t.set_typing, t.set_calibration].iter().map(|l| glass::galley(ui, l, Tok::BODY).size().x).fold(0.0_f32, f32::max) + 2.0 * Tok::GAP;
    set_row(ui, label_w, t.language, |ui| {
        glass::segmented(ui, &mut s.lang, &[(Lang::En, "English"), (Lang::De, "Deutsch")]);
    });
    set_hint(ui, label_w, t.language_note);
    ui.add_space(SETTINGS_BLOCK_GAP);
    set_row(ui, label_w, t.set_typing, |ui| {
        glass::toggle(ui, &mut s.completion, t.completion, Tok::ACCENT_HI);
    });
    set_hint(ui, label_w, t.completion_note);
    set_hint(ui, label_w, t.keys_note);
    ui.add_space(SETTINGS_BLOCK_GAP);
    set_row(ui, label_w, t.set_calibration, |ui| {
        let status = if s.fingers.is_some() { t.set_calibrated } else { t.set_not_calibrated };
        ui.label(RichText::new(status).size(Tok::BODY).color(white(Tok::VALUE)));
    });
    ui.add_space(SETTINGS_BLOCK_GAP);
    glass::disclosure_sm(ui, Id::new("settings-face"), &mut folds.face, t.face_overlay);
    if folds.face {
        set_indented(ui, SETTINGS_INDENT, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = Tok::GAP;
                let col = crate::head_ui::HEAD_COL;
                glass::toggle(ui, &mut s.face.head, t.h_head, col);
                glass::toggle(ui, &mut s.face.eyes, t.fo_eyes, col);
                glass::toggle(ui, &mut s.face.mouth, t.h_mouth, col);
                ui.add_space(Tok::GAP);
                glass::toggle(ui, &mut s.face.values, t.fo_values, col);
            });
            set_hint(ui, 0.0, t.fo_note);
        });
    }
    ui.add_space(SETTINGS_BLOCK_GAP);
    // the account note sits directly under the account settings
    set_hint(ui, 0.0, t.scope_note);
    if s != before {
        store.update(|x| *x = s);
    }
    set_sep(ui);
    ui.label(RichText::new(t.set_project).size(Tok::BODY).color(white(Tok::LABEL)));
    glass::disclosure_sm(ui, Id::new("settings-signals"), &mut folds.signals, t.signals);
    if folds.signals {
        set_indented(ui, SETTINGS_INDENT, |ui| {
            // four cells a row; the slider leaves its cell room for the value
            let cell = ((width - SETTINGS_INDENT - 3.0 * Tok::GAP) / 4.0).floor();
            let w = (cell - SIGNAL_VALUE_W).max(24.0);
            for i in 0..8 {
                ui.horizontal(|ui| {
                    for c in 0..4 {
                        let param = inp.params.signals()[i * 4 + c];
                        ui.allocate_ui_with_layout(vec2(cell, Tok::CTRL_H), Layout::left_to_right(Align::Center), |ui| {
                            ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                            ui.add(widgets::ParamSlider::for_param(param, inp.setter).with_width(w));
                        });
                    }
                });
            }
        });
    }
    glass::disclosure_sm(ui, Id::new("settings-diag"), &mut folds.diag, t.diagnostics);
    if folds.diag {
        set_indented(ui, SETTINGS_INDENT, |ui| {
            for line in inp.diag {
                ui.label(RichText::new(line).size(Tok::BODY).color(white(Tok::ROW)));
            }
            let can_shoot = inp.writer.pong.setup().on && inp.params.live.value();
            if glass::dim_button(ui, t.p_burst, can_shoot, t.p_test_off).clicked() {
                inp.writer.pong.test_shots(16);
            }
            ui.label(RichText::new(t.diag_log).size(Tok::BODY).color(white(Tok::LABEL)));
            egui::ScrollArea::vertical()
                .id_salt("settings-log")
                .max_height(SETTINGS_LOG_H)
                .auto_shrink([false, true])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in crate::log::last_lines(LOG_LINES) {
                        ui.label(RichText::new(line).monospace().size(Tok::SMALL).color(white(Tok::ROW)));
                    }
                });
        });
    }
    close
}

/// A soft separator with `SETTINGS_BLOCK_GAP` of room above and below.
fn set_sep(ui: &mut egui::Ui) {
    ui.add_space(SETTINGS_BLOCK_GAP);
    glass::sep_soft(ui);
    ui.add_space(SETTINGS_BLOCK_GAP);
}

/// One form row of Settings, `CTRL_H` high: `label` (13, LABEL) in the
/// `label_w` column, the controls `add` lays out beside it.
fn set_row(ui: &mut egui::Ui, label_w: f32, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        // the controls start at `label_w`: the cell and the gap after it
        ui.spacing_mut().item_spacing.x = Tok::GAP;
        let (r, _) = ui.allocate_exact_size(vec2(label_w - Tok::GAP, Tok::CTRL_H), Sense::hover());
        let g = glass::galley(ui, label, Tok::BODY);
        glass::text_at(ui.painter(), pos2(r.left(), r.center().y - g.size().y / 2.0), g, white(Tok::LABEL));
        add(ui);
    });
}

/// A hint (11, HINT) from `x` to the right edge, wrapped.
fn set_hint(ui: &mut egui::Ui, x: f32, text: &str) {
    set_indented(ui, x, |ui| {
        ui.add(egui::Label::new(RichText::new(text).size(Tok::SMALL).color(white(Tok::HINT))).wrap());
    });
}

/// `add` in a column `x` in from the left, as wide as the rest.
fn set_indented(ui: &mut egui::Ui, x: f32, add: impl FnOnce(&mut egui::Ui)) {
    let w = (ui.available_width() - x).max(0.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(x);
        ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(w);
            ui.spacing_mut().item_spacing = vec2(Tok::GAP, SETTINGS_ROW_GAP);
            add(ui);
        });
    });
}

/// What the top bar shows.
struct BarInfo {
    live: bool,
    /// The profiles 1 2 3: the active one, which hold a setup.
    profiles: preset::profile::Summary,
    key: String,
    playing: bool,
    tempo: f64,
    bar: i64,
    beat: i64,
}

/// What a click in the top bar asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct BarOut {
    live: Option<bool>,
    settings: bool,
    /// A profile to switch to (0-based).
    profile: Option<usize>,
}

/// The top bar over the camera's top 30 pt: name, Live, key, host
/// position, the profiles 1 2 3 and Settings, on a flat tint. It fades out
/// 2.5 s after the window opened or the pointer left it and comes back
/// when the pointer nears the top; Live, the profiles and the gear stay at
/// 0.55. Numbers sit in fixed-width slots.
fn top_bar(ui: &mut egui::Ui, cam: Rect, opened: &mut Instant, settings_open: bool, info: &BarInfo, t: &Text) -> BarOut {
    let bar = Rect::from_min_size(cam.min, vec2(cam.width(), BAR_H));
    // under an open popup the bar would show through its glass
    if glass::popup_cover(ui.ctx()).intersects(bar) {
        return BarOut::default();
    }
    let near = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| p.y < cam.top() + 48.0 && cam.x_range().contains(p.x));
    if near {
        *opened = Instant::now();
    }
    let show = near || opened.elapsed() < BAR_SHOW || settings_open;
    let op = ui.ctx().animate_bool_with_time(Id::new("top-bar"), show, 0.25);
    if show {
        ui.ctx().request_repaint_after(BAR_SHOW);
    }
    let keep = op.max(BAR_KEEP);
    let painter = ui.painter().with_clip_rect(bar);
    if op > 0.01 {
        painter.rect_filled(bar, 0.0, Color32::from_rgba_unmultiplied(30, 30, 30, (BAR_TINT * 255.0 * op) as u8));
        painter.hline(bar.x_range(), painter.round_to_pixel_center(bar.bottom() - 0.5), egui::Stroke::new(1.0_f32, white(Tok::EDGE * op)));
    }
    let mut out = BarOut::default();
    ui.scope_builder(UiBuilder::new().max_rect(bar.shrink2(vec2(Tok::INSET, 3.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = Tok::GAP;
        ui.scope(|ui| {
            ui.multiply_opacity(op);
            ui.label(RichText::new("synthexis").size(Tok::HEAD).color(white(Tok::STRONG)));
        });
        ui.scope(|ui| {
            ui.multiply_opacity(keep);
            let mut live = info.live;
            if glass::toggle(ui, &mut live, t.live, Tok::LIVE).changed() {
                out.live = Some(live);
            }
        });
        ui.scope(|ui| {
            ui.multiply_opacity(op);
            ui.label(RichText::new(&info.key).size(Tok::BODY).color(white(Tok::ROW)));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let c = white(Tok::ROW);
                let text = |ui: &mut egui::Ui, s: &str| {
                    let g = glass::galley(ui, s, Tok::BODY);
                    let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                    glass::text_at(ui.painter(), r.min, g, c);
                };
                text(ui, if info.playing { "▶ " } else { "■ " });
                glass::num_slot(ui, &format!("{:.0}", info.tempo), Tok::BODY, c);
                text(ui, " BPM · ");
                glass::num_slot(ui, &info.bar.to_string(), Tok::BODY, c);
                text(ui, ".");
                glass::num_slot(ui, &info.beat.to_string(), Tok::BODY, c);
            });
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.multiply_opacity(keep);
            if glass::quiet(ui, t.settings).clicked() {
                out.settings = true;
                if let Some(f) = ui.memory(|m| m.focused()) {
                    ui.memory_mut(|m| m.surrender_focus(f));
                }
            }
            let p = info.profiles;
            let mut pick = p.active;
            if glass::segmented_narrow(ui, &mut pick, &PROFILE_OPTS, &profile_tips(p, t)).changed() && pick != p.active {
                out.profile = Some(pick);
            }
        });
    });
    out
}

/// The profiles' segments: 0-based index and number.
const PROFILE_OPTS: [(usize, &str); preset::profile::COUNT] = [(0, "1"), (1, "2"), (2, "3")];

/// Each profile's hover text: the active one, one holding a setup, or an
/// empty one (it starts fresh).
fn profile_tips(p: preset::profile::Summary, t: &Text) -> Vec<String> {
    let now = p.active + 1;
    (0..preset::profile::COUNT)
        .map(|i| {
            let n = i + 1;
            match (i == p.active, p.filled.get(i).copied().unwrap_or(false)) {
                (true, _) => fill(t.profile_now, &[&n]),
                (false, true) => fill(t.profile_go, &[&n, &now]),
                (false, false) => fill(t.profile_go_empty, &[&n, &now]),
            }
        })
        .collect()
}

/// Grows the open list (R5.7) by `want` and unfolds those sections: Hands
/// after a reply about the hands (R7), Head after one about the face or
/// head, Pong after a wall pick or a wall sentence. True when it changed.
fn grow_view(view: &mut View, want: &[Section]) -> bool {
    let mut changed = false;
    for sec in want {
        if !view.sections.contains(sec) {
            view.sections.push(*sec);
            changed = true;
        }
        if view.is_folded(*sec) {
            view.set_folded(*sec, false);
            changed = true;
        }
    }
    changed
}

struct UiState {
    params: Arc<SynthexisParams>,
    shared: Arc<EngineShared>,
    writer: Arc<Writer>,
    cam: Cam,
    backdrop: Backdrop,
    sentence: Sentence,
    settings_open: bool,
    set_folds: SetFolds,
    fingers: crate::fingers::Fingers,
    pong: crate::pong_ui::PongUi,
    head: crate::head_ui::HeadUi,
    presets: Presets,
    /// The open sections and the picked preset (params `view`).
    view: View,
    /// The profiles as the top bar shows them (params `profiles`).
    profiles: preset::profile::Summary,
    /// The writer's `state_gen` the view was read at.
    view_gen: Option<u64>,
    /// Advanced is open.
    advanced: bool,
    opened: Instant,
    /// The host's NSView (Wrap).
    host: Arc<AtomicUsize>,
    /// `SYNTHEXIS_UI_SHOT`: the mockup state the first frame sets up.
    shot: Option<String>,
    /// `SYNTHEXIS_UI_SCROLL`: the panel area's scroll offset in the
    /// captures, negative for the bottom.
    shot_scroll: Option<f32>,
    /// The wall shot y picks a few frames in.
    shot_wall: Option<crate::pong::arena::Side>,
    /// The shot whose chat routes go in a few frames in, after
    /// initialize() bumped the state (a bump drops pending lines).
    shot_chat: Option<String>,
    /// A fixed face for the tracking boxes (shot o).
    shot_face: Option<(crate::engine::geom::FaceGeom, crate::head_ui::Pose)>,
}

impl UiState {
    /// Reads `view` again when the writer's state changed (a project load,
    /// initialize, an apply) and grows it by what the state plays (R5.10).
    fn sync_view(&mut self) {
        let g = self.writer.state_gen();
        if self.view_gen != Some(g) {
            self.view_gen = Some(g);
            self.view = View::read(&self.params).with_content(&preset::capture(&self.writer));
            self.profiles = preset::profile::Profiles::read(&self.params).summary();
        }
    }

    /// Verification hook for the window captures (ui-redesign.md, Unit 7;
    /// v3): `SYNTHEXIS_UI_SHOT` = a (no preset), b (Hands), c (Pong), f
    /// (Head), g (Head with the mouth's routes), d (Hands and Pong with the
    /// "+" menu open), e (Settings open), p (Hands, a big FL project and the
    /// plugin picker open), q (the same with no section), r (Hands with the
    /// preset picker open), z (every section open with a finger and the
    /// mouth picked, Advanced open) or y (the same with the top wall
    /// picked instead of the finger), t (z with the finger part's template
    /// picker open), s (Settings with Signals and Diagnostics open), k
    /// (Hands folded), m (Head with a kept mouth route), n (Head with
    /// three head routes and a pending right eye one), o (m over the
    /// stand-in camera with the face's tracking boxes), u (i in the Routes
    /// view with the first target box's search open), v (Pong with three
    /// stand-in projectiles and their trails), w (Pong with the left wall's
    /// Parameter search open), x (Pong with the bottom wall's chord editor),
    /// twin (Hands with two Serum 2 instances on tracks 3 and 10, the left
    /// thumb's tilt on one's Filter 1 Freq, the right hand's left-right on
    /// the other's, the right hand picked) or twin-routes (the same in the
    /// Routes view) or the chat routes of h, i, j and l (`shot_routes`),
    /// once. b, c, f and u to x show the stand-in camera picture. `SYNTHEXIS_UI_SCROLL` scrolls the panel area to that many
    /// points ("end": the bottom), `SYNTHEXIS_UI_SCROLL_X` the plugin
    /// picker's columns.
    fn apply_shot(&mut self, ctx: &egui::Context) {
        let Some(shot) = self.shot.take() else { return };
        if shot == "profiles" {
            return self.shot_profiles();
        }
        let (sections, name) = match shot.as_str() {
            "b" | "p" | "r" => (vec![Section::Hands], Some("Hands")),
            "c" | "v" | "w" | "x" => (vec![Section::Pong], Some("Pong")),
            "d" => (vec![Section::Hands, Section::Pong], Some("Hands")),
            "f" | "g" | "m" | "n" | "o" => (vec![Section::Head], Some("Head")),
            "i" | "j" | "k" | "u" | "twin" | "twin-routes" => (vec![Section::Hands], Some("Hands")),
            "y" | "z" | "t" => (vec![Section::Hands, Section::Pong, Section::Head], Some("Hands")),
            _ => (Vec::new(), None),
        };
        if sections.contains(&Section::Pong) {
            let snap = preset::with_section(preset::capture(&self.writer), Section::Pong);
            if let Err(e) = preset::apply(&self.writer, &snap) {
                crate::log::log(&format!("ui shot {shot}: Pong not loaded ({e})"));
            }
        }
        self.view = View { sections, preset: name.map(str::to_string), ..Default::default() };
        if shot == "k" {
            self.view.set_folded(Section::Hands, true);
        }
        self.view.write(&self.params);
        self.shot_chat = Some(shot.clone());
        let popup = match shot.as_str() {
            "d" => Some("preset-plus"),
            "p" | "q" => Some("synthexis-picker"),
            "r" => Some("preset-picker"),
            _ => None,
        };
        if let Some(p) = popup {
            ctx.memory_mut(|m| m.open_popup(Id::new(p).with("popup")));
        }
        if matches!(shot.as_str(), "b" | "c" | "d" | "f" | "p" | "q" | "r" | "o" | "u" | "v" | "w" | "x" | "twin" | "twin-routes") {
            // a stand-in camera picture so the frost shows its blur
            *self.shared.preview.lock() = Some(Arc::new(shot_frame()));
        }
        if shot == "p" || shot == "q" {
            let (targets, channels) = shot_inventory();
            crate::fl::bridge().update(|v| {
                v.targets = targets;
                v.channels = channels;
                v.loaded = true;
            });
        }
        if shot == "twin" || shot == "twin-routes" {
            let (lead, bass) = shot_twins();
            let target = |key: crate::fl::Key, track: u32| crate::fl::Target { key, track, track_name: String::new() };
            let eq = crate::fl::Key { at: crate::fl::At::Fx { track: 3, slot: 0 }, plugin: "Pro-Q 3".into() };
            crate::fl::bridge().update(|v| {
                v.targets = vec![target(eq, 3), target(lead, 3), target(bass, 10)];
                v.loaded = true;
            });
        }
        if shot == "u" || shot == "w" {
            // Serum 2 focused in FL with its parameter names loaded
            let serum = crate::fl::Key { at: crate::fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() };
            let names = shot_param_names();
            crate::fl::bridge().update(|v| {
                v.focus = Some(serum.clone());
                v.params = Some((serum, Ok(names)));
            });
        }
        if shot == "v" {
            self.pong.shot_balls();
        }
        if matches!(shot.as_str(), "g" | "y" | "z" | "t") {
            // the mouth plays a snare; the figure turned left, tilted, mouth open
            let mut st = self.writer.player.store();
            let id = st.add("Snare", crate::midi::Play::Note { pitch: 38, vel: 100 }, None);
            st.binds.push(crate::midi::Bind { signal: "cam0.mouth.open".into(), trigger: crate::route::Trigger::Above, part: id });
            self.writer.update_midi(st);
            let pose = crate::head_ui::Pose { yaw: 0.3, pitch: 0.55, roll: 0.35, mouth: 0.7, smile: 0.0, eye: [1.0, 0.15] };
            self.head.shot(crate::head_ui::Part::Mouth, pose);
        }
        match shot.as_str() {
            // the mouth picked, a little open; o adds the stand-in face's
            // tracking boxes over the stand-in camera picture
            "m" | "o" => self.head.shot(crate::head_ui::Part::Mouth, crate::head_ui::Pose { mouth: 0.62, eye: [0.95, 0.9], yaw: 0.46, ..Default::default() }),
            "n" => self.head.shot(crate::head_ui::Part::Head, crate::head_ui::Pose { yaw: 0.62, pitch: 0.48, roll: 0.55, ..Default::default() }),
            _ => {}
        }
        if shot == "o" {
            let g = crate::engine::geom::FaceGeom {
                face: Some([0.39, 0.21, 0.51, 0.45]),
                eyes: [Some([0.418, 0.293, 0.442, 0.307]), Some([0.458, 0.293, 0.482, 0.307])],
                mouth: Some([0.428, 0.383, 0.472, 0.397]),
            };
            self.shot_face = Some((g, crate::head_ui::Pose { mouth: 0.62, eye: [0.95, 0.9], yaw: 0.46, ..Default::default() }));
        }
        if shot == "z" || shot == "t" {
            // the left index plays a hi-hat pattern; the top wall is picked
            let slot = crate::fingers::Slot::finger(0, 1);
            let mut st = self.writer.player.store();
            if let Some(play) = crate::midi::template("Hi-hat basic") {
                let id = st.add("Hi-hat basic", play, None);
                st.binds.push(crate::midi::Bind { signal: slot.signal(), trigger: slot.trigger(), part: id });
                self.writer.update_midi(st);
                if shot == "t" {
                    ctx.memory_mut(|m| m.open_popup(Id::new(("part-picker", id)).with("popup")));
                }
            }
            self.fingers.shot_select(slot);
            self.advanced = true;
        }
        if shot == "y" {
            // the top wall picked instead of a finger (one editor at a
            // time), once the field has reloaded the state set up here
            self.shot_wall = Some(crate::pong::arena::Side::Top);
            self.advanced = true;
        }
        self.settings_open = shot == "e" || shot == "s";
        if shot == "e" {
            self.set_folds.face = true;
        }
        if shot == "s" {
            self.set_folds = SetFolds { signals: true, diag: true, face: true };
        }
        if let Some(x) = std::env::var("SYNTHEXIS_UI_SCROLL_X").ok().and_then(|v| v.parse::<f32>().ok()) {
            Sentence::shot_scroll_x(ctx, x);
        }
        crate::log::log(&format!("ui shot {shot} set up"));
    }

    /// `SYNTHEXIS_UI_SHOT` = profiles: profile 1 holds Hands and Head (the
    /// left index plays a hi-hat, the mouth a snare), profile 2 is active
    /// with the built-in Pong, over the stand-in camera picture.
    fn shot_profiles(&mut self) {
        let mut st = self.writer.player.store();
        let slot = crate::fingers::Slot::finger(0, 1);
        if let Some(play) = crate::midi::template("Hi-hat basic") {
            let id = st.add("Hi-hat basic", play, None);
            st.binds.push(crate::midi::Bind { signal: slot.signal(), trigger: slot.trigger(), part: id });
        }
        let snare = st.add("Snare", crate::midi::Play::Note { pitch: 38, vel: 100 }, None);
        st.binds.push(crate::midi::Bind { signal: "cam0.mouth.open".into(), trigger: crate::route::Trigger::Above, part: snare });
        self.writer.update_midi(st);
        self.view = View { sections: vec![Section::Hands, Section::Head], preset: None, ..Default::default() };
        self.view.write(&self.params);
        self.switch_profile(1, i18n::text(settings::store().get().lang));
        let snap = preset::with_section(preset::capture(&self.writer), Section::Pong);
        if let Err(e) = preset::apply(&self.writer, &snap) {
            crate::log::log(&format!("ui shot profiles: Pong not loaded ({e})"));
        }
        self.view = View { sections: vec![Section::Pong], preset: Some("Pong".into()), ..Default::default() };
        self.view.write(&self.params);
        *self.shared.preview.lock() = Some(Arc::new(shot_frame()));
        crate::log::log("ui shot profiles set up");
    }

    /// The chat routes of the captures h (Noa's "when i rise my hands up i
    /// want the filter": arm.raise reads hand.y, pending, Hands opens by
    /// itself), i (the left hand's openness kept on one parameter, the
    /// right hand's height on three, the right hand picked), j (the left
    /// index plays a hi-hat and its tilt drives a parameter, the Parameter
    /// kind shown), l (a pending mouth route, Head opens by itself), m and
    /// o (the mouth's drive kept) and n (the head's turn kept on three
    /// parameters, the right eye's wink pending).
    fn shot_routes(&mut self, shot: &str) {
        use synthexis_parser::resolve::{Bound, Route, Target, Via};
        let serum = crate::fl::Key { at: crate::fl::At::Gen { ch: 1 }, plugin: "Serum 2".into() };
        let target = |token: &str, text: &str, index: u32, name: &str| Target {
            token: token.into(),
            text: text.into(),
            bound: Some(Bound { index, name: name.into(), via: Via::Name, plugin: None }),
        };
        let route = |source: &str, side: Option<&str>, targets: Vec<Target>| Route {
            source: source.into(),
            side: side.map(str::to_string),
            inv: false,
            gates: Vec::new(),
            mods: Vec::new(),
            targets,
        };
        let cutoff = || target("T:param:filter.cutoff", "cutoff", 12, "Filter 1 Cutoff");
        match shot {
            "h" => {
                let filter = target("T:toggle:filter", "filter", 7, "Filter 1 On");
                self.sentence.shot_said(vec![route("arm.raise", Some("left"), vec![filter])], &serum, "when i rise my hands up i want the filter", false);
            }
            "w" => self.pong.shot_find(crate::pong::arena::Side::Left),
            "twin" | "twin-routes" => {
                let (lead, bass) = shot_twins();
                let freq = || target("T:param:filter.cutoff", "filter 1 freq", 12, "Filter 1 Freq");
                self.sentence.shot_said(vec![route("hand.finger.thumb.tilt", Some("left"), vec![freq()])], &lead, "tilting my left thumb moves the filter", true);
                self.sentence.shot_said(vec![route("hand.x", Some("right"), vec![freq()])], &bass, "my right hand left and right moves the bass filter", true);
                self.fingers.shot_hand(1);
                if shot == "twin-routes" {
                    self.fingers.shot_routes();
                }
            }
            "x" => self.pong.select(crate::pong::arena::Side::Bottom),
            "i" | "u" => {
                self.sentence.shot_said(vec![route("hand.openness", Some("left"), vec![cutoff()])], &serum, "open my left hand opens the cutoff", true);
                let three = vec![
                    target("T:param:filter.res", "resonance", 13, "Filter 1 Res"),
                    target("T:param:fx.reverb", "reverb", 40, "FX Reverb Mix"),
                    target("T:param:macro", "macro 1", 1, "Macro 1"),
                ];
                self.sentence.shot_said(vec![route("hand.y", Some("right"), three)], &serum, "my right hand height drives resonance, reverb and macro 1", true);
                self.fingers.shot_hand(1);
                if shot == "u" {
                    self.fingers.shot_routes_find();
                }
            }
            "j" => {
                let slot = crate::fingers::Slot::finger(0, 1);
                let mut st = self.writer.player.store();
                if let Some(play) = crate::midi::template("Hi-hat basic") {
                    let id = st.add("Hi-hat basic", play, None);
                    st.binds.push(crate::midi::Bind { signal: slot.signal(), trigger: slot.trigger(), part: id });
                    self.writer.update_midi(st);
                }
                self.sentence.shot_said(vec![route("hand.finger.index.tilt", Some("left"), vec![cutoff()])], &serum, "tilting my left index moves the cutoff", true);
                self.fingers.shot_params(slot);
            }
            "l" => {
                let drive = target("T:param:fx.drive", "drive", 30, "Dist Drive");
                self.sentence.shot_said(vec![route("mouth.open", None, vec![drive])], &serum, "opening my mouth adds drive", false);
            }
            "m" | "o" => {
                let drive = target("T:param:fx.drive", "drive", 30, "Dist Drive");
                self.sentence.shot_said(vec![route("mouth.open", None, vec![drive])], &serum, "opening my mouth adds drive", true);
            }
            "n" => {
                let three = vec![
                    target("T:param:pan", "pan", 3, "Pan"),
                    cutoff(),
                    target("T:param:macro", "macro 2", 2, "Macro 2"),
                ];
                self.sentence.shot_said(vec![route("head.yaw", None, three)], &serum, "turning my head pans and moves the cutoff and macro 2", true);
                let wink = target("T:param:fx.reverb", "reverb", 40, "FX Reverb Mix");
                self.sentence.shot_said(vec![route("eye.closed", Some("right"), vec![wink])], &serum, "closing my right eye adds reverb", false);
            }
            _ => {}
        }
    }

    /// Switches to profile `to` (the top bar's 1 2 3), the chat field's
    /// pending lines dropped first; the bar shows the new one at once.
    fn switch_profile(&mut self, to: usize, t: &'static Text) {
        let UiState { presets, view, writer, params, sentence, .. } = self;
        let mut discard = || sentence.discard_pending();
        let mut cx = Ctx { writer, params, t, discard: &mut discard };
        presets.switch_profile(to, view, &mut cx);
        self.profiles = preset::profile::Profiles::read(&self.params).summary();
    }

    fn grow(&mut self, want: &[Section]) {
        if grow_view(&mut self.view, want) {
            self.view.write(&self.params);
        }
    }
}

/// A stand-in camera frame for the popup captures (the standalone has no
/// camera permission): a lit wall, a bright window, a dark door and a
/// person in a red shirt with eyes and a mouth, 1280x720.
fn shot_frame() -> Preview {
    let (w, h) = (1280usize, 720usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let mut c = [214.0 - 60.0 * fy, 190.0 - 50.0 * fy, 160.0 - 40.0 * fy];
            if (0.68..0.92).contains(&fx) && (0.08..0.55).contains(&fy) {
                c = [245.0, 248.0, 250.0];
            }
            if (0.05..0.2).contains(&fx) && fy > 0.2 {
                c = [70.0, 52.0, 40.0];
            }
            let (dx, dy) = ((fx - 0.45) / 0.14, (fy - 0.75) / 0.3);
            if dx * dx + dy * dy < 1.0 {
                c = [170.0, 40.0, 50.0];
            }
            let (hx, hy) = ((fx - 0.45) / 0.06, (fy - 0.33) / 0.12);
            if hx * hx + hy * hy < 1.0 {
                c = [200.0, 150.0, 120.0];
                // the eyes and the mouth (shot o's tracking boxes)
                let spot = |cx: f32, cy: f32, rx: f32, ry: f32| ((fx - cx) / rx).powi(2) + ((fy - cy) / ry).powi(2) < 1.0;
                if spot(0.43, 0.30, 0.008, 0.005) || spot(0.47, 0.30, 0.008, 0.005) {
                    c = [60.0, 40.0, 35.0];
                }
                if spot(0.45, 0.39, 0.016, 0.005) {
                    c = [130.0, 55.0, 55.0];
                }
            }
            rgba.extend([c[0] as u8, c[1] as u8, c[2] as u8, 255]);
        }
    }
    Preview { w, h, rgba }
}

/// Serum 2's parameter names for the search captures (shots u and w),
/// at the indices the captured routes use.
fn shot_param_names() -> Vec<(u32, String)> {
    let names = [
        (0, "Master Volume"),
        (1, "Macro 1"),
        (2, "Macro 2"),
        (3, "Pan"),
        (4, "Macro 3"),
        (5, "Macro 4"),
        (6, "Osc A Level"),
        (7, "Filter 1 On"),
        (8, "Osc A Pan"),
        (9, "Osc A WT Pos"),
        (10, "Osc B Level"),
        (11, "Osc B WT Pos"),
        (12, "Filter 1 Cutoff"),
        (13, "Filter 1 Res"),
        (14, "Filter 1 Drive"),
        (15, "Filter 1 Mix"),
        (16, "Filter 2 Cutoff"),
        (17, "Filter 2 Res"),
        (18, "Env 1 Attack"),
        (19, "Env 1 Decay"),
        (20, "Env 1 Sustain"),
        (21, "Env 1 Release"),
        (22, "LFO 1 Rate"),
        (23, "LFO 2 Rate"),
        (30, "Dist Drive"),
        (31, "Dist Mix"),
        (35, "Delay Mix"),
        (36, "Delay Feedback"),
        (40, "FX Reverb Mix"),
        (41, "Reverb Size"),
        (42, "Chorus Mix"),
    ];
    names.iter().map(|(i, n)| (*i, (*n).to_string())).collect()
}

/// A big FL project for the plugin picker's capture (shot p): Master with
/// six effects, ten mixer tracks with one instrument and two to four
/// effects each, and the channel rack. Plugin names from Noa's demo
/// project (4.11.26 - synthexis demo.flp), the rest realistic.
/// The two Serum 2 instances of the twin captures: channel 3 on mixer
/// track 3, channel 10 on track 10 (Noa's project, 11-05).
fn shot_twins() -> (crate::fl::Key, crate::fl::Key) {
    let serum = |ch| crate::fl::Key { at: crate::fl::At::Gen { ch }, plugin: "Serum 2".into() };
    (serum(2), serum(9))
}

fn shot_inventory() -> (Vec<crate::fl::Target>, Vec<crate::fl::ChannelRow>) {
    use crate::fl::{At, ChannelRow, Key, Target};
    let fx = |track: u32, slot: u32, plugin: &str, name: &str| Target { key: Key { at: At::Fx { track, slot }, plugin: plugin.into() }, track, track_name: name.into() };
    let gen = |ch: u32, track: u32, plugin: &str, name: &str| Target { key: Key { at: At::Gen { ch }, plugin: plugin.into() }, track, track_name: name.into() };
    let master = ["Fruity Balance", "UADx Ampex ATR-102 Master Tape", "Pro-Q 3", "Pro-C 2", "FabFilter Pro-L 2", "Edison"];
    let tracks: [(&str, &str, &[&str]); 10] = [
        ("Lead", "Serum 2", &["Pro-Q 3", "ShaperBox 3", "ValhallaSupermassive"]),
        ("Bass", "Serum 2 #2", &["Pro-Q 3", "Pro-C 2", "Fruity Limiter"]),
        ("Keys", "Serum 2 #3", &["Pro-Q 3", "ValhallaFutureVerb"]),
        ("Pad", "Serum 2 #4", &["ShaperBox 3", "ValhallaSupermassive", "Pro-Q 3", "Fruity Balance"]),
        ("Arp", "Serum 2 #5", &["Pro-Q 3", "ValhallaFutureVerb", "Fruity Limiter"]),
        ("Pluck", "Serum 2 #6", &["Pro-Q 3", "Pro-C 2"]),
        ("Insert 7", "FPC", &["Pro-Q 3", "Pro-C 2", "FabFilter Pro-L 2"]),
        ("Vox", "Slicex", &["Pro-Q 3", "ValhallaSupermassive", "Fruity Balance"]),
        ("Insert 9", "Fruity Granulizer", &["ValhallaFutureVerb", "ShaperBox 3"]),
        ("Perc", "Vital", &["Pro-Q 3", "Pro-C 2", "Fruity Limiter", "Edison"]),
    ];
    let mut targets: Vec<Target> = master.iter().enumerate().map(|(i, p)| fx(0, i as u32, p, "")).collect();
    let mut channels = Vec::new();
    for (i, (name, inst, fxs)) in tracks.iter().enumerate() {
        let track = i as u32 + 1;
        targets.push(gen(i as u32, track, inst, name));
        channels.push(ChannelRow { index: i as u32, name: (*name).into(), plugin: (*inst).into() });
        targets.extend(fxs.iter().enumerate().map(|(s, p)| fx(track, s as u32, p, name)));
    }
    let samples = ["SFHD_Kick_06", "ukgn_snare_peckham", "ukgn_130_drum_loop_box_hihats", "ukgn_cymbal_high_touch", "SFHD_Perc_23", "BWB VOX (HEY) (2)"];
    let base = channels.len() as u32;
    channels.extend(samples.iter().enumerate().map(|(i, n)| ChannelRow { index: base + i as u32, name: (*n).into(), plugin: String::new() }));
    (targets, channels)
}

pub fn make_editor(params: Arc<SynthexisParams>, shared: Arc<EngineShared>, writer: Arc<Writer>) -> Option<Box<dyn Editor>> {
    let state = params.editor_state.clone();
    let host = Arc::new(AtomicUsize::new(0));
    let inner = create_egui_editor(
        state,
        {
            let routes = params.routes.clone();
            UiState {
                params,
                shared,
                writer: writer.clone(),
                cam: Cam::default(),
                backdrop: Backdrop::default(),
                sentence: Sentence::new(Some(crate::fl::bridge()), Some(writer), Some(routes)),
                settings_open: false,
                set_folds: SetFolds::default(),
                fingers: crate::fingers::Fingers::default(),
                pong: crate::pong_ui::PongUi::default(),
                head: crate::head_ui::HeadUi::default(),
                presets: Presets::default(),
                view: View::default(),
                profiles: preset::profile::Summary::default(),
                view_gen: None,
                advanced: false,
                opened: Instant::now(),
                host: host.clone(),
                shot: std::env::var("SYNTHEXIS_UI_SHOT").ok().filter(|s| !s.is_empty()),
                shot_wall: None,
                shot_chat: None,
                shot_face: None,
                shot_scroll: std::env::var("SYNTHEXIS_UI_SCROLL").ok().and_then(|s| if s == "end" { Some(-1.0) } else { s.parse().ok() }),
            }
        },
        // Runs on every open: the egui Context is new each time but UiState
        // persists, so drop the stale texture handles.
        |ctx, s| {
            s.cam.clear();
            s.backdrop.reset_texture();
            s.fingers.reset_textures();
            s.head.reset_textures();
            s.opened = Instant::now();
            glass::style(ctx);
        },
        window,
    )?;
    Some(Box::new(Wrap { inner, host }))
}

/// One frame of the window.
fn window(ctx: &egui::Context, setter: &ParamSetter, s: &mut UiState) {
    let store = settings::store();
    let cfg = store.get();
    let t = i18n::text(cfg.lang);
    let de = cfg.lang == Lang::De;
    s.sync_view();
    s.apply_shot(ctx);
    if ctx.cumulative_pass_nr() > 5 {
        if let Some(side) = s.shot_wall.take() {
            s.pong.select(side);
        }
        if let Some(shot) = s.shot_chat.take() {
            s.shot_routes(&shot);
        }
    }
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Tok::WINDOW)).show(ctx, |ui| {
        let full = ui.max_rect();
        // a host view smaller than egui's own: the layout fits what shows
        let actual = match mac::view_size(s.host.load(Ordering::Acquire)) {
            Some(v) if v.x > 1.0 && v.y > 1.0 => vec2(full.width().min(v.x), full.height().min(v.y)),
            _ => full.size(),
        };
        let win = Rect::from_min_size(full.min, actual);
        let w = win.width();
        // what is visibly open: an unfolded section or the suggestions
        let open = panel_open(&s.view, s.sentence.inline_shown());
        let folded = s.view.sections.iter().filter(|sec| s.view.is_folded(**sec)).count();
        let cam_h = ctx.animate_value_with_time(Id::new("cam-h"), cam_height_with(w, win.height(), open, folded), 0.18).floor();
        let cam = Rect::from_min_size(win.min, vec2(w, cam_h));
        let painter = ui.painter().clone();
        // first: the dither under everything; the text area gets the gentle one
        let text_area = Rect::from_min_max(pos2(win.left() + Tok::INSET, cam.bottom()), pos2(win.right() - Tok::INSET, win.bottom() - Tok::BOTTOM_ROOM));
        s.backdrop.paint(ctx, &painter, win, text_area);

        s.pong.frame(ctx, &s.writer);
        let pong = s.writer.pong.setup();
        // an open section or Pong shows the whole frame, else the cover crop
        let cr = fit_for(&s.view, pong.on).place(cam);
        s.cam.upload(ctx, s.shared.preview.lock().clone());
        if let Some(tex) = &s.cam.tex {
            painter.with_clip_rect(cam).image(tex.id(), cr.img, cr.uv, Color32::WHITE);
        }
        // what the frost behind every popup shows (v3 2.7); none of the
        // camera while Settings covers it
        let blur = s.cam.tex.is_some().then(|| s.cam.blur_id(ctx));
        let frost_cam = if s.settings_open { Rect::NOTHING } else { cam };
        glass::set_frost(ctx, glass::FrostSrc { win, cam: frost_cam, img: cr.img, uv: cr.uv, blur });
        let lit = s.writer.binds_on();
        if !s.settings_open {
            skeletons(&painter.with_clip_rect(cam), cr.img, cr.uv, &lit);
            // the face's tracking boxes, under the top bar
            if let Some((g, pose)) = s.shot_face.or_else(crate::head_ui::live_boxes) {
                crate::head_ui::crt_boxes(&painter.with_clip_rect(cam), cr.img, cr.uv, &g, &pose, t, cfg.face);
            }
        }
        if pong.on && !s.settings_open && s.pong.canvas(ui, &painter.with_clip_rect(cam), cam, cr.img, &pong, true) {
            s.grow(&[Section::Pong]);
        }
        let (cam_text, problem) = camera_status(t, &s.shared);
        // hidden under an open popup, whose glass would show it as a ghost
        if (problem || s.cam.tex.is_none()) && !ctx.memory(|m| m.any_popup_open()) {
            painter.text(cam.center(), egui::Align2::CENTER_CENTER, &cam_text, glass::font(Tok::BODY), if problem { ui.visuals().warn_fg_color } else { white(Tok::LABEL) });
        }
        // the 1 px line under the camera, full width
        painter.hline(win.x_range(), painter.round_to_pixel_center(cam.bottom() - 0.5), egui::Stroke::new(1.0_f32, white(Tok::CAM_EDGE)));

        // Settings' scrim takes every click while it is open; a disabled Ui
        // would paint nothing (glass's fade colour is transparent)
        ui.scope(|ui| {
            let player = &s.writer.player;
            let beats = player.beats_now();
            let pos = player.transport.load();
            let info = BarInfo {
                live: s.params.live.value(),
                profiles: s.profiles,
                key: crate::midi::key_name(player.store().key),
                playing: pos.playing,
                tempo: pos.tempo,
                bar: (beats / 4.0).floor() as i64 + 1,
                beat: beats.rem_euclid(4.0).floor() as i64 + 1,
            };
            let bar = top_bar(ui, cam, &mut s.opened, s.settings_open, &info, t);
            if let Some(v) = bar.live {
                set_bool(setter, &s.params.live, v);
            }
            if bar.settings {
                s.settings_open = true;
            }
            if let Some(to) = bar.profile {
                s.switch_profile(to, t);
            }

            let below = Rect::from_min_max(pos2(win.left(), cam.bottom()), win.max);
            let top_h = top_section(ui, s, below, cfg.completion && !s.settings_open, cfg.lang, t);
            let top_bottom = cam.bottom() + top_h;
            glass::section_rule(&painter, win.x_range(), top_bottom);
            let panel = Rect::from_min_max(pos2(win.left(), top_bottom + RULE_H), win.max);
            ui.scope_builder(UiBuilder::new().max_rect(panel), |ui| panel_area(ui, s, setter, t, de, w, &cam_text, problem));
        });
        // a click on a node in the Hands Routes view shows the kept lines too
        if s.fingers.take_unfold() {
            s.sentence.unfold_routes();
        }
        // a kept row's x in a drives list (Hands, Head) unmaps that route
        for (signal, target) in [s.fingers.take_unmap(), s.head.take_unmap()].into_iter().flatten() {
            let n = s.sentence.unmap_kept(&signal, &target);
            crate::log::log(&format!("unmapped {n} kept route(s) from {signal}"));
        }
        // a box of the Hands Routes view picked another target or movement
        for (signal, target, to) in s.fingers.take_retarget() {
            let n = s.sentence.retarget_kept(&signal, &target, &to);
            crate::log::log(&format!("retargeted {n} kept route(s) from {signal}"));
        }
        s.presets.toast(ctx, win);
    });
    if s.settings_open {
        let diag = diagnostics(t, &s.shared);
        let inp = SetIn { params: &s.params, setter, writer: &s.writer, diag: &diag };
        s.settings_open = settings_overlay(ctx, store, t, &inp, &mut s.set_folds);
    }
    if ctx.memory(|m| m.any_popup_open()) {
        s.cam.refresh_blur(ctx);
    }
    // after the frame's own writes (an apply bumps state_gen)
    s.sync_view();
}

/// The top section (2.5) at the top of `below` (the panel area under the
/// camera): the plugin picker, the chat field, the preset picker and "+",
/// and under them the plugin note, the routes strip and Save as with
/// Undo; then, while they show, the chat's suggestions full width (at
/// most `INLINE_SHARE` of the room under row 2, scrolling inside). Opens
/// the sections a reply asks for. Returns the section's height:
/// `TOP_SECTION_H`, grown by the suggestions.
fn top_section(ui: &mut egui::Ui, s: &mut UiState, below: Rect, completion: bool, lang: Lang, t: &'static Text) -> f32 {
    let top = below;
    let cols = top_columns(top.width());
    let y1 = top.top() + TOP_PAD;
    let y2 = y1 + Tok::TOP_H + TOP_ROW_GAP;
    let row1 = |i: usize| cols[i].translate(vec2(top.left(), y1));
    let row2 = |i: usize| Rect::from_min_size(pos2(top.left() + cols[i].left(), y2), vec2(cols[i].width(), Tok::CTRL_H));
    let at = |ui: &mut egui::Ui, r: Rect, add: &mut dyn FnMut(&mut egui::Ui)| {
        ui.scope_builder(UiBuilder::new().max_rect(r).layout(Layout::left_to_right(Align::Center)), |ui| add(ui));
    };
    s.sentence.tick(ui.ctx(), &s.params.target_pin, lang);
    s.pong.sync_scope(&mut s.sentence);
    at(ui, row1(0), &mut |ui| s.sentence.picker_ui(ui, cols[0].size(), &s.params.target_pin));
    at(ui, row1(1), &mut |ui| s.sentence.field_ui(ui, cols[1].size(), completion));
    at(ui, row2(0), &mut |ui| s.sentence.note_ui(ui, cols[0].width()));
    at(ui, row2(1), &mut |ui| {
        s.sentence.routes_strip(ui, cols[1].width());
    });
    let UiState { presets, view, writer, params, sentence, .. } = s;
    let mut discard = || sentence.discard_pending();
    let mut cx = Ctx { writer, params, t, discard: &mut discard };
    presets.tick(Instant::now(), &cx);
    at(ui, row1(2), &mut |ui| presets.picker(ui, cols[2].size(), view, &mut cx));
    at(ui, row1(3), &mut |ui| presets.plus(ui, view, &mut cx));
    at(ui, row2(2), &mut |ui| presets.row2(ui, cols[2].width(), view, &mut cx));
    // the suggestions under row 2: the section grows by them
    let y3 = y2 + Tok::CTRL_H + TOP_ROW_GAP;
    let max_h = (INLINE_SHARE * (top.height() - TOP_SECTION_H)).max(2.0 * Tok::CTRL_H);
    let room = Rect::from_min_size(pos2(top.left() + Tok::INSET, y3), vec2((top.width() - 2.0 * Tok::INSET).max(0.0), max_h));
    let inline = ui.scope_builder(UiBuilder::new().max_rect(room).layout(Layout::top_down(Align::Min)), |ui| s.sentence.routes_inline(ui, max_h)).inner;
    let want: Vec<Section> = [(s.sentence.take_open_hands(), Section::Hands), (s.sentence.take_open_head(), Section::Head), (s.sentence.take_open_pong(), Section::Pong)]
        .into_iter()
        .filter_map(|(w, sec)| w.then_some(sec))
        .collect();
    s.grow(&want);
    if inline.is_positive() {
        inline.bottom() + TOP_PAD - top.top()
    } else {
        TOP_SECTION_H
    }
}

/// The scrolling panel area: the open sections in the view's order, each
/// with its header and the full-width rule under it, then Advanced and
/// the bottom room. Every control in it is reachable by scrolling.
#[allow(clippy::too_many_arguments)]
fn panel_area(ui: &mut egui::Ui, s: &mut UiState, setter: &ParamSetter, t: &'static Text, de: bool, w: f32, cam_text: &str, problem: bool) {
    let mut area = egui::ScrollArea::vertical().id_salt("panel").auto_shrink([false, false]).drag_to_scroll(false);
    if let Some(y) = s.shot_scroll.filter(|y| *y >= 0.0) {
        area = area.vertical_scroll_offset(y);
    }
    area.show(ui, |ui| {
        let inset = Tok::INSET as i8;
        egui::Frame::NONE.inner_margin(Margin { left: inset, right: inset, top: 0, bottom: 0 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut closed = Vec::new();
            for sec in s.view.sections.clone() {
                if section(ui, s, setter, t, de, sec) {
                    closed.push(sec);
                }
                ui.add_space(Tok::SEC_BOTTOM - Tok::ROW_GAP);
                glass::section_rule(ui.painter(), Rangef::new(0.0, w), ui.cursor().top());
                ui.add_space(RULE_H);
            }
            for sec in closed {
                let UiState { presets, view, writer, params, sentence, .. } = &mut *s;
                let mut discard = || sentence.discard_pending();
                let mut cx = Ctx { writer, params, t, discard: &mut discard };
                presets.close(view, sec, &mut cx);
            }
            // a wall row in the All view opens that wall in Pong
            if let Some(side) = s.fingers.take_wall_pick() {
                s.pong.select(side);
                s.grow(&[Section::Pong]);
            }
            advanced(ui, s, setter, t, cam_text, problem);
            ui.add_space(Tok::BOTTOM_ROOM - Tok::ROW_GAP);
            if s.shot_scroll.is_some_and(|y| y < 0.0) {
                ui.scroll_to_cursor(Some(Align::Max));
            }
        });
    });
}

/// A section header's fold chevron (as Advanced's) in the `FOLD_W`
/// column left of the title in `head`: a click on the chevron or the
/// title (up to `title_r`) folds or unfolds the body. The header's inset
/// rule continues under the column while open. True on the click.
fn fold_chevron(ui: &mut egui::Ui, id: Id, head: Rect, title_r: f32, open: bool) -> bool {
    let row = Rect::from_min_max(head.min, pos2(title_r.max(head.left() + FOLD_W), head.bottom() - 2.0));
    let resp = ui.interact(row, id.with("fold"), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let turn = ui.ctx().animate_bool_with_time(id.with("fold-turn"), open, 0.15);
    let p = ui.painter();
    glass::chevron_at(p, pos2(head.left() + 4.0, row.center().y), 4.0, turn, white(if resp.hovered() { Tok::VALUE } else { Tok::HINT }));
    // folded: no title rule, the section separator follows
    if open {
        let y = p.round_to_pixel_center(head.bottom() - 2.0);
        let x = Rangef::new(head.left(), head.left() + FOLD_W);
        p.hline(x, y, egui::Stroke::new(1.0_f32, white(Tok::RULE)));
        p.hline(x, y + 1.0, egui::Stroke::new(1.0_f32, glass::black(Tok::RULE_DARK)));
    }
    resp.clicked()
}

/// One feature section: the header with its fold chevron and close arm,
/// the body unless folded (saved with the view). True on the confirming
/// click of its `x`.
fn section(ui: &mut egui::Ui, s: &mut UiState, setter: &ParamSetter, t: &'static Text, de: bool, sec: Section) -> bool {
    let title = match sec {
        Section::Hands => t.f_title,
        Section::Pong => t.p_title,
        Section::Head => t.h_title,
    };
    let id = Id::new(("section", sec));
    let hover = fill(t.close_section, &[&title]);
    // the arm text shows only while the pointer is on the header
    let head = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), Tok::HEAD_H));
    let arm = if ui.rect_contains_pointer(head) {
        let UiState { writer, params, sentence, presets, .. } = &mut *s;
        let mut discard = || sentence.discard_pending();
        let cx = Ctx { writer, params, t, discard: &mut discard };
        presets.close_text(sec, &cx)
    } else {
        String::new()
    };
    // a calibration in progress keeps Hands open
    let close = (sec != Section::Hands || !s.fingers.calibrating()).then_some((hover.as_str(), arm.as_str()));
    // the header right of the chevron's column
    let inner = Rect::from_min_max(pos2(head.left() + FOLD_W, head.top()), head.max);
    let folded = s.view.is_folded(sec);
    let (slot, confirmed) = ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)), |ui| glass::section_head(ui, id, title, close, !folded)).inner;
    if fold_chevron(ui, id, head, slot.left() - 16.0, !folded) {
        s.view.set_folded(sec, !folded);
        s.view.write(&s.params);
    }
    if folded {
        // Pong's On stays in its header; the Hands views have no body to show
        if sec == Section::Pong && s.pong.header(ui, slot, &s.writer, t, s.params.live.value()).live_on {
            set_bool(setter, &s.params.live, true);
        }
        return confirmed;
    }
    match sec {
        Section::Hands => {
            s.fingers.header(ui, slot, &s.writer, t);
            let routing = s.sentence.routing();
            s.fingers.section(ui, &s.writer, t, de, &routing);
        }
        Section::Pong => {
            let head = s.pong.header(ui, slot, &s.writer, t, s.params.live.value());
            if head.live_on {
                set_bool(setter, &s.params.live, true);
            }
            s.pong.body(ui, &s.writer, &mut s.fingers, t, de);
        }
        Section::Head => {
            let routing = s.sentence.face_routing();
            s.head.section(ui, &s.writer, t, &routing);
        }
    }
    confirmed
}

/// Advanced, the last row (R8): Test signal; the camera status as its
/// summary and its last line. (The hand roles left the window: what a
/// hand does comes from its routes and parts; the host parameters stay
/// for old projects.)
fn advanced(ui: &mut egui::Ui, s: &mut UiState, setter: &ParamSetter, t: &Text, cam_text: &str, problem: bool) {
    glass::disclosure(ui, Id::new("advanced"), &mut s.advanced, t.advanced, Some(cam_text));
    if !s.advanced {
        return;
    }
    ui.horizontal(|ui| {
        let mut test = s.params.test_signal.value();
        if glass::toggle(ui, &mut test, t.test_signal, Tok::TEST).changed() {
            set_bool(setter, &s.params.test_signal, test);
        }
    });
    let warn = ui.visuals().warn_fg_color;
    ui.label(RichText::new(cam_text).color(if problem { warn } else { white(Tok::LABEL) }));
}

#[cfg(test)]
mod tests {
    use nih_plug::prelude::{GuiContext, ParamPtr, PluginApi, PluginState};
    use nih_plug_egui::egui::{Event, Modifiers, Pos2, RawInput, Rect};

    use super::*;
    use crate::i18n::{DE, EN};

    /// A GUI context that ignores parameter changes (the Settings tests
    /// draw the signal sliders but never touch them).
    struct NoHost;

    #[allow(unsafe_code)]
    impl GuiContext for NoHost {
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Standalone
        }
        fn request_resize(&self) -> bool {
            false
        }
        unsafe fn raw_begin_set_parameter(&self, _: ParamPtr) {}
        unsafe fn raw_set_parameter_normalized(&self, _: ParamPtr, _: f32) {}
        unsafe fn raw_end_set_parameter(&self, _: ParamPtr) {}
        fn get_state(&self) -> PluginState {
            PluginState { version: String::new(), params: Default::default(), fields: Default::default() }
        }
        fn set_state(&self, _: PluginState) {}
    }

    struct Overlay {
        ctx: egui::Context,
        store: settings::Store,
        writer: Arc<Writer>,
        folds: SetFolds,
        open: bool,
        out: Option<egui::FullOutput>,
        time: f64,
    }

    impl Overlay {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("synthexis-overlay-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let store = settings::Store::open(dir.join("settings.json"));
            let ctx = egui::Context::default();
            glass::style(&ctx);
            let writer = Writer::new(Arc::new(SynthexisParams::default()));
            let mut o = Self { ctx, store, writer, folds: SetFolds::default(), open: true, out: None, time: 0.0 };
            o.settle();
            o
        }

        /// A new area shows from its second frame; the fade takes a few.
        fn settle(&mut self) {
            for _ in 0..20 {
                self.frame(vec![]);
            }
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 1.0 / 60.0;
            let raw = RawInput {
                events,
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 820.0))),
                ..Default::default()
            };
            let (store, open, writer, folds) = (&self.store, &mut self.open, &self.writer, &mut self.folds);
            let host = NoHost;
            let setter = ParamSetter::new(&host);
            self.out = Some(self.ctx.run(raw, |ctx| {
                if *open {
                    let t = i18n::text(store.get().lang);
                    let diag = vec!["diag line".to_string()];
                    let inp = SetIn { params: &writer.params, setter: &setter, writer, diag: &diag };
                    *open = settings_overlay(ctx, store, t, &inp, folds);
                }
            }));
        }

        fn texts(&self) -> Vec<String> {
            texts_of(self.out.as_ref())
        }

        fn find(&self, needle: &str) -> Option<Pos2> {
            find_in(self.out.as_ref(), needle)
        }

        fn click_at(&mut self, at: Pos2) {
            let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            self.frame(vec![Event::PointerMoved(at), button(true)]);
            self.frame(vec![button(false)]);
            self.settle();
        }

        /// False when `needle` is not drawn.
        fn click(&mut self, needle: &str) -> bool {
            let Some(at) = self.find(needle) else { return false };
            self.click_at(at);
            true
        }
    }

    fn walk_texts(s: &egui::Shape, out: &mut Vec<(String, Pos2, f32)>) {
        match s {
            egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos + t.galley.rect.center().to_vec2(), f32::from(t.fallback_color.a()) / 255.0)),
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk_texts(s, out)),
            _ => {}
        }
    }

    fn all_texts(out: Option<&egui::FullOutput>) -> Vec<(String, Pos2, f32)> {
        let mut v = Vec::new();
        for c in out.iter().flat_map(|o| &o.shapes) {
            walk_texts(&c.shape, &mut v);
        }
        v
    }

    fn texts_of(out: Option<&egui::FullOutput>) -> Vec<String> {
        all_texts(out).into_iter().map(|(s, _, _)| s).collect()
    }

    fn find_in(out: Option<&egui::FullOutput>, needle: &str) -> Option<Pos2> {
        all_texts(out).into_iter().find(|(s, _, _)| s == needle).map(|(_, p, _)| p)
    }

    #[test]
    fn overlay_starts_english_and_switches_to_german() {
        let mut o = Overlay::new("switch");
        let texts = o.texts();
        for s in [EN.settings_title, EN.language, EN.completion, EN.diagnostics, EN.signals, EN.set_project, "English", "Deutsch"] {
            assert!(texts.iter().any(|x| x == s), "{s:?} missing in {texts:?}");
        }
        assert!(!texts.iter().any(|x| x == DE.settings_title));
        assert!(o.click("Deutsch"), "{:?}", o.texts());
        assert_eq!(o.store.get().lang, Lang::De);
        o.frame(vec![]);
        assert!(o.texts().iter().any(|x| x == DE.settings_title), "{:?}", o.texts());
        assert!(o.open, "a click inside the panel keeps it open");
    }

    #[test]
    fn completion_switch_is_saved() {
        let mut o = Overlay::new("completion");
        assert!(o.click(EN.completion), "{:?}", o.texts());
        assert!(!o.store.get().completion);
        assert!(o.click(EN.completion), "{:?}", o.texts());
        assert!(o.store.get().completion);
    }

    #[test]
    fn esc_close_button_and_a_click_outside_close_it() {
        let mut o = Overlay::new("close");
        o.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        assert!(!o.open, "Esc");
        let mut o = Overlay::new("close-button");
        assert!(o.click("×"), "{:?}", o.texts());
        assert!(!o.open, "close button");
        let mut o = Overlay::new("close-outside");
        o.click_at(Pos2::new(5.0, 5.0));
        assert!(!o.open, "click outside the panel");
    }

    #[test]
    fn signals_and_log_start_folded() {
        let mut o = Overlay::new("folded");
        let texts = o.texts();
        assert!(!texts.iter().any(|x| x == "diag line" || x == EN.diag_log || x == EN.p_burst), "{texts:?}");
        assert!(!texts.iter().any(|x| x.starts_with("Signal 1")), "no slider shows: {texts:?}");
        assert!(o.click(EN.diagnostics), "{:?}", o.texts());
        let texts = o.texts();
        for s in ["diag line", EN.diag_log, EN.p_burst] {
            assert!(texts.iter().any(|x| x == s), "{s:?} missing after unfolding: {texts:?}");
        }
        // folded again, so all 32 sliders fit the window
        assert!(o.click(EN.diagnostics));
        let before = o.texts().len();
        assert!(o.click(EN.signals), "{:?}", o.texts());
        let after = o.texts().len();
        assert!(after >= before + 32, "32 slider values after unfolding: {before} -> {after}");
        assert!(o.open);
    }

    #[test]
    fn the_account_note_sits_under_the_account_settings() {
        let o = Overlay::new("account");
        let ys = [EN.keys_note, EN.set_calibration, EN.scope_note, EN.set_project].map(|s| o.find(s).map(|p| p.y));
        assert!(ys.iter().all(Option::is_some), "missing rows {ys:?}: {:?}", o.texts());
        let [keys, cal, note, project] = ys.map(|y| y.unwrap_or(0.0));
        assert!(keys < cal && cal < note && note < project, "keys {keys}, calibration {cal}, account note {note}, project {project}");
    }

    /// Settings > Face overlay: folded at first; each toggle is saved in
    /// the account settings at once, and the hint names what it shows.
    #[test]
    fn the_face_overlay_toggles_are_saved() {
        let mut o = Overlay::new("face");
        assert!(!o.texts().iter().any(|x| x == EN.fo_values), "folded: {:?}", o.texts());
        assert!(o.click(EN.face_overlay), "{:?}", o.texts());
        for s in [EN.h_head, EN.fo_eyes, EN.h_mouth, EN.fo_values, EN.fo_note] {
            assert!(o.texts().iter().any(|x| x == s), "{s:?} missing: {:?}", o.texts());
        }
        assert_eq!(o.store.get().face, settings::FaceOverlay::default());
        assert!(o.click(EN.fo_eyes));
        assert!(o.click(EN.fo_values));
        assert_eq!(o.store.get().face, settings::FaceOverlay { eyes: false, values: false, ..settings::FaceOverlay::default() });
        assert!(o.click(EN.fo_eyes));
        assert!(o.store.get().face.eyes);
        assert!(o.open);
    }

    /// The Settings form: the labels share one left edge, the controls'
    /// texts and the hints another, and every text is 15 (title), 13 or 11.
    #[test]
    fn settings_rows_share_one_label_column_and_three_sizes() {
        let o = Overlay::new("form");
        let mut texts = Vec::new();
        for c in o.out.iter().flat_map(|o| &o.shapes) {
            text_rects(&c.shape, &mut texts);
        }
        let left = |s: &str| texts.iter().find(|(t, _, _)| t == s).map(|(_, r, _)| r.left());
        let labels = [EN.language, EN.set_typing, EN.set_calibration].map(left);
        let column = [EN.set_not_calibrated, EN.language_note, EN.completion_note, EN.keys_note].map(left);
        assert!(labels.iter().chain(&column).all(Option::is_some), "{texts:?}");
        let (l0, c0) = (labels[0].unwrap_or(0.0), column[0].unwrap_or(0.0));
        assert!(labels.iter().flatten().all(|x| (x - l0).abs() < 0.5), "labels {labels:?}");
        assert!(column.iter().flatten().all(|x| (x - c0).abs() < 0.5), "controls and hints {column:?}");
        assert!(c0 > l0 + 40.0, "{l0} {c0}");
        for (t, _, size) in &texts {
            assert!([Tok::TITLE, Tok::BODY, Tok::SMALL].iter().any(|s| (size - s).abs() < 0.01), "{t:?} at {size}");
        }
    }

    /// Every text shape's string, rect and font size.
    fn text_rects(s: &egui::Shape, out: &mut Vec<(String, Rect, f32)>) {
        match s {
            egui::Shape::Text(t) => {
                let size = t.galley.job.sections.first().map_or(0.0, |s| s.format.font_id.size);
                out.push((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2()), size));
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| text_rects(s, out)),
            _ => {}
        }
    }

    /// Noa (v3 follow-up): Settings clearly smaller in the small window.
    #[test]
    fn settings_is_narrower_and_lower_than_v3s_first_cut() {
        let win = egui::vec2(WIN_W as f32, WIN_H as f32);
        let s = settings_size(win);
        assert!(s.x <= 460.0 && s.x / win.x <= 0.65, "width {}", s.x);
        assert!(s.y <= 0.8 * win.y, "height {}", s.y);
        assert!(s.x >= SETTINGS_MIN_W, "still readable: {}", s.x);
        // a big window caps the width; a tiny one keeps 16 at the sides and 24 above and below
        assert_eq!(settings_size(egui::vec2(1600.0, 1000.0)).x, SETTINGS_W);
        let tiny = settings_size(egui::vec2(300.0, 200.0));
        assert!(tiny.x <= 268.0 && tiny.y <= 152.0, "{tiny:?}");
    }

    #[test]
    fn the_panel_area_gets_its_share_and_the_camera_the_rest() {
        let (w, h) = (WIN_W as f32, WIN_H as f32);
        // no section: 30 % for the panel (more than the 146 it needs)
        assert_eq!(cam_height(w, h, false), 364.0);
        // a section open: 72 %, the camera 146 (v2: 60 % at 1200 x 820)
        assert_eq!(cam_height(w, h, true), 145.0);
        assert!((h - cam_height(w, h, true)) / h >= 0.6 * 1.2, "20 % more than v2's 60 %");
        // never more than the full frame, never under the floor while there is room
        assert_eq!(cam_height(w, 2000.0, false), cam_full(w));
        assert_eq!(cam_height(w, 400.0, true), CAM_FLOOR);
        // a tiny window keeps the panel's minimum first
        assert_eq!(cam_height(w, 200.0, true), 200.0 - PANEL_MIN);
        assert_eq!(cam_height(w, 100.0, true), 0.0);
        for h in [300.0f32, 400.0, 520.0, 700.0, 900.0] {
            for open in [false, true] {
                let cam = cam_height(w, h, open);
                assert!(cam >= 0.0 && cam <= cam_full(w), "{h} {open}: {cam}");
                assert!(h - cam >= PANEL_MIN - 0.01, "{h} {open}: the panel keeps its minimum");
            }
        }
        assert!(cam_height(300.0, 900.0, false) <= cam_full(300.0), "the floor never passes the full frame");
    }

    /// Noa: after folding or closing Hands the camera stayed small. The
    /// split follows what is visibly open: no section, or every open one
    /// folded, gives the big camera at once; one unfolded section or the
    /// suggestions the small one.
    #[test]
    fn folded_sections_give_the_camera_back() {
        let (w, h) = (WIN_W as f32, WIN_H as f32);
        let big = cam_height(w, h, false);
        let small = cam_height(w, h, true);
        assert!(big > small + 100.0, "{big} {small}");
        let cam = |v: &View, inline: bool| cam_height(w, h, panel_open(v, inline));
        // the big layout keeps the folded headers and Advanced in view
        let folded = |n: usize| cam_height_with(w, h, false, n);
        assert_eq!(folded(0), big);
        for n in 1..=3 {
            assert!(h - folded(n) >= PANEL_MIN + n as f32 * FOLDED_H - 0.01, "{n} folded: the panel shows them and Advanced");
            assert!(folded(n) > small + 50.0, "{n} folded: still the big camera, {}", folded(n));
        }
        let mut v = View::default();
        assert_eq!(cam(&v, false), big, "nothing open");
        assert_eq!(cam(&v, true), small, "the suggestions need the room");
        v.sections = vec![Section::Hands];
        assert_eq!(cam(&v, false), small, "Hands open");
        v.set_folded(Section::Hands, true);
        assert_eq!(cam(&v, false), big, "Hands folded: the camera is big again");
        v.sections.push(Section::Pong);
        assert_eq!(cam(&v, false), small, "Pong open under folded Hands");
        v.set_folded(Section::Pong, true);
        assert_eq!(cam(&v, false), big, "every open section folded");
        v.set_folded(Section::Hands, false);
        assert_eq!(cam(&v, false), small, "Hands unfolds: small again");
        v.sections.clear();
        v.folded.clear();
        assert_eq!(cam(&v, false), big, "closed: big");
    }

    #[test]
    fn the_default_window_is_40_percent_smaller_than_v2() {
        assert!((0.5..=0.62).contains(&(WIN_W as f32 / 1200.0)), "{WIN_W}");
        assert!((0.5..=0.65).contains(&(WIN_H as f32 / 820.0)), "{WIN_H}");
    }

    /// Room above the plugin picker for a header and five plugins (spec v3
    /// 2.7: the pickers open over the camera).
    #[test]
    fn the_camera_leaves_room_for_six_picker_rows() {
        let (w, h) = (WIN_W as f32, WIN_H as f32);
        let win = Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h));
        for open in [false, true] {
            let cam = cam_height(w, h, open);
            let button = top_columns(w)[0].translate(egui::vec2(0.0, cam + TOP_PAD));
            let room = glass::place(button, 200.0, 0.0, win, glass::Open::Up).max_h;
            assert!(room - 2.0 * glass::POP_PAD_PT >= 6.0 * Tok::POP_ROW_H, "{open}: room {room}");
        }
    }

    #[test]
    fn top_columns_share_one_picker_width_and_8_pt_gaps() {
        for w in [600.0, 720.0, 800.0, 1000.0, 1200.0, 1600.0] {
            let [plugin, chat, preset, plus] = top_columns(w);
            assert_eq!(plugin.width(), preset.width(), "{w}");
            assert!((PICKER_MIN_W..=PICKER_MAX_W).contains(&plugin.width()), "{w}: P {}", plugin.width());
            assert_eq!(plugin.left(), Tok::INSET);
            assert_eq!(chat.left() - plugin.right(), Tok::GAP, "{w}");
            assert_eq!(preset.left() - chat.right(), Tok::GAP, "{w}");
            assert_eq!(plus.left() - preset.right(), 4.0, "{w}");
            assert_eq!((plus.width(), plus.height()), (30.0, 30.0));
            assert!(plus.right() <= w - Tok::INSET + 0.01, "{w}: plus inside the right inset");
            assert!(chat.width() > plugin.width(), "{w}: the chat field is the widest");
            for r in [plugin, chat, preset, plus] {
                assert_eq!(r.height(), Tok::TOP_H);
            }
        }
        // the default window: pickers 137, the chat field 372
        let [plugin, chat, preset, plus] = top_columns(720.0);
        assert_eq!((plugin.left(), plugin.right()), (12.0, 149.0));
        assert_eq!((chat.left(), chat.right()), (157.0, 529.0));
        assert_eq!((preset.left(), preset.right()), (537.0, 674.0));
        assert_eq!((plus.left(), plus.right()), (678.0, 708.0));
        assert_eq!(top_columns(1600.0)[0].width(), 220.0);
    }

    fn frame_of(w: usize, h: usize, v: u8) -> Arc<Preview> {
        Arc::new(Preview { w, h, rgba: vec![v; w * h * 4] })
    }

    /// The frost's blur follows the shown frame, made only when it changed,
    /// and goes with the textures on a reopen.
    #[test]
    fn the_blur_follows_the_frame_and_resets_on_reopen() {
        let ctx = egui::Context::default();
        let mut cam = Cam::default();
        let id = cam.blur_id(&ctx);
        assert_eq!(cam.blur_id(&ctx), id, "one texture");
        cam.refresh_blur(&ctx);
        assert!(cam.blurred.is_none(), "no frame, nothing to blur");
        let a = frame_of(64, 36, 200);
        cam.upload(&ctx, Some(a.clone()));
        cam.refresh_blur(&ctx);
        assert!(cam.blurred.as_ref().is_some_and(|b| Arc::ptr_eq(b, &a)));
        assert_eq!(cam.blur.as_ref().map(|t| t.size()), Some([2, 2]), "one pixel per 32 px block");
        cam.clear();
        assert!(cam.blur.is_none() && cam.blurred.is_none());
    }

    #[test]
    fn a_frame_uploads_once() {
        let ctx = egui::Context::default();
        let mut cam = Cam::default();
        assert!(!cam.upload(&ctx, None));
        let a = frame_of(16, 9, 200);
        assert!(cam.upload(&ctx, Some(a.clone())), "the first frame uploads");
        assert!(!cam.upload(&ctx, Some(a.clone())), "the same frame does not upload again");
        assert!(needs_upload(None, &a) && !needs_upload(Some(&a), &a));
        let b = frame_of(16, 9, 10);
        assert!(cam.upload(&ctx, Some(b.clone())), "a new frame uploads");
        assert_eq!(cam.tex.as_ref().map(|t| t.size()), Some([16, 9]));
        let c = frame_of(32, 18, 10);
        assert!(cam.upload(&ctx, Some(c)), "a new size uploads");
        assert_eq!(cam.tex.as_ref().map(|t| t.size()), Some([32, 18]));
        let bad = Arc::new(Preview { w: 4, h: 4, rgba: vec![0; 3] });
        assert!(!cam.upload(&ctx, Some(bad)), "a short buffer never uploads");
    }

    #[test]
    fn a_reopen_uploads_the_current_frame_again() {
        let ctx = egui::Context::default();
        let mut cam = Cam::default();
        let a = frame_of(16, 9, 200);
        assert!(cam.upload(&ctx, Some(a.clone())));
        // the build closure on a reopen; the camera has stalled on `a`
        cam.clear();
        let ctx = egui::Context::default();
        assert!(cam.upload(&ctx, Some(a)), "the stalled frame shows at once");
        assert!(cam.tex.is_some());
    }

    /// A click on the chevron or the title folds the section; one right
    /// of the title (the header's own controls) does not.
    #[test]
    fn a_click_on_the_chevron_or_the_title_folds() {
        let ctx = egui::Context::default();
        let head = Rect::from_min_size(pos2(12.0, 40.0), vec2(600.0, Tok::HEAD_H));
        let y = head.center().y - 1.0;
        let mut hits = Vec::new();
        // a first frame lays the window out
        let _ = ctx.run(RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| fold_chevron(ui, Id::new("fold-test"), head, 120.0, true));
        });
        for x in [16.0, 70.0, 300.0] {
            let at = pos2(x, y);
            let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            let mut hit = false;
            for events in [vec![Event::PointerMoved(at), button(true)], vec![button(false)]] {
                let raw = RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 200.0))), ..Default::default() };
                let _ = ctx.run(raw, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| hit |= fold_chevron(ui, Id::new("fold-test"), head, 120.0, true));
                });
            }
            hits.push(hit);
        }
        assert_eq!(hits, [true, true, false]);
    }

    #[test]
    fn content_always_opens_its_section() {
        // a reply with a hand route while Pong is open appends Hands
        let mut v = View { sections: vec![Section::Pong], preset: Some("Pong".into()), ..Default::default() };
        assert!(grow_view(&mut v, &[Section::Hands]));
        assert_eq!(v.sections, vec![Section::Pong, Section::Hands]);
        assert_eq!(v.preset.as_deref(), Some("Pong"), "no preset is picked by it");
        assert!(!grow_view(&mut v, &[Section::Hands, Section::Pong]), "both open: no change");
        // a wall pick appends Pong, a face route Head
        let mut v = View { sections: vec![Section::Hands], preset: None, ..Default::default() };
        assert!(grow_view(&mut v, &[Section::Pong]));
        assert_eq!(v.sections, vec![Section::Hands, Section::Pong]);
        assert!(grow_view(&mut v, &[Section::Head]));
        assert_eq!(v.sections, vec![Section::Hands, Section::Pong, Section::Head]);
        // a folded section a reply asks for unfolds
        v.set_folded(Section::Hands, true);
        assert!(grow_view(&mut v, &[Section::Hands]));
        assert!(!v.is_folded(Section::Hands));
        let mut v = View::default();
        assert!(!grow_view(&mut v, &[]));
        assert!(v.sections.is_empty());
    }

    struct Bar {
        ctx: egui::Context,
        time: f64,
        out: Option<egui::FullOutput>,
    }

    impl Bar {
        fn frame(&mut self, opened: &mut Instant) -> BarOut {
            self.frame_with(opened, Vec::new(), &EN)
        }

        /// One frame at 720 x 520 with profile 2 active and profile 1
        /// holding a setup; bar 5, beat 4, so no number reads "1" to "3".
        fn frame_with(&mut self, opened: &mut Instant, events: Vec<Event>, t: &Text) -> BarOut {
            self.time += 0.1;
            let raw = RawInput { time: Some(self.time), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(720.0, 520.0))), events, ..Default::default() };
            let profiles = preset::profile::Summary { active: 1, filled: [true, true, false] };
            let info = BarInfo { live: false, profiles, key: "A minor".into(), playing: false, tempo: 140.0, bar: 5, beat: 4 };
            let mut out = BarOut::default();
            self.out = Some(self.ctx.run(raw, |ctx| {
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
                    let cam = Rect::from_min_size(Pos2::ZERO, egui::vec2(720.0, 145.0));
                    out = top_bar(ui, cam, opened, false, &info, t);
                });
            }));
            out
        }

        fn at(&self, needle: &str) -> Option<Pos2> {
            find_in(self.out.as_ref(), needle)
        }

        fn alpha(&self, needle: &str) -> Option<f32> {
            all_texts(self.out.as_ref()).into_iter().find(|(s, _, _)| s == needle).map(|(_, _, a)| a)
        }
    }

    #[test]
    fn the_top_bar_keeps_gear_and_live_visible_after_the_fade() {
        let ctx = egui::Context::default();
        glass::style(&ctx);
        let mut bar = Bar { ctx, time: 0.0, out: None };
        let mut opened = Instant::now();
        bar.frame(&mut opened);
        let shown = [EN.live, EN.settings].map(|s| bar.alpha(s).unwrap_or(0.0));
        assert!(bar.alpha("synthexis").is_some(), "shown after the open");
        assert!(shown.iter().all(|a| *a > 0.5), "{shown:?}");
        // 2.5 s later and the fade done
        opened = Instant::now().checked_sub(BAR_SHOW + Duration::from_secs(1)).unwrap_or(opened);
        for _ in 0..10 {
            bar.frame(&mut opened);
        }
        let name = bar.alpha("synthexis").unwrap_or(0.0);
        assert!(name <= 0.01, "the name faded: {name}");
        for (s, full) in [EN.live, EN.settings].into_iter().zip(shown) {
            let a = bar.alpha(s).map(|a| a / full);
            assert!(a.is_some_and(|a| (a - BAR_KEEP).abs() < 0.02), "{s:?} stays at 0.55 of its alpha: {a:?}");
        }
        assert!(bar.alpha("140").is_none_or(|a| a <= 0.01), "the tempo faded");
    }

    #[test]
    fn the_profiles_sit_left_of_the_gear_stay_after_the_fade_and_switch() {
        let ctx = egui::Context::default();
        glass::style(&ctx);
        let mut bar = Bar { ctx, time: 0.0, out: None };
        let mut opened = Instant::now();
        bar.frame(&mut opened);
        let (Some(one), Some(two), Some(three), Some(gear), Some(live)) = (bar.at("1"), bar.at("2"), bar.at("3"), bar.at(EN.settings), bar.at(EN.live)) else {
            return assert!(bar.at("1").is_some(), "{:?}", texts_of(bar.out.as_ref()));
        };
        assert!(one.x < two.x && two.x < three.x && three.x < gear.x, "1 2 3 left of the gear: {one:?} {two:?} {three:?} {gear:?}");
        // 26 pt segments, one gap to the gear's text button
        assert!(near(two.x - one.x, 26.0, 0.6) && near(three.x - two.x, 26.0, 0.6), "{one:?} {two:?} {three:?}");
        for p in [one, two, three] {
            assert!(near(p.y, gear.y, 0.6) && near(p.y, live.y, 0.6), "one line with the gear and Live: {p:?} {gear:?}");
        }
        let lit = bar.alpha("2").unwrap_or(0.0);
        let rest = bar.alpha("1").unwrap_or(1.0);
        assert!(lit > rest + 0.3, "the active one is lit: {lit} vs {rest}");
        // after the fade they stay at 0.55 like the gear
        opened = Instant::now().checked_sub(BAR_SHOW + Duration::from_secs(1)).unwrap_or(opened);
        for _ in 0..10 {
            bar.frame(&mut opened);
        }
        let after = bar.alpha("2").map(|a| a / lit);
        assert!(after.is_some_and(|a| (a - BAR_KEEP).abs() < 0.02), "profile 2 stays at 0.55: {after:?}");
        // a click on 3 asks for profile 3, a click on the active 2 for nothing
        let click = |at: Pos2| {
            let b = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            [vec![Event::PointerMoved(at)], vec![b(true)], vec![b(false)]]
        };
        let mut got = Vec::new();
        for ev in click(three) {
            got.push(bar.frame_with(&mut opened, ev, &EN).profile);
        }
        assert_eq!(got.into_iter().flatten().collect::<Vec<_>>(), [2]);
        let mut got = Vec::new();
        for ev in click(two) {
            got.push(bar.frame_with(&mut opened, ev, &EN).profile);
        }
        assert!(got.iter().all(Option::is_none), "{got:?}");
        // German keeps the numbers left of "Einstellungen"
        bar.frame_with(&mut opened, Vec::new(), &DE);
        let (Some(three), Some(gear)) = (bar.at("3"), bar.at(DE.settings)) else { return assert!(bar.at(DE.settings).is_some()) };
        assert!(three.x < gear.x);
    }

    #[test]
    fn profile_hover_texts_name_the_profile_left_and_an_empty_one() {
        let p = preset::profile::Summary { active: 1, filled: [true, true, false] };
        assert_eq!(
            profile_tips(p, &EN),
            [
                "Profile 1. Click to switch; what is set up now stays in profile 2.",
                "Profile 2: what is set up now",
                "Profile 3: empty. Click to set it up fresh; what is set up now stays in profile 2.",
            ]
        );
        assert_eq!(profile_tips(p, &DE)[2], "Profil 3: leer. Klicken, um es neu einzurichten; was jetzt eingerichtet ist, bleibt in Profil 2.");
    }

    #[test]
    fn no_stock_buttons_in_the_window() {
        let files = [
            ("editor.rs", include_str!("editor.rs")),
            ("preset/top.rs", include_str!("preset/top.rs")),
            ("sentence.rs", include_str!("sentence.rs")),
            ("fingers.rs", include_str!("fingers.rs")),
            ("pong_ui.rs", include_str!("pong_ui.rs")),
        ];
        let stock = [concat!("ui.", "button("), concat!("small_", "button(")];
        let tests = concat!("\n#[cfg(test)]\n", "mod tests");
        for (name, src) in files {
            let code = src.split(tests).next().unwrap_or(src);
            assert!(code.len() < src.len() || !src.contains("#[cfg(test)]"), "{name}: no test module found");
            for (i, line) in code.lines().enumerate() {
                for s in stock {
                    assert!(!line.contains(s), "{name}:{}: {s} in {line:?}", i + 1);
                }
            }
        }
    }

    fn near(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn crop_and_joint_mapping() {
        let cam = |w: f32, h: f32| Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h));
        // Hands open in the default window: the 145 pt camera shows wings
        let h = cam_height(720.0, 520.0, true);
        let c = crop(cam(720.0, h));
        assert!(c.wings);
        assert!(near(c.img.width(), h * ASPECT / V_MIN, 1.0), "{:?}", c.img);
        assert!(near(c.uv.height(), 0.70, 1e-4) && near(c.uv.top(), 0.105, 1e-4), "{:?}", c.uv);
        assert!(near(c.img.center().x, 360.0, 0.01));
        let top = map_joint(c.img, c.uv, [0.5, c.uv.top()]);
        let bottom = map_joint(c.img, c.uv, [0.5, c.uv.bottom()]);
        assert!(near(top.x, c.img.center().x, 0.01) && near(top.y, c.img.top(), 0.01), "{top:?}");
        assert!(near(bottom.x, c.img.center().x, 0.01) && near(bottom.y, c.img.bottom(), 0.01), "{bottom:?}");
        // no section: the full 16:9 frame
        let c = crop(cam(1200.0, cam_full(1200.0)));
        assert!(!c.wings && c.img == cam(1200.0, 675.0));
        assert!(near(c.uv.height(), 1.0, 0.001) && near(c.uv.width(), 1.0, 0.001), "{:?}", c.uv);
        // no section in the default window: a little cut, no wings
        let c = crop(cam(720.0, cam_height(720.0, 520.0, false)));
        assert!(!c.wings && near(c.uv.height(), 364.0 / 405.0, 0.002), "{:?}", c.uv);
        let c = crop(cam(800.0, 600.0));
        assert!(!c.wings && near(c.uv.width(), 0.75, 1e-4) && near(c.uv.height(), 1.0, 1e-6), "{:?}", c.uv);
    }

    #[test]
    fn the_contain_fit_keeps_the_whole_frame_for_wide_and_tall_rects() {
        let full = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        let pong_cam = cam_height(720.0, 520.0, true);
        for (w, h, wings) in [(720.0, pong_cam, true), (1200.0, 675.0, false), (400.0, 900.0, true), (800.0, 300.0, true)] {
            let cam = Rect::from_min_size(Pos2::new(10.0, 20.0), egui::vec2(w, h));
            let c = contain(cam);
            assert_eq!(c.uv, full, "{w}x{h}: the whole frame");
            assert!(near(c.img.width() / c.img.height(), ASPECT, 1e-3), "{w}x{h}: 16:9, {:?}", c.img);
            assert!(cam.expand(0.01).contains_rect(c.img), "{w}x{h}: inside");
            assert!(near(c.img.center().x, cam.center().x, 0.01) && near(c.img.center().y, cam.center().y, 0.01));
            // the largest: it touches two opposite edges
            assert!(near(c.img.width(), w, 0.01) || near(c.img.height(), h, 0.01), "{w}x{h}: {:?}", c.img);
            assert_eq!(c.wings, wings, "{w}x{h}");
        }
    }

    /// Hands or Head open look like Pong: the whole frame. No section, or
    /// every open one folded, keeps the big cover view; Pong playing
    /// always shows the whole frame (its walls sit on the frame's edge).
    #[test]
    fn the_fit_follows_what_is_visibly_open() {
        let view = |open: &[Section], folded: &[Section]| View { sections: open.to_vec(), folded: folded.to_vec(), ..View::default() };
        let cases: [(&str, View, bool, Fit); 11] = [
            ("none", view(&[], &[]), false, Fit::Cover),
            ("Hands", view(&[Section::Hands], &[]), false, Fit::Contain),
            ("Head", view(&[Section::Head], &[]), false, Fit::Contain),
            ("Pong section", view(&[Section::Pong], &[]), false, Fit::Contain),
            ("Pong playing", view(&[Section::Pong], &[]), true, Fit::Contain),
            ("Hands+Head", view(&[Section::Hands, Section::Head], &[]), false, Fit::Contain),
            ("Hands folded", view(&[Section::Hands], &[Section::Hands]), false, Fit::Cover),
            ("Hands+Head folded", view(&[Section::Hands, Section::Head], &[Section::Hands, Section::Head]), false, Fit::Cover),
            ("Head open under folded Hands", view(&[Section::Hands, Section::Head], &[Section::Hands]), false, Fit::Contain),
            ("Pong folded, playing", view(&[Section::Pong], &[Section::Pong]), true, Fit::Contain),
            ("nothing open, playing", view(&[], &[]), true, Fit::Contain),
        ];
        for (name, v, pong_on, want) in cases {
            assert_eq!(fit_for(&v, pong_on), want, "{name}");
        }
        // in the default window: Hands shows the whole frame, smaller, with
        // wings; no section keeps the big cover view without wings
        let (w, h) = (WIN_W as f32, WIN_H as f32);
        let hands = view(&[Section::Hands], &[]);
        let cam = Rect::from_min_size(Pos2::ZERO, vec2(w, cam_height(w, h, panel_open(&hands, false))));
        let c = fit_for(&hands, false).place(cam);
        assert!(c.wings && c.uv == Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), "{c:?}");
        assert!(near(c.img.width() / c.img.height(), ASPECT, 1e-3) && near(c.img.height(), cam.height(), 0.01), "{c:?}");
        assert_eq!(c, contain(cam), "the same place Pong gets");
        let none = View::default();
        let big = Rect::from_min_size(Pos2::ZERO, vec2(w, cam_height(w, h, panel_open(&none, false))));
        let c = fit_for(&none, false).place(big);
        assert!(!c.wings && c.img == big && c.uv.height() < 1.0, "{c:?}");
        assert_eq!(c, crop(big));
    }

    /// The inverse of `map_joint`: window points back into the preview.
    fn unmap(img: Rect, uv: Rect, p: Pos2) -> [f32; 2] {
        let q = uv.min + ((p - img.min) / img.size()) * uv.size();
        [q.x, q.y]
    }

    /// Both fits: preview points go to window points and back, the shown
    /// part's corners land on the image's corners, and the face boxes and
    /// Pong's arena use the same map (`crt_rects` through `img`/`uv`).
    #[test]
    fn both_fits_map_preview_points_round_trip() {
        let cams = [
            Rect::from_min_size(pos2(0.0, 0.0), vec2(720.0, cam_height(720.0, 520.0, true))),
            Rect::from_min_size(pos2(0.0, 0.0), vec2(720.0, cam_height(720.0, 520.0, false))),
            Rect::from_min_size(pos2(10.0, 20.0), vec2(800.0, 600.0)),
            Rect::from_min_size(pos2(10.0, 20.0), vec2(1200.0, 300.0)),
        ];
        for cam in cams {
            for fit in [Fit::Cover, Fit::Contain] {
                let c = fit.place(cam);
                let (img, uv) = (c.img, c.uv);
                assert!(cam.expand(0.01).contains_rect(img), "{fit:?} {cam:?}: {img:?}");
                for p in [[0.0, 0.0], [1.0, 1.0], [0.5, 0.5], [0.13, 0.87], [0.9, 0.2]] {
                    let back = unmap(img, uv, map_joint(img, uv, p));
                    assert!(near(back[0], p[0], 1e-4) && near(back[1], p[1], 1e-4), "{fit:?} {cam:?}: {p:?} -> {back:?}");
                }
                assert!(map_joint(img, uv, [uv.left(), uv.top()]).distance(img.min) < 0.01, "{fit:?} {cam:?}");
                assert!(map_joint(img, uv, [uv.right(), uv.bottom()]).distance(img.max) < 0.01, "{fit:?} {cam:?}");
                if fit == Fit::Contain {
                    // the whole frame: its corners are the image's
                    assert!(map_joint(img, uv, [0.0, 0.0]).distance(img.min) < 0.01 && map_joint(img, uv, [1.0, 1.0]).distance(img.max) < 0.01);
                }
                // the face box goes through the same map (4 % round)
                let b = [0.3, 0.25, 0.6, 0.7];
                let g = crate::engine::geom::FaceGeom { face: Some(b), ..Default::default() };
                let face = crate::head_ui::crt_rects(&g, img, uv)[0].unwrap_or(Rect::NOTHING);
                let want = Rect::from_two_pos(map_joint(img, uv, [b[0], b[1]]), map_joint(img, uv, [b[2], b[3]]));
                assert!(face.center().distance(want.center()) < 0.01 && near(face.width(), want.width() * 1.08, 0.01), "{fit:?} {cam:?}: {face:?} {want:?}");
            }
        }
    }
}
