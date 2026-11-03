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

use crate::pin::{Frame, Part};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    Chip,
    Alias,
    Name,
    Class,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bound {
    pub index: u32,
    pub name: String,
    pub via: Via,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    /// The frame's T: token.
    pub token: String,
    /// The words that named it (PNAME or TGT text), "" when none.
    pub text: String,
    /// The parameter, for T:param and T:toggle targets that found one.
    pub bound: Option<Bound>,
}

#[derive(Clone, Debug, PartialEq)]
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
    /// curve:, smooth:, hold:, quantize:, delta:, BIP), in frame order.
    pub mods: Vec<String>,
    pub targets: Vec<Target>,
}

const MODS: [&str; 10] = ["lo:", "hi:", "amt:", "mode:", "curve:", "smooth:", "hold:", "quantize:", "delta:", "BIP"];

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

fn gap(a: (usize, usize), b: (usize, usize)) -> usize {
    a.0.saturating_sub(b.1).max(b.0.saturating_sub(a.1))
}

/// Words that separate two targets rather than two pieces of one name.
const SEPARATE: [&str; 12] = ["and", "und", "or", "oder", "plus", "sowie", "to", "zu", "with", "mit", "then", "dann"];

/// The TGT and PNAME pieces of the BIO head, joined where at most one word
/// and no comma or conjunction lies between them: long names come back in
/// fragments ("crush resample" + "mid env threshold", "xlfo 1" + "1" +
/// "glide" for "xlfo 1 step 1 glide").
fn groups(sentence: &str, spans: &[Tagged]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for t in spans.iter().filter(|t| matches!(t.tag, "TGT" | "PNAME")) {
        if let Some(last) = out.last_mut() {
            let between = sentence.get(last.1..t.bytes.0).unwrap_or(",");
            let ws: Vec<&str> = between.split_whitespace().collect();
            let joins = ws.len() <= 1
                && !between.contains([',', ';', '+', '&', '>'])
                && !ws.iter().any(|w| SEPARATE.contains(&w.to_lowercase().as_str()));
            if joins && last.1 <= t.bytes.0 {
                last.1 = t.bytes.1;
                continue;
            }
        }
        out.push(t.bytes);
    }
    out
}

