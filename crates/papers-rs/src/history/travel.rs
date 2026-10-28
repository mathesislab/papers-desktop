//! Moving in the history: undo, redo, restore, with the steps of the pool
//! that went with the library's (spec/history.md, sections 6 and 7).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::files_json::{FILES_JSON, parse as parse_files_json};
use super::ops::absolute_ops;
use super::project::{Head, PoolLink, Project, State};
use super::recorder::History;
use super::restore::{Deleter, Restore};
use super::setup::read_bibtex;
use super::store::sha256;
use crate::bib::encoding::parse_file_relative;
use crate::bib::{Bib, read_file};
use crate::config::files::write_atomic;
use crate::config::paths::{dirname, isfile, join, normpath, realpath, relpath, short_home};
use crate::config::{Config, config_root};
use crate::error::{Error, Result};
use crate::lock::{self, Lock};
use crate::log;

/// The files that the entries of a bibtex text link to (normalized absolute
/// paths); the `file` fields are relative to `libdir`.
pub fn entry_links(text: &str, libdir: &str) -> BTreeSet<String> {
    let mut links = BTreeSet::new();
    for entry in Bib::parse(text).entries() {
        if let Some(file) = entry.get("file") {
            for path in parse_file_relative(file, Path::new(libdir)) {
                links.insert(normpath(&path.to_string_lossy()));
            }
        }
    }
    links
}

/// The files that the entries of a bibtex file link to; none when the file
/// does not exist.
pub fn bibtex_links(bibtex: &str, libdir: &str) -> BTreeSet<String> {
    match read_file(Path::new(bibtex)) {
        Ok(text) => entry_links(&text, libdir),
        Err(_) => BTreeSet::new(),
    }
}

/// The result of undo, redo and restore.
#[derive(Debug, Clone)]
pub struct Moved {
    /// The state of the library now (with `dry_run`: where it would go).
    pub target: u64,
    /// The report on the files; `None` with `dry_run`.
    pub files: Option<Vec<String>>,
    /// The lines on the pools: the steps that followed, and the pools that did not.
    pub pools: Vec<String>,
}

/// A pool whose steps go with those of the library.
struct Pool {
    history: History,
    shown: String,
    head: Head,
    /// Where the pool is, once the steps taken so far are.
    at: u64,
    /// The steps undone (redone) in the pool.
    taken: Vec<u64>,
    /// The steps to revert in the pool (it moved on since).
    reverts: Vec<u64>,
    lock: Option<Lock>,
}

impl History {
    /// The history of the library of `config`, for undo, redo and restore:
    /// the identifier is made when there is none. An error without bibtex.
    pub fn require(config: &mut Config, dry_run: bool) -> Result<History> {
        History::open(config, true, dry_run)?
            .ok_or_else(|| Error::exit("no history: the library has no bibtex"))
    }

    /// The line of a state: number, command, time, and the library that made
    /// it (a step of a pool).
    pub fn describe(&self, n: u64) -> String {
        describe(&self.project, n)
    }

    fn text(&self, state: &State) -> Result<String> {
        Ok(self.store.text(&state.bibtex_pieces())?)
    }

    /// The checksums of the files of a state (`files.json` of the state).
    fn hashes(&self, state: &State) -> Result<BTreeMap<String, String>> {
        match state.file(FILES_JSON) {
            Some(pieces) if !pieces.is_empty() => Ok(parse_files_json(
                &self.store.text(&pieces)?,
                &self.setup.libdir,
            )),
            _ => Ok(BTreeMap::new()),
        }
    }

