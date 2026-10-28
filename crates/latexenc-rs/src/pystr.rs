//! Python `str` semantics where they differ from Rust's: pylatexenc tokenises with
//! `str.isspace()`/`str.isalpha()` and the regex classes `\s`/`\w`, whose Unicode sets are
//! not those of `char::is_whitespace()`/`char::is_alphabetic()`. The tables
//! (`pystr_tables.rs`) are generated from CPython's own `unicodedata` by `tools/gen_tables.py`.
//!
//! Lengths and positions in pylatexenc are counted in code points (Python `str` indices);
//! here too, throughout the crate, a "position" is an index into the `char`s of the text.

use crate::pystr_tables::{ALNUM, ALPHA, DECIMAL, SPACE, WORD};

fn in_table(table: &[(u32, u32)], c: char) -> bool {
    let c = c as u32;
    table
        .binary_search_by(|&(lo, hi)| {
            if c < lo {
                std::cmp::Ordering::Greater
            } else if c > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// `c.isspace()`; also the regex class `\s` (on `str` patterns).
pub fn is_space(c: char) -> bool {
    in_table(SPACE, c)
}

/// `c.isalpha()`
pub fn is_alpha(c: char) -> bool {
    in_table(ALPHA, c)
}

/// `c.isalnum()`
pub fn is_alnum(c: char) -> bool {
    in_table(ALNUM, c)
}

/// The regex class `\w` (on `str` patterns): `c.isalnum() or c == '_'`.
pub fn is_word(c: char) -> bool {
    in_table(WORD, c)
}

/// `c.isdecimal()`; also the regex class `\d` (on `str` patterns).
pub fn is_decimal(c: char) -> bool {
    in_table(DECIMAL, c)
}

/// `s.isalpha()`: not empty and every char alphabetic.
pub fn str_is_alpha(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_alpha)
}

/// `s.strip()`
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// `s.lstrip()`
pub fn lstrip(s: &str) -> &str {
    s.trim_start_matches(is_space)
}

/// `s.rstrip()`
pub fn rstrip(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

/// `len(s)`: the number of code points.
pub fn len(s: &str) -> usize {
    s.chars().count()
}

/// `s.rjust(width, ' ')`
pub fn rjust(s: &str, width: usize) -> String {
    let n = len(s);
    if n >= width {
        s.to_string()
    } else {
        let mut r = " ".repeat(width - n);
        r.push_str(s);
        r
    }
}

/// `s.isspace()` for a whole string (not empty, all whitespace).
pub fn str_is_space(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_space)
}

/// `repr(s)` of a Python `str` (printable chars as they are, the usual escapes otherwise).
pub fn repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut r = String::new();
    r.push(quote);
    for c in s.chars() {
        match c {
            '\\' => r.push_str("\\\\"),
            '\n' => r.push_str("\\n"),
            '\r' => r.push_str("\\r"),
            '\t' => r.push_str("\\t"),
            c if c == quote => {
                r.push('\\');
                r.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => r.push_str(&format!("\\x{:02x}", c as u32)),
            c if (c as u32) >= 0x80 && (is_space(c) && c != ' ' || c.is_control()) => {
                let o = c as u32;
                if o < 0x100 {
                    r.push_str(&format!("\\x{o:02x}"));
                } else if o < 0x10000 {
                    r.push_str(&format!("\\u{o:04x}"));
                } else {
                    r.push_str(&format!("\\U{o:08x}"));
                }
            }
            c => r.push(c),
        }
    }
    r.push(quote);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        assert!(is_space('\u{a0}') && is_space('\u{1c}') && is_space('\u{85}'));
        assert!(!is_space('\u{200b}'));
        assert!(is_alpha('é') && !is_alpha('1') && !is_alpha('_'));
        assert!(is_word('_') && is_word('1') && !is_word('-'));
        assert_eq!(strip("\u{a0} x \n"), "x");
        assert_eq!(repr("it's"), "\"it's\"");
    }
}
