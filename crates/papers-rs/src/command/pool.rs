//! What the commands that write a library do with its pool (`papers.cli.common`: `save_entries`,
//! `push_changed`, `save_exchange`; `papers.library._repair_pool_links`; `papers.pools.use_pool_files`):
//! the broken links to pool files repaired when a shallow library is loaded, the files placed in
//! the files directory of the pool, the **automatic push** at the commit (the entries that the
//! command added go to the pool; with `auto-push` those it changed too; the file links of those whose
//! links changed, whatever `auto-push` says), and the save of the pools, the origin records and the
//! library in this order (spec `exchange.md` sections 3.5 and 12).
//!
//! The exchange commands `pull`, `push` and `sync` are in [`super::exchange`].

use std::collections::HashSet;

use crate::command::report::Notes;
use crate::config::json::py_repr_str;
use crate::config::paths::{dirname, realpath};
use crate::entry;
use crate::error::{Error, Result};
use crate::history::PoolLink;
use crate::library::Library;
use crate::log;
use crate::merge::{OnDuplicate, Resolver};
use crate::names::{Format, FormatKind};
use crate::pools::{Exchange, Outcome, PoolIx, What};

use super::session::{Options, Session};

/// The note of each outcome of an automatic push (`_PUSH_NOTES`), with the pool label and the key.
fn push_note(what: What, label: &str, key: &str) -> Option<String> {
    Some(match what {
        What::Added => format!("also added to {label}"),
        What::Updated => format!("also modified in {label}"),
        What::Links => format!("file links also updated in {label}"),
        What::Skipped => "not pushed: changed in the pool too; papers sync decides".to_string(),
        What::Empty => format!("not pushed to {label}: no fields or no key"),
        What::Removed => format!("removed from {label} (`papers push {key}` adds it back)"),
        _ => return None,
    })
}