    /// Bring the library from the state `cur` to the state `target`: the
    /// files (command by command, then by checksum), the bibtex and the
    /// `.papers/` files. `pool_dirs`: the folders whose files are copied
    /// back, not moved (by default, the files directory of the pool of a
    /// shallow library); `shared`: the files that another library links to,
    /// also copied back. The lock of the library is held by the caller and
    /// checked here. The lines of the report on the files.
    pub fn go_to(
        &self,
        cur: u64,
        target: u64,
        pool_dirs: Option<&[String]>,
        shared: &BTreeSet<String>,
        lock: Option<&Lock>,
    ) -> Result<Vec<String>> {
        if let Some(lock) = lock {
            lock.check()?;
        }
        let libdir = self.setup.libdir.clone();
        let project = &self.project;
        let state = |n: u64| -> Result<&State> {
            project.state(n).ok_or_else(|| {
                Error::exit(format!(
                    "no step {n} in the history of the library (see `papers history`)"
                ))
            })
        };
        let default_dirs: Vec<String> = self.setup.pool_files_dir.iter().cloned().collect();
        let pool_dirs = pool_dirs.unwrap_or(&default_dirs);
        let deleter = Deleter {
            keep: self.setup.keep_deleted_files,
            store: Some(&self.store),
            trash: &self.trash,
        };
        let (back, forward) = project.path_between(cur, target);
        let mut lines = Vec::new();
        let links =
            |s: &State| -> Result<BTreeSet<String>> { Ok(entry_links(&self.text(s)?, &libdir)) };
        let run = |current: &BTreeSet<String>,
                   goal: &BTreeSet<String>,
                   ops: &[super::ops::FileOp],
                   undo: bool,
                   hashes: &BTreeMap<String, String>|
         -> Result<Vec<String>> {
            Restore {
                current,
                target: goal,
                ops,
                undo,
                hashes,
                pool_dirs,
                shared,
                store: &self.store,
                deleter: &deleter,
                cwd: &self.setup.cwd,
            }
            .run(&mut None)
        };
        for n in &back {
            let s = state(*n)?;
            let Some(parent) = s.parent().and_then(|p| project.state(p)) else {
                break;
            };
            let ops = absolute_ops(&s.ops(), &libdir);
            if !ops.is_empty() {
                lines.extend(run(
                    &links(s)?,
                    &links(parent)?,
                    &ops,
                    true,
                    &self.hashes(parent)?,
                )?);
            }
        }
        for n in &forward {
            let s = state(*n)?;
            let ops = absolute_ops(&s.ops(), &libdir);
            if let Some(parent) = s.parent().and_then(|p| project.state(p))
                && !ops.is_empty()
            {
                lines.extend(run(
                    &links(parent)?,
                    &links(s)?,
                    &ops,
                    false,
                    &self.hashes(s)?,
                )?);
            }
        }
        let goal = state(target)?;
        let current = bibtex_links(&self.setup.bibtex, &libdir);
        lines.extend(run(
            &current,
            &links(goal)?,
            &[],
            forward.is_empty(),
            &self.hashes(goal)?,
        )?);

        write_atomic(&self.setup.bibtex, &self.text(goal)?)?;
        self.write_library_files(cur, target)?;
        Ok(lines)
    }

    /// The files of `.papers/` of the state `target`; those of the state
    /// `cur` that `target` has not are removed (a file no state had is left
    /// alone).
    fn write_library_files(&self, cur: u64, target: u64) -> Result<()> {
        let libdir = &self.setup.libdir;
        // the name stays inside the directory of the bibtex (as papers tests it: by its start)
        let inside = |name: &str| !relpath(&join(libdir, name), libdir).starts_with("..");
        let files = self
            .project
            .state(target)
            .and_then(State::files)
            .unwrap_or_default();
        for (name, pieces) in &files {
            if inside(name) {
                let path = join(libdir, name);
                fs::create_dir_all(dirname(&path))?;
                write_atomic(&path, &self.store.text(pieces)?)?;
            }
        }
        for (name, _) in self
            .project
            .state(cur)
            .and_then(State::files)
            .unwrap_or_default()
        {
            let path = join(libdir, &name);
            if !files.iter().any(|(n, _)| *n == name) && inside(&name) && isfile(&path) {
                log::info(&format!("remove {path}: not in the restored state"));
                fs::remove_file(&path)?;
            }
        }
        Ok(())
    }

