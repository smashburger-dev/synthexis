//! An [`Editor`] implementation for egui.

use crate::egui::Vec2;
use crate::egui::ViewportCommand;
use crate::EguiState;
use baseview::gl::GlConfig;
use baseview::PhySize;
use baseview::{Size, WindowHandle, WindowOpenOptions, WindowScalePolicy};
use crossbeam::atomic::AtomicCell;
use egui_baseview::egui::Context;
use egui_baseview::EguiWindow;
use nih_plug::prelude::{Editor, GuiContext, ParamSetter, ParentWindowHandle};
use parking_lot::RwLock;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// An [`Editor`] implementation that calls an egui draw loop.
pub(crate) struct EguiEditor<T> {
    pub(crate) egui_state: Arc<EguiState>,
    /// The plugin's state. This is kept in between editor openenings.
    pub(crate) user_state: Arc<RwLock<T>>,

    /// The user's build function. Applied once at the start of the application.
    pub(crate) build: Arc<dyn Fn(&Context, &mut T) + 'static + Send + Sync>,
    /// The user's update function.
    pub(crate) update: Arc<dyn Fn(&Context, &ParamSetter, &mut T) + 'static + Send + Sync>,

    /// The scaling factor reported by the host, if any. On macOS this will never be set and we
    /// should use the system scaling factor instead.
    pub(crate) scaling_factor: AtomicCell<Option<f32>>,
}

/// This version of `baseview` uses a different version of `raw_window_handle than NIH-plug, so we
/// need to adapt it ourselves.
struct ParentWindowHandleAdapter(nih_plug::editor::ParentWindowHandle);

unsafe impl HasRawWindowHandle for ParentWindowHandleAdapter {
    fn raw_window_handle(&self) -> RawWindowHandle {
        match self.0 {
            ParentWindowHandle::X11Window(window) => {
                let mut handle = raw_window_handle::XcbWindowHandle::empty();
                handle.window = window;
                RawWindowHandle::Xcb(handle)
            }
            ParentWindowHandle::AppKitNsView(ns_view) => {
                let mut handle = raw_window_handle::AppKitWindowHandle::empty();
                handle.ns_view = ns_view;
                RawWindowHandle::AppKit(handle)
            }
            ParentWindowHandle::Win32Hwnd(hwnd) => {
                let mut handle = raw_window_handle::Win32WindowHandle::empty();
                handle.hwnd = hwnd;
                RawWindowHandle::Win32(handle)
            }
        }
    }
}

impl<T> Editor for EguiEditor<T>
where
    T: 'static + Send + Sync,
{
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn std::any::Any + Send> {
        let build = self.build.clone();
        let update = self.update.clone();
        let state = self.user_state.clone();
        let egui_state = self.egui_state.clone();

        let (unscaled_width, unscaled_height) = self.egui_state.size();
        let scaling_factor = self.scaling_factor.load();
        let window = EguiWindow::open_parented(
            &ParentWindowHandleAdapter(parent),
            WindowOpenOptions {
                title: String::from("egui window"),
                // Baseview should be doing the DPI scaling for us
                size: Size::new(unscaled_width as f64, unscaled_height as f64),
                // NOTE: For some reason passing 1.0 here causes the UI to be scaled on macOS but
                //       not the mouse events.
                scale: scaling_factor
                    .map(|factor| WindowScalePolicy::ScaleFactor(factor as f64))
                    .unwrap_or(WindowScalePolicy::SystemScaleFactor),

                #[cfg(feature = "opengl")]
                gl_config: Some(GlConfig {
                    version: (3, 2),
                    red_bits: 8,
                    blue_bits: 8,
                    green_bits: 8,
                    alpha_bits: 8,
                    depth_bits: 24,
                    stencil_bits: 8,
                    samples: None,
                    srgb: true,
                    double_buffer: true,
                    vsync: true,
                    ..Default::default()
                }),
            },
            Default::default(),
            state,
            move |egui_ctx, _queue, state| build(egui_ctx, &mut state.write()),
            move |egui_ctx, queue, state| {
                let setter = ParamSetter::new(context.as_ref());

                // If the window was requested to resize, ask the plugin host to resize to
                // self.size(). synthexis patch: see `take_resize_request` (vendor/PATCHES.md).
                if let Some(new_size) =
                    take_resize_request(&egui_state, || context.request_resize())
                {
                    // Resize the content of egui window. The queue takes physical pixels, the
                    // viewport command logical ones.
                    let pixels_per_point = egui_ctx
                        .native_pixels_per_point()
                        .unwrap_or_else(|| egui_ctx.pixels_per_point());
                    queue.resize(physical_size(new_size, pixels_per_point));
                    egui_ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(
                        new_size.0 as f32,
                        new_size.1 as f32,
                    )));
                }

                // For now, just always redraw. Most plugin GUIs have meters, and those almost always
                // need a redraw. Later we can try to be a bit more sophisticated about this. Without
                // this we would also have a blank GUI when it gets first opened because most DAWs open
                // their GUI while the window is still unmapped.
                egui_ctx.request_repaint();
                (update)(egui_ctx, &setter, &mut state.write());
            },
        );

        self.egui_state.open.store(true, Ordering::Release);
        Box::new(EguiEditorHandle {
            egui_state: self.egui_state.clone(),
            window,
        })
    }

    /// Size of the editor window
    fn size(&self) -> (u32, u32) {
        let new_size = self.egui_state.requested_size.load();
        // This method will be used to ask the host for new size.
        // If the editor is currently being resized and new size hasn't been consumed and set yet, return new requested size.
        if let Some(new_size) = new_size {
            new_size
        } else {
            self.egui_state.size()
        }
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        // If the editor is currently open then the host must not change the current HiDPI scale as
        // we don't have a way to handle that. Ableton Live does this.
        if self.egui_state.is_open() {
            return false;
        }

        self.scaling_factor.store(Some(factor));
        true
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {
        // As mentioned above, for now we'll always force a redraw to allow meter widgets to work
        // correctly. In the future we can use an `Arc<AtomicBool>` and only force a redraw when
        // that boolean is set.
    }

    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}

    fn param_values_changed(&self) {
        // Same
    }
}

