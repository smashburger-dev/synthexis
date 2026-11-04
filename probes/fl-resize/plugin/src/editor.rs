//! The probe window and an `Editor` wrapper that keeps the host's NSView
//! (to measure what the host really shows) and logs every new answer of
//! `Editor::size()` (to show vendor patch 2 at work inside the host).

use std::any::Any;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use nih_plug::prelude::*;
use nih_plug_egui::egui::{self, Color32, RichText};
use nih_plug_egui::{create_egui_editor, EguiState};
use parking_lot::Mutex;

use crate::log::log;
use crate::mac::{host_geometry, HostGeometry};
use crate::probe::{Track, Verdict};
use crate::START;

/// The three requests of the ui-redesign spec (2.8): width 1200, heights 720,
/// 820 (the planned default) and 900 (the cap).
pub const ASKS: [(u32, u32); 3] = [(1200, 720), (1200, 820), (1200, 900)];

#[derive(Default)]
struct Shared {
    /// The host's `NSView *` while the editor is open, else 0.
    parent: AtomicUsize,
    /// The last answer of `size()` that was logged.
    last_size: Mutex<Option<(u32, u32)>>,
}

struct ProbeEditor {
    inner: Box<dyn Editor>,
    shared: Arc<Shared>,
}

struct ProbeHandle {
    shared: Arc<Shared>,
    // Dropped after `drop` below cleared the parent pointer.
    _inner: Box<dyn Any + Send>,
}

impl Drop for ProbeHandle {
    fn drop(&mut self) {
        self.shared.parent.store(0, Ordering::Release);
        log("editor closed");
    }
}

impl Editor for ProbeEditor {
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn Any + Send> {
        let view = match parent {
            ParentWindowHandle::AppKitNsView(view) => view as usize,
            _ => 0,
        };
        log(&format!(
            "editor opened api={:?} size()={:?}",
            context.plugin_api(),
            self.inner.size()
        ));
        let inner = self.inner.spawn(parent, context);
        self.shared.parent.store(view, Ordering::Release);
        Box::new(ProbeHandle {
            shared: self.shared.clone(),
            _inner: inner,
        })
    }

