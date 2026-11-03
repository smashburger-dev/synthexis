//! Standalone target for headless verification (-r 44100 on this machine).

use nih_plug::nih_export_standalone;
use synthexis::Synthexis;

fn main() {
    if let Ok(path) = std::env::var("SYNTHEXIS_BENCH") {
        std::process::exit(synthexis::perception_bench(&path));
    }
    nih_export_standalone::<Synthexis>();
}
