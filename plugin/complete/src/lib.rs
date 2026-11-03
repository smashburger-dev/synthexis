//! IDE-style completion for the mapping assistant's sentence field.
//!
//! Three kinds of items: parameters of the picked plugin (matched by their
//! name and, through the glossary, by phrases of their param-sense class:
//! "cut" or "Filter auf" offer "Filter 1 Freq"), gestures (glossary
//! phrases of the ontology sources and hand shapes, only where a gesture is
//! named: sentence start, "when I", "wenn ich", "with my") and targets that
//! are not plugin parameters (glossary phrases of MIDI note, chord, pattern,
//! CC and the macros: "Akk" offers "Akkord spielen"). An accepted
//! item becomes a chip that carries an id; on submit the chips become
//! pinned spans for the parser (synthexis_parser::pin).
//!
//! Rank = fuzzy match (fuzzy.rs) + class prior + span prior (which class
//! the training sentences meant with words starting like the query) + the
//! user's recent picks + the words before the cursor (switching words rank
//! switch parameters up).
//! A keystroke costs a prefix lookup and a fuzzy pass over the keys that
//! share the query's first letter at a word start, well under a
//! millisecond for a plugin of 2,600 parameters. Runs on the GUI thread;
//! never on the audio thread, never the neural parser.

pub mod context;
pub mod data;
pub mod doc;
pub mod fuzzy;
mod index;

use std::collections::HashMap;
use std::ops::Range;

use fuzzy::Query;
use index::Index;

/// Ghost text needs this many typed characters ...
pub const GHOST_MIN_CHARS: usize = 2;
/// ... and the best item at least this score.
pub const GHOST_MIN_SCORE: f32 = 0.8;
/// Matches below this never show.
const MIN_MATCH: f32 = 0.3;
/// A class phrase match counts this much of a name match.
const VIA_PHRASE: f32 = 0.85;
const GESTURE_BONUS: f32 = 0.05;
const WORD_BONUS: f32 = 0.03;
const PRIOR_MAX: f32 = 0.05;
const RECENT_MAX: f32 = 0.1;
const RECENT_CAP: usize = 32;
const SWITCH_UP: f32 = 0.15;
const SWITCH_DOWN: f32 = -0.05;
const NOT_MAPPABLE: f32 = -0.2;
/// Weight of P(class | typed query) from the training spans.
pub const SPAN_PRIOR: f32 = 0.2;
/// Target items rank below parameters of the same match.
const TARGET_DOWN: f32 = -0.1;
pub const SWITCH_CLASS: &str = "ctl.switch";
/// Classes that gather unrelated parameters: their glossary phrases
/// ("the gap before the reverb starts" in `other`) would offer every one of
/// them, so these parameters complete by name only.
const CATCH_ALL: [&str; 3] = ["other", "system", "unsafe"];

#[derive(Clone, Debug, PartialEq)]
pub struct Gesture {
    pub source: String,
    /// Hand shape id when `source` is hand.shape.
    pub shape: Option<String>,
    pub phrase: String,
    /// -1 when the phrase names the source's low end ("Finger zusammen").
    pub pol: i8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub index: u32,
    pub name: String,
    pub class: String,
}

/// What an accepted completion pins.
#[derive(Clone, Debug, PartialEq)]
pub enum Chip {
    Param { plugin: String, index: u32, name: String, class: String },
    /// `side`: the hand side picked in the variant panel (left, right,
    /// both); None leaves the side to the sentence.
    Gesture { source: String, shape: Option<String>, phrase: String, pol: i8, side: Option<String> },
    /// A target kind of the ontology that is no plugin parameter:
    /// midi.note, midi.chord, midi.pattern, midi.cc or macro.
    Target { kind: String, phrase: String },
}

impl Chip {
    /// Stable id: "param:<plugin id>#<index>" or "gesture:<source>[/<shape>]".
    pub fn id(&self) -> String {
        match self {
            Chip::Param { plugin, index, .. } => format!("param:{plugin}#{index}"),
            Chip::Gesture { source, shape: Some(s), .. } => format!("gesture:{source}/{s}"),
            Chip::Gesture { source, .. } => format!("gesture:{source}"),
            Chip::Target { kind, .. } => format!("target:{kind}"),
        }
    }