    // the pools
    // ---------

    /// How the messages name a pool: as the `pool` setting of the library
    /// writes it, else with `~`.
    fn shown(&self, pool_config: &Config) -> String {
        let root = pool_config
            .file
            .as_deref()
            .map(config_root)
            .unwrap_or_default();
        match &self.setup.pool {
            Some((path, label)) if realpath(path) == realpath(&root) => label.clone(),
            _ => short_home(&root),
        }
    }

    /// The pool of a link, or a message: not found, or its bibtex changed
    /// outside papers since its last step. The pool is locked, so that no
    /// other command writes it until the end of the undo.
    fn open_pool(
        &self,
        id: &str,
        link: &PoolLink,
        command: &str,
    ) -> Result<std::result::Result<Pool, String>> {
        let file = link.config.clone();
        let mut pool_config = if !file.is_empty() && isfile(&file) {
            Config::load(&file, self.setup.machine.clone(), None).ok()
        } else {
            None
        };
        let mut history = match pool_config.as_mut() {
            Some(c) => History::open(c, false, false)?,
            None => None,
        };
        let mut lock = None;
        if let (Some(h), Some(c)) = (&history, &pool_config)
            && h.project.id == id
        {
            lock = lock::acquire(c, command, None)?;
        }
        let missing = || {
            format!(
                "the pool of step {} is not found (at {})",
                link.state,
                short_home(&file)
            )
        };
        let (Some(mut history), Some(pool_config)) = (history.take(), pool_config) else {
            return Ok(Err(missing()));
        };
        history.project.reload(); // after the lock
        let Some(head) = history.head().filter(|_| history.project.id == id) else {
            return Ok(Err(missing()));
        };
        let shown = self.shown(&pool_config);
        let current = sha256(read_bibtex(&history.setup.bibtex)?.as_bytes());
        if history
            .project
            .state(head.state)
            .and_then(State::bibtex_sha)
            != Some(current.as_str())
        {
            return Ok(Err(format!(
                "pool {shown}: changed outside papers since its last step"
            )));
        }
        Ok(Ok(Pool {
            at: head.state,
            history,
            shown,
            head,
            taken: vec![],
            reverts: vec![],
            lock,
        }))
    }

    /// The steps of the pools that go with the steps of the library undone
    /// (undo) or redone, `steps` in the order they are taken. A pool still at
    /// the step linked to a library step (for redo: where the undo left it)
    /// takes it back (does it again); once it moved on, the linked steps are
    /// to be reverted (for redo: the reverts made by the undo). The pools
    /// that have steps, and the lines of the pools not found.
    fn pool_steps(
        &self,
        steps: &[u64],
        undo: bool,
        command: &str,
    ) -> Result<(Vec<Pool>, Vec<String>)> {
        let mut pools: Vec<(String, Pool)> = Vec::new();
        let mut lines = Vec::new();
        let mut gone: BTreeSet<String> = BTreeSet::new();
        for s in steps {
            let Some(state) = self.project.state(*s) else {
                continue;
            };
            for (id, link) in state.pools() {
                if gone.contains(&id) {
                    continue;
                }
                if !pools.iter().any(|(i, _)| *i == id) {
                    match self.open_pool(&id, &link, command)? {
                        Ok(pool) => pools.push((id.clone(), pool)),
                        Err(message) => {
                            lines.push(format!("{message}: the library only"));
                            gone.insert(id);
                            continue;
                        }
                    }
                }
                let Some((_, pool)) = pools.iter_mut().find(|(i, _)| *i == id) else {
                    continue;
                };
                let n = link.state;
                let Some(pool_state) = pool.history.project.state(n) else {
                    continue;
                };
                let Some(parent) = pool_state.parent() else {
                    continue;
                }; // no longer in the history of the pool
                let (before, after) = if undo { (n, parent) } else { (parent, n) };
                if pool.at == after {
                    continue; // there already
                }
                if pool.at == before && pool.reverts.is_empty() {
                    pool.taken.push(n);
                    pool.at = after;
                } else if undo {
                    pool.reverts.push(n);
                } else {
                    let project = &pool.history.project;
                    let revert = project
                        .line(pool.at)
                        .into_iter()
                        .find(|r| project.state(*r).is_some_and(|s| s.reverts().contains(&n)));
                    if let Some(r) = revert {
                        pool.reverts.push(r);
                    }
                }
            }
        }
        let pools = pools
            .into_iter()
            .map(|(_, p)| p)
            .filter(|p| !p.taken.is_empty() || !p.reverts.is_empty())
            .collect();
        Ok((pools, lines))
    }

