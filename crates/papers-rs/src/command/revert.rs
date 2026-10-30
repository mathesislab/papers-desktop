//! `papers history --revert N`: take back the changes of step N of the history of a library
//! alone, as a new step (`papers.revert`, spec `history.md` section 8). The same revert takes
//! back a step of a pool when `papers undo` or `redo` finds that the pool moved on since
//! (`history::travel`).
//!
//! Step N goes from the state of its parent P to the state N. For each entry that step N changed
//! (matched by key, or by a former key of the `ids` field for a key that step N changed), the
//! three versions are compared: the entry at P (*before*), at N (*after*), and now.
//!
//! - added by N: removed, unless it changed since (kept, with a line);
//! - removed by N: added back as it was at P (with its links), unless it is there already;
//! - modified by N: a three-way merge as `pull` does ([`Merge::exchange`]): base the entry at N,
//!   incoming the entry at P, target the entry now. A field that N changed and that did not change
//!   since goes back to its value at P; a field changed since N too is a conflict, decided by the
//!   [`Resolve`] (`--on-duplicate`; skipped: the entry stays as it is). The key goes back to that
//!   of P if N changed it and it did not change since. The file links and the tags (`keywords`) are
//!   taken one by one: those added by N are removed, those removed by N come back; the tags added
//!   since stay, without conflict.
//!
//! Then the files go back as undo moves them ([`Restore`], with the file operations of step N
//! only). The pairs of `distinct.json` that N added are removed, those it removed come back. The
//! origin records of the exchanges are left as they are: the next pull or push compares with them.
//!
//! A step is named by its identifier or a start of it (4 characters or more). With
//! [`Revert::forward`] the step is done *again* instead (redo in a pool whose step the undo took
//! back, and that changed since): the same rules from the state of the step to its parent, with
//! `added` and `removed` swapped in the lines. [`Revert::parent`] is the state the step is
//! compared with, instead of its parent (a pool that a command saved several times: its state
//! before the first save).
//!
//! [`revert`] changes a [`Library`] in memory and the files, and does not save;
//! [`Session::revert`](super::Session::revert) is the command (the caller then marks the step:
//! [`Session::mark_reverts`](super::Session::mark_reverts), and commits).

use std::collections::BTreeSet;

use serde_json::Value;

use crate::bib::encoding::parse_keywords;
use crate::bib::{Bib, Entry};
use crate::config::Config;
use crate::config::json::loads;
use crate::config::paths::{normpath, realpath};
use crate::entry::{self, val};
use crate::error::{Error, Result};
use crate::history::ops::absolute_ops;
use crate::history::project::{LinkedState, short};
use crate::history::restore::{Deleter, Restore};
use crate::history::travel::{STATE_FILE_DAMAGED, damaged};
use crate::history::{FileOp, History, State, StateId};
use crate::identity::{DISTINCT_FILE, Kind, Verdict};
use crate::library::Library;
use crate::merge::{Context, Decision, Merge, Resolve, apply_entry};

/// The entries of the bibtex of the state `n`; an error naming the step if it cannot be read.
fn state_entries(history: &History, n: &str, state: &State) -> Result<Vec<Entry>> {
    let text = history
        .store
        .text(&state.bibtex_pieces())
        .map_err(|e| damaged(n, e))?;
    Ok(Bib::parse(&text).entries().cloned().collect())
}

