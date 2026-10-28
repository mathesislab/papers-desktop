//! Python's `str` methods that papers (and bibtexparser) call, with their
//! exact definitions: Rust's `trim` and `lines` differ from `strip` and
//! `splitlines` on a few characters, and the text written must be the same.

/// `str.isspace()` of one character: the characters whose bidirectional class
/// is WS, B or S, and those of category Zs.
pub fn is_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{0b}'
            | '\u{0c}'
            | '\r'
            | '\u{1c}'..='\u{1f}'
            | ' '
            | '\u{85}'
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

/// `str.strip()`.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// `str.isdigit()`, for the ASCII digits (the other Unicode digits are not
/// found in a bibtex value that is a bare number).
pub fn is_digit(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `str.splitlines()`: the lines of a text, cut at `\n`, `\r`, `\r\n`,
/// `\v`, `\f`, `\x1c`-`\x1e`, `\x85`, U+2028 and U+2029, without their ends.
pub fn splitlines(s: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if matches!(
            c,
            '\n' | '\r' | '\u{0b}' | '\u{0c}' | '\u{1c}'
                ..='\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}'
        ) {
            lines.push(&s[start..i]);
            let mut end = i + c.len_utf8();
            if c == '\r' && matches!(chars.peek(), Some((_, '\n'))) {
                chars.next();
                end += 1;
            }
            start = end;
        }
    }
    if start < s.len() {
        lines.push(&s[start..]);
    }
    lines
}

/// `str.isalnum()` of one character.
pub fn is_alnum(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric();
    }
    let c = c as u32;
    crate::pytables::ALNUM
        .binary_search_by(|&(a, b)| {
            if c < a {
                std::cmp::Ordering::Greater
            } else if c > b {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// `\w` of Python's `re` on a `str`: `str.isalnum()` and `_`.
pub fn is_word(c: char) -> bool {
    is_alnum(c) || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_as_python() {
        assert_eq!(strip("\u{1c} a b \u{a0}\n"), "a b");
        assert_eq!(strip("\u{200b}a"), "\u{200b}a"); // zero width space is not a space
        assert_eq!(strip("\u{feff}a"), "\u{feff}a");
    }

    #[test]
    fn splitlines_as_python() {
        assert_eq!(
            splitlines("a\nb\r\nc\rd\u{2028}e"),
            vec!["a", "b", "c", "d", "e"]
        );
        assert_eq!(splitlines("a\n"), vec!["a"]);
        assert_eq!(splitlines("a\n\n"), vec!["a", ""]);
        assert!(splitlines("").is_empty());
    }
}
