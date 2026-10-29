//! The pools of a library (`papers.pools.Pool`, `Pools`): the library named by its `pool`
//! setting, and the libraries given on the command line (`--pool PATH`), resolved when first
//! used and kept for the run. Spec `exchange.md` section 2.

use std::collections::HashMap;
use std::sync::Arc;

use crate::bib::Entry;
use crate::config::paths::{
    abspath, exists, expanduser, getcwd, isdir, isfile, join, realpath, relpath, short_home,
};
use crate::config::{Config, library_config_file, not_a_library};
use crate::error::{Error, Result};
use crate::identity::{Distinct, Kind, Verdict, contradicts};
use crate::library::{Library, unreadable_message};
use crate::lock::{self, Lock};
use crate::log;
use crate::merge::checksum_hex;

/// The error of the exchanges without a pool.
pub const NO_POOL_MESSAGE: &str = "this library has no pool: give one with `--pool PATH`";

/// A pool that cannot be used (missing, not a library, has a pool): papers' `PoolError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolError(pub String);

impl std::fmt::Display for PoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PoolError {}

impl From<PoolError> for Error {
    fn from(error: PoolError) -> Error {
        Error::exit(error.0)
    }
}

/// How a pool is named in a [`Pools`].
pub type PoolIx = usize;

/// A library used as a pool. `id` is how the library names it in its origin records (see
/// [`Pools::id_of`]); `shown` is how the messages name it.
pub struct Pool {
    pub id: String,
    pub path: String,
    pub config: Config,
    pub shown: String,
    library: Option<Library>,
    /// The lock of the pool, held until the end of the command that writes the library.
    lock: Option<Lock>,
    keys: Option<HashMap<String, u64>>,
    sizes: Option<HashMap<u64, Vec<(u64, String)>>>,
    union: Option<((usize, usize), Arc<Distinct>)>,
}

impl Pool {
    fn new(id: String, path: String, config: Config, shown: String, lock: Option<Lock>) -> Pool {
        Pool {
            id,
            path,
            config,
            shown,
            library: None,
            lock,
            keys: None,
            sizes: None,
            union: None,
        }
    }

    /// `pool ../pool`
    pub fn label(&self) -> String {
        format!("pool {}", self.shown)
    }

    pub fn filesdir(&self) -> Option<String> {
        self.config.filesdir.clone().filter(|d| !d.is_empty())
    }

    /// The lock of the pool, to hand over (undo and redo keep it).
    pub fn take_lock(&mut self) -> Option<Lock> {
        self.lock.take()
    }

    /// The library of the pool, loaded when first used (`load_biblio`: a bibtex that does not
    /// exist is not created).
    pub fn library(&mut self) -> Result<&mut Library> {
        if self.library.is_none() {
            self.library = Some(Library::open_existing(&self.config)?);
        }
        Ok(self.library.as_mut().expect("loaded"))
    }

    /// Take the library out (to save it through a session); [`Pool::put_library`] gives it back.
    pub fn take_library(&mut self) -> Result<Library> {
        self.library()?;
        Ok(self.library.take().expect("loaded"))
    }

    pub fn put_library(&mut self, library: Library) {
        self.library = Some(library);
    }

    pub fn put_lock(&mut self, lock: Option<Lock>) {
        self.lock = lock;
    }

    /// The library, if it was loaded.
    pub fn loaded(&self) -> Option<&Library> {
        self.library.as_ref()
    }

    pub fn loaded_mut(&mut self) -> Option<&mut Library> {
        self.library.as_mut()
    }

    /// The modification time of the pool's bibtex in nanoseconds, if it exists.
    pub fn bibtex_mtime(&self) -> Option<i128> {
        mtime(self.config.bibtex.as_deref()?)
    }

    /// The pairs that are not the same paper of the pool and of `distinct`.
    pub fn distinct_with(&mut self, distinct: &Distinct) -> Result<Arc<Distinct>> {
        let own = self.library()?.distinct.pairs.len();
        let sizes = (own, distinct.pairs.len());
        if let Some((s, union)) = &self.union
            && *s == sizes
        {
            return Ok(Arc::clone(union));
        }
        let mut union = Distinct::new();
        union.update(&self.library()?.distinct);
        union.update(distinct);
        let union = Arc::new(union);
        self.union = Some((sizes, Arc::clone(&union)));
        Ok(union)
    }