/// Takes a pending resize request and runs `request_resize` for it. Returns the new size when the
/// request succeeded, `None` without a request or when it failed.
///
/// synthexis patch: the new size is stored before `request_resize` runs, because the wrappers read
/// [`Editor::size()`] from inside it (the VST3 wrapper on macOS calls the host synchronously when
/// it is on the GUI thread, the standalone queues `GuiTask::Resize(size())`). Upstream stored the
/// size only afterwards, so both resized to the old size. A failed request restores the old size.
fn take_resize_request(
    egui_state: &EguiState,
    request_resize: impl FnOnce() -> bool,
) -> Option<(u32, u32)> {
    let new_size = egui_state.requested_size.swap(None)?;
    let old_size = egui_state.size.swap(new_size);
    if request_resize() {
        Some(new_size)
    } else {
        egui_state.size.store(old_size);
        None
    }
}

/// A logical size in physical pixels, at least one pixel per side.
///
/// synthexis patch: upstream passed the logical size to `Queue::resize`, which expects physical
/// pixels, so a Retina window laid out at half its size until the next `Resized` event.
fn physical_size(logical: (u32, u32), pixels_per_point: f32) -> PhySize {
    let scale = |v: u32| (v as f32 * pixels_per_point).round().max(1.0) as u32;
    PhySize::new(scale(logical.0), scale(logical.1))
}

/// The window handle used for [`EguiEditor`].
struct EguiEditorHandle {
    egui_state: Arc<EguiState>,
    window: WindowHandle,
}

/// The window handle enum stored within 'WindowHandle' contains raw pointers. Is there a way around
/// having this requirement?
unsafe impl Send for EguiEditorHandle {}

impl Drop for EguiEditorHandle {
    fn drop(&mut self) {
        self.egui_state.open.store(false, Ordering::Release);
        // XXX: This should automatically happen when the handle gets dropped, but apparently not
        self.window.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn editor(state: &Arc<EguiState>) -> EguiEditor<()> {
        EguiEditor {
            egui_state: state.clone(),
            user_state: Arc::new(RwLock::new(())),
            build: Arc::new(|_, _| {}),
            update: Arc::new(|_, _, _| {}),
            scaling_factor: AtomicCell::new(Some(1.0)),
        }
    }

    #[test]
    fn size_reports_the_requested_size_while_the_request_runs() {
        let state = EguiState::from_size(1000, 640);
        let editor = editor(&state);
        state.set_requested_size((1200, 820));
        // The request is pending: size() already answers with it.
        assert_eq!(editor.size(), (1200, 820));

        let seen = Cell::new((0, 0));
        let taken = take_resize_request(&state, || {
            seen.set(editor.size());
            true
        });
        assert_eq!(seen.get(), (1200, 820));
        assert_eq!(taken, Some((1200, 820)));
        assert_eq!(editor.size(), (1200, 820));
        assert_eq!(state.size(), (1200, 820));
    }

    #[test]
    fn a_failed_request_restores_the_old_size() {
        let state = EguiState::from_size(1000, 640);
        let editor = editor(&state);
        state.set_requested_size((1200, 900));

        let seen = Cell::new((0, 0));
        let taken = take_resize_request(&state, || {
            seen.set(editor.size());
            false
        });
        assert_eq!(seen.get(), (1200, 900));
        assert_eq!(taken, None);
        assert_eq!(editor.size(), (1000, 640));
    }

    #[test]
    fn no_request_means_no_call() {
        let state = EguiState::from_size(1000, 640);
        let called = Cell::new(false);
        let taken = take_resize_request(&state, || {
            called.set(true);
            true
        });
        assert_eq!(taken, None);
        assert!(!called.get());
        assert_eq!(state.size(), (1000, 640));
    }

    #[test]
    fn the_queue_gets_physical_pixels() {
        let retina = physical_size((1200, 820), 2.0);
        assert_eq!((retina.width, retina.height), (2400, 1640));
        let odd = physical_size((1201, 721), 1.5);
        assert_eq!((odd.width, odd.height), (1802, 1082));
        let flat = physical_size((0, 0), 1.0);
        assert_eq!((flat.width, flat.height), (1, 1));
    }
}