    /// Bring the pools to the steps found by `pool_steps`; the lines of the
    /// report. The pools that must revert steps (they moved on) are reported
    /// only: papers-rs does not revert yet.
    fn move_pools(pools: &mut [Pool], undo: bool, kind: &str) -> Result<Vec<String>> {
        let mut lines = Vec::new();
        for pool in pools {
            if !pool.taken.is_empty() {
                let cur = pool.head.state;
                let file_lines =
                    pool.history
                        .go_to(cur, pool.at, None, &BTreeSet::new(), pool.lock.as_ref())?;
                let redo: Vec<u64> = if undo {
                    pool.taken
                        .iter()
                        .rev()
                        .chain(pool.head.redo.iter())
                        .copied()
                        .collect()
                } else {
                    pool.head
                        .redo
                        .iter()
                        .filter(|n| !pool.taken.contains(n))
                        .copied()
                        .collect()
                };
                pool.history
                    .project
                    .set_head(&pool.history.place, pool.at, &redo)?;
                lines.push(format!(
                    "pool {}: also {kind}, restored to {}",
                    pool.shown,
                    pool.history.describe(pool.at)
                ));
                lines.extend(file_lines);
            }
            if !pool.reverts.is_empty() {
                let steps: Vec<String> = pool.reverts.iter().map(u64::to_string).collect();
                lines.push(format!(
                    "pool {}: changed since; step{} {} not reverted there (papers-rs does not revert yet)",
                    pool.shown,
                    if steps.len() > 1 { "s" } else { "" },
                    steps.join(", ")
                ));
            }
        }
        Ok(lines)
    }

    // undo, redo, restore
    // -------------------

    /// Go back `steps` states, the pool too for the steps that wrote it
    /// (unless `library_only`). The lock of the library is held by the
    /// caller; `command` names the command for the locks of the pools.
    pub fn undo(
        &mut self,
        steps: usize,
        library_only: bool,
        lock: Option<&Lock>,
        command: &str,
    ) -> Result<Moved> {
        let cur = if self.dry_run {
            self.head().map(|h| h.state)
        } else {
            self.sync_head(None)?
        };
        let line = cur.map(|c| self.project.line(c)).unwrap_or_default();
        if line.len() <= steps {
            return Err(Error::exit(if line.len() <= 1 {
                "nothing to undo".to_string()
            } else {
                format!("only {} step(s) to undo", line.len() - 1)
            }));
        }
        let (cur, target) = (line[0], line[steps]);
        if self.dry_run {
            return Ok(Moved {
                target,
                files: None,
                pools: vec![],
            });
        }
        let (mut pools, missing) = if library_only {
            (vec![], vec![])
        } else {
            self.pool_steps(&line[..steps], true, command)?
        };
        let previous = self.head().map(|h| h.redo).unwrap_or_default();
        let redo: Vec<u64> = line[..steps]
            .iter()
            .rev()
            .chain(previous.iter())
            .copied()
            .collect();
        // the pools first: the files the command moved to a pool are moved back, unless the pool still links them
        let pool_lines = Self::move_pools(&mut pools, true, "undone")?;
        let shared: BTreeSet<String> = pools
            .iter()
            .flat_map(|p| bibtex_links(&p.history.setup.bibtex, &p.history.setup.libdir))
            .collect();
        let none: [String; 0] = [];
        let pool_dirs = if !pools.is_empty() && missing.is_empty() {
            Some(&none[..])
        } else {
            None
        };
        let files = self.go_to(cur, target, pool_dirs, &shared, lock)?;
        self.project.set_head(&self.place, target, &redo)?;
        let pools = missing.into_iter().chain(pool_lines).collect();
        Ok(Moved {
            target,
            files: Some(files),
            pools,
        })
    }

