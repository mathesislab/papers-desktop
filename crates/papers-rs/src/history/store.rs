//! The store of the history: the objects (pieces of text, zlib), the file
//! objects (files deleted by the commands), the size and the retention
//! (spec/history.md, sections 2.2 to 2.4 and 9).
//!
//! ```text
//! objects/ab/ab12…          a piece of text, compressed (zlib), named by the sha256 of its bytes
//! file-objects/ab/ab12….pdf a file deleted by a command, named by its sha256 and its extension
//! projects/<id>/            the states and the heads of a library (see `project`)
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use sha2::{Digest, Sha256};

use super::project::{Project, State};
use crate::config::paths::{basename, dirname, expanduser, home, isdir, join};
use crate::error::{Error, Result};

/// An object or a file object younger than this (seconds) is not removed: a
/// command writes its objects before its state.
pub const GRACE: f64 = 3600.0;
/// A piece ends after a segment whose crc32 is a multiple of this.
pub const PIECE_ENTRIES: u32 = 32;
/// A piece holds at most this many segments.
pub const PIECE_MAX: usize = 256;
/// One megabyte, for `history-size`.
pub const MB: u64 = 1024 * 1024;

/// The sha256 of some bytes, as 64 lowercase hexadecimal characters.
pub fn sha256(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The identifier of a new project: a random uuid (version 4) as 32
/// hexadecimal characters.
pub fn new_id() -> String {
    let mut bytes = [0u8; 16];
    let filled = fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .is_ok();
    if !filled {
        use std::hash::{BuildHasher, Hasher, RandomState};
        for chunk in bytes.chunks_mut(8) {
            let mut hasher = RandomState::new().build_hasher();
            hasher.write_u64(now().to_bits());
            chunk.copy_from_slice(&hasher.finish().to_le_bytes()[..chunk.len()]);
        }
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `time.time()`.
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Write bytes through a temporary file (`<path>.<pid>.tmp`) and a rename: a
/// reader never sees half a file.
pub fn write_bytes(path: &str, data: &[u8]) -> Result<()> {
    let dir = dirname(path);
    if !dir.is_empty() {
        fs::create_dir_all(&dir).map_err(|e| Error::other(format!("{dir}: {e}")))?;
    }
    let tmp = format!("{path}.{}.tmp", std::process::id());
    let result = fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(data))
        .and_then(|_| fs::rename(&tmp, path));
    result.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        Error::other(format!("{path}: {e}"))
    })
}

/// Set the modification time of a file to now.
pub fn touch(path: &str) {
    if let Ok(f) = fs::File::open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

fn mtime(meta: &fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// A text cut into pieces: the text is cut before each line that starts with
/// `@` (the segments), and the segments are grouped, a piece ending after a
/// segment of which the crc32 is a multiple of `PIECE_ENTRIES`, or at
/// `PIECE_MAX` segments. Adding or removing an entry thus changes the piece
/// it is in only.
pub fn cut_pieces(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut starts = vec![0usize];
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' && bytes.get(i + 1) == Some(&b'@') {
            starts.push(i + 1);
        }
    }
    let mut pieces = Vec::new();
    let mut from: Option<usize> = None;
    let mut count = 0;
    let mut ends: Vec<usize> = starts[1..].to_vec();
    ends.push(bytes.len());
    for (start, end) in starts.into_iter().zip(ends) {
        if start == end {
            continue; // an empty segment
        }
        let segment = &bytes[start..end];
        let from_at = *from.get_or_insert(start);
        count += 1;
        if count >= PIECE_MAX || crc32fast::hash(segment).is_multiple_of(PIECE_ENTRIES) {
            pieces.push(&text[from_at..end]);
            from = None;
            count = 0;
        }
    }
    if let Some(from_at) = from {
        pieces.push(&text[from_at..]);
    }
    pieces
}

/// The pieces of a text and their checksums, nothing stored (`pieces`).
pub fn piece_digests(text: &str) -> Vec<(String, &str)> {
    cut_pieces(text)
        .into_iter()
        .map(|p| (sha256(p.as_bytes()), p))
        .collect()
}

/// A missing or unreadable piece of text: the history is damaged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingPiece(pub String);

impl std::fmt::Display for MissingPiece {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "piece {} is missing from the history", self.0)
    }
}

/// What a state refers to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ref {
    Piece(String),
    File(String),
}

