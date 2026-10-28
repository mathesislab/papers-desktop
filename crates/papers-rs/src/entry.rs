//! What papers knows of the fields of an entry: the `file` links, the
//! `keywords` (tags, nested with `/`), the `ids` (former keys) and `crossref`
//! (`papers.entries`, `papers.bib.Biblio`, `papers.cli.entry_cmds`).
//!
//! The entries are those of [`crate::bib`]; these are functions on them.
//! The links of the `file` field are relative to a directory (the one of the
//! bibtex) or, with `/`, absolute.
//!
//! ```
//! use papers_rs::bib::Entry;
//! use papers_rs::entry::{add_tags, files, set_files, tags};
//!
//! let mut e = Entry::new("article", "x");
//! set_files(&mut e, &["/lib/files/x.pdf".to_string()], "/lib");
//! assert_eq!(e.get("file"), Some(":files/x.pdf:pdf"));
//! assert_eq!(files(&e, Some("/lib")), ["/lib/files/x.pdf"]);
//! add_tags(&mut e, &["climate/amoc".to_string(), "ice".to_string()]);
//! assert_eq!(tags(&e), ["climate/amoc", "ice"]);
//! ```

use crate::bib::Entry;
use crate::bib::encoding::{format_file, parse_file, parse_keywords};
use crate::bib::pystr::strip;
use crate::config::paths::{abspath, join, normpath, relpath};

/// The value of a field, empty if there is none (`get_entry_val(e, name, '')`).
pub fn val<'a>(entry: &'a Entry, name: &str) -> &'a str {
    entry.get(name).unwrap_or("")
}

// the file field
// --------------

/// The paths of the links of the `file` field (`parse_file`); made absolute
/// against `relative_to` if given (`os.path.abspath(os.path.join(relative_to, f))`).
pub fn files(entry: &Entry, relative_to: Option<&str>) -> Vec<String> {
    let paths = parse_file(val(entry, "file"));
    match relative_to {
        Some(base) => paths.iter().map(|f| abspath(&join(base, f))).collect(),
        None => paths,
    }
}

/// The `file` field of some paths (`format_file(files, relative_to)`): with
/// `relative_to` the directory of the bibtex, the paths relative to it and
/// normalised; with `/`, absolute; with none, as they are.
pub fn format_files(paths: &[String], relative_to: Option<&str>) -> String {
    let paths: Vec<String> = match relative_to {
        Some("/") => paths.iter().map(|p| abspath(p)).collect(),
        Some(base) => paths.iter().map(|p| normpath(&relpath(p, base))).collect(),
        None => paths.to_vec(),
    };
    format_file(&paths)
}

/// Give an entry these links (`Biblio.set_files`): a path given twice is kept
/// once, at its first place; with no link left, the field is removed.
pub fn set_files(entry: &mut Entry, paths: &[String], relative_to: &str) {
    let mut unique: Vec<String> = Vec::new();
    for p in paths {
        if !unique.contains(p) {
            unique.push(p.clone());
        }
    }
    let field = format_files(&unique, Some(relative_to));
    if strip(&field).is_empty() {
        entry.remove("file");
    } else {
        entry.set("file", field);
    }
}

/// The links of an entry written relative to another directory
/// (`papers.entries.update_file_path`): the field before and after, if it changed.
pub fn update_file_path(entry: &mut Entry, from: &str, to: &str) -> Option<(String, String)> {
    let old = entry.get("file")?.to_string();
    let abs = files(entry, Some(from));
    let new = format_files(&abs, Some(to));
    entry.set("file", new.clone());
    (old != new).then_some((old, new))
}

// tags
// ----

/// The tags of an entry: its `keywords` split at the commas (`parse_keywords`).
pub fn tags(entry: &Entry) -> Vec<String> {
    parse_keywords(val(entry, "keywords"))
}

/// The tags above a nested tag: `climate/amoc/tipping` gives `climate`, `climate/amoc`.
pub fn tag_parents(tag: &str) -> Vec<String> {
    let parts: Vec<&str> = tag.split('/').collect();
    (1..parts.len())
        .filter(|&i| !parts[i - 1].is_empty())
        .map(|i| parts[..i].join("/"))
        .collect()
}

