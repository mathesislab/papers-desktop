//! Write the .bib files of the library: only the entries changed are
//! rewritten, the rest of a file stays byte for byte. A file is written to a
//! temporary file then renamed over it, and not at all if it changed since
//! it was read. The library is not changed: it is loaded again from the
//! files.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use biblatex::{Pair, RawBibliography, RawEntry};
use serde::Serialize;

use crate::bib::{self, BibEntry, Snapshot};
use crate::item::Item;
use crate::search::Library;
use crate::{Error, Result};

/// A .bib file written, to undo or redo. The texts before and after are
/// kept where they differ.
#[derive(Debug)]
pub struct Change {
    bib: PathBuf,
    /// the bytes both texts start with
    start: usize,
    /// the bytes both texts end with
    end: usize,
    before: Vec<u8>,
    after: Vec<u8>,
    /// of the whole texts, before and after
    snapshots: (Snapshot, Snapshot),
}

impl Change {
    fn new(bib: PathBuf, before: &[u8], after: &[u8]) -> Change {
        let start = before.iter().zip(after).take_while(|(a, b)| a == b).count();
        let (b, a) = (&before[start..], &after[start..]);
        let end = b
            .iter()
            .rev()
            .zip(a.iter().rev())
            .take_while(|(x, y)| x == y)
            .count();
        Change {
            bib,
            start,
            end,
            before: b[..b.len() - end].to_vec(),
            after: a[..a.len() - end].to_vec(),
            snapshots: (Snapshot::of(before), Snapshot::of(after)),
        }
    }

    pub fn bib(&self) -> &Path {
        &self.bib
    }

    /// Put the file back as it was before the change, unless it changed since.
    pub fn undo(&self) -> Result<()> {
        self.restore(self.snapshots.1, &self.before)
    }

    /// Make the change again, unless the file changed since it was undone.
    pub fn redo(&self) -> Result<()> {
        self.restore(self.snapshots.0, &self.after)
    }

    fn restore(&self, now: Snapshot, middle: &[u8]) -> Result<()> {
        let current = fs::read(&self.bib).map_err(|source| Error::Read {
            path: self.bib.clone(),
            source,
        })?;
        if Snapshot::of(&current) != now {
            return Err(Error::Changed {
                path: self.bib.clone(),
            });
        }
        let rest = &current[current.len() - self.end..];
        write_file(&self.bib, &[&current[..self.start], middle, rest].concat())
    }
}

/// The keys of the entries given to add: added, and existing (already in
/// the library).
#[derive(Debug, Default, Serialize)]
pub struct Added {
    pub added: Vec<String>,
    pub existing: Vec<String>,
}

/// The new text of a .bib file, to write.
struct Rewrite {
    bib: PathBuf,
    before: String,
    after: String,
}

impl Library {
    /// Replace the text of an entry by `text`, which must be one entry with
    /// the same key, tags and files (see `check_edit`).
    pub fn replace_entry(&self, id: &str, text: &str) -> Result<Vec<Change>> {
        write(vec![self.edited(id, text)?])
    }

    /// Check an edit of the text of an entry, as `replace_entry` does,
    /// without writing it. An edit keeps the key, the tags and the files, as
    /// `papers edit --from` does: tags and files have their own commands.
    pub fn check_edit(&self, id: &str, text: &str) -> Result<()> {
        self.edited(id, text).map(|_| ())
    }

    /// The .bib file of an entry with its text replaced by `text`.
    fn edited(&self, id: &str, text: &str) -> Result<Rewrite> {
        let e = self.entry(id).ok_or_else(|| not_found(id))?;
        let text = text.trim();
        let given = given_entries(text)?;
        if given.len() != 1 {
            return Err(Error::NotOneEntry { found: given.len() });
        }
        let before = self.text(&e.bib)?;
        let after = splice(&before, vec![(e.span.clone(), text.to_string())]);
        let entries = check(&e.bib, &after, Some((e.span.start, text)))?;
        let new = entries
            .iter()
            .find(|n| n.span.start == e.span.start)
            .ok_or_else(|| not_found(id))?;
        let tags = |e: &BibEntry| Item::from_entry(e, String::new()).keywords;
        let kept = [
            ("key", new.entry.key == e.entry.key),
            ("tags", tags(new) == tags(e)),
            ("file", new.files == e.files),
        ];
        if let Some((what, _)) = kept.into_iter().find(|(_, same)| !same) {
            return Err(Error::Kept { what });
        }
        Ok(Rewrite {
            bib: e.bib.clone(),
            before,
            after,
        })
    }

