//! The names papers gives: the keys of the entries and the names of their
//! files (`papers.filename`, `Biblio.generate_key`, `append_abc`,
//! `attachment_names`).
//!
//! ```
//! use papers_rs::bib::Entry;
//! use papers_rs::names::Format;
//!
//! let mut entry = Entry::new("article", "x");
//! entry.set("author", "Müller, Hans and Doe, Jane");
//! entry.set("year", "2020");
//! entry.set("title", "Sea level rise: a review");
//! assert_eq!(Format::key().render(&entry).unwrap(), "muller_doe2020");
//! assert_eq!(Format::name().render(&entry).unwrap(), "muller_and_doe_2020_sea-level-rise-a-review");
//! ```
//!
//! - [`ascii`]: the transliteration to ASCII, in one place. papers uses the
//!   Unidecode table (Python); this crate uses the `deunicode` crate, which
//!   differs for some characters (see `tests/expected/translit.json`). Whoever
//!   changes the table changes it here, and only here.
//! - [`slugify`], [`path_safe`], [`title_words`]: what the placeholders are made of.
//! - [`Format`]: a name or key template with its options ([`Format::render`]);
//!   the template syntax is Python's `str.format` ([`template`]).
//! - [`append_abc`], [`is_valid_key`], [`check_key`]: the keys.
//! - [`attachment_names`], [`target_in`]: where the files of an entry go, and
//!   the check that they stay in the files directory.

pub mod template;

use std::collections::HashSet;

use unicode_normalization::UnicodeNormalization;

use crate::bib::pystr::is_space;
use crate::config::paths::{basename, dirname, join, normpath};
use crate::pytables::{DECIMAL, DIGIT, ENTITIES};

pub use template::{Fields, Format, FormatKind, PLACEHOLDERS, TemplateError, make_template_fields};

/// Whether `c` is in a table of ranges (`pytables`).
fn in_ranges(table: &[(u32, u32)], c: char) -> bool {
    let c = c as u32;
    table
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

/// `str.isdecimal()` of a character (what `\d` matches in a Python `str` pattern).
pub fn is_decimal(c: char) -> bool {
    c.is_ascii_digit() || (c as u32 >= 0x660 && in_ranges(DECIMAL, c))
}

/// The value of a decimal digit (`int(c)`).
pub(crate) fn decimal_value(c: char) -> u32 {
    if c.is_ascii_digit() {
        return c as u32 - '0' as u32;
    }
    let cp = c as u32;
    let start = DECIMAL
        .iter()
        .find(|&&(a, b)| a <= cp && cp <= b)
        .map_or(cp, |&(a, _)| a);
    (cp - start) % 10
}

/// `str.isdigit()` of a character.
pub fn is_digit(c: char) -> bool {
    c.is_ascii_digit() || (c as u32 >= 0xb2 && in_ranges(DIGIT, c))
}

/// A text as ASCII: the transliteration of papers (`unidecode.unidecode`).
///
/// The ASCII characters are kept, the others go through the table. papers uses the
/// Unidecode table; this uses `deunicode`. They agree on the letters of the Latin
/// scripts and most others, not on all characters (`tests/expected/library/translit.json`
/// lists the differences). A character that the table does not know is dropped, as
/// Unidecode does.
///
/// ```
/// assert_eq!(papers_rs::names::ascii("Müller, Åsa, Straße"), "Muller, Asa, Strasse");
/// ```
pub fn ascii(text: &str) -> String {
    if text.is_ascii() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
        } else if let Some(t) = deunicode::deunicode_char(c) {
            out.push_str(t);
        }
    }
    out
}

// slugify
// -------

/// The replacement of the `'` of a text by `-` (`QUOTE_PATTERN.sub`): a run
/// of them becomes one.
fn quotes_to_dash(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_run = false;
    for c in text.chars() {
        if c == '\'' {
            if !in_run {
                out.push('-');
            }
            in_run = true;
        } else {
            in_run = false;
            out.push(c);
        }
    }
    out
}

/// The text of `&name;` of the HTML 4 entities, else none.
fn entity_char(name: &str) -> Option<char> {
    let i = ENTITIES.binary_search_by(|&(n, _)| n.cmp(name)).ok()?;
    char::from_u32(ENTITIES[i].1)
}

