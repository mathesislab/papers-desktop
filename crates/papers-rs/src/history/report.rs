//! What the commands that move in the history print (spec/history.md, 6.5):
//! the state restored, the files that moved, the pools that followed, and the
//! files of the files directory that no entry links to any more.

use std::collections::BTreeSet;

use super::recorder::History;
use super::travel::{Moved, bibtex_links};
use crate::config::paths::{basename, dirname, is_inside, isfile, realpath};

impl Moved {
    /// The lines of `papers undo`, `redo` and `restore` (`kind` is the first
    /// word): the state restored, the files that moved, the pools that
    /// followed; with `--dry-run`, the one line that says it was not done.
    pub fn lines(&self, kind: &str, history: &History) -> Vec<String> {
        let Some(files) = &self.files else {
            return vec![format!(
                "DRYRUN: {kind}: {} not restored to {}",
                history.setup.bibtex,
                history.describe(&self.target)
            )];
        };
        let mut lines = vec![format!(
            "{kind}: {} restored to {}",
            basename(&history.setup.bibtex),
            history.describe(&self.target)
        )];
        lines.extend(files.iter().cloned());
        lines.extend(self.pools.iter().cloned());
        lines
    }
}

/// The real paths of the files that the entries of the bibtex link to (none
/// when it does not exist): what `papers undo` compares before and after.
pub fn linked_files(bibtex: &str) -> BTreeSet<String> {
    bibtex_links(bibtex, &dirname(bibtex))
        .iter()
        .map(|f| realpath(f))
        .collect()
}

/// The last lines of `papers undo`: the files of the files directory that
/// the entries linked before and no longer link.
pub fn unlinked_notice(
    before: &BTreeSet<String>,
    after: &BTreeSet<String>,
    filesdir: Option<&str>,
) -> Vec<String> {
    let Some(dir) = filesdir.filter(|d| !d.is_empty()) else {
        return vec![];
    };
    let unlinked: Vec<&String> = before
        .difference(after)
        .filter(|f| isfile(f) && is_inside(f, dir))
        .collect();
    if unlinked.is_empty() {
        return vec![];
    }
    let mut lines = vec![format!(
        "{} file(s) of the files directory are linked by no entry now (`papers file clean` removes them):",
        unlinked.len()
    )];
    lines.extend(unlinked.iter().map(|f| format!("  {f}")));
    lines
}
