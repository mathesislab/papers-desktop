//! What involves a library and a pool (`papers.pools.Exchange`): the pool entry of a library
//! entry (and the other way), `pull`, `push`, the pool links of a shallow library, the repair of
//! broken links to pool files. The command that uses it saves: the pools that changed, the
//! origin records, then the library (see [`crate::command::session::Session`]).
//!
//! Entries are named by their keys in the library of the command, and by handles in a pool (see
//! [`crate::library::Library::uids`]). Spec `exchange.md`.

use std::collections::{HashMap, HashSet};

use crate::bib::{Entry, encoding::parse_file};
use crate::command::files::{Mode, Placement, PolicyOptions, files_policy_with};
use crate::command::session::{NoPrompt, Say, SharedPrompt};
use crate::config::Config;
use crate::config::paths::{abspath, basename, exists, is_inside, join, realpath};
use crate::entry::{self, format_files, set_files};
use crate::error::{Error, Result};
use crate::identity::{Distinct, Kind, Verdict, same_paper};
use crate::library::{InsertOptions, Library, Policy};
use crate::log;
use crate::merge::{Context, Decision, Merge, Resolve, Resolver, apply_entry};
use crate::names::append_abc_in;

use super::origin::{Origin, Record};
use super::pool::{IdEvent, PoolError, PoolIx, Pools};

/// What separates the name of an entry from the label of its other files.
const ATTACHMENT_SEP: &str = "--";

/// What an exchange did with an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    Updated,
    Linked,
    Unchanged,
    Skipped,
    NotFound,
    Added,
    /// not pushed: no fields or no key
    Empty,
    /// the file links of the pool entry were updated
    Links,
    /// the entry is no longer in the pool
    Removed,
}

/// The outcome for an entry (its key in the library), and the pool it concerns.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub key: String,
    pub pool: Option<PoolIx>,
    pub what: What,
}

impl Outcome {
    fn new(key: &str, pool: Option<PoolIx>, what: What) -> Outcome {
        Outcome {
            key: key.to_string(),
            pool,
            what,
        }
    }
}

/// What replaced each broken link of an entry: `[(link, the files that took its place)]`.
pub type Relinks = Vec<(String, Vec<String>)>;

/// The entries whose broken links to pool files were repaired: their keys, and what replaced what.
pub type Repaired = Vec<(String, Relinks)>;

/// The links of a text of a `file` field, as absolute paths (`parse_file(text, relative_to)`).
pub fn links_of(text: &str, relative_to: &str) -> Vec<String> {
    parse_file(text)
        .iter()
        .map(|f| abspath(&join(relative_to, f)))
        .collect()
}

fn real_set(files: &[String]) -> HashSet<String> {
    files.iter().map(|f| realpath(f)).collect()
}

/// A copy of an entry of a library for the other, its file links written for the other: `key` for
/// it if given; without `ids` (former keys mean nothing in the other library), and without a
/// `file` field if it has no link (`exchange_copy`).
pub fn exchange_copy(
    entry: &Entry,
    source_relative_to: &str,
    target_relative_to: &str,
    key: Option<&str>,
) -> Entry {
    let mut e = entry.clone();
    e.source = None;
    if let Some(key) = key {
        e.key = key.to_string();
    }
    let files = entry::files(entry, Some(source_relative_to));
    e.remove("file");
    if !files.is_empty() {
        set_files(&mut e, &files, target_relative_to);
    }
    e.remove("ids");
    e
}

fn rekey(e: &Entry, key: &str) -> Entry {
    let mut e = e.clone();
    e.source = None;
    e.key = key.to_string();
    e
}

/// The note on an entry that a pool no longer has (`removed_note`): `options` are those of the
/// command (`--pool PATH`), to repeat in it.
pub fn removed_note(key: &str, label: &str, options: &str) -> String {
    format!("{key}: removed from {label} (`papers push{options} {key}` adds it back)")
}

/// A library (configuration and entries) with its pools and origin records: the pool of the
/// library, and the libraries given with `--pool PATH`. The origin records are used and written for
/// every pool; the file links of a shallow library are those of the pool entries for its own pool
/// only.
pub struct Exchange {
    pub config: Config,
    pub origin: Origin,
    pub pools: Pools,
    /// The pools to save.
    pub changed_pools: Vec<PoolIx>,
    /// The pools pushed to (changed or not): [`Exchange::after_push`] after the save.
    pub pushed_pools: Vec<PoolIx>,
    /// pool id -> [the pool had not changed since the last exchange, a pushed entry left it ahead
    /// of the library]
    after: HashMap<String, [bool; 2]>,
    pool_id: Option<String>,
    /// (lower-case key of a library entry, pool id) -> its links that the pool removed, which a push leaves
    /// out and the next record keeps (see [`Exchange::outgoing`]).
    removed_links: HashMap<(String, String), Vec<String>>,
    /// A file deleted -> its identical copy that stays (see `relink`).
    pub same: Vec<(String, String)>,
    dry_run: bool,
    say: Say,
    prompt: SharedPrompt,
}

impl Exchange {
    /// The exchange of the library of `config`, whose bibtex is `bibtex`.
    pub fn new(
        config: &Config,
        dry_run: bool,
        pools: Option<Pools>,
        say: Say,
        prompt: SharedPrompt,
    ) -> Exchange {
        Exchange::build(config, dry_run, pools, say, prompt, true)
    }

    fn build(
        config: &Config,
        dry_run: bool,
        pools: Option<Pools>,
        say: Say,
        prompt: SharedPrompt,
        moves: bool,
    ) -> Exchange {
        let origin = match config.bibtex {
            Some(_) => Origin::for_library(&config.libdir(), dry_run),
            None => Origin::default(),
        };
        let mut pools = pools.unwrap_or_else(|| Pools::new(config));
        if moves {
            pools.move_records();
        }
        pools.set_known_ids(origin.pool_ids());
        let pool_id = pools.path().map(|p| pools.id_of(&p));
        let mut exchange = Exchange {
            config: config.clone(),
            origin,
            pools,
            changed_pools: Vec::new(),
            pushed_pools: Vec::new(),
            after: HashMap::new(),
            pool_id,
            removed_links: HashMap::new(),
            same: Vec::new(),
            dry_run,
            say,
            prompt,
        };
        exchange.settle();
        exchange
    }

