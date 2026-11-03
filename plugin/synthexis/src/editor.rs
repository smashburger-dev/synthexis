//! Minimal functional egui editor (glass styling is a later phase).
//! Main view: Live/Test toggles, hand-role combos, camera preview with its
//! status in words, the mapping assistant's sentence field with completion
//! (sentence.rs), 32 macro bars. The "Settings" button opens a frosted
//! overlay over the whole window: language, completion on/off and
//! diagnostics (camera numbers and the log); Esc, the close button or a
//! click outside the panel close it. Every string comes from i18n.rs in
//! the per-user language (settings.rs). Texture reset on every open (probe
//! v0.2.1 fix).

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use nih_plug::prelude::*;
use nih_plug_egui::egui::{self, Color32, ColorImage, Id, Key, Order, RichText, Sense, TextureHandle, UiBuilder};
use nih_plug_egui::{create_egui_editor, widgets};
use synthexis_complete::Lang;

use crate::engine::camera::{median_p95, EngineShared};
use crate::i18n::{self, fill, Text};
use crate::params::{HandRole, SynthexisParams};
use crate::sentence::Sentence;
use crate::settings;

pub static EDITOR_OPEN: AtomicBool = AtomicBool::new(false);

const PREVIEW: [usize; 2] = [320, 180];
/// The frosted preview: one pixel per FROST x FROST block, drawn back at
/// full size with linear filtering.
const FROST: usize = 8;
const FADE_SECS: f32 = 0.15;
/// No camera frame for this long: the camera sends no picture.
const STALE: Duration = Duration::from_secs(2);
const LOG_LINES: usize = 40;

struct UiState {
    params: Arc<SynthexisParams>,
    shared: Arc<EngineShared>,
    texture: Option<TextureHandle>,
    frost: Option<TextureHandle>,
    sentence: Sentence,
    settings_open: bool,
}

struct EditorHandle {
    _inner: Box<dyn Any + Send>,
}

impl Drop for EditorHandle {
    fn drop(&mut self) {
        EDITOR_OPEN.store(false, Ordering::Release);
    }
}

struct Wrap {
    inner: Box<dyn Editor>,
}

impl Editor for Wrap {
    fn spawn(&self, p: ParentWindowHandle, c: Arc<dyn GuiContext>) -> Box<dyn Any + Send> {
        EDITOR_OPEN.store(true, Ordering::Release);
        Box::new(EditorHandle { _inner: self.inner.spawn(p, c) })
    }
    fn size(&self) -> (u32, u32) { self.inner.size() }
    fn set_scale_factor(&self, f: f32) -> bool { self.inner.set_scale_factor(f) }
    fn param_value_changed(&self, id: &str, v: f32) { self.inner.param_value_changed(id, v) }
    fn param_modulation_changed(&self, id: &str, v: f32) { self.inner.param_modulation_changed(id, v) }
    fn param_values_changed(&self) { self.inner.param_values_changed() }
}

fn role_name(t: &Text, r: HandRole) -> &'static str {
    match r {
        HandRole::Midi => t.role_midi,
        HandRole::Parameter => t.role_parameter,
        HandRole::Motion => t.role_motion,
        HandRole::Off => t.role_off,
    }
}

