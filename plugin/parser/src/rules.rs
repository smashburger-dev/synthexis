//! Decode-time rules on a predicted frame, ported from tools/mapping/rules.py
//! with conventions.py and normalize.py (branch claude/parser-v11-de5d19,
//! aeb198a):
//!
//!   conv     Noa's label conventions: pinch next to a hand movement is a
//!            hold (G:hand.pinch); single fingers shown together ("together",
//!            "both", "gleichzeitig", ...) become one S:hand.shape mask;
//!            since v11 S:hand.fist folds into S:hand.openness and
//!            S:eye.closed into S:eye.open with the route's INV flipped.
//!   conv10   conv as v10 was scored, without the fold.
//!   pinch    a route with S:hand.pinch and another source holds the pinch.
//!   pronoun  a later route without a target restates an earlier one
//!            (dropped) or takes the previous route's target; a later route
//!            without a source whose targets are used is dropped.
//!
//! Tokens compare by their printed form (spans as "=text"), as in Python.

use std::sync::OnceLock;

use regex::Regex;

use crate::Item;

pub const NAMES: [&str; 4] = ["conv", "conv10", "pinch", "pronoun"];

const MOVES: [&str; 7] = [
    "S:hand.x",
    "S:hand.y",
    "S:hand.roll",
    "S:hand.depth",
    "S:hand.pitch",
    "S:hand.speed",
    "S:hands.distance",
];
const FINGERS: [&str; 5] = ["thumb", "index", "middle", "ring", "pinky"];

/// The sentence says the fingers are shown together (conventions.TOGETHER).
fn together(text: &str) -> bool {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)at the same time|together|\bboth\b|gleichzeitig|zusammen|\bbeide").ok()
    })
    .as_ref()
    .is_some_and(|re| re.is_match(text))
}

fn key(it: &Item) -> String {
    match it {
        Item::Token(t) => t.clone(),
        Item::Span { text, .. } => format!("={text}"),
    }
}

fn tok(s: &str) -> Item {
    Item::Token(s.to_string())
}

/// Applies `names` in order (each one of NAMES).
pub fn apply(mut frame: Vec<Item>, text: &str, names: &[String]) -> Vec<Item> {
    for name in names {
        frame = match name.as_str() {
            "conv" | "conv10" => {
                pinch_gate(&mut frame);
                let frame = finger_state(&frame, text).unwrap_or(frame);
                if name == "conv" {
                    fold(&frame).unwrap_or(frame)
                } else {
                    frame
                }
            }
            "pinch" | "pronoun" => normalize(frame, name),
            _ => frame,
        };
    }
    frame
}

/// [(start, end)) of route bodies.
fn routes(frame: &[Item]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, it) in frame.iter().enumerate() {
        match key(it).as_str() {
            "<route>" => start = Some(i + 1),
            "</route>" => {
                if let Some(s) = start.take() {
                    out.push((s, i));
                }
            }
            _ => {}
        }
    }
    out
}

fn pinch_gate(frame: &mut [Item]) {
    for (a, b) in routes(frame) {
        let keys: Vec<String> = frame[a..b].iter().map(key).collect();
        if let Some(k) = keys.iter().position(|t| t == "S:hand.pinch") {
            if keys.iter().any(|t| MOVES.contains(&t.as_str())) {
                frame[a + k] = tok("G:hand.pinch");
            }
        }
    }
}

fn finger_state(frame: &[Item], text: &str) -> Option<Vec<Item>> {
    if !together(text) {
        return None;
    }
    let mut out = Vec::new();
    let mut last = 0;
    let mut changed = false;
    for (a, b) in routes(frame) {
        let body = &frame[a..b];
        let keys: Vec<String> = body.iter().map(key).collect();
        let idx: Vec<usize> = (0..keys.len())
            .filter(|&k| {
                keys[k]
                    .strip_prefix("S:hand.finger.")
                    .is_some_and(|f| FINGERS.contains(&f))
            })
            .collect();
        if idx.len() < 2
            || idx
                .iter()
                .any(|&k| keys.get(k + 1).is_some_and(|t| t.starts_with("side:")))
        {
            continue;
        }
        let mask: String = FINGERS
            .iter()
            .map(|f| {
                let want = format!("S:hand.finger.{f}");
                if idx.iter().any(|&k| keys[k] == want) {
                    '1'
                } else {
                    '0'
                }
            })
            .collect();
        let mut new: Vec<Item> = body
            .iter()
            .enumerate()
            .filter(|(k, _)| !idx[1..].contains(k))
            .map(|(_, it)| it.clone())
            .collect();
        new.splice(
            idx[0]..idx[0] + 1,
            [tok("S:hand.shape"), tok(&format!("shape:m{mask}"))],
        );
        out.extend_from_slice(&frame[last..a]);
        out.extend(new);
        last = b;
        changed = true;
    }
    changed.then(|| {
        out.extend_from_slice(&frame[last..]);
        out
    })
}

