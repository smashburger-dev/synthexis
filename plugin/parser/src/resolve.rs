//! Resolver: binds the parameter targets of a pinned frame to parameters of
//! the picked plugin, in the order of docs/parser-runtime-plan.md (Resolver
//! row): a chip pin, then a user alias, then the target's words against the
//! parameter names, then the parser's class against the parameter classes
//! (labels of a mapped plugin or param-sense). Only the picked plugin's
//! parameters are candidates; the picker is worth 10 to 14 points of top-1
//! (probes/open-vocab-mapping/picker.py).
//!
//! The words of a target: the BIO head's TGT and PNAME pieces joined into
//! name groups (see `groups`), plus the frame's PNAME span next to it.
//! Name similarity is resolve.py's sim: char 3-gram Dice plus word-prefix
//! cover, digits must match. Measured in probes/resolver/RESULT.md.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::align;
use crate::pin::{Frame, Part};
use crate::verb;
use crate::{Item, Tagged};

/// Words a user puts around a parameter name (resolve.py FILLER).
/// Plus the verbs of a request ("turn up the power"), from the dev seed.
const FILLER: [&str; 25] = [
    "the", "a", "an", "my", "knob", "slider", "parameter", "param", "die", "der", "das", "den", "dem", "mehr", "more",
    "regler", "knopf", "wert", "of", "von", "turn", "up", "down", "hoch", "runter",
];
/// A name match below this falls through to the class.
pub const NAME_MIN: f32 = 0.5;
/// A name match this strong overrides the frame's reading of the target.
pub const MERGE_MIN: f32 = 0.8;
/// A name of another class beats the class only this close to exact.
pub const EXACT_MIN: f32 = 0.85;
/// Spoken numbers, said where a name has digits ("macro two" -> MACRO 2).
const NUMBERS: [[&str; 2]; 8] = [
    ["one", "eins"], ["two", "zwei"], ["three", "drei"], ["four", "vier"], ["five", "fünf"], ["six", "sechs"],
    ["seven", "sieben"], ["eight", "acht"],
];
/// v11's classes for a target that is only a name ("B Warp", "Macro 2"):
/// they never outvote the name.
const NAME_ONLY: [&str; 2] = ["other", "macro"];
/// Words of a plugin's main controls, preferred among equals of a class.
const MAIN: [&str; 4] = ["master", "main", "global", "output"];
/// param-sense's class for on/off parameters (synthexis_complete::SWITCH_CLASS).
const SWITCH_CLASS: &str = "ctl.switch";

