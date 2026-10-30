//! Errors, as papers' command line reports them.

use std::fmt;

/// The version of papers-cli that this port follows (its `--version`, and the
/// version in the errors of configuration files that are too new).
pub const PAPERS_VERSION: &str = "3.0.0b7.dev94+ge1790670a";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// An expected error (papers' `PapersExit`): the command line prints
    /// `papers: error: MESSAGE` and exits with status 1; an empty message
    /// exits with status 1 and prints nothing.
    Exit(String),
    /// What papers does not catch (a traceback there): a file that is not
    /// JSON, a directory that cannot be created...
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn exit(message: impl Into<String>) -> Error {
        Error::Exit(message.into())
    }

    pub fn other(message: impl Into<String>) -> Error {
        Error::Other(message.into())
    }

    pub fn message(&self) -> &str {
        match self {
            Error::Exit(m) | Error::Other(m) => m,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Error {
        Error::Other(error.to_string())
    }
}
