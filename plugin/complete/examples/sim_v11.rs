//! Simulated user for probes/completion-v11 (see its RESULT.md): reads rows
//! from stdin (text, dumped frame with span bytes, gold SRC spans with their
//! sources, gold TGT spans with their classes, the gold sources), types each
//! SRC span until the top 3 hold its source, pins the chips on the frame and
//! writes per row the pinned frames (gestures only, parameters only, both)
//! and the gesture counts.
//!
//!   sim_v11 <completion dir> < rows.jsonl > out.jsonl

use std::io::{BufRead, Write};
use std::path::Path;

use serde::Deserialize;
use serde_json::json;
use synthexis_complete::{data, Chip, Engine, SIDES};
use synthexis_parser::{pin, Item, Pin, PinWhat};

#[derive(Deserialize)]
struct Row {
    text: String,
    pred: Vec<String>,
    pred_bytes: Vec<Option<(usize, usize)>>,
    /// [byte start, byte end, source, shape, side]
    src: Vec<(usize, usize, String, Option<String>, Option<String>)>,
    /// [byte start, byte end, class, switch]
    tgt: Vec<(usize, usize, String, bool)>,
    /// [byte start, byte end, target kind] of MIDI and macro spans
    /// (probes/completion-targets)
    #[serde(default)]
    tk: Vec<(usize, usize, String)>,
    gold_sources: Vec<String>,
}

fn fold(s: &str) -> &str {
    match s {
        "hand.fist" => "hand.openness",
        "eye.closed" => "eye.open",
        _ => s,
    }
}

fn items(r: &Row) -> Vec<Item> {
    r.pred
        .iter()
        .zip(&r.pred_bytes)
        .map(|(t, b)| match (t.strip_prefix('='), b) {
            (Some(s), Some(b)) => Item::Span { text: s.to_string(), bytes: *b },
            _ => Item::Token(t.clone()),
        })
        .collect()
}

/// Types text[b..e] char by char after text[..b]; the first chip of the
/// gold source in the top 3, and whether it was also the ghost text. The
/// side (probes/completion-side): SIM_SIDES=gold picks the gold side when
/// it is left or right, else the side the sentence names; SIM_SIDES=forced
/// as gold, but left where neither gives one (the field always asks);
/// SIM_SIDES=named only the side the sentence names; unset none.
fn type_span(e: &Engine, text: &str, b: usize, end: usize, src: &str, shape: Option<&str>, side: Option<&str>) -> Option<(Chip, bool)> {
    let mode = std::env::var("SIM_SIDES").unwrap_or_default();
    let ends = text[b..end].char_indices().map(|(i, c)| b + i + c.len_utf8());
    for c in ends {
        let typed = &text[..c];
        let sugg = e.complete(typed, c, 0, 3);
        for (k, s) in sugg.iter().enumerate() {
            if let Chip::Gesture { source, shape: sh, .. } = &s.chip {
                if fold(source) == src && (shape.is_none() || sh.as_deref() == shape) {
                    let gold = side.and_then(|x| SIDES.iter().position(|y| *y == x));
                    let v = s.variants;
                    let pick = match mode.as_str() {
                        "gold" if v.sided => gold.or(v.side),
                        "forced" if v.sided => gold.or(v.side).or(Some(0)),
                        "named" if v.sided => v.side,
                        _ => None,
                    };
                    return Some((s.chip.with_variant(pick, None), k == 0 && s.ghost_worthy(typed)));
                }
            }
        }
    }
    None
}

/// Types text[b..e] like type_span; true once the 8-item list holds a
/// target item of `kind`.
fn type_target(e: &Engine, text: &str, b: usize, end: usize, kind: &str) -> bool {
    text[b..end].char_indices().map(|(i, c)| b + i + c.len_utf8()).any(|c| {
        e.complete(&text[..c], c, 0, 8)
            .iter()
            .any(|s| matches!(&s.chip, Chip::Target { kind: k, .. } if k == kind))
    })
}

fn frame(items: &[Item], pins: &[Pin]) -> Vec<String> {
    pin(items, pins).frame().into_iter().filter(|t| !t.starts_with('@')).collect()
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_default();
    let (engine, _) = match data::load(Path::new(&dir)) {
        Ok(x) => x,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let r: Row = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("bad row: {err}");
                std::process::exit(1);
            }
        };
        let its = items(&r);
        let (mut gpins, mut hits, mut ghost) = (Vec::new(), 0, 0);
        for (b, e, src, shape, side) in &r.src {
            if let Some((chip, g)) = type_span(&engine, &r.text, *b, *e, src, shape.as_deref(), side.as_deref()) {
                hits += 1;
                ghost += usize::from(g);
                if let Chip::Gesture { source, shape, side, .. } = chip {
                    gpins.push(Pin { bytes: (*b, *e), what: PinWhat::Gesture { source, shape, side } });
                }
            }
        }
        let ppins: Vec<Pin> = r
            .tgt
            .iter()
            .enumerate()
            .map(|(k, (b, e, class, switch))| Pin {
                bytes: (*b, *e),
                what: PinWhat::Param { id: format!("p{k}"), class: class.clone(), switch: *switch },
            })
            .collect();
        // false ghosts: word starts outside SRC spans, typed up to 8 chars
        let in_src = |i: usize| r.src.iter().any(|(b, e, ..)| i >= *b && i < *e);
        let (mut words, mut false_ghost) = (0, 0);
        let mut prev_ws = true;
        for (i, ch) in r.text.char_indices() {
            let start = prev_ws && !ch.is_whitespace();
            prev_ws = ch.is_whitespace();
            if !start || in_src(i) {
                continue;
            }
            words += 1;
            let word_end = r.text[i..].find(char::is_whitespace).map_or(r.text.len(), |n| i + n);
            let shown = r.text[i..word_end].char_indices().take(8).any(|(k, c)| {
                let end = i + k + c.len_utf8();
                let typed = &r.text[..end];
                engine.complete(typed, end, 0, 1).first().is_some_and(|s| {
                    s.ghost_worthy(typed)
                        && matches!(&s.chip, Chip::Gesture { source, .. } if !r.gold_sources.iter().any(|g| g == fold(source)))
                })
            });
            false_ghost += usize::from(shown);
        }
        let all: Vec<Pin> = gpins.iter().chain(&ppins).cloned().collect();
        let tpins: Vec<Pin> = r
            .tk
            .iter()
            .filter(|(b, e, k)| type_target(&engine, &r.text, *b, *e, k))
            .map(|(b, e, k)| Pin { bytes: (*b, *e), what: PinWhat::Target { kind: k.clone() } })
            .collect();
        let with_t: Vec<Pin> = all.iter().chain(&tpins).cloned().collect();
        let o = json!({
            "G": frame(&its, &gpins),
            "P": frame(&its, &ppins),
            "GP": frame(&its, &all),
            "GPT": frame(&its, &with_t),
            "tk_total": r.tk.len(),
            "tk_hits": tpins.len(),
            "src_total": r.src.len(),
            "src_hits": hits,
            "src_ghost": ghost,
            "words": words,
            "false_ghost": false_ghost,
        });
        if writeln!(out, "{o}").is_err() {
            std::process::exit(1);
        }
    }
}
