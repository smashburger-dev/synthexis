//! Minimal functional egui editor (glass styling is a later phase):
//! camera preview, 32 macro bars, Live/Test toggles, hand-role combos,
//! latency line, the mapping assistant's sentence field with completion
//! (sentence.rs) and a settings section. Texture reset on every open
//! (probe v0.2.1 fix).

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::egui::{self, Color32, ColorImage, TextureHandle};
use nih_plug_egui::{create_egui_editor, widgets};

use crate::engine::camera::EngineShared;
use crate::params::{HandRole, SynthexisParams};
use crate::sentence::Sentence;

pub static EDITOR_OPEN: AtomicBool = AtomicBool::new(false);

struct UiState {
    params: Arc<SynthexisParams>,
    shared: Arc<EngineShared>,
    texture: Option<TextureHandle>,
    sentence: Sentence,
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

fn role_combo(
    ui: &mut egui::Ui, setter: &ParamSetter, label: &str,
    param: &EnumParam<HandRole>,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(label)
            .selected_text(match param.value() {
                HandRole::Midi => "MIDI",
                HandRole::Parameter => "Parameter",
                HandRole::Motion => "Motion",
                HandRole::Off => "Off",
            })
            .show_ui(ui, |ui| {
                for (v, t) in [
                    (HandRole::Midi, "MIDI"),
                    (HandRole::Parameter, "Parameter"),
                    (HandRole::Motion, "Motion"),
                    (HandRole::Off, "Off"),
                ] {
                    if ui.selectable_label(param.value() == v, t).clicked() {
                        setter.begin_set_parameter(param);
                        setter.set_parameter(param, v);
                        setter.end_set_parameter(param);
                    }
                }
            });
    });
}

pub fn make_editor(
    params: Arc<SynthexisParams>,
    shared: Arc<EngineShared>,
) -> Option<Box<dyn Editor>> {
    let state = params.editor_state.clone();
    let inner = create_egui_editor(
        state,
        UiState { params, shared, texture: None, sentence: Sentence::new(Some(crate::fl::bridge())) },
        // Runs on every open: the egui Context is new each time but UiState
        // persists, so drop the stale texture handle.
        |_, s| {
            s.texture = None;
        },
        move |ctx, setter, s| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("synthexis");
                ui.horizontal(|ui| {
                    let mut live = s.params.live.value();
                    if ui.checkbox(&mut live, "Live").changed() {
                        setter.begin_set_parameter(&s.params.live);
                        setter.set_parameter(&s.params.live, live);
                        setter.end_set_parameter(&s.params.live);
                    }
                    let mut test = s.params.test_signal.value();
                    if ui.checkbox(&mut test, "Test signal").changed() {
                        setter.begin_set_parameter(&s.params.test_signal);
                        setter.set_parameter(&s.params.test_signal, test);
                        setter.end_set_parameter(&s.params.test_signal);
                    }
                    role_combo(ui, setter, "Hand L", &s.params.hand_role_left);
                    role_combo(ui, setter, "Hand R", &s.params.hand_role_right);
                });

                if let Some(frame) = s.shared.preview.lock().clone() {
                    let tex = s.texture.get_or_insert_with(|| {
                        ctx.load_texture(
                            "cam",
                            ColorImage::new([320, 180], Color32::BLACK),
                            Default::default(),
                        )
                    });
                    tex.set(
                        ColorImage::from_rgba_unmultiplied([320, 180], frame.as_ref()),
                        Default::default(),
                    );
                    ui.image((tex.id(), egui::vec2(320.0, 180.0)));
                }
                ui.monospace(format!(
                    "auth: {}  frames: {}  diag: {}",
                    s.shared.auth_status.lock(),
                    s.shared.frame_count.load(Ordering::Relaxed),
                    s.shared.diag.lock(),
                ));

                ui.separator();
                ui.label("Mapping assistant");
                let completion = s.params.completion.load(Ordering::Relaxed);
                s.sentence.ui(ui, completion, &s.params.target_pin);
                ui.collapsing("Settings", |ui| {
                    let mut on = completion;
                    if ui.checkbox(&mut on, "Completion in the sentence field").changed() {
                        s.params.completion.store(on, Ordering::Relaxed);
                    }
                });

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
                ui.separator();
                ui.label("log:");
                for line in crate::log::last_lines(8) {
                    ui.monospace(line);
                }
            });
        },
    )?;
    Some(Box::new(Wrap { inner }))
}
