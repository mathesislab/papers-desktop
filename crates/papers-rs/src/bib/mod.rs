//! The bibtex of a library: read as papers reads it (bibtexparser v2 with
//! the middlewares of papers) and written as papers writes it, byte for byte.
//!
//! ```
//! use papers_rs::bib::Bib;
//!
//! let bib = Bib::parse("@Article{k,\n Title = {Ice},\n year = 2020,\n}\n");
//! assert_eq!(bib.entries().next().unwrap().get("title"), Some("Ice"));
//! // the text papers saves: one-space indent, fields in alphabetical order
//! assert_eq!(bib.to_string(), "@article{k,\n title = {Ice},\n year = {2020}\n}\n");
//! ```
//!
//! What papers keeps of a text, `Bib` keeps: the entries (type in lower case,
//! key, fields with their values without the braces), the strings, the
//! preambles, the comments, and the text of what bibtexparser cannot read
//! (an entry with a field given twice, a second entry with a key: both
//! ignore case, as BibTeX does; a syntax error), which is written back as it
//! was and is not seen as an entry.
//! What papers drops, `Bib` drops: the string references are resolved, the
//! quotes of a value become braces, the field names are in lower case, the
//! spacing of an entry is gone. The values are as written, LaTeX included:
//! nothing is converted at reading or writing, the conversions are those of
//! [`latex_table`] and [`unicode`] below, called where papers calls them.
//!
//! - [`Bib::parse`], [`read_file`]: a text, a file.
//! - [`Bib::unreadable`]: what could not be read, and where.
//! - [`Bib::sort`], [`Bib::format`], and `Display` (both: the text of a save).
//! - [`latex`]: LaTeX as unicode, the decoder of the same-paper rule (`papers.latex`).
//! - [`latex_table`]: LaTeX by the table of `papers.latexenc`
//!   (`check --encoding latex`).
//! - [`unicode`]: the LaTeX of the values as unicode text, by pylatexenc
//!   with the [`latexenc`] crate: of a library or an entry
//!   (`fix --encoding unicode`, `add`; bibtexparser's `LatexDecodingMiddleware`),
//!   of a value or the fields of an entry (the export formats). Not ported:
//!   bibtexparser's `LatexEncodingMiddleware` (papers does not use it: it
//!   writes LaTeX with [`latex_table`]).
//! - [`encoding`]: names and keywords of a value, and the `file` field.

#[rustfmt::skip]
mod latexenc_tables;
pub mod pystr;
mod read;
mod split;
mod write;

pub mod encoding;
pub mod latex;
pub mod latex_table;
pub mod unicode;

use std::fmt;
use std::path::Path;

/// A bibtex file: its blocks, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bib {
    pub blocks: Vec<Block>,
}

/// A block of a bibtex file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// `@article{key, ...}`
    Entry(Entry),
    /// `@string{key = {value}}`
    String(StringDef),
    /// `@preamble{...}`: what is between the braces, as written
    Preamble(String),
    /// `@comment{...}`: what is between the braces, without the spaces around it
    Comment(String),
    /// the text outside of a block, between two of them (without the empty lines around it)
    Text(String),
    /// a block that bibtexparser could not read, as written
    Unreadable(Unreadable),
}

/// An entry: its type is in lower case, as papers reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub entry_type: String,
    pub key: String,
    /// the fields in the order they have in the text; the names are in lower case
    pub fields: Vec<Field>,
    /// where it was read, if it was
    pub source: Option<Source>,
}

/// The text of a block as it was read, and its line: what bibtexparser keeps
/// of a block (`raw`, `start_line`). It does not change with the block, and
/// is what is written back if the block turns unreadable (see
/// [`UnreadableKind::Decoding`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub raw: String,
    /// the line where it starts, counted from 0 as bibtexparser does
    pub line: usize,
}

/// A field, its value without the braces (or quotes) around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub key: String,
    pub value: String,
    /// The value was not enclosed and is not a number (`month = jan`,
    /// `note = "a" # "b"`): it is written as it is. Whoever gives a field
    /// a new value gives it `false` (see [`Field::new`]).
    pub verbatim: bool,
}

/// A `@string` definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringDef {
    pub key: String,
    pub value: String,
    /// see [`Field::verbatim`]
    pub verbatim: bool,
    /// where it was read, if it was
    pub source: Option<Source>,
}

/// A block that bibtexparser could not read: its text is written back as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    /// the text of the block
    pub raw: String,
    /// the line where it starts, counted from 0 as bibtexparser does
    pub line: usize,
    pub kind: UnreadableKind,
}

/// Why a block could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnreadableKind {
    /// the names (sorted, in lower case) of the fields given twice (field names ignore case)
    DuplicateFields(Vec<String>),
    /// the key of an entry that an entry before it has already (keys ignore
    /// case), or of a string whose name another string has exactly
    DuplicateKey(String),
    /// a syntax error
    Syntax,
    /// an entry or string whose values could not be decoded
    /// ([`unicode::latex_to_unicode_library`]): papers keeps the text it had
    /// before, and does not see it as an entry. The message of the error is
    /// that of the Rust port, not of Python.
    Decoding(String),
}

