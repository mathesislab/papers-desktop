//! LaTeX to Unicode, the decoder of the same-paper rule and of the Mendeley
//! file links (`papers.latex`, spec `bibtex.md` 4.1). The text is read token
//! by token, as LaTeX reads it:
//!
//! - an accent command (`\'e`, `\'{e}`, `{\'e}`, `\c c`, `\v{S}`) puts its
//!   accent on the first character of its argument (the next character or
//!   braced group, spaces before it skipped); on `\i` and `\j` the accent goes
//!   on `i` and `j`;
//! - a command that names a character (`\o`, `\ss`, `\alpha`, `\textendash`)
//!   gives that character; `\&`, `\%`, `\$`, `\#`, `\_`, `\{`, `\}` give the
//!   character after the backslash;
//! - a formatting command (`\textit`, `\emph`, `\url`...) is dropped and its
//!   argument kept;
//! - any other command is kept as written;
//! - `{`, `}` and `$` are dropped, `~` is a space, `--` and `---` are en and
//!   em dashes, ``` `` ``` and `''` are double quotes;
//! - the result is in Unicode NFC form.
//!
//! ```
//! use papers_rs::bib::latex::latex_to_unicode;
//! assert_eq!(latex_to_unicode("T\\^ete \\v{S}koda", true), "Tête Škoda");
//! assert_eq!(latex_to_unicode("\\url{x}", true), "x");
//! // a file path: only the commands and the braces
//! assert_eq!(latex_to_unicode("a~b--c", false), "a~b--c");
//! ```
//!
//! The commands that name a character are those of the character table of
//! [`super::latex_table`] that are written as one command.

use std::collections::HashMap;
use std::sync::OnceLock;

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;

use super::latexenc_tables::{ACCENTED_LATEX, UNICODE_TO_LATEX};

/// The accent commands: the combining character each puts on its argument.
fn accent(name: &str) -> Option<char> {
    Some(match name {
        "`" | "grave" => '\u{300}',
        "'" | "acute" => '\u{301}',
        "^" | "hat" => '\u{302}',
        "~" | "tilde" => '\u{303}',
        "=" | "bar" => '\u{304}',
        "u" | "breve" => '\u{306}',
        "." | "dot" => '\u{307}',
        "\"" | "ddot" => '\u{308}',
        "r" => '\u{30a}',
        "H" => '\u{30b}',
        "v" | "check" => '\u{30c}',
        "d" => '\u{323}',
        "c" => '\u{327}',
        "k" => '\u{328}',
        "b" => '\u{331}',
        "t" => '\u{361}',
        "vec" => '\u{20d7}',
        _ => return None,
    })
}

/// The formatting commands: dropped, their argument kept.
const FORMATTING: &[&str] = &[
    "emph",
    "textit",
    "textbf",
    "textsc",
    "textrm",
    "textsf",
    "texttt",
    "textup",
    "textsl",
    "textmd",
    "textnormal",
    "textsuperscript",
    "textsubscript",
    "mathrm",
    "mathit",
    "mathbf",
    "mathsf",
    "mathtt",
    "mathcal",
    "mathnormal",
    "text",
    "mbox",
    "hbox",
    "url",
    "href",
    "ensuremath",
    "protect",
    "relax",
    "cyrchar",
    "it",
    "bf",
    "sc",
    "rm",
    "sf",
    "tt",
    "em",
    "sl",
    "up",
    "normalfont",
    "nocase",
];

/// Character names that the table lacks.
const MORE_CHARACTERS: &[(&str, char)] = &[
    ("dh", 'ð'),
    ("j", 'ȷ'),
    ("times", '×'),
    ("ell", 'ℓ'),
    ("copyright", '©'),
    ("textregistered", '®'),
];

/// The name of a command in a LaTeX text of the table written as one command
/// (`_NAMED`): `{\o}`, `\o{}`, `\o` or `\o `, `$\alpha$`, with `\cyrchar` before.
fn named(latex: &str) -> Option<&str> {
    // the `$` of the pattern also matches before a last newline
    named_whole(latex).or_else(|| named_whole(latex.strip_suffix('\n')?))
}

fn named_whole(text: &str) -> Option<&str> {
    if let Some(rest) = text.strip_prefix("\\cyrchar")
        && let Some(name) = named_command(rest)
    {
        return Some(name);
    }
    named_command(text)
}

/// `[A-Za-z]+`, the whole of the text.
fn word(text: &str) -> Option<&str> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_alphabetic())).then_some(text)
}

fn named_command(text: &str) -> Option<&str> {
    let braced = text.strip_prefix("{\\").and_then(|t| t.strip_suffix('}'));
    let empty_group = text.strip_prefix('\\').and_then(|t| t.strip_suffix("{}"));
    let plain = text
        .strip_prefix('\\')
        .map(|t| t.strip_suffix(' ').unwrap_or(t));
    let math = text.strip_prefix("$\\").and_then(|t| t.strip_suffix('$'));
    [braced, empty_group, plain, math]
        .into_iter()
        .flatten()
        .find_map(word)
}