/// A parameter of the picked plugin.
#[derive(Clone, Copy, Debug)]
pub struct Candidate<'a> {
    pub index: u32,
    pub name: &'a str,
    pub class: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Via {
    Chip,
    Alias,
    Name,
    Class,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bound {
    pub index: u32,
    pub name: String,
    pub via: Via,
    /// The chip's plugin (fl::Key id) when it is not the picked one: the
    /// chip was picked before the picker moved on.
    pub plugin: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    /// The frame's T: token.
    pub token: String,
    /// The words that named it (PNAME or TGT text), "" when none.
    pub text: String,
    /// The parameter, for T:param and T:toggle targets that found one.
    pub bound: Option<Bound>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Route {
    /// Source id without "S:", "" when the frame has none.
    pub source: String,
    /// side: of the route, else the one outside the routes (it holds for
    /// all of them); left means the left hand only.
    pub side: Option<String>,
    pub inv: bool,
    /// Gates ("only while I pinch"): each G: source with its side and
    /// shape tokens, e.g. ["G:hand.pinch", "side:left"].
    pub gates: Vec<Vec<String>>,
    /// The route's modifiers as the frame has them (lo:, hi:, amt:, mode:,
    /// curve:, smooth:, hold:, quantize:, delta:, BIP, grid:), in frame
    /// order, then the values a MIDI target needs: the source's shape
    /// (shape:peace) and the pointer values after NOTE and CHORD (note:g2,
    /// chord:Am).
    pub mods: Vec<String>,
    pub targets: Vec<Target>,
}

const MODS: [&str; 11] = ["lo:", "hi:", "amt:", "mode:", "curve:", "smooth:", "hold:", "quantize:", "delta:", "BIP", "grid:"];

/// G: tokens with the side:/shape: tokens right after them.
fn gates(items: &[Item]) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut open = false;
    for t in items.iter().map(tok) {
        if t.starts_with("G:") {
            out.push(vec![t.to_string()]);
            open = true;
        } else if open && (t.starts_with("side:") || t.starts_with("shape:")) {
            if let Some(g) = out.last_mut() {
                g.push(t.to_string());
            }
        } else {
            open = false;
        }
    }
    out
}

/// The plugin name of an fl::Key id ("fx:0:1:Portal", "gen:2:Serum 2").
fn plugin_name(id: &str) -> &str {
    let n = if id.starts_with("fx:") { 3 } else { 2 };
    id.splitn(n + 1, ':').nth(n).unwrap_or(id)
}

fn percent(v: &str) -> String {
    v.parse::<f32>().map_or_else(|_| v.to_string(), |x| format!("{:.0}", x * 100.0))
}

fn tok(it: &Item) -> &str {
    match it {
        Item::Token(t) => t,
        Item::Span { .. } => "",
    }
}

/// resolve.py words(): camelCase split, casefold, runs of [0-9a-zäöüß].
fn words(s: &str) -> Vec<String> {
    let mut split = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if prev_lower && c.is_ascii_uppercase() {
            split.push(' ');
        }
        prev_lower = c.is_ascii_lowercase();
        split.push(c);
    }
    let folded = split.to_lowercase().replace('ß', "ss");
    folded
        .split(|c: char| !(c.is_ascii_digit() || c.is_ascii_lowercase() || "äöü".contains(c)))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn grams(ws: &[String]) -> Vec<String> {
    let t: Vec<char> = format!(" {} ", ws.join(" ")).chars().collect();
    let mut g: Vec<String> = t.windows(3).map(|w| w.iter().collect()).collect();
    g.sort();
    g.dedup();
    g
}

/// The words of a target text that can name a parameter.
fn content(text: &str) -> Vec<String> {
    words(text)
        .into_iter()
        .filter(|w| !FILLER.contains(&w.as_str()))
        .map(|w| NUMBERS.iter().position(|n| n.contains(&w.as_str())).map_or(w, |i| (i + 1).to_string()))
        .collect()
}

/// resolve.py sim(): 0.5 Dice over char 3-grams + 0.5 share of the
/// span's words that equal or start a word of the name (or the reverse, 3+
/// chars); +0.2 when every digit the span says is in the name, -0.2 when
/// not. Unlike resolve.py: request verbs are filler, a single letter counts
/// when equal ("b warp"), number words count as digits.
pub fn sim(span: &str, name: &str) -> f32 {
    let (a, b) = (content(span), words(name));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (ga, gb) = (grams(&a), grams(&b));
    let common = ga.iter().filter(|g| gb.binary_search(g).is_ok()).count();
    let dice = 2.0 * common as f32 / (ga.len() + gb.len()) as f32;
    let cover = a
        .iter()
        .filter(|w| b.iter().any(|x| x == *w || (x.len().min(w.len()) >= 3 && (x.starts_with(w.as_str()) || w.starts_with(x.as_str())))))
        .count() as f32
        / a.len() as f32;
    let nums: Vec<&String> = a.iter().filter(|w| w.chars().all(|c| c.is_ascii_digit())).collect();
    let digit = if nums.is_empty() {
        0.0
    } else if nums.iter().all(|n| b.contains(n)) {
        0.2
    } else {
        -0.2
    };
    0.5 * dice + 0.5 * cover + digit
}

/// Alias key of a phrase: its content words joined by spaces.
pub fn alias_key(phrase: &str) -> String {
    content(phrase).join(" ")
}

/// INV as the sentence's verb says it: a lowering verb on the target
/// ("pinch my fingers decrease the filter", "Filter runter") inverts the
/// route, unless the movement is said falling too ("close my fist to lower
/// the drive": the fist is the openness inverted already, so the route is
/// straight). A raising verb or none keeps the parser's INV. Only for
/// continuous targets: a MIDI part or a toggle has no direction.
fn verb_inv(parsed: bool, sentence: &str, src: &[(usize, usize)], source: &str, targets: &[Target], anchors: &[(usize, usize)]) -> bool {
    let continuous = |t: &Target| ["T:param:", "T:macro:", "T:midi.cc:"].iter().any(|p| t.token.starts_with(p));
    if source.is_empty() || source == "hand.shape" || targets.is_empty() || !targets.iter().all(continuous) {
        return parsed;
    }
    let names: Vec<String> = targets.iter().filter_map(|t| t.bound.as_ref()).flat_map(|b| words(&b.name)).collect();
    match verb::target_dir(sentence, src, anchors, &names) {
        Some(verb::Dir::Down) => !verb::source_falls(sentence, source),
        _ => parsed,
    }
}

fn is_param_target(t: &str) -> bool {
    t.starts_with("T:param:") || t.starts_with("T:toggle:")
}

/// The text of `sentence` in `b`, with the PLUGIN and TRACK spans inside it
/// blanked out (resolve.py's cover does this for PLUGIN).
fn cut(sentence: &str, b: (usize, usize), spans: &[Tagged]) -> String {
    let mut s = sentence.get(b.0..b.1).unwrap_or("").to_string();
    for t in spans.iter().filter(|t| matches!(t.tag, "PLUGIN" | "TRACK")) {
        if b.0 <= t.bytes.0 && t.bytes.1 <= b.1 {
            if let Some(inner) = sentence.get(t.bytes.0..t.bytes.1) {
                s = s.replacen(inner, " ", 1);
            }
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text of a pointer value as the sentence writes it ("Am", where the
/// span's text is the normalised "am"); the span's own text when it has
/// no bytes (rules.rs). None for a token.
fn span_text(it: &Item, sentence: &str, spans: &[Tagged]) -> Option<String> {
    let Item::Span { text, bytes } = it else { return None };
    let s = cut(sentence, *bytes, spans);
    let s = s.trim_matches(|c: char| ".,;:!?\"'()".contains(c));
    Some(if s.is_empty() { text.clone() } else { s.to_string() })
}

/// The PNAME span text that belongs to the target at `item`: the PNAME
/// whose nearest parameter target it is.
fn pname_for(items: &[Item], item: usize) -> Option<String> {
    let targets: Vec<usize> = (0..items.len()).filter(|&i| tok(&items[i]).starts_with("T:")).collect();
    (0..items.len().saturating_sub(1))
        .filter(|&i| tok(&items[i]) == "PNAME")
        .filter(|&i| targets.iter().min_by_key(|&&t| t.abs_diff(i)) == Some(&item))
        .find_map(|i| match &items[i + 1] {
            Item::Span { text, .. } => Some(text.clone()),
            Item::Token(_) => None,
        })
}

/// The best parameter by name for `texts`; class agreement only breaks
/// ties, then the lower index.
fn by_name<'a>(texts: &[String], pool: &[&'a Candidate<'a>], class: Option<&str>) -> Option<(f32, &'a Candidate<'a>)> {
    pool.iter()
        .map(|p| {
            let s = texts.iter().map(|t| sim(t, p.name)).fold(0.0f32, f32::max);
            (s + if class == Some(p.class) { 0.01 } else { 0.0 }, *p)
        })
        .fold(None, |a: Option<(f32, &Candidate)>, b| match a {
            Some(a) if a.0 >= b.0 => Some(a),
            _ => Some(b),
        })
}

/// Binds one target; `texts` are the words that named it. Returns the
/// binding and its name score (0 unless bound by name).
fn bind(
    token: &str,
    pinned: Option<&str>,
    texts: &[String],
    plugin: &str,
    params: &[Candidate],
    aliases: &HashMap<String, String>,
) -> (Option<Bound>, f32) {
    let found = |index: u32, via| params.iter().find(|p| p.index == index).map(|p| Bound { index, name: p.name.into(), via, plugin: None });
    let all: Vec<&Candidate> = params.iter().collect();
    // a macro or CC guess that names a parameter of the picked plugin
    // ("slot 18 target" read as macro 18) is that parameter
    if !is_param_target(token) {
        if !(token.starts_with("T:macro:") || token.starts_with("T:midi.cc:")) || pinned.is_some() {
            return (None, 0.0);
        }
        return match by_name(texts, &all, None) {
            Some((s, p)) if s >= MERGE_MIN => (Some(Bound { index: p.index, name: p.name.into(), via: Via::Name, plugin: None }), s),
            _ => (None, 0.0),
        };
    }
    // 1. chip: "param:<plugin id>#<index>"
    if let Some(id) = pinned {
        let Some((pl, index)) = id.strip_prefix("param:").and_then(|x| x.rsplit_once('#')) else { return (None, 0.0) };
        let Ok(index) = index.parse::<u32>() else { return (None, 0.0) };
        let b = if pl == plugin {
            found(index, Via::Chip).unwrap_or(Bound { index, name: format!("#{index}"), via: Via::Chip, plugin: None })
        } else {
            Bound { index, name: format!("#{index}"), via: Via::Chip, plugin: Some(pl.to_string()) }
        };
        return (Some(b), 0.0);
    }
    let toggle = token.strip_prefix("T:toggle:");
    let class = token.strip_prefix("T:param:");
    // 2. alias
    for t in texts {
        if let Some(name) = aliases.get(&alias_key(t)) {
            if let Some(p) = params.iter().find(|p| p.name == name) {
                return (Some(Bound { index: p.index, name: p.name.into(), via: Via::Alias, plugin: None }), 0.0);
            }
        }
    }
    // 3. name; a toggle looks among the switches first
    let switches: Vec<&Candidate> = params.iter().filter(|p| p.class == SWITCH_CLASS).collect();
    let pool = if toggle.is_some() && !switches.is_empty() { &switches } else { &all };
    // a weaker name of another class loses to the class when the plugin
    // has parameters of it ("cutoff" -> "Cutoff Rand" vs "Filter 1 Freq")
    let has_class = class.is_some_and(|c| params.iter().any(|p| p.class == c));
    if let Some((s, p)) = by_name(texts, pool, class) {
        let agrees = class == Some(p.class) || !has_class || class.is_some_and(|c| NAME_ONLY.contains(&c));
        if s >= EXACT_MIN || (s >= NAME_MIN && agrees) {
            return (Some(Bound { index: p.index, name: p.name.into(), via: Via::Name, plugin: None }), s);
        }
    }
    // 4. class: the parameters of the target's class, best name first; a
    // toggle takes a switch that names its effect ("Reverb On")
    let of_class: Vec<&Candidate> = match (class, toggle) {
        (Some(c), _) => params.iter().filter(|p| p.class == c).collect(),
        (None, Some(fx)) => switches.iter().copied().filter(|p| sim(fx, p.name) > 0.0).collect(),
        _ => Vec::new(),
    };
    let texts: Vec<String> = texts.iter().cloned().chain(toggle.map(str::to_string)).collect();
    // on a tie the plugin's main control wins, the shorter name first
    // ("mix" -> MASTER DRY/WET, not GRAIN or MASTER COMPRESSOR DRY/WET)
    let main = |p: &Candidate| words(p.name).iter().any(|w| MAIN.contains(&w.as_str()));
    let b = of_class
        .iter()
        .map(|p| {
            let s = texts.iter().map(|t| sim(t, p.name)).fold(0.0f32, f32::max);
            (s + if main(p) { 0.01 } else { 0.0 } - 0.001 * words(p.name).len() as f32, *p)
        })
        .fold(None, |a: Option<(f32, &Candidate)>, b| match a {
            Some(a) if a.0 >= b.0 => Some(a),
            _ => Some(b),
        })
        .map(|(_, p)| Bound { index: p.index, name: p.name.into(), via: Via::Class, plugin: None });
    (b, 0.0)
}

/// Routes of `frame` with their targets bound to `params` of `plugin` (a
/// fl::Key id, as chip pins carry it). `spans`: the parse's BIO spans;
/// `aliases`: alias_key(phrase) -> parameter name for this plugin.
///
/// Per route its name groups pair with its T: tokens in order. More tokens
/// than groups: the last group names the rest too, and when it names one
/// parameter by at least MERGE_MIN the extra tokens go (the parser read one
/// long name as several classes). More groups than tokens: an extra group
/// that names a parameter by at least MERGE_MIN becomes a target.
pub fn resolve(
    frame: &Frame,
    sentence: &str,
    spans: &[Tagged],
    plugin: &str,
    params: &[Candidate],
    aliases: &HashMap<String, String>,
) -> Vec<Route> {
    let shared = frame.parts.iter().find_map(|p| match p {
        Part::Item(it) => tok(it).strip_prefix("side:").map(str::to_string),
        Part::Route(_) => None,
    });
    let al = align::align(frame, sentence, spans);
    let edits = frame.frame().iter().any(|t| matches!(t.as_str(), "I:map" | "I:modify" | "I:replace"));
    let src: Vec<(usize, usize)> = spans.iter().filter(|t| t.tag == "SRC").map(|t| t.bytes).collect();
    let texts: Vec<String> = al.groups.iter().map(|b| cut(sentence, *b, spans)).collect();
    frame
        .routes()
        .zip(&al.routes)
        .map(|(route, ra)| {
            let find = |p: &str| route.items.iter().find_map(|it| tok(it).strip_prefix(p).map(str::to_string));
            // the side and shape that follow the S: token, not a gate's
            let after_source: Vec<&str> = route
                .items
                .iter()
                .map(tok)
                .skip_while(|t| !t.starts_with("S:"))
                .skip(1)
                .take_while(|t| ["side:", "shape:", "object:"].iter().any(|p| t.starts_with(p)))
                .collect();
            let side = after_source.iter().find_map(|t| t.strip_prefix("side:").map(str::to_string));
            let shape = after_source.iter().find(|t| t.starts_with("shape:")).map(|t| t.to_string());
            let mut targets = Vec::new();
            let mut k = 0;
            while k < ra.slots.len() {
                let slot = &ra.slots[k];
                let item = slot.item;
                // the slots after it that share its group
                let rest = ra.slots[k + 1..].iter().take_while(|s| s.shares).count();
                let token = tok(&route.items[item]).to_string();
                let words: Vec<String> = slot
                    .group
                    .map(|g| texts[g].clone())
                    .into_iter()
                    .chain(pname_for(&route.items, item))
                    .filter(|t| !t.is_empty())
                    .collect();
                let (bound, score) = bind(&token, route.pinned[item].as_deref(), &words, plugin, params, aliases);
                let merged = rest > 0
                    && score >= MERGE_MIN
                    && ra.slots[k + 1..=k + rest].iter().all(|s| route.pinned[s.item].is_none());
                targets.push(Target { token, text: words.first().cloned().unwrap_or_default(), bound });
                k += if merged { rest + 1 } else { 1 };
            }
            // a target bound to a parameter another target of the route
            // already drives is a misread of the same words (pin.rs keeps a
            // slot that only inherited a chip's group); the chip's wins
            let chip_of = |t: &Target| t.bound.as_ref().filter(|b| b.via == Via::Chip).map(|b| (b.index, b.plugin.clone()));
            let chipped: Vec<(u32, Option<String>)> = targets.iter().filter_map(chip_of).collect();
            let mut seen: Vec<(u32, Option<String>)> = Vec::new();
            targets.retain(|t| match &t.bound {
                Some(b) if b.via == Via::Chip => true,
                Some(b) => {
                    let k = (b.index, b.plugin.clone());
                    let keep = !chipped.contains(&k) && !seen.contains(&k);
                    seen.push(k);
                    keep
                }
                None => true,
            });
            let all: Vec<&Candidate> = params.iter().collect();
            for &g in &ra.extra {
                if let Some((s, p)) = by_name(std::slice::from_ref(&texts[g]), &all, None) {
                    if s >= MERGE_MIN {
                        let bound = Some(Bound { index: p.index, name: p.name.into(), via: Via::Name, plugin: None });
                        targets.push(Target { token: format!("T:param:{}", p.class), text: texts[g].clone(), bound });
                    }
                }
            }
            Route {
                source: find("S:").unwrap_or_default(),
                side: side.or_else(|| shared.clone()),
                inv: {
                    let parsed = route.items.iter().any(|it| tok(it) == "INV");
                    let source = find("S:").unwrap_or_default();
                    let pnames = route.items.windows(2).filter(|w| tok(&w[0]) == "PNAME").filter_map(|w| match &w[1] {
                        Item::Span { bytes, .. } => Some(*bytes),
                        Item::Token(_) => None,
                    });
                    let anchors: Vec<(usize, usize)> = ra
                        .slots
                        .iter()
                        .filter_map(|s| s.group)
                        .chain(ra.extra.iter().copied())
                        .filter_map(|g| al.groups.get(g).copied())
                        .chain(pnames)
                        .collect();
                    if edits { verb_inv(parsed, sentence, &src, &source, &targets, &anchors) } else { parsed }
                },
                gates: gates(&route.items),
                mods: {
                    let toks: Vec<&str> = route.items.iter().map(tok).collect();
                    let mut m: Vec<String> =
                        toks.iter().filter(|t| MODS.iter().any(|p| t.starts_with(p))).map(|t| t.to_string()).collect();
                    m.extend(shape);
                    // the value after NOTE and CHORD is a pointer span
                    for w in route.items.windows(2) {
                        match (tok(&w[0]), span_text(&w[1], sentence, spans)) {
                            ("NOTE", Some(v)) => m.push(format!("note:{v}")),
                            ("CHORD", Some(v)) => m.push(format!("chord:{v}")),
                            _ => {}
                        }
                    }
                    m
                },
                targets,
            }
        })
        .collect()
}

/// The words of a route line, so the plugin can show it in its language.
/// Templates fill "{}" in order. Source ids and parameter names stay as
/// they are.
#[derive(Clone, Copy, Debug)]
pub struct Words {
    pub left: &'static str,
    pub right: &'static str,
    pub both_hands: &'static str,
    /// "left hand openness" (true) or "hand openness (links)" (false).
    pub side_first: bool,
    pub inverted: &'static str,
    pub no_parameter: &'static str,
    pub range: &'static str,
    pub amount: &'static str,
    pub steps: &'static str,
    pub toggles: &'static str,
    pub while_held: &'static str,
    pub ramps: &'static str,
    pub curve: &'static str,
    pub snappy: &'static str,
    pub smooth: &'static str,
    pub smoothing: &'static str,
    pub holds: &'static str,
    pub freezes: &'static str,
    pub resets: &'static str,
    pub quantize: &'static str,
    pub centred: &'static str,
    pub while_: &'static str,
}

pub const EN_WORDS: Words = Words {
    left: "left",
    right: "right",
    both_hands: "both hands",
    side_first: true,
    inverted: "inverted",
    no_parameter: "{} (no parameter)",
    range: "range {}-{} %",
    amount: "amount {} %",
    steps: "steps",
    toggles: "toggles",
    while_held: "while held",
    ramps: "ramps",
    curve: "{} curve",
    snappy: "snappy",
    smooth: "smooth",
    smoothing: "{} smoothing",
    holds: "holds",
    freezes: "freezes",
    resets: "resets",
    quantize: "{} steps",
    centred: "centred",
    while_: "while {}",
};

/// `template` with each "{}" replaced by the next of `args`.
fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::new();
    let mut args = args.iter();
    let mut rest = template;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        out.push_str(args.next().copied().unwrap_or_default());
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

impl Words {
    fn side(&self, s: &str) -> String {
        match s {
            "left" => self.left.into(),
            "right" => self.right.into(),
            "both" => self.both_hands.into(),
            s => s.into(),
        }
    }

    fn sided(&self, side: Option<&str>, what: &str) -> String {
        match side {
            Some(s) if self.side_first => format!("{} {what}", self.side(s)),
            Some(s) => format!("{what} ({})", self.side(s)),
            None => what.to_string(),
        }
    }
}

impl Route {
    /// "left hand openness": side, then the source with its dots as spaces.
    pub fn source_label(&self) -> String {
        self.source_label_in(&EN_WORDS)
    }

    pub fn source_label_in(&self, w: &Words) -> String {
        let src = self.source.replace(['.', '_'], " ");
        let src = if src.is_empty() { "?".to_string() } else { src };
        let inv = if self.inv { format!(" {}", w.inverted) } else { String::new() };
        format!("{}{inv}", w.sided(self.side.as_deref(), &src))
    }

    /// "left hand openness -> Topos / Drive"; a target without a parameter
    /// shows its kind ("drive (no parameter)", "macro 3").
    pub fn describe(&self, plugin: &str) -> String {
        self.describe_in(plugin, &EN_WORDS)
    }

    pub fn describe_in(&self, plugin: &str, w: &Words) -> String {
        let targets: Vec<String> = self
            .targets
            .iter()
            .map(|t| match (&t.bound, t.token.split_once(':').map(|x| x.1).unwrap_or(&t.token)) {
                (Some(b), _) => format!("{} / {}", b.plugin.as_deref().map_or(plugin, plugin_name), b.name),
                (None, kind) if is_param_target(&t.token) => {
                    let what = kind.split_once(':').map_or(kind, |x| x.1);
                    fill(w.no_parameter, &[what])
                }
                (None, kind) => kind.replace(':', " ").trim().to_string(),
            })
            .collect();
        let targets = if targets.is_empty() { "?".to_string() } else { targets.join(", ") };
        let mods = self.mod_labels_in(w);
        let mods = if mods.is_empty() { String::new() } else { format!(" ({})", mods.join(", ")) };
        format!("{} -> {targets}{mods}", self.source_label_in(w))
    }

    /// The words between gesture and parameter, as the frame read them:
    /// "range 5-35 %", "amount 25 %", "steps", "snappy", "centred".
    pub fn mod_labels(&self) -> Vec<String> {
        self.mod_labels_in(&EN_WORDS)
    }

    pub fn mod_labels_in(&self, w: &Words) -> Vec<String> {
        let get = |p: &str| self.mods.iter().find_map(|m| m.strip_prefix(p));
        let mut out: Vec<String> = self
            .gates
            .iter()
            .map(|g| {
                let src = g[0].trim_start_matches("G:").replace(['.', '_'], " ");
                let side = g.iter().find_map(|t| t.strip_prefix("side:"));
                let shape = g.iter().find_map(|t| t.strip_prefix("shape:")).map(|s| format!(" {s}")).unwrap_or_default();
                fill(w.while_, &[&format!("{}{shape}", w.sided(side, &src))])
            })
            .collect();
        if get("lo:").is_some() || get("hi:").is_some() {
            out.push(fill(w.range, &[&percent(get("lo:").unwrap_or("0")), &percent(get("hi:").unwrap_or("1"))]));
        }
        for m in &self.mods {
            let (k, v) = m.split_once(':').unwrap_or((m, ""));
            let label = match (k, v) {
                ("lo" | "hi", _) => continue,
                ("amt", v) => fill(w.amount, &[&percent(v)]),
                ("delta", v) => format!("{}{} %", if v.starts_with('-') { "-" } else { "+" }, percent(v.trim_start_matches(['+', '-']))),
                ("mode", "step") => w.steps.into(),
                ("mode", "toggle") => w.toggles.into(),
                ("mode", "momentary") => w.while_held.into(),
                ("mode", "ramp") => w.ramps.into(),
                ("curve", c) => fill(w.curve, &[c]),
                ("smooth", "low") => w.snappy.into(),
                ("smooth", "high") => w.smooth.into(),
                ("smooth", v) => fill(w.smoothing, &[v]),
                ("hold", "hold") => w.holds.into(),
                ("hold", "freeze") => w.freezes.into(),
                ("hold", "reset") => w.resets.into(),
                ("quantize", v) => fill(w.quantize, &[v]),
                ("BIP", _) => w.centred.into(),
                (k, v) => format!("{k} {v}"),
            };
            out.push(label);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pin::{pin, Pin, PinWhat};

    fn items(frame: &[&str], text: &str) -> Vec<Item> {
        frame
            .iter()
            .map(|t| match t.strip_prefix('=') {
                Some(s) => {
                    let b = text.find(s).unwrap_or(0);
                    Item::Span { text: s.to_string(), bytes: (b, b + s.len()) }
                }
                None => Item::Token(t.to_string()),
            })
            .collect()
    }

    fn tgt(text: &str, words: &str) -> Tagged {
        let b = text.find(words).unwrap_or(0);
        Tagged { tag: "TGT", bytes: (b, b + words.len()) }
    }

    fn topos() -> Vec<(u32, &'static str, &'static str)> {
        vec![(3, "Dry/Wet", "fx.mix"), (12, "Drive", "fx.drive"), (13, "Drive On", "ctl.switch"), (20, "Tone", "filter.cutoff")]
    }

    fn cands(p: &[(u32, &'static str, &'static str)]) -> Vec<Candidate<'static>> {
        p.iter().map(|&(index, name, class)| Candidate { index, name, class }).collect()
    }

    #[test]
    fn sim_matches_python() {
        // values from probes/open-vocab-mapping/resolve.py sim(); "hp" now
        // covers "HP" (0.833 there), "b" covers "B", "two" is 2
        for (span, name, want) in [
            ("feedback", "SLOT 1 DELAY FEEDBACK", 0.775_862),
            ("mehr noise low hp", "Noise Low HP", 1.0),
            ("b warp", "B Warp", 1.0),
            ("macro two", "MACRO 2", 1.2),
            ("filter 2", "Filter 1 Freq", 0.335_714),
            ("the drive", "Drive On", 0.884_615),
        ] {
            assert!((sim(span, name) - want).abs() < 1e-5, "{span} / {name}: {}", sim(span, name));
        }
        assert_eq!(words("GrainDensity (Hz)"), ["grain", "density", "hz"]);
    }

    #[test]
    fn name_then_class_and_left_hand() {
        let text = "by opening my fist i want the drive";
        let f = pin(&items(&["I:map", "<route>", "S:hand.openness", "side:left", "T:param:fx.drive", "</route>"], text), &[]);
        let ps = cands(&topos());
        let r = resolve(&f, text, &[tgt(text, "the drive")], "fx:0:0:Topos", &ps, &HashMap::new());
        assert_eq!(r[0].targets[0].bound, Some(Bound { index: 12, name: "Drive".into(), via: Via::Name, plugin: None }));
        assert_eq!(r[0].describe("Topos"), "left hand openness -> Topos / Drive");
        // no target words: the class decides
        let r = resolve(&f, text, &[], "fx:0:0:Topos", &ps, &HashMap::new());
        assert_eq!(r[0].targets[0].bound.as_ref().map(|b| (b.index, b.via)), Some((12, Via::Class)));
    }

    #[test]
    fn chip_wins_then_alias() {
        let text = "fist controls the tone";
        let it = items(&["I:map", "<route>", "S:hand.openness", "INV", "T:param:filter.cutoff", "</route>"], text);
        let ps = cands(&topos());
        let b = text.find("tone").unwrap_or(0);
        let p = Pin { bytes: (b, b + 4), what: PinWhat::Param { id: "param:fx:0:0:Topos#3".into(), class: "fx.mix".into(), switch: false } };
        let r = resolve(&pin(&it, &[p]), text, &[tgt(text, "the tone")], "fx:0:0:Topos", &ps, &HashMap::new());
        assert_eq!(r[0].targets[0].bound.as_ref().map(|b| (b.index, b.via)), Some((3, Via::Chip)));
        assert_eq!(r[0].describe("Topos"), "hand openness inverted -> Topos / Dry/Wet");
        let aliases = HashMap::from([("tone".to_string(), "Drive".to_string())]);
        let r = resolve(&pin(&it, &[]), text, &[tgt(text, "the tone")], "fx:0:0:Topos", &ps, &aliases);
        assert_eq!(r[0].targets[0].bound.as_ref().map(|b| (b.index, b.via)), Some((12, Via::Alias)));
    }

    /// The parser read one chipped name as two targets: the second binds
    /// the chip's parameter by its words and goes.
    #[test]
    fn a_target_repeating_the_chip_goes() {
        let text = "mouth opens pan mid env attack";
        let it = items(&["I:map", "<route>", "S:mouth.open", "T:param:amp.pan", "T:param:env.attack", "</route>"], text);
        let b = text.find("pan").unwrap_or(0);
        let p = Pin { bytes: (b, text.len()), what: PinWhat::Param { id: "param:fx:0:0:SB#7".into(), class: "other".into(), switch: false } };
        let spans = [tgt(text, "pan"), tgt(text, "mid env attack")];
        let ps = cands(&[(7, "Pan Mid Env Attack", "env.attack"), (8, "Pan Mid Mix", "amp.pan")]);
        let f = crate::pin::pin_spans(&it, text, &spans, &[p]);
        let r = resolve(&f, text, &spans, "fx:0:0:SB", &ps, &HashMap::new());
        let bound: Vec<_> = r[0].targets.iter().map(|t| t.bound.as_ref().map(|b| (b.index, b.via))).collect();
        assert_eq!(bound, [Some((7, Via::Chip))]);
    }

    #[test]
    fn toggles_take_switches_and_spans_pair_in_order() {
        let text = "mouth turns the drive on and pinch opens the tone";
        let it = items(
            &["I:map", "<route>", "S:mouth.open", "T:toggle:distortion", "</route>", "<route>", "S:hand.pinch", "T:param:filter.cutoff", "</route>"],
            text,
        );
        let ps = cands(&topos());
        let r = resolve(&pin(&it, &[]), text, &[tgt(text, "the drive"), tgt(text, "the tone")], "fx:0:0:Topos", &ps, &HashMap::new());
        assert_eq!(r[0].targets[0].bound.as_ref().map(|b| b.index), Some(13));
        assert_eq!(r[1].targets[0].bound.as_ref().map(|b| b.index), Some(20));
        assert_eq!(r[1].describe("Topos"), "hand pinch -> Topos / Tone");
    }

    #[test]
    fn unbound_and_other_targets() {
        let text = "pinch plays a note and drives the bitcrush";
        let it = items(&["I:map", "<route>", "S:hand.pinch", "T:midi.note:", "T:param:fx.bitcrush", "</route>"], text);
        let ps = cands(&topos());
        let r = resolve(&pin(&it, &[]), text, &[], "fx:0:0:Topos", &ps, &HashMap::new());
        assert_eq!(r[0].describe("Topos"), "hand pinch -> midi.note, fx.bitcrush (no parameter)");
    }

    /// The parser's NOTE and CHORD values are pointer spans with normalised
    /// text: the mods take the words as the sentence writes them.
    #[test]
    fn note_and_chord_values_go_into_the_mods() {
        let text = "right pinch plays G2 and left index plays Am.";
        let span = |s: &str, end: usize| {
            let b = text.find(s).unwrap_or(0);
            Item::Span { text: s.to_lowercase(), bytes: (b, b + end) }
        };
        let t = |s: &str| Item::Token(s.into());
        let it = vec![
            t("I:map"), t("<route>"), t("S:hand.pinch"), t("side:right"), t("T:midi.note:"), t("NOTE"), span("G2", 2), t("</route>"),
            t("<route>"), t("S:hand.finger.index"), t("side:left"), t("T:midi.chord:"), t("CHORD"), span("Am.", 3), t("</route>"),
        ];
        let r = resolve(&pin(&it, &[]), text, &[], "fx:0:0:Topos", &cands(&topos()), &HashMap::new());
        assert_eq!(r[0].mods, ["note:G2"]);
        assert_eq!(r[1].mods, ["chord:Am"]);
        // a span without bytes (rules.rs) keeps its own text
        let it = vec![t("I:map"), t("<route>"), t("S:hand.pinch"), t("T:midi.note:"), t("NOTE"), Item::Span { text: "f#3".into(), bytes: (0, 0) }, t("</route>")];
        let r = resolve(&pin(&it, &[]), text, &[], "fx:0:0:Topos", &cands(&topos()), &HashMap::new());
        assert_eq!(r[0].mods, ["note:f#3"]);
    }

    #[test]
    fn modifiers_show_in_the_line() {
        let text = "fist steps the tone only up to 80 percent, snappy";
        let it = items(
            &["I:map", "<route>", "S:hand.openness", "INV", "T:param:filter.cutoff", "mode:step", "hi:0.8", "smooth:low", "</route>"],
            text,
        );
        let ps = cands(&topos());
        let r = resolve(&pin(&it, &[]), text, &[tgt(text, "the tone")], "fx:0:0:Topos", &ps, &HashMap::new());
        assert_eq!(r[0].describe("Topos"), "hand openness inverted -> Topos / Tone (range 0-80 %, steps, snappy)");
    }

    /// A lowering verb inverts the route the parser left straight (plugin
    /// log 2026-11-04: "pinch my fingers decrease Filter 1 Freq" came
    /// without INV); a raising verb keeps it.
    #[test]
    fn a_lowering_verb_inverts_the_route() {
        let ps = cands(&topos());
        let run = |frame: &[&str], text: &str, name: &str| {
            let r = resolve(&pin(&items(frame, text), &[]), text, &[tgt(text, name)], "fx:0:0:Topos", &ps, &HashMap::new());
            r.first().map(|r| (r.inv, r.describe("Topos")))
        };
        let pinch = ["I:map", "<route>", "S:hand.pinch", "side:left", "T:param:filter.cutoff", "</route>"];
        let (inv, line) = run(&pinch, "pinch my fingers decrease the filter", "the filter").unwrap_or_default();
        assert!(inv);
        assert_eq!(line, "left hand pinch inverted -> Topos / Tone");
        assert_eq!(run(&pinch, "pinch to increase the filter", "the filter").map(|r| r.0), Some(false));
        let fist = ["I:map", "<route>", "S:hand.openness", "T:param:fx.reverb", "</route>"];
        assert_eq!(run(&fist, "when I open my fist decrease the reverb", "the reverb").map(|r| r.0), Some(true));
        let de = ["I:map", "<route>", "S:hand.pinch", "T:param:filter.cutoff", "</route>"];
        assert_eq!(run(&de, "wenn ich die Finger zusammendrücke, Filter runter", "Filter").map(|r| r.0), Some(true));
        // the parser's INV stays, a raising verb keeps the parser's reading
        let inv = ["I:map", "<route>", "S:hand.y", "T:param:filter.cutoff", "INV", "</route>"];
        assert_eq!(run(&inv, "raise my hand to lower the filter", "the filter").map(|r| r.0), Some(true));
        assert_eq!(run(&inv, "pinch to increase the filter", "the filter").map(|r| r.0), Some(true));
        // a closing fist is the openness inverted: lowering with it is straight
        let closed = ["I:map", "<route>", "S:hand.openness", "T:param:filter.cutoff", "INV", "</route>"];
        assert_eq!(run(&closed, "make a fist to lower the filter", "the filter").map(|r| r.0), Some(false));
    }

    /// A MIDI part, a toggle and an unmap have no direction to invert.
    #[test]
    fn a_lowering_verb_leaves_parts_toggles_and_unmaps() {
        let ps = cands(&topos());
        let run = |frame: &[&str], text: &str| {
            let r = resolve(&pin(&items(frame, text), &[]), text, &[], "fx:0:0:Topos", &ps, &HashMap::new());
            r.first().map(|r| r.inv)
        };
        assert_eq!(run(&["I:map", "<route>", "S:hand.pinch", "T:midi.note:", "</route>"], "pinch plays a lower note"), Some(false));
        assert_eq!(run(&["I:map", "<route>", "S:hand.pinch", "T:toggle:distortion", "</route>"], "pinch turns the drive down"), Some(false));
        assert_eq!(run(&["I:unmap", "<route>", "S:hand.pinch", "T:param:filter.cutoff", "</route>"], "pinch no longer lowers the filter"), Some(false));
    }
}
