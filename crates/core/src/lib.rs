//! Library loading and search for papers-desktop.
//!
//! The library is one or more .bib files, read as they are: the entries
//! become search [`Item`]s, and keep their BibTeX and their CSL-JSON (for a
//! citation processor).

mod bib;
mod fields;
mod item;
mod query;
mod reference;
mod search;
mod text;
mod to_csl;
mod watch;

use std::path::PathBuf;

pub use item::{Cite, Item};
pub use search::{Found, Hit, Library, YearCount};
pub use text::{Matching, Span};
pub use watch::BibWatch;

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
}

pub type Result<T> = std::result::Result<T, Error>;