/// The commands that name a character, and the character (`characters`): those of the
/// character table written as one command, the first one of each name.
fn characters() -> &'static HashMap<&'static str, char> {
    static MAP: OnceLock<HashMap<&'static str, char>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut names: HashMap<&str, char> = HashMap::new();
        for (c, latex) in UNICODE_TO_LATEX.iter().chain(ACCENTED_LATEX) {
            if let Some(name) = named(latex)
                && accent(name).is_none()
            {
                names.entry(name).or_insert(*c);
            }
        }
        for (name, c) in MORE_CHARACTERS {
            names.entry(name).or_insert(*c);
        }
        names
    })
}

/// The index after the `}` that closes the `{` at `s[i]` (or the end of `s`).
fn group_end(s: &[char], i: usize) -> usize {
    let mut depth = 0;
    for (j, &c) in s.iter().enumerate().skip(i) {
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                return j + 1;
            }
        }
    }
    s.len()
}

/// The name of the command at `s[i]` (a backslash) and the index after it.
fn command(s: &[char], i: usize) -> (String, usize) {
    let mut j = i + 1;
    while j < s.len() && s[j].is_ascii_alphabetic() {
        j += 1;
    }
    if j == i + 1 {
        // a control symbol: one character
        let end = (i + 2).min(s.len());
        return (s[i + 1..end].iter().collect(), end);
    }
    (s[i + 1..j].iter().collect(), j)
}

fn skip_spaces(s: &[char], mut i: usize) -> usize {
    while i < s.len() && matches!(s[i], ' ' | '\t' | '\n') {
        i += 1;
    }
    i
}

/// The decoded text of the argument of an accent that starts at `s[i]`, with
/// the accent on, and the index after the argument.
fn decode_accent(s: &[char], i: usize, mark: char, text: bool) -> (String, usize) {
    let i = skip_spaces(s, i);
    if i >= s.len() {
        return (String::new(), i);
    }
    let (arg, end): (Vec<char>, usize) = if s[i] == '{' {
        let end = group_end(s, i);
        let inner = if s.get(end - 1) == Some(&'}') {
            &s[i + 1..end - 1]
        } else {
            &s[i + 1..end]
        };
        (decode(inner, text).chars().collect(), end)
    } else if s[i] == '\\' {
        let end = command(s, i).1;
        (decode(&s[i..end], text).chars().collect(), end)
    } else {
        (vec![s[i]], i + 1)
    };
    if arg.is_empty() {
        return (String::new(), end);
    }
    let first = match arg[0] {
        'ı' => 'i',
        'ȷ' => 'j',
        c => c,
    };
    let mut k = 1; // after the accents the first character already has
    while k < arg.len() && canonical_combining_class(arg[k]) != 0 {
        k += 1;
    }
    let mut out = String::new();
    out.push(first);
    out.extend(&arg[1..k]);
    out.push(mark);
    out.extend(&arg[k..]);
    (out, end)
}

fn starts_with(s: &[char], i: usize, text: &str) -> bool {
    text.chars()
        .enumerate()
        .all(|(k, c)| s.get(i + k) == Some(&c))
}

fn decode(s: &[char], text: bool) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == '\\' {
            let (name, j) = command(s, i);
            if let Some(mark) = accent(&name) {
                let (accented, end) = decode_accent(s, j, mark, text);
                out.push_str(&accented);
                i = end;
                continue;
            }
            if !name.is_empty() && name.chars().all(char::is_alphabetic) {
                // the spaces after a command word are not text
                let after = skip_spaces(s, j);
                if FORMATTING.contains(&name.as_str()) {
                    i = after;
                } else if let Some(c) = characters().get(name.as_str()) {
                    out.push(*c);
                    i = after;
                } else {
                    out.extend(&s[i..j]);
                    i = j;
                }
            } else if matches!(name.as_str(), "&" | "%" | "$" | "#" | "_" | "{" | "}") {
                out.push_str(&name);
                i = j;
            } else if matches!(
                name.as_str(),
                " " | "," | ";" | ":" | "!" | "\\" | "\n" | "\t"
            ) {
                out.push(' ');
                i = j;
            } else if name == "-" {
                // a hyphenation point
                i = j;
            } else {
                out.extend(&s[i..j]);
                i = j;
            }
        } else if c == '{' || c == '}' {
            i += 1;
        } else if !text {
            out.push(c);
            i += 1;
        } else if c == '$' {
            i += 1;
        } else if c == '~' {
            out.push('\u{a0}');
            i += 1;
        } else if starts_with(s, i, "---") {
            out.push('—');
            i += 3;
        } else if starts_with(s, i, "--") {
            out.push('–');
            i += 2;
        } else if starts_with(s, i, "``") || starts_with(s, i, "''") {
            out.push('"');
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// The Unicode text of a LaTeX text. With `text` false (a file path), only the
/// commands and the braces are decoded: `$`, `~`, dashes and quotes stay as written.
pub fn latex_to_unicode(s: &str, text: bool) -> String {
    if s.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    decode(&chars, text).nfc().collect()
}