/// Replace the references `&...;` that `read` turns into a code point (the
/// replacement of a regular expression): all of them, or none if one is not
/// a character (`chr()` fails, the error is caught by slugify).
///
/// `prefix` is what the reference starts with after the `&`, `digit` what
/// its digits are.
fn replace_references(
    text: &str,
    prefix: &str,
    digit: impl Fn(char) -> Option<u32>,
    base: u32,
) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let prefix: Vec<char> = prefix.chars().collect();
    let mut i = 0;
    let mut failed = false;
    while i < chars.len() {
        if chars[i] == '&' && chars[i + 1..].starts_with(&prefix) {
            let mut j = i + 1 + prefix.len();
            let first = j;
            let mut value: u64 = 0;
            while j < chars.len() {
                match digit(chars[j]) {
                    Some(d) => {
                        value = (value * base as u64 + d as u64).min(u64::MAX >> 8);
                        j += 1;
                    }
                    None => break,
                }
            }
            if j > first && j < chars.len() && chars[j] == ';' {
                if value > 0x10ffff {
                    failed = true;
                } else {
                    // a surrogate is a character in Python; it is no letter or digit here either
                    out.push(char::from_u32(value as u32).unwrap_or('\u{fffd}'));
                }
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    if failed { text.to_string() } else { out }
}

fn replace_named_entities(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_ascii_alphanumeric() {
                j += 1;
            }
            if j > i + 1 && j < chars.len() && chars[j] == ';' {
                let name: String = chars[i + 1..j].iter().collect();
                if let Some(c) = entity_char(&name) {
                    out.push(c);
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// A text as a slug of `a-z`, `0-9` and `-` (python-slugify's `slugify` with
/// its defaults, as papers uses it, always with the transliteration table of
/// Unidecode).
///
/// In order: the `'` become `-`; NFKD and the transliteration to ASCII
/// ([`ascii`]), the `'` of the table dropped; the HTML entities, decimal and hexadecimal references are
/// decoded; NFKD again and lower case; the `'` that are left are removed, and
/// a comma between two digits; each run of other characters becomes `-`, runs
/// of `-` become one, and the `-` at the ends are removed.
///
/// ```
/// use papers_rs::names::slugify;
/// assert_eq!(slugify("Müller"), "muller");
/// assert_eq!(slugify("l'eau"), "l-eau");
/// assert_eq!(slugify("10.1000/xyz(1)"), "10-1000-xyz-1");
/// assert_eq!(slugify("1,000 &amp; more"), "1000-more");
/// ```
pub fn slugify(text: &str) -> String {
    let text = quotes_to_dash(text);
    let text: String = text.nfkd().collect();
    // an apostrophe that the table gives (as for Ь) is dropped, one of the text separated words
    let text: String = ascii(&text).chars().filter(|&c| c != '\'').collect();
    let text = replace_named_entities(&text);
    let text = replace_references(&text, "#", |c| is_decimal(c).then(|| decimal_value(c)), 10);
    let text = replace_references(
        &text,
        "#x",
        |c| match c {
            'a'..='f' => Some(c as u32 - 'a' as u32 + 10),
            'A'..='F' => Some(c as u32 - 'A' as u32 + 10),
            c => is_decimal(c).then(|| decimal_value(c)),
        },
        16,
    );
    let text: String = text.nfkd().collect();
    let text = text.to_lowercase();
    let text: String = text.chars().filter(|&c| c != '\'').collect();
    // a comma between two digits is removed (the digits are those of the text before)
    let chars: Vec<char> = text.chars().collect();
    let mut cleaned = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ','
            && i > 0
            && i + 1 < chars.len()
            && is_decimal(chars[i - 1])
            && is_decimal(chars[i + 1])
        {
            continue;
        }
        cleaned.push(c);
    }
    // each run of other characters (and of `-`) becomes one `-`
    let mut out = String::with_capacity(cleaned.len());
    let mut last_dash = false;
    for c in cleaned.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            last_dash = false;
        } else {
            if !last_dash {
                out.push('-');
            }
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// A value of a field for a path: without separators (`/`, `\` and NUL
/// become `-`), and not made of dots only (`papers.filename.path_safe`).
///
/// ```
/// use papers_rs::names::path_safe;
/// assert_eq!(path_safe("a/b\\c"), "a-b-c");
/// assert_eq!(path_safe(".."), "--");
/// ```
pub fn path_safe(value: &str) -> String {
    let value: String = value
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '\0') {
                '-'
            } else {
                c
            }
        })
        .collect();
    if !value.is_empty() && value.chars().all(|c| c == '.') {
        return "-".repeat(value.chars().count());
    }
    value
}

/// The longest name (in bytes, as UTF-8) of a file or folder that papers names
/// after an entry: 255 is the limit of most file systems, minus room for the
/// `-2`, `-3`... suffix of a name already taken.
pub const MAX_NAME_BYTES: usize = 250;

/// The start of `text` that is at most `size` bytes as UTF-8 (`cut_bytes`).
pub fn cut_bytes(text: &str, size: usize) -> &str {
    let mut end = size.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The names that Windows reserves for devices.
fn is_reserved(name: &str) -> bool {
    let name = name.to_uppercase();
    matches!(name.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (name.len() == 4
            && (name.starts_with("COM") || name.starts_with("LPT"))
            && matches!(name.as_bytes()[3], b'1'..=b'9'))
}

/// A file name made from a template, valid on every system (`safe_file_name`):
/// in each folder of it, the characters that Windows forbids (`< > : " | ? *`
/// and the control characters) replaced with `-`, no trailing dot or space, `-`
/// after a reserved name of Windows (`CON`, `NUL`...), and at most
/// [`MAX_NAME_BYTES`] bytes.
///
/// ```
/// use papers_rs::names::safe_file_name;
/// assert_eq!(safe_file_name("2020/what: a?"), "2020/what- a-");
/// assert_eq!(safe_file_name("nul.pdf"), "nul-.pdf");
/// ```
pub fn safe_file_name(name: &str) -> String {
    let parts: Vec<String> = name
        .split('/')
        .map(|part| {
            if matches!(part, "" | "." | "..") {
                return part.to_string();
            }
            let part: String = part
                .chars()
                .map(|c| match c {
                    '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\0'..='\u{1f}' => '-',
                    c => c,
                })
                .collect();
            let part = cut_bytes(&part, MAX_NAME_BYTES).trim_end_matches(['.', ' ']);
            let part = if part.is_empty() { "-" } else { part };
            let (first, rest) = part.split_once('.').map_or((part, ""), |(f, r)| (f, r));
            if is_reserved(first) {
                let dot = if part.contains('.') { "." } else { "" };
                return cut_bytes(&format!("{first}-{dot}{rest}"), MAX_NAME_BYTES).to_string();
            }
            part.to_string()
        })
        .collect();
    parts.join("/")
}

/// A path with its file name (extension included) cut to [`MAX_NAME_BYTES`]
/// bytes, keeping the extension (`fit_name`).
///
/// ```
/// use papers_rs::names::fit_name;
/// assert_eq!(fit_name("/lib/a.pdf"), "/lib/a.pdf");
/// assert_eq!(fit_name(&format!("/lib/{}.pdf", "x".repeat(300))), format!("/lib/{}.pdf", "x".repeat(246)));
/// ```
pub fn fit_name(path: &str) -> String {
    let head = dirname(path);
    let name = basename(path);
    if name.len() <= MAX_NAME_BYTES {
        return path.to_string();
    }
    let (stem, ext) = splitext(&name);
    // (an extension of more than MAX_NAME_BYTES leaves a negative size, which cuts from the end, as Python's slice does)
    let room = MAX_NAME_BYTES as i64 - ext.len() as i64;
    let size = if room >= 0 {
        room as usize
    } else {
        stem.len().saturating_sub(room.unsigned_abs() as usize)
    };
    let stem = cut_bytes(stem, size).trim_end_matches(['.', ' ']);
    let stem = if stem.is_empty() { "-" } else { stem };
    join(&head, &format!("{stem}{ext}"))
}

/// `s[:n]` in characters, with Python's meaning of a negative `n`.
pub(crate) fn head_chars(s: &str, n: i64) -> String {
    let len = s.chars().count() as i64;
    let end = if n >= 0 { n.min(len) } else { (len + n).max(0) };
    s.chars().take(end as usize).collect()
}

/// `items[:n]` with Python's meaning of a negative `n`.
pub(crate) fn head<T>(items: &[T], n: i64) -> &[T] {
    let len = items.len() as i64;
    let end = if n >= 0 { n.min(len) } else { (len + n).max(0) };
    &items[..end as usize]
}

/// The words of a title that go to a name (`papers.filename.title_words`):
/// the first `n` of those of at least `min_word_len` characters, without the
/// last ones that make them longer than `max_length` once joined with `sep`
/// (a hard limit): one word is kept, cut to `max_length` if needed.
pub fn title_words(
    words: &[&str],
    max_length: i64,
    min_word_len: i64,
    n: i64,
    sep: &str,
) -> Vec<String> {
    let kept: Vec<&str> = words
        .iter()
        .copied()
        .filter(|w| w.chars().count() as i64 >= min_word_len)
        .collect();
    let mut words: Vec<String> = head(&kept, n).iter().map(|w| w.to_string()).collect();
    let joined_len = |words: &[String]| words.join(sep).chars().count() as i64;
    while words.len() > 1 && joined_len(&words) > max_length {
        words.pop();
    }
    if let Some(first) = words.first_mut()
        && first.chars().count() as i64 > max_length
    {
        *first = head_chars(first, max_length);
    }
    words
}

/// The words of [`title_words`] joined with `sep` (`listtag`).
pub fn list_tag(words: &[&str], max_length: i64, min_word_len: i64, n: i64, sep: &str) -> String {
    title_words(words, max_length, min_word_len, n, sep).join(sep)
}

/// `str.capitalize()` of an ASCII text.
pub(crate) fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c
            .to_uppercase()
            .chain(chars.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    }
}

// keys
// ----

/// The next free key of a key that is taken (`papers.bib.append_abc`): a
/// letter after a digit is a suffix of this function (`Author2000b`,
/// `Author2000B`), the next letters are tried in its case (`c`, `d`..., or
/// `C`, `D`...); otherwise `b` to `z` are appended. Then the key itself followed
/// by each of `a` to `z` (`Author2000za` after `z`). The keys are compared
/// without case. The first free one is returned.
///
/// ```
/// use papers_rs::names::append_abc;
/// assert_eq!(append_abc("Author2000", &[]).unwrap(), "Author2000b");
/// assert_eq!(append_abc("Author2000", &["author2000", "author2000b"]).unwrap(), "Author2000c");
/// assert_eq!(append_abc("Author2000B", &[]).unwrap(), "Author2000C");
/// assert_eq!(append_abc("Author2000z", &[]).unwrap(), "Author2000za");
/// assert_eq!(append_abc("Ab", &["ab"]).unwrap(), "Abb");
/// ```
pub fn append_abc(key: &str, keys: &[&str]) -> Result<String, String> {
    let taken: HashSet<String> = keys.iter().map(|k| k.to_lowercase()).collect();
    append_abc_in(key, |k| taken.contains(k))
}

/// [`append_abc`] with the set of the keys (in lower case) as a function.
pub(crate) fn append_abc_in(key: &str, taken: impl Fn(&str) -> bool) -> Result<String, String> {
    let chars: Vec<char> = key.chars().collect();
    let alphabet: Vec<char> = ('a'..='z').collect();
    // a letter after a digit is a suffix of this function, in either case
    let suffix = match chars.as_slice() {
        [.., prev, last] if is_digit(*prev) => {
            let lower: Vec<char> = last.to_lowercase().collect();
            match lower.as_slice() {
                [l] if l.is_ascii_lowercase() => Some((last, *l)),
                _ => None,
            }
        }
        _ => None,
    };
    let (base, letters): (String, Vec<char>) = match suffix {
        Some((last, l)) => {
            let next = alphabet.iter().skip_while(|&&a| a != l).skip(1);
            let letters = if last.is_uppercase() {
                next.map(char::to_ascii_uppercase).collect()
            } else {
                next.copied().collect()
            };
            (chars[..chars.len() - 1].iter().collect(), letters)
        }
        None => (key.to_string(), alphabet[1..].to_vec()), // start at b
    };
    // no letter left (after z): a second letter (Author2000za)
    let candidates = letters
        .iter()
        .map(|l| format!("{base}{l}"))
        .chain(alphabet.iter().map(|l| format!("{key}{l}")));
    for candidate in candidates {
        if !taken(&candidate.to_lowercase()) {
            return Ok(candidate);
        }
    }
    Err(format!("{key}: no free key with a letter suffix"))
}

/// A key is valid if it is not empty and does not start with a digit
/// (`papers.bib.isvalidkey`).
pub fn is_valid_key(key: &str) -> bool {
    key.chars().next().is_some_and(|c| !is_digit(c))
}

/// The characters that a key cannot have (`_KEY_FORBIDDEN`).
fn key_forbidden(c: char) -> bool {
    is_space(c)
        || matches!(
            c,
            ',' | '{' | '}' | '"' | '#' | '%' | '~' | '=' | '(' | ')' | '\\'
        )
}

/// Why `key` cannot be the key of an entry (`papers edit --key`): papers'
/// message, or none.
pub fn check_key(key: &str) -> Option<String> {
    if !is_valid_key(key) || key.chars().any(key_forbidden) {
        return Some(format!(
            "{} is not a valid key (no spaces, commas, braces, quotes, or #%~=()\\; not starting with a digit)",
            crate::config::json::py_repr_str(key)
        ));
    }
    None
}

// files
// -----

/// `os.path.splitext`: the path without its extension, and the extension
/// (with the dot; none for a name that starts with a dot and has no other).
pub fn splitext(path: &str) -> (&str, &str) {
    let sep = path.rfind('/');
    let Some(dot) = path.rfind('.') else {
        return (path, "");
    };
    if sep.is_some_and(|s| dot < s) {
        return (path, "");
    }
    let start = sep.map_or(0, |s| s + 1);
    if path[start..dot].chars().all(|c| c == '.') {
        return (path, "");
    }
    (&path[..dot], &path[dot..])
}

/// What separates the name of an entry from the label of its other files.
const ATTACHMENT_SEP: &str = "--";

/// The label of a file of an entry other than the first: its name without
/// extension, without a `<prefix>--` that a rename added.
pub fn attachment_label(file: &str, prefixes: &[&str]) -> String {
    let base = basename(file);
    let stem = splitext(&base).0;
    for prefix in prefixes {
        let head = format!("{prefix}{ATTACHMENT_SEP}");
        if !prefix.is_empty()
            && stem.starts_with(&head)
            && stem.chars().count() > head.chars().count()
        {
            return stem[head.len()..].to_string();
        }
    }
    stem.to_string()
}

/// The new paths of the files of an entry (`attachment_names`): `target.ext`
/// for the first, `target--label.ext` for the others, with `-2`, `-3`... after
/// the label if two coincide. `files` is not empty.
///
/// ```
/// use papers_rs::names::attachment_names;
/// let files = ["/x/a.pdf", "/x/notes.txt", "/x/notes.pdf"].map(String::from);
/// assert_eq!(
///     attachment_names(&files, "/lib/files/doe2020"),
///     ["/lib/files/doe2020.pdf", "/lib/files/doe2020--notes.txt", "/lib/files/doe2020--notes.pdf"]
/// );
/// ```
pub fn attachment_names(files: &[String], target: &str) -> Vec<String> {
    let mut new = vec![fit_name(&format!("{target}{}", splitext(&files[0]).1))];
    // the prefixes of a previous rename: the current name of the first file, the target name
    let first_base = basename(&files[0]);
    let target_base = basename(target);
    let prefixes = [splitext(&first_base).0, target_base.as_str()];
    let mut taken: HashSet<String> = HashSet::from([new[0].clone()]);
    for file in &files[1..] {
        let ext = splitext(file).1;
        let label = attachment_label(file, &prefixes);
        let mut newfile = fit_name(&format!("{target}{ATTACHMENT_SEP}{label}{ext}"));
        let stem = newfile[..newfile.len() - ext.len()].to_string();
        let mut i = 2;
        while taken.contains(&newfile) {
            newfile = format!("{stem}-{i}{ext}");
            i += 1;
        }
        taken.insert(newfile.clone());
        new.push(newfile);
    }
    new
}

/// Where a name goes in the files directory, or none if it points outside:
/// the directory (absolute) joined with the name, normalised lexically; it
/// must be strictly inside the directory (path safety, library.md).
///
/// ```
/// use papers_rs::names::target_in;
/// assert_eq!(target_in("/lib/files", "2020/doe").as_deref(), Some("/lib/files/2020/doe"));
/// assert_eq!(target_in("/lib/files", "../doe"), None);
/// assert_eq!(target_in("/lib/files", "."), None);
/// assert_eq!(target_in("/lib/files", "/tmp/doe"), None);
/// ```
pub fn target_in(filesdir: &str, name: &str) -> Option<String> {
    let filesdir = crate::config::paths::abspath(filesdir);
    let target = normpath(&join(&filesdir, name));
    let prefix = if filesdir.ends_with('/') {
        filesdir.clone()
    } else {
        format!("{filesdir}/")
    };
    (target != filesdir && target.starts_with(&prefix)).then_some(target)
}