    /// Link files to an entry, as they are (not moved or renamed): their links come after those
    /// the `file` field has. With `text`, the entry is first replaced by it, as `replace_entry`
    /// does (the key, tags and files stay), in the same write.
    pub fn attach_files(
        &self,
        id: &str,
        text: Option<&str>,
        files: &[PathBuf],
    ) -> Result<Vec<Change>> {
        let e = self.entry(id).ok_or_else(|| not_found(id))?;
        if let Some(text) = text {
            self.check_edit(id, text)?;
        }
        let entry = text.map_or(e.raw.as_str(), str::trim);
        let new = splice(entry, vec![files_edit(&e.bib, entry, files)?]);
        let before = self.text(&e.bib)?;
        let after = splice(&before, vec![(e.span.clone(), new.clone())]);
        check(&e.bib, &after, Some((e.span.start, &new)))?;
        write(vec![Rewrite {
            bib: e.bib.clone(),
            before,
            after,
        }])
    }

    /// Set the tags of items: for each (id, tags), the `keywords` field
    /// rewritten, or removed when no tag is left, in the style of the
    /// entry's other fields. One write per file.
    pub fn set_tags(&self, changes: &[(String, Vec<String>)]) -> Result<Vec<Change>> {
        let mut edits: BTreeMap<&Path, Vec<Edit>> = BTreeMap::new();
        for (id, tags) in changes {
            let (e, item) = (self.entry(id), self.get(id));
            let (Some(e), Some(item)) = (e, item) else {
                return Err(not_found(id));
            };
            let tags: Vec<&str> = tags
                .iter()
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .collect();
            if let Some(tag) = tags.iter().find(|t| !balanced(t)) {
                return Err(Error::Tag {
                    tag: tag.to_string(),
                });
            }
            if item.keywords == tags {
                continue;
            }
            let (range, text) = keywords_edit(&e.bib, &e.raw, &tags)?;
            let at = e.span.start;
            edits
                .entry(e.bib.as_path())
                .or_default()
                .push((range.start + at..range.end + at, text));
        }
        let rewrites = edits
            .into_iter()
            .map(|(bib, edits)| {
                let before = self.text(bib)?;
                let after = splice(&before, edits);
                check(bib, &after, None)?;
                Ok(Rewrite {
                    bib: bib.to_path_buf(),
                    before,
                    after,
                })
            })
            .collect::<Result<_>>()?;
        write(rewrites)
    }

    /// Add the entries of `text` at the end of .bib file `bib`, after a blank
    /// line, but those whose key is in the file already: these are existing.
    pub fn add_entries(&self, bib: &Path, text: &str) -> Result<(Vec<Change>, Added)> {
        let before = self.text(bib)?;
        let text = text.trim();
        let given = given_entries(text)?;
        if given.is_empty() {
            return Err(Error::NoEntry);
        }
        let in_file: HashSet<String> = self
            .entries_in(bib)
            .map(|e| e.entry.key.to_ascii_lowercase())
            .collect();
        let mut keys = HashSet::new();
        if let Some((key, _)) = given
            .iter()
            .find(|(key, _)| !keys.insert(key.to_ascii_lowercase()))
        {
            return Err(Error::DuplicateKey {
                path: bib.to_path_buf(),
                key: key.clone(),
            });
        }
        let (existing, new): (Vec<_>, Vec<_>) = given
            .into_iter()
            .partition(|(key, _)| in_file.contains(&key.to_ascii_lowercase()));
        let added = Added {
            added: new.iter().map(|(key, _)| key.clone()).collect(),
            existing: existing.into_iter().map(|(key, _)| key).collect(),
        };
        if new.is_empty() {
            return Ok((Vec::new(), added));
        }
        let nl = newline(&before);
        let blank = format!("{nl}{nl}");
        let gap = if before.trim().is_empty() || before.ends_with(&blank) {
            ""
        } else if before.ends_with(nl) {
            nl
        } else {
            &blank
        };
        let text = new
            .into_iter()
            .map(|(_, span)| &text[span])
            .collect::<Vec<_>>()
            .join(&blank);
        let at = before.len() + gap.len();
        let after = format!("{before}{gap}{text}{nl}");
        check(bib, &after, Some((at, &text)))?;
        let changes = write(vec![Rewrite {
            bib: bib.to_path_buf(),
            before,
            after,
        }])?;
        Ok((changes, added))
    }