    /// An exchange for `papers status`: it does not move the records of a path id to the identifier of
    /// the pool (see [`Exchange::begin_moves`]).
    pub fn for_status(config: &Config) -> Exchange {
        let prompt: SharedPrompt = std::sync::Arc::new(std::sync::Mutex::new(Box::new(NoPrompt)));
        Exchange::build(config, false, None, Say::new(None), prompt, false)
    }

    /// From here the records of a path id move to the identifier of the pool's library (in memory: the
    /// command that saves them is the one that exchanges).
    pub fn begin_moves(&mut self) {
        self.pools.move_records();
        self.pools.set_known_ids(self.origin.pool_ids());
        self.pool_id = self.pools.path().map(|p| self.pools.id_of(&p));
        self.settle();
    }

    /// What the pool ids found say to the origin records: the records of a path id move to the identifier
    /// of the pool's library, and the pool is located for the name of its file (saved with the records).
    fn settle(&mut self) {
        for event in self.pools.take_events() {
            match event {
                IdEvent::Moved { from, to, path } => {
                    self.origin.rename_pool(&from, &to, Some(&path))
                }
                IdEvent::Located { id, path } => self.origin.locate(&id, &path),
            }
        }
    }

    /// The pool that `pull`, `push` and the shallow libraries exchange with ([`Pools::target`]).
    pub fn target(&mut self) -> std::result::Result<PoolIx, PoolError> {
        let ix = self.pools.target();
        self.settle();
        ix
    }

    /// A library given on the command line, without pool ([`Pools::get`]).
    pub fn get_pool(&mut self, path: &str) -> std::result::Result<PoolIx, PoolError> {
        let ix = self.pools.get(path);
        self.settle();
        ix
    }

    /// A library given on the command line, which may have a pool ([`Pools::get_library`]).
    pub fn get_library(&mut self, path: &str) -> std::result::Result<PoolIx, PoolError> {
        let ix = self.pools.get_library(path);
        self.settle();
        ix
    }

    /// The pool of the library, if it can be read ([`Pools::pool`]).
    pub fn pool(&mut self, quiet: bool) -> Option<PoolIx> {
        let ix = self.pools.pool(quiet);
        self.settle();
        ix
    }

    /// An exchange without terminal and without output sink (what only reads).
    pub fn quiet(config: &Config, dry_run: bool) -> Exchange {
        let prompt: SharedPrompt = std::sync::Arc::new(std::sync::Mutex::new(Box::new(NoPrompt)));
        Exchange::new(config, dry_run, None, Say::new(None), prompt)
    }

    fn changed(&mut self, pool: PoolIx) {
        if !self.changed_pools.contains(&pool) {
            self.changed_pools.push(pool);
        }
    }

    /// Whether `pool` is the pool of the library (rather than a library given with `--pool PATH`).
    pub fn is_own(&self, pool: PoolIx) -> bool {
        self.pool_id
            .as_ref()
            .is_some_and(|id| *id == self.pools.at(pool).id)
    }

    /// A shallow library and its own pool: the library entry's file links are the pool entry's.
    fn shares_links(&self, pool: PoolIx) -> bool {
        self.config.shallow() && self.is_own(pool)
    }

    pub fn label(&self, pool: PoolIx) -> String {
        self.pools.at(pool).label()
    }

    /// A copy of the entry of a handle of a pool.
    pub fn pool_entry_of(&mut self, pool: PoolIx, uid: u64) -> Result<Entry> {
        self.pool_entry_copy(pool, uid)
    }

    fn pool_entry_copy(&mut self, pool: PoolIx, uid: u64) -> Result<Entry> {
        let library = self.pools.at_mut(pool).library()?;
        Ok(library.entry_by_uid(uid).expect("a pool entry").clone())
    }

    /// What the links of the bibtex of a pool are relative to.
    pub fn pool_relative(&mut self, pool: PoolIx) -> Result<String> {
        self.pool_relative_to(pool)
    }

    fn pool_relative_to(&mut self, pool: PoolIx) -> Result<String> {
        Ok(self.pools.at_mut(pool).library()?.relative_to.clone())
    }

    // finding the pool entry of a library entry
    // -----------------------------------------

    /// The pool entry and the origin record of a library entry in `pool`: with an origin record,
    /// the pool entry of its pool key if it is the same paper as the origin copy or as the entry,
    /// else the same paper as the origin copy, then as the entry, else the pool entry of the pool
    /// key if its identifiers do not contradict the origin copy; without record, the same paper
    /// as the entry (record none). `(None, record)` if the pool does not have it.
    pub fn pool_entry(
        &mut self,
        lib: &Library,
        pool: PoolIx,
        e: &Entry,
    ) -> Result<(Option<u64>, Option<Record>)> {
        let id = self.pools.at(pool).id.clone();
        let record = self.origin.get(e, Some(&id));
        let p = self.pools.at_mut(pool);
        let Some(record) = record else {
            return Ok((p.find(e, &lib.distinct)?, None));
        };
        if let Some(uid) = p.entry_of_key(&record.pool_key)? {
            let union = p.distinct_with(&lib.distinct)?;
            let pe = p.library()?.entry_by_uid(uid).expect("an entry").clone();
            if same_paper(&pe, &record.entry, Some(&union)).kind == Kind::Same
                || same_paper(&pe, e, Some(&union)).kind == Kind::Same
            {
                return Ok((Some(uid), Some(record)));
            }
        }
        let mut found = p.find(&record.entry, &lib.distinct)?;
        if found.is_none() {
            found = p.find(e, &lib.distinct)?;
        }
        if found.is_none() {
            found = p.by_key(&record.pool_key, &record.entry)?;
        }
        Ok((found, Some(record)))
    }