fn role_combo(ui: &mut egui::Ui, setter: &ParamSetter, label: &str, param: &EnumParam<HandRole>, t: &Text) {
    ui.horizontal(|ui| {
        ui.label(label);
        // the id stays the same in both languages
        egui::ComboBox::from_id_salt(param.name())
            .selected_text(role_name(t, param.value()))
            .show_ui(ui, |ui| {
                for v in [HandRole::Midi, HandRole::Parameter, HandRole::Motion, HandRole::Off] {
                    if ui.selectable_label(param.value() == v, role_name(t, v)).clicked() {
                        setter.begin_set_parameter(param);
                        setter.set_parameter(param, v);
                        setter.end_set_parameter(param);
                    }
                }
            });
    });
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

/// The numbers behind the camera status, for the diagnostics section.
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

/// The preview averaged over FROST x FROST blocks.
fn frosted(rgba: &[u8]) -> ColorImage {
    let (w, h) = (PREVIEW[0] / FROST, PREVIEW[1] / FROST);
    let mut out = Vec::with_capacity(w * h * 4);
    for by in 0..h {
        for bx in 0..w {
            let mut acc = [0u32; 3];
            for y in by * FROST..(by + 1) * FROST {
                for x in bx * FROST..(bx + 1) * FROST {
                    let p = (y * PREVIEW[0] + x) * 4;
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += u32::from(rgba[p + c]);
                    }
                }
            }
            let n = (FROST * FROST) as u32;
            out.extend_from_slice(&[(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, 255]);
        }
    }
    ColorImage::from_rgba_unmultiplied([w, h], &out)
}

/// The settings overlay: a dimmed, frosted layer over the whole window
/// with a panel in the middle. Returns false once the user closed it.
fn settings_overlay(ctx: &egui::Context, store: &settings::Store, t: &Text, diag: &[String]) -> bool {
    let id = Id::new("synthexis-settings");
    let fade = ctx.animate_bool_with_time(id.with("fade"), true, FADE_SECS);
    let screen = ctx.screen_rect();
    let mut keep = !ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
    egui::Area::new(id).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        ui.multiply_opacity(fade);
        let bg = ui.visuals().panel_fill;
        ui.painter().rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), 200));
        // takes every click outside the panel
        let scrim = ui.interact(screen, id.with("scrim"), Sense::click());
        let width = (screen.width() - 32.0).min(480.0);
        let panel = egui::Rect::from_center_size(screen.center(), egui::vec2(width, screen.height() - 48.0));
        let mut drawn = panel;
        ui.scope_builder(UiBuilder::new().max_rect(panel), |ui| {
            let frame = egui::Frame::window(ui.style()).show(ui, |ui| {
                ui.set_width(width - 32.0);
                ui.horizontal(|ui| {
                    ui.heading(t.settings_title);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕").on_hover_text(t.close).clicked() {
                            keep = false;
                        }
                    });
                });
                ui.add_space(6.0);
                let mut s = store.get();
                ui.label(RichText::new(t.language).strong());
                ui.horizontal(|ui| {
                    for (lang, name) in [(Lang::En, "English"), (Lang::De, "Deutsch")] {
                        ui.radio_value(&mut s.lang, lang, name);
                    }
                });
                ui.label(RichText::new(t.language_note).weak());
                ui.add_space(8.0);
                ui.checkbox(&mut s.completion, t.completion);
                ui.label(RichText::new(t.completion_note).weak());
                store.update(|x| *x = s);
                ui.add_space(4.0);
                ui.label(RichText::new(t.scope_note).weak().small());
                ui.separator();
                ui.label(RichText::new(t.diagnostics).strong());
                for line in diag {
                    ui.monospace(line);
                }
                ui.add_space(4.0);
                ui.label(RichText::new(t.diag_log).weak());
                egui::ScrollArea::vertical()
                    .max_height(ui.available_height().max(80.0))
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in crate::log::last_lines(LOG_LINES) {
                            ui.label(RichText::new(line).monospace().small());
                        }
                    });
            });
            drawn = frame.response.rect;
        });
        let outside = ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| !drawn.contains(p));
        if scrim.clicked() && outside {
            keep = false;
        }
    });
    keep
}

pub fn make_editor(
    params: Arc<SynthexisParams>,
    shared: Arc<EngineShared>,
) -> Option<Box<dyn Editor>> {
    let state = params.editor_state.clone();
    let inner = create_egui_editor(
        state,
        UiState {
            params,
            shared,
            texture: None,
            frost: None,
            sentence: Sentence::new(Some(crate::fl::bridge())),
            settings_open: false,
        },
        // Runs on every open: the egui Context is new each time but UiState
        // persists, so drop the stale texture handles.
        |_, s| {
            s.texture = None;
            s.frost = None;
        },
        move |ctx, setter, s| {
            let store = settings::store();
            let cfg = store.get();
            let t = i18n::text(cfg.lang);
            egui::CentralPanel::default().show(ctx, |ui| {
                // under the overlay nothing takes input; the main view dims
                ui.add_enabled_ui(!s.settings_open, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading("synthexis");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(t.settings).clicked() {
                                s.settings_open = true;
                                if let Some(f) = ui.memory(|m| m.focused()) {
                                    ui.memory_mut(|m| m.surrender_focus(f));
                                }
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        let mut live = s.params.live.value();
                        if ui.checkbox(&mut live, t.live).changed() {
                            setter.begin_set_parameter(&s.params.live);
                            setter.set_parameter(&s.params.live, live);
                            setter.end_set_parameter(&s.params.live);
                        }
                        let mut test = s.params.test_signal.value();
                        if ui.checkbox(&mut test, t.test_signal).changed() {
                            setter.begin_set_parameter(&s.params.test_signal);
                            setter.set_parameter(&s.params.test_signal, test);
                            setter.end_set_parameter(&s.params.test_signal);
                        }
                        role_combo(ui, setter, t.hand_left, &s.params.hand_role_left, t);
                        role_combo(ui, setter, t.hand_right, &s.params.hand_role_right, t);
                    });

                    if let Some(frame) = s.shared.preview.lock().clone() {
                        let size = egui::vec2(PREVIEW[0] as f32, PREVIEW[1] as f32);
                        if s.settings_open {
                            let img = frosted(&frame);
                            let tex = s.frost.get_or_insert_with(|| ctx.load_texture("cam-frost", img.clone(), egui::TextureOptions::LINEAR));
                            tex.set(img, egui::TextureOptions::LINEAR);
                            ui.image((tex.id(), size));
                        } else {
                            let tex = s.texture.get_or_insert_with(|| {
                                ctx.load_texture("cam", ColorImage::new(PREVIEW, Color32::BLACK), Default::default())
                            });
                            tex.set(ColorImage::from_rgba_unmultiplied(PREVIEW, frame.as_ref()), Default::default());
                            ui.image((tex.id(), size));
                        }
                    }
                    let (cam, problem) = camera_status(t, &s.shared);
                    if problem {
                        ui.colored_label(ui.visuals().warn_fg_color, cam);
                    } else {
                        ui.weak(cam);
                    }

                    ui.separator();
                    ui.label(t.assistant);
                    s.sentence.ui(ui, cfg.completion && !s.settings_open, &s.params.target_pin, cfg.lang);

                    ui.separator();
                    for i in 0..8 {
                        ui.horizontal(|ui| {
                            for c in 0..4 {
                                let idx = i * 4 + c;
                                let param = s.params.signals()[idx];
                                ui.vertical(|ui| {
                                    ui.add(widgets::ParamSlider::for_param(param, setter));
                                });
                            }
                        });
                    }
                });
            });
            if s.settings_open {
                let diag = diagnostics(t, &s.shared);
                s.settings_open = settings_overlay(ctx, store, t, &diag);
            }
        },
    )?;
    Some(Box::new(Wrap { inner }))
}