    /// The pool entry that is the same paper as `entry` (`same` verdicts only), with the pairs of
    /// `distinct` too.
    pub fn find(&mut self, entry: &Entry, distinct: &Distinct) -> Result<Option<u64>> {
        let union = self.distinct_with(distinct)?;
        let found = self.library()?.find_same(entry, Some(&union));
        Ok(found
            .into_iter()
            .find(|(_, v): &(u64, Verdict)| v.kind == Kind::Same)
            .map(|(uid, _)| uid))
    }

    fn key_map(&mut self) -> Result<&HashMap<String, u64>> {
        if self.keys.is_none() {
            let mut keys = HashMap::new();
            let library = self.library()?;
            for uid in library.uids() {
                if let Some(e) = library.entry_by_uid(uid) {
                    keys.entry(e.key.to_lowercase()).or_insert(uid);
                }
            }
            self.keys = Some(keys);
        }
        Ok(self.keys.as_ref().expect("keys"))
    }

    /// The pool entry of key `key` (without case), if any.
    pub fn entry_of_key(&mut self, key: &str) -> Result<Option<u64>> {
        if key.is_empty() {
            return Ok(None);
        }
        Ok(self.key_map()?.get(&key.to_lowercase()).copied())
    }

    /// The pool entry of key `key` (the pool key of an origin record) if its identifiers do not
    /// contradict the origin copy: found this way when the title (the only identifier of entries
    /// without DOI) changed on both sides.
    pub fn by_key(&mut self, key: &str, origin: &Entry) -> Result<Option<u64>> {
        let Some(uid) = self.entry_of_key(key)? else {
            return Ok(None);
        };
        let library = self.library()?;
        let pe = library.entry_by_uid(uid).expect("the entry");
        Ok((!contradicts(pe, origin)).then_some(uid))
    }

    /// The pool entry of this DOI.
    pub fn find_doi(&mut self, doi: &str) -> Result<Option<u64>> {
        let mut probe = Entry::new("misc", "doi");
        probe.set("doi", doi);
        self.find(&probe, &Distinct::new())
    }

    /// The pool entry and the pool file of a file with the content of `file` (compared by size,
    /// then checksum).
    pub fn find_file(&mut self, file: &str) -> Result<Option<(u64, String)>> {
        if self.sizes.is_none() {
            let mut sizes: HashMap<u64, Vec<(u64, String)>> = HashMap::new();
            let library = self.library()?;
            for uid in library.uids() {
                let Some(e) = library.entry_by_uid(uid) else {
                    continue;
                };
                for f in crate::entry::files(e, Some(&library.relative_to)) {
                    if isfile(&f)
                        && let Ok(meta) = std::fs::metadata(&f)
                    {
                        sizes.entry(meta.len()).or_default().push((uid, f));
                    }
                }
            }
            self.sizes = Some(sizes);
        }
        let Ok(size) = std::fs::metadata(file).map(|m| m.len()) else {
            return Ok(None);
        };
        let mut hash: Option<Option<String>> = None;
        for (uid, f) in self
            .sizes
            .as_ref()
            .and_then(|s| s.get(&size))
            .cloned()
            .unwrap_or_default()
        {
            if realpath(&f) == realpath(file) {
                return Ok(Some((uid, f)));
            }
            let hash = hash.get_or_insert_with(|| checksum_hex(file).ok());
            if hash.is_some() && checksum_hex(&f).ok() == *hash {
                return Ok(Some((uid, f)));
            }
        }
        Ok(None)
    }

    /// The links of the entries changed: the lookups by file are made again.
    pub fn forget_sizes(&mut self) {
        self.sizes = None;
    }

    /// Keep the lookups up to date after an entry was added to the pool.
    pub fn added(&mut self, uid: u64) {
        if let Some(keys) = &mut self.keys
            && let Some(e) = self.library.as_ref().and_then(|l| l.entry_by_uid(uid))
        {
            keys.entry(e.key.to_lowercase()).or_insert(uid);
        }
        self.sizes = None;
    }

    /// Replace the content of a pool entry (keeping the lookups up to date).
    pub fn update_entry(&mut self, uid: u64, new: &Entry) {
        let Some(library) = self.library.as_mut() else {
            return;
        };
        let key = library.entry_by_uid(uid).map(|e| e.key.clone());
        library.apply_by_uid(uid, new);
        if key.as_deref() != Some(new.key.as_str()) {
            self.keys = None;
        }
    }