    /// Remove entries, and the blank line after each.
    pub fn remove_entries(&self, ids: &[String]) -> Result<Vec<Change>> {
        let mut spans: BTreeMap<&Path, Vec<Range<usize>>> = BTreeMap::new();
        for id in ids {
            let e = self.entry(id).ok_or_else(|| not_found(id))?;
            spans
                .entry(e.bib.as_path())
                .or_default()
                .push(e.span.clone());
        }
        let rewrites = spans
            .into_iter()
            .map(|(bib, spans)| {
                let before = self.text(bib)?;
                let removed = removals(&before, spans);
                let after = splice(
                    &before,
                    removed.into_iter().map(|r| (r, String::new())).collect(),
                );
                check(bib, &after, None)?;
                Ok(Rewrite {
                    bib: bib.to_path_buf(),
                    before,
                    after,
                })
            })
            .collect::<Result<_>>()?;
        write(rewrites)
    }

    /// The text of a .bib file of the library, unless it changed since it
    /// was read.
    fn text(&self, bib: &Path) -> Result<String> {
        let path = || bib.to_path_buf();
        let snapshot = self
            .snapshot(bib)
            .ok_or_else(|| Error::NotInLibrary { path: path() })?;
        let read_error = |source| Error::Read {
            path: path(),
            source,
        };
        let bytes = fs::read(bib).map_err(read_error)?;
        if Snapshot::of(&bytes) != snapshot {
            return Err(Error::Changed { path: path() });
        }
        String::from_utf8(bytes)
            .map_err(|e| read_error(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))
    }
}

/// The keys of the entries of a BibTeX text to add: it must parse, with
/// nothing but white space and comments outside its entries.
pub fn entry_keys(text: &str) -> Result<Vec<String>> {
    let given = given_entries(text.trim())?;
    if given.is_empty() {
        return Err(Error::NoEntry);
    }
    Ok(given.into_iter().map(|(key, _)| key).collect())
}

fn not_found(id: &str) -> Error {
    Error::NotFound { id: id.to_string() }
}

/// The entries of a BibTeX text given to write, with their spans: it must
/// parse, with nothing but white space and comments outside its entries.
pub(crate) fn given_entries(text: &str) -> Result<Vec<(String, Range<usize>)>> {
    let raw = RawBibliography::parse(text).map_err(|e| Error::Bibtex {
        line: line_of(text, e.span.start.min(text.len())),
        message: e.kind.to_string(),
    })?;
    let found: Vec<(String, Range<usize>)> = raw
        .entries
        .iter()
        .map(|e| (e.v.key.v.to_string(), bib::entry_span(text, e.span.clone())))
        .collect();
    // between the entries
    let mut gaps: Vec<Range<usize>> = Vec::new();
    let mut at = 0;
    for (_, span) in &found {
        gaps.push(at..span.start);
        at = span.end;
    }
    gaps.push(at..text.len());
    for gap in gaps {
        let mut at = gap.start;
        for line in text[gap].split_inclusive('\n') {
            let t = line.trim();
            if !t.is_empty() && !t.starts_with('%') {
                return Err(Error::Bibtex {
                    line: line_of(text, at),
                    message: format!("not in an entry: {t}"),
                });
            }
            at += line.len();
        }
    }
    Ok(found)
}

