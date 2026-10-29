//! Moving in the history: undo, redo, restore, with the steps of the pool
//! that went with the library's (spec/history.md, sections 6 and 7).
//!
//! A pool still where the command left it takes its step back (does it again). A pool that
//! moved on since gets a new step that *reverts* the step of the command (for redo, when the
//! undo took the step back there and the pool moved on since: that step is done *again*;
//! [`crate::command::revert`]): conflicts, an entry changed in the pool since, are decided by
//! the [`Resolve`] that `undo` and `redo` are given (papers asks in a terminal, else skips: the
//! entry stays as it is). The files that the revert places are part of the new step of the pool.
//! A command that saved the pool several times is undone in full: the link of the library step
//! has the pool's state before the first save (`base`).
//!
//! **Auto-push** (`papers undo` in a library with a pool and `auto-push`, spec 7.6) is for the
//! caller: after `undo` or `redo` without `library_only`, push what differs from the origin
//! records. The step of a pool that this command reverted is in [`Moved::links`]: papers leaves it
//! pending for the next step of the library (the push), which records it in its `pools`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::files_json::{FILES_JSON, parse as parse_files_json};
use super::ops::absolute_ops;
use super::project::{Base, Head, LinkedState, PoolLink, Project, State, StateId, short};
use super::recorder::History;
use super::restore::{Deleter, Restore};
use super::setup::{Setup, read_bibtex};
use super::store::{new_id, sha256};
use crate::bib::encoding::parse_file_relative;
use crate::bib::{Bib, read_file};
use crate::command::revert::Revert;
use crate::command::session::{Options, Session};
use crate::config::files::write_atomic;
use crate::config::paths::{
    abspath, dirname, isfile, join, normpath, realpath, relpath, short_home,
};
use crate::config::{Config, config_root};
use crate::error::{Error, Result};
use crate::lock::{self, Lock};
use crate::log;
use crate::merge::Resolve;

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
    pub target: StateId,
    /// The report on the files; `None` with `dry_run`.
    pub files: Option<Vec<String>>,
    /// The lines on the pools: the steps that followed, and the pools that did not.
    pub pools: Vec<String>,
    /// The steps that the revert of a pool step added to the history of the pool, `(project id of
    /// the pool, its step)`: the next step of the library records them in its `pools` (see the
    /// module).
    pub links: Links,
}

/// The steps that a command added to the histories of other libraries: `(project id, step)`.
pub type Links = Vec<(String, PoolLink)>;

/// What the revert of the steps of a pool gives: the lines of the reverts, and the new step of the pool
/// with the link to record, when the pool changed.
type Reverted = (Vec<String>, Option<(String, PoolLink)>);

/// A pool whose steps go with those of the library.
struct Pool {
    history: History,
    config: Config,
    shown: String,
    head: Head,
    /// Where the pool is, once the steps taken so far are.
    at: StateId,
    /// The steps undone (redone) in the pool.
    taken: Vec<StateId>,
    /// The steps to revert in the pool (it moved on since).
    reverts: Vec<StateId>,
    /// The steps to do again in the pool (redo: it moved on since the undo of the step).
    again: Vec<StateId>,
    /// For a step of `reverts` or `again`: the state it is compared with (its `base`).
    bases: BTreeMap<StateId, StateId>,
    lock: Option<Lock>,
}

impl History {
    /// The history of the library of `config`, for undo, redo and restore:
    /// the identifier is made when there is none. An error without bibtex.
    pub fn require(config: &mut Config, dry_run: bool) -> Result<History> {
        History::open(config, true, dry_run)?
            .ok_or_else(|| Error::exit("no history: the library has no bibtex"))
    }

    /// The history of the library of `config` for undo, redo and restore, which leave no trace
    /// when they have nothing to do (spec/history.md 5.3, 6.2): no identifier is made, and nothing
    /// is written. A library without identifier has no history yet: its project is empty, and
    /// every one of the three stops with an error before it writes anything (`nothing to undo`,
    /// `nothing to redo`, `no history: nothing to restore`, no step named), so that the identifier
    /// it is given here is never kept. An error without bibtex.
    pub fn for_travel(config: &mut Config, dry_run: bool) -> Result<History> {
        if let Some(history) = History::open(config, false, dry_run)? {
            return Ok(history);
        }
        let Some(bibtex) = config.bibtex.clone() else {
            return Err(Error::exit("no history: the library has no bibtex"));
        };
        let setup = Setup::from_config(config);
        let id = new_id();
        let store = setup.store(Some(&id));
        let project = store.project(&id)?;
        Ok(History {
            setup,
            store,
            project,
            place: realpath(&bibtex),
            dry_run,
            trash: super::store::to_system_trash,
        })
    }

