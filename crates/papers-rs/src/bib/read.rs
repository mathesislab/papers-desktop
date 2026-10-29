//! The parse stack of bibtexparser v2 (`ResolveStringReferencesMiddleware`,
//! then `RemoveEnclosingMiddleware`) and the lower-casing of the field names
//! that `papers.entries.parse_string` adds.

use std::collections::{BTreeSet, HashMap, HashSet};

use super::pystr::{is_digit, strip};
use super::{Block, Entry, Field, Unreadable, UnreadableKind};

/// Run the middlewares on the blocks of the splitter.
pub(super) fn transform(blocks: &mut [Block]) {
    resolve_string_references(blocks);
    let mut keys: HashSet<String> = HashSet::new();
    for block in blocks.iter_mut() {
        match block {
            Block::Entry(entry) => {
                for field in &mut entry.fields {
                    strip_field(field);
                }
                if let Some(unreadable) = case_duplicate(entry, &mut keys) {
                    *block = Block::Unreadable(unreadable);
                    continue;
                }
                for field in &mut entry.fields {
                    if field.key != field.key.to_lowercase() {
                        field.key = field.key.to_lowercase();
                    }
                }
            }
            Block::String(string) => {
                let (value, verbatim) = strip_enclosing(&string.value);
                string.verbatim = verbatim && !is_digit(&value);
                string.value = value;
            }
            _ => {}
        }
    }
}

/// BibTeX ignores the case of the names of the fields and of the keys: an entry
/// with two fields that differ only by case (`Title` and `title`), or with the
/// key of an entry before it in another case (`smith2000` after `Smith2000`), is
/// a block that could not be read, as an entry with a field or a key given
/// exactly twice is (`papers.entries.lowercase_fields`). `keys` holds the keys
/// of the entries kept, in lower case.
fn case_duplicate(entry: &Entry, keys: &mut HashSet<String>) -> Option<Unreadable> {
    let names: Vec<String> = entry.fields.iter().map(|f| f.key.to_lowercase()).collect();
    let twice: BTreeSet<&String> = names
        .iter()
        .filter(|n| names.iter().filter(|m| m == n).count() > 1)
        .collect();
    let (raw, line) = entry
        .source
        .as_ref()
        .map_or((String::new(), 0), |s| (s.raw.clone(), s.line));
    if !twice.is_empty() {
        let twice = twice.into_iter().cloned().collect();
        return Some(Unreadable {
            raw,
            line,
            kind: UnreadableKind::DuplicateFields(twice),
        });
    }
    if !keys.insert(entry.key.to_lowercase()) {
        return Some(Unreadable {
            raw,
            line,
            kind: UnreadableKind::DuplicateKey(entry.key.clone()),
        });
    }
    None
}

/// A value that is the key of a string (case-insensitive) is replaced by the
/// value of that string, as written (still enclosed): the last definition wins.
fn resolve_string_references(blocks: &mut [Block]) {
    let mut values: HashMap<String, String> = HashMap::new();
    for block in blocks.iter() {
        if let Block::String(string) = block {
            values.insert(string.key.to_lowercase(), string.value.clone());
        }
    }
    if values.is_empty() {
        return;
    }
    for block in blocks.iter_mut() {
        if let Block::Entry(entry) = block {
            for field in &mut entry.fields {
                let v = &field.value;
                let enclosed = (v.starts_with('"') && v.ends_with('"'))
                    || (v.starts_with('{') && v.ends_with('}'));
                if enclosed {
                    continue;
                }
                if let Some(resolved) = values.get(&v.to_lowercase()) {
                    field.value = resolved.clone();
                }
            }
        }
    }
}

fn strip_field(field: &mut Field) {
    let (value, verbatim) = strip_enclosing(&field.value);
    // string references and concatenations must stay unenclosed
    field.verbatim = verbatim && !is_digit(&value);
    field.value = value;
}

/// The value without its enclosing braces or quotes, and whether it had none
/// (`no-enclosing`: a number, a string reference, a concatenation).
fn strip_enclosing(value: &str) -> (String, bool) {
    let value = strip(value);
    if value.chars().count() >= 2 {
        if value.starts_with('{') && value.ends_with('}') && is_enclosed_in_braces(value) {
            return (value[1..value.len() - 1].to_string(), false);
        }
        if value.starts_with('"') && value.ends_with('"') && is_enclosed_in_quotes(value) {
            return (value[1..value.len() - 1].to_string(), false);
        }
    }
    (value.to_string(), true)
}

/// The unescaped (not after a backslash) occurrences of the bytes of `set`.
fn unescaped<'a>(value: &'a str, set: &'a [u8]) -> impl Iterator<Item = (usize, u8)> + 'a {
    let bytes = value.as_bytes();
    (0..bytes.len()).filter_map(move |i| {
        if set.contains(&bytes[i]) && !(i > 0 && bytes[i - 1] == b'\\') {
            Some((i, bytes[i]))
        } else {
            None
        }
    })
}

/// Whether the brace at the start is the one closed at the end.
fn is_enclosed_in_braces(value: &str) -> bool {
    let inner = &value[1..value.len() - 1];
    if !inner.contains('{') && !inner.contains('}') {
        return !inner.ends_with('\\');
    }
    let last = value.len() - 1;
    let mut depth = 0i64;
    for (i, c) in unescaped(value, b"{}") {
        if c == b'{' {
            depth += 1;
            continue;
        }
        depth -= 1;
        if depth < 0 {
            return false;
        }
        if depth == 0 && i != last {
            return false;
        }
    }
    depth == 0
}

/// Whether the quote at the start is the one closed at the end (not those
/// in braces or escaped).
fn is_enclosed_in_quotes(value: &str) -> bool {
    let last = value.len() - 1;
    if !value[1..last].contains('"') {
        return true;
    }
    let mut depth = 0i64;
    for (i, c) in unescaped(value, b"{}\"") {
        if i == 0 || i == last {
            continue;
        }
        match c {
            b'{' => depth += 1,
            b'}' => depth = (depth - 1).max(0),
            _ if depth == 0 => return false,
            _ => {}
        }
    }
    true
}

/// Whether a value written as it is parses back to the same value
/// (`_is_writable_unenclosed`).
pub(super) fn is_writable_unenclosed(value: &str) -> bool {
    let mut depth = 0i64;
    let mut in_quotes = false;
    for (_, c) in unescaped(value, b"{}\",=\n") {
        match c {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ if depth == 0 => {
                if c == b'"' {
                    in_quotes = !in_quotes;
                } else if !in_quotes {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0 && !in_quotes
}