    /// The text the chip shows and puts into the sentence.
    pub fn label(&self) -> &str {
        match self {
            Chip::Param { name, .. } => name,
            Chip::Gesture { phrase, .. } | Chip::Target { phrase, .. } => phrase,
        }
    }

    /// A switch parameter becomes a toggle target, any other a param target.
    pub fn is_switch(&self) -> bool {
        matches!(self, Chip::Param { class, .. } if class == SWITCH_CLASS)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub chip: Chip,
    pub score: f32,
    /// Bytes of the typed text the chip replaces.
    pub replace: Range<usize>,
    /// The class phrase that matched, when the name did not ("Cutoff").
    pub via: Option<String>,
    /// The typed text is a prefix of the label or of `via`.
    pub prefix: bool,
    /// Offered from a weak gesture position: list only, no ghost text.
    pub weak: bool,
    pub variants: Variants,
}

/// Hand sides a gesture can be pinned to, in panel order (Left key, Right
/// key). Never a default and never both hands (Noa, 11-03).
pub const SIDES: [&str; 2] = ["left", "right"];
/// Fingers, in panel order: the source is hand.finger.<finger>.
pub const FINGERS: [&str; 5] = ["thumb", "index", "middle", "ring", "pinky"];

/// What the variant panel asks for an item.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Variants {
    /// The move is per hand (ontology `side: true`): a side must be picked.
    pub sided: bool,
    /// The side the sentence names right before the item ("die linke
    /// Fau"), an index into SIDES; the panel starts there.
    pub side: Option<usize>,
    /// A bare "finger": which finger is picked in a second step.
    pub finger: bool,
}

impl Variants {
    pub fn any(&self) -> bool {
        self.sided || self.finger
    }
}

/// A finger phrase that names no finger at all ("Finger"): which finger is
/// picked in the panel. "pointing finger" or "Daumen" keep their own.
pub fn finger_open(source: &str, phrase: &str) -> bool {
    source.starts_with("hand.finger.") && phrase.trim().eq_ignore_ascii_case("finger")
}

impl Chip {
    /// The chip with the panel's choice: a finger replaces the source, the
    /// side (an index into SIDES) is kept for the pin.
    pub fn with_variant(&self, side: Option<usize>, finger: Option<usize>) -> Chip {
        let mut c = self.clone();
        if let Chip::Gesture { source, side: s, .. } = &mut c {
            if let Some(f) = finger.and_then(|f| FINGERS.get(f)) {
                *source = format!("hand.finger.{f}");
            }
            *s = side.and_then(|i| SIDES.get(i)).map(|x| x.to_string());
        }
        c
    }
}

impl Suggestion {
    /// The rest of the label when the typed text is its prefix (shown as
    /// ghost text after the cursor); None for a fuzzy or indirect match.
    pub fn ghost_suffix(&self, text: &str) -> Option<String> {
        let typed = text.get(self.replace.clone())?;
        let label = self.chip.label();
        let n = typed.chars().count();
        let head: String = label.chars().take(n).collect();
        (head.to_lowercase() == typed.to_lowercase()).then(|| label.chars().skip(n).collect())
    }

    /// Ghost text only continues what was typed: a prefix match of two or
    /// more characters with a good score. Fuzzy matches stay in the list.
    pub fn ghost_worthy(&self, text: &str) -> bool {
        self.prefix
            && !self.weak
            && self.score >= GHOST_MIN_SCORE
            && text
                .get(self.replace.clone())
                .is_some_and(|t| t.chars().count() >= GHOST_MIN_CHARS)
    }
}

struct Plugin {
    id: String,
    params: Vec<Param>,
    class: Vec<Option<usize>>,
    by_class: Vec<Vec<u32>>,
    index: Index,
}

pub struct Engine {
    gestures: Vec<Gesture>,
    gindex: Index,
    phrase_class: Vec<usize>,
    phrases: Vec<String>,
    pindex: Index,
    classes: Vec<String>,
    class_id: HashMap<String, usize>,
    prior: Vec<f32>,
    plugin: Option<Plugin>,
    recent: Vec<String>,
    /// (kind, phrase) of the non-parameter targets.
    targets: Vec<(String, String)>,
    tindex: Index,
    /// Folded training span texts, sorted, with their class and count.
    spans: Vec<(Vec<char>, String, u32)>,
    pub span_weight: f32,
    /// Sources that take a hand side (ontology `side: true`).
    sided: std::collections::HashSet<String>,
}

