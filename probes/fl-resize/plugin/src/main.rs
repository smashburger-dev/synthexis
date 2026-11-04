//! Standalone target. `-b dummy` skips the audio device; the window is the
//! point. `FL_RESIZE_PROBE_AUTO=1` runs the three requests by itself and
//! `FL_RESIZE_PROBE_EXIT=1` quits after the last verdict.

use fl_resize_probe::FlResizeProbe;
use nih_plug::nih_export_standalone;

fn main() {
    nih_export_standalone::<FlResizeProbe>();
}
