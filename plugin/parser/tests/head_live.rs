//! Face and head sentences, German and English, through the shipped
//! parser, pinning and the resolver (probes/head-signals/sentences.tsv).
//! Needs the exported runs; skipped unless SYNTHEXIS_PARSER_DIR is set:
//!
//!   SYNTHEXIS_PARSER_DIR=../data/mapping/runtime cargo test \
//!     -p synthexis-parser --test head_live -- --nocapture
//!
//! Each sentence runs twice: as typed (the parser alone) and with the
//! gesture chip the completion offers on its gesture words (and the
//! pattern chip where the sentence names one). A case passes
//! when its route has the expected source (eye.closed counts as eye.open
//! with INV flipped, as the conv rule folds it), side and target kind.

#![allow(clippy::panic)]

use std::collections::HashMap;
use std::path::PathBuf;

use synthexis_parser::resolve::{resolve, Candidate, Route};
use synthexis_parser::{pin_spans, Options, Parser, Pin, PinWhat};

const CASES: &str = include_str!("../../../probes/head-signals/sentences.tsv");

/// A small synth's parameters, so param targets have something to bind.
const PARAMS: [(u32, &str, &str); 6] = [
    (0, "Filter Cutoff", "filter.cutoff"),
    (1, "Filter Resonance", "filter.resonance"),
    (2, "Reverb Mix", "fx.reverb.mix"),
    (3, "Volume", "amp.gain"),
    (4, "Pan", "amp.pan"),
    (5, "Drive", "fx.distortion.drive"),
];

struct Case<'a> {
    lang: &'a str,
    text: &'a str,
    source: &'a str,
    side: Option<&'a str>,
    inv: bool,
    kind: &'a str,
    /// The gesture words a chip covers, from the completion's phrases.
    chip: Option<&'a str>,
    /// A target chip (words, kind): the pattern names the completion
    /// offers come from the MIDI store and its templates (kick, hi-hat).
    target: Option<(&'a str, &'a str)>,
}

fn cases() -> Vec<Case<'static>> {
    CASES
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Some(Case {
                lang: f.first()?,
                text: f.get(1)?,
                source: f.get(2)?,
                side: f.get(3).filter(|s| **s != "-").copied(),
                inv: f.get(4) == Some(&"1"),
                kind: f.get(5)?,
                chip: f.get(6).filter(|s| !s.is_empty()).copied(),
                target: f.get(7).and_then(|t| t.split_once('=')),
            })
        })
        .collect()
}

/// Source and INV with a closed eye folded into the open one.
fn folded(r: &Route) -> (&str, bool) {
    match r.source.as_str() {
        "eye.closed" => ("eye.open", !r.inv),
        s => (s, r.inv),
    }
}

fn kind_of(token: &str) -> &str {
    if token.starts_with("T:midi.") {
        "midi"
    } else if token.starts_with("T:macro") {
        "macro"
    } else if token.starts_with("T:param") || token.starts_with("T:toggle") {
        "param"
    } else {
        "other"
    }
}

/// Why the case's routes miss it, None when one route matches.
fn miss(c: &Case, routes: &[Route]) -> Option<String> {
    let hit = routes.iter().any(|r| {
        let (src, inv) = folded(r);
        src == c.source && inv == c.inv && r.side.as_deref() == c.side && r.targets.iter().any(|t| kind_of(&t.token) == c.kind)
    });
    if hit {
        return None;
    }
    let got: Vec<String> = routes
        .iter()
        .map(|r| {
            let kinds: Vec<&str> = r.targets.iter().map(|t| kind_of(&t.token)).collect();
            format!("{} side:{:?} inv:{} -> {}", r.source, r.side, r.inv, kinds.join(","))
        })
        .collect();
    Some(format!("want {} side:{:?} inv:{} -> {}; got [{}]", c.source, c.side, c.inv, c.kind, got.join(" | ")))
}

fn target_pin(c: &Case) -> Option<Pin> {
    let (words, kind) = c.target?;
    let b = c.text.find(words)?;
    Some(Pin { bytes: (b, b + words.len()), what: PinWhat::Target { kind: kind.into() } })
}

fn chip_pin(c: &Case) -> Option<Pin> {
    let words = c.chip?;
    let b = c.text.find(words)?;
    let source = if c.source == "eye.open" && c.inv { "eye.closed" } else { c.source };
    let side = c.side.map(str::to_string);
    Some(Pin { bytes: (b, b + words.len()), what: PinWhat::Gesture { source: source.into(), shape: None, side } })
}

#[test]
fn head_sentences_de_en() -> Result<(), String> {
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
    let aliases = HashMap::new();
    let (mut typed_ok, mut chip_ok, mut n) = (HashMap::<&str, usize>::new(), 0usize, 0usize);
    let mut chip_misses = Vec::new();
    for c in cases() {
        n += 1;
        let p = parser.parse(c.text).map_err(|e| format!("parse failed: {e:#}"))?;
        let plain = pin_spans(&p.items, c.text, &p.spans, &[]);
        let typed = resolve(&plain, c.text, &p.spans, "Synth", &cands, &aliases);
        let typed_miss = miss(&c, &typed);
        if typed_miss.is_none() {
            *typed_ok.entry(c.lang).or_default() += 1;
        }
        let pins: Vec<Pin> = chip_pin(&c).into_iter().chain(target_pin(&c)).collect();
        let pinned = pin_spans(&p.items, c.text, &p.spans, &pins);
        let chipped = resolve(&pinned, c.text, &p.spans, "Synth", &cands, &aliases);
        let chip_miss = miss(&c, &chipped);
        match &chip_miss {
            None => chip_ok += 1,
            Some(m) => chip_misses.push(format!("{}: {m}", c.text)),
        }
        eprintln!(
            "[{}] {}\n  frame: {}\n  typed: {}\n  chip:  {}",
            c.lang,
            c.text,
            p.frame().join(" "),
            typed_miss.as_deref().unwrap_or("ok"),
            chip_miss.as_deref().unwrap_or("ok")
        );
    }
    eprintln!("typed ok: {typed_ok:?} of {n}; with chip: {chip_ok}/{n}");
    // With the chip the completion offers, every head sentence must land;
    // typed alone, at least 12 per language (docs/session-2026-11-04-head.md).
    assert!(chip_misses.is_empty(), "with chip:\n{}", chip_misses.join("\n"));
    for lang in ["de", "en"] {
        let ok = typed_ok.get(lang).copied().unwrap_or(0);
        assert!(ok >= 12, "{lang}: {ok} typed sentences land");
    }
    Ok(())
}
