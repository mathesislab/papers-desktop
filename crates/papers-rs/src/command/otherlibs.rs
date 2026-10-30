//! Where `papers file fix` (also `papers file check`, and `papers file fetch` before it downloads)
//! looks for the file of a broken link beyond the files of the pool entry of the same paper, by the
//! checksum that the library's `.papers/files.json` has for the link (`papers.otherlibs`, spec
//! library.md "The file of a broken link"): in the library itself, in its pool, and in the other
//! libraries of the machine.
//!
//! The other libraries are the places of the history stores of the machine (the bibtex of each
//! library, or bibtex without library, that recorded a step here), except the current library and
//! the libraries searched already (its pool, `--pool PATH`); a place whose bibtex does not exist is
//! skipped. Their checksums are those of the `.papers/files.json` kept in the head state of each
//! place (not the file on disk: a change made outside papers in another library counts after the
//! next papers command there). Nothing is read in them but the files found (their checksum), and
//! nothing is written: they are not locked, migrated nor recorded.
//!
//! papers keeps an index of these checksums in its cache (`<cache dir>/file-index/`), private to
//! it (history.md 11: another implementation must not write into it). papers-rs reads the head
//! states when a broken link with a checksum is first searched in a command: one state file and
//! its `files.json` per place, no bibtex parsed.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::config::paths::{abspath, dirname, is_inside, isfile, join, normpath, realpath};
use crate::config::Config;
use crate::history::ojson::J;
use crate::history::project::is_state_id;
use crate::history::{FilesJson, Setup, State, Store, files_json};
use crate::library::Library;

/// The name of `.papers/files.json` among the kept files of a state (history.md 3.3).
const FILES_JSON: &str = ".papers/files.json";

/// The sha256 of the files read in the command, each file read once (`Digests`).
#[derive(Default)]
struct Digests(HashMap<String, Option<String>>);

impl Digests {
    fn of(&mut self, path: &str) -> Option<String> {
        self.0
            .entry(path.to_string())
            .or_insert_with(|| crate::merge::checksum_hex(path).ok())
            .clone()
    }
}

/// The files of a `.papers/files.json` by checksum (`Checksums`): a file is found if it exists and
/// still has that content.
struct Checksums {
    paths: HashMap<String, Vec<String>>,
}

impl Checksums {
    fn new(hashes: &BTreeMap<String, String>) -> Checksums {
        let mut paths: HashMap<String, Vec<String>> = HashMap::new();
        for (path, digest) in hashes {
            paths.entry(digest.clone()).or_default().push(path.clone());
        }
        Checksums { paths }
    }

    /// A file with this checksum, not one of `exclude` (real paths).
    fn find(&self, digest: &str, exclude: &HashSet<String>, digests: &mut Digests) -> Option<String> {
        self.paths.get(digest)?.iter().find_map(|path| {
            (isfile(path)
                && !exclude.contains(&realpath(path))
                && digests.of(path).as_deref() == Some(digest))
            .then(|| path.clone())
        })
    }
}

/// Where a file found by checksum comes from (`Source` of papers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// the library itself: the key of the entry that links to the file (none: no entry does)
    Own(Option<String>),
    /// the pool of the library
    Pool,
    /// another library of the machine: the real path of its bibtex
    Other(String),
}

/// The places of the history stores of the machine and the checksums of the `.papers/files.json`
/// of their head states (`{absolute path: sha256}`), the places sorted.
fn other_places(config: &Config) -> Vec<(String, BTreeMap<String, String>)> {
    let setup = Setup::from_config(config);
    // a place of several projects: the first one (as papers reads them)
    let mut heads: BTreeMap<String, (Store, String, String)> = BTreeMap::new();
    for store in setup.all_stores() {
        for id in store.project_ids() {
            let dir = join(&store.projects(), &id);
            let Some(J::Obj(places)) = read_json(&join(&dir, "heads.json")) else {
                continue;
            };
            for (place, head) in places {
                // a head numbered by papers 3.0.0b5 is not read (the store is not upgraded here)
                let Some(state) = head.get("state").and_then(J::as_str) else {
                    continue;
                };
                heads
                    .entry(place)
                    .or_insert_with(|| (store.clone(), dir.clone(), state.to_string()));
            }
        }
    }
    heads
        .into_iter()
        .filter(|(place, _)| isfile(place))
        .map(|(place, (store, dir, state))| {
            let hashes = head_checksums(&store, &dir, &state, &place);
            (place, hashes)
        })
        .collect()
}

