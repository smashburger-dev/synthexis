//! Reads the host side of the window: the NSView the host handed to the
//! editor, its NSWindow, and the main screen. These are what the user sees;
//! the egui screen rect only says what the plug-in's own view thinks.
//!
//! Every function runs on the main thread (the egui update closure does) and
//! only while the editor is open, so the parent view is alive.

#![allow(unsafe_code)]

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::NSRect;

/// What the host shows, in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HostGeometry {
    /// The parent NSView's frame size: the area the host gives the plug-in.
    pub view: Option<(f32, f32)>,
    /// The size of the parent window's content view (the host's whole window).
    pub window_content: Option<(f32, f32)>,
    /// `windowNumber` of the parent window; `screencapture -l` takes it.
    pub window_number: Option<isize>,
    /// `NSScreen.mainScreen.visibleFrame` height. `mainScreen` is the screen
    /// of the key window, which need not be the plug-in's.
    pub screen_visible_h: Option<f32>,
    /// `visibleFrame` height of the screen the parent window is on.
    pub window_screen_visible_h: Option<f32>,
}

fn size(rect: NSRect) -> (f32, f32) {
    (rect.size.width as f32, rect.size.height as f32)
}

/// Reads the geometry around `parent_view` (an `NSView *`, 0 when unknown).
pub fn host_geometry(parent_view: usize) -> HostGeometry {
    let mut out = HostGeometry {
        screen_visible_h: screen_visible_h(),
        ..Default::default()
    };
    if parent_view == 0 {
        return out;
    }
    let view = parent_view as *mut AnyObject;
    // SAFETY: `view` is the NSView pointer the host passed to `Editor::spawn`;
    // the editor wrapper clears it before the editor handle drops, and this
    // runs on the main thread inside the editor's frame callback. `frame`,
    // `window`, `screen`, `contentView` and `visibleFrame` are plain getters;
    // nil results are checked before use.
    unsafe {
        let frame: NSRect = msg_send![view, frame];
        out.view = Some(size(frame));
        let window: *mut AnyObject = msg_send![view, window];
        if !window.is_null() {
            let number: isize = msg_send![window, windowNumber];
            out.window_number = Some(number);
            let screen: *mut AnyObject = msg_send![window, screen];
            if !screen.is_null() {
                let frame: NSRect = msg_send![screen, visibleFrame];
                out.window_screen_visible_h = Some(frame.size.height as f32);
            }
            let content: *mut AnyObject = msg_send![window, contentView];
            if !content.is_null() {
                let frame: NSRect = msg_send![content, frame];
                out.window_content = Some(size(frame));
            }
        }
    }
    out
}

fn screen_visible_h() -> Option<f32> {
    let class = AnyClass::get(c"NSScreen")?;
    // SAFETY: class methods and getters of NSScreen, called on the main
    // thread; a nil main screen is checked before use.
    unsafe {
        let screen: *mut AnyObject = msg_send![class, mainScreen];
        if screen.is_null() {
            return None;
        }
        let frame: NSRect = msg_send![screen, visibleFrame];
        Some(frame.size.height as f32)
    }
}
