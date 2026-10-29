//! Bringing the files from one state of a library to another: undo, redo,
//! restore, and the files of a revert (spec/history.md, section 6.4;
//! `papers.journal.restore`).
//!
//! The operations say what the commands did (moved or copied, deleted or
//! only unlinked, where from); the checksums find the files. No file is
//! overwritten, a file that changed since is left where it is, a file that no
//! operation touched and no checksum identifies is left alone.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Read;

use sha2::{Digest, Sha256};

use super::ops::{FileOp, OpKind, is_file_object};
use super::store::Store;
use crate::config::paths::{basename, dirname, isfile, realpath, relpath, short_home};
use crate::error::{Error, Result};

/// `lexists`: something is at the path, a link that points nowhere included.
fn lexists(path: &str) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn exists(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

/// The sha256 of a file, or `None` when it is not a file.
pub fn file_digest(path: &str) -> Option<String> {
    if !isfile(path) {
        return None;
    }
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

fn copy_tree(src: &str, dst: &str) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.is_dir() {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            copy_tree(&format!("{src}/{name}"), &format!("{dst}/{name}"))?;
        }
        Ok(())
    } else if meta.file_type().is_symlink() {
        std::os::unix::fs::symlink(fs::read_link(src)?, dst)
    } else {
        copy_file(src, dst)
    }
}

/// `shutil.copy2`: the content, the permissions and the modification time.
pub fn copy_file(src: &str, dst: &str) -> std::io::Result<()> {
    fs::copy(src, dst)?;
    if let Ok(modified) = fs::metadata(src).and_then(|m| m.modified()) {
        let _ = fs::File::options()
            .write(true)
            .open(dst)
            .and_then(|f| f.set_modified(modified));
    }
    Ok(())
}

/// `shutil.move`: a rename, or a copy and a delete across file systems.
pub fn move_path(src: &str, dst: &str) -> std::io::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            copy_tree(src, dst)?;
            if fs::symlink_metadata(src)?.is_dir() {
                fs::remove_dir_all(src)
            } else {
                fs::remove_file(src)
            }
        }
        Err(e) => Err(e),
    }
}

/// A path for the report: relative to `cwd` (the current directory) when it
/// is below it, else absolute with `~` for the home directory.
pub fn show(path: &str, cwd: &str) -> String {
    let relative = relpath(path, cwd);
    if relative.starts_with("..") {
        short_home(path)
    } else {
        relative
    }
}

fn arrow(src: &str, dst: &str, verb: &str, cwd: &str) -> String {
    if dirname(src) == dirname(dst) && verb.starts_with("moved") {
        format!(
            "    {} --> {} {}",
            show(src, cwd),
            verb.replacen("moved", "renamed", 1),
            basename(dst)
        )
    } else {
        format!("    {} --> {verb} {}", show(src, cwd), show(dst, cwd))
    }
}

/// Where the files that a command deletes go: the store (`trashed`), unless
/// `keep` is false (the system trash, `removed`).
pub struct Deleter<'a> {
    pub keep: bool,
    pub store: Option<&'a Store>,
    /// Where a file that is not kept goes (`to_system_trash`).
    pub trash: &'a dyn Fn(&str),
}

impl Deleter<'_> {
    /// Delete a file for the command (`journal.remove`): kept in the store
    /// (and recorded as `trashed`), or sent to the system trash (`removed`).
    /// The operation is recorded when `record` is given.
    pub fn remove(&self, path: &str, record: &mut Option<Vec<FileOp>>) -> Result<()> {
        let file = isfile(path);
        match self.store.filter(|_| self.keep && file) {
            None => {
                let digest = file_digest(path);
                if file {
                    push(record, OpKind::Removed, path, None, digest);
                    (self.trash)(path);
                    Ok(())
                } else {
                    fs::remove_file(path).map_err(|e| Error::other(format!("{path}: {e}")))
                }
            }
            Some(store) => {
                let digest = file_digest(path).unwrap_or_default();
                store.put_file(path, &digest)?;
                push(record, OpKind::Trashed, path, None, Some(digest));
                Ok(())
            }
        }
    }
}

