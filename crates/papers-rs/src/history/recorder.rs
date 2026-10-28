//! Recording the states of a library (spec/history.md, sections 5 and 9.2):
//! the head of a place and the changes made outside papers, the state after
//! a save, the operations after the save, the retention, the guard before a
//! command deletes files.

use std::collections::BTreeMap;
use std::path::Path;

use super::ops::{FileOp, OpKind, deleted_refs, relative_ops};
use super::project::{Head, NewState, PoolLink, Project, State, Text};
use super::setup::{Setup, Snapshot, library_id, path_id, read_bibtex, read_kept};
use super::store::{MB, Store, now, sha256, to_system_trash};
use crate::config::Config;
use crate::config::paths::{abspath, basename, realpath};
use crate::error::{Error, Result};
use crate::log;

/// Without files deleted, the store is pruned every this many states.
pub const PRUNE_EVERY: u64 = 50;
/// A day, in seconds.
pub const DAY: f64 = 86400.0;

/// The kept `.papers/` files of a state: name (relative to the bibtex directory) and pieces.
pub type KeptFiles = Vec<(String, Vec<String>)>;

/// The history of one library: its store, its project, its place.
#[derive(Debug)]
pub struct History {
    pub setup: Setup,
    pub store: Store,
    pub project: Project,
    /// The real path of the bibtex.
    pub place: String,
    /// `--dry-run`: nothing is written.
    pub dry_run: bool,
    /// Where the files go that the history no longer keeps (the system trash).
    pub trash: fn(&str),
}

/// A step to record after a library is saved: the hook of the library.
///
/// The paths and texts are plain; the library model fills it at its save.
#[derive(Debug, Clone, Default)]
pub struct Step {
    /// The command line: `papers ` and its arguments joined by spaces.
    pub message: String,
    /// The file operations of the command so far, oldest first, with
    /// absolute normalized paths (`FileOp::new` makes them). Only for the
    /// library of the command: none for another library the command wrote
    /// (its pool).
    pub ops: Vec<FileOp>,
    /// The text now of the bibtex and of the kept `.papers/` files, when the
    /// caller has it; else they are read from disk.
    pub snapshot: Option<Snapshot>,
    /// For the library of the command: the steps that the command added
    /// before to the histories of other libraries (the `Recorded::link` of
    /// their records, one per project id: the later wins).
    pub pools: Vec<(String, PoolLink)>,
    /// For a library that is not the library of the command (its pool): the
    /// directory of the bibtex of the command's library.
    pub from: Option<String>,
    /// The states that this step takes back (`history --revert`, the revert
    /// of a pool step by undo).
    pub reverts: Vec<u64>,
}

impl Step {
    pub fn new(message: impl Into<String>) -> Step {
        Step {
            message: message.into(),
            ..Step::default()
        }
    }
}

/// What `History::record` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// The state of the library after the save: the new one, or the head
    /// when nothing changed.
    pub number: Option<u64>,
    /// A state was added.
    pub added: bool,
    /// For a library that is not the library of the command: the link
    /// (project id, step) to give to the `pools` of the next step of the
    /// command's library.
    pub link: Option<(String, PoolLink)>,
}

fn files_map(files: &[(String, Vec<String>)]) -> BTreeMap<&str, &[String]> {
    files
        .iter()
        .map(|(n, p)| (n.as_str(), p.as_slice()))
        .collect()
}

impl History {
    /// The history of the library of `config`: `None` without a bibtex, or
    /// (with `create` false) without an identifier yet. A library without
    /// identifier gets one when `create` is true: saved into its
    /// configuration (not with `dry_run`).
    pub fn open(config: &mut Config, create: bool, dry_run: bool) -> Result<Option<History>> {
        let Some(bibtex) = config.bibtex.clone() else {
            return Ok(None);
        };
        let place = realpath(&bibtex);
        let id = if config.file.is_some() && config.is_library {
            match library_id(config, create && !dry_run, dry_run)? {
                Some(id) => id,
                None => return Ok(None),
            }
        } else {
            path_id(&place)
        };
        let setup = Setup::from_config(config);
        let store = setup.store(Some(&id));
        let project = store.project(&id)?;
        Ok(Some(History {
            setup,
            store,
            project,
            place,
            dry_run,
            trash: to_system_trash,
        }))
    }