/// The new text of a .bib file must load, as the library reads it: its
/// entries. An error in the text given to write, at `given.0` in the new
/// text, tells its line in that text.
fn check(bib: &Path, text: &str, given: Option<(usize, &str)>) -> Result<Vec<BibEntry>> {
    let (at, message) = match bib::parse(bib, text) {
        Ok(entries) => return Ok(entries),
        Err(e) => e,
    };
    match given {
        Some((start, given)) if (start..start + given.len()).contains(&at) => Err(Error::Bibtex {
            line: line_of(given, at - start),
            message,
        }),
        _ => Err(Error::Parse {
            path: bib.to_path_buf(),
            line: line_of(text, at.min(text.len())),
            message,
        }),
    }
}

/// Write the new texts that differ from the old ones.
fn write(rewrites: Vec<Rewrite>) -> Result<Vec<Change>> {
    let mut changes = Vec::new();
    for r in rewrites.into_iter().filter(|r| r.after != r.before) {
        write_file(&r.bib, r.after.as_bytes())?;
        changes.push(Change::new(r.bib, r.before.as_bytes(), r.after.as_bytes()));
    }
    Ok(changes)
}

/// Write a file through a temporary file in its folder, renamed over it: a
/// reader finds the old text or the new, never a part. The permissions stay;
/// a link stays, its target is written. A read-only file is not written.
fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let error = |source| Error::Write {
        path: path.to_path_buf(),
        source,
    };
    let target = fs::canonicalize(path).map_err(error)?;
    let permissions = fs::metadata(&target).map_err(error)?.permissions();
    if permissions.readonly() {
        return Err(error(std::io::ErrorKind::PermissionDenied.into()));
    }
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let temporary = target.with_file_name(format!(".{name}.{}-{nanos}.tmp", std::process::id()));
    let written = File::create_new(&temporary).and_then(|mut file| {
        file.write_all(bytes)?;
        file.set_permissions(permissions)?;
        file.sync_all()?;
        fs::rename(&temporary, &target)
    });
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written.map_err(error)
}

/// A text with parts replaced, the ranges in order and apart.
type Edit = (Range<usize>, String);

fn splice(text: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|(r, _)| r.start);
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (range, new) in edits {
        out.push_str(&text[at..range.start]);
        out.push_str(&new);
        at = range.end;
    }
    out.push_str(&text[at..]);
    out
}

/// What removing entries removes: each entry, the rest of its line and the
/// blank line after it; at the end of the file, the blank lines before.
fn removals(text: &str, spans: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = spans
        .into_iter()
        .map(|span| {
            // the rest of the line, then a blank line
            let mut end = span.end;
            for _ in 0..2 {
                let Some(next) = line_end(text, end) else {
                    break;
                };
                end = next;
            }
            span.start..end
        })
        .collect();
    ranges.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for r in ranges {
        match merged.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    if let Some(last) = merged.last_mut()
        && text[last.end..].trim().is_empty()
    {
        let content = text[..last.start].trim_end().len();
        last.start = text[content..last.start]
            .find('\n')
            .map_or(last.start, |n| content + n + 1);
        last.end = text.len();
    }
    merged
}

/// The edit of an entry that sets its keywords, or removes the field
/// without tags, in the style of the entry's other fields. `raw` is the
/// text of the entry; the range is in it.
fn keywords_edit(bib: &Path, raw: &str, tags: &[&str]) -> Result<Edit> {
    let parsed = parse_entry(bib, raw)?;
    let entry = &parsed.entries[0].v;
    let fields = &entry.fields;
    let value = tags.join(", ");
    // the library reads the last one
    let found = fields
        .iter()
        .rposition(|f| f.key.v.eq_ignore_ascii_case("keywords"));
    Ok(match found {
        Some(i) if tags.is_empty() => (remove_field(raw, fields, i), String::new()),
        Some(i) => {
            let start = fields[i].value.span.start;
            let quoted = raw[start..].starts_with('"');
            (start..value_end(raw, &fields[i]), delimit(quoted, &value))
        }
        None => insert_field(raw, entry, keywords_name(fields), &value),
    })
}

/// The text of one entry, parsed: it has an entry.
fn parse_entry<'a>(bib: &Path, raw: &'a str) -> Result<RawBibliography<'a>> {
    let parsed = RawBibliography::parse(raw).map_err(|e| Error::Parse {
        path: bib.to_path_buf(),
        line: 1,
        message: e.kind.to_string(),
    })?;
    if parsed.entries.is_empty() {
        return Err(Error::Parse {
            path: bib.to_path_buf(),
            line: 1,
            message: "not an entry".into(),
        });
    }
    Ok(parsed)
}

