//! Which words of the sentence belong to which target of a frame, for
//! the resolver (the words that name a parameter) and pin.rs (the target a
//! parameter chip lands on). The BIO head's TGT and PNAME pieces are joined
//! into name groups; groups go to routes, and inside a route they pair with
//! its T: tokens in order. With fewer groups than tokens the last group
//! names the rest too (v11 reads one long name as several classes: "pan mid
//! env attack" -> amp.pan + env.attack); groups beyond the tokens are
//! extra (a target the parser left out).

use crate::pin::Frame;
use crate::{Item, Tagged};

fn tok(it: &Item) -> &str {
    match it {
        Item::Token(t) => t,
        Item::Span { .. } => "",
    }
}

/// One T: token of a route and the group that names it.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    /// Index of the T: token in the route's items.
    pub item: usize,
    pub group: Option<usize>,
    /// The group already names the slot before (several tokens, one name).
    pub shares: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RouteAlign {
    pub slots: Vec<Slot>,
    /// Groups of the route that no T: token takes.
    pub extra: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Alignment {
    /// Byte ranges of the name groups, in sentence order.
    pub groups: Vec<(usize, usize)>,
    /// One per route of the frame, in frame order.
    pub routes: Vec<RouteAlign>,
}

impl Alignment {
    /// (route, item) of the first free slot whose group overlaps `bytes`;
    /// `free` says whether a slot may take a chip.
    pub fn slot_at(&self, bytes: (usize, usize), free: impl Fn(usize, usize) -> bool) -> Option<(usize, usize)> {
        let g = self.groups.iter().position(|g| g.0 < bytes.1 && bytes.0 < g.1)?;
        self.routes
            .iter()
            .enumerate()
            .find_map(|(r, ra)| ra.slots.iter().find(|s| s.group == Some(g) && free(r, s.item)).map(|s| (r, s.item)))
    }
}

/// The alignment of `frame` on `sentence` with the parse's BIO `spans`.
pub fn align(frame: &Frame, sentence: &str, spans: &[Tagged]) -> Alignment {
    let groups = groups(sentence, spans);
    let owned = by_route(frame, spans, &groups);
    let routes = frame
        .routes()
        .zip(owned)
        .map(|(route, gs)| {
            let items: Vec<usize> = (0..route.items.len()).filter(|&i| tok(&route.items[i]).starts_with("T:")).collect();
            let slots = items
                .iter()
                .enumerate()
                .map(|(k, &item)| {
                    let at = k.min(gs.len().saturating_sub(1));
                    Slot { item, group: gs.get(at).copied(), shares: !gs.is_empty() && k >= gs.len() }
                })
                .collect();
            RouteAlign { slots, extra: gs.iter().skip(items.len()).copied().collect() }
        })
        .collect();
    Alignment { groups, routes }
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
pub fn groups(sentence: &str, spans: &[Tagged]) -> Vec<(usize, usize)> {
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