/// Which groups belong to which route: with as many groups as T: tokens
/// they pair in order; else each goes to the route whose spans (and SRC
/// span, when there is one per route) lie nearest.
fn by_route(frame: &Frame, spans: &[Tagged], groups: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let routes: Vec<_> = frame.routes().collect();
    let mut out = vec![Vec::new(); routes.len()];
    if routes.is_empty() {
        return out;
    }
    let slots: Vec<usize> = routes.iter().map(|r| r.items.iter().filter(|it| tok(it).starts_with("T:")).count()).collect();
    if slots.iter().sum::<usize>() == groups.len() {
        let mut g = 0;
        for (r, n) in slots.iter().enumerate() {
            out[r].extend(g..g + n);
            g += n;
        }
        return out;
    }
    let src: Vec<(usize, usize)> = spans.iter().filter(|t| t.tag == "SRC").map(|t| t.bytes).collect();
    let anchors: Vec<Option<(usize, usize)>> = routes
        .iter()
        .enumerate()
        .map(|(r, route)| {
            let own = route.items.iter().filter_map(|it| match it {
                Item::Span { bytes, .. } => Some(*bytes),
                Item::Token(_) => None,
            });
            let src = (src.len() == routes.len()).then(|| src[r]);
            own.chain(src).reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
        })
        .collect();
    for (g, b) in groups.iter().enumerate() {
        let near = (0..routes.len()).filter(|&r| anchors[r].is_some()).min_by_key(|&r| anchors[r].map_or(usize::MAX, |a| gap(a, *b)));
        // no anchors anywhere: fill the routes in order
        let r = near.unwrap_or_else(|| {
            let mut left = g;
            (0..routes.len()).find(|&r| {
                let fits = left < slots[r];
                left = left.saturating_sub(slots[r]);
                fits
            })
            .unwrap_or(routes.len() - 1)
        });
        out[r].push(g);
    }
    out
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
    let found = |index: u32, via| params.iter().find(|p| p.index == index).map(|p| Bound { index, name: p.name.into(), via });
    let all: Vec<&Candidate> = params.iter().collect();
    // a macro or CC guess that names a parameter of the picked plugin
    // ("slot 18 target" read as macro 18) is that parameter
    if !is_param_target(token) {
        if !(token.starts_with("T:macro:") || token.starts_with("T:midi.cc:")) || pinned.is_some() {
            return (None, 0.0);
        }
        return match by_name(texts, &all, None) {
            Some((s, p)) if s >= MERGE_MIN => (Some(Bound { index: p.index, name: p.name.into(), via: Via::Name }), s),
            _ => (None, 0.0),
        };
    }
    // 1. chip: "param:<plugin id>#<index>"
    if let Some(id) = pinned {
        let Some((pl, index)) = id.strip_prefix("param:").and_then(|x| x.rsplit_once('#')) else { return (None, 0.0) };
        let Ok(index) = index.parse::<u32>() else { return (None, 0.0) };
        let b = if pl == plugin {
            found(index, Via::Chip).unwrap_or(Bound { index, name: format!("#{index}"), via: Via::Chip })
        } else {
            Bound { index, name: format!("{pl} #{index}"), via: Via::Chip }
        };
        return (Some(b), 0.0);
    }
    let toggle = token.strip_prefix("T:toggle:");
    let class = token.strip_prefix("T:param:");
    // 2. alias
    for t in texts {
        if let Some(name) = aliases.get(&alias_key(t)) {
            if let Some(p) = params.iter().find(|p| p.name == name) {
                return (Some(Bound { index: p.index, name: p.name.into(), via: Via::Alias }), 0.0);
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
            return (Some(Bound { index: p.index, name: p.name.into(), via: Via::Name }), s);
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
        .map(|(_, p)| Bound { index: p.index, name: p.name.into(), via: Via::Class });
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
    let groups = groups(sentence, spans);
    let texts: Vec<String> = groups.iter().map(|b| cut(sentence, *b, spans)).collect();
    let owned = by_route(frame, spans, &groups);
    frame
        .routes()
        .zip(owned)
        .map(|(route, gs)| {
            let find = |p: &str| route.items.iter().find_map(|it| tok(it).strip_prefix(p).map(str::to_string));
            let slots: Vec<usize> = (0..route.items.len()).filter(|&i| tok(&route.items[i]).starts_with("T:")).collect();
            let mut targets = Vec::new();
            let mut k = 0;
            while k < slots.len() {
                let item = slots[k];
                // the last group also names every token after it
                let g = gs.get(k.min(gs.len().saturating_sub(1))).copied().filter(|_| !gs.is_empty());
                let rest = if k + 1 == gs.len() { slots.len() - k - 1 } else { 0 };
                let token = tok(&route.items[item]).to_string();
                let words: Vec<String> =
                    g.map(|g| texts[g].clone()).into_iter().chain(pname_for(&route.items, item)).filter(|t| !t.is_empty()).collect();
                let (bound, score) = bind(&token, route.pinned[item].as_deref(), &words, plugin, params, aliases);
                let merged = rest > 0 && score >= MERGE_MIN && slots[k + 1..].iter().all(|&i| route.pinned[i].is_none());
                targets.push(Target { token, text: words.first().cloned().unwrap_or_default(), bound });
                k += if merged { rest + 1 } else { 1 };
            }
            let all: Vec<&Candidate> = params.iter().collect();
            for &g in gs.iter().skip(slots.len()) {
                if let Some((s, p)) = by_name(std::slice::from_ref(&texts[g]), &all, None) {
                    if s >= MERGE_MIN {
                        let bound = Some(Bound { index: p.index, name: p.name.into(), via: Via::Name });
                        targets.push(Target { token: format!("T:param:{}", p.class), text: texts[g].clone(), bound });
                    }
                }
            }
            Route {
                source: find("S:").unwrap_or_default(),
                side: find("side:").or_else(|| shared.clone()),
                inv: route.items.iter().any(|it| tok(it) == "INV"),
                gates: gates(&route.items),
                mods: route
                    .items
                    .iter()
                    .map(tok)
                    .filter(|t| MODS.iter().any(|m| t.starts_with(m)))
                    .map(str::to_string)
                    .collect(),
                targets,
            }
        })
        .collect()
}

impl Route {
    /// "left hand openness": side, then the source with its dots as spaces.
    pub fn source_label(&self) -> String {
        let src = self.source.replace(['.', '_'], " ");
        let src = if src.is_empty() { "?".to_string() } else { src };
        let side = self.side.as_deref().map(|s| if s == "both" { "both hands" } else { s });
        let inv = if self.inv { " inverted" } else { "" };
        match side {
            Some(s) => format!("{s} {src}{inv}"),
            None => format!("{src}{inv}"),
        }
    }

    /// "left hand openness -> Topos / Drive"; a target without a parameter
    /// shows its kind ("drive (no parameter)", "macro 3").
    pub fn describe(&self, plugin: &str) -> String {
        let targets: Vec<String> = self
            .targets
            .iter()
            .map(|t| match (&t.bound, t.token.split_once(':').map(|x| x.1).unwrap_or(&t.token)) {
                (Some(b), _) => format!("{plugin} / {}", b.name),
                (None, kind) if is_param_target(&t.token) => {
                    let what = kind.split_once(':').map_or(kind, |x| x.1);
                    format!("{what} (no parameter)")
                }
                (None, kind) => kind.replace(':', " ").trim().to_string(),
            })
            .collect();
        let targets = if targets.is_empty() { "?".to_string() } else { targets.join(", ") };
        let mods = self.mod_labels();
        let mods = if mods.is_empty() { String::new() } else { format!(" ({})", mods.join(", ")) };
        format!("{} -> {targets}{mods}", self.source_label())
    }

    /// The words between gesture and parameter, as the frame read them:
    /// "range 5-35 %", "amount 25 %", "steps", "snappy", "centred".
    pub fn mod_labels(&self) -> Vec<String> {
        let get = |p: &str| self.mods.iter().find_map(|m| m.strip_prefix(p));
        let mut out: Vec<String> = self
            .gates
            .iter()
            .map(|g| {
                let src = g[0].trim_start_matches("G:").replace(['.', '_'], " ");
                let side = g.iter().find_map(|t| t.strip_prefix("side:")).map(|s| format!("{s} ")).unwrap_or_default();
                let shape = g.iter().find_map(|t| t.strip_prefix("shape:")).map(|s| format!(" {s}")).unwrap_or_default();
                format!("while {side}{src}{shape}")
            })
            .collect();
        if get("lo:").is_some() || get("hi:").is_some() {
            out.push(format!("range {}-{} %", percent(get("lo:").unwrap_or("0")), percent(get("hi:").unwrap_or("1"))));
        }
        for m in &self.mods {
            let (k, v) = m.split_once(':').unwrap_or((m, ""));
            let label = match (k, v) {
                ("lo" | "hi", _) => continue,
                ("amt", v) => format!("amount {} %", percent(v)),
                ("delta", v) => format!("{}{} %", if v.starts_with('-') { "-" } else { "+" }, percent(v.trim_start_matches(['+', '-']))),
                ("mode", "step") => "steps".into(),
                ("mode", "toggle") => "toggles".into(),
                ("mode", "momentary") => "while held".into(),
                ("mode", "ramp") => "ramps".into(),
                ("curve", c) => format!("{c} curve"),
                ("smooth", "low") => "snappy".into(),
                ("smooth", "high") => "smooth".into(),
                ("smooth", v) => format!("{v} smoothing"),
                ("hold", "hold") => "holds".into(),
                ("hold", v) => format!("{v}s"),
                ("quantize", v) => format!("{v} steps"),
                ("BIP", _) => "centred".into(),
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
        assert_eq!(r[0].targets[0].bound, Some(Bound { index: 12, name: "Drive".into(), via: Via::Name }));
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
}
