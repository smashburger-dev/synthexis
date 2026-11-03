//! Pinning on frames of the shipped parser, synthetic sentences only.
//! Needs the exported runs; skipped unless SYNTHEXIS_PARSER_DIR is set:
//!
//!   SYNTHEXIS_PARSER_DIR=../data/mapping/runtime cargo test --release \
//!     -p synthexis-parser --test pin_live -- --nocapture

#![allow(clippy::panic)]

use std::path::PathBuf;

use synthexis_parser::{pin, Options, Parser, Pin, PinWhat};

fn span(text: &str, chip: &str) -> (usize, usize) {
    let b = text.find(chip).unwrap_or(0);
    (b, b + chip.len())
}

fn param(text: &str, chip: &str, id: &str, class: &str, switch: bool) -> Pin {
    Pin { bytes: span(text, chip), what: PinWhat::Param { id: id.into(), class: class.into(), switch } }
}

fn gesture(text: &str, chip: &str, source: &str) -> Pin {
    Pin { bytes: span(text, chip), what: PinWhat::Gesture { source: source.into(), shape: None } }
}

#[test]
fn pinned_frames() {
    let Some(dir) = std::env::var_os("SYNTHEXIS_PARSER_DIR").map(PathBuf::from) else {
        eprintln!("SYNTHEXIS_PARSER_DIR not set, skipped");
        return;
    };
    let parser = match Parser::load(&dir, &["p-v11-s0", "p-v11-s1", "p-v11-s2", "p-v11-s3"], &Options::default()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("parser not loaded: {e:#}");
            return;
        }
    };
    let cases: Vec<(&str, Vec<Pin>)> = {
        let a = "when I spread my fingers the Filter 1 Freq goes up";
        let b = "wenn ich Faust schließen geht Filter 2 Freq runter";
        let c = "pinch to switch Filter 1 On and my mouth opens Reverb Mix";
        let d = "Faust schließen Filter 1 Freq";
        vec![
            (a, vec![gesture(a, "spread my fingers", "hand.spread"), param(a, "Filter 1 Freq", "p#205", "filter.cutoff", false)]),
            (b, vec![gesture(b, "Faust schließen", "hand.fist"), param(b, "Filter 2 Freq", "p#216", "filter.cutoff", false)]),
            (c, vec![param(c, "Filter 1 On", "p#203", "ctl.switch", true), param(c, "Reverb Mix", "p#9", "fx.reverb.mix", false)]),
            (d, vec![gesture(d, "Faust schließen", "hand.fist"), param(d, "Filter 1 Freq", "p#205", "filter.cutoff", false)]),
        ]
    };
    for (text, pins) in &cases {
        let p = match parser.parse(text) {
            Ok(p) => p,
            Err(e) => panic!("parse failed: {e:#}"),
        };
        let f = pin(&p.items, pins);
        eprintln!("{text}\n  parser: {}\n  pinned: {}", p.frame().join(" "), f.frame().join(" "));
        let params: Vec<&str> = pins
            .iter()
            .filter_map(|p| match &p.what {
                PinWhat::Param { id, .. } => Some(id.as_str()),
                PinWhat::Gesture { .. } => None,
            })
            .collect();
        for id in params {
            assert!(f.routes().any(|r| r.param.as_deref() == Some(id)), "{id} not pinned in {text}");
        }
    }
}