const FOLD: [(&str, &str); 2] = [
    ("S:hand.fist", "S:hand.openness"),
    ("S:eye.closed", "S:eye.open"),
];

fn folded(t: &str) -> Option<&'static str> {
    FOLD.iter().find(|(from, _)| *from == t).map(|(_, to)| *to)
}

/// Closing the fist is the hand's openness inverted (conventions.fold).
fn fold(frame: &[Item]) -> Option<Vec<Item>> {
    if !frame.iter().any(|it| folded(&key(it)).is_some()) {
        return None;
    }
    let mut out = Vec::new();
    let mut last = 0;
    for (a, b) in routes(frame) {
        let body = &frame[a..b];
        let flips = body.iter().filter(|it| folded(&key(it)).is_some()).count();
        if flips == 0 {
            continue;
        }
        let mut body: Vec<Item> = body
            .iter()
            .map(|it| folded(&key(it)).map_or_else(|| it.clone(), tok))
            .collect();
        if flips % 2 == 1 {
            if body.iter().any(|it| key(it) == "INV") {
                body.retain(|it| key(it) != "INV");
            } else {
                body.push(tok("INV"));
            }
        }
        out.extend_from_slice(&frame[last..a]);
        out.extend(body);
        last = b;
    }
    out.extend_from_slice(&frame[last..]);
    Some(out)
}

type Unit = Vec<Item>;

fn ukey(u: &Unit) -> String {
    u.iter().map(key).collect::<Vec<_>>().join("|")
}

/// First two characters of the unit's head token ("S:", "T:", ...).
fn kind(u: &Unit) -> String {
    u.first().map(key).unwrap_or_default().chars().take(2).collect()
}

/// frame_parser.units: head token plus its attributes or pointer span.
fn units(frame: Vec<Item>) -> Vec<Unit> {
    let mut out: Vec<Unit> = Vec::new();
    for it in frame {
        let t = key(&it);
        let attach = out.last().is_some_and(|prev| {
            let p = ukey(prev);
            t.starts_with("object:")
                || t.starts_with("shape:")
                || (t.starts_with("side:") && (p.starts_with("S:") || p.starts_with("G:")))
                || (t.starts_with("mode:") && p.starts_with("T:"))
                || t.starts_with('=')
                || (t.starts_with("T:") && (p == "FROM" || p == "TO"))
        });
        match out.last_mut() {
            Some(prev) if attach => prev.push(it),
            _ => out.push(vec![it]),
        }
    }
    out
}

fn normalize(frame: Vec<Item>, rule: &str) -> Vec<Item> {
    if frame.first().map(key).as_deref() != Some("I:map") {
        return frame;
    }
    let (mut head, mut routes, mut tail) = (Vec::new(), Vec::new(), Vec::new());
    let mut cur: Option<Vec<Unit>> = None;
    for u in units(frame) {
        match ukey(&u).as_str() {
            "<route>" => cur = Some(Vec::new()),
            "</route>" => routes.push(cur.take().unwrap_or_default()),
            _ => match cur.as_mut() {
                Some(c) => c.push(u),
                None if routes.is_empty() => head.push(u),
                None => tail.push(u),
            },
        }
    }
    if let Some(c) = cur {
        routes.push(c);
    }
    let routes = if rule == "pinch" { pinch(routes) } else { pronoun(routes) };
    let mut out: Vec<Item> = head.into_iter().flatten().collect();
    for r in routes {
        out.push(tok("<route>"));
        out.extend(r.into_iter().flatten());
        out.push(tok("</route>"));
    }
    out.extend(tail.into_iter().flatten());
    out
}