/// The extension of a name as `os.path.splitext` gives it, with its dot.
pub fn splitext(path: &str) -> &str {
    let base_start = path.rfind('/').map_or(0, |i| i + 1);
    let base = &path[base_start..];
    let lead = base.len() - base.trim_start_matches('.').len();
    match base[lead..].rfind('.') {
        Some(i) => &base[lead + i..],
        None => "",
    }
}

/// What is stored: where, how big, when written.
#[derive(Debug, Clone)]
pub struct Item {
    pub path: String,
    pub size: u64,
    pub mtime: f64,
}

/// A state that the retention drops.
#[derive(Debug, Clone)]
pub struct Dropped {
    pub project: String,
    pub number: u64,
    pub state: State,
}

#[derive(Debug, Clone)]
pub struct Store {
    pub root: String,
}

impl Store {
    pub fn new(root: impl Into<String>) -> Store {
        Store { root: root.into() }
    }

    pub fn objects(&self) -> String {
        join(&self.root, "objects")
    }

    pub fn file_objects(&self) -> String {
        join(&self.root, "file-objects")
    }

    pub fn projects(&self) -> String {
        join(&self.root, "projects")
    }

    /// Whether a store lives in a folder `separate/<id>` (its retention is
    /// counted alone, see `size_limit`).
    pub fn is_separate(&self) -> bool {
        basename(&dirname(&self.root)) == "separate"
    }

    // pieces of text
    // --------------

    fn object(&self, digest: &str) -> String {
        join(&join(&self.objects(), &digest[..2]), digest)
    }

