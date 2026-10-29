//! `.papers/origin/` of a library (`papers.pools.Origin`): one bibtex file per pool, with the
//! pool id and the time of the last exchange in its first comments, and the records of the
//! entries exchanged with the pool (the base of the next three-way merge, under the library's
//! key, with the pool's key and the time). Spec `exchange.md` section 3.
//!
//! ```text
//! @comment{papers-pool 7e5754a0ccd246968e910bcd2a16dcf2}
//!
//! @comment{papers-pulled 1793189510108569698}
//!
//! @comment{papers-pool-path ~/papers/pool}
//!
//! @article{Smith2020,
//!  author = {Smith, John},
//!  file = {:../pool/files/smith_2020.pdf:pdf},
//!  papers-pool-key = {smith2020},
//!  papers-time = {2026-10-28T13:11:50+01:00},
//!  title = {A title}
//! }
//! ```
//!
//! The pool id is the identifier of the pool's library (its `id`); a pool of a library without one,
//! and the records of earlier versions, have the path of the pool as id (a *path id*: the records
//! move to the identifier, see [`Origin::rename_pool`]). The files are read in sorted name order,
//! and the files of one pool id are merged; the pool is named by the `papers-pool` comment, not by
//! the file name. When a pool is written, the other files of its id are removed. The legacy file
//! `.papers/origin.bib` (earlier v3 builds) is read after them, and split into one file per pool.

use std::collections::{BTreeMap, BTreeSet};

use crate::bib::{Bib, Block, Entry, Field, read_file};
use crate::config::paths::{
    basename, dirname, exists, expanduser, isdir, isfile, join, normpath, realpath, short_home,
};
use crate::config::write_atomic;
use crate::entry::{self, val};
use crate::error::Result;
use crate::log;
use crate::names::slugify;

/// The folder of the records, in the `.papers` folder of the bibtex.
pub const ORIGIN_DIR: &str = ".papers/origin";
/// The one file of earlier v3 builds, next to the folder.
pub const LEGACY_ORIGIN_FILE: &str = ".papers/origin.bib";
/// In the legacy file only: the pool of a record.
pub const POOL_FIELD: &str = "papers-pool";
pub const POOL_KEY_FIELD: &str = "papers-pool-key";
pub const TIME_FIELD: &str = "papers-time";
/// The file links of a record (a shallow library: those of the pool entry after the exchange).
pub const FILE_FIELD: &str = "file";
/// Written by development builds: ignored when read, removed when the links of the record are set.
const DEV_FILES_FIELD: &str = "papers-files";
const ORIGIN_FIELDS: [&str; 4] = [POOL_FIELD, POOL_KEY_FIELD, TIME_FIELD, DEV_FILES_FIELD];
const POOL_COMMENT: &str = "papers-pool";
const PULLED_COMMENT: &str = "papers-pulled";
/// Where the pool was at the last exchange (for the reader, and the name of the file).
const PATH_COMMENT: &str = "papers-pool-path";
/// `<mtime> <pool id>`, in the legacy file.
const LEGACY_PULL_COMMENT: &str = "papers-pull";