/// The link of a file in a `file` field, as JabRef writes it: `:path:PDF`, with `\`, `:` and `;`
/// of the path escaped.
fn file_link(path: &Path) -> String {
    let path = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace(';', "\\;");
    format!(":{path}:PDF")
}

/// The edit of an entry that adds links to files to its `file` field (made if it has none), after
/// those it has, in the style of the entry's other fields. `raw` is the text of the entry; the
/// range is in it.
fn files_edit(bib: &Path, raw: &str, files: &[PathBuf]) -> Result<Edit> {
    let parsed = parse_entry(bib, raw)?;
    let entry = &parsed.entries[0].v;
    let fields = &entry.fields;
    let links: Vec<String> = files.iter().map(|f| file_link(f)).collect();
    // the library reads the last one
    let found = fields
        .iter()
        .rposition(|f| f.key.v.eq_ignore_ascii_case("file"));
    Ok(match found {
        Some(i) => {
            let start = fields[i].value.span.start;
            let end = value_end(raw, &fields[i]);
            let quoted = raw[start..].starts_with('"');
            let old = crate::bib::field_text(&raw[start..end]);
            let value = std::iter::once(old)
                .filter(|o| !o.trim().is_empty())
                .chain(links.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(";");
            (start..end, delimit(quoted, &value))
        }
        None => insert_field(raw, entry, "file", &links.join(";")),
    })
}

/// The text of an entry with links to files added to its `file` field. It must be one entry.
pub fn with_files(text: &str, files: &[PathBuf]) -> Result<String> {
    let text = text.trim();
    let given = given_entries(text)?;
    if given.len() != 1 {
        return Err(Error::NotOneEntry { found: given.len() });
    }
    let edit = files_edit(Path::new(""), text, files)?;
    Ok(splice(text, vec![edit]))
}

/// Where a field is removed: with its comma, and with its line when it has
/// one of its own; the last field takes the comma before it.
fn remove_field(raw: &str, fields: &[Pair], i: usize) -> Range<usize> {
    let field = &fields[i];
    let (key, end) = (field.key.span.start, value_end(raw, field));
    if let Some(comma) = comma_after(raw, end) {
        return match (line_start(raw, key), line_end(raw, comma + 1)) {
            (Some(start), Some(end)) => start..end,
            _ => key..skip_blanks(raw, comma + 1),
        };
    }
    if i > 0 {
        let previous = value_end(raw, &fields[i - 1]);
        if raw[previous..key].trim() == "," {
            return previous..end;
        }
    }
    match (line_start(raw, key), line_end(raw, end)) {
        (Some(start), Some(end)) => start..end,
        _ => key..end,
    }
}

/// A new field before the closing brace, indented and spaced as the last
/// field; a trailing comma stays at the end.
fn insert_field(raw: &str, entry: &RawEntry, name: &str, value: &str) -> Edit {
    let nl = newline(raw);
    let Some(last) = entry.fields.last() else {
        let key = entry.key.span.end;
        let at = comma_after(raw, key).map_or(key, |c| c + 1);
        return (at..at, format!("{nl} {name} = {{{value}}}"));
    };
    let (key, end) = (last.key.span.start, value_end(raw, last));
    let separator = match line_start(raw, key) {
        Some(start) => format!("{nl}{}", &raw[start..key]),
        None => " ".to_string(),
    };
    let quoted = raw[last.value.span.start..].starts_with('"');
    let field = format!(
        "{name}{}{}",
        equals(raw, &entry.fields, last, name),
        delimit(quoted, value)
    );
    match comma_after(raw, end) {
        Some(comma) => (comma + 1..comma + 1, format!("{separator}{field},")),
        None => (end..end, format!(",{separator}{field}")),
    }
}

/// The `=` of a new field, spaced as in the last field, and at the column
/// of the others when their `=` are aligned.
fn equals(raw: &str, fields: &[Pair], last: &Pair, name: &str) -> String {
    let gap = |f: &Pair| &raw[f.key.span.end..f.value.span.start];
    let (before, after) = gap(last).split_once('=').unwrap_or((" ", " "));
    // the column of each `=`, its field on a line of its own
    let columns: Vec<Option<usize>> = fields
        .iter()
        .map(|f| {
            let start = line_start(raw, f.key.span.start)?;
            Some(f.key.span.end + gap(f).find('=')? - start)
        })
        .collect();
    let lengths: HashSet<usize> = fields.iter().map(|f| f.key.v.len()).collect();
    let aligned = columns.len() > 1
        && lengths.len() > 1
        && columns[0].is_some()
        && columns.iter().all(|c| *c == columns[0]);
    match (aligned, columns[0], line_start(raw, last.key.span.start)) {
        (true, Some(column), Some(start)) => {
            let indent = last.key.span.start - start;
            let pad = column.saturating_sub(indent + name.len()).max(1);
            format!("{}={after}", " ".repeat(pad))
        }
        _ => format!("{before}={after}"),
    }
}

/// `keywords`, in the case of the other field names: `Keywords`, `KEYWORDS`.
fn keywords_name(fields: &[Pair]) -> &'static str {
    let capital = |f: &Pair| f.key.v.starts_with(|c: char| c.is_ascii_uppercase());
    let upper = |f: &Pair| !f.key.v.contains(|c: char| c.is_ascii_lowercase());
    if fields.is_empty() || !fields.iter().all(capital) {
        "keywords"
    } else if fields.iter().all(upper) {
        "KEYWORDS"
    } else {
        "Keywords"
    }
}

