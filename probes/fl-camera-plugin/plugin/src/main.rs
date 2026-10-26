//! Standalone target: runs the plugin headlessly (no editor). The camera
//! engine starts on plugin construction; the GUI-context pump will log
//! "no GuiContext" until an editor has been opened in a host.

use nih_plug::nih_export_standalone;
use synthexis_probe::SynthexisProbe;

fn main() {
    nih_export_standalone::<SynthexisProbe>();
}
