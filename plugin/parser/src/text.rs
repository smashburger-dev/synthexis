//! Span text as frame_parser.items prints it: the sentence bytes from the
//! first to the last pointed piece, casefolded, whitespace collapsed, outer
//! punctuation stripped.

const STRIP: &[char] = &[' ', '.', ',', ';', ':', '!', '?', '"', '\'', '(', ')'];

/// bytes[b..e] decoded with invalid UTF-8 dropped (Python "ignore").
pub fn slice(text: &str, b: usize, e: usize) -> String {
    let bytes = text.as_bytes();
    if b >= e || e > bytes.len() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes[b..e]).replace('\u{FFFD}', "")
}

/// Python str.casefold for the letters German and English text uses:
/// lowercase plus sharp s -> "ss" and final sigma -> sigma.
fn casefold(s: &str) -> String {
    s.to_lowercase().replace('ß', "ss").replace('ς', "σ")
}

pub fn norm(s: &str) -> String {
    let s = casefold(s).split_whitespace().collect::<Vec<_>>().join(" ");
    s.trim_matches(STRIP).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_matches_python() {
        assert_eq!(norm("  Die GROSSE  Straße. "), "die grosse strasse");
        assert_eq!(norm("(\"Pluck\")"), "pluck");
        assert_eq!(slice("ab", 1, 1), "");
    }
}