/// `journal.record`: an operation done (moved, copied: the checksum is taken
/// at `dst`).
fn push(
    record: &mut Option<Vec<FileOp>>,
    kind: OpKind,
    src: &str,
    dst: Option<&str>,
    digest: Option<String>,
) {
    let Some(ops) = record else { return };
    let digest = match (digest, kind) {
        (None, OpKind::Moved | OpKind::Copied) => dst.and_then(file_digest),
        (d, _) => d,
    };
    ops.push(FileOp::new(kind, src, dst, digest));
}

/// What to bring from one state to another (`journal.restore`).
pub struct Restore<'a> {
    /// The paths of the files that the entries link to, now.
    pub current: &'a BTreeSet<String>,
    /// The paths linked in the state restored.
    pub target: &'a BTreeSet<String>,
    /// The operations of the commands between the two states, oldest first
    /// (absolute normalized paths).
    pub ops: &'a [FileOp],
    /// The state restored is the earlier one.
    pub undo: bool,
    /// The checksums of the files of the state restored.
    pub hashes: &'a BTreeMap<String, String>,
    /// Folders whose files are copied, not moved (the files of the pool).
    pub pool_dirs: &'a [String],
    /// Files that another library links to: copied, not moved.
    pub shared: &'a BTreeSet<String>,
    /// Where the deleted files are (a file deleted again is kept there).
    pub store: &'a Store,
    pub deleter: &'a Deleter<'a>,
    /// The current directory: the paths of the report are relative to it.
    pub cwd: &'a str,
}

fn inside(path: &str, dirs: &[String]) -> bool {
    let real = realpath(path);
    dirs.iter()
        .filter(|d| !d.is_empty())
        .any(|d| real.starts_with(&format!("{}/", realpath(d))))
}

fn io<T>(path: &str, result: std::io::Result<T>) -> Result<T> {
    result.map_err(|e| Error::other(format!("{path}: {e}")))
}

fn place_file(src: &str, dst: &str, copy: bool) -> Result<()> {
    let parent = dirname(dst);
    if !parent.is_empty() {
        io(&parent, fs::create_dir_all(&parent))?;
    }
    io(
        dst,
        if copy {
            copy_file(src, dst)
        } else {
            move_path(src, dst)
        },
    )
}

