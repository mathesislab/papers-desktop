//! `add` looks in the pool first (`papers.pools.PoolLookup`, spec `exchange.md` section 13): an added PDF,
//! DOI, key or bibtex entry that is the same paper as a pool entry is taken from the pool, without fetching
//! anything. The entries taken are recorded in the origin records once inserted.

use std::collections::HashMap;

use crate::bib::Entry;
use crate::config::paths::{abspath, is_inside, isfile, realpath};
use crate::entry::{self, set_files, val};
use crate::error::Result;
use crate::extract::PdfFile;
use crate::extract::doi::{parse_dois, title_in_text};
use crate::library::{Action, Inserted, Library, Policy, Taker, What as InsertWhat};
use crate::log;
use crate::merge::{Merge, checksum_hex};

use super::exchange::{Exchange, exchange_copy};
use super::pool::PoolIx;

/// An entry taken from the pool, until it is inserted.
#[derive(Debug, Clone)]
struct Taken {
    pe: u64,
    /// The files given that have the content of a file of the pool entry.
    same: Vec<String>,
}

/// The pool lookup of an `add`: the library to look in (`add --pool PATH`, by default the pool of the library).
#[derive(Debug, Clone)]
pub struct PoolLookup {
    pub pool: PoolIx,
    pending: Option<Taken>,
    /// lower-case library key -> the pool of the entry taken
    pub taken_from: HashMap<String, PoolIx>,
    /// The files given that are removed (`move` mode): the same content is stored already: `(file, the copy)`.
    pub to_remove: Vec<(String, String)>,
}

fn same_content_hashes(files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|f| isfile(f))
        .filter_map(|f| checksum_hex(f).ok())
        .collect()
}

impl PoolLookup {
    pub fn new(pool: PoolIx) -> PoolLookup {
        PoolLookup {
            pool,
            pending: None,
            taken_from: HashMap::new(),
            to_remove: Vec::new(),
        }
    }

    /// The entry to insert for pool entry `pe`: the pool's fields and files, the fields only in `entry` (an
    /// added bibtex entry), and the `files` the pool does not have (same content).
    fn take(
        &mut self,
        exchange: &mut Exchange,
        lib: &Library,
        pe: u64,
        key: &str,
        entry: Option<&Entry>,
        files: &[String],
    ) -> Result<Entry> {
        let pool_entry = exchange.pool_entry_of(self.pool, pe)?;
        let pool_relative = exchange.pool_relative(self.pool)?;
        let mut new = exchange_copy(&pool_entry, &pool_relative, &lib.relative_to, Some(key));
        if let Some(entry) = entry {
            let mut merge =
                Merge::new(&[new.clone(), entry.clone()], Some(&lib.relative_to), false);
            merge.choose_all(0);
            new = merge.result();
        }
        let have = entry::files(&new, Some(&lib.relative_to));
        let hashes = same_content_hashes(&have);
        let is_extra =
            |f: &String| !(isfile(f) && checksum_hex(f).is_ok_and(|h| hashes.contains(&h)));
        let extra: Vec<String> = files.iter().filter(|f| is_extra(f)).cloned().collect();
        let same: Vec<String> = files.iter().filter(|f| !is_extra(f)).cloned().collect();
        if !extra.is_empty() {
            let all: Vec<String> = have.into_iter().chain(extra).collect();
            set_files(&mut new, &all, &lib.relative_to);
        }
        self.pending = Some(Taken { pe, same });
        log::info(&format!(
            "found in {}: {}",
            exchange.label(self.pool),
            pool_entry.key
        ));
        Ok(new)
    }

    /// Whether a pool entry has a file with the content of `file`.
    pub fn has_file(&self, exchange: &mut Exchange, file: &str) -> Result<bool> {
        Ok(exchange.pools.at_mut(self.pool).find_file(file)?.is_some())
    }

    /// The entry for an added PDF found in the pool (same file content, or a DOI of the PDF that is in the pool,
    /// its title in the text), or none.
    pub fn pdf(
        &mut self,
        exchange: &mut Exchange,
        lib: &Library,
        pdf: &str,
        files: &[String],
        doi: Option<&str>,
    ) -> Result<Option<Entry>> {
        if let Some((pe, _)) = exchange.pools.at_mut(self.pool).find_file(pdf)? {
            let pool_entry = exchange.pool_entry_of(self.pool, pe)?;
            let key = lib.keyformat.render(&pool_entry)?;
            return self.take(exchange, lib, pe, &key, None, files).map(Some);
        }
        if let Some(doi) = doi {
            return self.doi(exchange, lib, doi, files);
        }
        let file = PdfFile::new(pdf);
        match file.metadata_doi() {
            Ok(Some(doi)) => return self.doi(exchange, lib, &doi, files),
            Ok(None) => {}
            Err(error) => {
                log::debug(&format!("{pdf}: {}", error.message));
                return Ok(None);
            }
        }
        let read = (|| -> std::result::Result<(String, Vec<String>), String> {
            let txt = file.head(10, 200, false).map_err(|e| e.message)?;
            let tail = file.tail().map_err(|e| e.message)?;
            let candidates = parse_dois(&txt, &tail).map_err(|e| e.0)?;
            Ok((txt, candidates))
        })();
        let (txt, candidates) = match read {
            Ok(found) => found,
            Err(error) => {
                log::debug(&format!("{pdf}: {error}"));
                return Ok(None);
            }
        };
        for candidate in candidates {
            let Some(pe) = exchange.pools.at_mut(self.pool).find_doi(&candidate)? else {
                continue;
            };
            let pool_entry = exchange.pool_entry_of(self.pool, pe)?;
            if title_in_text(val(&pool_entry, "title"), &txt) {
                let key = lib.keyformat.render(&pool_entry)?;
                return self.take(exchange, lib, pe, &key, None, files).map(Some);
            }
        }
        Ok(None)
    }

