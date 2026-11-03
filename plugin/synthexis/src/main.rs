//! Standalone target for headless verification (-r 44100 on this machine).

use nih_plug::nih_export_standalone;
use synthexis::Synthexis;

fn main() {
    if let Ok(n) = std::env::var("SYNTHEXIS_MEMPROBE") {
        std::process::exit(synthexis::mem_probe(n.parse().unwrap_or(1)));
    }
    if std::env::var_os("SYNTHEXIS_LOADPROBE").is_some() {
        std::process::exit(synthexis::load_probe());
    }
    if let Ok(path) = std::env::var("SYNTHEXIS_BENCH") {
        std::process::exit(synthexis::perception_bench(&path));
    }
    nih_export_standalone::<Synthexis>();
}
