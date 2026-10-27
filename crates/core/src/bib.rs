//! Read a .bib file: its entries, each with the text it has in the file.

use std::hash::{DefaultHasher, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};

use biblatex::{Bibliography, Entry, RawBibliography};

use crate::{Error, Result};

/// An entry of a .bib file.
pub(crate) struct BibEntry {
    pub entry: Entry,
    /// the entry as written in the file
    pub raw: String,
    /// where `raw` is in the file, in bytes
    pub span: Range<usize>,
    /// the linked files, absolute
    pub files: Vec<PathBuf>,
    /// the .bib file
    pub bib: PathBuf,
}

/// A .bib file as read.
pub(crate) struct BibFile {
    pub entries: Vec<BibEntry>,
    pub snapshot: Snapshot,
}

/// The length and a hash of the bytes of a file as read: a write checks
/// that the file is still the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Snapshot {
    len: usize,
    hash: u64,
}

impl Snapshot {
    pub fn of(bytes: &[u8]) -> Snapshot {
        let mut hasher = DefaultHasher::new();
        hasher.write(bytes);
        Snapshot {
            len: bytes.len(),
            hash: hasher.finish(),
        }
    }
}

pub(crate) fn read(bib: &Path) -> Result<BibFile> {
    let read_error = |source| Error::Read {
        path: bib.to_path_buf(),
        source,
    };
    let bytes = std::fs::read(bib).map_err(read_error)?;
    let snapshot = Snapshot::of(&bytes);
    let src = String::from_utf8(bytes)
        .map_err(|e| read_error(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
    let entries = parse(bib, &src).map_err(|(at, message)| Error::Parse {
        path: bib.to_path_buf(),
        line: src[..at.min(src.len())].lines().count().max(1),
        message,
    })?;
    Ok(BibFile { entries, snapshot })
}

/// The entries of the text of .bib file `bib`; else where the text does
/// not parse, and why.
pub(crate) fn parse(bib: &Path, src: &str) -> std::result::Result<Vec<BibEntry>, (usize, String)> {
    let raw = RawBibliography::parse(src).map_err(|e| (e.span.start, e.kind.to_string()))?;
    // the key, the text and the file links of each entry, in the order of the file
    let dir = bib.parent().unwrap_or(Path::new("."));
    let located: Vec<_> = raw
        .entries
        .iter()
        .map(|e| {
            let file =
                e.v.fields
                    .iter()
                    .find(|f| f.key.v.eq_ignore_ascii_case("file"));
            let files = file.map_or_else(Vec::new, |f| {
                file_links(field_text(&src[f.value.span.clone()]), dir)
            });
            (
                e.v.key.v.to_string(),
                entry_span(src, e.span.clone()),
                files,
            )
        })
        .collect();
    let typed = Bibliography::from_raw(raw).map_err(|e| (e.span.start, e.kind.to_string()))?;
    Ok(located
        .into_iter()
        .filter_map(|(key, span, files)| {
            Some(BibEntry {
                entry: typed.get(&key)?.clone(),
                raw: src[span.clone()].to_string(),
                span,
                files,
                bib: bib.to_path_buf(),
            })
        })
        .collect())
}

/// Where the text of an entry is: its span stops before the closing brace
/// (or parenthesis), which this takes in.
pub(crate) fn entry_span(src: &str, span: Range<usize>) -> Range<usize> {
    let rest = &src[span.end..];
    let close = rest
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .filter(|(_, c)| matches!(c, '}' | ')'));
    span.start..close.map_or(span.end, |(i, _)| span.end + i + 1)
}

/// The value of a raw field, without its outer braces or quotes.
fn field_text(value: &str) -> &str {
    let value = value.trim();
    let inner = value
        .strip_prefix('{')
        .and_then(|v| v.strip_suffix('}'))
        .or_else(|| value.strip_prefix('"').and_then(|v| v.strip_suffix('"')));
    inner.unwrap_or(value)
}

/// The paths of a `file` field: JabRef's `description:path:type;…` (with
/// `\:` and `\;` escaped), `path:type`, or plain paths separated by `;`.
/// Relative paths are relative to the directory of the .bib file.
pub(crate) fn file_links(field: &str, dir: &Path) -> Vec<PathBuf> {
    split(field, ';')
        .into_iter()
        .filter(|link| !link.trim().is_empty())
        .filter_map(|link| {
            let parts: Vec<String> = split(link, ':').into_iter().map(unescape).collect();
            let path = match parts.as_slice() {
                [path] => path.clone(),
                // a Windows drive, C:\…
                [drive, rest] if drive.len() == 1 => format!("{drive}:{rest}"),
                [path, kind] if !kind.contains(['/', '\\']) => path.clone(),
                [_, path, ..] => path.clone(),
                [] => return None,
            };
            let path = path.trim();
            (!path.is_empty()).then(|| dir.join(path))
        })
        .collect()
}

/// Split at the separators that no backslash escapes, keeping the escapes.
fn split(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut escaped) = (0, false);
    for (i, c) in s.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            c if c == sep => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// `\:`, `\;` and `\\` to `:`, `;` and `\`.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (c, chars.clone().next()) {
            ('\\', Some(next @ (':' | ';' | '\\'))) => {
                out.push(next);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links() {
        let dir = Path::new("/lib");
        let paths = |field| file_links(field, dir);
        assert_eq!(
            paths(":/a/b.pdf:pdf;:/a/c d.pdf:PDF"),
            [PathBuf::from("/a/b.pdf"), PathBuf::from("/a/c d.pdf")]
        );
        assert_eq!(
            paths("Full text:papers/x.pdf:application/pdf"),
            [PathBuf::from("/lib/papers/x.pdf")]
        );
        assert_eq!(
            paths("/a/b.pdf;/a/c.pdf"),
            [PathBuf::from("/a/b.pdf"), PathBuf::from("/a/c.pdf")]
        );
        // absolute on Windows only
        assert_eq!(
            paths(r":C\:\\docs\\x.pdf:PDF"),
            [dir.join(r"C:\docs\x.pdf")]
        );
        assert_eq!(
            paths(r":/a/odd\;name.pdf:pdf"),
            [PathBuf::from("/a/odd;name.pdf")]
        );
        assert_eq!(paths("/a/b.pdf:pdf"), [PathBuf::from("/a/b.pdf")]);
        assert!(paths("").is_empty());
    }
}
