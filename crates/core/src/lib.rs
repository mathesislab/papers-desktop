//! Library loading and search for papers-desktop.
//!
//! The library is one or more .bib files, read as they are: the entries
//! become search [`Item`]s, and keep their BibTeX and their CSL-JSON (for a
//! citation processor). The app's own writer changes the entries of a file
//! and leaves the rest of it as it is.

mod bib;
mod fields;
mod item;
mod query;
mod reference;
mod search;
mod table;
mod tag;
mod text;
mod to_csl;
mod watch;
mod write;

use std::path::PathBuf;

pub use item::{Cite, Item};
pub use search::{Found, Hit, Library, YearCount};
pub use table::{FileState, Incoming, Row, TagCount};
pub use tag::{same_tag, under};
pub use text::{Matching, Span};
pub use watch::BibWatch;
pub use write::{Added, Change, entry_keys};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{}, line {line}: {message}", path.display())]
    Parse {
        path: PathBuf,
        line: usize,
        message: String,
    },
    /// a write refused: the file is no longer as the library read it
    #[error("{}: changed since it was read; reloaded, try again", path.display())]
    Changed { path: PathBuf },
    #[error("{}: {source}", path.display())]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{id}: not in the library")]
    NotFound { id: String },
    #[error("{}: not a .bib file of the library", path.display())]
    NotInLibrary { path: PathBuf },
    /// BibTeX given to write that does not parse, at a line of it
    #[error("BibTeX, line {line}: {message}")]
    Bibtex { line: usize, message: String },
    #[error("the BibTeX has {found} entries, not one")]
    NotOneEntry { found: usize },
    #[error("the BibTeX has no entry")]
    NoEntry,
    #[error("{}: already has an entry {key}", path.display())]
    DuplicateKey { path: PathBuf, key: String },
    /// an edit of the text of an entry that changes what it keeps
    #[error("the {what} of an entry cannot be changed by editing its BibTeX")]
    Kept { what: &'static str },
    #[error("tag {tag:?}: its braces do not match")]
    Tag { tag: String },
}

pub type Result<T> = std::result::Result<T, Error>;