/// The SHA-1 of a text, as hexadecimal (the name of an origin file only: no security).
pub fn sha1_hex(data: &[u8]) -> String {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in message.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// A pool id of an earlier version: the pool's path, not a library identifier (32 lower-case
/// hexadecimal digits).
pub fn is_path_id(id: &str) -> bool {
    !is_library_id(id)
}

/// A library identifier: 32 lower-case hexadecimal digits.
pub fn is_library_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The origin file of a pool: the name of the pool folder (`path`: where the pool is, for a pool id
/// that is a library identifier; else the pool id itself) and the start of the sha1 of the pool id
/// (two pools in folders of the same name get different files).
///
/// ```
/// use papers_rs::pools::origin::origin_file_name;
/// assert_eq!(origin_file_name("../pool", None), "pool-615fe0.bib");
/// assert_eq!(origin_file_name("../pool/", None), "pool-b6adbc.bib");
/// assert_eq!(origin_file_name("~/Documents/papers", None), "papers-e311da.bib");
/// assert_eq!(origin_file_name("../My Papers", None), "my-papers-94036c.bib");
/// let id = "7e5754a0ccd246968e910bcd2a16dcf2";
/// assert_eq!(origin_file_name(id, None), "pool-569371.bib");
/// assert_eq!(origin_file_name(id, Some("/home/me/papers/shared-with-bob")), "shared-with-bob-569371.bib");
/// ```
pub fn origin_file_name(pool: &str, path: Option<&str>) -> String {
    let folder = match path {
        Some(path) => path,
        None if is_path_id(pool) => pool,
        None => "",
    };
    let name = if folder.is_empty() {
        String::new()
    } else {
        slugify(&basename(&normpath(folder)))
    };
    let name = if name.is_empty() {
        "pool".to_string()
    } else {
        name
    };
    format!("{name}-{}.bib", &sha1_hex(pool.as_bytes())[..6])
}

/// The origin record of a library entry for a pool: `entry` is the base version (under the
/// library key, without the `papers-*` fields and the links). `files`: the `file` field of the
/// record, in a shallow library the links of the pool entry after the last exchange (as written
/// in the library's bibtex), or none if not recorded.
#[derive(Debug, Clone)]
pub struct Record {
    pub key: String,
    pub pool: String,
    pub pool_key: String,
    pub time: String,
    pub entry: Entry,
    pub files: Option<String>,
}

/// `datetime.now().astimezone().isoformat(timespec='seconds')`.
fn now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: localtime_r writes the broken-down time in `tm`
    unsafe { libc::localtime_r(&secs, &mut tm) };
    let offset = tm.tm_gmtoff;
    let sign = if offset < 0 { '-' } else { '+' };
    let offset = offset.abs();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{sign}{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        offset / 3600,
        offset % 3600 / 60
    )
}

fn strip_fields(e: &Entry, names: &[&str]) -> Entry {
    let mut e = e.clone();
    e.source = None;
    e.fields.retain(|f| !names.contains(&f.key.as_str()));
    e
}

/// Same entry type and fields (values compared as written, the order aside).
fn same_fields(a: &Entry, b: &Entry) -> bool {
    let fields = |e: &Entry| {
        let mut f: Vec<(String, String)> = e
            .fields
            .iter()
            .map(|f| (f.key.clone(), f.value.clone()))
            .collect();
        f.sort();
        (e.entry_type.to_lowercase(), f)
    };
    fields(a) == fields(b)
}

/// Not part of the base entry: the fields of the record, the file links, and the `ids` (former keys)
/// of the side that sent the entry, which are not exchanged.
fn record_extra() -> Vec<&'static str> {
    ORIGIN_FIELDS
        .iter()
        .copied()
        .chain([FILE_FIELD, "ids"])
        .collect::<Vec<_>>()
}

type Records = BTreeMap<String, Entry>;

/// The records of a library, by pool (a pool id) and lower-case library key.
#[derive(Debug, Clone, Default)]
pub struct Origin {
    pub dir: Option<String>,
    /// pool id, in the order the pools were first met; the records of each, by lower-case key
    entries: Vec<(String, Records)>,
    /// pool id -> modification time (ns) of the pool bibtex at the last exchange
    pulls: BTreeMap<String, i128>,
    /// the pools whose file is to be written
    changed: BTreeSet<String>,
    /// the pools that have records when loaded
    known: BTreeSet<String>,
    /// pool id -> where the pool was (`papers-pool-path`, with `~` for the home directory)
    paths: BTreeMap<String, String>,
    /// pool id -> the files its records were read from, in the order read
    read_from: BTreeMap<String, Vec<String>>,
    /// files to remove at the save (the records of a pool that moved to another id)
    stale: BTreeSet<String>,
    dry_run: bool,
}

