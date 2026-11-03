//! Simulated user for probes/completion-targets (see its RESULT.md): reads
//! rows from stdin (split, text, gold TGT spans with their target kind),
//! types each span character by character with the sentence field's
//! 8-item list and prints the measures per split and their mean.
//!
//!   sim_targets <completion dir> < targets.jsonl

use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::Path;

use serde::Deserialize;
use synthexis_complete::{data, Chip, Engine, SWITCH_CLASS};

const K: usize = 8;
const MIN_TYPED: usize = 2;
const CATCH_ALL: [&str; 3] = ["other", "system", "unsafe"];

#[derive(Deserialize)]
struct Row {
    split: String,
    text: String,
    /// [byte start, byte end, kind]: param:<class>, toggle, midi.note, macro ...
    tgt: Vec<(usize, usize, String)>,
}

/// The target kind an item offers, in the rows' terms.
fn kind_of(c: &Chip) -> String {
    match c {
        Chip::Param { class, .. } if class == SWITCH_CLASS => "toggle".into(),
        Chip::Param { class, .. } => format!("param:{class}"),
        Chip::Gesture { .. } => "gesture".into(),
        #[allow(unreachable_patterns)]
        _ => c.id().strip_prefix("target:").unwrap_or("?").to_string(),
    }
}

fn group(kind: &str) -> Option<&'static str> {
    match kind {
        // any parameter of a catch-all class counts as a hit there, so
        // these spans are kept apart
        k if CATCH_ALL.iter().any(|c| k.strip_prefix("param:") == Some(c)) => Some("X"),
        k if k.starts_with("param:") || k == "toggle" => Some("P"),
        "midi.note" | "midi.chord" | "midi.pattern" | "midi.cc" => Some("M"),
        "macro" => Some("C"),
        k if k.starts_with("midi.prop:") => Some("MP"),
        "xy" => Some("XY"),
        _ => None,
    }
}

#[derive(Default)]
struct Acc {
    n: BTreeMap<&'static str, f64>,
    r8: BTreeMap<&'static str, f64>,
    r1: BTreeMap<&'static str, f64>,
    keys_p: f64,
    shown: f64,
    junk: f64,
    noise: f64,
    false_ghost: f64,
    /// sum of per-span mean reciprocal ranks, and spans with an open list
    rr: f64,
    rr_n: f64,
    skipped: f64,
}

struct Span<'a> {
    hit: Option<usize>,
    ghost: bool,
    shown: usize,
    junk: usize,
    noise: usize,
    false_ghost: bool,
    rr: f64,
    rr_n: usize,
    _kind: &'a str,
}