impl Restore<'_> {
    /// The line of a file deleted again by redo: kept in the history, or sent to the trash of
    /// the system (`keep-deleted-files` false).
    fn deleted_again(&self, path: &str) -> String {
        let where_ = if self.deleter.keep {
            "kept in the history"
        } else {
            "to the trash of the system"
        };
        format!("    {}: deleted again ({where_})", show(path, self.cwd))
    }

    /// The source of a copy; a copy from the history (`src` is a file object of a store) is found
    /// by its checksum when the file is not there any more (the history may have moved).
    fn copied_from(&self, op: &FileOp) -> String {
        if is_file_object(&op.src)
            && !isfile(&op.src)
            && let Some(found) = op
                .checksum
                .as_deref()
                .and_then(|c| self.store.file_object(c))
        {
            return found;
        }
        op.src.clone()
    }

    /// Bring the files; the lines of the report. The operations done are
    /// added to `record` when it is given (a revert records them, undo, redo
    /// and restore do not).
    pub fn run(&self, record: &mut Option<Vec<FileOp>>) -> Result<Vec<String>> {
        let mut lines: Vec<String> = Vec::new();
        let undo = self.undo;
        let back = if undo { " back" } else { "" };
        let ops = self.ops;
        let (current, target) = (self.current, self.target);
        let cwd = self.cwd;
        let show = |path: &str| self::show(path, cwd);
        let mut known: HashMap<String, String> = self
            .hashes
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let moves: Vec<&FileOp> = ops
            .iter()
            .filter(|o| matches!(o.kind, OpKind::Moved | OpKind::Copied))
            .collect();
        for op in ops {
            if let Some(sum) = &op.checksum {
                for path in [Some(&op.src), op.dst.as_ref()]
                    .into_iter()
                    .flatten()
                    .filter(|p| !p.is_empty())
                {
                    known.entry(path.clone()).or_insert_with(|| sum.clone());
                }
            }
        }
        let mut digests: HashMap<String, Option<String>> = HashMap::new();
        let digest =
            |digests: &mut HashMap<String, Option<String>>, path: &str| -> Option<String> {
                digests
                    .entry(path.to_string())
                    .or_insert_with(|| file_digest(path))
                    .clone()
            };

        // the other ends of the moves and copies of a path, the last command first
        let partners = |path: &str| -> Vec<(String, &FileOp)> {
            let mut found = Vec::new();
            for op in moves.iter().rev() {
                match &op.dst {
                    Some(dst) if !dst.is_empty() && op.src == path => {
                        found.push((dst.clone(), *op))
                    }
                    _ if op.dst.as_deref() == Some(path) => found.push((op.src.clone(), *op)),
                    _ => {}
                }
            }
            found
        };

        let mentioned: BTreeSet<String> = moves
            .iter()
            .flat_map(|o| [Some(o.src.clone()), o.dst.clone()])
            .flatten()
            .filter(|p| !p.is_empty())
            .collect();
        // the identical files kept when the commands deleted a file (e.g. that of the pool): copied
        let kept: BTreeSet<String> = ops
            .iter()
            .filter(|o| o.kind == OpKind::Removed)
            .filter_map(|o| o.dst.clone())
            .filter(|d| !d.is_empty())
            .collect();
        let mut candidates: Vec<String> = current
            .union(&mentioned)
            .filter(|p| !target.contains(*p))
            .cloned()
            .collect();
        candidates.extend(target.iter().cloned());
        let mut reported: BTreeSet<String> = BTreeSet::new();

        // put the file `src` at `path`, and report it
        let place = |path: &str,
                     src: &str,
                     copy: bool,
                     verb: &str,
                     digests: &mut HashMap<String, Option<String>>,
                     lines: &mut Vec<String>,
                     record: &mut Option<Vec<FileOp>>|
         -> Result<()> {
            place_file(src, path, copy)?;
            push(
                record,
                if copy { OpKind::Copied } else { OpKind::Moved },
                src,
                Some(path),
                None,
            );
            digests.remove(src);
            digests.remove(path);
            lines.push(arrow(src, path, verb, cwd));
            Ok(())
        };

        // the files the restored state links to and that are not linked now
        for path in target.difference(current) {
            let want = known.get(path).cloned();
            if lexists(path) {
                // the place is taken by another file: what the command put elsewhere stays there
                if want.is_some() && digest(&mut digests, path) != want {
                    for (other, _) in partners(path) {
                        if exists(&other) && !reported.contains(&other) {
                            lines.push(format!(
                                "    {}: not moved{back}, {} exists",
                                show(&other),
                                show(path)
                            ));
                            reported.insert(other);
                        }
                    }
                }
                continue;
            }
            // (other, copy, found through an operation)
            let mut source: Option<(String, bool, bool)> = None;
            for (other, op) in partners(path) {
                if !exists(&other) {
                    continue;
                }
                if want.is_some() && isfile(&other) && digest(&mut digests, &other) != want {
                    lines.push(format!(
                        "    {}: not where the command left it (or changed since): left as it is",
                        show(&other)
                    ));
                    reported.insert(other);
                    continue;
                }
                let copy = op.kind == OpKind::Copied && other == op.src;
                source = Some((other, copy, true));
                break;
            }
            if source.is_none()
                && let Some(w) = &want
            {
                let found = candidates
                    .iter()
                    .find(|c| {
                        c.as_str() != path.as_str()
                            && isfile(c)
                            && digest(&mut digests, c).as_deref() == Some(w.as_str())
                    })
                    .cloned();
                if let Some(other) = found {
                    source = Some((other, false, false));
                }
            }
            if let Some((other, copy, by_operation)) = source {
                let copy = copy
                    || target.contains(&other)
                    || kept.contains(&other)
                    || inside(&other, self.pool_dirs)
                    || self.shared.contains(&other);
                if copy && !by_operation {
                    place_file(&other, path, true)?;
                    push(record, OpKind::Copied, &other, Some(path), None);
                    lines.push(format!(
                        "    {}: restored (a copy of {})",
                        show(path),
                        show(&other)
                    ));
                } else {
                    let verb = format!("{}{back} to", if copy { "copied" } else { "moved" });
                    place(path, &other, copy, &verb, &mut digests, &mut lines, record)?;
                }
                continue;
            }
            let found = want.as_deref().and_then(|w| self.store.file_object(w));
            if let Some(found) = found {
                place_file(&found, path, true)?; // the history keeps it, for the other states
                push(record, OpKind::Copied, &found, Some(path), None);
                lines.push(format!("    {}: restored from the history", show(path)));
                continue;
            }
            let deleted: Vec<&FileOp> = ops
                .iter()
                .filter(|o| o.src == *path && matches!(o.kind, OpKind::Trashed | OpKind::Removed))
                .collect();
            match deleted.last() {
                Some(op) if op.kind == OpKind::Trashed => lines.push(format!(
                    "    {}: no longer in the history (see the history-size setting), not restored",
                    show(path)
                )),
                Some(_) => lines.push(format!(
                    "    {}: deleted by the command, not restored",
                    show(path)
                )),
                None if want.is_some() => lines.push(format!(
                    "    {}: deleted by the command, not restored",
                    show(path)
                )),
                None => {}
            }
        }

        // the files deleted by the commands whose link did not change (no entry links to them, e.g.
        // papers file clean, or the same entry before and after): brought back by undo, deleted
        // again by redo
        let ordered: Vec<&FileOp> = if undo {
            ops.iter().rev().collect()
        } else {
            ops.iter().collect()
        };
        for op in &ordered {
            let path = &op.src;
            if op.kind != OpKind::Trashed
                || current.contains(path) != target.contains(path)
                || reported.contains(path)
            {
                continue;
            }
            reported.insert(path.clone());
            if undo && !lexists(path) {
                let found = op
                    .checksum
                    .as_deref()
                    .and_then(|c| self.store.file_object(c));
                match found {
                    Some(found) => {
                        place_file(&found, path, true)?;
                        push(record, OpKind::Copied, &found, Some(path), None);
                        lines.push(format!("    {}: restored from the history", show(path)));
                    }
                    None => lines.push(format!(
                        "    {}: no longer in the history (see the history-size setting), not restored",
                        show(path)
                    )),
                }
            } else if !undo && isfile(path) && digest(&mut digests, path) == op.checksum {
                self.deleter.remove(path, record)?;
                lines.push(self.deleted_again(path));
            }
        }

        // the files that the commands copied to a place linked before and after (papers file fix
        // from the history): removed by undo, copied again by redo
        for op in &ordered {
            let Some(path) = op.dst.as_deref().filter(|d| !d.is_empty()) else {
                continue;
            };
            if op.kind != OpKind::Copied
                || !current.contains(path)
                || !target.contains(path)
                || reported.contains(path)
            {
                continue;
            }
            reported.insert(path.to_string());
            let src = self.copied_from(op);
            if undo && isfile(path) && digest(&mut digests, path) == op.checksum && isfile(&src) {
                let own = digest(&mut digests, path);
                push(record, OpKind::Removed, path, Some(&src), own);
                io(path, fs::remove_file(path))?;
                lines.push(format!(
                    "    {}: removed (a copy of {})",
                    show(path),
                    show(&src)
                ));
            } else if !undo && !lexists(path) && isfile(&src) {
                place(
                    path,
                    &src,
                    true,
                    "copied to",
                    &mut digests,
                    &mut lines,
                    record,
                )?;
            }
        }

        // the files that the commands put in place, and that the restored state does not link
        let leftover: Vec<String> = current
            .union(&mentioned)
            .filter(|p| !target.contains(*p))
            .cloned()
            .collect();
        for path in leftover {
            if reported.contains(&path) || !isfile(&path) {
                continue;
            }
            if undo {
                let mut decided = false;
                for (other, op) in partners(&path) {
                    if op.dst.as_deref() != Some(path.as_str())
                        || lexists(&other)
                        || current.contains(&other)
                    {
                        continue;
                    }
                    if op.checksum.is_some() && digest(&mut digests, &path) != op.checksum {
                        lines.push(format!(
                            "    {}: not where the command left it (or changed since): left as it is",
                            show(&path)
                        ));
                    } else if op.kind == OpKind::Moved {
                        let copy = inside(&path, self.pool_dirs) || self.shared.contains(&path);
                        let verb = format!("{} back to", if copy { "copied" } else { "moved" });
                        place(&other, &path, copy, &verb, &mut digests, &mut lines, record)?;
                    }
                    decided = true;
                    break;
                }
                if !decided {
                    let copied = moves.iter().rev().find(|o| {
                        o.kind == OpKind::Copied && o.dst.as_deref() == Some(path.as_str())
                    });
                    if let Some(copied) = copied {
                        let src = self.copied_from(copied);
                        let original = digest(&mut digests, &src);
                        if isfile(&src) && original == digest(&mut digests, &path) {
                            let own = digest(&mut digests, &path);
                            push(record, OpKind::Removed, &path, Some(&src), own);
                            io(&path, fs::remove_file(&path))?;
                            lines.push(format!(
                                "    {}: removed (a copy of {})",
                                show(&path),
                                show(&src)
                            ));
                        }
                    }
                }
            } else {
                let deleted = ops.iter().rev().find(|o| {
                    o.src == path
                        && matches!(o.kind, OpKind::Trashed | OpKind::Removed | OpKind::Moved)
                });
                let Some(deleted) = deleted else { continue };
                if deleted.checksum.is_some() && digest(&mut digests, &path) != deleted.checksum {
                    continue;
                }
                match deleted.kind {
                    OpKind::Moved => {
                        // copied back by the undo (e.g. from the pool, which keeps it): moved again
                        let dst = deleted.dst.clone().unwrap_or_default();
                        if target.contains(&dst)
                            && digest(&mut digests, &dst) == digest(&mut digests, &path)
                        {
                            let own = digest(&mut digests, &path);
                            push(record, OpKind::Removed, &path, Some(&dst), own);
                            io(&path, fs::remove_file(&path))?;
                            lines.push(arrow(&path, &dst, "moved to", cwd));
                        }
                    }
                    OpKind::Trashed => {
                        self.deleter.remove(&path, record)?;
                        lines.push(self.deleted_again(&path));
                    }
                    _ => {
                        let own = digest(&mut digests, &path);
                        push(record, OpKind::Removed, &path, None, own);
                        io(&path, fs::remove_file(&path))?;
                        lines.push(format!("    {}: removed again", show(&path)));
                    }
                }
            }
        }
        Ok(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sandbox {
        _dir: tempfile::TempDir,
        root: String,
        store: Store,
    }

    fn sandbox() -> Sandbox {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let store = Store::new(format!("{root}/store"));
        Sandbox {
            _dir: dir,
            root,
            store,
        }
    }

    fn set(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    fn run(
        s: &Sandbox,
        current: &[&str],
        target: &[&str],
        ops: &[FileOp],
        undo: bool,
        hashes: &[(&str, &str)],
        record: &mut Option<Vec<FileOp>>,
    ) -> Vec<String> {
        let hashes: BTreeMap<String, String> = hashes
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let deleter = Deleter {
            keep: true,
            store: Some(&s.store),
            trash: &|p| drop(fs::remove_file(p)),
        };
        Restore {
            current: &set(current),
            target: &set(target),
            ops,
            undo,
            hashes: &hashes,
            pool_dirs: &[],
            shared: &BTreeSet::new(),
            store: &s.store,
            deleter: &deleter,
            cwd: &s.root,
        }
        .run(record)
        .unwrap()
    }

    #[test]
    fn a_moved_file_goes_back_and_forth() {
        let s = sandbox();
        let (src, dst) = (
            format!("{}/dl/x.pdf", s.root),
            format!("{}/files/a.pdf", s.root),
        );
        fs::create_dir_all(dirname(&dst)).unwrap();
        fs::write(&dst, "x").unwrap();
        let sum = crate::history::store::sha256(b"x");
        let ops = vec![FileOp::new(
            OpKind::Moved,
            &src,
            Some(&dst),
            Some(sum.clone()),
        )];
        // undo: the entry no longer links dst: the file goes back to src
        let lines = run(&s, &[&dst], &[], &ops, true, &[], &mut None);
        assert!(isfile(&src) && !isfile(&dst), "{lines:?}");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("--> moved back to"), "{lines:?}");
        // redo: the entry links dst again: it comes from src
        let lines = run(&s, &[], &[&dst], &ops, false, &[], &mut None);
        assert!(isfile(&dst) && !isfile(&src), "{lines:?}");
        assert!(lines[0].contains("--> moved to"), "{lines:?}");
        // a file is never overwritten
        fs::write(&src, "other").unwrap();
        let lines = run(&s, &[&dst], &[], &ops, true, &[], &mut None);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("not where the command left it"))
                || isfile(&dst),
            "{lines:?}"
        );
        assert_eq!(fs::read_to_string(&src).unwrap(), "other");
    }

    #[test]
    fn a_deleted_file_comes_back_from_the_store_and_goes_again() {
        let s = sandbox();
        let path = format!("{}/files/a.pdf", s.root);
        fs::create_dir_all(dirname(&path)).unwrap();
        fs::write(&path, "pdf").unwrap();
        let sum = crate::history::store::sha256(b"pdf");
        let mut record = Some(Vec::new());
        Deleter {
            keep: true,
            store: Some(&s.store),
            trash: &|p| drop(fs::remove_file(p)),
        }
        .remove(&path, &mut record)
        .unwrap();
        assert!(!isfile(&path));
        assert_eq!(record.as_ref().unwrap()[0].kind, OpKind::Trashed);
        let ops = vec![FileOp::new(OpKind::Trashed, &path, None, Some(sum.clone()))];
        let lines = run(&s, &[], &[&path], &ops, true, &[(&path, &sum)], &mut None);
        assert!(isfile(&path), "{lines:?}");
        assert!(lines[0].ends_with("restored from the history"), "{lines:?}");
        // redo deletes it again, into the store
        let lines = run(&s, &[&path], &[], &ops, false, &[], &mut None);
        assert!(!isfile(&path), "{lines:?}");
        assert!(
            lines[0].ends_with("deleted again (kept in the history)"),
            "{lines:?}"
        );
        // and what is no longer in the store is said
        fs::remove_file(s.store.file_object(&sum).unwrap()).unwrap();
        let lines = run(&s, &[], &[&path], &ops, true, &[(&path, &sum)], &mut None);
        assert!(
            lines[0]
                .ends_with("no longer in the history (see the history-size setting), not restored"),
            "{lines:?}"
        );
    }

    #[test]
    fn a_copy_is_removed_when_the_original_stays() {
        let s = sandbox();
        let (src, dst) = (
            format!("{}/x.pdf", s.root),
            format!("{}/files/a.pdf", s.root),
        );
        fs::create_dir_all(dirname(&dst)).unwrap();
        fs::write(&src, "x").unwrap();
        fs::write(&dst, "x").unwrap();
        let ops = vec![FileOp::new(
            OpKind::Copied,
            &src,
            Some(&dst),
            Some(crate::history::store::sha256(b"x")),
        )];
        let mut record = Some(Vec::new());
        let lines = run(&s, &[&dst], &[], &ops, true, &[], &mut record);
        assert!(!isfile(&dst) && isfile(&src), "{lines:?}");
        assert!(lines[0].contains("removed (a copy of"), "{lines:?}");
        assert_eq!(record.unwrap()[0].kind, OpKind::Removed);
    }
}
