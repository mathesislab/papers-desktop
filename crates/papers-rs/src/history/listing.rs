//! What `papers history` shows (design/history.md, sections 9.3 and 12), as
//! lines, and the removal of a history.

use std::collections::{BTreeSet, HashMap};
use std::fs;

use super::project::{Project, StateId};
use super::recorder::{History, prune_store};
use super::setup::Setup;
use super::store::{MB, Ref, Store};
use super::travel::{damaged_before, describe, kept_size};
use crate::config::paths::{isdir, join, short_home};
use crate::config::{config_root, dirs};
use crate::error::{Error, Result};

fn mb(size: u64) -> String {
    format!("{:.1} MB", size as f64 / MB as f64)
}

fn size_text(size: u64) -> String {
    if size < MB {
        format!("{:.0} KB", size as f64 / 1024.0)
    } else {
        mb(size)
    }
}

/// The lines of `papers history` for a library: the redo list, the line of
/// the head (`n` states: 0 for all; a negative `n` leaves out that many of the
/// oldest, as Python's `line[:n]` does), and the store's line. `history` is the
/// library's (none: no history yet, or no library).
pub fn history_lines(
    setup: &Setup,
    history: Option<&History>,
    has_bibtex: bool,
    n: i64,
) -> Vec<String> {
    let mut lines = Vec::new();
    let head = history.and_then(|h| h.head());
    // a head on a damaged state: recorded again by the next command
    let lost = history.filter(|_| head.is_none()).and_then(|h| {
        let place = h.project.heads().into_iter().find(|p| p.place == h.place)?;
        place.state.filter(|n| h.project.is_damaged(n))
    });
    match (&head, history, lost) {
        (None, Some(h), Some(lost)) => lines.push(format!("> {}", describe(&h.project, &lost))),
        (Some(head), Some(h), _) => {
            let project = &h.project;
            let line = project.line(&head.state);
            let count = if n >= 0 {
                (n as usize).min(line.len())
            } else {
                line.len().saturating_sub(n.unsigned_abs() as usize)
            };
            let shown: &[StateId] = if n == 0 { &line } else { &line[..count] };
            let kept = |n: &str| {
                let size = kept_size(project, &h.store, n);
                if size > 0 {
                    format!("  [{} of deleted files]", size_text(size))
                } else {
                    String::new()
                }
            };
            for r in head.redo.iter().rev() {
                lines.push(format!(
                    "  {}  (undone: `papers redo`){}",
                    describe(project, r),
                    kept(r)
                ));
            }
            // a state one of whose pieces is missing or empty (spec/history.md 2.9, 12)
            let mark = |n: &str| {
                let pieces: Vec<String> = project
                    .state(n)
                    .map(|s| {
                        let files = s.files().unwrap_or_default();
                        s.bibtex_pieces()
                            .into_iter()
                            .chain(files.into_iter().flat_map(|(_, p)| p))
                            .collect()
                    })
                    .unwrap_or_default();
                if h.store.unreadable(&pieces).is_some() {
                    "  (damaged: a piece of it is missing or empty in the history)"
                } else {
                    ""
                }
            };
            for s in shown {
                lines.push(format!(
                    "{} {}{}{}",
                    if *s == head.state { ">" } else { " " },
                    describe(project, s),
                    kept(s),
                    mark(s)
                ));
            }
            if line.len() > shown.len() {
                lines.push(format!(
                    "  ... {} older step(s) (-n 0 shows them all)",
                    line.len() - shown.len()
                ));
            } else if let Some(before) = line.last().and_then(|n| damaged_before(project, n)) {
                lines.push(format!("  {}", describe(project, &before)));
            }
        }
        _ => lines.push(
            if has_bibtex {
                "no history yet: each command that changes the library records a step"
            } else {
                "no library: `papers history --all` lists the histories of the machine"
            }
            .to_string(),
        ),
    }
    let store = history
        .map(|h| h.store.clone())
        .unwrap_or_else(|| setup.store(None));
    let size = setup.history_size(&store);
    let limit = if size > 0 {
        format!("up to {size} MB")
    } else {
        "no limit".to_string()
    };
    lines.push(format!(
        "history: {}, {} ({limit}: the history-size setting; deleted files {})",
        short_home(&store.root),
        mb(store.size()),
        if setup.keep_deleted_files {
            "kept"
        } else {
            "not kept: keep-deleted-files false"
        }
    ));
    for (path, size) in stale_backups(setup) {
        lines.push(format!(
            "stale: {} (backup of an earlier papers, {}, no longer used) \u{2014} delete with: rm -rf {}",
            short_home(&path),
            mb(size),
            short_home(&path)
        ));
    }
    lines
}

