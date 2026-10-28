//! The commands that write a library, and the write path they share.
//!
//! - [`session`]: what every writing command does around its change (the
//!   lock, the history, the atomic save, the checksums): the module that the
//!   other commands use.
//! - [`report`]: the lines of a command: an entry on one line, and what changed
//!   (`Added -> ...`, `--porcelain`).
//! - [`add`], [`edit`], [`rm`], [`tag`]: the commands, as operations on a
//!   [`session::Session`]; the command line only parses and prints.
//! - [`files`]: what `add` does with the files of the entries added
//!   (`--move`, `--copy`, `--link`, the `files` setting).

pub mod add;
pub mod check;
pub mod edit;
pub mod files;
pub mod migrate;
pub mod report;
pub mod rm;
pub mod session;
pub mod tag;

pub use session::{Committed, NoPrompt, Options, Outcome, Prompt, Say, Session};

/// The identifiers of the command line, where `-` stands for the lines of
/// the standard input, one per line, the blank ones ignored
/// (`papers.identifiers.read_selection`).
pub fn read_selection(
    args: &[String],
    stdin: &mut dyn std::io::Read,
) -> std::io::Result<Vec<String>> {
    let mut selection = Vec::new();
    for arg in args {
        if arg == "-" {
            let mut text = String::new();
            stdin.read_to_string(&mut text)?;
            selection.extend(
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_string),
            );
        } else {
            selection.push(arg.clone());
        }
    }
    Ok(selection)
}
