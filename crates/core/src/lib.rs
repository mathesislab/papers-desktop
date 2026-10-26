//! Library loading and search for papers-desktop.
//!
//! The papers CLI stays the only reader of the bibtex file: entries come in
//! through `papers list --format csl-json` and become search [`Item`]s.

mod cli;
mod csl;
mod item;
mod search;
mod watch;

pub use cli::Papers;
pub use item::{Cite, Item};
pub use search::Library;
pub use watch::BibWatch;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not run {cmd}: {source}")]
    Spawn { cmd: String, source: std::io::Error },
    #[error("{cmd} failed: {stderr}")]
    Command { cmd: String, stderr: String },
    #[error("invalid CSL-JSON: {0}")]
    Csl(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
