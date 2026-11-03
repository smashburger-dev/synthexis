//! What the words around the cursor say: which typed words form the query,
//! whether the query sits where a gesture is named, and whether the clause
//! talks about switching something on or off.

/// Words before a gesture: "when I ...", "wenn ich ...", "with my ...".
const GESTURE_AFTER: &[&str] = &[
    "when i", "if i", "while i", "once i", "as i", "wenn ich", "falls ich", "sobald ich",
    "während ich", "solange ich", "with", "mit", "by", "durch", "follow", "follows", "folgt",
];
/// A query never starts with one of these: "the res" is "res" after "the".
const FILLER: &[&str] = &["when", "if", "i", "wenn", "ich", "with", "mit", "and", "und"];
/// Skipped between the trigger and the gesture ("wenn ich die Faust").
const ARTICLES: &[&str] = &[
    "the", "a", "my", "both", "die", "der", "den", "das", "dem", "mein", "meine", "meiner",
    "meinem", "meinen", "beide", "beiden",
];
/// Weaker signs of a gesture: a possessive before the word ("i want my
/// head ..."), a list or a manner ("pinch und drehen", "indem ich").
const POSSESSIVES: &[&str] = &["my", "mein", "meine", "meiner", "meinem", "meinen"];
const WEAK_AFTER: &[&str] = &["and", "und", "then", "dann", "indem ich", "dass ich"];
/// Side words, skipped like articles ("wenn ich die linke Faust").
const SIDES: &[&str] = &[
    "left", "right", "linke", "linken", "linker", "linkes", "rechte", "rechten", "rechter",
    "rechtes",
];
const SWITCH_WORDS: &[&str] = &[
    "on", "off", "toggle", "toggles", "switch", "switches", "enable", "enables", "disable",
    "bypass", "mute", "unmute", "activate", "deactivate", "an", "aus", "einschalten",
    "ausschalten", "anschalten", "anmachen", "ausmachen", "schalten", "schaltet", "schalte",
    "umschalten", "aktivieren", "deaktivieren", "toggeln", "stumm", "stummschalten", "muten",
];
const CLAUSE_END: &[char] = &['.', ',', ';', '!', '?', ':'];
const MAX_WORDS: usize = 3;

/// The hand side the words name, the last one winning: "with my left
/// hand" -> left, "die rechte" -> right, "beide Hände" -> both.
pub fn named_side(text: &str) -> Option<&'static str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter_map(|w| match w.to_lowercase().as_str() {
            "left" | "links" | "linke" | "linken" | "linker" | "linkes" => Some("left"),
            "right" | "rechts" | "rechte" | "rechten" | "rechter" | "rechtes" => Some("right"),
            "both" | "beide" | "beiden" | "beidhändig" => Some("both"),
            _ => None,
        })
        .last()
}

/// The side named right before a gesture ("die linke Fau", "my left
/// hand"): the word before it, after articles and possessives. So "left
/// swipe to eq and circle" and "Hand nach rechts und Faust" give no side.
/// "both" counts too; completion offers no both-hands side and drops it.
pub fn side_before(text: &str) -> Option<&'static str> {
    let words: Vec<String> = text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    // "both" and "beide" sit in ARTICLES too; a side word is never skipped
    let skip = |w: &String| (ARTICLES.contains(&w.as_str()) || POSSESSIVES.contains(&w.as_str())) && named_side(w).is_none();
    let last = words.iter().rev().find(|w| !skip(w))?;
    named_side(last)
}

/// A query: the last `words` words before the cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct QuerySpan {
    pub start: usize,
    pub words: usize,
}

/// Start of the clause the cursor is in, never before `floor`.
pub fn clause_start(text: &str, cursor: usize, floor: usize) -> usize {
    text[floor..cursor]
        .rfind(CLAUSE_END)
        .map_or(floor, |i| floor + i + 1)
}

/// The last 1..=3 words before the cursor inside its clause; none when the
/// cursor follows a space (nothing typed yet) or `floor` (a chip's end).
pub fn queries(text: &str, cursor: usize, floor: usize) -> Vec<QuerySpan> {
    let lo = clause_start(text, cursor, floor);
    let pre = &text[lo..cursor];
    if pre.is_empty() || pre.ends_with(char::is_whitespace) {
        return Vec::new();
    }
    let mut starts = Vec::new();
    let mut prev_ws = true;
    for (i, c) in pre.char_indices() {
        if prev_ws && !c.is_whitespace() {
            starts.push(lo + i);
        }
        prev_ws = c.is_whitespace();
    }
    starts
        .iter()
        .rev()
        .take(MAX_WORDS)
        .enumerate()
        .map(|(k, &start)| QuerySpan { start, words: k + 1 })
        .filter(|q| q.words == 1 || !starts_with_filler(&text[q.start..cursor]))
        .collect()
}