    /// A pool with entries that could not be read is not written to: papers does not see them,
    /// and would add the same papers again.
    pub fn check_readable(&mut self) -> Result<()> {
        let bibtex = self.config.bibtex.clone().unwrap_or_default();
        let unreadable = self.library()?.unreadable().to_vec();
        if unreadable.is_empty() {
            return Ok(());
        }
        Err(Error::exit(format!(
            "{}. Fix {} before writing to the pool (nothing was changed)",
            unreadable_message(&bibtex, &unreadable),
            short_home(&bibtex)
        )))
    }
}

/// A modification time in nanoseconds.
pub fn mtime(file: &str) -> Option<i128> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(file).ok()?;
    Some(meta.mtime() as i128 * 1_000_000_000 + meta.mtime_nsec() as i128)
}

/// The pool of a library, or a library given with `--pool PATH`, has no pool itself: an exchange
/// goes one level.
pub fn check_poolless(pool: &Pool) -> std::result::Result<(), PoolError> {
    if pool.config.has_pool() {
        return Err(PoolError(format!(
            "{} has a pool itself ({}): give a library without pool",
            pool.shown,
            pool.config.pool_label().unwrap_or_default()
        )));
    }
    Ok(())
}

/// The pool of a library (its `pool` setting) and the pools named on the command line, resolved
/// when first used and kept for the run.
pub struct Pools {
    pub config: Config,
    list: Vec<Pool>,
    /// The pool ids of the origin records of the library (see [`Pools::set_known_ids`]).
    known_ids: Vec<String>,
    /// The name of the command, for the locks of the pools.
    command: String,
    /// The command holds the lock of its library: it takes the lock of each pool before reading it.
    locking: bool,
}

impl Pools {
    pub fn new(config: &Config) -> Pools {
        Pools {
            config: config.clone(),
            list: Vec::new(),
            known_ids: Vec::new(),
            command: "papers".to_string(),
            locking: false,
        }
    }

    /// The pools are locked before they are read, by a command of this name (a command that
    /// writes its library).
    pub fn lock_as(&mut self, command: &str) {
        self.command = command.to_string();
        self.locking = true;
    }

    /// The directory of the library (a relative pool setting is relative to it).
    pub fn libdir(&self) -> String {
        if self.config.file.is_some() {
            self.config.configdir()
        } else {
            getcwd()
        }
    }

    fn is_self(&self, path: &str) -> bool {
        self.config.file.is_some() && realpath(path) == realpath(&self.libdir())
    }

    /// The pool ids of the origin records of the library: a pool keeps the id that its records
    /// use, however it is given.
    pub fn set_known_ids(&mut self, ids: impl IntoIterator<Item = String>) {
        let mut ids: Vec<String> = ids.into_iter().collect();
        ids.sort();
        ids.dedup();
        self.known_ids = ids;
        for i in 0..self.list.len() {
            let id = self.id_of(&self.list[i].path);
            self.list[i].id = id;
        }
    }

    /// How the origin records name the pool in `path`: the id of its records if it has some, else
    /// for the pool of the library its pool setting, else its path relative to the library.
    pub fn id_of(&self, path: &str) -> String {
        let libdir = self.libdir();
        for id in &self.known_ids {
            if realpath(&join(&libdir, &expanduser(id))) == realpath(path) {
                return id.clone();
            }
        }
        if let Some(own) = self.path()
            && realpath(&own) == realpath(path)
        {
            return self.config.pool_label().unwrap_or_default();
        }
        let rel = relpath(path, &libdir);
        if rel.starts_with('.') {
            rel
        } else {
            join(".", &rel)
        }
    }

    /// How the messages name the pool in `path` when the command line does not give it: the pool of
    /// the library as its pool setting writes it, else with `~` for the home directory.
    fn shown_of(&self, path: &str) -> String {
        if let Some(own) = self.path()
            && realpath(&own) == realpath(path)
        {
            return self.config.pool_label().unwrap_or_default();
        }
        short_home(path)
    }