#[derive(Default)]
struct Scratch {
    seen: Vec<bool>,
    ids: Vec<u32>,
}

impl Engine {
    /// `phrases`: (class, phrase) from the glossary; `prior`: label count
    /// per class over the harvested plugins.
    pub fn new(
        gestures: Vec<Gesture>,
        mut phrases: Vec<(String, String)>,
        prior: &HashMap<String, u64>,
    ) -> Self {
        phrases.retain(|(c, _)| !CATCH_ALL.contains(&c.as_str()));
        let mut classes: Vec<String> = prior.keys().cloned().collect();
        classes.extend(phrases.iter().map(|(c, _)| c.clone()));
        classes.sort();
        classes.dedup();
        let class_id: HashMap<String, usize> =
            classes.iter().enumerate().map(|(i, c)| (c.clone(), i)).collect();
        let top = prior.values().copied().max().unwrap_or(1).max(1) as f32;
        let prior = classes
            .iter()
            .map(|c| {
                let n = prior.get(c).copied().unwrap_or(0) as f32;
                PRIOR_MAX * (1.0 + n).ln() / (1.0 + top).ln()
            })
            .collect();
        let phrase_class = phrases.iter().map(|(c, _)| class_id[c]).collect();
        let phrases: Vec<String> = phrases.into_iter().map(|(_, p)| p).collect();
        Self {
            gindex: Index::new(gestures.iter().map(|g| g.phrase.as_str())),
            gestures,
            pindex: Index::new(phrases.iter().map(String::as_str)),
            phrases,
            phrase_class,
            classes,
            class_id,
            prior,
            plugin: None,
            recent: Vec::new(),
            targets: Vec::new(),
            tindex: Index::new(std::iter::empty()),
            spans: Vec::new(),
            span_weight: SPAN_PRIOR,
            sided: Default::default(),
        }
    }

    /// Sources that take a hand side: their items get the side axis.
    pub fn with_sided(mut self, sources: Vec<String>) -> Self {
        self.sided = sources.into_iter().collect();
        self
    }

    /// What the panel asks for a gesture item; it starts at the side named
    /// right before the item ("die linke Fau"), else at none.
    fn gesture_variants(&self, g: &Gesture, named: Option<&str>) -> Variants {
        let sided = self.sided.contains(&g.source);
        let side = named.filter(|_| sided).and_then(|n| SIDES.iter().position(|s| *s == n));
        Variants { sided, side, finger: finger_open(&g.source, &g.phrase) }
    }

    /// Training span texts with the class (or target) they named and how
    /// often: the span prior.
    pub fn with_span_prior(mut self, rows: Vec<(String, String, u32)>) -> Self {
        self.spans = rows.into_iter().map(|(t, c, n)| (fuzzy::fold(&t), c, n)).collect();
        self.spans.sort_by(|a, b| a.0.cmp(&b.0));
        self
    }

    /// P(class | the training span starts with the query), per class.
    fn span_prior(&self, q: &Query) -> HashMap<&str, f32> {
        let mut out: HashMap<&str, f32> = HashMap::new();
        if self.span_weight == 0.0 || q.chars.is_empty() {
            return out;
        }
        let lo = self.spans.partition_point(|s| s.0.as_slice() < q.chars.as_slice());
        let mut total = 0.0;
        for (_, c, n) in self.spans[lo..].iter().take_while(|s| s.0.starts_with(&q.chars)) {
            *out.entry(c.as_str()).or_default() += *n as f32;
            total += *n as f32;
        }
        out.values_mut().for_each(|v| *v /= total);
        out
    }

    /// Targets that are no plugin parameter, as (kind, phrase).
    pub fn with_targets(mut self, targets: Vec<(String, String)>) -> Self {
        self.tindex = Index::new(targets.iter().map(|(_, p)| p.as_str()));
        self.targets = targets;
        self
    }

    /// The plugin whose parameters complete (the target picker's choice).
    pub fn set_plugin(&mut self, id: &str, params: Vec<Param>) {
        let class: Vec<Option<usize>> =
            params.iter().map(|p| self.class_id.get(&p.class).copied()).collect();
        let mut by_class = vec![Vec::new(); self.classes.len()];
        for (i, c) in class.iter().enumerate() {
            if let Some(c) = c {
                by_class[*c].push(i as u32);
            }
        }
        self.plugin = Some(Plugin {
            id: id.to_string(),
            index: Index::new(params.iter().map(|p| p.name.as_str())),
            params,
            class,
            by_class,
        });
    }