    /// The entry for an added DOI found in the pool, or none.
    pub fn doi(
        &mut self,
        exchange: &mut Exchange,
        lib: &Library,
        doi: &str,
        files: &[String],
    ) -> Result<Option<Entry>> {
        let Some(pe) = exchange.pools.at_mut(self.pool).find_doi(doi)? else {
            return Ok(None);
        };
        let pool_entry = exchange.pool_entry_of(self.pool, pe)?;
        let key = lib.keyformat.render(&pool_entry)?;
        self.take(exchange, lib, pe, &key, None, files).map(Some)
    }

    /// The entry for `add --key KEY` of the pool entry of key KEY (without case), or none.
    pub fn key(
        &mut self,
        exchange: &mut Exchange,
        lib: &Library,
        key: &str,
        files: &[String],
    ) -> Result<Option<Entry>> {
        let Some(pe) = exchange.pools.at_mut(self.pool).entry_of_key(key)? else {
            return Ok(None);
        };
        let pool_entry = exchange.pool_entry_of(self.pool, pe)?;
        self.take(exchange, lib, pe, &pool_entry.key, None, files)
            .map(Some)
    }

    /// The entry for an added bibtex entry found in the pool (it keeps its key), or none.
    pub fn entry(
        &mut self,
        exchange: &mut Exchange,
        lib: &Library,
        e: &Entry,
    ) -> Result<Option<Entry>> {
        let Some(pe) = exchange.pools.at_mut(self.pool).find(e, &lib.distinct)? else {
            return Ok(None);
        };
        let files = entry::files(e, Some(&lib.relative_to));
        let mut stripped = e.clone();
        stripped.remove("file");
        self.take(exchange, lib, pe, &e.key, Some(&stripped), &files)
            .map(Some)
    }

    /// Record the origin of the library entries that an entry taken from the pool ended in. In `move`
    /// mode (`rename` places the files by a policy that moves), a file given that has the same content as a
    /// file that the entry links to now is to be removed: it is stored already (`to_remove`).
    pub fn inserted(
        &mut self,
        exchange: &mut Exchange,
        lib: &Library,
        inserted: &Inserted,
        rename: Option<&Policy>,
    ) -> Result<()> {
        let Some(taken) = self.pending.take() else {
            return Ok(());
        };
        if inserted.what == InsertWhat::Skipped {
            return Ok(());
        }
        let key = inserted.key.clone();
        self.taken_from.insert(key.to_lowercase(), self.pool);
        let id = exchange.pools.at(self.pool).id.clone();
        if let Some(e) = lib.get(&key)
            && exchange.origin.get(e, Some(&id)).is_none()
        {
            let pool_entry = exchange.pool_entry_of(self.pool, taken.pe)?;
            exchange.record(lib, &key, self.pool, &pool_entry, taken.pe)?;
        }
        if let Some(policy) = rename {
            let moved: Vec<String> = taken
                .same
                .iter()
                .filter(|f| policy.action(f) == Action::Move)
                .cloned()
                .collect();
            self.remove_moved(exchange, lib, &key, &moved);
        }
        Ok(())
    }

    fn remove_moved(&mut self, exchange: &Exchange, lib: &Library, key: &str, files: &[String]) {
        let linked: std::collections::HashSet<String> = lib
            .entries()
            .flat_map(|e| entry::files(e, Some(&lib.relative_to)))
            .map(|f| realpath(&f))
            .collect();
        let mut stored: HashMap<String, String> = HashMap::new();
        for f in lib.files(key) {
            if isfile(&f)
                && let Ok(h) = checksum_hex(&f)
            {
                stored.entry(h).or_insert(f);
            }
        }
        let pool_dir = exchange.pools.at(self.pool).filesdir();
        for f in files {
            if !isfile(f)
                || linked.contains(&realpath(f))
                || pool_dir.as_deref().is_some_and(|d| is_inside(f, d))
            {
                continue;
            }
            let Some(target) = checksum_hex(f).ok().and_then(|h| stored.get(&h).cloned()) else {
                continue;
            };
            if same_file(&target, f) {
                continue;
            }
            self.to_remove.push((abspath(f), target));
        }
    }
}

fn same_file(a: &str, b: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

/// The lookup and the exchange of a session, as a [`Taker`] for the entries of an added file.
pub struct PoolTaker<'a> {
    pub lookup: &'a mut PoolLookup,
    pub exchange: &'a mut Exchange,
}

impl Taker for PoolTaker<'_> {
    fn take(&mut self, library: &Library, entry: &Entry) -> Result<Option<Entry>> {
        self.lookup.entry(self.exchange, library, entry)
    }

    fn inserted(
        &mut self,
        library: &mut Library,
        inserted: &Inserted,
        rename: Option<&Policy>,
    ) -> Result<()> {
        self.lookup
            .inserted(self.exchange, library, inserted, rename)
    }
}