/// The pairs of `distinct.json` in the state `n`; an error naming the step if it cannot be read.
fn distinct_pairs(history: &History, n: &str, state: &State) -> Result<Vec<[Value; 2]>> {
    let Some(pieces) = state.file(DISTINCT_FILE).filter(|p| !p.is_empty()) else {
        return Ok(Vec::new());
    };
    let text = history.store.text(&pieces).map_err(|e| damaged(n, e))?;
    Ok(match loads(&text) {
        Ok(Value::Object(map)) => match map.get("distinct") {
            Some(Value::Array(pairs)) => pairs
                .iter()
                .filter_map(|p| match p.as_array().map(Vec::as_slice) {
                    Some([a, b]) => Some([a.clone(), b.clone()]),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    })
}

/// The former keys of an entry, in lower case.
fn ids_of(e: &Entry) -> Vec<String> {
    entry::ids(e).iter().map(|i| i.to_lowercase()).collect()
}

/// Two entries have the same content (`entry_content_equal`): the type, the key and the values of
/// the fields as written, but the fields of `skip`.
fn content_equal(a: &Entry, b: &Entry, skip: &[&str]) -> bool {
    use std::collections::BTreeMap;
    let content = |e: &Entry| -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        map.insert("ENTRYTYPE".to_string(), e.entry_type.clone());
        map.insert("ID".to_string(), e.key.clone());
        for f in &e.fields {
            map.insert(f.key.clone(), e.get(&f.key).unwrap_or("").to_string());
        }
        for k in skip {
            map.remove(*k);
        }
        map
    };
    content(a) == content(b)
}

/// The entries that a step changed: `(entry before, entry after)`, none for an entry added or
/// removed; an entry whose key the step changed is matched by the `ids` field of the entry after
/// (papers keeps former keys there).
pub fn changes(before: &[Entry], after: &[Entry]) -> Vec<(Option<Entry>, Option<Entry>)> {
    // dictionaries by key without case: a repeated key replaces the entry, in the first place
    fn by_key(entries: &[Entry]) -> Vec<(String, &Entry)> {
        let mut found: Vec<(String, &Entry)> = Vec::new();
        for e in entries {
            let key = e.key.to_lowercase();
            match found.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = e,
                None => found.push((key, e)),
            }
        }
        found
    }
    let (old, new) = (by_key(before), by_key(after));
    let lookup = |list: &[(String, &Entry)], key: &str| -> Option<Entry> {
        list.iter()
            .find(|(k, _)| k == key)
            .map(|(_, e)| (*e).clone())
    };
    let mut pairs = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (key, e) in &new {
        let counterpart = match lookup(&old, key) {
            Some(p) => Some(p),
            None => ids_of(e)
                .iter()
                .find(|k| lookup(&old, k).is_some() && lookup(&new, k).is_none())
                .and_then(|k| lookup(&old, k)),
        };
        match counterpart {
            Some(p) => {
                seen.insert(p.key.to_lowercase());
                if !content_equal(&p, e, &[]) {
                    pairs.push((Some(p), Some((*e).clone())));
                }
            }
            None => pairs.push((None, Some((*e).clone()))),
        }
    }
    for (key, p) in &old {
        if !seen.contains(key) {
            pairs.push((Some((*p).clone()), None));
        }
    }
    pairs
}

/// The key of the entry of the library now for the entry of a state: the entry with its key, else
/// the one that has its key among its former keys.
fn current(library: &Library, e: &Entry) -> Option<String> {
    let key = e.key.to_lowercase();
    library
        .entries()
        .find(|x| x.key.to_lowercase() == key)
        .or_else(|| library.entries().find(|x| ids_of(x).contains(&key)))
        .map(|x| x.key.clone())
}

/// The file links of `now` (absolute paths) without those that step N added, with those it
/// removed. `before` and `after` are entries of states, whose links are relative to the folder of
/// the bibtex (`libdir`).
fn revert_links(
    library: &Library,
    now: &Entry,
    before: &Entry,
    after: &Entry,
    libdir: &str,
) -> Vec<String> {
    let real =
        |files: &[String]| -> BTreeSet<String> { files.iter().map(|f| realpath(f)).collect() };
    let old = entry::files(before, Some(libdir));
    let new = entry::files(after, Some(libdir));
    let added: BTreeSet<String> = real(&new).difference(&real(&old)).cloned().collect();
    let mut files: Vec<String> = entry::files(now, Some(&library.relative_to))
        .into_iter()
        .filter(|f| !added.contains(&realpath(f)))
        .collect();
    let blocked: BTreeSet<String> = real(&new).union(&real(&files)).cloned().collect();
    files.extend(old.into_iter().filter(|f| !blocked.contains(&realpath(f))));
    files
}

/// The tags of `now` without those that step N added, with those it removed (without case).
fn revert_tags(now: &Entry, before: &Entry, after: &Entry) -> Vec<String> {
    let lower =
        |tags: &[String]| -> BTreeSet<String> { tags.iter().map(|t| t.to_lowercase()).collect() };
    let (old, new) = (
        parse_keywords(val(before, "keywords")),
        parse_keywords(val(after, "keywords")),
    );
    let added: BTreeSet<String> = lower(&new).difference(&lower(&old)).cloned().collect();
    let mut tags: Vec<String> = parse_keywords(val(now, "keywords"))
        .into_iter()
        .filter(|t| !added.contains(&t.to_lowercase()))
        .collect();
    let blocked: BTreeSet<String> = lower(&new).union(&lower(&tags)).cloned().collect();
    tags.extend(
        old.into_iter()
            .filter(|t| !blocked.contains(&t.to_lowercase())),
    );
    tags
}

/// A copy of the entry without its tags (they are merged one by one, see [`revert_tags`]).
fn without_tags(e: &Entry) -> Entry {
    let mut e = e.clone();
    e.remove("keywords");
    e
}

/// How a step is taken back (see the module).
#[derive(Debug, Clone, Default)]
pub struct Revert {
    /// Do the step again instead (the same rules, `before` and `after` swapped).
    pub forward: bool,
    /// The step need not be in the current history of the library (a pool, whose step was undone
    /// or done again by a step of another library).
    pub anywhere: bool,
    /// The state that the step is compared with, instead of its parent.
    pub parent: Option<StateId>,
}

/// Take back step `n` (an identifier) in `library` (loaded): the entries, the files and
/// `distinct.json`; not saved. The lines of the report: the entries left as they are and why, the
/// files. With `--dry-run` (`history.dry_run`) no file is touched. The operations on the files go
/// to `record` when it is given (a revert records them in its step).
pub fn revert(
    history: &mut History,
    library: &mut Library,
    n: &str,
    resolver: &mut dyn Resolve,
    record: &mut Option<Vec<FileOp>>,
    how: &Revert,
) -> Result<Vec<String>> {
    let head = if history.dry_run {
        history.head().map(|h| h.state)
    } else {
        history.sync_head(None)?
    };
    let n = history.step(n)?;
    let n = n.as_str();
    let state = history
        .project
        .state(n)
        .cloned()
        .ok_or_else(|| crate::history::travel::no_step(n))?;
    if !how.anywhere && !head.is_some_and(|h| history.project.line(&h).iter().any(|l| l == n)) {
        return Err(Error::exit(format!(
            "step {} is not in the current history of the library: `papers restore {}` goes there",
            short(n),
            short(n)
        )));
    }
    let parent = how.parent.clone().or_else(|| state.parent());
    if let Some(parent) = parent.as_deref().filter(|p| history.project.is_damaged(p)) {
        return Err(damaged(parent, STATE_FILE_DAMAGED));
    }
    let parent_state = parent
        .as_deref()
        .and_then(|p| Some((p.to_string(), history.project.state(p)?.clone())));
    let Some((parent, parent_state)) = parent_state else {
        return Err(Error::exit(format!(
            "step {} is the first step of the history: nothing before it to go back to",
            short(n)
        )));
    };
    let n_short = short(n);
    // what the revert reads from the history, read before anything changes (spec/history.md 2.9)
    let (mut before, mut after) = (
        state_entries(history, &parent, &parent_state)?,
        state_entries(history, n, &state)?,
    );
    let (parent_pairs, pairs) = (
        distinct_pairs(history, &parent, &parent_state)?,
        distinct_pairs(history, n, &state)?,
    );
    let hashes = history.hashes(if how.forward { n } else { &parent })?;
    if !how.forward {
        for digest in state.deleted() {
            history
                .store
                .check_file(&digest)
                .map_err(|e| damaged(n, e))?;
        }
    }
    // what step n did, as the lines say it (done again: an entry added by n is taken as removed, and so on)
    let (added, removed) = if how.forward {
        ("removed", "added")
    } else {
        ("added", "removed")
    };
    if how.forward {
        std::mem::swap(&mut before, &mut after);
    }
    let libdir = history.setup.libdir.clone();
    let links = |library: &Library| -> BTreeSet<String> {
        library
            .entries()
            .flat_map(|x| entry::files(x, Some(&library.relative_to)))
            .map(|f| normpath(&f))
            .collect()
    };
    let current_links = links(library);
    let mut lines = Vec::new();
    let verdict = Verdict {
        kind: Kind::Same,
        reason: format!("step {n_short}"),
    };
    for (p, e) in changes(&before, &after) {
        match (p, e) {
            (None, Some(e)) => {
                // added by N
                let Some(key) = current(library, &e) else {
                    continue;
                };
                let now = library.get(&key).cloned().expect("an entry of the library");
                if content_equal(&now, &e, &["file"]) {
                    library.remove(&key);
                } else {
                    lines.push(format!(
                        "{key}: {added} by step {n_short} and changed since: kept (`papers rm {key}` removes it)"
                    ));
                }
            }
            (Some(p), None) => {
                // removed by N: an entry now for it (back already, or another entry has its key)
                if let Some(key) = current(library, &p) {
                    let now = library.get(&key).cloned().expect("an entry of the library");
                    if !content_equal(&now, &p, &["file"]) {
                        lines.push(format!(
                            "{}: {removed} by step {n_short}; the entry {} has its key now: not added",
                            p.key, now.key
                        ));
                    }
                    continue;
                }
                let files = entry::files(&p, Some(&libdir));
                let key = p.key.clone();
                library.add_unchecked(p);
                library.set_files(&key, &files)?;
            }
            (Some(p), Some(e)) => {
                let Some(key) = current(library, &e) else {
                    lines.push(format!(
                        "{}: changed by step {n_short} and removed since: not added",
                        e.key
                    ));
                    continue;
                };
                let now = library.get(&key).cloned().expect("an entry of the library");
                let merge = Merge::exchange(
                    without_tags(&now),
                    without_tags(&p),
                    Some(&library.relative_to),
                    Some(&without_tags(&e)),
                );
                let new = if merge.conflicts().is_empty() {
                    merge.result()
                } else {
                    match resolver.decide(merge, Context::Revert, &verdict)? {
                        Decision::Merge(_, merged) => merged,
                        _ => {
                            lines.push(format!(
                                "{key}: changed since step {n_short} too: left as it is"
                            ));
                            continue;
                        }
                    }
                };
                let files = revert_links(library, &now, &p, &e, &libdir);
                let tags = revert_tags(&now, &p, &e);
                apply_entry(
                    library.get_mut(&key).expect("an entry of the library"),
                    &new,
                );
                library.set_files(&new.key, &files)?;
                let target = library.get_mut(&new.key).expect("an entry of the library");
                if tags.is_empty() {
                    target.remove("keywords");
                } else {
                    target.set("keywords", tags.join(", "));
                }
                // the key goes back to that of P if N changed it and it did not change since
                if p.key != e.key && new.key == e.key {
                    let taken = library
                        .entries()
                        .any(|x| x.key != new.key && x.key.to_lowercase() == p.key.to_lowercase());
                    if taken {
                        lines.push(format!(
                            "{}: renamed by step {n_short} (key {} before), now another entry's key: the key stays",
                            new.key, p.key
                        ));
                    } else {
                        library.set_key(&new.key, &p.key, false)?;
                        let target = library.get_mut(&p.key).expect("an entry of the library");
                        let ids: Vec<String> = val(target, "ids")
                            .split(',')
                            .map(|i| crate::bib::pystr::strip(i).to_string())
                            .filter(|i| !i.is_empty() && i.to_lowercase() != p.key.to_lowercase())
                            .collect();
                        if ids.is_empty() {
                            target.remove("ids");
                        } else {
                            target.set("ids", ids.join(", "));
                        }
                    }
                }
            }
            (None, None) => {}
        }
    }
    if how.forward {
        revert_distinct(library, &pairs, &parent_pairs);
    } else {
        revert_distinct(library, &parent_pairs, &pairs);
    }
    if history.dry_run {
        return Ok(lines);
    }
    let ops = absolute_ops(&state.ops(), &libdir);
    let goal = links(library);
    let pool_dirs: Vec<String> = history.setup.pool_files_dir.iter().cloned().collect();
    let trash = history.trash;
    let deleter = Deleter {
        keep: history.setup.keep_deleted_files,
        store: Some(&history.store),
        trash: &trash,
    };
    lines.extend(
        Restore {
            current: &current_links,
            target: &goal,
            ops: &ops,
            undo: !how.forward,
            hashes: &hashes,
            pool_dirs: &pool_dirs,
            shared: &BTreeSet::new(),
            store: &history.store,
            deleter: &deleter,
            cwd: &history.setup.cwd,
        }
        .run(record)?,
    );
    Ok(lines)
}

/// The pairs that step N added to `distinct.json` are removed, those it removed come back (two
/// pairs are the same when they have the same two sides, in either order).
fn revert_distinct(library: &mut Library, before: &[[Value; 2]], after: &[[Value; 2]]) {
    let same = |a: &[Value; 2], b: &[Value; 2]| a == b || (a[0] == b[1] && a[1] == b[0]);
    let is_in = |p: &[Value; 2], list: &[[Value; 2]]| list.iter().any(|q| same(p, q));
    let added: Vec<[Value; 2]> = after
        .iter()
        .filter(|p| !is_in(p, before))
        .cloned()
        .collect();
    let removed: Vec<[Value; 2]> = before
        .iter()
        .filter(|p| !is_in(p, after))
        .cloned()
        .collect();
    let distinct = &mut library.distinct;
    let mut kept: Vec<[Value; 2]> = distinct
        .pairs
        .iter()
        .filter(|p| !is_in(p, &added))
        .cloned()
        .collect();
    let back: Vec<[Value; 2]> = removed.into_iter().filter(|p| !is_in(p, &kept)).collect();
    kept.extend(back);
    if kept != distinct.pairs {
        distinct.pairs = kept;
        distinct.changed = true;
    }
}

/// The lines of the pools that step `n` also changed: the command that takes back their step
/// (`pool_lines`). A pool whose configuration file is gone is left out.
pub fn pool_lines(history: &History, n: &str) -> Vec<String> {
    let Some(state) = history.project.state(n) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for (_, link) in state.pools() {
        if link.config.is_empty() || !crate::config::paths::isfile(&link.config) {
            continue;
        }
        let Ok(pool_config) = Config::load(&link.config, history.setup.machine.clone(), None)
        else {
            continue;
        };
        let shown = history.shown(&pool_config);
        // a number: a link of an earlier version to a pool in another store (see `Store::upgrade`)
        let step = match &link.state {
            LinkedState::Number(number) => {
                let mut pool_config = pool_config;
                History::open(&mut pool_config, false, false)
                    .ok()
                    .flatten()
                    .and_then(|h| h.project.upgraded(&link.state))
                    .map_or(number.to_string(), |id| short(&id).to_string())
            }
            id => id.shown(),
        };
        lines.push(format!(
            "step {} also changed pool {shown} (its step {step}): `papers -C {shown} history --revert {step}` takes it back there",
            short(n)
        ));
    }
    lines
}
