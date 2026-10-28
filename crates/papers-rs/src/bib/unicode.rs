//! The LaTeX of the values as unicode text, as papers does it with
//! pylatexenc (the [`latexenc`] crate): two conversions that
//! differ, and the places that use them.
//!
//! - The decoding of a library or of an entry (`papers.encoding`:
//!   [`latex_to_unicode_library`], [`convert_entry_to_unicode`]) is
//!   bibtexparser's `LatexDecodingMiddleware` with its defaults: every value
//!   of every entry and `@string` goes through `LatexNodes2Text` (the macro
//!   `\url` first, so that its argument is not put in `<...>`; the braced
//!   groups lose their braces; the math is left as written). The values are
//!   not stripped, and what could not be decoded is as papers has it: the
//!   entry (or string) becomes an unreadable block, written back as it was
//!   read. What the blocks hold besides the values, and the way a value is
//!   enclosed, stay.
//! - The conversion of the export (`papers.formats._latex_to_unicode`:
//!   [`value_to_unicode`], and [`entry_unicode_fields`] for the fields of an
//!   entry) is `LatexNodes2Text` with its defaults but for the math, which is
//!   kept as written, stripped. A value with none of the characters of LaTeX
//!   markup is not converted (but stripped), one that cannot be converted
//!   has its braces removed.
//!
//! ```
//! use papers_rs::bib::{Bib, unicode};
//!
//! let bib = unicode::latex_to_unicode_library(Bib::parse(
//!     "@article{k, title = {{\\\"U}ber $\\alpha$ \\url{x.org}}, month = jan}",
//! ));
//! let entry = bib.entries().next().unwrap();
//! assert_eq!(entry.get("title"), Some("Über $\\alpha$ x.org"));
//! assert_eq!(unicode::value_to_unicode(" M{\\\"u}ller "), "Müller");
//! ```
//!
//! Where it differs from papers: the nesting of a text is limited to 250
//! levels (Python's `RecursionError` comes at 165 to 330, and the same text
//! may or may not raise it), and the message of an error is that of the Rust
//! port. [`value_to_unicode`] shares one converter for the whole process, as
//! papers does (`functools.lru_cache`): the `\title`, `\author` and `\date`
//! of a value are seen by the `\maketitle` of a later one.

use std::sync::OnceLock;

use latexenc::latex2text::{
    LatexNodes2Text, MacroTextSpec, MathMode, get_default_latex_context_db,
};
use latexenc::macrospec::CategoryPosition;

use super::pystr::strip;
use super::write;
use super::{Bib, Block, Entry, Field, Source, Unreadable, UnreadableKind};

/// What failed in the decoding of an entry: the messages of the values that
/// could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Middleware could not be fully applied: {}", self.0)
    }
}

impl std::error::Error for DecodeError {}

/// The decoder of `LatexDecodingMiddleware()`.
fn decoder() -> LatexNodes2Text {
    let mut db = get_default_latex_context_db();
    db.add_macro_category(
        "bibtexparse-default-context",
        vec![MacroTextSpec::with_repl("url", "%s")],
        CategoryPosition::Prepend,
    )
    .expect("the category is new");
    LatexNodes2Text::builder()
        .latex_context(db)
        .keep_braced_groups(false)
        .math_mode(MathMode::Verbatim)
        .build()
        .expect("the options are valid")
}