/// The lines of `papers history --all`: for each store with projects, its
/// root and size, and each project.
pub fn all_lines(setup: &Setup) -> Vec<String> {
    let mut lines = Vec::new();
    for store in setup.all_stores() {
        let ids = store.project_ids();
        if ids.is_empty() {
            continue;
        }
        let sizes = store.sizes();
        let mut refs: HashMap<Ref, BTreeSet<String>> = HashMap::new();
        let mut projects: Vec<Project> = Vec::new();
        for id in &ids {
            let Ok(project) = store.project(id) else {
                continue;
            };
            for state in project.states().values() {
                for r in state.refs() {
                    refs.entry(r).or_default().insert(id.clone());
                }
            }
            projects.push(project);
        }
        lines.push(format!("{}: {}", short_home(&store.root), mb(store.size())));
        // in the order of the bibtex of the place last used (the identifiers are random), those
        // without place last, the identifier breaking ties (history.md 12)
        let mut projects: Vec<(Option<String>, &Project)> = projects
            .iter()
            .map(|project| {
                let mut last: Option<(String, f64)> = None;
                for h in project.heads() {
                    if last.as_ref().is_none_or(|(_, t)| h.time > *t) {
                        last = Some((h.place, h.time));
                    }
                }
                (last.map(|(place, _)| place), project)
            })
            .collect();
        projects.sort_by(|(a, x), (b, y)| {
            (a.is_none(), a.as_deref().unwrap_or(""), &x.id).cmp(&(
                b.is_none(),
                b.as_deref().unwrap_or(""),
                &y.id,
            ))
        });
        for (last, project) in projects {
            let own: u64 = refs
                .iter()
                .filter(|(_, users)| users.len() == 1 && users.contains(&project.id))
                .filter_map(|(r, _)| sizes.get(r).map(|i| i.size))
                .sum();
            lines.push(format!(
                "* {}: {} steps, {} of its own{}",
                project.id,
                project.states().len(),
                mb(own),
                last.map(|place| format!(", last used at {}", short_home(&place)))
                    .unwrap_or_default()
            ));
        }
    }
    lines
}