    /// The pairs that are not the same paper, united both ways.
    pub fn merge_distinct(&mut self, lib: &mut Library, pool: PoolIx) -> Result<()> {
        let theirs = self.pools.at_mut(pool).library()?.distinct.clone();
        lib.distinct.update(&theirs);
        let ours = lib.distinct.clone();
        self.pools.at_mut(pool).library()?.distinct.update(&ours);
        Ok(())
    }

    // the file links of a shallow library: those of the pool entry
    // -------------------------------------------------------------

    /// Record the origin of a library entry (`origin.set`): `base`, and in a shallow library, for
    /// its pool, the file links of the pool entry `pe`.
    pub(crate) fn record(
        &mut self,
        lib: &Library,
        key: &str,
        pool: PoolIx,
        base: &Entry,
        pe: u64,
    ) -> Result<()> {
        let id = self.pools.at(pool).id.clone();
        let files = if self.shares_links(pool) {
            let pending = self
                .removed_links
                .remove(&(key.to_lowercase(), id.clone()))
                .unwrap_or_default();
            let p = self.pools.at_mut(pool).library()?;
            let e = p.entry_by_uid(pe).expect("a pool entry");
            let mut links = entry::files(e, Some(&p.relative_to));
            links.extend(pending);
            Some(format_files(&links, Some(&lib.relative_to)))
        } else {
            None
        };
        self.origin.set(key, &id, base, files.as_deref());
        Ok(())
    }

    /// The library entry as push sends it to the pool entry `pe`: without the file links that the pool
    /// removed since the last exchange (see [`Exchange::pool_removed`]; pull removes them from the
    /// library). They stay in the record that the push writes, so that the next pull removes them.
    pub(crate) fn outgoing(
        &mut self,
        lib: &Library,
        e: &Entry,
        pool: PoolIx,
        pe: u64,
        record: Option<&Record>,
    ) -> Result<Entry> {
        let pool_relative = self.pool_relative_to(pool)?;
        let key = self.pool_entry_copy(pool, pe)?.key;
        let mut out = exchange_copy(e, &lib.relative_to, &pool_relative, Some(&key));
        let removed = self.pool_removed(lib, e, pe, record, pool)?;
        if !removed.is_empty() {
            let kept: Vec<String> = entry::files(&out, Some(&pool_relative))
                .into_iter()
                .filter(|f| !removed.contains(&realpath(f)))
                .collect();
            set_files(&mut out, &kept, &pool_relative);
            let pending: Vec<String> = entry::files(e, Some(&lib.relative_to))
                .into_iter()
                .filter(|f| removed.contains(&realpath(f)))
                .collect();
            let id = self.pools.at(pool).id.clone();
            self.removed_links
                .insert((e.key.to_lowercase(), id), pending);
        }
        Ok(out)
    }

    /// In a shallow library, the file links that the command added to a library entry (not there
    /// when the library was loaded): they go to the pool entry as they are, wherever they point.
    fn new_links(&self, lib: &Library, e: &Entry, pool: PoolIx) -> Vec<String> {
        if !self.shares_links(pool) {
            return Vec::new();
        }
        let Some(before) = lib.loaded_files(e) else {
            return Vec::new();
        };
        let old = real_set(&before);
        entry::files(e, Some(&lib.relative_to))
            .into_iter()
            .filter(|f| !old.contains(&realpath(f)))
            .collect()
    }

    /// In a shallow library, the file links of a library entry that the pool entry had at the last
    /// exchange (origin record) and no longer has: real paths. A link that the pool entry never had
    /// stays.
    pub(crate) fn pool_removed(
        &mut self,
        lib: &Library,
        e: &Entry,
        pe: u64,
        record: Option<&Record>,
        pool: PoolIx,
    ) -> Result<HashSet<String>> {
        let Some(files) = record.and_then(|r| r.files.as_deref()) else {
            return Ok(HashSet::new());
        };
        if !self.shares_links(pool) {
            return Ok(HashSet::new());
        }
        let base = real_set(&links_of(files, &lib.relative_to));
        let p = self.pools.at_mut(pool).library()?;
        let now = real_set(&entry::files(
            p.entry_by_uid(pe).expect("a pool entry"),
            Some(&p.relative_to),
        ));
        let mine = real_set(&entry::files(e, Some(&lib.relative_to)));
        let gone: HashSet<String> = base.difference(&now).cloned().collect();
        Ok(mine.intersection(&gone).cloned().collect())
    }

    // files
    // -----

    /// What to do with the files of entries taken from a pool: in a shallow library, those of
    /// its pool are linked where they are; otherwise as the files mode says (a pool file is
    /// copied, never moved). In a shallow library, the files of another library go to the files
    /// directory of its pool. `pools`: the pools that the files come from (default: the pool of
    /// the library).
    pub fn files_policy(
        &mut self,
        lib: &Library,
        mode: Option<Mode>,
        keep: &[String],
        pools: &[PoolIx],
    ) -> Placement {
        let own = pools.is_empty() || pools.iter().any(|p| self.is_own(*p));
        let mut mode = if self.config.shallow() && own {
            Mode::Link
        } else {
            mode.or_else(|| {
                self.config
                    .get("files")
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .and_then(Mode::parse)
            })
            .unwrap_or(Mode::Move)
        };
        if lib.filesdir.is_none() {
            mode = Mode::Link;
        }
        let mut pool_dirs = self.pools.files_dirs();
        pool_dirs.extend(pools.iter().filter_map(|p| self.pools.at(*p).filesdir()));
        let options = PolicyOptions {
            keep: keep.to_vec(),
            pool_dirs,
            in_pool: self.config.shallow(),
            ..PolicyOptions::default()
        };
        files_policy_with(
            &self.config,
            mode,
            self.say.clone(),
            self.prompt.clone(),
            &options,
        )
    }

    fn place_files(
        &self,
        lib: &mut Library,
        key: &str,
        placement: &Placement,
        link: bool,
    ) -> Result<()> {
        if link || lib.filesdir.is_none() {
            return Ok(());
        }
        lib.rename_entry_files(key, &placement.policy, None, None, true)?;
        placement.check()
    }