/// A value decoded, or the message of the error (the value stays).
fn decode(decoder: &LatexNodes2Text, value: &mut String) -> Result<(), String> {
    match decoder.latex_to_text(value) {
        Ok(text) => {
            *value = text;
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// The messages of the errors, joined as bibtexparser joins them.
fn join(errors: &[String]) -> String {
    errors.join("\n\n=====\n\n")
}

/// Decode the values of an entry, those that can be; the messages of the others.
fn decode_entry(decoder: &LatexNodes2Text, entry: &mut Entry) -> Vec<String> {
    let mut errors = Vec::new();
    for field in &mut entry.fields {
        if let Err(e) = decode(decoder, &mut field.value) {
            errors.push(e);
        }
    }
    errors
}

/// The text of a block that was not read from a text (it has no source).
fn written(block: &Block) -> Source {
    let raw = write::format(std::slice::from_ref(block));
    Source {
        raw: raw.trim_end_matches('\n').to_string(),
        line: 0,
    }
}

/// A library with the LaTeX of all its values decoded
/// (`papers.encoding.latex_to_unicode_library`).
///
/// The values of the entries and of the `@string` definitions, not the keys,
/// the preambles, the comments or the blocks that could not be read. An
/// entry or a string with a value that cannot be decoded is an unreadable
/// block ([`UnreadableKind::Decoding`]): it has the text it was read as and
/// the line; one of an entry that was not read is its text as written.
pub fn latex_to_unicode_library(mut bib: Bib) -> Bib {
    let decoder = decoder();
    for block in &mut bib.blocks {
        let (errors, source) = match block {
            Block::Entry(entry) => (decode_entry(&decoder, entry), entry.source.clone()),
            Block::String(string) => {
                let mut errors = Vec::new();
                if let Err(e) = decode(&decoder, &mut string.value) {
                    errors.push(e);
                }
                (errors, string.source.clone())
            }
            _ => continue,
        };
        if errors.is_empty() {
            continue;
        }
        let source = source.unwrap_or_else(|| written(block));
        *block = Block::Unreadable(Unreadable {
            raw: source.raw,
            line: source.line,
            kind: UnreadableKind::Decoding(join(&errors)),
        });
    }
    bib
}

/// The LaTeX of the values of an entry decoded, in place
/// (`papers.encoding.convert_entry_to_unicode`).
///
/// As papers does it: the values go through [`latex_to_unicode_library`]'s
/// decoder and are set again, so a field that was written without braces
/// (`month = jan`) is written with them. If a value cannot be decoded the
/// error is returned (papers fails with an `IndexError`): the other values
/// have been decoded, but not set again.
pub fn convert_entry_to_unicode(entry: &mut Entry) -> Result<(), DecodeError> {
    let decoder = decoder();
    let errors = decode_entry(&decoder, entry);
    if !errors.is_empty() {
        return Err(DecodeError(join(&errors)));
    }
    for field in &mut entry.fields {
        field.verbatim = false;
    }
    Ok(())
}

/// The characters of LaTeX markup: a value without them is text already.
const LATEX_CHARS: [char; 7] = ['\\', '{', '}', '$', '~', '%', '&'];

/// The converter of the export, built once.
fn export_converter() -> &'static LatexNodes2Text {
    static CONVERTER: OnceLock<LatexNodes2Text> = OnceLock::new();
    CONVERTER.get_or_init(|| {
        LatexNodes2Text::builder()
            .math_mode(MathMode::Verbatim)
            .build()
            .expect("the options are valid")
    })
}

/// A bibtex value as unicode text: LaTeX commands decoded, braces removed,
/// the math as written, stripped (`papers.formats._latex_to_unicode`).
///
/// A value without any of `\ { } $ ~ % &` is only stripped. If the
/// conversion fails, the value is returned with its braces removed (and not
/// stripped). See the module for the converter that all calls share.
pub fn value_to_unicode(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    if !value.contains(LATEX_CHARS) {
        return strip(value).to_string();
    }
    match export_converter().latex_to_text(value) {
        Ok(text) => strip(&text).to_string(),
        Err(_) => value.replace(['{', '}'], ""),
    }
}

/// The fields of an entry as unicode text, for export
/// (`papers.formats._unicode_fields`): the name of each field (in lower case)
/// and its value, [`value_to_unicode`] of it, but for `url`, `doi`, `file`,
/// `eprint`, `isbn` and `issn`, which are as they are. The fields with an
/// empty value are left out; of a field given twice, the last value is at the
/// place of the first.
pub fn entry_unicode_fields(entry: &Entry) -> Vec<(String, String)> {
    const RAW: [&str; 6] = ["url", "doi", "file", "eprint", "isbn", "issn"];
    let mut out: Vec<(String, String)> = Vec::new();
    for Field { key, value, .. } in &entry.fields {
        if value.is_empty() {
            continue;
        }
        let key = key.to_lowercase();
        let value = if RAW.contains(&key.as_str()) {
            value.clone()
        } else {
            value_to_unicode(value)
        };
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => *v = value,
            None => out.push((key, value)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value() {
        assert_eq!(value_to_unicode(""), "");
        assert_eq!(value_to_unicode("  plain "), "plain");
        assert_eq!(value_to_unicode(r#" {M\"u}ller "#), "Müller");
        assert_eq!(value_to_unicode("$\\alpha$ {CO}$_2$"), "$\\alpha$ CO$_2$");
        assert_eq!(value_to_unicode(r"\url{http://a.b/c}"), "<http://a.b/c>");
        // a value that cannot be converted: the braces go, the spaces stay
        assert_eq!(value_to_unicode(r" {a \verb"), " a \\verb");
    }

    #[test]
    fn library() {
        let bib = latex_to_unicode_library(Bib::parse(
            "@string{s = {{\\\"u}}}\n@article{a, title = {{\\\"U}ber \\url{x.org}}, month = jan}\n\n@article{b, note = {\\verb}}\n",
        ));
        assert_eq!(bib.strings().next().unwrap().value, "ü");
        let a = bib.entries().next().unwrap();
        assert_eq!(a.get("title"), Some("Über x.org"));
        // as it was written: a value without braces stays so
        assert!(a.fields[1].verbatim);
        assert_eq!(bib.entries().count(), 1);
        assert_eq!(bib.unreadable(), vec![(4, "syntax error".to_string())]);
        assert!(matches!(
            bib.blocks.last(),
            Some(Block::Unreadable(Unreadable { raw, kind: UnreadableKind::Decoding(_), .. }))
                if raw == "@article{b, note = {\\verb}}"
        ));
    }

    #[test]
    fn entry() {
        let mut bib = Bib::parse("@article{a, title = {{\\\"u}}, month = jan}");
        let entry = bib.entries_mut().next().unwrap();
        convert_entry_to_unicode(entry).unwrap();
        // set again as papers does: in braces
        assert!(!entry.fields[1].verbatim);
        assert_eq!(entry.get("title"), Some("ü"));

        let mut bib = Bib::parse("@article{a, title = {{\\\"u}}, note = {\\verb}}");
        let entry = bib.entries_mut().next().unwrap();
        assert!(convert_entry_to_unicode(entry).is_err());
        assert_eq!(entry.get("title"), Some("ü"));
    }

    #[test]
    fn fields() {
        let mut e = Entry::new("article", "k");
        e.set("Title", r"{\'e}");
        e.set("url", r"http://x.org/a_b~c");
        e.set("empty", "");
        assert_eq!(
            entry_unicode_fields(&e),
            vec![
                ("title".to_string(), "é".to_string()),
                ("url".to_string(), "http://x.org/a_b~c".to_string())
            ]
        );
    }
}
