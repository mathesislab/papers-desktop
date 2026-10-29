//! The library of papers (`papers.bib.Biblio`, `papers.library`): a bibtex
//! with its entries, its configuration (the templates of the keys and file
//! names, the files directory, the relative paths), and the pairs of
//! `distinct.json`.
//!
//! ```no_run
//! use papers_rs::config::MachineConfig;
//! use papers_rs::library::{InsertOptions, Library};
//! use papers_rs::merge::{OnDuplicate, Resolver};
//!
//! let config = papers_rs::config::find_config(".", MachineConfig::load(None)?)?;
//! let mut library = Library::open(&config)?;
//! let mut resolver = Resolver::new(OnDuplicate::Keep);
//! let options = InsertOptions { update_key: true, check_duplicate: true, ..Default::default() };
//! # let entry = papers_rs::bib::Entry::new("article", "x");
//! let inserted = library.insert(entry, &options, &mut resolver)?;
//! let saved = library.save(false)?; // what the history of the library records: saved.before, saved.after, saved.file_ops
//! # Ok::<(), papers_rs::error::Error>(())
//! ```
//!
//! What it does as papers does:
//!
//! - **load**: the bibtex is read (`Bib`), the `distinct.json` of the library
//!   folder (that of the bibtex, for a bibtex without library), and the
//!   entries as they are are remembered ([`Library::mark_loaded`]) for the
//!   report of the changes ([`Library::changed_entries`]) and the keys that
//!   were renamed;
//! - **insert** ([`Library::insert`]): the key from the template, the same
//!   paper in the library (merged as `--on-duplicate` says), a key that is
//!   taken (the suffix rule), the files placed by the name template;
//! - **save** ([`Library::save`]): a library that did not change is not
//!   written; a text that is already on disk is not written; the file is
//!   replaced in one step; a bibtex that another program wrote since the
//!   load is not written. The `crossref` fields follow the keys renamed.
//!
//! What it leaves to others: the history (what the save returns: the text
//! before and after, and the file operations, see [`Saved`]), the lock of
//! the library, the pools, `.papers/files.json`.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use crate::bib::{Bib, Block, Entry, Field, read_file};
use crate::config::paths::{
    abspath, basename, dirname, exists, expanduser, is_abs, isfile, join, realpath,
};
use crate::config::{Config, write_atomic};
use crate::entry::{self, files as entry_files, set_files, val};
use crate::error::{Error, Result};
use crate::identity::{
    Distinct, IdKind, IdentityIndex, Kind, Verdict, duplicate_groups, entry_identifiers,
    find_duplicates, parse_identifier, same_paper,
};
use crate::log;
use crate::merge::{Context, Decision, DuplicateError, Merge, OnDuplicate, Resolve, Resolver};
use crate::names::{
    Format, FormatKind, append_abc_in, attachment_names, check_key, fallback_key, target_in,
};

// files
// -----

/// What to do with a file: move it, copy it (a hard link when possible), or
/// leave it where it is and link it there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Move,
    Copy,
    Link,
}

/// What to do with each file of an entry (`copy`, `files_policy`).
#[derive(Clone)]
pub struct Policy(Arc<dyn Fn(&str) -> Action + Send + Sync>);

impl Policy {
    /// The same for all the files.
    pub fn all(action: Action) -> Policy {
        Policy(Arc::new(move |_| action))
    }

    /// Decided file by file.
    pub fn by_file(f: impl Fn(&str) -> Action + Send + Sync + 'static) -> Policy {
        Policy(Arc::new(f))
    }

    /// What to do with this file.
    pub fn action(&self, file: &str) -> Action {
        (self.0)(file)
    }
}

impl fmt::Debug for Policy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Policy")
    }
}

/// The kind of a file operation that the library did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Moved,
    Copied,
}

/// A file operation of a command, as the history records it (`papers.journal`):
/// the paths are absolute, the checksum is the sha256 (hex) of the file after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOp {
    pub op: OpKind,
    pub src: String,
    pub dst: String,
    pub checksum: Option<String>,
}

impl FileOp {
    /// The name of the operation in the history (`moved`, `copied`).
    pub fn name(&self) -> &'static str {
        match self.op {
            OpKind::Moved => "moved",
            OpKind::Copied => "copied",
        }
    }
}

fn io_error(path: &str, e: &std::io::Error) -> Error {
    Error::other(format!("{path}: {}", crate::config::machine::io_message(e)))
}

fn same_file(a: &str, b: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

fn file_checksum(path: &str) -> Option<String> {
    isfile(path)
        .then(|| crate::merge::checksum_hex(path).ok())
        .flatten()
}

/// The path where the file `source` goes when its name is `path` (`free_name`): `path`
/// itself if it is free, or `source` itself, or a file with the same content (the same
/// file); else `path` with a `-2`, `-3`... suffix before the extension: a file of
/// another entry is never replaced.
pub fn free_name(path: &str, source: &str) -> String {
    let (base, ext) = crate::names::splitext(path);
    let lexists = |p: &str| std::fs::symlink_metadata(p).is_ok();
    let (mut candidate, mut i) = (path.to_string(), 2);
    while lexists(&candidate) {
        if realpath(&candidate) == realpath(source)
            || (isfile(&candidate)
                && isfile(source)
                && file_checksum(&candidate) == file_checksum(source))
        {
            return candidate;
        }
        candidate = format!("{base}-{i}{ext}");
        i += 1;
    }
    candidate
}

/// The legacy name of the hidden bibtex of an entry directory (before papers 2.19).
fn hidden_bibtex(direc: &str) -> String {
    join(direc, &format!(".{}.bib", basename(direc)))
}

// the library
// -----------

/// What `insert` did with an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum What {
    /// a new entry
    New,
    /// a new entry whose key was taken: it got a letter suffix
    NewKey { from: String },
    /// the key is in the library, for the same paper: only the files were added
    ExactDuplicate,
    /// the same paper as a library entry, merged into it; `already_in_library` if
    /// there was nothing to add
    Merged { already_in_library: bool },
    /// the same paper, left alone (`skip`, or a title guard question that `keep` does not answer)
    Skipped,
    /// found to be the same paper and declared not (`distinct.json`): inserted
    NotSame,
}

/// The result of `insert`: the key of the entry that it ended in (for `Skipped`,
/// the one of the entry that was not inserted), and what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inserted {
    pub key: String,
    /// the key of the library entry that the new one is a duplicate of, for `Merged`, `Skipped`, `NotSame`
    pub existing: Option<String>,
    pub what: What,
}

/// The options of `insert` (`insert_entry`).
#[derive(Debug, Clone, Default)]
pub struct InsertOptions {
    /// give the entry a key from the key template (an entry new to the library only)
    pub update_key: bool,
    /// look for the same paper in the library, merge or not as the resolver says
    pub check_duplicate: bool,
    /// place the files of the entry by the name template, as the policy says
    pub rename: Option<Policy>,
    /// the fields given on the command line (`add --title ...`): they replace the
    /// fields of the entry; `ID` sets the key, `ENTRYTYPE` the type, and the
    /// files of `file` are added to those of the entry
    pub metadata: Vec<(String, String)>,
}

/// Finds an added entry elsewhere (in the pool of the library): [`Library::add_entries_taking`]
/// inserts the entry that it takes instead of the added one, and tells it where it ended.
pub trait Taker {
    /// The entry to insert in place of an added entry (`pool_lookup.entry`), if it is found.
    fn take(&mut self, library: &Library, entry: &Entry) -> Result<Option<Entry>>;

    /// The entry taken was inserted (`inserted`): `rename` is how the files are placed.
    fn inserted(
        &mut self,
        library: &mut Library,
        inserted: &Inserted,
        rename: Option<&Policy>,
    ) -> Result<()>;
}

/// The entries as loaded: for the changes and the keys renamed.
#[derive(Debug, Clone)]
struct Loaded {
    /// the uid and the entry at that time
    entries: Vec<(u64, Entry)>,
    relative_to: String,
}

/// The bibtex as it was read or last written.
#[derive(Debug, Clone)]
struct ReadState {
    path: String,
    size: u64,
    mtime_ns: i128,
    text: String,
}

/// What `save` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveStatus {
    /// the library did not change since it was loaded: nothing was looked at or written
    Unchanged,
    /// the text to write is the text on disk: not written
    Identical,
    /// written
    Written,
    /// a dry run: not written
    DryRun,
}

/// What a save leaves for the history of the library: the text of the bibtex
/// before and after, and what the command did to the files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub path: String,
    pub status: SaveStatus,
    /// the text of the bibtex as it was read (or last written) by this library
    pub before: Option<String>,
    /// the text of the library (what is on disk after a save that wrote or found it identical)
    pub after: String,
    /// the keys renamed by the command, `(old, new)`: whoever keeps records by key (pools) follows them
    pub renamed_keys: Vec<(String, String)>,
    /// the file operations since the library was loaded (or last saved)
    pub file_ops: Vec<FileOp>,
    /// `distinct.json` was written
    pub distinct_written: bool,
}

