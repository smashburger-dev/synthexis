//! Parity with the Python parser on 200 synthetic dev sentences
//! (data/mapping/train/ds-v6p/dev.jsonl, first 200). The fixture is the
//! output of
//!
//!   uv run tools/mapping/predict.py --run p-v7a-s0,p-v7a-s1 --f16 \
//!       --file dev200.txt --jsonl dev200-v7a-s0s1.jsonl
//!
//! Needs the exported runs (uv run tools/mapping/export_runtime.py --run
//! p-v7a-s0,p-v7a-s1) in $SYNTHEXIS_PARSER_DIR or data/mapping/runtime:
//!
//!   ./guarded-cargo.sh test -p synthexis-parser --release -- --ignored

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use synthexis_parser::{Options, Parser};

fn runtime_dir() -> PathBuf {
    std::env::var_os("SYNTHEXIS_PARSER_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/mapping/runtime"),
        PathBuf::from,
    )
}

#[test]
#[ignore = "needs the exported parser runs (data/mapping/runtime)"]
fn ensemble_matches_python_frames() {
    let parser = Parser::load(&runtime_dir(), &["p-v7a-s0", "p-v7a-s1"], &Options::default())
        .expect("load exported runs");
    let fixture = include_str!("fixtures/dev200-v7a-s0s1.jsonl");
    let (mut n, mut same, mut worst) = (0, 0, 0.0f64);
    let mut diffs = Vec::new();
    for line in fixture.lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let text = row["text"].as_str().unwrap();
        let want: Vec<String> = serde_json::from_value(row["frame"].clone()).unwrap();
        let got = parser.parse(text).unwrap();
        n += 1;
        if got.frame() == want {
            same += 1;
            let conf = f64::from(got.logp).exp();
            worst = worst.max((conf - row["conf"].as_f64().unwrap()).abs());
        } else {
            diffs.push(format!("{text}\n  py: {}\n  rs: {}", want.join(" "), got.frame().join(" ")));
        }
    }
    eprintln!("{same}/{n} frames equal, max |conf diff| {worst:.4}");
    assert_eq!(n, 200);
    assert!(diffs.is_empty(), "frames differ:\n{}", diffs.join("\n"));
    assert!(worst <= 0.002, "confidence differs by {worst}");
}

#[test]
#[ignore = "needs the exported parser runs (data/mapping/runtime)"]
fn single_run_fallback_parses() {
    let parser = Parser::load(&runtime_dir(), &["p-v7a-s0"], &Options::default()).unwrap();
    let p = parser.parse("my fist opens the filter on the bass").unwrap();
    assert_eq!(p.frame().first().map(String::as_str), Some("I:map"));
    assert!(p.logp <= 0.0);
}