    pub fn plugin(&self) -> Option<&str> {
        self.plugin.as_ref().map(|p| p.id.as_str())
    }

    /// Remembers an accepted chip; recent picks rank up.
    pub fn picked(&mut self, chip: &Chip) {
        let id = chip.id();
        self.recent.retain(|r| *r != id);
        self.recent.push(id);
        if self.recent.len() > RECENT_CAP {
            self.recent.remove(0);
        }
    }

    fn recency(&self, id: &str) -> f32 {
        self.recent
            .iter()
            .rev()
            .position(|r| r == id)
            .map_or(0.0, |r| RECENT_MAX * (1.0 - r as f32 / RECENT_CAP as f32))
    }

    fn class_adjust(&self, class: Option<usize>, switchy: bool) -> f32 {
        let Some(c) = class else { return 0.0 };
        let name = self.classes[c].as_str();
        self.prior[c]
            + match name {
                SWITCH_CLASS if switchy => SWITCH_UP,
                SWITCH_CLASS => SWITCH_DOWN,
                "system" | "unsafe" => NOT_MAPPABLE,
                _ => 0.0,
            }
    }

    /// Top `k` completions for the word(s) before `cursor`; `floor` is the
    /// end of the last chip before the cursor (queries never reach into it).
    pub fn complete(&self, text: &str, cursor: usize, floor: usize, k: usize) -> Vec<Suggestion> {
        let spans = context::queries(text, cursor, floor);
        let Some(last) = spans.first() else { return Vec::new() };
        let clause = context::clause_start(text, cursor, floor);
        let switchy = context::switching(&text[clause..last.start]);
        let mut best: HashMap<String, Suggestion> = HashMap::new();
        let mut keep = |s: Suggestion, key: String| {
            if best.get(&key).is_none_or(|b| s.score > b.score) {
                best.insert(key, s);
            }
        };
        let mut sc = Scratch::default();
        for span in &spans {
            let q = Query::new(&text[span.start..cursor]);
            let Some(&first) = q.chars.first() else { continue };
            let first = first.to_string();
            let replace = span.start..cursor;
            let wb = WORD_BONUS * (span.words - 1) as f32;
            let sp = self.span_prior(&q);
            let learned = |c: &str| self.span_weight * sp.get(c).copied().unwrap_or(0.0);
            let pos = context::gesture_position(&text[..span.start]);
            let named = context::side_before(&text[clause..span.start]);
            if pos != context::GesturePos::No {
                let weak = pos == context::GesturePos::Weak;
                self.gindex.with_word_prefix(&first, &mut sc.seen, &mut sc.ids);
                for &i in &sc.ids {
                    let k = &self.gindex.keys[i as usize];
                    let Some(m) = q.score(k).filter(|&m| m >= MIN_MATCH) else {
                        continue;
                    };
                    let prefix = q.is_prefix(k);
                    let g = &self.gestures[i as usize];
                    let chip = Chip::Gesture {
                        source: g.source.clone(),
                        shape: g.shape.clone(),
                        phrase: g.phrase.clone(),
                        pol: g.pol,
                        side: None,
                    };
                    let key = format!("gesture:{}/{:?}/{}", g.source, g.shape, g.pol);
                    let score = m + GESTURE_BONUS + wb + self.recency(&chip.id());
                    let variants = self.gesture_variants(g, named);
                    keep(Suggestion { chip, score, replace: replace.clone(), via: None, prefix, weak, variants }, key);
                }
            }
            self.tindex.with_word_prefix(&first, &mut sc.seen, &mut sc.ids);
            for &i in &sc.ids {
                let k = &self.tindex.keys[i as usize];
                let Some(m) = q.score(k).filter(|&m| m >= MIN_MATCH) else {
                    continue;
                };
                let (kind, phrase) = &self.targets[i as usize];
                let chip = Chip::Target { kind: kind.clone(), phrase: phrase.clone() };
                let key = chip.id();
                let score = m + TARGET_DOWN + wb + learned(kind) + self.recency(&key);
                // list only: short target words ("fire chords", "pad chords")
                // start like parameter words and would take the ghost text
                keep(Suggestion { chip, score, replace: replace.clone(), via: None, prefix: q.is_prefix(k), weak: true, variants: Variants::default() }, key);
            }
            let Some(p) = &self.plugin else { continue };
            // (param, match, via phrase, prefix)
            let mut cands: Vec<(u32, f32, Option<String>, bool)> = Vec::new();
            p.index.with_word_prefix(&first, &mut sc.seen, &mut sc.ids);
            for &i in &sc.ids {
                let k = &p.index.keys[i as usize];
                if let Some(m) = q.score(k).filter(|&m| m >= MIN_MATCH) {
                    cands.push((i, m, None, q.is_prefix(k)));
                }
            }
            // best phrase per class: (match, phrase, prefix)
            let mut via: HashMap<usize, (f32, usize, bool)> = HashMap::new();
            self.pindex.with_word_prefix(&first, &mut sc.seen, &mut sc.ids);
            for &i in &sc.ids {
                let k = &self.pindex.keys[i as usize];
                // a phrase brings in a whole class, so it has to match word
                // by word; parameter names keep the full fuzzy match
                if !q.word_aligned(k) {
                    continue;
                }
                let Some(m) = q.score(k).filter(|&m| m >= MIN_MATCH) else {
                    continue;
                };
                let c = self.phrase_class[i as usize];
                if via.get(&c).is_none_or(|&(b, _, _)| m > b) {
                    via.insert(c, (m, i as usize, q.is_prefix(k)));
                }
            }
            for (&c, &(m, phrase, prefix)) in &via {
                for &i in &p.by_class[c] {
                    cands.push((i, m * VIA_PHRASE, Some(self.phrases[phrase].clone()), prefix));
                }
            }
            for (i, m, via, prefix) in cands {
                let par = &p.params[i as usize];
                let chip = Chip::Param {
                    plugin: p.id.clone(),
                    index: par.index,
                    name: par.name.clone(),
                    class: par.class.clone(),
                };
                let score = m + wb + self.class_adjust(p.class[i as usize], switchy) + learned(&par.class) + self.recency(&chip.id());
                let key = chip.id();
                keep(Suggestion { chip, score, replace: replace.clone(), via, prefix, weak: false, variants: Variants::default() }, key);
            }
        }
        let mut out: Vec<Suggestion> = best.into_values().collect();
        out.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.chip.label().len().cmp(&b.chip.label().len()))
                .then_with(|| param_index(&a.chip).cmp(&param_index(&b.chip)))
                .then_with(|| a.chip.label().cmp(b.chip.label()))
                .then_with(|| a.chip.id().cmp(&b.chip.id()))
        });
        contrast(&mut out);
        // a target item never takes the first row (the ghost text) from an
        // item that could be ghost text
        if matches!(out.first(), Some(s) if matches!(s.chip, Chip::Target { .. })) {
            if let Some(j) = out.iter().position(|s| s.ghost_worthy(text)) {
                let s = out.remove(j);
                out.insert(0, s);
            }
        }
        out.truncate(k);
        out
    }
}

/// What a list row offers: a gesture source, or a parameter class.
fn kind(c: &Chip) -> &str {
    match c {
        Chip::Param { class, .. } => class,
        Chip::Gesture { source, .. } => source,
        Chip::Target { kind, .. } => kind,
    }
}

/// At most this many rows of one kind before the other kinds get a row.
const PER_KIND: usize = 3;

/// Moves rows past the third of one kind (seven send levels for "vol")
/// behind the other kinds, so a long list also holds the readings the user
/// may have meant. The order within a kind stays; a top 3 never changes.
fn contrast(out: &mut Vec<Suggestion>) {
    if out.len() <= PER_KIND {
        return;
    }
    let mut count: HashMap<String, usize> = HashMap::new();
    let (front, back): (Vec<Suggestion>, Vec<Suggestion>) = std::mem::take(out).into_iter().partition(|s| {
        let n = count.entry(kind(&s.chip).to_string()).or_default();
        *n += 1;
        *n <= PER_KIND
    });
    *out = front;
    out.extend(back);
}

fn param_index(c: &Chip) -> u32 {
    match c {
        Chip::Param { index, .. } => *index,
        Chip::Gesture { .. } | Chip::Target { .. } => 0,
    }
}

#[cfg(test)]
mod tests;