/// Why a save failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// the bibtex was written by another program since it was read: nothing was written, and
    /// the files that the command placed should be put back
    BibtexChanged(String),
    Error(Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::BibtexChanged(m) => f.write_str(m),
            SaveError::Error(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<SaveError> for Error {
    fn from(e: SaveError) -> Error {
        match e {
            SaveError::BibtexChanged(m) => Error::exit(m),
            SaveError::Error(e) => e,
        }
    }
}

impl From<Error> for SaveError {
    fn from(e: Error) -> SaveError {
        SaveError::Error(e)
    }
}

/// The bibtex of a library, with its entries and configuration.
pub struct Library {
    /// the path of the bibtex
    pub bibtex: String,
    pub filesdir: Option<String>,
    /// what the links of the `file` fields are relative to: the directory of the bibtex, or `/` for absolute links
    pub relative_to: String,
    pub nameformat: Format,
    pub keyformat: Format,
    pub distinct: Distinct,
    /// the changes that were done at load (links written as the `paths` setting says): saved even if the command changes nothing
    pub load_changes: bool,
    /// a dry run: the files are not touched, the library is not written
    pub dry_run: bool,
    /// asked before a file replaces another with other content (papers asks `Replace? (y/n)`); the default says no
    pub confirm_replace: Box<dyn FnMut(&str) -> bool + Send>,
    bib: Bib,
    /// the uid of each block of the bib (0 for what is not an entry)
    uids: Vec<u64>,
    next_uid: u64,
    index: Option<IdentityIndex>,
    unreadable: Vec<(usize, String)>,
    read_state: Option<ReadState>,
    loaded: Option<Loaded>,
    ops: Vec<FileOp>,
    placed: HashMap<String, (String, &'static str)>,
}

impl fmt::Debug for Library {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Library")
            .field("bibtex", &self.bibtex)
            .field("entries", &self.len())
            .finish()
    }
}

/// The message about the entries of a bibtex that could not be read (`unreadable_message`).
pub fn unreadable_message(bibtex: &str, unreadable: &[(usize, String)]) -> String {
    unreadable_message_with(
        bibtex,
        unreadable,
        "papers ignores them until they are fixed",
    )
}

/// [`unreadable_message`] with what becomes of the entries (`outcome`).
pub fn unreadable_message_with(
    bibtex: &str,
    unreadable: &[(usize, String)],
    outcome: &str,
) -> String {
    let lines: Vec<String> = unreadable
        .iter()
        .map(|(line, why)| format!("line {line}: {why}"))
        .collect();
    let n = unreadable.len();
    let home = expanduser("~");
    let shown = match bibtex.strip_prefix(&format!("{home}/")) {
        Some(rest) => format!("~/{rest}"),
        None => bibtex.to_string(),
    };
    format!(
        "{shown}: {n} entr{} could not be read ({}): {outcome}",
        if n > 1 { "ies" } else { "y" },
        lines.join("; ")
    )
}

/// Read a bibtex text that is to be added (`add_bibtex`): a warning says which entries
/// could not be read, they are not added.
pub fn parse_added_bibtex(text: &str) -> Bib {
    let bib = Bib::parse(text);
    let unreadable = bib.unreadable();
    if !unreadable.is_empty() {
        log::warning(&unreadable_message_with(
            "the added bibtex",
            &unreadable,
            "they are not added",
        ));
    }
    bib
}

impl Library {
    /// An empty library in memory (`Biblio(...)`): `relative_to` is `/` for absolute links.
    pub fn new(
        bibtex: &str,
        filesdir: Option<&str>,
        relative_to: Option<&str>,
        nameformat: Format,
        keyformat: Format,
    ) -> Library {
        Library {
            bibtex: bibtex.to_string(),
            filesdir: filesdir.map(str::to_string),
            relative_to: relative_to.map_or_else(|| "/".to_string(), str::to_string),
            nameformat,
            keyformat,
            distinct: Distinct::new(),
            load_changes: false,
            dry_run: false,
            confirm_replace: Box::new(|_| false),
            bib: Bib::default(),
            uids: Vec::new(),
            next_uid: 1,
            index: None,
            unreadable: Vec::new(),
            read_state: None,
            loaded: None,
            ops: Vec::new(),
            placed: HashMap::new(),
        }
    }

    /// Read a bibtex (`Biblio.load`): `relative_to` is the directory of the bibtex if not given.
    /// A warning says which entries could not be read. The pairs of `distinct.json` are those of
    /// a bibtex without library, in the `.papers` next to it (see [`Library::load_with`]).
    pub fn load(
        bibtex: &str,
        filesdir: Option<&str>,
        relative_to: Option<&str>,
        nameformat: Format,
        keyformat: Format,
    ) -> Result<Library> {
        let distinct = Distinct::for_bibtex(bibtex);
        Library::load_with(
            bibtex,
            filesdir,
            relative_to,
            nameformat,
            keyformat,
            distinct,
        )
    }

    /// [`Library::load`] with the pairs of `distinct.json` of the library (`Distinct::for_library`).
    pub fn load_with(
        bibtex: &str,
        filesdir: Option<&str>,
        relative_to: Option<&str>,
        nameformat: Format,
        keyformat: Format,
        distinct: Distinct,
    ) -> Result<Library> {
        let text = read_file(std::path::Path::new(bibtex)).map_err(|e| io_error(bibtex, &e))?;
        let relative_to = relative_to.map_or_else(|| dirname(bibtex), str::to_string);
        let mut library = Library::new(bibtex, filesdir, Some(&relative_to), nameformat, keyformat);
        library.distinct = distinct;
        library.set_bib(Bib::parse(&text));
        library.remember(&text);
        library.unreadable = library.bib.unreadable();
        if !library.unreadable.is_empty() {
            log::warning(&unreadable_message(bibtex, &library.unreadable));
        }
        Ok(library)
    }

    /// A library whose bibtex does not exist yet: it is created, empty (`Biblio.newbib`).
    pub fn create(
        bibtex: &str,
        filesdir: Option<&str>,
        relative_to: Option<&str>,
        nameformat: Format,
        keyformat: Format,
    ) -> Result<Library> {
        let distinct = Distinct::for_bibtex(bibtex);
        Library::create_with(
            bibtex,
            filesdir,
            relative_to,
            nameformat,
            keyformat,
            distinct,
        )
    }

    /// [`Library::create`] with the pairs of `distinct.json` of the library.
    pub fn create_with(
        bibtex: &str,
        filesdir: Option<&str>,
        relative_to: Option<&str>,
        nameformat: Format,
        keyformat: Format,
        distinct: Distinct,
    ) -> Result<Library> {
        let dir = dirname(bibtex);
        if !dir.is_empty() && !exists(&dir) {
            std::fs::create_dir_all(&dir).map_err(|e| io_error(&dir, &e))?;
        }
        write_atomic(bibtex, "")?;
        Library::load_with(
            bibtex,
            filesdir,
            relative_to,
            nameformat,
            keyformat,
            distinct,
        )
    }

    /// The library of a configuration (`papers.library.get_biblio`): its bibtex (created if
    /// it does not exist), with the file links as the `paths` setting says. A shallow
    /// library does not repair the links to the files of its pool here.
    pub fn open(config: &Config) -> Result<Library> {
        let Some(bibtex) = config.bibtex.as_deref() else {
            return Err(Error::other("bibtex is not initialized"));
        };
        let relative_to = if config.absolute_paths {
            "/".to_string()
        } else {
            dirname(bibtex)
        };
        let nameformat = Format::from_config(config, FormatKind::Name);
        let keyformat = Format::from_config(config, FormatKind::Key);
        let filesdir = config.filesdir.as_deref();
        let distinct = Distinct::for_library(&config.libdir());
        let mut library;
        if exists(bibtex) {
            library = Library::load_with(bibtex, filesdir, None, nameformat, keyformat, distinct)?;
            if library.relative_to != relative_to {
                if !library.set_relative_to(&relative_to).is_empty() {
                    library.load_changes = true;
                }
            } else if !config.absolute_paths && !library.relative_links().is_empty() {
                // absolute links in a library with relative paths become relative
                library.load_changes = true;
            }
        } else {
            library = Library::create_with(
                bibtex,
                filesdir,
                Some(&relative_to),
                nameformat,
                keyformat,
                distinct,
            )?;
        }
        library.mark_loaded();
        Ok(library)
    }

    /// [`Library::open`] for a library that is read, not written to: a bibtex that does not
    /// exist is an empty library in memory, not created (`load_biblio` of the pools).
    pub fn open_existing(config: &Config) -> Result<Library> {
        let Some(bibtex) = config.bibtex.as_deref() else {
            return Err(Error::other("bibtex is not initialized"));
        };
        if exists(bibtex) {
            return Library::open(config);
        }
        let relative_to = if config.absolute_paths {
            "/".to_string()
        } else {
            dirname(bibtex)
        };
        let mut library = Library::new(
            bibtex,
            config.filesdir.as_deref(),
            Some(&relative_to),
            Format::from_config(config, FormatKind::Name),
            Format::from_config(config, FormatKind::Key),
        );
        library.distinct = Distinct::for_library(&config.libdir());
        Ok(library)
    }

    fn set_bib(&mut self, bib: Bib) {
        self.uids = bib
            .blocks
            .iter()
            .map(|b| {
                if matches!(b, Block::Entry(_)) {
                    self.next_uid += 1;
                    self.next_uid - 1
                } else {
                    0
                }
            })
            .collect();
        self.bib = bib;
        self.index = None;
    }

    fn remember(&mut self, text: &str) {
        let path = realpath(&self.bibtex);
        if let Ok(meta) = std::fs::metadata(&path) {
            self.read_state = Some(ReadState {
                path,
                size: meta.len(),
                mtime_ns: mtime_ns(&meta),
                text: text.to_string(),
            });
        }
    }

    /// Remember the entries as they are (`mark_loaded`), for [`Library::changed_entries`],
    /// [`Library::renamed_keys`] and [`Library::unchanged`].
    pub fn mark_loaded(&mut self) {
        let entries = self.uid_entries().map(|(u, e)| (u, e.clone())).collect();
        self.loaded = Some(Loaded {
            entries,
            relative_to: self.relative_to.clone(),
        });
    }

    /// The blocks that could not be read: the line and why (`unreadable_blocks`).
    pub fn unreadable(&self) -> &[(usize, String)] {
        &self.unreadable
    }

    // the entries
    // -----------

    fn uid_entries(&self) -> impl Iterator<Item = (u64, &Entry)> {
        self.bib
            .blocks
            .iter()
            .zip(&self.uids)
            .filter_map(|(b, &u)| match b {
                Block::Entry(e) => Some((u, e)),
                _ => None,
            })
    }

    /// The entries, in the order of the library.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.bib.entries()
    }

    pub fn len(&self) -> usize {
        self.entries().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The text of the library as a save writes it (`Biblio.format` after `sort`).
    pub fn text(&self) -> String {
        self.bib.to_string()
    }

    /// The index of the block of an entry: that of this key, else the first whose key is the
    /// same without case.
    fn position(&self, key: &str) -> Option<usize> {
        let entries = || {
            self.bib.blocks.iter().enumerate().filter_map(|(i, b)| {
                if let Block::Entry(e) = b {
                    Some((i, e))
                } else {
                    None
                }
            })
        };
        entries()
            .find(|(_, e)| e.key == key)
            .or_else(|| entries().find(|(_, e)| e.key.to_lowercase() == key.to_lowercase()))
            .map(|(i, _)| i)
    }

    /// An entry by its key (that key, else the same without case).
    pub fn get(&self, key: &str) -> Option<&Entry> {
        match &self.bib.blocks[self.position(key)?] {
            Block::Entry(e) => Some(e),
            _ => None,
        }
    }

    /// An entry to change by its key. Whatever the caller does, the index of identities is rebuilt when needed.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Entry> {
        let i = self.position(key)?;
        self.index = None;
        match &mut self.bib.blocks[i] {
            Block::Entry(e) => Some(e),
            _ => None,
        }
    }

    /// The entries named by identifiers (a key, a former key, a DOI, an arXiv id, an ISBN; `-` is
    /// for the caller to read), in the order given and without repetition: their keys, and the
    /// identifiers that match no entry (`select_some`). An identifier that matches several entries
    /// is an error, raised before anything is changed.
    pub fn select_some(&self, args: &[String]) -> Result<(Vec<String>, Vec<String>)> {
        let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
        let all: Vec<(u64, &Entry)> = self.uid_entries().collect();
        for (n, (_, e)) in all.iter().enumerate() {
            by_key.entry(e.key.to_lowercase()).or_default().push(n);
        }
        let mut index: Option<HashMap<(IdKind, String), Vec<usize>>> = None;
        let (mut found_keys, mut errors, mut missing): (Vec<usize>, Vec<String>, Vec<String>) =
            Default::default();
        for arg in args {
            let ident = match parse_identifier(arg) {
                Ok(i) => i,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
            let mut found: Vec<usize> = if ident.0 == IdKind::Key {
                by_key.get(&ident.1).cloned().unwrap_or_default()
            } else {
                Vec::new()
            };
            if found.len() != 1 {
                let index = index.get_or_insert_with(|| {
                    let mut m: HashMap<(IdKind, String), Vec<usize>> = HashMap::new();
                    for (n, (_, e)) in all.iter().enumerate() {
                        for id in entry_identifiers(e) {
                            let v = m.entry(id).or_default();
                            if !v.contains(&n) {
                                v.push(n);
                            }
                        }
                    }
                    m
                });
                found = index.get(&ident).cloned().unwrap_or_default();
                if found.is_empty() && ident.0 == IdKind::Doi {
                    // a key that is a DOI (entries fetched with the DOI as key)
                    found = index
                        .get(&(IdKind::Key, ident.1.clone()))
                        .cloned()
                        .unwrap_or_default();
                }
            }
            match found.as_slice() {
                [] => {
                    if !missing.contains(arg) {
                        missing.push(arg.clone());
                    }
                }
                [one] => {
                    if !found_keys.contains(one) {
                        found_keys.push(*one);
                    }
                }
                several => errors.push(format!(
                    "{arg}: several entries match: {}",
                    several
                        .iter()
                        .map(|&n| all[n].1.key.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            }
        }
        if !errors.is_empty() {
            return Err(Error::exit(format!(
                "{}\n(nothing was changed)",
                errors.join("\n")
            )));
        }
        Ok((
            found_keys
                .into_iter()
                .map(|n| all[n].1.key.clone())
                .collect(),
            missing,
        ))
    }

    /// Like [`Library::select_some`], but an identifier that matches no entry is an error.
    pub fn select(&self, args: &[String]) -> Result<Vec<String>> {
        let (keys, missing) = self.select_some(args)?;
        if !missing.is_empty() {
            let lines: Vec<String> = missing
                .iter()
                .map(|a| format!("{a}: no entry found"))
                .collect();
            return Err(Error::exit(format!(
                "{}\n(nothing was changed)",
                lines.join("\n")
            )));
        }
        Ok(keys)
    }

    // keys
    // ----

    fn lower_keys(&self) -> HashSet<String> {
        self.entries().map(|e| e.key.to_lowercase()).collect()
    }

    /// A key for an entry from the key template, not yet in the library (`generate_key` of a
    /// new entry): a key that is taken gets a letter suffix. Never empty: when the template gives
    /// an empty key, it is made from the title and the year ([`fallback_key`]).
    pub fn generate_key(&self, entry: &Entry) -> Result<String> {
        self.template_key(entry, false)
    }

    /// The key from the key template of an entry that is in the library already (a copy of
    /// it: `check --auto-key`): its own key is not taken, so that an entry whose key is the
    /// template key keeps it (`smith2000` stays `smith2000`). Empty when the template gives an
    /// empty key (the entry keeps its key).
    pub fn regenerate_key(&self, entry: &Entry) -> Result<String> {
        self.template_key(entry, true)
    }

    fn template_key(&self, entry: &Entry, in_library: bool) -> Result<String> {
        let own = entry.key.to_lowercase();
        let keys: HashSet<String> = self
            .entries()
            .map(|e| e.key.to_lowercase())
            .filter(|k| !(in_library && *k == own))
            .collect();
        let mut key = self.keyformat.render(entry)?;
        if key.is_empty() {
            if in_library {
                return Ok(key);
            }
            key = fallback_key(
                entry.get("title").unwrap_or(""),
                entry.get("year").unwrap_or(""),
            );
        }
        if keys.contains(&key.to_lowercase()) {
            return append_abc_in(&key, |k| keys.contains(k)).map_err(Error::exit);
        }
        Ok(key)
    }

    /// Give the entry of key `old` the key `new` (`set_key`); with `keep_old`, its former key
    /// goes to its `ids`. The key must be valid, and not another entry's (without case).
    /// The records that name the entry (`crossref` of the others, the records of the pools) follow
    /// when the library is saved.
    pub fn set_key(&mut self, old: &str, new: &str, keep_old: bool) -> Result<()> {
        if let Some(message) = check_key(new) {
            return Err(Error::exit(message));
        }
        let i = self
            .position(old)
            .ok_or_else(|| Error::exit(format!("{old}: no entry found")))?;
        let others = self
            .bib
            .blocks
            .iter()
            .enumerate()
            .any(|(j, b)| matches!(b, Block::Entry(e) if j != i && e.key.to_lowercase() == new.to_lowercase()));
        if others {
            return Err(Error::exit(format!(
                "{new}: another entry of the library has this key"
            )));
        }
        if let Block::Entry(e) = &mut self.bib.blocks[i] {
            entry::set_key(e, new, keep_old);
        }
        self.index = None;
        Ok(())
    }

    /// The keys that changed since [`Library::mark_loaded`], `(old, new)`: the same entry, its
    /// old key no longer in the library, its new key not in it when loaded (a merge is not a
    /// rename); or a change of case only, the key being that of no other entry. None without
    /// `mark_loaded`.
    pub fn renamed_keys(&self) -> Vec<(String, String)> {
        let Some(loaded) = &self.loaded else {
            return Vec::new();
        };
        let count = |keys: Vec<String>| {
            let mut counts: HashMap<String, usize> = HashMap::new();
            for key in keys {
                *counts.entry(key).or_default() += 1;
            }
            counts
        };
        let now = count(self.entries().map(|e| e.key.to_lowercase()).collect());
        let before = count(
            loaded
                .entries
                .iter()
                .map(|(_, e)| e.key.to_lowercase())
                .collect(),
        );
        let current: HashMap<u64, &Entry> = self.uid_entries().collect();
        loaded
            .entries
            .iter()
            .filter_map(|(uid, old)| {
                let e = current.get(uid)?;
                let (new_lower, old_lower) = (e.key.to_lowercase(), old.key.to_lowercase());
                let changed = if new_lower == old_lower {
                    // a change of case only, the key being that of no other entry
                    e.key != old.key
                        && now.get(&old_lower) == Some(&1)
                        && before.get(&old_lower) == Some(&1)
                } else {
                    !now.contains_key(&old_lower) && !before.contains_key(&new_lower)
                };
                changed.then(|| (old.key.clone(), e.key.clone()))
            })
            .collect()
    }

    /// The `crossref` fields that name `old` (a key that changed to `new`), without case, name `new` (`rekey_references`).
    pub fn rekey_references(&mut self, old: &str, new: &str) {
        for e in self.bib.entries_mut() {
            if val(e, "crossref").to_lowercase() == old.to_lowercase() {
                e.set("crossref", new);
            }
        }
        self.index = None;
    }

    // files and tags
    // --------------

    /// The paths (absolute) of the files of an entry (`get_files`).
    pub fn files(&self, key: &str) -> Vec<String> {
        self.get(key)
            .map(|e| entry_files(e, Some(&self.relative_to)))
            .unwrap_or_default()
    }

    /// Give an entry these files (`set_files`): the field is written as the `paths` setting says.
    pub fn set_files(&mut self, key: &str, files: &[String]) -> Result<()> {
        let relative_to = self.relative_to.clone();
        let e = self
            .get_mut(key)
            .ok_or_else(|| Error::exit(format!("{key}: no entry found")))?;
        set_files(e, files, &relative_to);
        Ok(())
    }

    /// Add tags to the entries of these keys (after their tags, those they have already skipped): the keys of the entries that changed.
    pub fn add_tags(&mut self, keys: &[String], tags: &[String]) -> Vec<String> {
        self.map_entries(keys, |e| entry::add_tags(e, tags))
    }

    /// Remove tags from the entries of these keys: the keys of those that changed.
    pub fn remove_tags(&mut self, keys: &[String], tags: &[String]) -> Vec<String> {
        self.map_entries(keys, |e| entry::remove_tags(e, tags))
    }

    /// Rename a tag, and the tags under it, in all the entries: the keys of those that changed.
    pub fn rename_tag(&mut self, old: &str, new: &str) -> Vec<String> {
        let keys: Vec<String> = self.entries().map(|e| e.key.clone()).collect();
        self.map_entries(&keys, |e| entry::rename_tag(e, old, new))
    }

    fn map_entries(
        &mut self,
        keys: &[String],
        mut f: impl FnMut(&mut Entry) -> bool,
    ) -> Vec<String> {
        let mut changed = Vec::new();
        let wanted: HashSet<&str> = keys.iter().map(String::as_str).collect();
        for e in self.bib.entries_mut() {
            if wanted.contains(e.key.as_str()) && f(e) {
                changed.push(e.key.clone());
            }
        }
        self.index = None;
        changed
    }

    /// Write the links of all the entries relative to another directory, `/` for absolute
    /// links (`update_file_path`): the fields that changed, before and after.
    pub fn set_relative_to(&mut self, relative_to: &str) -> Vec<(String, String)> {
        let from = self.relative_to.clone();
        let updates: Vec<(String, String)> = self
            .bib
            .entries_mut()
            .filter_map(|e| entry::update_file_path(e, &from, relative_to))
            .collect();
        if !updates.is_empty() {
            log::info(&format!("{} entry files were updated", updates.len()));
        }
        self.relative_to = relative_to.to_string();
        self.index = None;
        updates
    }

    /// The absolute links of a library with relative links written relative to the
    /// bibtex directory (`relative_links`): the fields that changed, before and after.
    pub fn relative_links(&mut self) -> Vec<(String, String)> {
        let relative_to = self.relative_to.clone();
        let updates: Vec<(String, String)> = self
            .bib
            .entries_mut()
            .filter(|e| entry::files(e, None).iter().any(|f| is_abs(f)))
            .filter_map(|e| entry::update_file_path(e, &relative_to, &relative_to))
            .collect();
        if !updates.is_empty() {
            self.index = None;
        }
        updates
    }

    /// Place the files of an entry by the name template (`rename_entry_files`): the first is
    /// `<filesdir>/<name>.<ext>`, the others `<name>--<label>.<ext>`. The name must stay in
    /// the files directory: if it does not, the files are left where they are (and an error is logged).
    /// `formatter` is another name template; `relative_to` how to write the links (the library's).
    /// Moves and copies are recorded in [`Library::file_ops`].
    pub fn rename_entry_files(
        &mut self,
        key: &str,
        policy: &Policy,
        formatter: Option<&Format>,
        relative_to: Option<&str>,
        hardlink: bool,
    ) -> Result<()> {
        let Some(filesdir) = self.filesdir.clone() else {
            return Err(Error::other("filesdir is None, cannot rename entries"));
        };
        let Some(entry) = self.get(key).cloned() else {
            return Err(Error::exit(format!("{key}: no entry found")));
        };
        let files = entry_files(&entry, Some(&self.relative_to));
        if files.is_empty() {
            log::info("no files to rename");
            return Ok(());
        }
        let newname = formatter.unwrap_or(&self.nameformat).render(&entry)?;
        let Some(target) = target_in(&filesdir, &newname) else {
            log::error(&format!(
                "{}: the file name {} points outside the files directory: the files are left where they are",
                entry.key,
                crate::config::json::py_repr_str(&newname)
            ));
            return Ok(());
        };
        let actions: Vec<Action> = files.iter().map(|f| policy.action(f)).collect();
        let mut count = 0;
        let mut newfiles = Vec::new();
        for ((file, newfile), action) in files
            .iter()
            .zip(attachment_names(&files, &target))
            .zip(&actions)
        {
            let newfile = if *action == Action::Link {
                file.clone()
            } else if !exists(file) {
                log::warning(&format!("{file}: original file link is broken"));
                file.clone()
            } else if *file != newfile {
                // the name of another entry's file: a suffix
                let newfile = free_name(&newfile, file);
                if *file != newfile {
                    self.move_file(file, &newfile, *action == Action::Copy, hardlink)?;
                    count += 1;
                }
                newfile
            } else {
                newfile
            };
            newfiles.push(newfile);
        }
        let relative_to = relative_to.map_or_else(|| self.relative_to.clone(), str::to_string);
        if let Some(e) = self.get_mut(key) {
            set_files(e, &newfiles, &relative_to);
        }
        let mut dirs: Vec<String> = Vec::new();
        for (file, action) in files.iter().zip(&actions) {
            let d = dirname(file);
            if *action == Action::Move && !dirs.contains(&d) {
                dirs.push(d);
            }
        }
        for d in dirs {
            self.remove_entry_dir_if_empty(&d);
        }
        if count > 0 {
            log::info(&format!("renamed file(s): {count}"));
        }
        Ok(())
    }

    /// `rename_entry_files` for all the entries; an error of one is logged and does not stop the others.
    pub fn rename_all_files(&mut self, policy: &Policy, relative_to: Option<&str>, hardlink: bool) {
        let keys: Vec<String> = self.entries().map(|e| e.key.clone()).collect();
        for key in keys {
            if let Err(e) = self.rename_entry_files(&key, policy, None, relative_to, hardlink) {
                log::error(e.message());
            }
        }
        if let Some(r) = relative_to {
            self.relative_to = r.to_string();
        }
    }

    /// A directory of the layout before papers 2.19 (`<name>/` with a hidden `.<name>.bib`) is removed when nothing else is left in it.
    fn remove_entry_dir_if_empty(&mut self, direc: &str) {
        let hidden = hidden_bibtex(direc);
        if !isfile(&hidden) {
            return;
        }
        let Ok(read) = std::fs::read_dir(direc) else {
            return;
        };
        let mut leftovers: Vec<String> = read
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|f| *f != basename(&hidden))
            .collect();
        if !leftovers.is_empty() {
            leftovers.sort();
            log::info(&format!(
                "keep directory {direc}: other files remain ({})",
                leftovers.join(", ")
            ));
            return;
        }
        log::info(&format!("remove directory: {direc}"));
        if !self.dry_run {
            let _ = std::fs::remove_dir_all(direc);
        }
    }

    /// Move or copy a file (`papers.utils.move`, and the record of `Biblio.move`).
    fn move_file(&mut self, src: &str, dst: &str, copy: bool, hardlink: bool) -> Result<()> {
        let maybe = if self.dry_run { "dry-run:: " } else { "" };
        if src == dst || realpath(src) == realpath(dst) {
            log::info(&format!("dest is identical to src: {src}"));
            return Ok(());
        }
        let dir = dirname(dst);
        if !dir.is_empty() && !exists(&dir) {
            log::info(&format!("{maybe}create directory: {dir}"));
            if !self.dry_run {
                std::fs::create_dir_all(&dir).map_err(|e| io_error(&dir, &e))?;
            }
        }
        let mut done = true;
        if exists(dst)
            && same_file(src, dst)
            && realpath(src).to_lowercase() == realpath(dst).to_lowercase()
        {
            // a case-only rename on a case-insensitive file system: dst is src
            if copy {
                log::info(&format!(
                    "{maybe}dest is identical to src (case-insensitive file system): {src}"
                ));
                return Ok(());
            }
            log::info(&format!("{maybe}mv {src} {dst}"));
            if !self.dry_run {
                std::fs::rename(src, dst).map_err(|e| io_error(src, &e))?;
            }
        } else if exists(dst) && (same_file(src, dst) || file_checksum(dst) == file_checksum(src)) {
            // an identical file (a hard link, the same content): nothing to do but to remove the source
            if !copy && !self.dry_run {
                log::info(&format!("{maybe}rm {src}"));
                std::fs::remove_file(src).map_err(|e| io_error(src, &e))?;
            }
        } else {
            if exists(dst) && !(self.confirm_replace)(dst) {
                return Ok(());
            }
            if copy {
                if hardlink {
                    log::info(&format!("{maybe}ln {src} {dst}"));
                    if self.dry_run || std::fs::hard_link(src, dst).is_ok() {
                        done = true;
                    } else {
                        // another file system, or no hard links there
                        self.copy_file(src, dst, maybe)?;
                    }
                } else {
                    self.copy_file(src, dst, maybe)?;
                }
            } else {
                log::info(&format!("{maybe}mv {src} {dst}"));
                if !self.dry_run && std::fs::rename(src, dst).is_err() {
                    std::fs::copy(src, dst).map_err(|e| io_error(src, &e))?;
                    std::fs::remove_file(src).map_err(|e| io_error(src, &e))?;
                }
            }
        }
        if done && !self.dry_run && realpath(src) != realpath(dst) {
            let source = self
                .placed
                .remove(src)
                .map_or_else(|| src.to_string(), |(s, _)| s);
            self.placed.insert(
                dst.to_string(),
                (source, if copy { "copied" } else { "moved" }),
            );
            self.ops.push(FileOp {
                op: if copy { OpKind::Copied } else { OpKind::Moved },
                src: abspath(src),
                dst: abspath(dst),
                checksum: file_checksum(dst),
            });
        }
        Ok(())
    }

    fn copy_file(&self, src: &str, dst: &str, maybe: &str) -> Result<()> {
        log::info(&format!("{maybe}cp {src} {dst}"));
        if !self.dry_run {
            if exists(dst) {
                std::fs::remove_file(dst).map_err(|e| io_error(dst, &e))?;
            }
            std::fs::copy(src, dst).map_err(|e| io_error(src, &e))?;
        }
        Ok(())
    }

    /// The file operations since the library was loaded or last saved (not removed).
    pub fn file_ops(&self) -> &[FileOp] {
        &self.ops
    }

    /// The files placed by the command: `{new path: (original path, "moved" or "copied")}`.
    pub fn placed(&self) -> &HashMap<String, (String, &'static str)> {
        &self.placed
    }

    // inserting
    // ---------

    fn ensure_index(&mut self) {
        if self.index.is_none() {
            let mut index = IdentityIndex::new();
            for (uid, e) in self.uid_entries() {
                index.add(uid, e);
            }
            self.index = Some(index);
        }
    }

    fn push_entry(&mut self, e: Entry) -> u64 {
        let uid = self.next_uid;
        self.next_uid += 1;
        if let Some(index) = &mut self.index {
            index.add(uid, &e);
        }
        self.bib.blocks.push(Block::Entry(e));
        self.uids.push(uid);
        uid
    }

    fn block_of_uid(&self, uid: u64) -> Option<usize> {
        self.uids.iter().position(|&u| u == uid)
    }

    /// Insert an entry and say where it ended (`insert_entry`): with `metadata`, its fields are
    /// set first. With `check_duplicate`, the library is searched for the same paper
    /// ([`crate::identity`]); the resolver decides what to do with it ([`Resolver`]: merge as
    /// `--on-duplicate keep` or `update`, skip, error; or a dialogue). An entry without a duplicate
    /// gets its key from the template with `update_key`; a key that is taken gets a letter
    /// suffix, unless it is the key of the same paper (the files are added to the library entry)
    /// or of another (an error).
    pub fn insert(
        &mut self,
        mut entry: Entry,
        options: &InsertOptions,
        resolver: &mut dyn Resolve,
    ) -> Result<Inserted> {
        let mut update_key = options.update_key;
        if !options.metadata.is_empty() {
            self.apply_metadata(&mut entry, &options.metadata);
            if options.metadata.iter().any(|(k, _)| k == "ID") {
                update_key = false;
            }
        }
        if options.check_duplicate {
            return self.insert_check(entry, update_key, options, resolver);
        }
        self.insert_new(entry, update_key, options, false)
    }

    /// The fields of `metadata` set in the entry; the files of `file` are added to its own.
    fn apply_metadata(&self, entry: &mut Entry, metadata: &[(String, String)]) {
        for (k, v) in metadata.iter().filter(|(k, _)| k != "file") {
            match k.as_str() {
                "ID" => entry.key = v.clone(),
                "ENTRYTYPE" => entry.entry_type = v.clone(),
                k => entry.set(k, v.clone()),
            }
        }
        let mut files = entry_files(entry, Some(&self.relative_to));
        if let Some((_, v)) = metadata.iter().find(|(k, _)| k == "file") {
            let mut meta = Entry::new("misc", "");
            meta.set("file", v.clone());
            files.extend(entry_files(&meta, Some(&self.relative_to)));
        }
        if !files.is_empty() {
            set_files(entry, &files, &self.relative_to);
        }
    }

    fn insert_new(
        &mut self,
        mut entry: Entry,
        update_key: bool,
        options: &InsertOptions,
        free_key: bool,
    ) -> Result<Inserted> {
        if update_key || entry.key.is_empty() {
            entry.key = self.generate_key(&entry)?;
        }
        let key_lower = entry.key.to_lowercase();
        let existing = self
            .entries()
            .find(|e| e.key.to_lowercase() == key_lower)
            .map(|e| e.key.clone());
        let mut what = What::New;
        if let Some(existing_key) = existing {
            log::debug(&format!("key duplicate: {}", existing_key.to_lowercase()));
            if free_key {
                let keys = self.lower_keys();
                let newkey =
                    append_abc_in(&entry.key, |k| keys.contains(k)).map_err(Error::exit)?;
                log::info(&format!("key conflict: {} => {newkey}", entry.key));
                what = What::NewKey {
                    from: entry.key.clone(),
                };
                entry.key = newkey;
            } else {
                let existing_entry = self.get(&existing_key).cloned().unwrap();
                // the same paper with nothing to decide, as for "already in library"
                // (equivalent values, such as titles that differ only by case or braces,
                // are no difference)
                if same_paper(&existing_entry, &entry, Some(&self.distinct)).kind == Kind::Same
                    && Merge::new(
                        &[existing_entry.clone(), entry.clone()],
                        Some(&self.relative_to),
                        false,
                    )
                    .is_exact()
                {
                    log::info("exact duplicate");
                    // keep the files of the new entry
                    let mut files = entry_files(&existing_entry, Some(&self.relative_to));
                    files.extend(entry_files(&entry, Some(&self.relative_to)));
                    if !files.is_empty() {
                        self.set_files(&existing_key, &files)?;
                    }
                    if let Some(policy) = &options.rename {
                        self.rename_entry_files(&existing_key, policy, None, None, true)?;
                    }
                    return Ok(Inserted {
                        key: existing_key.clone(),
                        existing: Some(existing_key),
                        what: What::ExactDuplicate,
                    });
                }
                // (with update_key, the key generated above is not in the library)
                return Err(Error::exit(format!(
                    "{}: this key is already in the library, for another paper or another version of it: pass -u/--update-key to generate a new key, or --key KEY",
                    entry.key
                )));
            }
        } else {
            log::info(&format!("new entry: {}", entry.key));
        }
        let key = entry.key.clone();
        self.push_entry(entry);
        if let Some(policy) = &options.rename {
            self.rename_entry_files(&key, policy, None, None, true)?;
        }
        Ok(Inserted {
            key,
            existing: None,
            what,
        })
    }

    fn insert_check(
        &mut self,
        entry: Entry,
        update_key: bool,
        options: &InsertOptions,
        resolver: &mut dyn Resolve,
    ) -> Result<Inserted> {
        self.ensure_index();
        let found = find_duplicates(
            &entry,
            self.index.as_ref().unwrap(),
            Some(&self.distinct),
            None,
        );
        let Some((uid, verdict)) = found.into_iter().next() else {
            log::debug("not a duplicate");
            return self.insert_new(entry, update_key, options, false);
        };
        let at = self
            .block_of_uid(uid)
            .expect("the index has the entries of the library");
        let Block::Entry(existing) = &self.bib.blocks[at] else {
            unreachable!()
        };
        let existing = existing.clone();
        log::debug(&format!(
            "duplicate of {}: {}",
            existing.key, verdict.reason
        ));
        let merge = Merge::new(
            &[existing.clone(), entry.clone()],
            Some(&self.relative_to),
            false,
        );
        let (decision, already) = if verdict.kind == Kind::Same && merge.is_exact() {
            let result = merge.result();
            let already = merge.new_files.is_empty();
            (Decision::Merge(Box::new(merge), result), already)
        } else {
            (resolver.decide(merge, Context::Add, &verdict)?, false)
        };
        match decision {
            Decision::Merge(_, merged) => {
                if let Some(index) = &mut self.index {
                    index.remove(uid);
                }
                if let Block::Entry(target) = &mut self.bib.blocks[at] {
                    // the target in the library becomes the merged entry, in place
                    target.entry_type = merged.entry_type.clone();
                    target.key = merged.key.clone();
                    target.fields = merged
                        .fields
                        .iter()
                        .map(|f| Field::new(&f.key, &f.value))
                        .collect();
                }
                if let (Some(index), Block::Entry(target)) = (&mut self.index, &self.bib.blocks[at])
                {
                    index.add(uid, target);
                }
                if let Some(policy) = &options.rename {
                    self.rename_entry_files(&existing.key, policy, None, None, true)?;
                }
                Ok(Inserted {
                    key: existing.key.clone(),
                    existing: Some(existing.key),
                    what: What::Merged {
                        already_in_library: already,
                    },
                })
            }
            Decision::Distinct => {
                self.distinct.add(&existing, &entry);
                let mut inserted = self.insert_new(entry, update_key, options, true)?;
                inserted.existing = Some(existing.key);
                if inserted.what == What::New {
                    inserted.what = What::NotSame;
                }
                Ok(inserted)
            }
            Decision::Skip => Ok(Inserted {
                key: entry.key,
                existing: Some(existing.key),
                what: What::Skipped,
            }),
        }
    }

    /// Insert the entries of a bibtex text (`add_bibtex`): the links of their `file` fields are
    /// relative to `relative_to` (the directory of that bibtex), `attachments` are added to
    /// them; an entry without a key gets one from the template. With `convert_to_unicode`, the
    /// LaTeX of the values is converted first.
    pub fn add_bibtex(
        &mut self,
        text: &str,
        relative_to: Option<&str>,
        attachments: &[String],
        convert_to_unicode: bool,
        options: &InsertOptions,
        resolver: &mut dyn Resolve,
    ) -> Result<Vec<Inserted>> {
        let mut bib = parse_added_bibtex(text);
        if convert_to_unicode {
            bib = crate::bib::unicode::latex_to_unicode_library(bib);
        }
        let entries: Vec<Entry> = bib.entries().cloned().collect();
        self.add_entries(
            entries,
            relative_to,
            attachments,
            "bibtex",
            options,
            resolver,
        )
    }

    /// Insert entries read from a file (bibtex, RIS, CSL-JSON...) (`add_entries`).
    pub fn add_entries(
        &mut self,
        added: Vec<Entry>,
        relative_to: Option<&str>,
        attachments: &[String],
        what: &str,
        options: &InsertOptions,
        resolver: &mut dyn Resolve,
    ) -> Result<Vec<Inserted>> {
        self.add_entries_taking(
            added,
            relative_to,
            attachments,
            what,
            options,
            resolver,
            None,
        )
    }

    /// [`Library::add_entries`] with a [`Taker`]: an entry that it finds elsewhere (in a pool) is
    /// inserted in place of the added one (`pool_lookup.entry`, `_insert_taken`).
    #[allow(clippy::too_many_arguments)]
    pub fn add_entries_taking(
        &mut self,
        added: Vec<Entry>,
        relative_to: Option<&str>,
        attachments: &[String],
        what: &str,
        options: &InsertOptions,
        resolver: &mut dyn Resolve,
        mut taker: Option<&mut dyn Taker>,
    ) -> Result<Vec<Inserted>> {
        let fields = options.metadata.iter().filter(|(k, _)| k != "file").count();
        if fields > 0 && added.len() > 1 {
            return Err(Error::other(format!(
                "the {what} has {} entries: the fields given on the command line (--title, --key, --metadata...) apply to one entry only",
                added.len()
            )));
        }
        let mut out = Vec::new();
        if options.check_duplicate {
            self.ensure_index();
        }
        for mut e in added {
            if e.key.is_empty() {
                e.key = self.generate_key(&e)?;
            }
            let mut files = Vec::new();
            if e.get("file").is_some() {
                // make sure paths relative to other bibtex are inserted correctly
                files.extend(entry_files(&e, relative_to.or(Some(&self.relative_to))));
            }
            files.extend(attachments.iter().map(|f| abspath(f)));
            if !files.is_empty() {
                set_files(&mut e, &files, &self.relative_to);
            }
            let taken = match taker.as_deref_mut() {
                Some(taker) => taker.take(self, &e)?,
                None => None,
            };
            match (taken, taker.as_deref_mut()) {
                (Some(taken), Some(taker)) => {
                    let inserted = self.insert(taken, options, resolver)?;
                    taker.inserted(self, &inserted, options.rename.as_ref())?;
                    out.push(inserted);
                }
                _ => out.push(self.insert(e, options, resolver)?),
            }
        }
        Ok(out)
    }

    /// The key of the library entry that is the same paper as an entry of this DOI (an arXiv
    /// DOI matches the eprint), if there is one (`find_doi`).
    pub fn find_doi(&mut self, doi: &str) -> Option<String> {
        self.ensure_index();
        let mut probe = Entry::new("misc", "doi");
        probe.set("doi", doi.to_string());
        let found = find_duplicates(
            &probe,
            self.index.as_ref().expect("the index"),
            Some(&self.distinct),
            None,
        );
        let (uid, _) = found.into_iter().find(|(_, v)| v.kind == Kind::Same)?;
        let at = self.block_of_uid(uid)?;
        match &self.bib.blocks[at] {
            Block::Entry(e) => Some(e.key.clone()),
            _ => None,
        }
    }

    /// Add an entry as it is, at the end (`db.add`): no search for the same paper, no key from the template.
    pub fn add_unchecked(&mut self, entry: Entry) -> u64 {
        self.push_entry(entry)
    }

    /// Take an entry out of the library (`db.remove`): the entry, if there was one. Its files are for the caller.
    pub fn remove(&mut self, key: &str) -> Option<Entry> {
        let i = self.position(key)?;
        self.index = None;
        self.uids.remove(i);
        match self.bib.blocks.remove(i) {
            Block::Entry(e) => Some(e),
            _ => None,
        }
    }

    // entries by handle (for the pools: an entry keeps its handle while it is in the library)
    // -------------------------------------------------------------------------------------

    /// The handles of the entries, in the order of the library.
    pub fn uids(&self) -> Vec<u64> {
        self.uid_entries().map(|(u, _)| u).collect()
    }

    /// The entry of a handle.
    pub fn entry_by_uid(&self, uid: u64) -> Option<&Entry> {
        match &self.bib.blocks[self.block_of_uid(uid)?] {
            Block::Entry(e) => Some(e),
            _ => None,
        }
    }

    /// The handle of the first entry of this key without case.
    pub fn uid_of_key(&self, key: &str) -> Option<u64> {
        let lower = key.to_lowercase();
        self.uid_entries()
            .find(|(_, e)| e.key.to_lowercase() == lower)
            .map(|(u, _)| u)
    }

    /// The entries that are the same paper as `entry` (`same` first, then `ask`, each in the
    /// order of the library), with the pairs of `distinct`: the handle and the verdict.
    pub fn find_same(&mut self, entry: &Entry, distinct: Option<&Distinct>) -> Vec<(u64, Verdict)> {
        self.ensure_index();
        find_duplicates(
            entry,
            self.index.as_ref().expect("the index"),
            distinct,
            None,
        )
    }

    /// Make the entry of a handle equal to `entry` (`apply_entry`), keeping the index of
    /// identities up to date.
    pub fn apply_by_uid(&mut self, uid: u64, entry: &Entry) {
        let Some(at) = self.block_of_uid(uid) else {
            return;
        };
        if let Some(index) = &mut self.index {
            index.remove(uid);
        }
        if let Block::Entry(target) = &mut self.bib.blocks[at] {
            target.entry_type = entry.entry_type.clone();
            target.key = entry.key.clone();
            target.fields = entry.fields.clone(); // a bare value stays bare
        }
        if let (Some(index), Block::Entry(target)) = (&mut self.index, &self.bib.blocks[at]) {
            index.add(uid, target);
        }
    }

    /// Give the entry of a handle these links (`set_files`).
    pub fn set_files_by_uid(&mut self, uid: u64, files: &[String]) {
        let relative_to = self.relative_to.clone();
        if let Some(at) = self.block_of_uid(uid)
            && let Block::Entry(e) = &mut self.bib.blocks[at]
        {
            set_files(e, files, &relative_to);
        }
    }

    /// The entry of the bibtex as it was loaded that this entry is the one of now: by its key, a
    /// key it had before in the command, or a former key of its `ids` (`_loaded_state`); none for
    /// an entry new since [`Library::mark_loaded`].
    pub fn loaded_entry_of(&self, e: &Entry) -> Option<&Entry> {
        let loaded = self.loaded.as_ref()?;
        let renamed: HashMap<String, String> = self
            .renamed_keys()
            .into_iter()
            .map(|(old, new)| (new.to_lowercase(), old.to_lowercase()))
            .collect();
        let key = e.key.to_lowercase();
        let keys = std::iter::once(key.clone())
            .chain(renamed.get(&key).cloned())
            .chain(entry::ids(e).into_iter().map(|i| i.to_lowercase()));
        keys.into_iter().find_map(|k| {
            loaded
                .entries
                .iter()
                .find(|(_, x)| x.key.to_lowercase() == k)
                .map(|(_, x)| x)
        })
    }

    /// The files of an entry as loaded (`loaded_files`): none for an entry new since, `None`
    /// without [`Library::mark_loaded`].
    pub fn loaded_files(&self, e: &Entry) -> Option<Vec<String>> {
        let loaded = self.loaded.as_ref()?;
        Some(match self.loaded_entry_of(e) {
            Some(old) => entry_files(old, Some(&self.relative_to)),
            None => {
                let _ = loaded;
                Vec::new()
            }
        })
    }

    /// The entries as they were at [`Library::mark_loaded`] (copies), in the order of the library then:
    /// for the report of a command (`loaded_entries`). None without `mark_loaded`.
    pub fn loaded_entries(&self) -> Option<Vec<&Entry>> {
        self.loaded
            .as_ref()
            .map(|l| l.entries.iter().map(|(_, e)| e).collect())
    }

    /// Put `edited` entries in place of the entries of the keys `old` (`replace_entries`:
    /// `biblio.entries = others + edited`): they are new entries for the report of the changes (a key
    /// that changed is a removal and an addition, not a rename). A key of the edited entries that
    /// another entry has is refused, and nothing is changed (papers compares these keys with case).
    pub fn replace_entries(&mut self, old: &[String], edited: Vec<Entry>) -> Result<()> {
        let old_positions: Vec<usize> = old.iter().filter_map(|k| self.position(k)).collect();
        let taken: HashSet<&str> = self
            .bib
            .blocks
            .iter()
            .enumerate()
            .filter(|(i, _)| !old_positions.contains(i))
            .filter_map(|(_, b)| match b {
                Block::Entry(e) => Some(e.key.as_str()),
                _ => None,
            })
            .collect();
        if let Some(e) = edited.iter().find(|e| taken.contains(e.key.as_str())) {
            return Err(Error::exit(format!(
                "{}: another entry of the library has this key -- keeping original entries",
                e.key
            )));
        }
        let mut gone = old_positions;
        gone.sort_unstable();
        gone.dedup();
        for i in gone.into_iter().rev() {
            self.bib.blocks.remove(i);
            self.uids.remove(i);
        }
        self.index = None;
        for e in edited {
            self.push_entry(e);
        }
        Ok(())
    }

    /// The entries that are new since [`Library::mark_loaded`] (neither their key nor a former key
    /// was there) and those that changed (an entry whose key changed, and has its former key in its
    /// `ids`, is modified): their keys (`changed_entries`). Removed entries are in neither. None
    /// without `mark_loaded`.
    pub fn changed_entries(&self) -> (Vec<String>, Vec<String>) {
        let Some(loaded) = &self.loaded else {
            return (Vec::new(), Vec::new());
        };
        let state = |e: &Entry| {
            let mut fields: Vec<(&str, &str)> = e
                .fields
                .iter()
                .map(|f| (f.key.as_str(), f.value.as_str()))
                .collect();
            fields.sort();
            (
                e.entry_type.to_lowercase(),
                fields
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect::<Vec<_>>(),
            )
        };
        let by_key: HashMap<String, (String, Vec<(String, String)>)> = loaded
            .entries
            .iter()
            .map(|(_, e)| (e.key.to_lowercase(), state(e)))
            .collect();
        let renamed: HashMap<String, String> = self
            .renamed_keys()
            .into_iter()
            .map(|(old, new)| (new.to_lowercase(), old.to_lowercase()))
            .collect();
        let (mut new, mut modified) = (Vec::new(), Vec::new());
        for e in self.entries() {
            let former = entry::ids(e).into_iter().map(|i| i.to_lowercase());
            let key = e.key.to_lowercase();
            let candidates = std::iter::once(key.clone())
                .chain(renamed.get(&key).cloned())
                .chain(former);
            match candidates.into_iter().find_map(|k| by_key.get(&k)) {
                None => new.push(e.key.clone()),
                Some(s) if *s != state(e) => modified.push(e.key.clone()),
                Some(_) => {}
            }
        }
        (new, modified)
    }

    /// The entries are as loaded (the same keys, types and fields), nothing changed at load and no
    /// pair was added to `distinct.json` (`unchanged`); false without [`Library::mark_loaded`].
    pub fn unchanged(&self) -> bool {
        let Some(loaded) = &self.loaded else {
            return false;
        };
        if self.load_changes || self.distinct.changed || loaded.entries.len() != self.len() {
            return false;
        }
        let exact = |e: &Entry| {
            let mut fields: Vec<(String, String)> = e
                .fields
                .iter()
                .map(|f| (f.key.clone(), f.value.clone()))
                .collect();
            fields.sort();
            (e.entry_type.clone(), e.key.clone(), fields)
        };
        let mut before: Vec<_> = loaded.entries.iter().map(|(_, e)| exact(e)).collect();
        let mut now: Vec<_> = self.entries().map(exact).collect();
        before.sort();
        now.sort();
        before == now
    }

    /// Merge the entries of the library that are the same paper (`check_duplicates`). The target
    /// of each group is its entry with most fields; it keeps its key, and the keys of the others
    /// go to its `ids`. The merged entries are removed. Return what was merged, `(target key, keys removed)`.
    /// With `skip` nothing is done; with `error`, a group is an error.
    pub fn check_duplicates(
        &mut self,
        resolver: &mut dyn Resolve,
    ) -> Result<Vec<(String, Vec<String>)>> {
        self.check_duplicates_with(resolver, &mut |_, _| {})
    }

    /// [`Library::check_duplicates`], telling `merged(key of the target now, keys removed)` as
    /// each group is merged (`papers check` prints a line then, between the questions of `ask`).
    pub fn check_duplicates_with(
        &mut self,
        resolver: &mut dyn Resolve,
        merged_group: &mut dyn FnMut(&str, &[String]),
    ) -> Result<Vec<(String, Vec<String>)>> {
        if resolver.mode() == OnDuplicate::Update {
            return Err(Error::other(
                "--on-duplicate update is not available for check (no side is new)",
            ));
        }
        let entries: Vec<Entry> = self.entries().cloned().collect();
        let refs: Vec<&Entry> = entries.iter().collect();
        let groups = duplicate_groups(&refs, Some(&self.distinct));
        log::info(&format!("{} group(s) of duplicates", groups.len()));
        if resolver.mode() == OnDuplicate::Skip {
            return Ok(Vec::new());
        }
        if resolver.mode() == OnDuplicate::Error && !groups.is_empty() {
            let lines: Vec<String> = groups
                .iter()
                .map(|g| {
                    let group: Vec<Entry> = g.entries.iter().map(|&i| entries[i].clone()).collect();
                    let merge = Merge::new(&group, Some(&self.relative_to), true);
                    Resolver::describe(
                        &merge,
                        Context::Check,
                        &crate::identity::Verdict {
                            kind: g.kind,
                            reason: g.reason.clone(),
                        },
                    )
                })
                .collect();
            return Err(DuplicateError(format!(
                "{}\n{}",
                lines.join("\n"),
                Resolver::option_hint(Context::Check)
            ))
            .into());
        }
        let mut merged = Vec::new();
        for g in &groups {
            if resolver.quit() {
                break;
            }
            let group: Vec<Entry> = g.entries.iter().map(|&i| entries[i].clone()).collect();
            let merge = Merge::new(&group, Some(&self.relative_to), true);
            let decision = if g.kind == Kind::Same && merge.is_exact() {
                let result = merge.result();
                Decision::Merge(Box::new(merge), result)
            } else {
                resolver.decide(
                    merge,
                    Context::Check,
                    &crate::identity::Verdict {
                        kind: g.kind,
                        reason: g.reason.clone(),
                    },
                )?
            };
            match decision {
                Decision::Merge(merge, result) => {
                    let target_key = merge.key().to_string();
                    let others: Vec<String> =
                        merge.others().iter().map(|e| e.key.clone()).collect();
                    // the target is replaced in place by the merged entry, the others are removed
                    let at = self.position_exact(&target_key);
                    if let Some(at) = at
                        && let Block::Entry(target) = &mut self.bib.blocks[at]
                    {
                        target.entry_type = result.entry_type.clone();
                        target.key = result.key.clone();
                        target.fields = result.fields.clone(); // a bare value stays bare
                    }
                    for k in &others {
                        if let Some(i) = self.position_exact(k) {
                            self.uids.remove(i);
                            self.bib.blocks.remove(i);
                        }
                        if k.to_lowercase() != result.key.to_lowercase() {
                            // a crossref to the entry merged into the target
                            self.rekey_references(k, &result.key);
                        }
                    }
                    self.index = None;
                    merged_group(&result.key, &others);
                    merged.push((target_key, others));
                }
                Decision::Distinct => {
                    for (i, a) in group.iter().enumerate() {
                        for b in &group[i + 1..] {
                            self.distinct.add(a, b);
                        }
                    }
                }
                Decision::Skip => {}
            }
        }
        Ok(merged)
    }

    /// The block of the entry with exactly this key.
    fn position_exact(&self, key: &str) -> Option<usize> {
        self.bib
            .blocks
            .iter()
            .position(|b| matches!(b, Block::Entry(e) if e.key == key))
    }

    // saving
    // ------

    /// The text of the bibtex on disk, if it is the text that this library read or last wrote
    /// (the size and the modification time first); `BibtexChanged` if another program wrote it since.
    fn current_text(&self) -> std::result::Result<Option<String>, SaveError> {
        let path = realpath(&self.bibtex);
        let read_text = || {
            std::fs::read(&path)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .map(|t| crate::bib::universal_newlines(crate::bib::without_bom(&t)))
        };
        let Some(state) = self.read_state.as_ref().filter(|s| s.path == path) else {
            return Ok(read_text());
        };
        let meta = std::fs::metadata(&path).map_err(|e| SaveError::Error(io_error(&path, &e)))?;
        if meta.len() == state.size && mtime_ns(&meta) == state.mtime_ns {
            return Ok(Some(state.text.clone()));
        }
        let text = read_text();
        if text.as_deref() != Some(state.text.as_str()) {
            return Err(SaveError::BibtexChanged(format!(
                "{} changed since papers read it (another program wrote it meanwhile): papers did not write it, and put back the files that the command moved, copied or deleted. Run the command again.",
                self.bibtex
            )));
        }
        Ok(text)
    }

    /// Order the blocks as a save does: by type and key, the comments staying with the block below them.
    pub fn sort(&mut self) {
        let order = self.bib.sort_order();
        let mut blocks: Vec<Option<Block>> = std::mem::take(&mut self.bib.blocks)
            .into_iter()
            .map(Some)
            .collect();
        let uids = std::mem::take(&mut self.uids);
        self.bib.blocks = order.iter().map(|&i| blocks[i].take().unwrap()).collect();
        self.uids = order.iter().map(|&i| uids[i]).collect();
    }

    /// Save the library (`papers.library.savebib` with `Biblio.save`).
    ///
    /// A library loaded by [`Library::open`] that did not change since ([`Library::unchanged`]) is
    /// not written, unless `other_changes` (e.g. the records of a pool changed, which the history
    /// has too). The `crossref` fields follow the keys that were renamed. The text on disk, if it
    /// is the same as the text to write, is left alone (its modification time is kept); otherwise
    /// the file is replaced in one step, keeping its permissions. A bibtex that another program
    /// wrote since the library read it is not written ([`SaveError::BibtexChanged`]). `distinct.json`
    /// is written if a pair was added.
    ///
    /// What the history records is in the result ([`Saved`]): it is for the caller to record
    /// it after the save, and a failure of the history does not undo the save.
    pub fn save(&mut self, other_changes: bool) -> std::result::Result<Saved, SaveError> {
        let before = self.read_state.as_ref().map(|s| s.text.clone());
        let mut saved = Saved {
            path: self.bibtex.clone(),
            status: SaveStatus::Unchanged,
            before,
            after: String::new(),
            renamed_keys: Vec::new(),
            file_ops: Vec::new(),
            distinct_written: false,
        };
        if self.dry_run {
            println!("DRYRUN: {} not saved", self.bibtex);
            saved.status = SaveStatus::DryRun;
            return Ok(saved);
        }
        if !other_changes && self.unchanged() {
            log::info(&format!("{}: nothing changed, not saved", self.bibtex));
            saved.after = saved.before.clone().unwrap_or_default();
            return Ok(saved);
        }
        log::info(&format!("Saving {}", self.bibtex));
        saved.renamed_keys = self.renamed_keys();
        for (old, new) in saved.renamed_keys.clone() {
            self.rekey_references(&old, &new);
        }
        if !matches!(self.relative_to.as_str(), "/" | "")
            && realpath(&self.relative_to) != realpath(&dirname(&abspath(&self.bibtex)))
        {
            log::warning(
                "Saving bibtex file with relative paths may break links. Consider using `Biblio.update_file_path(Path(bibtex).parent)` before.",
            );
        }
        self.sort();
        let text = self.bib.format();
        if exists(&self.bibtex) {
            if self.current_text()?.as_deref() == Some(text.as_str()) {
                log::debug(&format!("{}: unchanged, not written", self.bibtex));
                saved.status = SaveStatus::Identical;
            } else {
                write_atomic(&self.bibtex, &text)?;
                saved.status = SaveStatus::Written;
            }
        } else {
            write_atomic(&self.bibtex, &text)?;
            saved.status = SaveStatus::Written;
        }
        self.remember(&text);
        saved.after = text;
        if self.distinct.changed {
            if self.distinct.path.is_none() {
                let mut ondisk = Distinct::for_bibtex(&self.bibtex);
                ondisk.update(&self.distinct);
                self.distinct = ondisk;
            }
            self.distinct
                .save()
                .map_err(|e| SaveError::Error(io_error(&self.bibtex, &e)))?;
            saved.distinct_written = true;
        }
        saved.file_ops = std::mem::take(&mut self.ops);
        Ok(saved)
    }

    /// The directory the links of the library were relative to when it was loaded.
    pub fn loaded_relative_to(&self) -> Option<&str> {
        self.loaded.as_ref().map(|l| l.relative_to.as_str())
    }
}

fn mtime_ns(meta: &std::fs::Metadata) -> i128 {
    use std::os::unix::fs::MetadataExt;
    meta.mtime() as i128 * 1_000_000_000 + meta.mtime_nsec() as i128
}