    fn mode_is_link(&self, lib: &Library, mode: Option<Mode>, pools: &[PoolIx]) -> bool {
        let own = pools.is_empty() || pools.iter().any(|p| self.is_own(*p));
        if lib.filesdir.is_none() || (self.config.shallow() && own) {
            return true;
        }
        let mode = mode
            .or_else(|| {
                self.config
                    .get("files")
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .and_then(Mode::parse)
            })
            .unwrap_or(Mode::Move);
        mode == Mode::Link
    }

    // pull
    // ----

    /// The pool key (lower case) -> the library keys of its records, the library keys by lower case.
    fn library_lookup(&self, lib: &Library, pool: PoolIx) -> HashMap<String, Vec<String>> {
        let id = &self.pools.at(pool).id;
        let mut by_pool_key: HashMap<String, Vec<String>> = HashMap::new();
        for (key, pool_key) in self.origin.pool_keys(id) {
            by_pool_key.entry(pool_key).or_default().push(key);
        }
        let _ = lib;
        by_pool_key
    }

    /// The library entry (its key) of a pool entry: the entry whose origin record has its pool
    /// key, else the same paper; or none.
    pub fn library_entry(
        &mut self,
        lib: &mut Library,
        pe: &Entry,
        pool: PoolIx,
    ) -> Result<Option<String>> {
        let by_pool_key = self.library_lookup(lib, pool);
        self.library_entry_with(lib, pe, pool, &by_pool_key)
    }

    fn library_entry_with(
        &mut self,
        lib: &mut Library,
        pe: &Entry,
        pool: PoolIx,
        by_pool_key: &HashMap<String, Vec<String>>,
    ) -> Result<Option<String>> {
        for key in by_pool_key
            .get(&pe.key.to_lowercase())
            .into_iter()
            .flatten()
        {
            if let Some(e) = lib.uid_of_key(key).and_then(|u| lib.entry_by_uid(u)) {
                return Ok(Some(e.key.clone()));
            }
        }
        let theirs = self.pools.at_mut(pool).library()?.distinct.clone();
        let mut union = Distinct::new();
        union.update(&lib.distinct);
        union.update(&theirs);
        let found = lib.find_same(pe, Some(&union));
        Ok(found
            .into_iter()
            .find(|(_, v): &(u64, Verdict)| v.kind == Kind::Same)
            .and_then(|(uid, _)| lib.entry_by_uid(uid).map(|e| e.key.clone())))
    }

    /// The entries of the pool (their handles) that are not in the library (no library entry).
    pub fn missing(&mut self, lib: &mut Library, pool: PoolIx) -> Result<Vec<u64>> {
        let by_pool_key = self.library_lookup(lib, pool);
        let uids = self.pools.at_mut(pool).library()?.uids();
        let mut missing = Vec::new();
        for uid in uids {
            let pe = self.pool_entry_copy(pool, uid)?;
            if self
                .library_entry_with(lib, &pe, pool, &by_pool_key)?
                .is_none()
            {
                missing.push(uid);
            }
        }
        Ok(missing)
    }

    /// `(shared, removed)` among the entries (keys): the entries shared with `pool`, those whose
    /// paper the pool has (by origin record, else by the same-paper rule), and the entries that
    /// the pool no longer has although they have an origin record of it (removed from the pool
    /// since the last exchange).
    pub fn sharing(
        &mut self,
        lib: &Library,
        keys: &[String],
        pool: PoolIx,
    ) -> Result<(Vec<String>, Vec<String>)> {
        let (mut shared, mut removed) = (Vec::new(), Vec::new());
        for key in keys {
            let Some(e) = lib.get(key).cloned() else {
                continue;
            };
            let (pe, record) = self.pool_entry(lib, pool, &e)?;
            if pe.is_some() {
                shared.push(key.clone());
            } else if record.is_some() {
                removed.push(key.clone());
            }
        }
        Ok((shared, removed))
    }

    /// Drop the origin records of `pool` of entries that it no longer has: they are no longer
    /// shared with it.
    pub fn forget(&mut self, lib: &Library, keys: &[String], pool: PoolIx) {
        let id = self.pools.at(pool).id.clone();
        for key in keys {
            let Some(e) = lib.get(key) else { continue };
            if let Some(record) = self.origin.get(e, Some(&id)) {
                self.origin.remove(&record.key, &id);
            }
        }
    }

    /// Three-way merge of the entries of `pool` into the library entries, and the pool entries
    /// `new` added to the library. An entry that the pool does not have is reported (`NotFound`)
    /// only if it is one of `named` (the entries named on the command line). Without a pool,
    /// nothing is merged.
    #[allow(clippy::too_many_arguments)]
    pub fn pull(
        &mut self,
        lib: &mut Library,
        entries: &[String],
        pool: Option<PoolIx>,
        resolver: &mut dyn Resolve,
        mode: Option<Mode>,
        record_pull: bool,
        new: &[u64],
        named: &[String],
    ) -> Result<Vec<Outcome>> {
        let mut report = Vec::new();
        let mut before = None;
        if let Some(pool) = pool {
            self.merge_distinct(lib, pool)?;
            before = self.pools.at(pool).bibtex_mtime();
        }
        for key in entries {
            if resolver.quit() {
                break;
            }
            let Some(e) = lib.get(key).cloned() else {
                continue;
            };
            let (pe, record) = match pool {
                Some(pool) => self.pool_entry(lib, pool, &e)?,
                None => (None, None),
            };
            let (Some(pool), Some(pe)) = (pool, pe) else {
                if named.contains(key) {
                    report.push(Outcome::new(key, None, What::NotFound));
                }
                continue;
            };
            let what = self.pull_one(lib, &e, pool, pe, record, resolver, mode)?;
            report.push(Outcome::new(&e.key, Some(pool), what));
        }
        if let Some(pool) = pool {
            for uid in new {
                let key = self.pull_new(lib, *uid, pool, mode)?;
                report.push(Outcome::new(&key, Some(pool), What::Added));
            }
        }
        // a pull that skipped (or quit) leaves pool changes to take: not a pull of the pool
        if record_pull
            && let Some(pool) = pool
            && !resolver.quit()
            && !report.iter().any(|o| o.what == What::Skipped)
        {
            let id = self.pools.at(pool).id.clone();
            self.origin.set_pulled(&id, before);
        }
        Ok(report)
    }

