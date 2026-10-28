//! egui editor plus an intercepting `Editor` wrapper that stashes the
//! `Arc<dyn GuiContext>` when the editor opens, so the camera thread can keep
//! automating Signal 1 after the editor is closed (probe P2).

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::egui::{self, Color32, ColorImage, TextureHandle};
use nih_plug_egui::{create_egui_editor, widgets};

use crate::camera::CaptureShared;
use crate::{probe_log, Drive, Gesture, ProbeParams, Source, WritePath};

pub static EDITOR_OPEN: AtomicBool = AtomicBool::new(false);

pub struct EditorUiState {
    params: Arc<ProbeParams>,
    shared: Arc<CaptureShared>,
    texture: Option<TextureHandle>,
}

struct InterceptingEditor {
    inner: Box<dyn Editor>,
}

struct EditorHandle {
    inner: Box<dyn Any + Send>,
}

impl Drop for EditorHandle {
    fn drop(&mut self) {
        EDITOR_OPEN.store(false, Ordering::Release);
    }
}

impl Editor for InterceptingEditor {
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn Any + Send> {
        *crate::gui_context_slot().lock() = Some(context.clone());
        EDITOR_OPEN.store(true, Ordering::Release);
        probe_log::log("editor opened, GuiContext captured");
        Box::new(EditorHandle {
            inner: self.inner.spawn(parent, context),
        })
    }

    fn size(&self) -> (u32, u32) {
        self.inner.size()
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        self.inner.set_scale_factor(factor)
    }

    fn param_value_changed(&self, id: &str, normalized_value: f32) {
        self.inner.param_value_changed(id, normalized_value)
    }

    fn param_modulation_changed(&self, id: &str, modulation_offset: f32) {
        self.inner.param_modulation_changed(id, modulation_offset)
    }

    fn param_values_changed(&self) {
        self.inner.param_values_changed()
    }
}

/// Combo box for an `EnumParam`, same pattern as the Drive selector.
fn enum_combo<E: Enum + PartialEq + Copy + 'static>(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    id: &str,
    label: &str,
    param: &EnumParam<E>,
    variants: &[(E, &'static str)],
) {
    ui.label(label);
    egui::ComboBox::from_id_salt(id)
        .selected_text(
            variants
                .iter()
                .find(|(v, _)| *v == param.value())
                .map(|(_, l)| *l)
                .unwrap_or("?"),
        )
        .show_ui(ui, |ui| {
            for (v, text) in variants {
                if ui.selectable_label(param.value() == *v, *text).clicked() {
                    setter.begin_set_parameter(param);
                    setter.set_parameter(param, *v);
                    setter.end_set_parameter(param);
                }
            }
        });
}

pub fn make_editor(
    params: Arc<ProbeParams>,
    shared: Arc<CaptureShared>,
) -> Option<Box<dyn Editor>> {
    let egui_state = params.editor_state.clone();
    let inner = create_egui_editor(
        egui_state.clone(),
        EditorUiState {
            params,
            shared,
            texture: None,
        },
        // Runs on every editor open. The egui Context is recreated per open
        // while this user state persists (nih_plug_egui::editor::EguiEditor
        // keeps user_state across openings), so any TextureHandle from a
        // previous open points at a texture id the new Context does not
        // have -> drop it and let the update closure recreate it.
        |_, state| {
            state.texture = None;
        },
        move |egui_ctx, setter, state| {
            egui::CentralPanel::default().show(egui_ctx, |ui| {
                ui.heading("synthexis probe");
                let st = state.shared.stats.lock();
                let (lat_med, lat_p95) = crate::camera::Stats::median_p95(&st.latencies_ms);
                let (int_med, int_p95) =
                    crate::camera::Stats::median_p95(&st.intervals_ms);
                let fps = st.delivered_fps();
                drop(st);
                ui.label(format!(
                    "camera auth: {}   frames: {}   fps: {:.1}",
                    state.shared.auth_status.lock().clone(),
                    state.shared.frame_count.load(Ordering::Relaxed),
                    fps
                ));
                ui.label(format!(
                    "frame interval ms: med {:.1} p95 {:.1}   capture->callback ms: med {:.2} p95 {:.2}",
                    int_med, int_p95, lat_med, lat_p95
                ));
                ui.label(format!(
                    "luminance (central 25%): {:.3}   dropped: {}",
                    state.shared.luminance(),
                    state.shared.drop_count.load(Ordering::Relaxed)
                ));
                ui.monospace(state.shared.diag_line.lock().clone());
                ui.monospace(crate::pump::last_write_line());

                ui.separator();
                ui.label("Signal 1 (set from camera when Drive != Off):");
                ui.add(widgets::ParamSlider::for_param(
                    &state.params.signal_1,
                    setter,
                ));

                ui.label("Drive:");
                egui::ComboBox::from_id_salt("drive")
                    .selected_text(match state.params.drive.value() {
                        Drive::Off => "Off",
                        Drive::EditorOnly => "Editor open only",
                        Drive::Always => "Always",
                    })
                    .show_ui(ui, |ui| {
                        for (v, label) in [
                            (Drive::Off, "Off"),
                            (Drive::EditorOnly, "Editor open only"),
                            (Drive::Always, "Always"),
                        ] {
                            if ui
                                .selectable_label(state.params.drive.value() == v, label)
                                .clicked()
                            {
                                setter.begin_set_parameter(&state.params.drive);
                                setter.set_parameter(&state.params.drive, v);
                                setter.end_set_parameter(&state.params.drive);
                            }
                        }
                    });

                enum_combo(ui, setter, "source", "Source:", &state.params.source, &[
                    (Source::Luminance, "Luminance"),
                    (Source::Triangle, "Triangle"),
                ]);
                enum_combo(ui, setter, "path", "Path:", &state.params.path, &[
                    (WritePath::Background, "A: background thread"),
                    (WritePath::Main, "B: main thread"),
                    (WritePath::MidiCc, "D: MIDI CC"),
                ]);
                enum_combo(ui, setter, "gesture", "Gesture:", &state.params.gesture, &[
                    (Gesture::PerWrite, "Per write"),
                    (Gesture::Held, "Held"),
                ]);

                let mut midi = state.params.midi_test.value();
                if ui.checkbox(&mut midi, "MIDI test (note 60 on beats)").changed() {
                    setter.begin_set_parameter(&state.params.midi_test);
                    setter.set_parameter(&state.params.midi_test, midi);
                    setter.end_set_parameter(&state.params.midi_test);
                }

                ui.separator();
                // Camera preview, updated at most ~15 fps.
                if let Some(frame) = state.shared.preview.lock().clone() {
                    let tex = state.texture.get_or_insert_with(|| {
                        egui_ctx.load_texture(
                            "camera",
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

                ui.separator();
                ui.label("log:");
                for line in probe_log::last_lines(10) {
                    ui.monospace(line);
                }
            });
        },
    )?;
    Some(Box::new(InterceptingEditor { inner }))
}
