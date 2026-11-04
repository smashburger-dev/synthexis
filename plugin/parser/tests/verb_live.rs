//! Lowering and raising verbs through the shipped parser and the resolver
//! (verb.rs): "pinch my fingers decrease the filter" must come out
//! inverted even where the parser leaves INV out. Needs the exported runs;
//! skipped unless SYNTHEXIS_PARSER_DIR is set:
//!
//!   SYNTHEXIS_PARSER_DIR=$PWD/../data/mapping/runtime cargo test \
//!     -p synthexis-parser --test verb_live -- --nocapture

use std::collections::HashMap;
use std::path::PathBuf;

use synthexis_parser::resolve::{resolve, Candidate};
use synthexis_parser::{pin_spans, Options, Parser};

const PARAMS: [(u32, &str, &str); 4] =
    [(0, "Filter 1 Freq", "filter.cutoff"), (1, "Reverb Mix", "fx.reverb.mix"), (2, "Volume", "amp.gain"), (3, "Drive", "fx.distortion.drive")];

/// (sentence, the route's INV)
const CASES: [(&str, bool); 12] = [
    ("pinch my fingers decrease Filter 1 Freq", true),
    ("pinch my fingers decrease the filter", true),
    ("pinch to increase the filter", false),
    ("when I open my fist decrease the reverb", true),
    ("decrease the reverb when I open my fist", true),
    // the parser reads squeezed fingers as the spread closing: squeezing
    // lowers the filter on a straight route, raises it on an inverted one
    ("wenn ich die Finger zusammendrücke, Filter runter", false),
    ("wenn ich die Finger zusammendrücke, mehr Filter", true),
    ("raise my hand to lower the volume", true),
    ("wenn ich die Hand hebe, Lautstärke runter", true),
    ("make a fist to lower the drive", false),
    ("closing my right eye lowers the volume", false),
    ("pinch to reduce the drive", true),
];

#[test]
fn verbs_set_the_direction() -> Result<(), String> {
    let Some(dir) = std::env::var_os("SYNTHEXIS_PARSER_DIR").map(PathBuf::from) else {
        eprintln!("SYNTHEXIS_PARSER_DIR not set, skipped");
        return Ok(());
    };
    let parser = match Parser::load(&dir, &["p-v11-s0", "p-v11-s1", "p-v11-s2", "p-v11-s3"], &Options::default()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("parser not loaded: {e:#}");
            return Ok(());
        }
    };
    let cands: Vec<Candidate> = PARAMS.iter().map(|&(index, name, class)| Candidate { index, name, class }).collect();
    let mut misses = Vec::new();
    for (text, inv) in CASES {
        let p = parser.parse(text).map_err(|e| format!("parse failed: {e:#}"))?;
        let frame = pin_spans(&p.items, text, &p.spans, &[]);
        let routes = resolve(&frame, text, &p.spans, "Synth", &cands, &HashMap::new());
        let lines: Vec<String> = routes.iter().map(|r| r.describe("Synth")).collect();
        eprintln!("{text}\n  frame: {}\n  line:  {}", p.frame().join(" "), lines.join(" | "));
        if !routes.iter().any(|r| r.inv == inv && r.targets.iter().any(|t| t.token.starts_with("T:param:"))) {
            misses.push(format!("{text}: want inv {inv}, got [{}]", lines.join(" | ")));
        }
    }
    if misses.is_empty() {
        Ok(())
    } else {
        Err(misses.join("\n"))
    }
}
