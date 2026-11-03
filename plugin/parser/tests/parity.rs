//! Parity with the Python parser on the first 200 rows of ds-v11c dev
//! (synthetic; never Noa's real sentences). The fixture is the output of
//!
//!   uv run tools/mapping/predict.py --rules conv,pronoun \
//!       --run p-v11-s0-q8,p-v11-s1-q8,p-v11-s2-q8,p-v11-s3-q8 \
//!       --file <texts> --jsonl dev200-v11-q8-s0123.jsonl
//!
//! on branch claude/parser-v11-de5d19: the four v11 seeds with the Q8_0
//! rounding of candle's QTensor::quantize simulated in torch, decoded with
//! the conv,pronoun rules. The runtime loads the same runs with their
//! parser.json defaults (Q8_0, conv,pronoun), so this checks the shipped
//! path. Needs the export
//!
//!   uv run tools/mapping/export_runtime.py --run p-v11-s0,p-v11-s1,p-v11-s2,p-v11-s3 \
//!       --rules conv,pronoun --q8
//!
//! in $SYNTHEXIS_PARSER_DIR or data/mapping/runtime:
//!
//!   ./guarded-cargo.sh test -p synthexis-parser --release -- --include-ignored

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use synthexis_parser::{Options, Parser};

const V11: [&str; 4] = ["p-v11-s0", "p-v11-s1", "p-v11-s2", "p-v11-s3"];

fn runtime_dir() -> PathBuf {
    std::env::var_os("SYNTHEXIS_PARSER_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/mapping/runtime"),
        PathBuf::from,
    )
}

#[test]
#[ignore = "needs the exported parser runs (data/mapping/runtime)"]
fn v11_q8_ensemble_matches_python() {
    let parser = Parser::load(&runtime_dir(), &V11, &Options::default()).expect("load exported runs");
    let (mut n, mut same, mut worst) = (0, 0, 0.0f64);
    let mut diffs = Vec::new();
    for line in include_str!("fixtures/dev200-v11-q8-s0123.jsonl").lines() {
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
    // torch decodes in padded batches, the runtime one sentence at a time;
    // on all 805 dev rows the confidences differ by at most 0.003
    assert!(worst <= 0.005, "confidence differs by {worst}");
}

#[test]
#[ignore = "needs the exported parser runs (data/mapping/runtime)"]
fn rules_and_weights_can_be_overridden() {
    let opt = Options {
        q8: Some(false),
        rules: Some(Vec::new()),
        ..Options::default()
    };
    let parser = Parser::load(&runtime_dir(), &V11[..1], &opt).unwrap();
    let p = parser.parse("my fist opens the filter on the bass").unwrap();
    assert_eq!(p.frame().first().map(String::as_str), Some("I:map"));
    assert!(p.logp <= 0.0);
}
