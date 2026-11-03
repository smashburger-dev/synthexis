//! Prefix index over completion keys: the folded text from every word start
//! of every key, sorted. A trie flattened into one array: all words that
//! start with a prefix form one contiguous range, found by binary search.

use crate::fuzzy::Key;

pub struct Index {
    pub keys: Vec<Key>,
    starts: Vec<(String, u32)>,
}

impl Index {
    pub fn new<'a>(texts: impl IntoIterator<Item = &'a str>) -> Self {
        let keys: Vec<Key> = texts.into_iter().map(Key::new).collect();
        let mut starts = Vec::new();
        for (i, k) in keys.iter().enumerate() {
            for (j, _) in k.ws.iter().enumerate().filter(|(_, &w)| w) {
                starts.push((k.chars[j..].iter().collect::<String>(), i as u32));
            }
        }
        starts.sort_unstable();
        Self { keys, starts }
    }

    /// Ids of the keys with a word that starts with `prefix` (folded), each
    /// once, ascending; `seen` is scratch space of the index's key count.
    pub fn with_word_prefix(&self, prefix: &str, seen: &mut Vec<bool>, out: &mut Vec<u32>) {
        out.clear();
        seen.clear();
        seen.resize(self.keys.len(), false);
        let lo = self.starts.partition_point(|(s, _)| s.as_str() < prefix);
        for (s, i) in &self.starts[lo..] {
            if !s.starts_with(prefix) {
                break;
            }
            if !seen[*i as usize] {
                seen[*i as usize] = true;
                out.push(*i);
            }
        }
        out.sort_unstable();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_inner_words_once() {
        let ix = Index::new(["spread my fingers", "Faust schließen", "my fist my way"]);
        let (mut seen, mut out) = (Vec::new(), Vec::new());
        ix.with_word_prefix("my", &mut seen, &mut out);
        assert_eq!(out, vec![0, 2]);
        ix.with_word_prefix("schliess", &mut seen, &mut out);
        assert_eq!(out, vec![1]);
        ix.with_word_prefix("q", &mut seen, &mut out);
        assert!(out.is_empty());
    }
}