    /// Store a text by pieces; the checksums of the pieces.
    pub fn put_text(&self, text: &str) -> Result<Vec<String>> {
        let mut pieces = Vec::new();
        for piece in cut_pieces(text) {
            let data = piece.as_bytes();
            let digest = sha256(data);
            let path = self.object(&digest);
            if fs::metadata(&path).is_ok() {
                touch(&path); // recent again: not removed before the state that refers to it is written
            } else {
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(6));
                encoder.write_all(data).map_err(Error::from)?;
                write_bytes(&path, &encoder.finish().map_err(Error::from)?)?;
            }
            pieces.push(digest);
        }
        Ok(pieces)
    }

    /// The text of a list of pieces.
    pub fn text(&self, pieces: &[String]) -> std::result::Result<String, MissingPiece> {
        let mut data = Vec::new();
        for digest in pieces {
            let missing = || MissingPiece(digest.clone());
            if digest.len() < 2 || !digest.is_char_boundary(2) {
                return Err(missing());
            }
            let compressed = fs::read(self.object(digest)).map_err(|_| missing())?;
            ZlibDecoder::new(compressed.as_slice())
                .read_to_end(&mut data)
                .map_err(|_| missing())?;
        }
        String::from_utf8(data).map_err(|_| MissingPiece("(not UTF-8)".into()))
    }

    // files deleted by the commands
    // -----------------------------

    /// The stored file of this checksum, if any: a file of its folder whose
    /// name, up to the first dot, is the checksum.
    pub fn file_object(&self, digest: &str) -> Option<String> {
        if digest.len() < 2 || !digest.is_char_boundary(2) {
            return None;
        }
        let folder = join(&self.file_objects(), &digest[..2]);
        let names = fs::read_dir(&folder).ok()?;
        for name in names.flatten() {
            let name = name.file_name().to_string_lossy().into_owned();
            if name.split('.').next() == Some(digest) {
                return Some(join(&folder, &name));
            }
        }
        None
    }

    /// Move a file into the store (named by its checksum `digest`); its path
    /// in the store. A file whose content is stored already is deleted.
    pub fn put_file(&self, path: &str, digest: &str) -> Result<String> {
        if let Some(found) = self.file_object(digest) {
            fs::remove_file(path).map_err(|e| Error::other(format!("{path}: {e}")))?;
            touch(&found);
            return Ok(found);
        }
        let target = join(
            &join(&self.file_objects(), &digest[..2]),
            &format!("{digest}{}", splitext(path)),
        );
        fs::create_dir_all(dirname(&target)).map_err(Error::from)?;
        super::restore::move_path(path, &target)
            .map_err(|e| Error::other(format!("{path}: {e}")))?;
        touch(&target);
        Ok(target)
    }

    // projects
    // --------

    pub fn project(&self, id: &str) -> Result<Project> {
        Project::open(self, id)
    }

    /// The identifiers of the projects of the store, sorted.
    pub fn project_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = fs::read_dir(self.projects())
            .map(|d| {
                d.flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        ids
    }

    // size and retention
    // ------------------

    /// Everything stored: pieces and file objects, by checksum.
    pub fn sizes(&self) -> HashMap<Ref, Item> {
        let mut found = HashMap::new();
        for (is_file, root) in [(false, self.objects()), (true, self.file_objects())] {
            let Ok(subs) = fs::read_dir(&root) else {
                continue;
            };
            for sub in subs.flatten() {
                if !sub.path().is_dir() {
                    continue;
                }
                let Ok(entries) = fs::read_dir(sub.path()) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.ends_with(".tmp") {
                        continue;
                    }
                    let Ok(meta) = fs::metadata(entry.path()) else {
                        continue;
                    };
                    let digest = name.split('.').next().unwrap_or("").to_string();
                    let key = if is_file {
                        Ref::File(digest)
                    } else {
                        Ref::Piece(digest)
                    };
                    found.insert(
                        key,
                        Item {
                            path: entry.path().to_string_lossy().into_owned(),
                            size: meta.len(),
                            mtime: mtime(&meta),
                        },
                    );
                }
            }
        }
        found
    }

    /// The bytes stored (pieces and files).
    pub fn size(&self) -> u64 {
        self.sizes().values().map(|i| i.size).sum()
    }

    /// Drop the oldest states of all projects until the store holds no more
    /// than `limit` bytes (counting `extra` bytes about to be stored), then
    /// remove the pieces and files that no state refers to (not those
    /// younger than `GRACE`). The current states of the places are kept. A
    /// removed file object goes through `discard` (the system trash).
    /// `dry`: change nothing. The states dropped.
    pub fn prune(&self, limit: u64, extra: u64, dry: bool, discard: &dyn Fn(&str)) -> Vec<Dropped> {
        self.prune_at(limit, extra, dry, discard, now())
    }

    pub fn prune_at(
        &self,
        limit: u64,
        extra: u64,
        dry: bool,
        discard: &dyn Fn(&str),
        now: f64,
    ) -> Vec<Dropped> {
        let sizes = self.sizes();
        let mut refs: HashMap<Ref, i64> = HashMap::new();
        let mut candidates: Vec<(f64, u64, String, State)> = Vec::new();
        for id in self.project_ids() {
            let Ok(project) = self.project(&id) else {
                continue;
            };
            let heads: HashSet<u64> = project
                .heads()
                .into_iter()
                .filter_map(|h| h.state)
                .collect();
            for (n, state) in project.states() {
                for r in state.refs() {
                    *refs.entry(r).or_insert(0) += 1;
                }
                if !heads.contains(n) {
                    candidates.push((state.time(), *n, id.clone(), state.clone()));
                }
            }
        }
        // oldest first, then by number (stable: the projects stay in order)
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));

        let removable = |r: &Ref, refs: &HashMap<Ref, i64>| {
            sizes.get(r).is_some_and(|item| {
                refs.get(r).copied().unwrap_or(0) == 0 && now - item.mtime >= GRACE
            })
        };
        let mut total = sizes.values().map(|i| i.size).sum::<u64>() as i64 + extra as i64;
        let mut freed: HashSet<Ref> = sizes
            .keys()
            .filter(|r| removable(r, &refs))
            .cloned()
            .collect();
        total -= freed.iter().map(|r| sizes[r].size as i64).sum::<i64>();
        let mut dropped = Vec::new();
        for (_, n, id, state) in candidates {
            if limit == 0 || total <= limit as i64 {
                break;
            }
            for r in state.refs() {
                *refs.entry(r.clone()).or_insert(0) -= 1;
                if !freed.contains(&r) && removable(&r, &refs) {
                    total -= sizes[&r].size as i64;
                    freed.insert(r);
                }
            }
            dropped.push(Dropped {
                project: id,
                number: n,
                state,
            });
        }
        if dry {
            return dropped;
        }
        let mut by_project: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
        for d in &dropped {
            by_project.entry(&d.project).or_default().push(d.number);
        }
        for (id, numbers) in by_project {
            if let Ok(mut project) = self.project(id) {
                for n in numbers {
                    project.drop_state(n);
                }
            }
        }
        for r in freed {
            let path = &sizes[&r].path;
            match r {
                Ref::File(_) => discard(path),
                Ref::Piece(_) => {
                    let _ = fs::remove_file(path);
                }
            }
        }
        dropped
    }
}

// the system trash
// ================

/// A file that papers does not keep goes to the trash of the system (the
/// user empties it), or is deleted if there is none.
pub fn to_system_trash(path: &str) {
    if move_to_trash(path).is_err() {
        let _ = fs::remove_file(path);
    }
}

