//! Parity with the Python parser on 200 synthetic dev sentences per set
//! (the first 200 rows; never Noa's real sentences). Each fixture is the
//! output of
//!
//!   uv run tools/mapping/predict.py --run <s0>,<s1> --f16 \
//!       --file <texts> --jsonl <fixture>
//!
//!   dev200-v9-s0s1.jsonl        p-v9-s0,p-v9-s1 on ds-v9c/dev.jsonl (shipped)
//!   dev_s6-200-v9-s0s1.jsonl    p-v9-s0,p-v9-s1 on ds-v9c/dev_s6.jsonl
//!   dev200-v7a-s0s1.jsonl       p-v7a-s0,p-v7a-s1 on ds-v6p/dev.jsonl
//!
//! Needs the exported runs (uv run tools/mapping/export_runtime.py --run
//! p-v9-s0,p-v9-s1 and the same for v7a) in $SYNTHEXIS_PARSER_DIR or
//! data/mapping/runtime:
//!
//!   ./guarded-cargo.sh test -p synthexis-parser --release -- --include-ignored

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use synthexis_parser::{Options, Parser};

fn runtime_dir() -> PathBuf {
    std::env::var_os("SYNTHEXIS_PARSER_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/mapping/runtime"),
        PathBuf::from,
    )
}

/// Every frame equal, confidence within 0.002 (the fixture rounds to 3 places).
fn check(runs: &[&str], fixture: &str) {
    let parser = Parser::load(&runtime_dir(), runs, &Options::default()).expect("load exported runs");
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
    eprintln!("{runs:?}: {same}/{n} frames equal, max |conf diff| {worst:.4}");
    assert_eq!(n, 200);
    assert!(diffs.is_empty(), "frames differ:\n{}", diffs.join("\n"));
    assert!(worst <= 0.002, "confidence differs by {worst}");
}

#[test]
#[ignore = "needs the exported parser runs (data/mapping/runtime)"]
fn v9_ensemble_matches_python_on_dev() {
    check(&["p-v9-s0", "p-v9-s1"], include_str!("fixtures/dev200-v9-s0s1.jsonl"));
}

#[test]
#[ignore = "needs the exported parser runs (data/mapping/runtime)"]
fn v9_ensemble_matches_python_on_stage6() {
    check(&["p-v9-s0", "p-v9-s1"], include_str!("fixtures/dev_s6-200-v9-s0s1.jsonl"));
}

#[test]
#[ignore = "needs the exported parser runs (data/mapping/runtime)"]
fn v7a_ensemble_matches_python_on_dev() {
    check(&["p-v7a-s0", "p-v7a-s1"], include_str!("fixtures/dev200-v7a-s0s1.jsonl"));
}

#[test]
#[ignore = "needs the exported parser runs (data/mapping/runtime)"]
fn single_run_fallback_parses() {
    let parser = Parser::load(&runtime_dir(), &["p-v9-s0"], &Options::default()).unwrap();
    let p = parser.parse("my fist opens the filter on the bass").unwrap();
    assert_eq!(p.frame().first().map(String::as_str), Some("I:map"));
    assert!(p.logp <= 0.0);
}