fn read_json(path: &str) -> Option<J> {
    let bytes = std::fs::read(path).ok()?;
    J::parse(std::str::from_utf8(&bytes).ok()?)
}

/// The checksums of the `files.json` that a state of a place keeps (none: the state keeps none, or
/// cannot be read).
fn head_checksums(store: &Store, dir: &str, n: &str, place: &str) -> BTreeMap<String, String> {
    if !is_state_id(n) {
        return BTreeMap::new();
    }
    let Some(state) = read_json(&join(&join(dir, "states"), &format!("{n}.json")))
        .filter(J::is_object)
        .map(State)
    else {
        return BTreeMap::new();
    };
    let Some(pieces) = state.file(FILES_JSON).filter(|p| !p.is_empty()) else {
        return BTreeMap::new();
    };
    match store.text(&pieces) {
        Ok(text) => files_json::parse(&text, &dirname(place)),
        Err(_) => BTreeMap::new(),
    }
}

/// The searches by checksum of a command (`Lookup`): the library, its pool, the other libraries of
/// the machine (read when first needed).
pub struct Lookup {
    digests: Digests,
    /// `{absolute path: sha256}` of the library's `files.json`
    hashes: BTreeMap<String, String>,
    checksums: Checksums,
    filesdir: Option<String>,
    /// the real path of each file linked: the key of the first entry that links to it
    owners: HashMap<String, String>,
    unlinked: Option<Vec<String>>,
    pool: Option<Checksums>,
    /// the real paths of the bibtex of the libraries searched already
    exclude: HashSet<String>,
    config: Config,
    others: Option<Vec<(String, BTreeMap<String, String>)>>,
}

impl Lookup {
    /// The searches of the library of `config` and `library`; `pool`: the configuration of its
    /// pool, if it has one; `searched`: the bibtex files of the libraries searched already (the
    /// pool, `--pool PATH`).
    pub fn new(
        config: &Config,
        library: &Library,
        pool: Option<&Config>,
        searched: &[String],
    ) -> Lookup {
        let libdir = dirname(&abspath(config.bibtex.as_deref().unwrap_or_default()));
        let hashes = FilesJson::of(config, &libdir)
            .map(|f| f.load())
            .unwrap_or_default();
        let mut owners: HashMap<String, String> = HashMap::new();
        for e in library.entries() {
            for f in library.files(&e.key) {
                owners.entry(realpath(&f)).or_insert_with(|| e.key.clone());
            }
        }
        let pool = pool.map(|pool| {
            let dir = dirname(&abspath(pool.bibtex.as_deref().unwrap_or_default()));
            Checksums::new(
                &FilesJson::of(pool, &dir)
                    .map(|f| f.load())
                    .unwrap_or_default(),
            )
        });
        let exclude = config
            .bibtex
            .iter()
            .chain(searched)
            .filter(|b| !b.is_empty())
            .map(|b| realpath(b))
            .collect();
        Lookup {
            digests: Digests::default(),
            checksums: Checksums::new(&hashes),
            hashes,
            filesdir: config.filesdir.clone(),
            owners,
            unlinked: None,
            pool,
            exclude,
            config: config.clone(),
            others: None,
        }
    }

    /// The checksum recorded for a link.
    fn digest(&self, link: &str) -> Option<String> {
        self.hashes.get(&normpath(link)).cloned()
    }