    /// At the start of a command: the library as it is before it, for its
    /// first state if it has no history yet (the command may write the files
    /// of `.papers/` before it saves the bibtex). Nothing is written. Not for
    /// `status`, `config`, `init`, `install`, `uninstall`, `migrate`,
    /// `history`, `backup`, `git`, `undo`, `redo`, `restore`, `restore-backup`,
    /// nor with `--dry-run` (the caller decides).
    pub fn before_command(config: &mut Config) -> Option<Snapshot> {
        let bibtex = config.bibtex.clone()?;
        if !Path::new(&bibtex).exists() {
            return None;
        }
        match History::open(config, false, false) {
            Ok(Some(h)) if h.project.head(&h.place).is_some() => None,
            Ok(_) => {
                match Snapshot::read(&bibtex, &crate::config::paths::dirname(&abspath(&bibtex))) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        log::debug(&format!("the library before the command is not kept: {e}"));
                        None
                    }
                }
            }
            Err(e) => {
                log::debug(&format!("the library before the command is not kept: {e}"));
                None
            }
        }
    }

    pub fn project_id(&self) -> &str {
        &self.project.id
    }

    pub fn head(&self) -> Option<Head> {
        self.project.head(&self.place)
    }

    /// The bibtex and the files as they are on disk, stored: the pieces of
    /// the bibtex, its checksum, the pieces of the kept files. `files`: those
    /// of another state (the files on disk are not read); `texts`: the texts to
    /// store instead of those on disk.
    fn snapshot(
        &self,
        files: Option<Vec<(String, Vec<String>)>>,
        texts: Option<&Snapshot>,
    ) -> Result<(Text, KeptFiles)> {
        let text = match texts {
            Some(t) => t.bibtex.clone(),
            None => read_bibtex(&self.setup.bibtex)?,
        };
        let files = match files {
            Some(f) => f,
            None => {
                let others = match texts {
                    Some(t) => t.files.clone(),
                    None => read_kept(&self.setup.libdir)?,
                };
                let mut stored = Vec::new();
                for (name, content) in others {
                    let pieces = self.store.put_text(&content)?;
                    stored.push((name, pieces));
                }
                stored
            }
        };
        let pieces = self.store.put_text(&text)?;
        Ok((
            Text {
                pieces,
                sha: sha256(text.as_bytes()),
            },
            files,
        ))
    }

    // the head of a place
    // -------------------

    /// The current state of the library, before a command writes it: the
    /// state of its place; a new place starts from the state that has its
    /// bibtex (a moved or copied library). If the bibtex on disk is not that
    /// state, it is recorded first: changed outside papers (or, without
    /// history yet, the library before its first command, `before`). The
    /// number of the state, or `None` (dry run).
    pub fn sync_head(&mut self, before: Option<&Snapshot>) -> Result<Option<u64>> {
        if self.dry_run {
            return Ok(None);
        }
        self.project.reload();
        let place = self.place.clone();
        let text = read_bibtex(&self.setup.bibtex)?;
        let sha = sha256(text.as_bytes());
        let mut head = self.project.head(&place);
        if head.is_none() {
            self.project.forget_missing_places()?;
            if let Some(n) = self.project.start_state(&sha) {
                self.project.set_head(&place, n, &[])?;
                head = self.project.head(&place);
            }
        }
        if let Some(h) = &head
            && self.project.state(h.state).and_then(State::bibtex_sha) == Some(sha.as_str())
        {
            return Ok(Some(h.state));
        }
        let (kind, message, files, texts) = match &head {
            None => (
                "start",
                "the library before its first recorded command",
                None,
                before,
            ),
            Some(h) => {
                log::info(&format!(
                    "{} changed outside papers since its last state: recorded",
                    basename(&self.setup.bibtex)
                ));
                (
                    "outside",
                    "changed outside papers",
                    self.project.state(h.state).and_then(State::files),
                    None,
                )
            }
        };
        let (bibtex, files) = self.snapshot(files, texts)?;
        let state = State::new(NewState {
            parent: head.as_ref().map(|h| h.state),
            time: now(),
            kind: kind.into(),
            message: message.into(),
            ops: vec![],
            deleted: vec![],
            bibtex,
            files,
        });
        let n = self.project.add(state)?;
        self.project.set_head(&place, n, &[])?;
        if head.is_none()
            && let Some(t) = before
            && t.bibtex != text
        {
            return self.sync_head(before); // changed by the command before its save: recorded after
        }
        Ok(Some(n))
    }

    // the state after a save
    // ----------------------

    /// After the save of the library: its state, with the file operations of
    /// the command. No state is added when there is a parent, no operations,
    /// no links with other libraries, and the bibtex and the kept files equal
    /// those of the parent (the head stays).
    pub fn record(&mut self, step: Step) -> Result<Recorded> {
        if self.dry_run {
            return Ok(Recorded {
                number: None,
                added: false,
                link: None,
            });
        }
        self.project.reload();
        let place = self.place.clone();
        let head = self.project.head(&place);
        // the library had no state (e.g. its bibtex created by the command): the command is its first state
        let parent = head.as_ref().map(|h| h.state);
        let own = step.from.is_none();
        let ops = relative_ops(&step.ops, &self.setup.libdir, self.setup.absolute_paths);
        let (bibtex, files) = self.snapshot(None, step.snapshot.as_ref())?;
        let linked = if own { step.pools.clone() } else { Vec::new() };
        let previous = parent.and_then(|p| self.project.state(p));
        let unchanged = previous.is_some_and(|prev| {
            ops.is_empty()
                && linked.is_empty()
                && prev.bibtex_pieces() == bibtex.pieces
                && prev.bibtex_sha() == Some(bibtex.sha.as_str())
                && prev
                    .files()
                    .is_some_and(|f| files_map(&f) == files_map(&files))
        });
        let mut link = None;
        let (n, added) = if unchanged {
            (parent, false)
        } else {
            let mut state = State::new(NewState {
                parent,
                time: now(),
                kind: "command".into(),
                message: step.message.clone(),
                deleted: deleted_refs(&ops),
                ops: ops.clone(),
                bibtex,
                files,
            });
            if !linked.is_empty() {
                state.set_pools(&linked);
            }
            if !step.reverts.is_empty() {
                state.set_reverts(&step.reverts);
            }
            if let Some(from) = &step.from {
                state.set_from(from);
            }
            let n = self.project.add(state)?;
            self.project.set_head(&place, n, &[])?;
            if !own && let Some(file) = &self.setup.file {
                link = Some((
                    self.project.id.clone(),
                    PoolLink {
                        state: n,
                        config: abspath(file),
                    },
                ));
            }
            (Some(n), true)
        };
        if ops.iter().any(|op| op.kind == OpKind::Trashed)
            || n.is_some_and(|n| n % PRUNE_EVERY == 0)
        {
            self.prune();
        }
        Ok(Recorded {
            number: n,
            added,
            link,
        })
    }

    /// The operations done after the save of the library join its state `n`,
    /// if it is still current (a warning otherwise).
    pub fn add_operations(&mut self, n: u64, ops: &[FileOp]) -> Result<()> {
        if self.dry_run {
            return Ok(());
        }
        self.project.reload();
        let still_current = self.project.head(&self.place).is_some_and(|h| h.state == n);
        let Some(state) = self.project.state(n).filter(|_| still_current) else {
            log::warning(&format!(
                "{} file operation(s) not recorded for undo: the library changed since",
                ops.len()
            ));
            return Ok(());
        };
        let mut state = state.clone();
        let mut all = state.ops();
        all.extend(relative_ops(
            ops,
            &self.setup.libdir,
            self.setup.absolute_paths,
        ));
        let deleted = deleted_refs(&all);
        state.set_ops(&all, &deleted);
        self.project.update(n, state)?;
        if !deleted.is_empty() {
            self.prune();
        }
        Ok(())
    }

    // retention
    // ---------

    /// Drop the oldest states until the store is within `history-size`
    /// (nothing when the limit is 0).
    pub fn prune(&self) {
        prune_store(&self.setup, &self.store, &self.trash);
    }

    // the files that a command deletes
    // ---------------------------------

    /// Before a command deletes `count` files of `total` bytes: `Some(true)`,
    /// they are kept for undo; `Some(false)`, they go to the system trash (undo
    /// cannot bring them back); `None`, the user said no: keep the files.
    /// `ask` puts a question and gives the answer (`-y/--yes`: true; without a
    /// terminal: false, with a warning).
    pub fn guard(
        &mut self,
        total: u64,
        count: usize,
        ask: &mut dyn FnMut(&str) -> bool,
    ) -> Option<bool> {
        if !self.setup.keep_deleted_files {
            return Some(false);
        }
        let limit = self.setup.limit(&self.store);
        let size = self.setup.history_size(&self.store);
        if limit == 0 {
            return Some(true);
        }
        let what = format!(
            "{count} file{} to delete, {:.0} MB",
            if count > 1 { "s" } else { "" },
            total as f64 / MB as f64
        );
        if total > limit {
            let question = format!(
                "{what}: more than the history keeps (history-size: {size} MB).\n- This command cannot be undone: the files go to the trash of the system, not to the history.\n- The history of the libraries is dropped: no undo before this point."
            );
            return if ask(&question) { Some(false) } else { None };
        }
        let dropped = self.store.prune(limit, total, true, &|_| {});
        if dropped.is_empty() {
            return Some(true);
        }
        let t = now();
        let recent = dropped.iter().filter(|d| t - d.state.time() < DAY).count();
        let count_all: usize = self
            .store
            .project_ids()
            .iter()
            .filter_map(|id| self.store.project(id).ok())
            .map(|p| p.states().len())
            .sum();
        if recent > 0 || 2 * dropped.len() > count_all {
            let question = format!(
                "{what}: to keep them for undo, the history drops {} of its {count_all} steps{} (history-size: {size} MB).",
                dropped.len(),
                if recent > 0 {
                    format!(", {recent} of them from the last day")
                } else {
                    String::new()
                }
            );
            return if ask(&question) { Some(true) } else { None };
        }
        Some(true)
    }
}

/// `prune` of a store, with its limit, and the message.
pub fn prune_store(setup: &Setup, store: &Store, trash: &dyn Fn(&str)) {
    let limit = setup.limit(store);
    if limit == 0 {
        return;
    }
    let dropped = store.prune(limit, 0, false, trash);
    if !dropped.is_empty() {
        log::info(&format!(
            "{} old step(s) dropped from the history (history-size: {} MB)",
            dropped.len(),
            setup.history_size(store)
        ));
    }
}

/// papers prints the `KeyError` of the missing piece: with its quotes (the specification
/// leaves them out; the code is the truth).
impl From<crate::history::store::MissingPiece> for Error {
    fn from(m: crate::history::store::MissingPiece) -> Error {
        Error::exit(format!("the history of the library is damaged: '{m}'"))
    }
}