    #[allow(clippy::too_many_arguments)]
    fn pull_one(
        &mut self,
        lib: &mut Library,
        e: &Entry,
        pool: PoolIx,
        pe: u64,
        record: Option<Record>,
        resolver: &mut dyn Resolve,
        mode: Option<Mode>,
    ) -> Result<What> {
        let mut e = e.clone();
        if self.shares_links(pool) {
            self.repair_links(lib, Some(std::slice::from_ref(&e.key)), pool)?;
            e = lib.get(&e.key).cloned().unwrap_or(e);
        }
        let pool_relative = self.pool_relative_to(pool)?;
        let pool_entry = self.pool_entry_copy(pool, pe)?;
        let incoming = exchange_copy(&pool_entry, &pool_relative, &lib.relative_to, None);
        let merge = Merge::exchange(
            e.clone(),
            incoming,
            Some(&lib.relative_to),
            record.as_ref().map(|r| &r.entry),
        );
        let removed = self.pool_removed(lib, &e, pe, record.as_ref(), pool)?;
        if merge.conflicts().is_empty()
            && merge.additions().is_empty()
            && merge.new_files.is_empty()
            && removed.is_empty()
        {
            self.record(lib, &e.key, pool, &pool_entry, pe)?;
            return Ok(if record.is_some() {
                What::Unchanged
            } else {
                What::Linked
            });
        }
        let new = if !merge.conflicts().is_empty() {
            let verdict = Verdict {
                kind: Kind::Same,
                reason: self.label(pool),
            };
            match resolver
                .decide(merge, Context::Pull, &verdict)
                .map_err(Error::from)?
            {
                Decision::Merge(_, entry) => entry,
                _ => return Ok(What::Skipped),
            }
        } else {
            merge.result()
        };
        let old_files = lib.files(&e.key);
        if let Some(target) = lib.get_mut(&e.key) {
            apply_entry(target, &new);
        }
        if !removed.is_empty() {
            // the pool entry no longer links to them
            let kept: Vec<String> = lib
                .files(&e.key)
                .into_iter()
                .filter(|f| !removed.contains(&realpath(f)))
                .collect();
            lib.set_files(&e.key, &kept)?;
        }
        let placement = self.files_policy(lib, mode, &old_files, &[pool]);
        let link = self.mode_is_link(lib, mode, &[pool]);
        self.place_files(lib, &e.key, &placement, link)?;
        self.record(lib, &e.key, pool, &pool_entry, pe)?;
        Ok(What::Updated)
    }

    /// Add a pool entry to the library, under the pool key (with a letter suffix if the library
    /// has this key already); its files are linked in a shallow library (its own pool), else placed
    /// as the files mode says. The key of the new entry.
    fn pull_new(
        &mut self,
        lib: &mut Library,
        pe: u64,
        pool: PoolIx,
        mode: Option<Mode>,
    ) -> Result<String> {
        let pool_entry = self.pool_entry_copy(pool, pe)?;
        let keys: HashSet<String> = lib.entries().map(|e| e.key.to_lowercase()).collect();
        let key = if keys.contains(&pool_entry.key.to_lowercase()) {
            append_abc_in(&pool_entry.key, |k| keys.contains(k)).map_err(Error::exit)?
        } else {
            pool_entry.key.clone()
        };
        let pool_relative = self.pool_relative_to(pool)?;
        let e = exchange_copy(&pool_entry, &pool_relative, &lib.relative_to, Some(&key));
        lib.add_unchecked(e);
        let placement = self.files_policy(lib, mode, &[], &[pool]);
        let link = self.mode_is_link(lib, mode, &[pool]);
        self.place_files(lib, &key, &placement, link)?;
        self.record(lib, &key, pool, &pool_entry, pe)?;
        Ok(key)
    }

    /// The report of `pull --check` with `pool`: a line on the pool if it changed since the last
    /// pull, on the entries whose pool version changed since it was taken, and on those removed
    /// from the pool. No merge, nothing written. `options`: those of the command, to repeat in a note.
    pub fn check(
        &mut self,
        lib: &Library,
        keys: &[String],
        pool: PoolIx,
        all_entries: bool,
        options: &str,
    ) -> Result<Vec<String>> {
        let mut lines = Vec::new();
        if all_entries {
            lines.push(format!("{}: {}", self.label(pool), self.pool_state(pool)));
        }
        let pool_relative = self.pool_relative_to(pool)?;
        for key in keys {
            let Some(e) = lib.get(key).cloned() else {
                continue;
            };
            let (pe, record) = self.pool_entry(lib, pool, &e)?;
            let Some(pe) = pe else {
                if record.is_some() {
                    lines.push(removed_note(&e.key, &self.label(pool), options));
                }
                continue;
            };
            let pool_entry = self.pool_entry_copy(pool, pe)?;
            let incoming = exchange_copy(&pool_entry, &pool_relative, &lib.relative_to, None);
            let Some(record) = record else {
                let merge = Merge::exchange(e.clone(), incoming, Some(&lib.relative_to), None);
                if !merge.is_exact() || !merge.new_files.is_empty() {
                    lines.push(format!(
                        "{}: in {} as {}, not pulled yet",
                        e.key,
                        self.label(pool),
                        pool_entry.key
                    ));
                }
                continue;
            };
            let merge = Merge::exchange(
                e.clone(),
                incoming,
                Some(&lib.relative_to),
                Some(&record.entry),
            );
            let mut changed: Vec<String> = merge
                .additions()
                .iter()
                .chain(merge.conflicts().iter())
                .map(|f| f.name.clone())
                .collect();
            if !merge.new_files.is_empty()
                || !self
                    .pool_removed(lib, &e, pe, Some(&record), pool)?
                    .is_empty()
            {
                changed.push("file".to_string());
            }
            if !changed.is_empty() {
                let n = merge.conflicts().len();
                let note = if n > 0 {
                    format!(" ({n} conflict{})", if n > 1 { "s" } else { "" })
                } else {
                    String::new()
                };
                changed.sort();
                lines.push(format!(
                    "{}: changed in {}: {}{note}",
                    e.key,
                    self.label(pool),
                    changed.join(", ")
                ));
            }
        }
        Ok(lines)
    }