impl Field {
    /// A field with a value to be written in braces.
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Field {
        Field {
            key: key.into(),
            value: value.into(),
            verbatim: false,
        }
    }
}

impl Entry {
    /// An entry without fields.
    pub fn new(entry_type: impl Into<String>, key: impl Into<String>) -> Entry {
        Entry {
            entry_type: entry_type.into(),
            key: key.into(),
            fields: Vec::new(),
            source: None,
        }
    }

    /// The value of a field: of a field given twice, the last one.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .rev()
            .find(|f| f.key == key)
            .map(|f| f.value.as_str())
    }

    /// Give a field a value (as `entry[key] = value`): the first field of
    /// this name has it, or a new field is added at the end.
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let field = Field::new(key, value);
        match self.fields.iter().position(|f| f.key == key) {
            Some(i) => self.fields[i] = field,
            None => self.fields.push(field),
        }
    }

    /// Remove the fields of this name; the value of the first, if any.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        let first = self.fields.iter().position(|f| f.key == key)?;
        let value = self.fields[first].value.clone();
        self.fields.retain(|f| f.key != key);
        Some(value)
    }
}

impl Unreadable {
    /// The key it is sorted by: that of a duplicate, else none.
    pub(crate) fn key(&self) -> &str {
        match &self.kind {
            UnreadableKind::DuplicateKey(key) => key,
            _ => "",
        }
    }

    /// The reason, as papers says it.
    pub fn reason(&self) -> String {
        match &self.kind {
            UnreadableKind::DuplicateFields(keys) => {
                let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
                format!("a field given twice: '{}'", keys.join(", "))
            }
            UnreadableKind::DuplicateKey(key) => {
                format!("a key given twice (keys ignore case): '{key}'")
            }
            _ => "syntax error".to_string(),
        }
    }
}

impl Bib {
    /// Read a text as papers does: `bibtexparser.parse_string` with its
    /// default parse stack, and the names of the fields in lower case.
    /// A text is read as `Biblio.load` reads a file: see [`read_file`] for
    /// the line ends.
    pub fn parse(text: &str) -> Bib {
        let mut blocks = split::split(text);
        read::transform(&mut blocks);
        Bib { blocks }
    }

    /// The entries, in order. The blocks that could not be read are not entries.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.blocks.iter().filter_map(|b| match b {
            Block::Entry(e) => Some(e),
            _ => None,
        })
    }

    /// The entries, to change.
    pub fn entries_mut(&mut self) -> impl Iterator<Item = &mut Entry> {
        self.blocks.iter_mut().filter_map(|b| match b {
            Block::Entry(e) => Some(e),
            _ => None,
        })
    }

    /// The `@string` definitions, in order.
    pub fn strings(&self) -> impl Iterator<Item = &StringDef> {
        self.blocks.iter().filter_map(|b| match b {
            Block::String(s) => Some(s),
            _ => None,
        })
    }

    /// The blocks that bibtexparser could not read (`papers.bib.unreadable_blocks`):
    /// the line where each starts (counted from 1) and why.
    pub fn unreadable(&self) -> Vec<(usize, String)> {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Unreadable(u) => Some((u.line + 1, u.reason())),
                _ => None,
            })
            .collect()
    }

    /// Order the blocks as a save does (`Biblio.sort`): strings, preambles,
    /// entries by key, comments, and what could not be read; the comments
    /// above a block stay with it.
    pub fn sort(&mut self) {
        let blocks = std::mem::take(&mut self.blocks);
        self.blocks = write::sort_blocks(blocks);
    }

    /// The order that [`Bib::sort`] gives the blocks: their indexes, sorted.
    pub fn sort_order(&self) -> Vec<usize> {
        write::sort_order(&self.blocks)
    }

    /// The text of the blocks in the order they have (`Biblio.format`): the
    /// fields of an entry in alphabetical order, one space of indent, the values
    /// in braces, one empty line between the blocks.
    pub fn format(&self) -> String {
        write::format(&self.blocks)
    }
}

/// The text papers saves: the blocks sorted, then formatted (`Biblio.save`).
impl fmt::Display for Bib {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut sorted = self.clone();
        sorted.sort();
        f.write_str(&sorted.format())
    }
}

/// The text of a file as papers reads it (`open(file, encoding='utf-8-sig').read()`):
/// UTF-8 without the byte order mark that some editors write first, the line
/// ends `\r\n` and `\r` read as `\n`.
pub fn read_file(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8(bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(universal_newlines(without_bom(&text)))
}

/// A text without the byte order mark at its start (the `utf-8-sig` codec).
pub fn without_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// Python's universal newlines: `\r\n` and `\r` become `\n`.
pub fn universal_newlines(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_string();
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}