/// `fnmatch.fnmatchcase`: `*`, `?` and `[seq]` (`[!seq]`) against the whole name.
pub fn fnmatch(name: &str, pattern: &str) -> bool {
    enum Tok {
        Star,
        Any,
        Lit(char),
        Set(bool, Vec<(char, char)>),
    }
    let p: Vec<char> = pattern.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < p.len() {
        let c = p[i];
        i += 1;
        match c {
            '*' => {
                if !matches!(toks.last(), Some(Tok::Star)) {
                    toks.push(Tok::Star);
                }
            }
            '?' => toks.push(Tok::Any),
            '[' => {
                let mut j = i;
                if j < p.len() && p[j] == '!' {
                    j += 1;
                }
                if j < p.len() && p[j] == ']' {
                    j += 1;
                }
                while j < p.len() && p[j] != ']' {
                    j += 1;
                }
                if j >= p.len() {
                    toks.push(Tok::Lit('['));
                    continue;
                }
                let mut stuff = &p[i..j];
                let negated = stuff.first() == Some(&'!');
                if negated {
                    stuff = &stuff[1..];
                }
                let mut items = Vec::new();
                let mut k = 0;
                while k < stuff.len() {
                    if k + 2 < stuff.len() && stuff[k + 1] == '-' {
                        if stuff[k] <= stuff[k + 2] {
                            items.push((stuff[k], stuff[k + 2]));
                        }
                        k += 3;
                    } else {
                        items.push((stuff[k], stuff[k]));
                        k += 1;
                    }
                }
                toks.push(Tok::Set(negated, items));
                i = j + 1;
            }
            c => toks.push(Tok::Lit(c)),
        }
    }
    let n: Vec<char> = name.chars().collect();
    let matches_one = |t: &Tok, c: char| match t {
        Tok::Any => true,
        Tok::Lit(l) => *l == c,
        Tok::Set(neg, items) => items.iter().any(|&(a, b)| a <= c && c <= b) != *neg,
        Tok::Star => false,
    };
    // the classic matcher with one point to go back to, at the last star
    let (mut t, mut s) = (0, 0);
    let mut back: Option<(usize, usize)> = None;
    while s < n.len() {
        if t < toks.len() && matches!(toks[t], Tok::Star) {
            back = Some((t, s));
            t += 1;
        } else if t < toks.len() && matches_one(&toks[t], n[s]) {
            t += 1;
            s += 1;
        } else if let Some((bt, bs)) = back {
            back = Some((bt, bs + 1));
            t = bt + 1;
            s = bs + 1;
        } else {
            return false;
        }
    }
    while t < toks.len() && matches!(toks[t], Tok::Star) {
        t += 1;
    }
    t == toks.len()
}

/// `--tag PATTERN`: the whole tag, whatever its case, with `*` patterns; a
/// tag also finds the tags under it (`climate` finds `climate/amoc`).
///
/// ```
/// use papers_rs::entry::tag_matches;
/// assert!(tag_matches("Climate/AMOC", "climate"));
/// assert!(tag_matches("ice-review", "ice*"));
/// assert!(!tag_matches("ice-review", "ice"));
/// ```
pub fn tag_matches(tag: &str, pattern: &str) -> bool {
    let tag = tag.to_lowercase();
    let pattern = pattern.to_lowercase();
    let pattern = pattern.trim_end_matches('/');
    fnmatch(&tag, pattern) || fnmatch(&tag, &format!("{pattern}/*"))
}

/// How many entries each tag has, a tag above nested tags counting the
/// entries under it (`tag_counts`), in the order they are first met.
pub fn tag_counts<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for e in entries {
        let mut seen: Vec<String> = Vec::new();
        for tag in tags(e) {
            for t in tag_parents(&tag).into_iter().chain([tag]) {
                if !seen.contains(&t) {
                    seen.push(t);
                }
            }
        }
        for t in seen {
            match counts.iter_mut().find(|(n, _)| *n == t) {
                Some((_, c)) => *c += 1,
                None => counts.push((t, 1)),
            }
        }
    }
    counts
}

/// Write the tags in the `keywords` field (comma and space), removing it when none is left.
pub fn set_tags(entry: &mut Entry, tags: &[String]) {
    if tags.is_empty() {
        entry.remove("keywords");
    } else {
        entry.set("keywords", tags.join(", "));
    }
}