fn dir_size(path: &str) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for e in entries.flatten() {
            let p = e.path();
            match fs::symlink_metadata(&p) {
                Ok(m) if m.is_dir() => total += dir_size(&p.to_string_lossy()),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

/// The backups of earlier versions that papers no longer uses (listed, never
/// deleted): `.papers/backup` of the library, and the
/// `backups` folders of the data directories. `(path, bytes)`.
pub fn stale_backups(setup: &Setup) -> Vec<(String, u64)> {
    let mut found: Vec<String> = Vec::new();
    if let Some(file) = setup.file.as_deref() {
        let local = join(&join(&config_root(file), ".papers"), "backup");
        if isdir(&local) {
            found.push(local);
        }
    }
    let legacy_data = match std::env::var("XDG_DATA_HOME") {
        Ok(v) => v,
        Err(_) => join(&join(&crate::config::paths::home(), ".local"), "share"),
    };
    for root in [
        join(&dirs::data_dir(), "backups"),
        join(&join(&legacy_data, "papers"), "backups"),
    ] {
        if isdir(&root) && !found.contains(&root) {
            found.push(root);
        }
    }
    found
        .into_iter()
        .map(|p| (p.clone(), dir_size(&p)))
        .collect()
}

/// `papers history --remove` without an id: forget the place of the library;
/// when no other place uses the project, the project is to be removed
/// (`Removal::Project`, after the confirmation of the user).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// No history: `the library has no history`.
    NoHistory,
    /// Other places use the project, and this place has no head (a copy of the library never used
    /// in its folder): nothing is removed.
    NoPlace { place: String, others: Vec<String> },
    /// Other places use the project: this place's head was removed (not with a dry run).
    Place { place: String, others: Vec<String> },
    /// No other place uses it: the number of its steps.
    Project { steps: usize },
}

impl Removal {
    /// What `papers history --remove` prints when it is done, or for
    /// `Project`, the question to ask first (`removed the history of the
    /// library (N steps)` once it is deleted).
    pub fn message(&self) -> String {
        match self {
            Removal::NoHistory => "the library has no history".to_string(),
            Removal::NoPlace { place, others } => format!(
                "nothing to remove: {} has no history; the history of the library is that of the other folder(s) that use it: {}",
                short_home(place),
                others
                    .iter()
                    .map(|p| short_home(p))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Removal::Place { place, others } => format!(
                "removed the history of {}; the history of the library stays for the other folder(s) that use it: {}",
                short_home(place),
                others
                    .iter()
                    .map(|p| short_home(p))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Removal::Project { steps } => format!("the history of the library ({steps} steps)"),
        }
    }
}

impl Removal {
    /// What a dry run says instead of removing (`None`: nothing would be removed anyway).
    pub fn dry_message(&self) -> Option<String> {
        match self {
            Removal::Place { place, .. } => Some(format!(
                "DRYRUN: the history of {} not removed",
                short_home(place)
            )),
            Removal::Project { .. } => Some(format!("DRYRUN: {} not removed", self.message())),
            Removal::NoHistory | Removal::NoPlace { .. } => None,
        }
    }
}

/// What `papers history --drop-files` shows and asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropFiles {
    /// The files that would be dropped, with their sizes.
    pub list: Vec<String>,
    /// The question (not asked with `-y`).
    pub question: String,
    /// What is printed when it is done.
    pub done: String,
    /// The total size, as the messages say it.
    pub size: String,
}

/// The lines of `papers history --drop-files` for the files found (see
/// `History::files_to_drop`).
pub fn drop_files_plan(found: &std::collections::BTreeMap<String, (String, u64)>) -> DropFiles {
    let total: u64 = found.values().map(|(_, size)| size).sum();
    let mut files: Vec<&(String, u64)> = found.values().collect();
    files.sort();
    let s = if found.len() > 1 { "s" } else { "" };
    DropFiles {
        list: files
            .iter()
            .map(|(path, size)| format!("  {} ({})", short_home(path), size_text(*size)))
            .collect(),
        question: format!(
            "drop {} file{s} ({}) from the history? Undo will not bring them back; they go to the trash of the system",
            found.len(),
            size_text(total)
        ),
        done: format!(
            "dropped {} file{s} ({}) from the history",
            found.len(),
            size_text(total)
        ),
        size: size_text(total),
    }
}

impl History {
    /// Forget this place; says what is left to do (see `Removal`). With `dry_run` nothing is
    /// removed.
    pub fn remove_place(&mut self) -> Result<Removal> {
        if !self.project.exists() {
            return Ok(Removal::NoHistory);
        }
        let heads: Vec<String> = self.project.heads().into_iter().map(|h| h.place).collect();
        let others: Vec<String> = heads
            .iter()
            .filter(|p| **p != self.place)
            .cloned()
            .collect();
        if !others.is_empty() && !heads.contains(&self.place) {
            return Ok(Removal::NoPlace {
                place: self.place.clone(),
                others,
            });
        }
        if !others.is_empty() {
            if !self.dry_run {
                self.project.remove_head(&self.place)?;
            }
            return Ok(Removal::Place {
                place: self.place.clone(),
                others,
            });
        }
        Ok(Removal::Project {
            steps: self.project.states().len(),
        })
    }

    /// Delete the project of the library (after the confirmation).
    pub fn remove_project(&mut self) {
        self.project.remove();
    }
}

/// `papers history --remove ID`: the project of that id in the shared store
/// or a separate store (the first found), and the store it is in.
pub fn find_project(setup: &Setup, id: &str) -> Result<(Store, Project)> {
    for store in setup.all_stores() {
        if store.project_ids().iter().any(|i| i == id) {
            let project = store.project(id)?;
            return Ok((store, project));
        }
    }
    Err(Error::exit(format!(
        "no history of a library {id} (see `papers history --all`)"
    )))
}

/// After the removal of a project: the store is pruned.
pub fn prune_after_removal(setup: &Setup, store: &Store, trash: &dyn Fn(&str)) {
    prune_store(setup, store, trash);
}