    /// Go forward `steps` states (see `undo`).
    pub fn redo(
        &mut self,
        steps: usize,
        library_only: bool,
        lock: Option<&Lock>,
        command: &str,
    ) -> Result<Moved> {
        let Some(head) = self.head().filter(|h| !h.redo.is_empty()) else {
            return Err(Error::exit("nothing to redo"));
        };
        let cur = head.state;
        if !self.dry_run && self.sync_head(None)? != Some(cur) {
            return Err(Error::exit(
                "nothing to redo: the library changed outside papers since the undo (recorded: `papers undo` goes back before that change)",
            ));
        }
        if head.redo.len() < steps {
            return Err(Error::exit(format!(
                "only {} step(s) to redo",
                head.redo.len()
            )));
        }
        let target = head.redo[steps - 1];
        if self.dry_run {
            return Ok(Moved {
                target,
                files: None,
                pools: vec![],
            });
        }
        let (mut pools, missing) = if library_only {
            (vec![], vec![])
        } else {
            self.pool_steps(&head.redo[..steps], false, command)?
        };
        // the library first: the files the command moved to the pool are there when the pool comes back
        let files = self.go_to(cur, target, None, &BTreeSet::new(), lock)?;
        self.project
            .set_head(&self.place, target, &head.redo[steps..])?;
        let pool_lines = Self::move_pools(&mut pools, false, "redone")?;
        Ok(Moved {
            target,
            files: Some(files),
            pools: missing.into_iter().chain(pool_lines).collect(),
        })
    }

    /// Go to the state `n` of the project of the library; without `n`, to the
    /// current state (the changes made outside papers since are undone, and
    /// recorded: redo brings them back). The pools are not touched.
    pub fn restore(&mut self, n: Option<u64>, lock: Option<&Lock>) -> Result<Moved> {
        let head = self.head();
        let target = match n {
            None => head
                .map(|h| h.state)
                .ok_or_else(|| Error::exit("no history: nothing to restore"))?,
            Some(n) if self.project.state(n).is_none() => {
                return Err(Error::exit(format!(
                    "no step {n} in the history of the library (see `papers history`)"
                )));
            }
            Some(n) => n,
        };
        if self.dry_run {
            return Ok(Moved {
                target,
                files: None,
                pools: vec![],
            });
        }
        let cur = self
            .sync_head(None)?
            .ok_or_else(|| Error::exit("no history: nothing to restore"))?;
        if cur == target {
            return Err(Error::exit(format!(
                "nothing to restore: {} is step {target}",
                crate::config::paths::basename(&self.setup.bibtex)
            )));
        }
        let files = self.go_to(cur, target, None, &BTreeSet::new(), lock)?;
        let (back, forward) = self.project.path_between(cur, target);
        let redo: Vec<u64> = if forward.is_empty() {
            back.into_iter().rev().collect()
        } else {
            vec![]
        };
        self.project.set_head(&self.place, target, &redo)?;
        Ok(Moved {
            target,
            files: Some(files),
            pools: vec![],
        })
    }

    // the files kept by the history
    // -----------------------------

    /// The bytes of the files that the state `n` keeps (the files its
    /// command deleted, still in the history).
    pub fn kept_size(&self, n: u64) -> u64 {
        kept_size(&self.project, &self.store, n)
    }

