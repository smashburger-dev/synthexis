//! Engine tests on a small synthetic vocabulary and synthetic sentences
//! (never the real-sentence sets in data/mapping/eval/).

use std::collections::HashMap;
use std::time::Instant;

use super::*;

fn g(source: &str, phrase: &str) -> Gesture {
    Gesture { source: source.into(), shape: None, phrase: phrase.into(), pol: 1 }
}

fn p(index: u32, name: &str, class: &str) -> Param {
    Param { index, name: name.into(), class: class.into() }
}

fn engine() -> Engine {
    let gestures = vec![
        g("hand.spread", "spread my fingers"),
        g("hand.spread", "finger spread"),
        g("hand.fist", "Faust schließen"),
        g("hand.fist", "make a fist"),
        g("mouth.open", "open my mouth"),
        g("head.roll", "tilt my head"),
        Gesture { source: "hand.shape".into(), shape: Some("peace".into()), phrase: "peace sign".into(), pol: 1 },
    ];
    let phrases = [
        ("filter.cutoff", "Cutoff"),
        ("filter.cutoff", "Filter aufmachen"),
        ("filter.cutoff", "open the filter"),
        ("fx.reverb.mix", "Hall"),
        ("fx.reverb.mix", "reverb amount"),
        ("ctl.switch", "Bypass"),
        ("ctl.switch", "on/off switch"),
    ]
    .iter()
    .map(|(c, p)| (c.to_string(), p.to_string()))
    .collect();
    let prior: HashMap<String, u64> = [("filter.cutoff", 900), ("fx.reverb.mix", 400), ("ctl.switch", 2000), ("system", 9000)]
        .iter()
        .map(|(c, n)| (c.to_string(), *n))
        .collect();
    let mut e = Engine::new(gestures, phrases, &prior);
    e.set_plugin(
        "Synth/Test",
        vec![
            p(0, "Filter 1 Freq", "filter.cutoff"),
            p(1, "Filter 2 Freq", "filter.cutoff"),
            p(2, "Filter 1 On", "ctl.switch"),
            p(3, "Reverb Mix", "fx.reverb.mix"),
            p(4, "Reverb On", "ctl.switch"),
            p(5, "Spread", "stereo.width"),
            p(6, "Filter Routing Internal", "system"),
        ],
    );
    e
}

fn top(e: &Engine, text: &str) -> Vec<Suggestion> {
    e.complete(text, text.len(), 0, 3)
}

fn labels(s: &[Suggestion]) -> Vec<&str> {
    s.iter().map(|s| s.chip.label()).collect()
}

#[test]
fn cut_offers_filter_freq() {
    let e = engine();
    let s = top(&e, "the cut");
    assert_eq!(labels(&s)[..2], ["Filter 1 Freq", "Filter 2 Freq"]);
    assert_eq!(s[0].via.as_deref(), Some("Cutoff"));
    assert_eq!(&"the cut"[s[0].replace.clone()], "cut");
}

#[test]
fn three_rows_per_kind_then_other_kinds() {
    let mut e = engine();
    let filters = (0..6).map(|i| p(i, &format!("Filter {i} Freq"), "filter.cutoff"));
    e.set_plugin("Synth/Test", filters.chain([p(9, "Filter Drive", "fx.drive")]).collect());
    let s = e.complete("the fil", 7, 0, 5);
    assert!(s[..3].iter().all(|s| kind(&s.chip) == "filter.cutoff"));
    assert_eq!(s[3].chip.label(), "Filter Drive");
    assert_eq!(s[4].chip.label(), "Filter 3 Freq");
}

#[test]
fn catch_all_phrases_offer_nothing() {
    let phrases = vec![("other".to_string(), "the gap before the reverb starts".to_string())];
    let mut e = Engine::new(Vec::new(), phrases, &HashMap::new());
    e.set_plugin("Synth/Test", vec![p(0, "Swing", "other"), p(1, "Filter 1 Res", "filter.resonance")]);
    assert_eq!(labels(&top(&e, "the res")), ["Filter 1 Res"]);
    assert_eq!(labels(&top(&e, "the swi")), ["Swing"]);
}