#[cfg(test)]
mod tests {
    use nih_plug_egui::egui::{Event, Modifiers, Pos2, RawInput, Rect};

    use super::*;

    struct Overlay {
        ctx: egui::Context,
        store: settings::Store,
        open: bool,
        out: Option<egui::FullOutput>,
    }

    impl Overlay {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("synthexis-overlay-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let store = settings::Store::open(dir.join("settings.json"));
            let mut o = Self { ctx: egui::Context::default(), store, open: true, out: None };
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
            let raw = RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(700.0, 760.0))),
                ..Default::default()
            };
            let (store, open) = (&self.store, &mut self.open);
            self.out = Some(self.ctx.run(raw, |ctx| {
                if *open {
                    let t = i18n::text(store.get().lang);
                    *open = settings_overlay(ctx, store, t, &["diag line".into()]);
                }
            }));
        }

        fn texts(&self) -> Vec<String> {
            fn walk(s: &egui::Shape, out: &mut Vec<String>) {
                match s {
                    egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            let mut out = Vec::new();
            for c in self.out.iter().flat_map(|o| &o.shapes) {
                walk(&c.shape, &mut out);
            }
            out
        }

        fn find(&self, needle: &str) -> Option<Pos2> {
            fn walk(s: &egui::Shape, needle: &str) -> Option<Pos2> {
                match s {
                    egui::Shape::Text(t) if t.galley.text() == needle => Some(t.pos + t.galley.rect.center().to_vec2()),
                    egui::Shape::Vec(v) => v.iter().find_map(|s| walk(s, needle)),
                    _ => None,
                }
            }
            self.out.as_ref()?.shapes.iter().find_map(|c| walk(&c.shape, needle))
        }

        fn click_at(&mut self, at: Pos2) {
            let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            self.frame(vec![Event::PointerMoved(at), button(true)]);
            self.frame(vec![button(false)]);
            self.settle();
        }

        fn click(&mut self, needle: &str) {
            let at = self.find(needle).unwrap_or_else(|| panic!("{needle:?} not drawn in {:?}", self.texts()));
            self.click_at(at);
        }
    }

    #[test]
    fn overlay_starts_english_and_switches_to_german() {
        let mut o = Overlay::new("switch");
        let texts = o.texts();
        for s in [EN.settings_title, EN.language, EN.completion, EN.diagnostics, "English", "Deutsch", "diag line"] {
            assert!(texts.iter().any(|x| x == s), "{s:?} missing in {texts:?}");
        }
        assert!(!texts.iter().any(|x| x == DE.settings_title));
        o.click("Deutsch");
        assert_eq!(o.store.get().lang, Lang::De);
        o.frame(vec![]);
        assert!(o.texts().iter().any(|x| x == DE.settings_title), "{:?}", o.texts());
        assert!(o.open, "a click inside the panel keeps it open");
    }

    #[test]
    fn completion_switch_is_saved() {
        let mut o = Overlay::new("completion");
        o.click(EN.completion);
        assert!(!o.store.get().completion);
        o.click(EN.completion);
        assert!(o.store.get().completion);
    }

    #[test]
    fn esc_close_button_and_a_click_outside_close_it() {
        let mut o = Overlay::new("close");
        o.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        assert!(!o.open, "Esc");
        let mut o = Overlay::new("close-button");
        o.click("✕");
        assert!(!o.open, "close button");
        let mut o = Overlay::new("close-outside");
        o.click_at(Pos2::new(5.0, 5.0));
        assert!(!o.open, "click outside the panel");
    }

    use crate::i18n::{DE, EN};

    #[test]
    fn frosted_preview_averages_blocks() {
        let mut rgba = vec![0u8; PREVIEW[0] * PREVIEW[1] * 4];
        for px in rgba.chunks_mut(4).take(FROST) {
            px.copy_from_slice(&[255, 255, 255, 255]);
        }
        let img = frosted(&rgba);
        assert_eq!(img.size, [PREVIEW[0] / FROST, PREVIEW[1] / FROST]);
        // one white row of 8 in a block of 64 pixels
        assert_eq!(img.pixels[0].r(), (255 * FROST / (FROST * FROST)) as u8);
        assert_eq!(img.pixels[1].r(), 0);
    }
}