    fn size(&self) -> (u32, u32) {
        let size = self.inner.size();
        let mut last = self.shared.last_size.lock();
        if *last != Some(size) {
            *last = Some(size);
            log(&format!("size() -> {} x {}", size.0, size.1));
        }
        size
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        log(&format!("set_scale_factor({factor})"));
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

/// One request while it runs.
struct Run {
    asked: (u32, u32),
    at: Instant,
    egui: Track,
    host: Track,
}

/// One finished request.
struct Row {
    asked: (u32, u32),
    egui: Verdict,
    host: Verdict,
}

struct State {
    egui_state: Arc<EguiState>,
    shared: Arc<Shared>,
    queue: VecDeque<(u32, u32)>,
    run: Option<Run>,
    rows: Vec<Row>,
    auto: bool,
    exit_when_done: bool,
    started: bool,
    /// The window number last written to the log.
    logged_window: Option<isize>,
}

fn env_on(key: &str) -> bool {
    std::env::var(key).map(|v| v == "1").unwrap_or(false)
}

pub fn make_editor(egui_state: Arc<EguiState>) -> Option<Box<dyn Editor>> {
    let shared = Arc::new(Shared::default());
    let state = State {
        egui_state: egui_state.clone(),
        shared: shared.clone(),
        queue: VecDeque::new(),
        run: None,
        rows: Vec::new(),
        auto: env_on("FL_RESIZE_PROBE_AUTO"),
        exit_when_done: env_on("FL_RESIZE_PROBE_EXIT"),
        started: false,
        logged_window: None,
    };
    let inner = create_egui_editor(
        egui_state,
        state,
        // Runs on every editor open; a request from an earlier open is void.
        |ctx, state| {
            state.run = None;
            state.queue.clear();
            state.logged_window = None;
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = Color32::from_rgb(0x1e, 0x1e, 0x1e);
            ctx.set_visuals(visuals);
        },
        frame,
    )?;
    Some(Box::new(ProbeEditor { inner, shared }))
}

fn pair(v: (f32, f32)) -> String {
    format!("{:.1} x {:.1}", v.0, v.1)
}

fn opt_pair(v: Option<(f32, f32)>) -> String {
    v.map(pair).unwrap_or_else(|| "n/a".to_string())
}

fn opt_num(v: Option<f32>) -> String {
    v.map(|v| format!("{v:.0}")).unwrap_or_else(|| "n/a".to_string())
}

fn advance(state: &mut State, now: Instant, screen: (f32, f32), geo: &HostGeometry) {
    if let Some(run) = &mut state.run {
        let elapsed = now.saturating_duration_since(run.at);
        run.egui.observe(elapsed, screen);
        if let Some(view) = geo.view {
            run.host.observe(elapsed, view);
        }
        let host_done = run.host.settled() || geo.view.is_none();
        if run.egui.settled() && host_done {
            let row = Row {
                asked: run.asked,
                egui: run.egui.verdict(),
                host: run.host.verdict(),
            };
            log(&format!(
                "RESULT asked={} x {} egui=[{}] host=[{}] host_window={} ",
                row.asked.0,
                row.asked.1,
                row.egui.text(),
                row.host.text(),
                opt_pair(geo.window_content)
            ));
            state.rows.push(row);
            state.run = None;
        }
    }
    if state.run.is_none() {
        if let Some(asked) = state.queue.pop_front() {
            log(&format!(
                "ask {} x {} (now egui={} host={} stored={:?})",
                asked.0,
                asked.1,
                pair(screen),
                opt_pair(geo.view),
                state.egui_state.size()
            ));
            state.egui_state.set_requested_size(asked);
            state.run = Some(Run {
                asked,
                at: now,
                egui: Track::new(asked),
                host: Track::new(asked),
            });
        } else if state.auto && state.started && state.exit_when_done {
            log("auto run done, exiting");
            std::process::exit(0);
        }
    }
}

fn frame(ctx: &egui::Context, setter: &ParamSetter, state: &mut State) {
    let now = Instant::now();
    let screen = ctx.screen_rect().size();
    let screen = (screen.x, screen.y);
    let geo = host_geometry(state.shared.parent.load(Ordering::Acquire));
    if state.auto && !state.started {
        state.queue.extend(ASKS);
        state.started = true;
    }
    if geo.window_number.is_some() && geo.window_number != state.logged_window {
        state.logged_window = geo.window_number;
        log(&format!(
            "geometry window_number={:?} visible_h main={:?} window_screen={:?} ppp={:.2} native_ppp={:?}",
            geo.window_number,
            geo.screen_visible_h,
            geo.window_screen_visible_h,
            ctx.pixels_per_point(),
            ctx.native_pixels_per_point()
        ));
    }
    advance(state, now, screen, &geo);

    let label = |text: String| RichText::new(text).color(Color32::from_white_alpha(140));
    let value = |text: String| RichText::new(text).color(Color32::from_white_alpha(204));

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading(
            RichText::new(format!(
                "fl resize probe v{} build {}",
                env!("CARGO_PKG_VERSION"),
                env!("FL_RESIZE_PROBE_BUILD")
            ))
            .color(Color32::WHITE),
        );
        ui.label(label(format!(
            "api {:?}. Each request must arrive within 500 ms and stay 3 s.",
            setter.raw_context.plugin_api()
        )));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let idle = state.run.is_none() && state.queue.is_empty();
            for asked in ASKS {
                let text = format!("{} x {}", asked.0, asked.1);
                if ui.add_enabled(idle, egui::Button::new(text)).clicked() {
                    state.queue.push_back(asked);
                }
            }
            if ui.add_enabled(idle, egui::Button::new("Run all three")).clicked() {
                state.queue.extend(ASKS);
            }
            let back = format!("Back to {} x {}", START.0, START.1);
            if ui.add_enabled(idle, egui::Button::new(back)).clicked() {
                state.queue.push_back(START);
            }
        });
        ui.add_space(8.0);
        egui::Grid::new("live").num_columns(2).show(ui, |ui| {
            let ppp = ctx.pixels_per_point();
            let native = ctx.native_pixels_per_point().unwrap_or(0.0);
            let stored = state.egui_state.size();
            let rows = [
                ("egui screen rect (pt)", pair(screen)),
                ("host NSView frame (pt)", opt_pair(geo.view)),
                ("host window content (pt)", opt_pair(geo.window_content)),
                ("EguiState size", format!("{} x {}", stored.0, stored.1)),
                ("pixels_per_point / native", format!("{ppp:.2} / {native:.2}")),
                (
                    "visible height: main / window screen (pt)",
                    format!(
                        "{} / {}",
                        opt_num(geo.screen_visible_h),
                        opt_num(geo.window_screen_visible_h)
                    ),
                ),
                (
                    "window number",
                    geo.window_number
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "n/a".to_string()),
                ),
            ];
            for (name, text) in rows {
                ui.label(label(name.to_string()));
                ui.label(value(text));
                ui.end_row();
            }
        });
        ui.add_space(8.0);
        if let Some(run) = &state.run {
            let ms = now.saturating_duration_since(run.at).as_millis();
            ui.label(value(format!(
                "asking {} x {} for {ms} ms: egui {}, host {}",
                run.asked.0,
                run.asked.1,
                run.egui.verdict().text(),
                run.host.verdict().text()
            )));
        }
        egui::Grid::new("rows").num_columns(3).show(ui, |ui| {
            ui.label(label("asked".to_string()));
            ui.label(label("egui view".to_string()));
            ui.label(label("host view".to_string()));
            ui.end_row();
            for row in &state.rows {
                ui.label(value(format!("{} x {}", row.asked.0, row.asked.1)));
                ui.label(value(row.egui.text()));
                ui.label(value(row.host.text()));
                ui.end_row();
            }
        });
    });
}