fn pinch(routes: Vec<Vec<Unit>>) -> Vec<Vec<Unit>> {
    routes
        .into_iter()
        .map(|mut r| {
            let heads: Vec<String> = r
                .iter()
                .filter(|u| kind(u) == "S:")
                .map(|u| u.first().map(key).unwrap_or_default())
                .collect();
            let distinct: std::collections::HashSet<&String> = heads.iter().collect();
            if heads.iter().any(|h| h == "S:hand.pinch")
                && distinct.len() > 1
                && !r.iter().any(|u| ukey(u).starts_with("G:hand.pinch"))
            {
                for u in r.iter_mut() {
                    if u.first().map(key).as_deref() == Some("S:hand.pinch") {
                        u[0] = tok("G:hand.pinch");
                    }
                }
            }
            r
        })
        .collect()
}

fn pronoun(routes: Vec<Vec<Unit>>) -> Vec<Vec<Unit>> {
    use std::collections::HashSet;
    let mut out: Vec<Vec<Unit>> = Vec::new();
    for mut r in routes {
        let srcs: HashSet<String> = r.iter().filter(|u| kind(u) == "S:").map(ukey).collect();
        let tgts: Vec<String> = r.iter().filter(|u| kind(u) == "T:").map(ukey).collect();
        if let Some(prev) = out.last() {
            if tgts.is_empty() && !srcs.is_empty() {
                let used: HashSet<String> = out
                    .iter()
                    .flatten()
                    .filter(|u| kind(u) == "S:")
                    .map(ukey)
                    .collect();
                if srcs.is_subset(&used) {
                    continue;
                }
                let add: Vec<Unit> = prev.iter().filter(|u| kind(u) == "T:").cloned().collect();
                r.extend(add);
            } else if !tgts.is_empty() && srcs.is_empty() {
                let used: HashSet<String> = out
                    .iter()
                    .flatten()
                    .filter(|u| kind(u) == "T:")
                    .map(ukey)
                    .collect();
                if tgts.iter().all(|t| used.contains(t)) {
                    continue;
                }
            }
        }
        out.push(r);
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn frame(s: &str) -> Vec<Item> {
        s.split(' ')
            .map(|t| match t.strip_prefix('=') {
                Some(x) => Item::Span { text: x.into(), bytes: (0, 0) },
                None => tok(t),
            })
            .collect()
    }

    fn show(f: &[Item]) -> String {
        f.iter().map(key).collect::<Vec<_>>().join(" ")
    }

    fn run(f: &str, text: &str, rules: &str) -> String {
        let names: Vec<String> = rules.split(',').map(String::from).collect();
        show(&apply(frame(f), text, &names))
    }

    #[test]
    fn conv_pinch_with_movement_is_a_hold() {
        assert_eq!(
            run("I:map <route> S:hand.pinch S:hand.x T:param:filter.cutoff </route>", "", "conv"),
            "I:map <route> G:hand.pinch S:hand.x T:param:filter.cutoff </route>"
        );
    }

    #[test]
    fn conv_fingers_together_become_a_shape() {
        assert_eq!(
            run(
                "I:map <route> S:hand.finger.index S:hand.finger.ring T:param:amp.gain </route>",
                "index and ring finger together",
                "conv"
            ),
            "I:map <route> S:hand.shape shape:m01010 T:param:amp.gain </route>"
        );
        let f = "I:map <route> S:hand.finger.index S:hand.finger.ring T:param:amp.gain </route>";
        assert_eq!(run(f, "index then ring", "conv"), f);
    }

    #[test]
    fn conv_folds_fist_into_inverted_openness() {
        assert_eq!(
            run("I:map <route> S:hand.fist T:param:fx.reverb.mix </route>", "", "conv"),
            "I:map <route> S:hand.openness T:param:fx.reverb.mix INV </route>"
        );
        assert_eq!(
            run("I:map <route> S:eye.closed INV T:param:amp.gain </route> TRACK =pad", "", "conv"),
            "I:map <route> S:eye.open T:param:amp.gain </route> TRACK =pad"
        );
    }

    #[test]
    fn pronoun_takes_previous_target_or_drops_restatement() {
        assert_eq!(
            run(
                "I:map <route> S:hand.y T:param:fx.delay.time </route> <route> S:hand.spread </route>",
                "",
                "pronoun"
            ),
            "I:map <route> S:hand.y T:param:fx.delay.time </route> <route> S:hand.spread T:param:fx.delay.time </route>"
        );
        assert_eq!(
            run(
                "I:map <route> S:hand.y T:param:fx.delay.time </route> <route> S:hand.y INV </route> TRACK =bass",
                "",
                "pronoun"
            ),
            "I:map <route> S:hand.y T:param:fx.delay.time </route> TRACK =bass"
        );
    }
}