fn with_targets(e: Engine) -> Engine {
    let t = [("midi.chord", "Akkord spielen"), ("midi.chord", "play a chord"), ("midi.pattern", "fire pattern")];
    e.with_targets(t.iter().map(|(k, p)| (k.to_string(), p.to_string())).collect())
}

#[test]
fn target_items_in_the_list_only() {
    let e = with_targets(engine());
    let s = e.complete("then Akk", 8, 0, 8);
    assert_eq!(s[0].chip, Chip::Target { kind: "midi.chord".into(), phrase: "Akkord spielen".into() });
    assert_eq!(s[0].chip.id(), "target:midi.chord");
    assert!(!s[0].ghost_worthy("then Akk"));
}

#[test]
fn target_item_leaves_the_ghost_row() {
    // "fi" starts "fire pattern" and "Filter 1 Freq"; the parameter keeps row 0
    let e = with_targets(engine());
    let s = e.complete("the fi", 6, 0, 8);
    assert!(s[0].ghost_worthy("the fi"));
    assert!(matches!(s[0].chip, Chip::Param { .. }));
    assert!(s.iter().any(|x| matches!(x.chip, Chip::Target { .. })));
}

#[test]
fn span_prior_ranks_the_class_training_meant() {
    let e = engine();
    assert_eq!(top(&e, "the filter")[0].chip.label(), "Filter 1 Freq");
    let rows = vec![("Filter".to_string(), "ctl.switch".to_string(), 9), ("filter cutoff".to_string(), "filter.cutoff".to_string(), 1)];
    let e = engine().with_span_prior(rows);
    assert_eq!(top(&e, "the filter")[0].chip.label(), "Filter 1 On");
}

#[test]
fn german_phrase_over_two_words() {
    let e = engine();
    let t = "mit der Faust Filter auf";
    let s = top(&e, t);
    assert_eq!(s[0].chip.label(), "Filter 1 Freq");
    assert_eq!(&t[s[0].replace.clone()], "Filter auf");
}

#[test]
fn gesture_after_when_i() {
    let e = engine();
    let s = top(&e, "when I spr");
    assert_eq!(s[0].chip.label(), "spread my fingers");
    assert_eq!(s[0].chip.id(), "gesture:hand.spread");
    assert_eq!(s[0].ghost_suffix("when I spr").as_deref(), Some("ead my fingers"));
    assert!(s[0].ghost_worthy("when I spr"));
}

#[test]
fn gesture_german_after_wenn_ich_die() {
    let e = engine();
    let s = top(&e, "Cutoff hoch, wenn ich die Fau");
    assert_eq!(s[0].chip.label(), "Faust schließen");
}

#[test]
fn weak_position_lists_gesture_without_ghost() {
    let e = engine();
    let t = "i want my mou";
    let s = top(&e, t);
    assert_eq!(s[0].chip.id(), "gesture:mouth.open");
    assert!(s[0].weak);
    assert!(!s[0].ghost_worthy(t));
    let t = "cutoff hoch wenn ich die linke Fau";
    let s = top(&e, t);
    assert_eq!(s[0].chip.label(), "Faust schließen");
    assert!(s[0].ghost_worthy(t));
}

#[test]
fn gesture_at_sentence_start() {
    let e = engine();
    assert_eq!(top(&e, "Fau")[0].chip.label(), "Faust schließen");
}

#[test]
fn no_gesture_outside_gesture_position() {
    let e = engine();
    let s = top(&e, "the reverb gets spr");
    assert!(s.iter().all(|s| matches!(s.chip, Chip::Param { .. })), "{:?}", labels(&s));
    assert_eq!(s[0].chip.label(), "Spread");
}