fn move_to_trash(path: &str) -> std::io::Result<()> {
    let trash = if cfg!(target_os = "macos") {
        join(&home(), ".Trash")
    } else if cfg!(unix) {
        let data = match std::env::var("XDG_DATA_HOME") {
            Ok(v) if !v.is_empty() => v,
            _ => expanduser("~/.local/share"),
        };
        join(&join(&data, "Trash"), "files")
    } else {
        return Err(std::io::Error::other("no trash"));
    };
    if !isdir(&trash) {
        if cfg!(target_os = "macos") {
            return Err(std::io::Error::other("no trash")); // not made by papers
        }
        fs::create_dir_all(&trash)?;
    }
    let name = basename(path);
    let mut target = join(&trash, &name);
    let mut n = 1;
    while fs::symlink_metadata(&target).is_ok() {
        target = join(
            &trash,
            &format!(
                "{} {n}{}",
                name.trim_end_matches(splitext(&name)),
                splitext(&name)
            ),
        );
        n += 1;
    }
    super::restore::move_path(path, &target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(n: usize) -> String {
        format!("@article{{k{n},\n title = {{T}},\n}}\n\n")
    }

    #[test]
    fn the_example_of_the_specification() {
        let mut text = String::from("% comment\n");
        for n in 0..40 {
            text.push_str(&entry(n));
        }
        let pieces = cut_pieces(&text);
        assert_eq!(pieces.len(), 2);
        assert_eq!(crc32fast::hash(entry(30).as_bytes()), 977124256);
        assert!(
            pieces[0].ends_with(&entry(30)) && pieces[0].starts_with("% comment\n@article{k0,")
        );
        assert!(pieces[1].starts_with("@article{k31,") && pieces[1].ends_with(&entry(39)));
        assert_eq!(pieces.concat(), text);
    }

    #[test]
    fn cutting() {
        assert!(cut_pieces("").is_empty());
        assert_eq!(cut_pieces("x"), vec!["x"]);
        // a lone \r does not end a line, \r\n does
        let segments = |t: &str| t.matches("\n@").count() + 1;
        assert_eq!(segments("a\r@b\n@c"), 2);
        let text = "% c\r\n@a{k,\r\n}\r\n@b{l,\r\n}\r\n";
        assert_eq!(cut_pieces(text).concat(), text);
        // at most PIECE_MAX segments in a piece
        let text: String = (0..300).map(|_| "@x{k,\n}\n").collect();
        let pieces = cut_pieces(&text);
        assert!(pieces.iter().all(|p| p.matches("@x{").count() <= PIECE_MAX));
        assert_eq!(pieces.concat(), text);
    }

    #[test]
    fn objects_files_and_retention() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let store = Store::new(join(&root, "store"));
        let text: String = (0..100).map(entry).collect();
        let pieces = store.put_text(&text).unwrap();
        assert!(pieces.len() > 1);
        assert_eq!(store.text(&pieces).unwrap(), text);
        assert_eq!(
            store.text(&["ab".repeat(32)]).unwrap_err().0,
            "ab".repeat(32)
        );
        assert_eq!(store.put_text(&text).unwrap(), pieces);
        assert_eq!(
            store.size(),
            store.sizes().values().map(|i| i.size).sum::<u64>()
        );

        let file = join(&root, "x.PDF");
        fs::write(&file, "pdf").unwrap();
        let digest = sha256(b"pdf");
        let stored = store.put_file(&file, &digest).unwrap();
        assert!(stored.ends_with(&format!("{}/{digest}.PDF", &digest[..2])));
        assert!(!isdir(&file) && fs::metadata(&file).is_err());
        assert_eq!(store.file_object(&digest), Some(stored));
        // objects that no state refers to are removed, when old enough
        let removed = store.prune_at(
            1,
            0,
            false,
            &|p| drop(fs::remove_file(p)),
            now() + 2.0 * GRACE,
        );
        assert!(removed.is_empty());
        assert_eq!(store.size(), 0);
    }

    #[test]
    fn extension_like_python() {
        assert_eq!(splitext("/a/b.pdf"), ".pdf");
        assert_eq!(splitext("/a/.bashrc"), "");
        assert_eq!(splitext("/a.b/c"), "");
        assert_eq!(splitext("x.tar.gz"), ".gz");
        assert_eq!(splitext("x."), ".");
    }

    #[test]
    fn ids() {
        let id = new_id();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()) && id.as_bytes()[12] == b'4');
        assert_ne!(id, new_id());
    }
}