    /// The files kept by the history that `target` names: a step number (the
    /// files its command deleted), or a path (the file deleted from that
    /// place, by any step). `{checksum: (path before the deletion, bytes)}`.
    pub fn files_to_drop(
        &self,
        step: Option<u64>,
        path: Option<&str>,
    ) -> Result<BTreeMap<String, (String, u64)>> {
        let mut found = BTreeMap::new();
        let chosen: Vec<u64> = match step {
            Some(n) if self.project.state(n).is_none() => {
                return Err(Error::exit(format!(
                    "no step {n} in the history of the library (see `papers history`)"
                )));
            }
            Some(n) => vec![n],
            None => self.project.states().keys().copied().collect(),
        };
        let wanted =
            path.map(|p| normpath(&join(&self.setup.cwd, &crate::config::paths::expanduser(p))));
        for n in chosen {
            let Some(state) = self.project.state(n) else {
                continue;
            };
            let kept = state.deleted();
            for op in absolute_ops(&state.ops(), &self.setup.libdir) {
                let Some(digest) = op.checksum.clone() else {
                    continue;
                };
                if op.kind != super::ops::OpKind::Trashed || !kept.contains(&digest) {
                    continue;
                }
                if wanted.as_ref().is_some_and(|w| normpath(&op.src) != *w) {
                    continue;
                }
                if let Some(stored) = self.store.file_object(&digest) {
                    let size = fs::metadata(&stored).map(|m| m.len()).unwrap_or(0);
                    found.insert(digest, (op.src.clone(), size));
                }
            }
        }
        Ok(found)
    }

    /// Remove files from the history: no state refers to them any more (their
    /// steps stay: undo says they are no longer in the history), and they go
    /// to the system trash, unless another library of the store keeps them.
    pub fn drop_files(&mut self, digests: &BTreeSet<String>) -> Result<()> {
        let trash = self.trash;
        let numbers: Vec<u64> = self.project.states().keys().copied().collect();
        for n in numbers {
            let state = self.project.states()[&n].clone();
            let deleted = state.deleted();
            let kept: Vec<String> = deleted
                .iter()
                .filter(|d| !digests.contains(*d))
                .cloned()
                .collect();
            if kept != deleted {
                let mut state = state;
                state.set_deleted(&kept);
                self.project.update(n, state)?;
            }
        }
        let used: BTreeSet<String> = self
            .store
            .project_ids()
            .iter()
            .filter_map(|id| self.store.project(id).ok())
            .flat_map(|p| p.deleted_refs())
            .collect();
        for digest in digests {
            if let Some(path) = self.store.file_object(digest)
                && !used.contains(digest)
            {
                trash(&path);
            }
        }
        Ok(())
    }
}

/// The bytes of the files that the state `n` keeps.
pub fn kept_size(project: &Project, store: &super::store::Store, n: u64) -> u64 {
    let Some(state) = project.state(n) else {
        return 0;
    };
    state
        .deleted()
        .iter()
        .filter_map(|d| store.file_object(d))
        .filter_map(|p| fs::metadata(p).ok())
        .map(|m| m.len())
        .sum()
}

/// `time.strftime('%Y-%m-%d %H:%M', time.localtime(t))`.
pub fn when(t: f64) -> String {
    let secs = t as libc::time_t;
    // SAFETY: localtime_r fills the tm it is given, from a time_t it reads
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&secs, &mut tm).is_null() };
    if !ok {
        return String::new();
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

/// The line of a state: `<number> <message> (<local time>[, from <from>])`.
pub fn describe(project: &Project, n: u64) -> String {
    let Some(state) = project.state(n) else {
        return format!("{n}  (not found)");
    };
    let origin = state
        .from()
        .map(|f| format!(", from {}", short_home(f)))
        .unwrap_or_default();
    format!("{n} {} ({}{origin})", state.message(), when(state.time()))
}
