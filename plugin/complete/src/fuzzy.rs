//! Subsequence match score in the spirit of VS Code's fuzzyScore: every
//! query char appears in the key in order, the first one at a word start;
//! a match at the key start, at a word start or right after the previous
//! match scores more, a jump over skipped chars costs. The score is divided
//! by the query's score against itself, so a prefix match is 1.0.

const CHAR: i32 = 1;
const START: i32 = 8;
const WORD: i32 = 5;
const CONSEC: i32 = 5;
const JUMP: i32 = 3;
const NEG: i32 = i32::MIN / 2;

/// Lowercase, sharp s as "ss", every run of non-alphanumerics as one space.
pub fn fold(s: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        if c == 'ß' {
            out.extend(['s', 's']);
        } else if c.is_alphanumeric() {
            out.push(c);
        } else if out.last().is_some_and(|&l| l != ' ') {
            out.push(' ');
        }
    }
    if out.last() == Some(&' ') {
        out.pop();
    }
    out
}

/// True where a word starts: after a space and at letter/digit changes
/// ("Filter1" has starts at 'f' and '1').
pub fn word_starts(t: &[char]) -> Vec<bool> {
    (0..t.len())
        .map(|j| {
            j == 0
                || (t[j] != ' '
                    && (t[j - 1] == ' ' || t[j - 1].is_numeric() != t[j].is_numeric()))
        })
        .collect()
}

/// Folded key with its word starts, built once per key.
pub struct Key {
    pub chars: Vec<char>,
    pub ws: Vec<bool>,
}

impl Key {
    pub fn new(s: &str) -> Self {
        let chars = fold(s);
        let ws = word_starts(&chars);
        Self { chars, ws }
    }
}

fn raw(q: &[char], t: &[char], ws: &[bool]) -> Option<i32> {
    let (m, n) = (q.len(), t.len());
    if m == 0 || m > n {
        return None;
    }
    // prev[j]: best score with q[i-1] matched at t[j]
    let mut prev = vec![NEG; n];
    let mut cur = vec![NEG; n];
    for (i, &qc) in q.iter().enumerate() {
        let mut before = NEG; // max of prev[..j-1]
        for j in 0..n {
            if i > 0 && j >= 2 {
                before = before.max(prev[j - 2]);
            }
            cur[j] = NEG;
            if t[j] != qc {
                continue;
            }
            let bonus = if j == 0 {
                START
            } else if ws[j] {
                WORD
            } else {
                0
            };
            if i == 0 {
                if ws[j] {
                    cur[j] = CHAR + bonus;
                }
                continue;
            }
            let consec = if j >= 1 && prev[j - 1] > NEG {
                prev[j - 1] + CONSEC
            } else {
                NEG
            };
            let jump = if before > NEG { before - JUMP } else { NEG };
            let b = consec.max(jump);
            if b > NEG {
                cur[j] = b + CHAR + bonus;
            }
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev.into_iter().max().filter(|&s| s > NEG)
}

/// Query folded once per keystroke, with its perfect score.
pub struct Query {
    pub chars: Vec<char>,
    best: i32,
}

impl Query {
    pub fn new(s: &str) -> Self {
        let chars = fold(s);
        let best = raw(&chars, &chars, &word_starts(&chars)).unwrap_or(1).max(1);
        Self { chars, best }
    }

    /// 0..1.05: match quality plus a small bonus for short keys, so "cut"
    /// ranks "Cut" over "Cutoff Envelope".
    pub fn score(&self, key: &Key) -> Option<f32> {
        let s = raw(&self.chars, &key.chars, &key.ws)?;
        let m = (s as f32 / self.best as f32).min(1.0);
        Some(m + 0.05 * self.chars.len() as f32 / key.chars.len() as f32)
    }

    /// The key starts with the query: the typed text continues into it.
    pub fn is_prefix(&self, key: &Key) -> bool {
        key.chars.starts_with(&self.chars)
    }

    /// Every typed word sits inside one word of the key, starting at its
    /// start, and the typed words take the key's words in order: "Filter
    /// au" fits "Filter aufmachen", "res" does not fit "Bend Regler vom
    /// Synth" (r, e in one word, s in another).
    pub fn word_aligned(&self, key: &Key) -> bool {
        let mut words = key.chars.split(|&c| c == ' ');
        self.chars.split(|&c| c == ' ').all(|q| {
            words.any(|w| w.first() == q.first() && {
                let mut rest = w.iter();
                q.iter().all(|c| rest.any(|x| x == c))
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(q: &str, k: &str) -> Option<f32> {
        Query::new(q).score(&Key::new(k))
    }

    #[test]
    fn fold_keeps_letters_and_digits() {
        assert_eq!(fold("Faust schließen!").iter().collect::<String>(), "faust schliessen");
        assert_eq!(fold("LP/HP  Cutoff").iter().collect::<String>(), "lp hp cutoff");
    }

    #[test]
    fn prefix_is_best() {
        let p = s("spr", "spread my fingers").unwrap_or(0.0);
        assert!(p >= 1.0, "{p}");
        let mid = s("cut", "Filter Cutoff").unwrap_or(0.0);
        assert!(mid > 0.7 && mid < p, "{mid}");
    }

    #[test]
    fn scattered_letters_score_low() {
        let scattered = s("rev", "Ringfinger verstecken").unwrap_or(0.0);
        let across = s("mou", "Mod 1 Out").unwrap_or(0.0);
        let prefix = s("mou", "mouth opens").unwrap_or(0.0);
        assert!(scattered < 0.6, "{scattered}");
        assert!(across < prefix, "{across} {prefix}");
    }

    #[test]
    fn first_char_needs_word_start() {
        assert_eq!(s("ut", "Cutoff"), None);
        assert_eq!(s("xyz", "Cutoff"), None);
        assert!(s("frq", "Filter 1 Freq").is_some());
    }

    #[test]
    fn multi_word_query() {
        let a = s("Filter au", "Filter aufmachen").unwrap_or(0.0);
        let b = s("Filter au", "Filter 1 Freq");
        assert!(a >= 1.0);
        assert_eq!(b, None);
    }

    #[test]
    fn word_aligned_keeps_typed_words_inside_key_words() {
        let a = |q: &str, k: &str| Query::new(q).word_aligned(&Key::new(k));
        assert!(a("Filter au", "Filter aufmachen"));
        assert!(a("frq", "Freq Filter"));
        assert!(a("cut", "the Cutoff"));
        assert!(!a("res", "Bend Regler vom Synth"));
        assert!(!a("au Filter", "Filter aufmachen"));
    }

    #[test]
    fn word_starts_split_digits() {
        let k = Key::new("Filter1Freq");
        assert_eq!(k.ws.iter().filter(|&&w| w).count(), 3);
    }
}