fn delimit(quoted: bool, value: &str) -> String {
    if quoted && !value.contains('"') {
        format!("\"{value}\"")
    } else {
        format!("{{{value}}}")
    }
}

/// Whether the braces of a tag match, as in a field value.
fn balanced(tag: &str) -> bool {
    let mut depth = 0;
    let mut chars = tag.chars();
    while let Some(c) = chars.next() {
        match c {
            // escapes the next character, which must be there
            '\\' if chars.next().is_none() => return false,
            '{' => depth += 1,
            '}' if depth == 0 => return false,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth == 0
}

/// The end of the value of a field, without the white space after it.
fn value_end(raw: &str, field: &Pair) -> usize {
    let span = field.value.span.clone();
    span.start + raw[span].trim_end().len()
}

/// The comma after `at`, past white space.
fn comma_after(text: &str, at: usize) -> Option<usize> {
    let rest = &text[at..];
    let trimmed = rest.trim_start();
    trimmed
        .starts_with(',')
        .then(|| at + rest.len() - trimmed.len())
}

/// The start of the line of `at`, when only blanks come before it there.
fn line_start(text: &str, at: usize) -> Option<usize> {
    let start = text[..at].rfind('\n').map_or(0, |n| n + 1);
    is_blank(&text[start..at]).then_some(start)
}

/// The start of the next line, when only blanks follow `at` on its line.
fn line_end(text: &str, at: usize) -> Option<usize> {
    let n = text[at..].find('\n')?;
    is_blank(text[at..at + n].trim_end_matches('\r')).then_some(at + n + 1)
}

fn skip_blanks(text: &str, at: usize) -> usize {
    at + text[at..].len() - text[at..].trim_start_matches([' ', '\t']).len()
}

fn is_blank(s: &str) -> bool {
    s.chars().all(|c| c == ' ' || c == '\t')
}

/// The line break of a text: `\r\n` if it has one, else `\n`.
fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// The line of a position, from 1.
fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}
