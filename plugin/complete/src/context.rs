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
const SWITCH_WORDS: &[&str] = &[
    "on", "off", "toggle", "toggles", "switch", "switches", "enable", "enables", "disable",
    "bypass", "mute", "unmute", "activate", "deactivate", "an", "aus", "einschalten",
    "ausschalten", "anschalten", "anmachen", "ausmachen", "schalten", "schaltet", "schalte",
    "umschalten", "aktivieren", "deaktivieren", "toggeln", "stumm", "stummschalten", "muten",
];
const CLAUSE_END: &[char] = &['.', ',', ';', '!', '?', ':'];
const MAX_WORDS: usize = 3;

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
    ARTICLES.contains(&w.as_str()) || FILLER.contains(&w.as_str())
}

fn words_lower(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// True when `before` (the text up to the query) ends where a gesture is
/// named: the sentence start or one of GESTURE_AFTER, articles skipped.
pub fn gesture_position(before: &str) -> bool {
    let b = before.trim_end();
    if b.is_empty() || b.ends_with(['.', '!', '?', ';', ':']) {
        return true;
    }
    let mut w = words_lower(b);
    while w.last().is_some_and(|l| ARTICLES.contains(&l.as_str())) {
        w.pop();
    }
    let n = w.len();
    GESTURE_AFTER.iter().any(|t| {
        let k = t.split(' ').count();
        n >= k && w[n - k..].join(" ") == *t
    })
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
        assert!(gesture_position(""));
        assert!(gesture_position("when I "));
        assert!(gesture_position("Cutoff hoch, wenn ich die "));
        assert!(gesture_position("open the filter with my "));
        assert!(gesture_position("Mach das. "));
        assert!(gesture_position("make the reverb follow my "));
        assert!(!gesture_position("the "));
        assert!(!gesture_position("when I spread my fingers the "));
        assert!(!gesture_position("withdraw "));
    }

    #[test]
    fn switch_words() {
        assert!(switching("schalte den Chorus an"));
        assert!(switching("toggle the"));
        assert!(!switching("open the filter"));
    }
}