fn starts_with_filler(s: &str) -> bool {
    let w = s.split_whitespace().next().unwrap_or("").to_lowercase();
    ARTICLES.contains(&w.as_str()) || FILLER.contains(&w.as_str()) || SIDES.contains(&w.as_str())
}

fn words_lower(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Where a gesture may be named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GesturePos {
    No,
    /// Offered in the list only, never as ghost text.
    Weak,
    Strong,
}

fn ends_with_any(w: &[String], triggers: &[&str]) -> bool {
    let n = w.len();
    triggers.iter().any(|t| {
        let k = t.split(' ').count();
        n >= k && w[n - k..].join(" ") == *t
    })
}

/// Whether `before` (the text up to the query) ends where a gesture is
/// named. Strong: the sentence start or one of GESTURE_AFTER, articles and
/// side words skipped. Weak: a possessive or one of WEAK_AFTER.
pub fn gesture_position(before: &str) -> GesturePos {
    let b = before.trim_end();
    if b.is_empty() || b.ends_with(['.', '!', '?', ';', ':']) {
        return GesturePos::Strong;
    }
    let mut w = words_lower(b);
    let mut possessive = false;
    while w.last().is_some_and(|l| ARTICLES.contains(&l.as_str()) || SIDES.contains(&l.as_str())) {
        possessive |= w.last().is_some_and(|l| POSSESSIVES.contains(&l.as_str()));
        w.pop();
    }
    if ends_with_any(&w, GESTURE_AFTER) {
        GesturePos::Strong
    } else if possessive || ends_with_any(&w, WEAK_AFTER) {
        GesturePos::Weak
    } else {
        GesturePos::No
    }
}

/// True when the clause names switching ("schalte ... an", "toggle the").
pub fn switching(clause: &str) -> bool {
    words_lower(clause)
        .iter()
        .any(|w| SWITCH_WORDS.contains(&w.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_words() {
        let t = "when I spread my fin";
        let q = queries(t, t.len(), 0);
        assert_eq!(q.len(), 2); // "my fin" starts with an article
        assert_eq!(&t[q[0].start..], "fin");
        assert_eq!(&t[q[1].start..], "spread my fin");
        let t = "the res";
        assert_eq!(queries(t, t.len(), 0).len(), 1);
        assert!(queries("when I ", 7, 0).is_empty());
        let t = "Faust, cu";
        assert_eq!(queries(t, t.len(), 0).len(), 1);
    }

    #[test]
    fn floor_stops_queries() {
        let t = "spread my fingers cu";
        let q = queries(t, t.len(), 17);
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn gesture_positions() {
        assert_eq!(gesture_position(""), GesturePos::Strong);
        assert_eq!(gesture_position("when I "), GesturePos::Strong);
        assert_eq!(gesture_position("Cutoff hoch, wenn ich die "), GesturePos::Strong);
        assert_eq!(gesture_position("open the filter with my "), GesturePos::Strong);
        assert_eq!(gesture_position("Mach das. "), GesturePos::Strong);
        assert_eq!(gesture_position("make the reverb follow my "), GesturePos::Strong);
        assert_eq!(gesture_position("the "), GesturePos::No);
        assert_eq!(gesture_position("when I spread my fingers the "), GesturePos::No);
        assert_eq!(gesture_position("withdraw "), GesturePos::No);
        assert_eq!(gesture_position("Cutoff hoch, wenn ich die linke "), GesturePos::Strong);
        assert_eq!(gesture_position("i want my "), GesturePos::Weak);
        assert_eq!(gesture_position("pinch und "), GesturePos::Weak);
        assert_eq!(gesture_position("Filter auf, indem ich den "), GesturePos::Weak);
    }

    #[test]
    fn switch_words() {
        assert!(switching("schalte den Chorus an"));
        assert!(switching("toggle the"));
        assert!(!switching("open the filter"));
    }
}