    /// The pool of the library in directory `path`, which must be a library. The messages name it
    /// `shown` (the path as given on the command line), by default as [`Pools::shown_of`] says.
    pub fn resolve(
        &mut self,
        path: &str,
        shown: Option<&str>,
    ) -> std::result::Result<PoolIx, PoolError> {
        let path = abspath(path);
        let key = realpath(&path);
        if let Some(i) = self.list.iter().position(|p| realpath(&p.path) == key) {
            if let Some(shown) = shown {
                self.list[i].shown = shown.to_string();
            }
            return Ok(i);
        }
        let shown = shown.map_or_else(|| self.shown_of(&path), str::to_string);
        if self.is_self(&path) {
            return Err(PoolError(format!(
                "pool {shown}: this is the library itself"
            )));
        }
        let configfile =
            library_config_file(&path).map_err(|e| PoolError(e.message().to_string()))?;
        let Some(configfile) = configfile else {
            if exists(&path) && !isdir(&path) {
                return Err(PoolError(format!("pool {shown}: not a directory")));
            }
            return Err(PoolError(not_a_library("pool", &path)));
        };
        let config = Config::load(&configfile, self.config.machine.clone(), None)
            .map_err(|e| PoolError(e.message().to_string()))?;
        let lock = if self.locking {
            lock::acquire(&config, &self.command, None)
                .map_err(|e| PoolError(e.message().to_string()))?
        } else {
            None
        };
        let id = self.id_of(&path);
        self.list.push(Pool::new(id, path, config, shown, lock));
        Ok(self.list.len() - 1)
    }

    /// The path of the pool of the library, or none (pool none, or the library is its own pool).
    pub fn path(&self) -> Option<String> {
        let path = self.config.pool_path()?;
        (!self.is_self(&path)).then_some(path)
    }

    /// The pool of the library, or none (pool none, or the library is its own pool). A pool that
    /// cannot be read is a warning (none with `quiet`), and none.
    pub fn pool(&mut self, quiet: bool) -> Option<PoolIx> {
        let path = self.path()?;
        match self.resolve(&path, None) {
            Ok(i) => Some(i),
            Err(error) => {
                if !quiet {
                    log::warning(&error.0);
                }
                None
            }
        }
    }

    /// The pool that `pull`, `push` and the shallow libraries exchange with: an error if it is not a
    /// library, or if it has a pool itself (as `--pool PATH`).
    pub fn target(&mut self) -> std::result::Result<PoolIx, PoolError> {
        let Some(path) = self.path() else {
            return Err(PoolError(NO_POOL_MESSAGE.to_string()));
        };
        let ix = self.resolve(&path, None)?;
        check_poolless(&self.list[ix])?; // it may have taken a pool since the pool setting was made
        Ok(ix)
    }

    /// A library given on the command line (`--pool PATH`): a path relative to the current
    /// directory, of a library without pool.
    pub fn get(&mut self, path: &str) -> std::result::Result<PoolIx, PoolError> {
        let ix = self.get_library(path)?;
        check_poolless(&self.list[ix])?;
        Ok(ix)
    }

    /// A library given on the command line, which may have a pool itself (`papers file fix --pool
    /// PATH` only takes its files).
    pub fn get_library(&mut self, path: &str) -> std::result::Result<PoolIx, PoolError> {
        let expanded = expanduser(path);
        // named as given; an absolute path with ~ (the shell expands ~/...)
        let shown = if !expanded.starts_with('/') {
            path.to_string()
        } else {
            short_home(&expanded)
        };
        self.resolve(&abspath(&expanded), Some(&shown))
    }

    pub fn at(&self, ix: PoolIx) -> &Pool {
        &self.list[ix]
    }

    pub fn at_mut(&mut self, ix: PoolIx) -> &mut Pool {
        &mut self.list[ix]
    }

    /// The pools resolved so far.
    pub fn loaded(&self) -> std::ops::Range<PoolIx> {
        0..self.list.len()
    }

    /// The files directory of the pool of the library, if it has one, as a list.
    pub fn files_dirs(&mut self) -> Vec<String> {
        match self.pool(true) {
            Some(ix) => self.list[ix].filesdir().into_iter().collect(),
            None => Vec::new(),
        }
    }
}

/// The files directory of the pool of a shallow library, or none (no pool, not readable).
pub fn pool_files_dir(config: &Config) -> Option<String> {
    let path = config.pool_path()?;
    if realpath(&path) == realpath(&config.configdir()) {
        return None;
    }
    let file = library_config_file(&path).ok()??;
    let pool = Config::load(&file, config.machine.clone(), None).ok()?;
    pool.filesdir.clone().filter(|d| !d.is_empty())
}
