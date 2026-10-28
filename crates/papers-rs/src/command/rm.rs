//! `papers rm`: remove entries from the library, and what becomes of their
//! files (`papers.cli.entry_cmds.rmcmd`, `remove_entries`, `papers.files.files_to_delete`).
//!
//! The files that only the removed entries link to may be deleted once the
//! library is saved ([`Session::delete_files`](super::Session::delete_files)):
//! with `--delete-files` all of them; by default those in the files directory
//! if the user says so in a terminal, else none; with `--keep-files` none. A
//! file that a remaining entry links to is never deleted.

use std::collections::HashSet;

use crate::config::paths::{is_inside, realpath};
use crate::entry;
use crate::error::Result;

use super::session::Session;

/// What to do with the files of the removed entries (`--delete-files`,
/// `--keep-files`, neither).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Files {
    Delete,
    Keep,
    /// ask in a terminal about the files in the files directory, keep the others
    Ask,
}

/// The files of the removed entries that no remaining entry links to, by where
/// they are: in the files directory, elsewhere; and those that are still linked
/// (`files_to_delete`). Only the files that exist, once each.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Candidates {
    pub inside: Vec<String>,
    pub outside: Vec<String>,
    pub kept: Vec<String>,
    /// in the files directory of a pool, or linked by an entry of the pool of a shallow library: never deleted
    pub in_pool: Vec<String>,
}

/// Remove the entries of these keys from the library, and list what becomes of their files.
pub fn take_out(session: &mut Session, keys: &[String]) -> Candidates {
    let files: Vec<String> = keys.iter().flat_map(|k| session.library.files(k)).collect();
    for key in keys {
        session.library.remove(key);
    }
    let relative_to = session.library.relative_to.clone();
    let still_linked: HashSet<String> = session
        .library
        .entries()
        .flat_map(|e| entry::files(e, Some(&relative_to)))
        .map(|f| realpath(&f))
        .collect();
    let filesdir = session.library.filesdir.clone();
    let pool_dirs = session.pool_files_dirs();
    // a shallow library: the files that the pool entries link to, wherever they are, are the pool's
    let pool_linked = session.pool_linked_files();
    let mut found = Candidates::default();
    let mut seen: HashSet<String> = HashSet::new();
    for file in files {
        let real = realpath(&file);
        if !seen.insert(real.clone()) || std::fs::metadata(&file).is_err() {
            continue;
        }
        let inside = filesdir.as_deref().is_some_and(|d| is_inside(&file, d));
        if still_linked.contains(&real) {
            found.kept.push(file);
        } else if (pool_dirs.iter().any(|d| is_inside(&file, d)) && !inside)
            || pool_linked.contains(&real)
        {
            found.in_pool.push(file);
        } else if inside {
            found.inside.push(file);
        } else {
            found.outside.push(file);
        }
    }
    found
}

fn is_hard_link(file: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(file)
        .map(|m| m.is_file() && m.nlink() > 1)
        .unwrap_or(false)
}

/// A file in a list of files to delete.
fn describe_file(file: &str) -> String {
    if is_hard_link(file) {
        format!(
            "{file} (a hard link: only this link is deleted, the file remains at its other links)"
        )
    } else {
        file.to_string()
    }
}

/// Remove entries from the library, and return the files to delete once it
/// is saved (`remove_entries`). With [`Files::Ask`] and `ask` in a terminal, the user is
/// asked about the files in the files directory.
pub fn remove_entries(
    session: &mut Session,
    keys: &[String],
    files: Files,
    ask: bool,
) -> Result<Vec<String>> {
    let found = take_out(session, keys);
    if files == Files::Keep {
        return Ok(Vec::new());
    }
    for f in &found.kept {
        session
            .say
            .line(format!("kept {f}: another entry links to it"));
    }
    for f in &found.in_pool {
        session.say.line(format!("kept {f}: it is a file of a pool"));
    }
    if files == Files::Delete {
        return Ok(found.inside.into_iter().chain(found.outside).collect());
    }
    if !found.outside.is_empty() {
        let note = if session.config.shallow() {
            ""
        } else {
            " (`--delete-files` deletes them)"
        };
        session.say.line(format!(
            "kept {} file(s) outside the files directory{note}:",
            found.outside.len()
        ));
        for f in &found.outside {
            session.say.line(format!("  {f}"));
        }
    }
    if found.inside.is_empty() {
        return Ok(Vec::new());
    }
    if ask && session.interactive() {
        session
            .say
            .line("files of the removed entries that no other entry links to:");
        for f in &found.inside {
            session.say.line(format!("  {}", describe_file(f)));
        }
        let answer = session.input(&format!(
            "also delete {} file(s)? [y/N] ",
            found.inside.len()
        ))?;
        if matches!(answer.to_lowercase().as_str(), "y" | "yes") {
            return Ok(found.inside);
        }
        return Ok(Vec::new());
    }
    session.say.line(format!(
        "kept {} file(s) of the removed entries in the files directory:",
        found.inside.len()
    ));
    for f in &found.inside {
        session.say.line(format!("  {f}"));
    }
    session
        .say
        .line("delete them with `papers file clean`, or remove the entries with `--delete-files`");
    Ok(Vec::new())
}