#[test]
fn one_entry_per_gesture() {
    let e = engine();
    let s = top(&e, "when I f");
    let fingers = s.iter().filter(|s| s.chip.id() == "gesture:hand.spread").count();
    assert!(fingers <= 1, "{:?}", labels(&s));
}

#[test]
fn switching_words_rank_switches_up() {
    let e = engine();
    let plain = top(&e, "the reverb follows Rev");
    assert_eq!(plain[0].chip.label(), "Reverb Mix");
    let s = top(&e, "schalte mit der Faust Rev");
    assert_eq!(s[0].chip.label(), "Reverb On");
    assert!(s[0].chip.is_switch());
}

#[test]
fn system_params_rank_last() {
    let e = engine();
    let s = e.complete("the Filter", 10, 0, 10);
    let pos = |l: &str| s.iter().position(|x| x.chip.label() == l).unwrap_or(99);
    assert!(pos("Filter Routing Internal") > pos("Filter 1 Freq"));
}

#[test]
fn recent_pick_ranks_up() {
    let mut e = engine();
    let before = top(&e, "the cut");
    assert_eq!(before[0].chip.label(), "Filter 1 Freq");
    e.picked(&before[1].chip);
    assert_eq!(top(&e, "the cut")[0].chip.label(), "Filter 2 Freq");
}

#[test]
fn nothing_after_space_or_below_threshold() {
    let e = engine();
    assert!(top(&e, "when I ").is_empty());
    let s = top(&e, "the c");
    assert!(s.iter().all(|s| !s.ghost_worthy("the c")));
}

#[test]
fn fuzzy_ghost_needs_score() {
    let e = engine();
    let s = top(&e, "the fq");
    // "fq" reaches Filter 1 Freq only through a weak subsequence
    assert!(s.first().is_none_or(|s| !s.ghost_worthy("the fq")));
}

#[test]
fn query_stops_at_chip() {
    let e = engine();
    let t = "spread my fingers cu";
    let s = e.complete(t, t.len(), 17, 3);
    assert!(s.iter().all(|s| s.replace.start >= 17));
}

#[test]
fn shape_chip_carries_shape() {
    let e = engine();
    let s = top(&e, "with a pea");
    assert_eq!(s[0].chip.id(), "gesture:hand.shape/peace");
}

/// Synthetic vocabulary at the real sizes (2,700 gesture phrases, 2,100
/// class phrases, a 2,600-parameter plugin): a keystroke stays in the low
/// milliseconds even unoptimised.
#[test]
fn keystroke_latency() {
    let words = [
        "spread", "fist", "open", "close", "raise", "tilt", "mouth", "hand", "finger", "head",
        "pinch", "filter", "reverb", "delay", "cutoff", "drive", "mix", "level", "macro", "env",
    ];
    let w = |i: usize| words[i % words.len()];
    let gestures = (0..2700).map(|i| g("hand.x", &format!("{} my {} {}", w(i), w(i / 7), w(i / 3)))).collect();
    let phrases = (0..2100).map(|i| (format!("c{}", i % 40), format!("{} {} {i}", w(i / 5), w(i)))).collect();
    let mut e = Engine::new(gestures, phrases, &HashMap::new());
    e.set_plugin("Big/Synth", (0..2600).map(|i| p(i, &format!("{} {} {i}", w(i as usize), w(i as usize / 3)), &format!("c{}", i % 40))).collect());
    let text = "when I spread my fingers the filter cutoff goes up";
    let mut ms = Vec::new();
    for end in 8..text.len() {
        let t0 = Instant::now();
        let _ = e.complete(text, end, 0, 3);
        ms.push(t0.elapsed().as_secs_f64() * 1e3);
    }
    ms.sort_by(f64::total_cmp);
    let p95 = ms[ms.len() * 95 / 100];
    eprintln!("keystroke median {:.2} ms, p95 {p95:.2} ms", ms[ms.len() / 2]);
    assert!(p95 < 25.0, "p95 {p95:.2} ms");
}