    /// Whether a pool changed since the last exchange with it (pull, push or add: a modification
    /// time check): `not pulled in full yet`, `not pulled from yet`, `changed since last pull or push`, `up to date`.
    pub fn pool_state(&self, pool: PoolIx) -> &'static str {
        let id = &self.pools.at(pool).id;
        let current = self.pools.at(pool).bibtex_mtime();
        match self.origin.pulled(id) {
            None => {
                if self.origin.records(Some(id)).is_empty() {
                    "not pulled from yet"
                } else {
                    "not pulled in full yet"
                }
            }
            Some(pulled) if Some(pulled) != current => "changed since last pull or push",
            Some(_) => "up to date",
        }
    }

    // push
    // ----

    /// Three-way merge of the library entries into the pool entries; entries new to the pool are
    /// added to it, with a key from the pool's key template (`include_new`).
    pub fn push(
        &mut self,
        lib: &mut Library,
        keys: &[String],
        pool: PoolIx,
        resolver: &mut dyn Resolve,
        include_new: bool,
    ) -> Result<Vec<Outcome>> {
        self.pools.at_mut(pool).check_readable()?;
        self.merge_distinct(lib, pool)?;
        let id = self.pools.at(pool).id.clone();
        if !self.after.contains_key(&id) {
            let before = self.pools.at(pool).bibtex_mtime();
            self.after.insert(
                id,
                [
                    before.is_none() || self.origin.pulled(&self.pools.at(pool).id) == before,
                    false,
                ],
            );
        }
        let mut report = Vec::new();
        for key in keys {
            if resolver.quit() {
                break;
            }
            let Some(e) = lib.get(key).cloned() else {
                continue;
            };
            let (pe, record) = self.pool_entry(lib, pool, &e)?;
            match pe {
                None => {
                    if include_new {
                        let what = self.push_new(lib, &e, pool)?;
                        report.push(Outcome::new(&e.key, Some(pool), what));
                    }
                }
                Some(pe) => {
                    let what = self.push_one(lib, &e, pool, pe, record, resolver)?;
                    report.push(Outcome::new(&e.key, Some(pool), what));
                }
            }
        }
        if !self.pushed_pools.contains(&pool) {
            self.pushed_pools.push(pool);
        }
        if report
            .iter()
            .any(|o| matches!(o.what, What::Added | What::Updated))
        {
            self.changed(pool); // the pool entries changed
        }
        Ok(report)
    }

    /// After the pool was saved (a pool that the push did not change is not written): if it had not
    /// changed since the last exchange (or this is the first), the push does not count as a change
    /// of the pool, unless a pushed entry left the pool with something that the library does not
    /// have (a pull would bring it).
    pub fn after_push(&mut self, pool: PoolIx) {
        let id = self.pools.at(pool).id.clone();
        let Some(state) = self.after.get(&id) else {
            return;
        };
        if state[1] {
            return;
        }
        if state[0] || self.origin.first_exchange(&id) {
            let mtime = self.pools.at(pool).bibtex_mtime();
            self.origin.set_pulled(&id, mtime);
        }
    }

    /// After `add` took entries from the pool (not saved): the first exchange with the pool
    /// records its modification time, as a pull.
    pub fn after_take(&mut self, pool: PoolIx) {
        let id = self.pools.at(pool).id.clone();
        if self.origin.first_exchange(&id) && !self.origin.records(Some(&id)).is_empty() {
            let mtime = self.pools.at(pool).bibtex_mtime();
            self.origin.set_pulled(&id, mtime);
        }
    }

    /// Files pushed to a pool: copied (a hard link when possible), with the pool's name template.
    fn pool_placement(&self, pool: PoolIx, keep: &[String]) -> Placement {
        let p = self.pools.at(pool);
        let mut config = p.config.clone();
        config.filesdir = p.filesdir();
        let options = PolicyOptions {
            keep: keep.to_vec(),
            in_pool: true,
            ..PolicyOptions::default()
        };
        files_policy_with(
            &config,
            Mode::Copy,
            self.say.clone(),
            self.prompt.clone(),
            &options,
        )
    }

    fn push_new(&mut self, lib: &Library, e: &Entry, pool: PoolIx) -> Result<What> {
        let pool_relative = self.pool_relative_to(pool)?;
        let mut out = exchange_copy(e, &lib.relative_to, &pool_relative, None);
        let keep = self.new_links(lib, e, pool);
        let placement = self.pool_placement(pool, &keep);
        // an entry without fields, or without a key, is not written to the pool
        if out.fields.iter().all(|f| f.key == "file" || f.key == "ids") {
            log::info(&format!(
                "{}: not pushed to {}: the entry has no fields",
                if e.key.is_empty() {
                    "an entry without key"
                } else {
                    &e.key
                },
                self.label(pool)
            ));
            return Ok(What::Empty);
        }
        let label = self.label(pool);
        let filesdir = self.pools.at(pool).filesdir();
        let p = self.pools.at_mut(pool).library()?;
        let update_key = !p.keyformat.render(&out)?.is_empty();
        if !update_key {
            // the key template gives no key: the library key
            if e.key.is_empty() {
                log::info(&format!("an entry without key: not pushed to {label}"));
                return Ok(What::Empty);
            }
            let keys: HashSet<String> = p.entries().map(|x| x.key.to_lowercase()).collect();
            if keys.contains(&out.key.to_lowercase()) {
                out.key = append_abc_in(&out.key, |k| keys.contains(k)).map_err(Error::exit)?;
            }
        }
        let options = InsertOptions {
            update_key,
            check_duplicate: false,
            rename: filesdir.is_some().then(|| placement.policy.clone()),
            metadata: Vec::new(),
        };
        let mut resolver = Resolver::new(crate::merge::OnDuplicate::Error);
        let inserted = p.insert(out, &options, &mut resolver)?;
        placement.check()?;
        let uid = p.uid_of_key(&inserted.key).expect("the entry inserted");
        let new = p.entry_by_uid(uid).expect("the entry").clone();
        self.pools.at_mut(pool).added(uid);
        self.record(lib, &e.key, pool, &new, uid)?;
        Ok(What::Added)
    }

    fn push_one(
        &mut self,
        lib: &Library,
        e: &Entry,
        pool: PoolIx,
        pe: u64,
        record: Option<Record>,
        resolver: &mut dyn Resolve,
    ) -> Result<What> {
        let pool_relative = self.pool_relative_to(pool)?;
        let pool_entry = self.pool_entry_copy(pool, pe)?;
        let outgoing = self.outgoing(lib, e, pool, pe, record.as_ref())?;
        let base = record.as_ref().map(|r| rekey(&r.entry, &pool_entry.key));
        let merge = Merge::exchange(
            pool_entry.clone(),
            outgoing.clone(),
            Some(&pool_relative),
            base.as_ref(),
        );
        if merge.conflicts().is_empty()
            && merge.additions().is_empty()
            && merge.new_files.is_empty()
        {
            self.pushed(lib, e, pool, pe, &outgoing)?;
            return Ok(if record.is_some() {
                What::Unchanged
            } else {
                What::Linked
            });
        }
        let new = if !merge.conflicts().is_empty() {
            let verdict = Verdict {
                kind: Kind::Same,
                reason: self.label(pool),
            };
            match resolver
                .decide(merge, Context::Push, &verdict)
                .map_err(Error::from)?
            {
                Decision::Merge(_, entry) => entry,
                _ => return Ok(What::Skipped),
            }
        } else {
            merge.result()
        };
        let old_files = entry::files(&pool_entry, Some(&pool_relative));
        self.pools.at_mut(pool).update_entry(pe, &new);
        if self.pools.at(pool).filesdir().is_some() {
            // in a shallow library, the links added by the command are linked where they are
            let mut keep = old_files;
            keep.extend(self.new_links(lib, e, pool));
            let placement = self.pool_placement(pool, &keep);
            let key = new.key.clone();
            self.pools.at_mut(pool).library()?.rename_entry_files(
                &key,
                &placement.policy,
                None,
                None,
                true,
            )?;
            placement.check()?;
        }
        self.pushed(lib, e, pool, pe, &outgoing)?;
        Ok(What::Updated)
    }

    /// Record the origin of a pushed entry: the library's version, as sent, so that what the pool
    /// entry has beyond it comes with the next pull.
    fn pushed(
        &mut self,
        lib: &Library,
        e: &Entry,
        pool: PoolIx,
        pe: u64,
        outgoing: &Entry,
    ) -> Result<()> {
        self.record(lib, &e.key, pool, outgoing, pe)?;
        let pool_relative = self.pool_relative_to(pool)?;
        let pool_entry = self.pool_entry_copy(pool, pe)?;
        let incoming = exchange_copy(&pool_entry, &pool_relative, &lib.relative_to, None);
        let after = Merge::exchange(e.clone(), incoming, Some(&lib.relative_to), Some(outgoing));
        if !after.additions().is_empty()
            || !after.conflicts().is_empty()
            || !after.new_files.is_empty()
        {
            let id = self.pools.at(pool).id.clone();
            if let Some(state) = self.after.get_mut(&id) {
                state[1] = true;
            }
        }
        Ok(())
    }

    /// A shallow library: the pool entries of `keys` (whose file links changed in the command)
    /// link to the files that the command linked the library entries to, wherever they are, and to
    /// those that the library entries link to in the pool's files directory; and no longer to the
    /// files that the library entries no longer link to. Only the file links of the pool entry
    /// change (and in its origin record), and no file is deleted. A link that the library entry
    /// had when loaded, outside the pool's files directory, is not sent (`push` sends it, as the
    /// other fields). An entry that the pool does not have, with links to send, is pushed (added
    /// to the pool, as `add` does).
    pub fn push_links(
        &mut self,
        lib: &mut Library,
        keys: &[String],
        pool: PoolIx,
        resolver: &mut dyn Resolve,
    ) -> Result<Vec<Outcome>> {
        if !keys.is_empty() {
            self.pools.at_mut(pool).check_readable()?;
        }
        let mut report = Vec::new();
        let mut missing: Vec<String> = Vec::new();
        let filesdir = self.pools.at(pool).filesdir();
        let pool_relative = self.pool_relative_to(pool)?;
        for key in keys {
            let Some(e) = lib.get(key).cloned() else {
                continue;
            };
            let before = lib.loaded_files(&e).unwrap_or_default();
            let after = entry::files(&e, Some(&lib.relative_to));
            let after_real = real_set(&after);
            let removed: HashSet<String> =
                real_set(&before).difference(&after_real).cloned().collect();
            let new = real_set(&self.new_links(lib, &e, pool));
            let added: Vec<String> = after
                .iter()
                .filter(|f| {
                    new.contains(&realpath(f))
                        || filesdir.as_deref().is_some_and(|d| is_inside(f, d))
                })
                .cloned()
                .collect();
            let (pe, record) = self.pool_entry(lib, pool, &e)?;
            let Some(pe) = pe else {
                if !added.is_empty() {
                    missing.push(e.key.clone());
                }
                continue;
            };
            let pool_entry = self.pool_entry_copy(pool, pe)?;
            let old = entry::files(&pool_entry, Some(&pool_relative));
            let mut files: Vec<String> = old
                .iter()
                .filter(|f| !removed.contains(&realpath(f)))
                .cloned()
                .collect();
            let linked = real_set(&files);
            files.extend(
                added
                    .iter()
                    .filter(|f| !linked.contains(&realpath(f)))
                    .cloned(),
            );
            if files == old {
                continue;
            }
            let id = self.pools.at(pool).id.clone();
            if !self.after.contains_key(&id) {
                let before_mtime = self.pools.at(pool).bibtex_mtime();
                if before_mtime.is_some() && self.origin.pulled(&id) == before_mtime {
                    // the pool had not changed since the last exchange: it has not now either
                    self.after.insert(id.clone(), [true, false]);
                }
            }
            self.pools
                .at_mut(pool)
                .library()?
                .set_files_by_uid(pe, &files);
            self.pools.at_mut(pool).forget_sizes();
            self.changed(pool);
            if record.is_some() && self.shares_links(pool) {
                self.origin.set_files(
                    &e.key,
                    &id,
                    Some(&format_files(&files, Some(&lib.relative_to))),
                );
            }
            report.push(Outcome::new(&e.key, Some(pool), What::Links));
        }
        if !missing.is_empty() {
            report.extend(self.push(lib, &missing, pool, resolver, true)?);
        }
        Ok(report)
    }

    // links to pool files
    // -------------------

    /// The pool files for the broken links `broken` of a library entry: `{link: [pool files]}`,
    /// taken from the pool entry of the same paper, among its files that exist and that the entry
    /// does not link to already:
    ///
    /// - the pool file of the same file name as the link;
    /// - otherwise, the pool file named `<name>--<link name>` (the pool renamed the file as an
    ///   attachment, labelled with its former name), if only one is;
    /// - then, if one broken link and one pool file are left, that file;
    /// - otherwise, the first link into the files directory of the pool (the pool renamed its
    ///   files) gets the pool entry's files left, and the other links into it are dropped (`[]`).
    ///
    /// A link without a pool file is not in the result. In the order of `broken`.
    pub fn pool_relinks(
        &mut self,
        lib: &Library,
        e: &Entry,
        pool: PoolIx,
        broken: &[String],
    ) -> Result<Relinks> {
        if broken.is_empty() {
            return Ok(Vec::new());
        }
        let (pe, _) = self.pool_entry(lib, pool, e)?;
        let Some(pe) = pe else { return Ok(Vec::new()) };
        let linked: HashSet<String> = entry::files(e, Some(&lib.relative_to))
            .iter()
            .filter(|f| exists(f))
            .map(|f| realpath(f))
            .collect();
        let pool_relative = self.pool_relative_to(pool)?;
        let pool_entry = self.pool_entry_copy(pool, pe)?;
        let mut left: Vec<String> = entry::files(&pool_entry, Some(&pool_relative))
            .into_iter()
            .filter(|f| exists(f) && !linked.contains(&realpath(f)))
            .collect();
        let mut found: Vec<(String, Vec<String>)> = Vec::new();
        for link in broken {
            if let Some(i) = left.iter().position(|f| basename(f) == basename(link)) {
                found.push((link.clone(), vec![left.remove(i)]));
            }
        }
        let is_found = |found: &Vec<(String, Vec<String>)>, link: &String| {
            found.iter().any(|(l, _)| l == link)
        };
        for link in broken
            .iter()
            .filter(|l| !is_found(&found, l))
            .cloned()
            .collect::<Vec<_>>()
        {
            let label = format!("{ATTACHMENT_SEP}{}", basename(&link));
            let labelled: Vec<usize> = left
                .iter()
                .enumerate()
                .filter(|(_, f)| basename(f).ends_with(&label))
                .map(|(i, _)| i)
                .collect();
            if labelled.len() == 1 {
                found.push((link.clone(), vec![left.remove(labelled[0])]));
            }
        }
        let rest: Vec<String> = broken
            .iter()
            .filter(|l| !is_found(&found, l))
            .cloned()
            .collect();
        if rest.is_empty() || left.is_empty() {
            return Ok(found);
        }
        if rest.len() == 1 && left.len() == 1 {
            found.push((rest[0].clone(), left));
        } else if let Some(dir) = self.pools.at(pool).filesdir() {
            let inside: Vec<&String> = rest.iter().filter(|l| is_inside(l, &dir)).collect();
            if !inside.is_empty() {
                found.push((inside[0].clone(), left));
                found.extend(inside[1..].iter().map(|l| ((*l).clone(), Vec::new())));
            }
        }
        Ok(found)
    }

    /// Replace the broken links to the files of a pool (default: the pool of the library; the pool
    /// renamed or moved them) by the files of the pool entry (see [`Exchange::pool_relinks`]),
    /// linked where they are. The entries repaired (`keys`, else all), with what replaced what.
    pub fn repair_links(
        &mut self,
        lib: &mut Library,
        keys: Option<&[String]>,
        pool: PoolIx,
    ) -> Result<Repaired> {
        let Some(dir) = self.pools.at(pool).filesdir() else {
            return Ok(Vec::new());
        };
        let keys: Vec<String> = match keys {
            Some(keys) => keys.to_vec(),
            None => lib.entries().map(|e| e.key.clone()).collect(),
        };
        let mut repaired = Vec::new();
        for key in keys {
            let Some(e) = lib.get(&key).cloned() else {
                continue;
            };
            let files = entry::files(&e, Some(&lib.relative_to));
            let broken: Vec<String> = files
                .iter()
                .filter(|f| !exists(f) && is_inside(f, &dir))
                .cloned()
                .collect();
            let found = self.pool_relinks(lib, &e, pool, &broken)?;
            if found.is_empty() {
                continue;
            }
            let new: Vec<String> = files
                .iter()
                .flat_map(|f| match found.iter().find(|(l, _)| l == f) {
                    Some((_, fs)) => fs.clone(),
                    None => vec![f.clone()],
                })
                .collect();
            lib.set_files(&key, &new)?;
            repaired.push((key, found));
        }
        Ok(repaired)
    }

    /// A new round of exchanges with the same pools and records (`sync`: the push after the pull):
    /// what the first round saved is not saved again.
    pub fn next_round(&mut self) {
        self.changed_pools.clear();
        self.pushed_pools.clear();
        self.after.clear();
    }

    /// The dry-run flag of the command.
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }

    /// An error for a pool that cannot be used, from a message.
    pub fn pool_error(message: impl Into<String>) -> PoolError {
        PoolError(message.into())
    }

    /// The policy of the pool of the library, for a caller that places files itself.
    pub fn policy_of(&self, pool: PoolIx, keep: &[String]) -> Policy {
        self.pool_placement(pool, keep).policy
    }
}