    /// The files of the files directory that no entry links to and `files.json` does not have
    /// (hidden names and `.bib` files skipped), in the order of a walk of the folder.
    fn unlinked(&mut self) -> &[String] {
        if self.unlinked.is_none() {
            let known: HashSet<String> = self.hashes.keys().map(|p| realpath(p)).collect();
            let mut found = Vec::new();
            if let Some(filesdir) = self.filesdir.as_deref().filter(|d| !d.is_empty()) {
                walk(filesdir, &mut found);
            }
            found.retain(|path| {
                let real = realpath(path);
                !self.owners.contains_key(&real) && !known.contains(&real)
            });
            self.unlinked = Some(found);
        }
        self.unlinked.as_deref().unwrap_or_default()
    }

    /// The file of the library with the content of the broken link `link` (`OwnLibrary.find`): a
    /// file of `files.json` with its checksum, else an unlinked file of the files directory; not one
    /// of `exclude` (real paths).
    pub fn own(&mut self, link: &str, exclude: &HashSet<String>) -> Option<(String, Found)> {
        let digest = self.digest(link)?;
        let mut found = self.checksums.find(&digest, exclude, &mut self.digests);
        if found.is_none() {
            let candidates: Vec<String> = self.unlinked().to_vec();
            found = candidates.into_iter().find(|f| {
                !exclude.contains(&realpath(f)) && self.digests.of(f).as_deref() == Some(&digest)
            });
        }
        let found = found?;
        let key = self.owners.get(&realpath(&found)).cloned();
        Some((found, Found::Own(key)))
    }

    /// The file of the pool with the checksum of the broken link (`in_pool`).
    pub fn in_pool(&mut self, link: &str, exclude: &HashSet<String>) -> Option<(String, Found)> {
        let digest = self.digest(link)?;
        let found = self.pool.as_ref()?.find(&digest, exclude, &mut self.digests)?;
        Some((found, Found::Pool))
    }

    /// The file of another library of the machine with the checksum of the broken link
    /// (`in_others`): the first by the real path of the place's bibtex, then by the file's path.
    pub fn in_others(&mut self, link: &str, exclude: &HashSet<String>) -> Option<(String, Found)> {
        let digest = self.digest(link)?;
        if self.others.is_none() {
            self.others = Some(other_places(&self.config));
        }
        let others = self.others.as_ref()?;
        for (place, hashes) in others {
            if self.exclude.contains(&realpath(place)) {
                continue;
            }
            for (path, d) in hashes {
                if *d != digest || exclude.contains(&realpath(path)) || !isfile(path) {
                    continue;
                }
                if self.digests.of(path).as_deref() == Some(digest.as_str()) {
                    return Some((path.clone(), Found::Other(place.clone())));
                }
            }
        }
        None
    }

}

/// A file of the library's files directory `filesdir` that no entry links to, found by checksum
/// (`is_own_unlinked`): placed as the files setting says (renamed in place).
pub fn is_own_unlinked(found: &Found, file: &str, filesdir: Option<&str>) -> bool {
    *found == Found::Own(None) && filesdir.is_some_and(|d| !d.is_empty() && is_inside(file, d))
}

/// The files below a folder, as `os.walk` lists them: the files of a folder (sorted) before its
/// subfolders (sorted); hidden names, `.bib` files and the links to folders left out.
fn walk(root: &str, found: &mut Vec<String>) {
    let Ok(read) = std::fs::read_dir(root) else {
        return;
    };
    let (mut dirs, mut files): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = join(root, &name);
        if std::fs::metadata(&path).is_ok_and(|m| m.is_dir()) {
            let link = std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink());
            if !name.starts_with('.') && !link {
                dirs.push(name);
            }
        } else {
            files.push(name);
        }
    }
    files.sort();
    dirs.sort();
    for name in files {
        if !name.starts_with('.') && !name.ends_with(".bib") {
            found.push(join(root, &name));
        }
    }
    for d in dirs {
        walk(&join(root, &d), found);
    }
}