    /// Where `sync_head` would bring the head, without writing anything: the state it would start
    /// from, and whether it would add a state on top (the start, or a change made outside papers).
    /// Undo, redo and restore with nothing to do leave no trace.
    fn synced(&mut self) -> Result<(Option<StateId>, bool)> {
        self.project.reload();
        let sha = sha256(read_bibtex(&self.setup.bibtex)?.as_bytes());
        let n = match self.head() {
            Some(head) => Some(head.state),
            None => self.project.start_state(&sha),
        };
        let added = n.as_ref().is_none_or(|n| {
            self.project.state(n).and_then(State::bibtex_sha) != Some(sha.as_str())
        });
        Ok((n, added))
    }

    /// The line of a state: identifier (short), command, time, and the library that made
    /// it (a step of a pool).
    pub fn describe(&self, n: &str) -> String {
        describe(&self.project, n)
    }

    /// The identifier of the step that `reference` names: the identifier, or a unique start of
    /// it of 4 characters or more. An error if there is none, or if several have it.
    pub fn step(&self, reference: &str) -> Result<StateId> {
        match self.project.resolve(reference) {
            Err(message) => Err(Error::exit(message)),
            Ok(None) => Err(no_step(reference)),
            Ok(Some(n)) => Ok(n),
        }
    }

    fn text(&self, state: &State) -> Result<String> {
        Ok(self.store.text(&state.bibtex_pieces())?)
    }

