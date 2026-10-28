//! Python's `textwrap.wrap` (the defaults: break long words, break on
//! hyphens), which argparse uses to wrap the help of the options.

fn is_letter(c: char) -> bool {
    c == '_' || (c.is_alphabetic() && !c.is_numeric())
}

fn is_word_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

fn is_word_punct(c: char) -> bool {
    is_word_char(c) || matches!(c, '!' | '"' | '\'' | '&' | '.' | ',' | '?')
}

/// `TextWrapper._split`: the chunks of a text without tabs or newlines (the
/// words, cut after some hyphens, the white space, the em-dashes).
fn split_chunks(text: &str) -> Vec<Vec<char>> {
    let chars: Vec<char> = text.chars().collect();
    let mut chunks: Vec<Vec<char>> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            chunks.push(chars[i..j].to_vec());
            i = j;
            continue;
        }
        // an em-dash between words
        if chars[i] == '-' && i > 0 && is_word_punct(chars[i - 1]) {
            let mut j = i;
            while j < chars.len() && chars[j] == '-' {
                j += 1;
            }
            if j - i >= 2 && j < chars.len() && is_word_char(chars[j]) {
                chunks.push(chars[i..j].to_vec());
                i = j;
                continue;
            }
        }
        // a word: at least one character, then up to the first place where it may end
        let mut j = i + 1;
        loop {
            if j >= chars.len() || chars[j].is_whitespace() {
                break; // the end of the word
            }
            // a hyphenated word: the hyphen at j-1 ends the chunk
            let h = j - 1;
            if chars[h] == '-' && h >= 2 {
                let before = (is_letter(chars[h - 1]) && is_letter(chars[h - 2]))
                    || (h >= 3 && is_letter(chars[h - 1]) && chars[h - 2] == '-' && is_letter(chars[h - 3]));
                let after = j < chars.len()
                    && is_letter(chars[j])
                    && ((j + 1 < chars.len() && is_letter(chars[j + 1]))
                        || (j + 2 < chars.len() && chars[j + 1] == '-' && is_letter(chars[j + 2])));
                if before && after {
                    break;
                }
            }
            // before an em-dash
            if is_word_punct(chars[j - 1]) && chars[j] == '-' {
                let mut k = j;
                while k < chars.len() && chars[k] == '-' {
                    k += 1;
                }
                if k - j >= 2 && k < chars.len() && is_word_char(chars[k]) {
                    break;
                }
            }
            j += 1;
        }
        chunks.push(chars[i..j].to_vec());
        i = j;
    }
    chunks
}

fn len(chunk: &[char]) -> usize {
    chunk.len()
}

fn is_blank(chunk: &[char]) -> bool {
    chunk.iter().all(|c| c.is_whitespace())
}

/// `textwrap.wrap(text, width)`.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut chunks = split_chunks(text);
    chunks.reverse();
    let mut lines: Vec<String> = Vec::new();
    while !chunks.is_empty() {
        let mut current: Vec<Vec<char>> = Vec::new();
        let mut current_len = 0;
        // drop the white space at the start of a line that is not the first
        if chunks.last().is_some_and(|c| is_blank(c)) && !lines.is_empty() {
            chunks.pop();
        }
        while let Some(last) = chunks.last() {
            if current_len + len(last) <= width {
                current_len += len(last);
                current.push(chunks.pop().expect("a chunk"));
            } else {
                break;
            }
        }
        // a word longer than the line
        if chunks.last().is_some_and(|c| len(c) > width) {
            let space_left = if width < 1 { 1 } else { width - current_len };
            let chunk = chunks.last().expect("a chunk").clone();
            let mut end = space_left;
            if len(&chunk) > space_left {
                let hyphen = chunk[..space_left.min(chunk.len())].iter().rposition(|c| *c == '-');
                if let Some(h) = hyphen
                    && h > 0
                    && chunk[..h].iter().any(|c| *c != '-')
                {
                    end = h + 1;
                }
            }
            current.push(chunk[..end.min(chunk.len())].to_vec());
            let last = chunks.len() - 1;
            chunks[last] = chunk[end.min(chunk.len())..].to_vec();
        }
        if current.last().is_some_and(|c| is_blank(c)) {
            current.pop();
        }
        if !current.is_empty() {
            lines.push(current.iter().flat_map(|c| c.iter()).collect());
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_like_textwrap() {
        assert_eq!(
            wrap("separator of the title words in the filename (default: -)", 54),
            ["separator of the title words in the filename (default:", "-)"]
        );
        assert_eq!(
            wrap("minimum length of a title word: shorter words are left out of the filename (default: 1)", 54),
            ["minimum length of a title word: shorter words are left", "out of the filename (default: 1)"]
        );
        // hyphens: the word is cut after one
        assert_eq!(wrap("aaaa bbbb-cccc dddd", 10), ["aaaa bbbb-", "cccc dddd"]);
        // a long word is broken
        assert_eq!(wrap("x abcdefghijklmnop", 10), ["x abcdefgh", "ijklmnop"]);
    }
}
