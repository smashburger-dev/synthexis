//! Standalone target for headless verification (-r 44100 on this machine).

use nih_plug::nih_export_standalone;
use synthexis::Synthexis;

fn main() {
    nih_export_standalone::<Synthexis>();
}