    /// The checksums of the files of a state (`files.json` of the state).
    pub(crate) fn hashes(&self, state: &State) -> Result<BTreeMap<String, String>> {
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
        cur: &str,
        target: &str,
        pool_dirs: Option<&[String]>,
        shared: &BTreeSet<String>,
        lock: Option<&Lock>,
    ) -> Result<Vec<String>> {
        if let Some(lock) = lock {
            lock.check()?;
        }
        let libdir = self.setup.libdir.clone();
        let project = &self.project;
        let state = |n: &str| -> Result<&State> { project.state(n).ok_or_else(|| no_step(n)) };
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
            let s = state(n)?;
            let Some(parent) = s.parent().and_then(|p| project.state(&p).cloned()) else {
                break;
            };
            let ops = absolute_ops(&s.ops(), &libdir);
            if !ops.is_empty() {
                lines.extend(run(
                    &links(s)?,
                    &links(&parent)?,
                    &ops,
                    true,
                    &self.hashes(&parent)?,
                )?);
            }
        }
        for n in &forward {
            let s = state(n)?;
            let ops = absolute_ops(&s.ops(), &libdir);
            if let Some(parent) = s.parent().and_then(|p| project.state(&p).cloned())
                && !ops.is_empty()
            {
                lines.extend(run(
                    &links(&parent)?,
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
    fn write_library_files(&self, cur: &str, target: &str) -> Result<()> {
        let libdir = &self.setup.library_dir;
        // the name stays inside the library directory (as papers tests it: by its start)
        let inside = |name: &str| !relpath(&join(libdir, name), libdir).starts_with("..");
        let files = self
            .project
            .state(target)
            .and_then(State::files)
            .unwrap_or_default();
        // the names are stored with `/`, and read back with the separator of the system
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
    pub(crate) fn shown(&self, pool_config: &Config) -> String {
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
                link.state.shown(),
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
            .state(&head.state)
            .and_then(State::bibtex_sha)
            != Some(current.as_str())
        {
            return Ok(Err(format!(
                "pool {shown}: changed outside papers since its last step"
            )));
        }
        Ok(Ok(Pool {
            at: head.state.clone(),
            history,
            config: pool_config,
            shown,
            head,
            taken: vec![],
            reverts: vec![],
            again: vec![],
            bases: BTreeMap::new(),
            lock,
        }))
    }

    /// The steps of the pools that go with the steps of the library undone
    /// (undo) or redone, `steps` in the order they are taken. A pool still at
    /// the step linked to a library step (for redo: where the undo left it)
    /// takes it back (does it again); once it moved on, the linked steps are
    /// to be reverted (for redo: the reverts made by the undo are reverted, and the steps that
    /// the undo took back are done again). The pool step of a link is compared with its `base`
    /// (the state before the first save of the command). The pools that have steps, and the
    /// lines of the pools not found.
    fn pool_steps(
        &self,
        steps: &[StateId],
        undo: bool,
        command: &str,
    ) -> Result<(Vec<Pool>, Vec<String>)> {
        let mut pools: Vec<(String, Pool)> = Vec::new();
        let mut lines = Vec::new();
        let mut gone: BTreeSet<String> = BTreeSet::new();
        for s in steps {
            let Some(state) = self.project.state(s) else {
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
                // a number: a link of an earlier version to a pool in another store
                let Some(n) = pool.history.project.upgraded(&link.state) else {
                    continue;
                };
                // the pool's state before the command (several saves of the pool: before the first one)
                let parent = match &link.base {
                    Base::Id(base) => Some(base.clone()),
                    Base::Null => None,
                    Base::Absent => pool.history.project.state(&n).and_then(State::parent),
                };
                let project = &pool.history.project;
                let Some(parent) =
                    parent.filter(|p| project.state(&n).is_some() && project.state(p).is_some())
                else {
                    continue; // no longer in the history of the pool
                };
                let (before, after) = if undo {
                    (n.clone(), parent.clone())
                } else {
                    (parent.clone(), n.clone())
                };
                if pool.at == after {
                    continue; // there already
                }
                if pool.at == before && pool.reverts.is_empty() {
                    pool.taken.push(n);
                    pool.at = after;
                } else if undo {
                    pool.bases.insert(n.clone(), parent);
                    pool.reverts.push(n);
                } else {
                    let revert = project
                        .line(&pool.at)
                        .into_iter()
                        .find(|r| project.state(r).is_some_and(|s| s.reverts().contains(&n)));
                    match revert {
                        Some(r) => {
                            // one revert of several steps (undo -n N): reverted once
                            if !pool.reverts.contains(&r) {
                                pool.reverts.push(r);
                            }
                        }
                        None => {
                            // the undo took step n back there, and the pool moved on since: done again
                            pool.bases.insert(n.clone(), parent);
                            pool.again.push(n);
                        }
                    }
                }
            }
        }
        let pools = pools
            .into_iter()
            .map(|(_, p)| p)
            .filter(|p| !p.taken.is_empty() || !p.reverts.is_empty() || !p.again.is_empty())
            .collect();
        Ok((pools, lines))
    }

    /// Bring the pools to the steps found by `pool_steps`, then revert the steps of the pools
    /// that moved on (a new step of the pool); the lines of the report, and the links of the new
    /// steps of the pools.
    fn move_pools(
        &self,
        pools: &mut [Pool],
        undo: bool,
        kind: &str,
        command: &str,
        resolver: &mut dyn Resolve,
    ) -> Result<(Vec<String>, Links)> {
        let mut lines = Vec::new();
        let mut links = Vec::new();
        for pool in pools {
            if !pool.taken.is_empty() {
                let cur = pool.head.state.clone();
                let file_lines = pool.history.go_to(
                    &cur,
                    &pool.at,
                    None,
                    &BTreeSet::new(),
                    pool.lock.as_ref(),
                )?;
                let redo: Vec<StateId> = if undo {
                    pool.taken
                        .iter()
                        .rev()
                        .chain(pool.head.redo.iter())
                        .cloned()
                        .collect()
                } else {
                    pool.head
                        .redo
                        .iter()
                        .filter(|n| !pool.taken.contains(n))
                        .cloned()
                        .collect()
                };
                pool.history
                    .project
                    .set_head(&pool.history.place, &pool.at, &redo)?;
                lines.push(format!(
                    "pool {}: also {kind}, restored to {}",
                    pool.shown,
                    pool.history.describe(&pool.at)
                ));
                lines.extend(file_lines);
            }
            if !pool.reverts.is_empty() || !pool.again.is_empty() {
                let (revert_lines, link) = self.revert_pool(pool, command, resolver)?;
                let plural = |steps: &[StateId]| {
                    format!(
                        "step{} {}",
                        if steps.len() > 1 { "s" } else { "" },
                        steps
                            .iter()
                            .map(|n| short(n))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                let mut what = Vec::new();
                let mut done = Vec::new();
                if !pool.reverts.is_empty() {
                    what.push(format!("revert of {}", plural(&pool.reverts)));
                    done.push(format!("{} reverted", plural(&pool.reverts)));
                }
                if !pool.again.is_empty() {
                    what.push(format!("{} again", plural(&pool.again)));
                    done.push(format!("{} done again", plural(&pool.again)));
                }
                match link {
                    Some((id, link)) => {
                        let LinkedState::Id(state) = &link.state else {
                            unreachable!("a link written now has an identifier")
                        };
                        lines.push(format!(
                            "pool {}: changed since, so {} there: {}",
                            pool.shown,
                            done.join(" and "),
                            pool.history.describe(state)
                        ));
                        links.push((id, link));
                    }
                    None => lines.push(format!(
                        "pool {}: changed since; nothing changed there for {} (the entries changed since are left as they are)",
                        pool.shown,
                        what.join(", ")
                    )),
                }
                lines.extend(revert_lines.into_iter().map(|l| format!("    {l}")));
            }
        }
        Ok((lines, links))
    }

    /// Revert the steps of a pool that moved on, and do again those that the undo took back, in
    /// the pool, saved as one new step (`from`: the folder of this library; `reverts`: the steps
    /// reverted). The new step, if the pool changed, with the link for the next step of the
    /// library; the lines of the reverts. The files that the reverts place are in the new step of
    /// the pool, not in a step of the library; when the pool did not change they are not recorded.
    fn revert_pool(
        &self,
        pool: &mut Pool,
        command: &str,
        resolver: &mut dyn Resolve,
    ) -> Result<Reverted> {
        let before = pool.history.head().map(|h| h.state);
        let plural = |steps: &[StateId]| {
            format!(
                "step{} {}",
                if steps.len() > 1 { "s" } else { "" },
                steps
                    .iter()
                    .map(|n| short(n))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let mut what = Vec::new();
        if !pool.reverts.is_empty() {
            what.push(format!("revert of {}", plural(&pool.reverts)));
        }
        if !pool.again.is_empty() {
            what.push(format!("{} again", plural(&pool.again)));
        }
        let options = Options {
            message: format!("{command}: {}", what.join(", ")),
            ..Options::default()
        };
        let mut session = Session::open_locked(pool.config.clone(), options, pool.lock.take())?;
        session.mark_reverts(&pool.reverts);
        session.mark_from(&dirname(&realpath(&self.setup.bibtex)));
        let mut run = |session: &mut Session| -> Result<(Reverted, Vec<super::ops::FileOp>)> {
            let mut lines = Vec::new();
            // a conflict (an entry changed in the pool since) is asked in a terminal, else left as it is
            for (steps, forward) in [(&pool.reverts, false), (&pool.again, true)] {
                for n in steps {
                    let how = Revert {
                        forward,
                        anywhere: true,
                        parent: pool.bases.get(n).cloned(),
                    };
                    lines.extend(session.revert(n, resolver, &how)?.1);
                }
            }
            let committed = session.commit()?;
            // the files of the reverts go to the new state of the pool, not to a step of the library
            session.forget_operations();
            Ok(((lines, committed.link), committed.ops))
        };
        let result = run(&mut session);
        let (_, lock) = session.finish_keeping_lock();
        pool.lock = lock;
        pool.history.project.reload(); // with the state of the revert
        let ((lines, link), ops) = result?;
        let head = pool.history.head().map(|h| h.state);
        if let Some(state) = head.as_ref().filter(|h| Some(*h) != before.as_ref())
            && !ops.is_empty()
        {
            pool.history.add_operations(state, &ops)?;
        }
        Ok((lines, link.filter(|_| head != before)))
    }

    // undo, redo, restore
    // -------------------

    /// Go back `steps` states, the pool too for the steps that wrote it
    /// (unless `library_only`). The lock of the library is held by the
    /// caller; `command` is the command line (it names the command for the locks of the pools, and
    /// is the message of the revert of a pool step); `resolver` decides the conflicts of such a
    /// revert (see the module).
    pub fn undo(
        &mut self,
        steps: usize,
        library_only: bool,
        lock: Option<&Lock>,
        command: &str,
        resolver: &mut dyn Resolve,
    ) -> Result<Moved> {
        // the errors are decided as if the head were brought up to date, which is done after
        let (cur, added) = if self.dry_run {
            (self.head().map(|h| h.state), false)
        } else {
            self.synced()?
        };
        let length = cur.as_ref().map_or(0, |c| self.project.line(c).len()) + usize::from(added);
        if length <= steps {
            return Err(Error::exit(if length <= 1 {
                "nothing to undo".to_string()
            } else {
                format!("only {} step(s) to undo", length - 1)
            }));
        }
        let cur = if self.dry_run {
            cur
        } else {
            self.sync_head(None)?
        };
        let cur = cur.ok_or_else(|| Error::exit("nothing to undo"))?;
        let line = self.project.line(&cur);
        let target = line[steps].clone();
        if self.dry_run {
            return Ok(Moved {
                target,
                files: None,
                pools: vec![],
                links: vec![],
            });
        }
        let (mut pools, missing) = if library_only {
            (vec![], vec![])
        } else {
            self.pool_steps(&line[..steps], true, command)?
        };
        let previous = self.head().map(|h| h.redo).unwrap_or_default();
        let redo: Vec<StateId> = line[..steps]
            .iter()
            .rev()
            .chain(previous.iter())
            .cloned()
            .collect();
        // the pools first: the files the command moved to a pool are moved back, unless the pool still links them
        let (pool_lines, links) = self.move_pools(&mut pools, true, "undone", command, resolver)?;
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
        let files = self.go_to(&cur, &target, pool_dirs, &shared, lock)?;
        self.project.set_head(&self.place, &target, &redo)?;
        let pools = missing.into_iter().chain(pool_lines).collect();
        Ok(Moved {
            target,
            files: Some(files),
            pools,
            links,
        })
    }

    /// Go forward `steps` states (see `undo`).
    pub fn redo(
        &mut self,
        steps: usize,
        library_only: bool,
        lock: Option<&Lock>,
        command: &str,
        resolver: &mut dyn Resolve,
    ) -> Result<Moved> {
        let Some(head) = self.head().filter(|h| !h.redo.is_empty()) else {
            return Err(Error::exit("nothing to redo"));
        };
        let cur = head.state.clone();
        if !self.dry_run && self.sync_head(None)?.as_deref() != Some(cur.as_str()) {
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
        let target = head.redo[steps - 1].clone();
        if self.dry_run {
            return Ok(Moved {
                target,
                files: None,
                pools: vec![],
                links: vec![],
            });
        }
        let (mut pools, missing) = if library_only {
            (vec![], vec![])
        } else {
            self.pool_steps(&head.redo[..steps], false, command)?
        };
        // the library first: the files the command moved to the pool are there when the pool comes back
        let files = self.go_to(&cur, &target, None, &BTreeSet::new(), lock)?;
        self.project
            .set_head(&self.place, &target, &head.redo[steps..])?;
        let (pool_lines, links) =
            self.move_pools(&mut pools, false, "redone", command, resolver)?;
        Ok(Moved {
            target,
            files: Some(files),
            pools: missing.into_iter().chain(pool_lines).collect(),
            links,
        })
    }

    /// Go to the state that `reference` names (see `step`) in the project of the library; without
    /// it, to the current state (the changes made outside papers since are undone, and
    /// recorded: redo brings them back). The pools are not touched.
    pub fn restore(&mut self, reference: Option<&str>, lock: Option<&Lock>) -> Result<Moved> {
        let head = self.head();
        let target = match reference {
            None => head
                .map(|h| h.state)
                .ok_or_else(|| Error::exit("no history: nothing to restore"))?,
            Some(reference) => self.step(reference)?,
        };
        if self.dry_run {
            return Ok(Moved {
                target,
                files: None,
                pools: vec![],
                links: vec![],
            });
        }
        let cur = if self.synced()? == (Some(target.clone()), false) {
            target.clone() // nothing to do: the head is not brought up to date
        } else {
            self.sync_head(None)?
                .ok_or_else(|| Error::exit("no history: nothing to restore"))?
        };
        if cur == target {
            return Err(Error::exit(format!(
                "nothing to restore: {} is step {}",
                crate::config::paths::basename(&self.setup.bibtex),
                short(&target)
            )));
        }
        let files = self.go_to(&cur, &target, None, &BTreeSet::new(), lock)?;
        let (back, forward) = self.project.path_between(&cur, &target);
        let redo: Vec<StateId> = if forward.is_empty() {
            back.into_iter().rev().collect()
        } else {
            vec![]
        };
        self.project.set_head(&self.place, &target, &redo)?;
        Ok(Moved {
            target,
            files: Some(files),
            pools: vec![],
            links: vec![],
        })
    }

    // the files kept by the history
    // -----------------------------

    /// The bytes of the files that the state `n` keeps (the files its
    /// command deleted, still in the history).
    pub fn kept_size(&self, n: &str) -> u64 {
        kept_size(&self.project, &self.store, n)
    }

    /// A file deleted from `path` that the history still keeps: its copy in the history and the
    /// step that deleted it, or none. The file is that of the last step that deleted a file there,
    /// else the one whose checksum `.papers/files.json` has for the path (no step).
    pub fn deleted_file(&self, path: &str) -> Option<(String, Option<StateId>)> {
        if self.project.states().is_empty() {
            return None;
        }
        let path = normpath(&abspath(path));
        let mut steps: Vec<(&StateId, &State)> = self.project.states().iter().collect();
        // the most recent first
        steps.sort_by(|a, b| b.1.time().total_cmp(&a.1.time()).then(b.0.cmp(a.0)));
        for (n, state) in steps {
            for op in absolute_ops(&state.ops(), &self.setup.libdir) {
                if op.kind == super::ops::OpKind::Trashed
                    && normpath(&op.src) == path
                    && let Some(stored) = op
                        .checksum
                        .as_deref()
                        .and_then(|c| self.store.file_object(c))
                {
                    return Some((stored, Some(n.clone())));
                }
            }
        }
        let file = self.setup.file.as_deref()?;
        let files_json = super::files_json::FilesJson {
            path: join(&config_root(file), FILES_JSON),
            libdir: self.setup.libdir.clone(),
            absolute_paths: self.setup.absolute_paths,
        };
        let digest = files_json.load().get(&path).cloned()?;
        let stored = self.store.file_object(&digest)?;
        Some((stored, None))
    }

    /// Whether a value of `--drop-files` is a path, not a step: a file that exists, or a name
    /// with a folder separator or a dot.
    pub fn is_path(target: &str) -> bool {
        crate::config::paths::exists(target) || target.contains(['/', '.'])
    }

    /// The files kept by the history that `target` names: a step (its identifier or a start of
    /// it: the files its command deleted), or a path (the file deleted from that place, by any
    /// step; see `is_path`). `{checksum: (path before the deletion, bytes)}`.
    pub fn files_to_drop(&self, target: &str) -> Result<BTreeMap<String, (String, u64)>> {
        let mut found = BTreeMap::new();
        let (chosen, wanted): (Vec<StateId>, Option<String>) = if History::is_path(target) {
            (
                self.project.states().keys().cloned().collect(),
                Some(normpath(&join(
                    &self.setup.cwd,
                    &crate::config::paths::expanduser(target),
                ))),
            )
        } else {
            (vec![self.step(target)?], None)
        };
        for n in chosen {
            let Some(state) = self.project.state(&n) else {
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
        let ids: Vec<StateId> = self.project.states().keys().cloned().collect();
        for n in ids {
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
                self.project.update(&n, state)?;
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

/// The error of a step that the history does not have.
pub fn no_step(reference: &str) -> Error {
    Error::exit(format!(
        "no step {reference} in the history of the library (see `papers history`)"
    ))
}

/// The bytes of the files that the state `n` keeps.
pub fn kept_size(project: &Project, store: &super::store::Store, n: &str) -> u64 {
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

/// The line of a state: `<short identifier> <message> (<local time>[, from <from>])`.
pub fn describe(project: &Project, n: &str) -> String {
    let Some(state) = project.state(n) else {
        return format!("{}  (not found)", short(n));
    };
    let origin = state
        .from()
        .map(|f| format!(", from {}", short_home(f)))
        .unwrap_or_default();
    format!(
        "{} {} ({}{origin})",
        short(n),
        state.message(),
        when(state.time())
    )
}
