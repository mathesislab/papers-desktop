//! The part of Python's `textwrap` that latex2text's `fill_text` uses: `textwrap.fill(text,
//! width, initial_indent=...)` with the default options, as of Python 3.14.

use crate::pystr::{is_decimal, is_word};

/// The whitespace `textwrap` knows (ASCII only; NBSP, for instance, is not breakable).
fn is_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0b' | '\x0c' | '\r' | ' ')
}

/// `[\w!"'&.,?]`
fn is_word_punct(c: char) -> bool {
    is_word(c) || matches!(c, '!' | '"' | '\'' | '&' | '.' | ',' | '?')
}

/// `[^\d\W]`
fn is_letter(c: char) -> bool {
    is_word(c) && !is_decimal(c)
}

/// `text.expandtabs(8)` then every whitespace char to a space.
fn munge_whitespace(text: &str) -> Vec<char> {
    let mut out = Vec::new();
    let mut col = 0usize;
    for c in text.chars() {
        match c {
            '\t' => {
                let n = 8 - col % 8;
                out.extend(std::iter::repeat_n(' ', n));
                col += n;
            }
            '\n' | '\r' => {
                out.push(c);
                col = 0;
            }
            _ => {
                out.push(c);
                col += 1;
            }
        }
    }
    out.iter().map(|&c| if is_ws(c) { ' ' } else { c }).collect()
}

/// `TextWrapper.wordsep_re.split(text)` without the empty strings.
fn split_chunks(s: &[char]) -> Vec<String> {
    let n = s.len();
    let at = |i: usize| s.get(i).copied();
    // the number of dashes at j, if there are at least two and a word char after them
    let emdash_ahead = |j: usize| -> Option<usize> {
        let mut d = 0;
        while at(j + d) == Some('-') {
            d += 1;
        }
        (d >= 2 && at(j + d).is_some_and(is_word)).then_some(d)
    };
    let mut chunks = Vec::new();
    let mut i = 0;
    while i < n {
        // whitespace
        if is_ws(s[i]) {
            let mut j = i;
            while j < n && is_ws(s[j]) {
                j += 1;
            }
            chunks.push(s[i..j].iter().collect());
            i = j;
            continue;
        }
        // em-dash between words
        if i >= 1 && is_word_punct(s[i - 1])
            && let Some(d) = emdash_ahead(i) {
                chunks.push(s[i..i + d].iter().collect());
                i += d;
                continue;
            }
        // word, possibly hyphenated
        let mut k = 1;
        loop {
            let j = i + k;
            // hyphenated word
            if at(j) == Some('-') {
                let lb = (j >= 2 && is_letter(s[j - 2]) && is_letter(s[j - 1]))
                    || (j >= 3 && is_letter(s[j - 3]) && s[j - 2] == '-' && is_letter(s[j - 1]));
                let la = at(j + 1).is_some_and(is_letter)
                    && ((at(j + 2) == Some('-') && at(j + 3).is_some_and(is_letter))
                        || at(j + 2).is_some_and(is_letter));
                if lb && la {
                    chunks.push(s[i..j + 1].iter().collect());
                    i = j + 1;
                    break;
                }
            }
            // end of word
            if j >= n || is_ws(s[j]) {
                chunks.push(s[i..j].iter().collect());
                i = j;
                break;
            }
            // em-dash
            if is_word_punct(s[j - 1]) && emdash_ahead(j).is_some() {
                chunks.push(s[i..j].iter().collect());
                i = j;
                break;
            }
            k += 1;
        }
    }
    chunks
}

fn clen(s: &str) -> usize {
    s.chars().count()
}

fn is_blank(s: &str) -> bool {
    s.chars().all(crate::pystr::is_space)
}

/// `textwrap.fill(text, width, initial_indent=initial_indent)`
pub(crate) fn fill(text: &str, width: usize, initial_indent: &str) -> Result<String, String> {
    if width == 0 {
        return Err(format!("invalid width {width} (must be > 0)"));
    }
    let mut chunks = split_chunks(&munge_whitespace(text));
    chunks.reverse();
    let mut lines: Vec<String> = Vec::new();
    while !chunks.is_empty() {
        let mut cur_line: Vec<String> = Vec::new();
        let mut cur_len = 0usize;
        let indent = if lines.is_empty() { initial_indent } else { "" };
        let width = width as isize - clen(indent) as isize;

        if is_blank(chunks.last().unwrap()) && !lines.is_empty() {
            chunks.pop();
        }
        while let Some(last) = chunks.last() {
            let l = clen(last) as isize;
            if cur_len as isize + l <= width {
                cur_len += clen(last);
                cur_line.push(chunks.pop().unwrap());
            } else {
                break;
            }
        }
        if let Some(last) = chunks.last()
            && clen(last) as isize > width {
                // _handle_long_word
                let space_left = if width < 1 { 1 } else { width - cur_len as isize } as usize;
                if space_left > 0 {
                    let mut end = space_left;
                    let chunk: Vec<char> = last.chars().collect();
                    if chunk.len() > space_left {
                        // chunk.rfind('-', 0, space_left)
                        if let Some(hyphen) = chunk[..space_left].iter().rposition(|&c| c == '-')
                            && hyphen > 0 && chunk[..hyphen].iter().any(|&c| c != '-') {
                                end = hyphen + 1;
                            }
                    }
                    cur_line.push(chunk[..end].iter().collect());
                    *chunks.last_mut().unwrap() = chunk[end..].iter().collect();
                }
            }
        if let Some(last) = cur_line.last()
            && is_blank(last) {
                cur_line.pop();
            }
        if !cur_line.is_empty() {
            lines.push(format!("{indent}{}", cur_line.concat()));
        }
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks() {
        let c = split_chunks(&munge_whitespace("Look, goof-ball -- use the -b option!"));
        assert_eq!(c, ["Look,", " ", "goof-", "ball", " ", "--", " ", "use", " ", "the", " ", "-b", " ", "option!"]);
    }

    #[test]
    fn fills() {
        assert_eq!(fill("aaa bbb ccc ddd", 7, "").unwrap(), "aaa bbb\nccc ddd");
        assert_eq!(fill("abcdefghij", 4, "").unwrap(), "abcd\nefgh\nij");
    }
}