/// Add tags after the existing ones, those that are there already and the
/// repeats skipped (case-sensitive); true if the tags changed.
pub fn add_tags(entry: &mut Entry, new: &[String]) -> bool {
    let old = tags(entry);
    let mut all = old.clone();
    for t in new {
        if !all.contains(t) {
            all.push(t.clone());
        }
    }
    if all == old {
        return false;
    }
    set_tags(entry, &all);
    true
}

/// Remove the tags equal to one given (case-sensitive); true if the tags changed.
pub fn remove_tags(entry: &mut Entry, gone: &[String]) -> bool {
    let old = tags(entry);
    let kept: Vec<String> = old.iter().filter(|t| !gone.contains(t)).cloned().collect();
    if kept == old {
        return false;
    }
    set_tags(entry, &kept);
    true
}

/// Rename a tag and the tags under it (`climate/amoc` of `climate` renamed
/// `ocean` is `ocean/amoc`), the repeats removed, the first kept; true if the
/// tags changed.
pub fn rename_tag(entry: &mut Entry, old: &str, new: &str) -> bool {
    let rename = |t: &str| -> String {
        if t == old {
            new.to_string()
        } else if let Some(rest) = t.strip_prefix(&format!("{old}/")) {
            format!("{new}/{rest}")
        } else {
            t.to_string()
        }
    };
    let tags = tags(entry);
    if tags.iter().all(|t| rename(t) == *t) {
        return false;
    }
    let mut renamed: Vec<String> = Vec::new();
    for t in tags.iter().map(|t| rename(t)) {
        if !renamed.contains(&t) {
            renamed.push(t);
        }
    }
    set_tags(entry, &renamed);
    true
}

// former keys
// -----------

/// The former keys of an entry: its `ids` field split at the commas (and stripped, the empty ones dropped).
pub fn ids(entry: &Entry) -> Vec<String> {
    val(entry, "ids")
        .split(',')
        .map(strip)
        .filter(|i| !i.is_empty())
        .map(str::to_string)
        .collect()
}

/// Give an entry the key `new`; with `keep_old`, its former key goes to the
/// end of its `ids` (a part equal to the new key, without case, is removed).
/// No check of the key (see [`crate::names::check_key`], and the library for
/// the keys that are taken).
pub fn set_key(entry: &mut Entry, new: &str, keep_old: bool) {
    let old = std::mem::replace(&mut entry.key, new.to_string());
    if keep_old {
        let mut parts: Vec<String> = ids(entry)
            .into_iter()
            .filter(|i| i.to_lowercase() != new.to_lowercase())
            .collect();
        parts.push(old);
        entry.set("ids", parts.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn nested_tags() {
        assert_eq!(
            tag_parents("climate/amoc/tipping"),
            ["climate", "climate/amoc"]
        );
        assert!(tag_matches("climate/amoc", "Climate/"));
        assert!(!tag_matches("climatology", "climate"));
        assert!(tag_matches("a-b", "a-[a-c]"));
        assert!(tag_matches("ab", "a[!c]"));
        assert!(!tag_matches("ac", "a[!c]"));
        let mut e = Entry::new("article", "x");
        add_tags(&mut e, &s(&["a/b", "c", "c"]));
        assert_eq!(e.get("keywords"), Some("a/b, c"));
        assert!(rename_tag(&mut e, "a", "z"));
        assert_eq!(e.get("keywords"), Some("z/b, c"));
        assert!(remove_tags(&mut e, &s(&["z/b", "c"])));
        assert_eq!(e.get("keywords"), None);
    }

    #[test]
    fn counting() {
        let mut a = Entry::new("article", "a");
        let mut b = Entry::new("article", "b");
        add_tags(&mut a, &s(&["x/y", "x/z"]));
        add_tags(&mut b, &s(&["x"]));
        assert_eq!(
            tag_counts([&a, &b]),
            [
                ("x".to_string(), 2),
                ("x/y".to_string(), 1),
                ("x/z".to_string(), 1)
            ]
        );
    }

    #[test]
    fn keys() {
        let mut e = Entry::new("article", "old");
        e.set("ids", "a, New");
        set_key(&mut e, "new", true);
        assert_eq!((e.key.as_str(), e.get("ids")), ("new", Some("a, old")));
    }
}