/// The counts of the outcomes of pull or push, in the order they first appear, as the lines of the
/// entries say (`report_counts`): `1 added, 2 modified`; empty when nothing happened (the entries
/// unchanged are not counted).
pub fn report_counts(report: &[Outcome]) -> String {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for o in report {
        let word = match o.what {
            What::Unchanged => continue,
            What::Updated => "modified",
            What::Links => "with file links updated",
            What::Linked => "linked",
            What::Skipped => "skipped",
            What::NotFound => "not found",
            What::Added => "added",
            What::Empty => "empty",
            What::Removed => "removed",
        };
        match counts.iter_mut().find(|(w, _)| *w == word) {
            Some((_, n)) => *n += 1,
            None => counts.push((word, 1)),
        }
    }
    counts
        .iter()
        .map(|(w, n)| format!("{n} {w}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Add the notes of the entries that a push following another command changed in the pool
/// (`push_notes`); the push itself is reported at the info level.
pub fn push_notes(exchange: &Exchange, pushed: &[Outcome], notes: &mut Notes) {
    let Some(pool) = pushed.first().and_then(|o| o.pool) else {
        return;
    };
    let counts = report_counts(pushed);
    if !counts.is_empty() {
        log::info(&format!("push to {}: {counts}", exchange.label(pool)));
    }
    for o in pushed {
        let label = o.pool.map(|p| exchange.label(p)).unwrap_or_default();
        if let Some(text) = push_note(o.what, &label, &o.key) {
            let key = o.key.to_lowercase();
            match notes.get_mut(&key) {
                Some(old) => *old = format!("{old}; {text}"),
                None => {
                    notes.insert(key, text);
                }
            }
        }
    }
}

/// The entries new or modified since loaded whose file links changed (`links_changed`).
fn links_changed(library: &Library) -> Vec<String> {
    let (new, modified) = library.changed_entries();
    let real =
        |files: &[String]| -> HashSet<String> { files.iter().map(|f| realpath(f)).collect() };
    new.into_iter()
        .chain(modified)
        .filter(|key| {
            let Some(e) = library.get(key) else {
                return false;
            };
            let before = library.loaded_files(e).unwrap_or_default();
            real(&before) != real(&entry::files(e, Some(&library.relative_to)))
        })
        .collect()
}

/// Repair the broken links to pool files of a shallow library that was just loaded (`_repair_pool_links`):
/// the entries repaired, and the library is saved at the next save of the command even if nothing
/// else changed. The library is marked as loaded again: the repaired links are not a change of the
/// command.
pub fn repair_on_load(exchange: &mut Exchange, library: &mut Library) -> crate::pools::Repaired {
    let mut repaired_links = Vec::new();
    let result = (|| -> Result<crate::pools::Repaired> {
        let Some(pool) = exchange.pools.pool(true) else {
            return Ok(Vec::new());
        };
        let repaired = exchange.repair_links(library, None, pool)?;
        for (key, found) in &repaired {
            let new: Vec<String> = found
                .iter()
                .flat_map(|(_, files)| files.iter())
                .map(|f| py_repr_str(f))
                .collect();
            log::info(&format!(
                "{key}: broken link to a pool file repaired: [{}]",
                new.join(", ")
            ));
        }
        if !repaired.is_empty() {
            library.load_changes = true;
        }
        Ok(repaired)
    })();
    match result {
        Ok(repaired) => repaired_links = repaired,
        Err(error) => log::warning(&format!(
            "could not repair the links to the pool files: {error}"
        )),
    }
    library.mark_loaded();
    repaired_links
}

/// The library of a configuration as a command that reads it loads it (`get_biblio`): in a shallow
/// library, the broken links to pool files are repaired.
pub fn load_library(config: &crate::config::Config) -> Result<Library> {
    let mut library = Library::open(config)?;
    if config.shallow() {
        let mut exchange = Exchange::quiet(config, false);
        let _ = repair_on_load(&mut exchange, &mut library);
    }
    Ok(library)
}

impl Session {
    /// The exchanges with the pools of the library (made when first needed).
    pub fn exchange(&mut self) -> &mut Exchange {
        if self.exchange.is_none() {
            let mut exchange = Exchange::new(
                &self.config,
                self.dry_run(),
                None,
                self.say.clone(),
                self.prompt(),
            );
            if self.holds_lock() {
                exchange.pools.lock_as(self.message());
            }
            self.exchange = Some(exchange);
        }
        self.exchange.as_mut().expect("the exchange")
    }

    /// The library is loaded: its broken links to pool files are repaired.
    pub(crate) fn repair_pool_links(&mut self) {
        self.exchange();
        if let Some(exchange) = self.exchange.as_mut() {
            self.repaired = repair_on_load(exchange, &mut self.library);
        }
    }

    /// In a shallow library, the files that the command places go to the files directory of its pool,
    /// named with the pool's template (`use_pool_files`): an error if the pool is not a library or has
    /// no files directory. Nothing in a self-contained library.
    pub fn use_pool_files(&mut self) -> Result<()> {
        if !self.config.shallow() {
            return Ok(());
        }
        let name_from_library = self.config.source("name.template") == "library";
        let exchange = self.exchange();
        let ix = exchange.pools.target()?;
        let pool = exchange.pools.at(ix);
        let label = pool.label();
        let Some(filesdir) = pool.filesdir() else {
            return Err(Error::exit(format!(
                "{label} has no files directory, and this library is shallow (its files are in the pool)"
            )));
        };
        let pool_config = pool.config.clone();
        if name_from_library {
            log::warning(&format!(
                "the name template of a shallow library is not used: the files are named with the template of {label}"
            ));
        }
        self.own_nameformat
            .get_or_insert_with(|| self.library.nameformat.clone());
        self.config.use_pool_files(&pool_config);
        self.library.filesdir = Some(filesdir);
        self.library.nameformat = Format::from_config(&self.config, FormatKind::Name);
        Ok(())
    }

    /// `add` looks in a pool first: the library of `--pool PATH`, else the pool of the library (a
    /// pool that cannot be read is a warning, and no lookup). The pool, if there is one.
    pub fn start_pool_lookup(&mut self, path: Option<&str>) -> Result<Option<PoolIx>> {
        self.exchange();
        let exchange = self.exchange.as_mut().expect("the exchange");
        let pool = match path {
            Some(path) => Some(exchange.pools.get(path)?),
            None => exchange.pools.pool(false),
        };
        self.lookup = pool.map(crate::pools::lookup::PoolLookup::new);
        Ok(pool)
    }

    /// What `add` did with the pool: the notes `from pool X` for the entries it took, and the first exchange
    /// with the pool (`after_take`). The files given that were stored already are removed (in `move` mode).
    pub fn finish_pool_lookup(&mut self) {
        let Some(lookup) = self.lookup.as_ref() else {
            return;
        };
        let pool = lookup.pool;
        let taken: Vec<(String, PoolIx)> = lookup
            .taken_from
            .iter()
            .map(|(k, p)| (k.clone(), *p))
            .collect();
        if let Some(exchange) = self.exchange.as_ref() {
            for (key, p) in taken {
                self.notes
                    .insert(key, format!("from {}", exchange.label(p)));
            }
        }
        self.taken_from.push(pool);
    }

    /// The files given to `add` that were stored already (`_remove_moved`): removed, and recorded for undo.
    pub fn remove_stored_duplicates(&mut self) {
        let Some(lookup) = self.lookup.as_mut() else {
            return;
        };
        for (file, target) in std::mem::take(&mut lookup.to_remove) {
            if self.dry_run() {
                self.say
                    .line(format!("DRYRUN: {file} not removed (the same as {target})"));
                continue;
            }
            self.say.line(format!(
                "{file}: removed, the same as {target} (files mode move)"
            ));
            self.remove_same(&file, &target);
        }
    }

    /// The name template of the library itself (a shallow library places its files with the pool's).
    pub fn own_nameformat(&self) -> Format {
        self.own_nameformat
            .clone()
            .unwrap_or_else(|| self.library.nameformat.clone())
    }

    /// The files directories of the pool of the library.
    pub fn pool_files_dirs(&mut self) -> Vec<String> {
        if !self.config.shallow() {
            return Vec::new();
        }
        self.exchange().pools.files_dirs()
    }

    /// In a shallow library, the real paths of the files that the entries of its pool link to.
    pub fn pool_linked_files(&mut self) -> HashSet<String> {
        let mut linked = HashSet::new();
        if !self.config.shallow() {
            return linked;
        }
        let exchange = self.exchange();
        let Some(pool) = exchange.pools.pool(true) else {
            return linked;
        };
        if let Ok(library) = exchange.pools.at_mut(pool).library() {
            for e in library.entries() {
                linked.extend(
                    entry::files(e, Some(&library.relative_to))
                        .iter()
                        .map(|f| realpath(f)),
                );
            }
        }
        linked
    }

    /// The steps that another command (an undo) added to the histories of other libraries: the next
    /// step of this library records them in its `pools`.
    pub fn link_pools(&mut self, links: Vec<(String, PoolLink)>) {
        for (id, mut link) in links {
            // a pool saved several times by the command: its last state, and its state before the
            // first save
            if let Some((_, first)) = self.pending_links.iter().find(|(i, _)| *i == id) {
                link.base = first.base.clone();
            }
            self.pending_links.retain(|(i, _)| *i != id);
            self.pending_links.push((id, link));
        }
    }

    /// The automatic push of a shallow library, before the save (`push_changed`): the entries that
    /// the command added go to the pool (`push KEY`: added to it, or merged if it has the paper);
    /// with `auto-push`, those it changed that the pool shares; the file links of those whose links
    /// changed. A conflict is not pushed (`papers sync` decides). The notes for the lines of the
    /// entries are added to the notes of the session.
    pub(crate) fn auto_push(&mut self) -> Result<()> {
        if !self.config.shallow() {
            return Ok(());
        }
        let (new, modified) = self.library.changed_entries();
        let auto = crate::config::json::py_truthy(&self.config.get("auto-push"));
        let modified = if auto { modified } else { Vec::new() };
        let links = links_changed(&self.library);
        if new.is_empty() && modified.is_empty() && links.is_empty() {
            return Ok(());
        }
        let mut notes = std::mem::take(&mut self.notes);
        let report = self.push_changed(new, modified, links);
        match report {
            Ok(Some(report)) => {
                if let Some(exchange) = self.exchange.as_ref() {
                    push_notes(exchange, &report, &mut notes);
                }
            }
            Ok(None) => {}
            Err(error) => {
                self.notes = notes;
                return Err(error);
            }
        }
        self.notes = notes;
        Ok(())
    }

    fn push_changed(
        &mut self,
        new: Vec<String>,
        modified: Vec<String>,
        links: Vec<String>,
    ) -> Result<Option<Vec<Outcome>>> {
        let library = &mut self.library;
        let exchange = self.exchange.as_mut().expect("the exchange");
        let pool = match exchange.pools.target() {
            Ok(pool) => pool,
            Err(error) => {
                log::warning(&format!("not pushed to the pool: {error}"));
                return Ok(None);
            }
        };
        // the entries removed from the pool are not pushed (their note says so): no longer shared
        let new_set: HashSet<&String> = new.iter().collect();
        let mut others: Vec<String> = Vec::new();
        for key in modified.iter().chain(&links) {
            if !new_set.contains(key) && !others.contains(key) {
                others.push(key.clone());
            }
        }
        let (shared, removed) = exchange.sharing(library, &others, pool)?;
        exchange.forget(library, &removed, pool);
        let entries: Vec<String> = new
            .iter()
            .cloned()
            .chain(modified.iter().filter(|k| shared.contains(k)).cloned())
            .collect();
        let links: Vec<String> = links.into_iter().filter(|k| !removed.contains(k)).collect();
        let mut resolver = Resolver::new(OnDuplicate::Skip);
        let mut report = if entries.is_empty() {
            Vec::new()
        } else {
            exchange.push(library, &entries, pool, &mut resolver, true)?
        };
        // the pool entries just added, or not pushed for want of fields, are left as they are
        let done: HashSet<String> = report
            .iter()
            .filter(|o| matches!(o.what, What::Added | What::Empty))
            .map(|o| o.key.clone())
            .collect();
        let updated: HashSet<String> = report
            .iter()
            .filter(|o| o.what == What::Updated)
            .map(|o| o.key.clone())
            .collect(); // their line says so already
        let to_link: Vec<String> = links.into_iter().filter(|k| !done.contains(k)).collect();
        let linked = exchange.push_links(library, &to_link, pool, &mut resolver)?;
        report.extend(
            linked
                .into_iter()
                .filter(|o| !(o.what == What::Links && updated.contains(&o.key))),
        );
        for key in removed {
            report.push(Outcome {
                key,
                pool: Some(pool),
                what: What::Removed,
            });
        }
        Ok(Some(report))
    }

    /// Save what an exchange with a pool changed, before the library (`save_exchange`): the pools
    /// pushed to (each a step of its own history, linked to the next step of this library), the pairs
    /// that are not the same paper of the pools, and the origin records. Whether the origin records
    /// changed: the library is then saved (and has a step) even if its bibtex did not change.
    pub(crate) fn save_exchange(&mut self) -> Result<bool> {
        let Some(mut exchange) = self.exchange.take() else {
            return Ok(false);
        };
        let result = self.save_exchange_of(&mut exchange);
        self.exchange = Some(exchange);
        result
    }

    fn save_exchange_of(&mut self, exchange: &mut Exchange) -> Result<bool> {
        let dry_run = self.dry_run();
        let changed: Vec<PoolIx> = exchange.changed_pools.clone();
        for pool in &changed {
            exchange.pools.at_mut(*pool).check_readable()?;
        }
        let from = dirname(&realpath(self.library.bibtex.as_str()));
        for pool in &changed {
            let (config, library, lock) = {
                let p = exchange.pools.at_mut(*pool);
                let config = p.config.clone();
                let lock = p.take_lock();
                let library = p.take_library()?;
                (config, library, lock)
            };
            let options = Options {
                message: self.message().to_string(),
                dry_run,
                ..Options::default()
            };
            let mut nested = Session::adopt(config, library, options, lock);
            nested.mark_from(&from);
            let committed = nested.commit();
            let (outcome, lock, library) = nested.finish_adopted();
            for line in &outcome.out {
                self.say.line(line);
            }
            let p = exchange.pools.at_mut(*pool);
            p.put_lock(lock);
            p.put_library(library);
            let committed = committed?;
            if let Some(link) = committed.link {
                self.link_pools(vec![link]);
            }
            self.pool_ops.extend(committed.ops);
        }
        let pushed: Vec<PoolIx> = changed
            .iter()
            .copied()
            .chain(
                exchange
                    .pushed_pools
                    .iter()
                    .copied()
                    .filter(|p| !changed.contains(p)),
            )
            .collect();
        for pool in pushed {
            exchange.after_push(pool);
        }
        if !dry_run {
            for pool in exchange.pools.loaded() {
                if !changed.contains(&pool)
                    && let Some(library) = exchange.pools.at_mut(pool).loaded_mut()
                {
                    library
                        .distinct
                        .save()
                        .map_err(|e| Error::other(format!("{}: {e}", library.bibtex)))?;
                }
            }
        }
        let origin_changed = exchange.origin.changed();
        exchange.origin.save()?;
        Ok(origin_changed)
    }

    /// What `after_take` needs: the pool of which `add` took entries.
    pub(crate) fn after_taking(&mut self) {
        let Some(exchange) = self.exchange.as_mut() else {
            return;
        };
        let taken = std::mem::take(&mut self.taken_from);
        for pool in taken {
            if !exchange.changed_pools.contains(&pool) && !exchange.pushed_pools.contains(&pool) {
                exchange.after_take(pool);
            }
        }
    }
}

/// After an undo or a redo of a shallow library with `auto-push` (not with `--dry-run`, nor with
/// `--library-only`; `history.md` 7.6): the entries shared with the pool that differ from their origin
/// record are pushed (a conflict is left to `papers sync`; no new entry). Nothing to push in general: undo
/// and redo restore the origin records with the bibtex. If an entry was updated, the exchange is saved (a
/// pool step, then a library step): the push is a new step, so nothing is left to redo, as after
/// any command (the line says so, and how to go back to the steps undone).
/// `links` are the steps that the undo or redo added to the histories of the pools (they go to the library's
/// next step). The lines of the notes go to the sink of the options, or are in the outcome; the lock is
/// given back.
pub fn push_after_travel(
    config: crate::config::Config,
    options: Options,
    lock: Option<crate::lock::Lock>,
    links: Vec<(String, PoolLink)>,
) -> Result<(super::session::Outcome, Option<crate::lock::Lock>)> {
    let wanted = config.shallow()
        && crate::config::json::py_truthy(&config.get("auto-push"))
        && !options.dry_run;
    if !wanted {
        return Ok((super::session::Outcome::default(), lock));
    }
    let mut session = Session::open_locked(config, options, lock)?;
    session.no_auto_push();
    session.link_pools(links);
    let result = session.push_travelled();
    let (outcome, lock) = session.finish_keeping_lock();
    result.map(|_| (outcome, lock))
}

impl Session {
    fn push_travelled(&mut self) -> Result<()> {
        let keys: Vec<String> = self.library.entries().map(|e| e.key.clone()).collect();
        self.exchange();
        let exchange = self.exchange.as_mut().expect("the exchange");
        let pool = match exchange.pools.target() {
            Ok(pool) => pool,
            Err(error) => {
                log::warning(&format!("not pushed to the pool: {error}"));
                return Ok(());
            }
        };
        let (shared, _) = exchange.sharing(&self.library, &keys, pool)?;
        let mut resolver = Resolver::new(OnDuplicate::Skip);
        let report = exchange.push(&mut self.library, &shared, pool, &mut resolver, false)?;
        let mut notes = std::collections::HashMap::new();
        if report.iter().any(|o| o.what == What::Updated) {
            let undone = self.history()?.head().map(|h| h.redo).unwrap_or_default();
            self.commit()?;
            if let Some(first) = undone.first() {
                self.say.line(format!(
                    "the push is a new step: nothing to redo now (`papers restore {}` goes back to the steps undone)",
                    crate::history::project::short(first)
                ));
            }
        }
        push_notes(
            self.exchange.as_ref().expect("the exchange"),
            &report,
            &mut notes,
        );
        for line in push_note_lines(&self.library, &notes) {
            self.say.line(line);
        }
        Ok(())
    }
}

/// The notes of the entries as lines `KEY: note`, for a command that does not list the entries it
/// changed (`print_push_notes`): in the order of the library.
pub fn push_note_lines(library: &Library, notes: &Notes) -> Vec<String> {
    library
        .entries()
        .filter_map(|e| {
            notes
                .get(&e.key.to_lowercase())
                .map(|n| format!("{}: {n}", e.key))
        })
        .collect()
}

// the files follow a change of the pool (`papers config pool`)
// ---------------------------------------------------------------

/// The pool files that a shallow library links to, to copy into its files directory: `(key, files)`.
pub type OwnPlan = Vec<(String, Vec<String>)>;

/// What linking a library to the files of a pool does: `(key, [(file, the pool's copy)])` of the entries
/// whose links are replaced, and the keys of the entries with a file that the pool entry does not have.
pub type RelinkPlan = Vec<(String, Vec<(String, String)>)>;

impl Session {
    /// The files that `papers config pool none` copies into the files directory (`plan_own_files`): the files
    /// of the pool (`dirs`, by default the files directory of the pool of the library) that the library
    /// links to. The broken links to pool files are repaired first.
    pub fn plan_own_files(&mut self, dirs: Option<&[String]>) -> Result<OwnPlan> {
        self.exchange();
        let exchange = self.exchange.as_mut().expect("the exchange");
        if let Some(pool) = exchange.pools.pool(true) {
            exchange.repair_links(&mut self.library, None, pool)?;
        }
        let dirs: Vec<String> = match dirs {
            Some(dirs) => dirs.iter().filter(|d| !d.is_empty()).cloned().collect(),
            None => exchange.pools.files_dirs(),
        };
        let mut plan = Vec::new();
        for e in self.library.entries() {
            let files: Vec<String> = entry::files(e, Some(&self.library.relative_to))
                .into_iter()
                .filter(|f| {
                    crate::config::paths::exists(f)
                        && dirs.iter().any(|d| crate::config::paths::is_inside(f, d))
                })
                .collect();
            if !files.is_empty() {
                plan.push((e.key.clone(), files));
            }
        }
        Ok(plan)
    }

    /// Copy (a hard link when possible) the files of [`Session::plan_own_files`] into the files directory, with the
    /// library's name template, and relink them; the pool keeps its files.
    pub fn own_files(&mut self, plan: &OwnPlan, dirs: &[String]) -> Result<()> {
        if self.library.filesdir.is_none() {
            return Err(Error::exit(
                "the library has no files directory: set it with `papers config filesdir DIR` first",
            ));
        }
        for (key, files) in plan {
            let keep: Vec<String> = self
                .library
                .files(key)
                .into_iter()
                .filter(|f| !files.contains(f))
                .collect();
            let mut config = self.config.clone();
            config.filesdir = self.library.filesdir.clone();
            let placement = crate::command::files::files_policy_with(
                &config,
                crate::command::files::Mode::Copy,
                self.say.clone(),
                self.prompt(),
                &crate::command::files::PolicyOptions {
                    keep,
                    pool_dirs: dirs.to_vec(),
                    ..Default::default()
                },
            );
            self.library
                .rename_entry_files(key, &placement.policy, None, None, true)?;
            placement.check()?;
        }
        Ok(())
    }

    /// Make the pool of `config` (the new pool setting) the pool of the exchanges of this session, so that its
    /// entries can be looked up (`new_pools`). The pool, if it can be read.
    pub fn use_new_pool(&mut self, config: &crate::config::Config) -> Option<PoolIx> {
        let locking = self.holds_lock();
        let message = self.message().to_string();
        let exchange = self.exchange();
        let mut pools = crate::pools::Pools::new(config);
        if locking {
            pools.lock_as(&message);
        }
        pools.set_known_ids(exchange.origin.pool_ids());
        let pool = pools.pool(true);
        exchange.pools = pools;
        pool
    }

    /// What linking the library to the files of `pool` does (`papers config pool PATH`): each file that a library
    /// entry links to (existing files) must be linked by the pool entry of the same paper: the same file, or a file
    /// of the same content. `(plan, missing)`: the links to replace by the pool's copy, and the entries with a file
    /// that the pool entry does not have (or that the pool does not have).
    pub fn plan_relink(&mut self, pool: PoolIx) -> Result<(RelinkPlan, Vec<String>)> {
        self.exchange();
        let exchange = self.exchange.as_mut().expect("the exchange");
        let pool_relative = exchange.pool_relative(pool)?;
        let (mut plan, mut missing): (RelinkPlan, Vec<String>) = (Vec::new(), Vec::new());
        let keys: Vec<String> = self.library.entries().map(|e| e.key.clone()).collect();
        for key in keys {
            let Some(e) = self.library.get(&key).cloned() else {
                continue;
            };
            let files: Vec<String> = entry::files(&e, Some(&self.library.relative_to))
                .into_iter()
                .filter(|f| crate::config::paths::isfile(f))
                .collect();
            if files.is_empty() {
                continue;
            }
            let (pe, _) = exchange.pool_entry(&self.library, pool, &e)?;
            let pool_files: Vec<String> = match pe {
                Some(pe) => {
                    let pool_entry = exchange.pool_entry_of(pool, pe)?;
                    entry::files(&pool_entry, Some(&pool_relative))
                        .into_iter()
                        .filter(|f| crate::config::paths::isfile(f))
                        .collect()
                }
                None => Vec::new(),
            };
            let same: HashSet<String> = pool_files.iter().map(|f| realpath(f)).collect();
            let mut hashes: Option<Vec<(String, String)>> = None;
            let mut actions: Vec<(String, String)> = Vec::new();
            let mut complete = true;
            for f in &files {
                if same.contains(&realpath(f)) {
                    continue;
                }
                let hashes = hashes.get_or_insert_with(|| {
                    let mut seen: Vec<(String, String)> = Vec::new();
                    for pf in &pool_files {
                        let h = crate::merge::checksum_hex(pf).unwrap_or_default();
                        if !seen.iter().any(|(x, _)| *x == h) {
                            seen.push((h, pf.clone()));
                        }
                    }
                    seen
                });
                let digest = crate::merge::checksum_hex(f).unwrap_or_default();
                match hashes.iter().find(|(h, _)| *h == digest) {
                    Some((_, target)) => actions.push((f.clone(), target.clone())),
                    None => {
                        missing.push(e.key.clone());
                        complete = false;
                        break;
                    }
                }
            }
            if complete && !actions.is_empty() {
                plan.push((e.key.clone(), actions));
            }
        }
        Ok((plan, missing))
    }

    /// Apply [`Session::plan_relink`]: the library entries link to the pool's copies. The files to delete once the
    /// library is saved: the library's own copies (in its files directory) that no entry of the library or of the
    /// pool links to, outside `keep_dirs` (files directories of pools). `same` has each of them with its pool copy.
    pub fn relink(
        &mut self,
        plan: &RelinkPlan,
        pool: PoolIx,
        keep_dirs: &[String],
    ) -> Result<Vec<String>> {
        let filesdir = self.library.filesdir.clone();
        let mut delete: Vec<String> = Vec::new();
        let mut same: Vec<(String, String)> = Vec::new();
        for (key, actions) in plan {
            let mapping: Vec<(String, String)> = actions
                .iter()
                .map(|(f, t)| (realpath(f), t.clone()))
                .collect();
            let mut files: Vec<String> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            for f in self.library.files(key) {
                let f = mapping
                    .iter()
                    .find(|(r, _)| *r == realpath(&f))
                    .map_or(f, |(_, t)| t.clone());
                if seen.insert(realpath(&f)) {
                    files.push(f);
                }
            }
            self.library.set_files(key, &files)?;
            delete.extend(
                actions
                    .iter()
                    .filter(|(f, _)| {
                        filesdir
                            .as_deref()
                            .is_some_and(|d| crate::config::paths::is_inside(f, d))
                    })
                    .map(|(f, _)| f.clone()),
            );
            same.extend(actions.iter().cloned());
        }
        let mut linked: HashSet<String> = self
            .library
            .entries()
            .flat_map(|e| entry::files(e, Some(&self.library.relative_to)))
            .map(|f| realpath(&f))
            .collect();
        let exchange = self.exchange.as_mut().expect("the exchange");
        if let Ok(library) = exchange.pools.at_mut(pool).library() {
            for e in library.entries() {
                linked.extend(
                    entry::files(e, Some(&library.relative_to))
                        .iter()
                        .map(|f| realpath(f)),
                );
            }
        }
        exchange.same.extend(same);
        let dirs: Vec<&String> = keep_dirs.iter().filter(|d| !d.is_empty()).collect();
        let mut out: Vec<String> = Vec::new();
        for f in delete {
            if !out.contains(&f)
                && !linked.contains(&realpath(&f))
                && !dirs.iter().any(|d| crate::config::paths::is_inside(&f, d))
            {
                out.push(f);
            }
        }
        Ok(out)
    }

    /// The files deleted by a relink, each with its identical copy that stays (for [`Session::delete_files`]).
    pub fn relinked_same(&self) -> Vec<(String, String)> {
        self.exchange
            .as_ref()
            .map(|e| e.same.clone())
            .unwrap_or_default()
    }
}