fn parse_file(path: &str) -> Option<Bib> {
    match read_file(std::path::Path::new(path)) {
        Ok(text) => Some(Bib::parse(&text)),
        Err(error) => {
            log::warning(&format!(
                "{path}: could not be read ({}); ignored",
                crate::config::machine::io_message(&error)
            ));
            None
        }
    }
}

/// The words of the comments of a bibtex: a first word and the rest, stripped.
fn comment_words(bib: &Bib, parts: usize) -> Vec<Vec<String>> {
    bib.blocks
        .iter()
        .filter_map(|b| match b {
            Block::Comment(c) => Some(c),
            _ => None,
        })
        .map(|c| {
            let mut words: Vec<String> = Vec::new();
            let mut rest = c.trim_start();
            while words.len() + 1 < parts && !rest.is_empty() {
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                words.push(rest[..end].to_string());
                rest = rest[end..].trim_start();
            }
            if !rest.is_empty() || words.is_empty() {
                words.push(rest.trim_end().to_string());
            }
            words
        })
        .filter(|w| !w.is_empty() && !w[0].is_empty())
        .collect()
}

fn digits(text: &str) -> Option<i128> {
    (!text.is_empty() && text.chars().all(|c| c.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

impl Origin {
    /// The records of the library in the folder `libdir` (`Config::libdir`): its `.papers/origin/`.
    pub fn for_library(libdir: &str, dry_run: bool) -> Origin {
        Origin::load(&join(libdir, ORIGIN_DIR), dry_run)
    }

    /// Read the folder (and the legacy file next to it, which is split into one file per
    /// pool, not with `dry_run`).
    pub fn load(dir: &str, dry_run: bool) -> Origin {
        let mut origin = Origin {
            dir: Some(dir.to_string()),
            dry_run,
            ..Origin::default()
        };
        let mut names: Vec<String> = if isdir(dir) {
            std::fs::read_dir(dir)
                .map(|d| {
                    d.flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        names.sort();
        for name in names {
            if name.ends_with(".bib") {
                origin.read(&join(dir, &name));
            }
        }
        let legacy = join(&dirname(dir), &basename(LEGACY_ORIGIN_FILE));
        if exists(&legacy) {
            origin.migrate(&legacy);
        }
        origin.known = origin
            .entries
            .iter()
            .filter(|(_, r)| !r.is_empty())
            .map(|(p, _)| p.clone())
            .collect();
        origin
    }

    fn records_mut(&mut self, pool: &str) -> &mut Records {
        if let Some(i) = self.entries.iter().position(|(p, _)| p == pool) {
            return &mut self.entries[i].1;
        }
        self.entries.push((pool.to_string(), Records::new()));
        &mut self.entries.last_mut().expect("just pushed").1
    }

    fn records_of(&self, pool: &str) -> Option<&Records> {
        self.entries.iter().find(|(p, _)| p == pool).map(|(_, r)| r)
    }

    fn read(&mut self, path: &str) {
        let Some(bib) = parse_file(path) else { return };
        let words = comment_words(&bib, 2);
        let pool = words
            .iter()
            .find(|w| w.len() == 2 && w[0] == POOL_COMMENT)
            .map(|w| w[1].trim().to_string());
        let Some(pool) = pool else {
            log::warning(&format!(
                "{path}: no `{POOL_COMMENT}` comment naming the pool; ignored"
            ));
            return;
        };
        let pulled = words
            .iter()
            .find(|w| w.len() == 2 && w[0] == PULLED_COMMENT)
            .map(|w| w[1].trim().to_string())
            .unwrap_or_default();
        if let Some(n) = digits(&pulled) {
            self.pulls.insert(pool.clone(), n);
        }
        let place = words
            .iter()
            .find(|w| w.len() == 2 && w[0] == PATH_COMMENT)
            .map(|w| w[1].trim().to_string())
            .filter(|w| !w.is_empty());
        if let Some(place) = place {
            self.paths.insert(pool.clone(), place);
        }
        // several files of one pool id are merged: the last read wins
        self.read_from
            .entry(pool.clone())
            .or_default()
            .push(path.to_string());
        let records = self.records_mut(&pool);
        for e in bib.entries() {
            records.insert(e.key.to_lowercase(), e.clone());
        }
    }

    /// Split the single origin file of earlier v3 builds into one file per pool, and remove it.
    fn migrate(&mut self, legacy: &str) {
        let Some(bib) = parse_file(legacy) else {
            return;
        };
        for words in comment_words(&bib, 3) {
            if words.len() == 3
                && words[0] == LEGACY_PULL_COMMENT
                && let Some(n) = digits(&words[1])
            {
                self.pulls.entry(words[2].clone()).or_insert(n);
            }
        }
        for e in bib.entries() {
            let pool = val(e, POOL_FIELD).to_string();
            self.records_mut(&pool)
                .entry(e.key.to_lowercase())
                .or_insert_with(|| strip_fields(e, &[POOL_FIELD]));
        }
        let pools: Vec<String> = self
            .entries
            .iter()
            .map(|(p, _)| p.clone())
            .chain(self.pulls.keys().cloned())
            .collect();
        self.changed.extend(pools);
        if self.dry_run {
            return;
        }
        if self.save().is_err() {
            return;
        }
        if std::fs::remove_file(legacy).is_ok() {
            log::info(&format!(
                "{legacy}: split into one file per pool in {}",
                self.dir.clone().unwrap_or_default()
            ));
        }
    }

    /// The number of records, of every pool.
    pub fn len(&self) -> usize {
        self.entries.iter().map(|(_, r)| r.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The pool ids that have records or a time.
    pub fn pool_ids(&self) -> BTreeSet<String> {
        self.entries
            .iter()
            .map(|(p, _)| p.clone())
            .chain(self.pulls.keys().cloned())
            .collect()
    }

    /// A pool is to be written.
    pub fn changed(&self) -> bool {
        !self.changed.is_empty()
    }

    fn record(&self, pool: &str, e: &Entry) -> Record {
        let extra = record_extra();
        Record {
            key: e.key.clone(),
            pool: pool.to_string(),
            pool_key: val(e, POOL_KEY_FIELD).to_string(),
            time: val(e, TIME_FIELD).to_string(),
            entry: strip_fields(e, &extra),
            files: e.get(FILE_FIELD).map(str::to_string),
        }
    }

    /// The record of a library entry (by key, or a former key of its `ids`) for `pool`
    /// (none: any pool), or none.
    pub fn get(&self, entry: &Entry, pool: Option<&str>) -> Option<Record> {
        let mut keys = vec![entry.key.clone()];
        keys.extend(entry::ids(entry));
        for (id, records) in &self.entries {
            if pool.is_some_and(|p| p != id) {
                continue;
            }
            for key in &keys {
                if !key.is_empty()
                    && let Some(e) = records.get(&key.to_lowercase())
                {
                    return Some(self.record(id, e));
                }
            }
        }
        None
    }

    /// The records of a pool (none: all), in the order of their keys.
    pub fn records(&self, pool: Option<&str>) -> Vec<Record> {
        self.entries
            .iter()
            .filter(|(id, _)| pool.is_none_or(|p| p == id))
            .flat_map(|(id, records)| records.values().map(move |e| self.record(id, e)))
            .collect()
    }

    /// The pool key (lower case) of the record of each library key (lower case) of a pool, in
    /// the order of the records.
    pub fn pool_keys(&self, pool: &str) -> Vec<(String, String)> {
        self.records_of(pool)
            .map(|r| {
                r.iter()
                    .map(|(k, e)| (k.clone(), val(e, POOL_KEY_FIELD).to_lowercase()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Record the base version of an entry for a pool, with the key of the pool entry (the
    /// record does not change if it is the same as before, its time included), and `files`,
    /// the file links of the pool entry, in the `file` field of the record (none: no `file`
    /// field).
    pub fn set(&mut self, key: &str, pool: &str, pool_entry: &Entry, files: Option<&str>) {
        let extra = record_extra();
        let new = strip_fields(pool_entry, &extra);
        let records = self.records_mut(pool);
        if let Some(old) = records.get(&key.to_lowercase())
            && val(old, POOL_KEY_FIELD) == pool_entry.key
            && same_fields(&strip_fields(old, &extra), &new)
        {
            self.set_files(key, pool, files);
            return;
        }
        let mut new = new;
        new.key = key.to_string();
        if let Some(files) = files {
            new.fields.push(Field::new(FILE_FIELD, files));
        }
        new.fields
            .push(Field::new(POOL_KEY_FIELD, pool_entry.key.clone()));
        new.fields.push(Field::new(TIME_FIELD, now()));
        self.records_mut(pool).insert(key.to_lowercase(), new);
        self.changed.insert(pool.to_string());
    }

    /// Set the file links of the pool entry in the record of an entry, if it has one (none:
    /// remove them).
    pub fn set_files(&mut self, key: &str, pool: &str, files: Option<&str>) {
        let Some(records) = self.entries.iter_mut().find(|(p, _)| p == pool) else {
            return;
        };
        let Some(old) = records.1.get_mut(&key.to_lowercase()) else {
            return;
        };
        if old.get(FILE_FIELD) == files && old.get(DEV_FILES_FIELD).is_none() {
            return;
        }
        old.fields
            .retain(|f| f.key != FILE_FIELD && f.key != DEV_FILES_FIELD);
        if let Some(files) = files {
            old.fields.push(Field::new(FILE_FIELD, files));
        }
        self.changed.insert(pool.to_string());
    }

    pub fn remove(&mut self, key: &str, pool: &str) {
        let removed = self
            .entries
            .iter_mut()
            .find(|(p, _)| p == pool)
            .and_then(|(_, r)| r.remove(&key.to_lowercase()));
        if removed.is_some() {
            self.changed.insert(pool.to_string());
        }
    }

    /// The records of a library entry whose key changed from `old` to `new` (a key that no entry had),
    /// in every pool. A record left under `new` by a removed entry is another paper's: replaced, or
    /// dropped where the entry has no record.
    pub fn rekey(&mut self, old: &str, new: &str) {
        for (pool, records) in &mut self.entries {
            if let Some(mut record) = records.remove(&old.to_lowercase()) {
                record.key = new.to_string();
                records.insert(new.to_lowercase(), record);
                self.changed.insert(pool.clone());
            } else if records.remove(&new.to_lowercase()).is_some() {
                self.changed.insert(pool.clone());
            }
        }
    }

    /// The origin file of the pool id `pool` (see [`origin_file_name`]): the file its records were
    /// read from (the last one), else the file named after the pool folder; none without origin folder.
    pub fn file_of(&self, pool: &str) -> Option<String> {
        let dir = self.dir.as_deref()?;
        if let Some(file) = self.read_from.get(pool).and_then(|f| f.last()) {
            return Some(file.clone());
        }
        Some(join(dir, &self.name_of(pool)))
    }

    fn name_of(&self, pool: &str) -> String {
        let place = self.paths.get(pool).map(|p| expanduser(p));
        origin_file_name(pool, place.as_deref())
    }

    /// Where the pool `pool` was (`papers-pool-path`, with `~` for the home directory), if recorded.
    pub fn path_of(&self, pool: &str) -> Option<&str> {
        self.paths.get(pool).map(String::as_str)
    }

    /// Where the pool `pool` (a library identifier) is: its origin file is named after that folder, and
    /// says it (`papers-pool-path`).
    pub fn locate(&mut self, pool: &str, path: &str) {
        let place = short_home(path);
        if self.paths.get(pool) != Some(&place) {
            self.paths.insert(pool.to_string(), place);
            if self.records_of(pool).is_some() || self.pulls.contains_key(pool) {
                self.changed.insert(pool.to_string()); // written again, under the name of the folder
            }
        }
    }

    /// The records of the pool `old` move to the id `new` (the library identifier of the pool, see
    /// [`super::Pools::id_of`]), where they do not have a record of the key already; `path`: where the
    /// pool is. The file of `old` is removed at the save.
    pub fn rename_pool(&mut self, old: &str, new: &str, path: Option<&str>) {
        if self.records_of(old).is_none() && !self.pulls.contains_key(old) {
            return;
        }
        let moved = self
            .entries
            .iter()
            .position(|(p, _)| p == old)
            .map(|i| self.entries.remove(i).1)
            .unwrap_or_default();
        let target = self.records_mut(new);
        for (key, record) in moved {
            target.entry(key).or_insert(record);
        }
        if let Some(n) = self.pulls.remove(old) {
            self.pulls.entry(new.to_string()).or_insert(n);
        }
        if self.known.contains(old) {
            self.known.insert(new.to_string());
        }
        if let Some(path) = path {
            self.paths.insert(new.to_string(), short_home(path));
        }
        if let Some(files) = self.read_from.remove(old) {
            self.stale.extend(files);
        }
        self.changed.remove(old);
        self.changed.insert(new.to_string());
        log::info(&format!(
            "origin records of {old}: now under the identifier of the pool library, {new}"
        ));
    }

    /// The modification time (ns) of the pool's bibtex at the last exchange, if any.
    pub fn pulled(&self, pool: &str) -> Option<i128> {
        self.pulls.get(pool).copied()
    }

    /// No exchange with the pool before this command: no exchange time, and no record when loaded.
    pub fn first_exchange(&self, pool: &str) -> bool {
        self.pulled(pool).is_none() && !self.known.contains(pool)
    }

    pub fn set_pulled(&mut self, pool: &str, mtime: Option<i128>) {
        if let Some(mtime) = mtime
            && self.pulls.get(pool) != Some(&mtime)
        {
            self.pulls.insert(pool.to_string(), mtime);
            self.changed.insert(pool.to_string());
        }
    }

    /// The text of the file of a pool.
    fn text(&self, pool: &str) -> String {
        let mut blocks = vec![Block::Comment(format!("{POOL_COMMENT} {pool}"))];
        if let Some(n) = self.pulls.get(pool) {
            blocks.push(Block::Comment(format!("{PULLED_COMMENT} {n}")));
        }
        if let Some(place) = self.paths.get(pool) {
            blocks.push(Block::Comment(format!("{PATH_COMMENT} {place}")));
        }
        if let Some(records) = self.records_of(pool) {
            blocks.extend(records.values().cloned().map(Block::Entry));
        }
        Bib { blocks }.format()
    }

    /// Write the files of the pools that changed (atomically, and only if the text differs from
    /// the file on disk). With a dry run, nothing.
    pub fn save(&mut self) -> Result<()> {
        let Some(dir) = self.dir.clone() else {
            return Ok(());
        };
        if self.changed.is_empty() {
            return Ok(());
        }
        if self.dry_run {
            log::println(&format!("DRYRUN: {dir} not saved"));
            return Ok(());
        }
        std::fs::create_dir_all(&dir)?;
        let mut written: BTreeSet<String> = BTreeSet::new();
        for pool in std::mem::take(&mut self.changed) {
            let text = self.text(&pool);
            let path = join(&dir, &self.name_of(&pool));
            let on_disk = if isfile(&path) {
                read_file(std::path::Path::new(&path)).ok()
            } else {
                None
            };
            if on_disk.as_deref() != Some(text.as_str()) {
                write_atomic(&path, &text)?;
            }
            written.insert(realpath(&path));
            // the files of an earlier name, and the other files of the pool id
            for file in self.read_from.get(&pool).into_iter().flatten() {
                if realpath(file) != realpath(&path) {
                    self.stale.insert(file.clone());
                }
            }
            self.read_from.insert(pool, vec![path]);
        }
        for file in std::mem::take(&mut self.stale) {
            if !written.contains(&realpath(&file)) && isfile(&file) {
                let _ = std::fs::remove_file(&file);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, fields: &[(&str, &str)]) -> Entry {
        let mut e = Entry::new("article", key);
        for (k, v) in fields {
            e.set(k, *v);
        }
        e
    }

    #[test]
    fn sha1_vectors() {
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha1_hex(&[b'a'; 1000]),
            "291e9a6c66994949b57ba5e650361e98fc36b1ba"
        );
    }

    #[test]
    fn records_are_set_and_not_rewritten_when_equal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("origin").to_string_lossy().into_owned();
        let mut origin = Origin::load(&path, false);
        let pool_entry = entry("smith_2020", &[("title", "T"), ("year", "2020")]);
        origin.set("Smith2020", "../pool", &pool_entry, Some(""));
        origin.set_pulled("../pool", Some(1793189510108569698));
        origin.save().unwrap();
        let text =
            std::fs::read_to_string(join(&path, &origin_file_name("../pool", None))).unwrap();
        assert!(text.starts_with(
            "@comment{papers-pool ../pool}\n\n@comment{papers-pulled 1793189510108569698}\n\n@article{Smith2020,\n file = {},\n papers-pool-key = {smith_2020},\n papers-time = {"
        ), "{text}");
        // read back
        let mut again = Origin::load(&path, false);
        let record = again
            .get(&entry("smith2020", &[]), Some("../pool"))
            .unwrap();
        assert_eq!(record.pool_key, "smith_2020");
        assert_eq!(record.files.as_deref(), Some(""));
        assert_eq!(record.entry.get("title"), Some("T"));
        assert_eq!(again.pulled("../pool"), Some(1793189510108569698));
        // the same entry: the record (its time) does not change
        let time = record.time.clone();
        again.set("Smith2020", "../pool", &pool_entry, Some(""));
        assert!(!again.changed());
        assert_eq!(
            again.get(&entry("Smith2020", &[]), None).unwrap().time,
            time
        );
        // the links only
        again.set("Smith2020", "../pool", &pool_entry, None);
        assert!(again.changed());
        assert!(
            again
                .get(&entry("Smith2020", &[]), None)
                .unwrap()
                .files
                .is_none()
        );
        // a rekey
        again.rekey("Smith2020", "Smith2021");
        assert!(again.get(&entry("Smith2020", &[]), None).is_none());
        assert!(
            again
                .get(&entry("x", &[("ids", "smith2021")]), None)
                .is_some()
        );
    }

    #[test]
    fn the_legacy_file_is_split_into_one_file_per_pool() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        std::fs::create_dir_all(join(&root, ".papers")).unwrap();
        std::fs::write(
            join(&root, LEGACY_ORIGIN_FILE),
            "@comment{papers-pull 123 ../pool}\n\n@article{a,\n title = {T},\n papers-pool = {../pool},\n papers-pool-key = {b}\n}\n",
        )
        .unwrap();
        let origin = Origin::for_library(&root, false);
        assert_eq!(origin.pulled("../pool"), Some(123));
        assert!(origin.get(&entry("a", &[]), Some("../pool")).is_some());
        assert!(!exists(&join(&root, LEGACY_ORIGIN_FILE)));
        let text = std::fs::read_to_string(join(&root, ".papers/origin").as_str()).ok();
        assert!(text.is_none()); // a folder
        assert!(isfile(&join(
            &root,
            &format!(".papers/origin/{}", origin_file_name("../pool", None))
        )));
    }
}
