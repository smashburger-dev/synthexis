//! Completions and keystroke time on the exported data:
//!
//!   cargo run --release -p synthexis-complete --example try_complete -- \
//!     ../data/mapping/completion "Xfer Records/Serum 2" "when I spr" "the cut"

use std::path::Path;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, plugin, texts @ ..] = args.as_slice() else {
        eprintln!("usage: try_complete <completion dir> <plugin id> <text>...");
        std::process::exit(2);
    };
    let dir = Path::new(dir);
    let t0 = Instant::now();
    let (mut e, plugins) = match synthexis_complete::data::load(dir) {
        Ok(x) => x,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    if let Some(info) = plugins.iter().find(|p| p.plugin_id == *plugin) {
        match synthexis_complete::data::load_plugin(dir, info) {
            Ok(params) => e.set_plugin(&info.plugin_id, params),
            Err(err) => eprintln!("{err}"),
        }
    }
    println!("loaded in {:.0} ms", t0.elapsed().as_secs_f64() * 1e3);
    let mut ms = Vec::new();
    for text in texts {
        for end in 1..=text.len() {
            if !text.is_char_boundary(end) {
                continue;
            }
            let t = Instant::now();
            let s = e.complete(text, end, 0, 3);
            ms.push(t.elapsed().as_secs_f64() * 1e3);
            if end == text.len() {
                println!("{text:?}");
                for x in &s {
                    let ghost = if x.ghost_worthy(text) { " ghost" } else { "" };
                    println!(
                        "  {:.3} {} [{}] via {:?}{ghost}",
                        x.score,
                        x.chip.label(),
                        x.chip.id(),
                        x.via
                    );
                }
            }
        }
    }
    ms.sort_by(f64::total_cmp);
    if !ms.is_empty() {
        println!(
            "{} keystrokes: median {:.3} ms, p95 {:.3} ms, max {:.3} ms",
            ms.len(),
            ms[ms.len() / 2],
            ms[ms.len() * 95 / 100],
            ms[ms.len() - 1]
        );
    }
}