fn type_span<'a>(e: &Engine, text: &str, b: usize, end: usize, kind: &'a str) -> Span<'a> {
    let mut s = Span { hit: None, ghost: false, shown: 0, junk: 0, noise: 0, false_ghost: false, rr: 0.0, rr_n: 0, _kind: kind };
    let ends: Vec<usize> = text[b..end].char_indices().map(|(i, c)| b + i + c.len_utf8()).collect();
    for (n, c) in ends.into_iter().enumerate() {
        let typed = &text[..c];
        let sugg = e.complete(typed, c, 0, K);
        let Some(top) = sugg.first() else { continue };
        let open = sugg.len() >= 2 && typed[top.replace.clone()].chars().count() >= MIN_TYPED;
        if open {
            s.shown += sugg.len();
            s.junk += sugg
                .iter()
                .filter(|x| matches!(&x.chip, Chip::Param { class, .. } if CATCH_ALL.contains(&class.as_str())) && x.via.is_some())
                .count();
            // fuzzy phrase rows of another class
            s.noise += sugg.iter().filter(|x| x.via.is_some() && !x.prefix && kind_of(&x.chip) != kind).count();
            s.rr += sugg.iter().position(|x| kind_of(&x.chip) == kind).map_or(0.0, |i| 1.0 / (i + 1) as f64);
            s.rr_n += 1;
        }
        let ghost = top.ghost_worthy(typed);
        if ghost && top.chip.id().starts_with("target:") {
            s.false_ghost = true;
        }
        if s.hit.is_none() && sugg.iter().any(|x| kind_of(&x.chip) == kind) {
            s.hit = Some(n + 1);
        }
        if ghost && kind_of(&top.chip) == kind {
            s.ghost = true;
        }
    }
    s
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_default();
    let dir = Path::new(&dir);
    let (_, plugins) = data::load(dir).unwrap_or_else(|e| panic!("{e}"));
    // one engine per demo plugin, with the classes it has
    let engines: Vec<(Engine, Vec<String>)> = plugins
        .iter()
        .map(|p| {
            let (mut e, _) = data::load(dir).unwrap_or_else(|e| panic!("{e}"));
            // SPAN_WEIGHT overrides the span prior's weight (probe sweeps)
            if let Some(w) = std::env::var("SPAN_WEIGHT").ok().and_then(|w| w.parse().ok()) {
                e.span_weight = w;
            }
            let params = data::load_plugin(dir, p).unwrap_or_else(|e| panic!("{e}"));
            let classes = params.iter().map(|q| q.class.clone()).collect();
            e.set_plugin(&p.plugin_id, params);
            (e, classes)
        })
        .collect();
    let mut turn: BTreeMap<String, usize> = BTreeMap::new();
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let r: Row = serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad row: {e}"));
        let a = acc.entry(r.split.clone()).or_default();
        for (b, end, kind) in &r.tgt {
            let Some(g) = group(kind) else { continue };
            let need = match kind.as_str() {
                "toggle" => Some(SWITCH_CLASS),
                k => k.strip_prefix("param:"),
            };
            let fit: Vec<usize> = (0..engines.len())
                .filter(|&i| need.is_none_or(|c| engines[i].1.iter().any(|x| x == c)))
                .collect();
            if fit.is_empty() {
                a.skipped += 1.0;
                continue;
            }
            let t = turn.entry(need.unwrap_or("*").to_string()).or_default();
            let e = &engines[fit[*t % fit.len()]].0;
            *t += 1;
            let s = type_span(e, &r.text, *b, *end, kind);
            *a.n.entry(g).or_default() += 1.0;
            *a.r8.entry(g).or_default() += f64::from(u8::from(s.hit.is_some()));
            *a.r1.entry(g).or_default() += f64::from(u8::from(s.ghost));
            if g == "P" {
                a.keys_p += s.hit.unwrap_or(0) as f64;
                a.shown += s.shown as f64;
                a.junk += s.junk as f64;
                a.noise += s.noise as f64;
                if s.rr_n > 0 {
                    a.rr += s.rr / s.rr_n as f64;
                    a.rr_n += 1.0;
                }
                a.false_ghost += f64::from(u8::from(s.false_ghost));
            }
        }
    }
    // mean over the sets that have spans of the measure's group
    let mut mean: BTreeMap<&str, (f64, f64)> = BTreeMap::new();
    for (split, a) in &acc {
        let raw = |g| a.n.get(g).copied().unwrap_or(0.0);
        let n = |g| raw(g).max(1.0);
        let m = [
            ("R8_P", a.r8.get("P").copied().unwrap_or(0.0) / n("P")),
            ("R1_P", a.r1.get("P").copied().unwrap_or(0.0) / n("P")),
            ("K_P", a.keys_p / a.r8.get("P").copied().unwrap_or(0.0).max(1.0)),
            ("J", a.junk / a.shown.max(1.0)),
            ("N", a.noise / a.shown.max(1.0)),
            ("MRR_P", a.rr / a.rr_n.max(1.0)),
            ("FG", a.false_ghost / n("P")),
            ("R8_M", a.r8.get("M").copied().unwrap_or(0.0) / n("M")),
            ("R1_M", a.r1.get("M").copied().unwrap_or(0.0) / n("M")),
            ("R8_C", a.r8.get("C").copied().unwrap_or(0.0) / n("C")),
            ("R8_X", a.r8.get("X").copied().unwrap_or(0.0) / n("X")),
            ("R8_MP", a.r8.get("MP").copied().unwrap_or(0.0) / n("MP")),
            ("R8_XY", a.r8.get("XY").copied().unwrap_or(0.0) / n("XY")),
        ];
        let line: Vec<String> = m.iter().map(|(k, v)| format!("{k} {v:.4}")).collect();
        println!(
            "  {split:9} n_P {} n_M {} n_C {} n_MP {} n_XY {} skipped {} {}",
            raw("P"), raw("M"), raw("C"), raw("MP"), raw("XY"), a.skipped, line.join(" ")
        );
        for (k, v) in m {
            let g = k.rsplit('_').next().unwrap_or("P");
            let g = if ["J", "N", "FG"].contains(&k) { "P" } else { g };
            if raw(g) > 0.0 {
                let e = mean.entry(k).or_default();
                e.0 += v;
                e.1 += 1.0;
            }
        }
    }
    let line: Vec<String> = mean.iter().map(|(k, (v, n))| format!("{k} {:.4}", v / n)).collect();
    println!("mean: {}", line.join(" "));
}
