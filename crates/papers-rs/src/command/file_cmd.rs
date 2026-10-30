//! `papers file`: the files of the entries (`papers.cli.file_cmds`, `papers.files`): place them
//! under their template name (`rename`), move them (`mv`), attach and detach them, remove them
//! (`rm`), report and repair the links (`check`, `fix`), clean the files directory (`clean`),
//! write their checksums (`checksum`). The actions of `papers list` on the files
//! (`--attach`, `--detach`, `--rename`, `--fix-files`) are these functions on the listed entries.
//!
//! Operations on a [`Session`]: the command line selects the entries, calls one, commits the
//! session and prints what changed. In a shallow library the files go to the pool (the command calls
//! [`Session::use_pool_files`]) and the links that change are updated in the pool entries (the automatic
//! push at the commit); `check` and `fix` also look in the libraries of `--pool PATH`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::bib::Entry;
use crate::bib::encoding::parse_file;
use crate::bib::latex::latex_to_unicode;
use crate::config::json::py_repr_str;
use crate::config::paths::{
    abspath, basename, dirname, exists, expanduser, is_abs, is_inside, isdir, isfile, join,
    normpath, realpath, short_home,
};
use crate::entry::{self, val};
use crate::error::{Error, Result};
use crate::extract::{doi::isvaliddoi, extract_pdf_doi};
use crate::fetch::download::{download_url, is_download};
use crate::history::{FilesJson, History, OpKind, short};
use crate::library::{Library, Policy};
use crate::log;
use crate::names::{Format, fit_name};
use crate::pools::PoolIx;

use super::files::{Mode, Placement, PolicyOptions, files_mode, files_policy_with};
use super::pool::push_note_lines;
use super::queries::Queries;
use super::session::Session;

fn islink(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

fn lexists(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// `os.path.splitext(path)[1]`: the extension with its dot (a leading dot of the name is not one).
fn splitext_ext(path: &str) -> String {
    let base = basename(path);
    let stem = base.len() - base.trim_start_matches('.').len();
    match base[stem..].rfind('.') {
        Some(i) => base[stem + i..].to_string(),
        None => String::new(),
    }
}

/// The files mode of the `files` setting, else `move` (`config.files or 'move'`).
fn setting_mode(session: &Session) -> Mode {
    session
        .config
        .get("files")
        .as_str()
        .filter(|s| !s.is_empty())
        .and_then(Mode::parse)
        .unwrap_or(Mode::Move)
}

/// What to do with each file placed in the files directory (`FilesPolicy`, with its `keep`, `copied`
/// and `pool_files`): the files that papers downloaded are moved; those of `keep` (the files an entry
/// had) stay where they are; in link mode all stay; those of `copied` (the files that `fix` found)
/// are copied, never moved, even from the files directory; those of `pool_files` (files of pool
/// entries) are copied (linked, in a shallow library); for the others, see [`files_policy_with`].
fn policy_for(
    session: &mut Session,
    mode: Mode,
    keep: &[String],
    copied: &[String],
    pool_files: &[String],
) -> (Policy, Placement) {
    let pool_dirs = session.pool_files_dirs();
    let filesdir = session.library.filesdir.clone();
    // a shallow library places its files in the files directory of its pool (`use_pool_files`)
    let in_pool = session.config.shallow()
        && filesdir
            .as_deref()
            .is_some_and(|d| pool_dirs.iter().any(|p| realpath(p) == realpath(d)));
    let mut config = session.config.clone();
    config.filesdir = filesdir;
    let placement = files_policy_with(
        &config,
        mode,
        session.say.clone(),
        session.prompt(),
        &PolicyOptions {
            keep: keep.to_vec(),
            copied: copied.to_vec(),
            pool_files: pool_files.to_vec(),
            pool_dirs,
            in_pool,
        },
    );
    (placement.policy.clone(), placement)
}

// rename
// ------

/// `papers file rename` (also `papers list --rename`): place the files of the entries in the
/// files directory under their template name. The files already there are renamed; the others
/// are moved, copied or left where they are as the files mode says (`mode`, else the `files`
/// setting; `keep_source`: copy). A file that belongs to another library is copied. Returns the
/// number of entries whose files could not be placed (an error is logged for each).
pub fn rename_entries(
    session: &mut Session,
    keys: &[String],
    keep_source: bool,
    mode: Option<Mode>,
) -> Result<usize> {
    if session.config.filesdir.is_none() {
        return Err(Error::exit(
            "the library has no files directory: pass --filesdir, or set it with `papers config filesdir DIR`",
        ));
    }
    let mode = if keep_source {
        Mode::Copy
    } else {
        mode.unwrap_or_else(|| setting_mode(session))
    };
    let (policy, placement) = policy_for(session, mode, &[], &[], &[]);
    let mut failed = 0;
    for key in keys {
        match session
            .library
            .rename_entry_files(key, &policy, None, None, true)
        {
            Ok(true) => {}
            Ok(false) => failed += 1,
            Err(error) => {
                log::error(&format!("{key}: {}", error.message()));
                failed += 1;
            }
        }
    }
    placement.check()?;
    Ok(failed)
}

// attach
// ------

/// How [`attach_files`] places the files.
#[derive(Debug, Clone)]
pub struct Attach {
    pub mode: Mode,
    /// Links of the entry (broken ones) replaced in place by files, placed in the same way
    /// (the files to attach come after them).
    pub replace: Vec<(String, Vec<String>)>,
    /// Files that are copied, never moved (the files that `fix` found).
    pub copied: Vec<String>,
    /// Files of pool entries: never moved (linked, in a shallow library).
    pub pool_files: Vec<String>,
}

impl Attach {
    pub fn new(mode: Mode) -> Attach {
        Attach {
            mode,
            replace: Vec::new(),
            copied: Vec::new(),
            pool_files: Vec::new(),
        }
    }
}

/// A file to attach: a URL is downloaded (it is moved into the files directory), a file must exist.
fn resolve_file(file: &str) -> Result<String> {
    if file.starts_with("http://") || file.starts_with("https://") {
        return download_url(file, false, None).map_err(Error::exit);
    }
    if !exists(file) {
        return Err(Error::exit(format!("file {file} does not exist")));
    }
    Ok(abspath(file))
}

/// `papers file attach` (also `papers list --attach`): add files to the links of an entry, and
/// place them as the files mode says (the files the entry had stay where they are). False when
/// the name points outside the files directory (the links are kept, the files left where they are).
pub fn attach_files(
    session: &mut Session,
    key: &str,
    files: &[String],
    options: &Attach,
) -> Result<bool> {
    let old = session.library.files(key);
    let mut replace: Vec<(String, Vec<String>)> = Vec::new();
    for (link, files) in &options.replace {
        let resolved = files
            .iter()
            .map(|f| resolve_file(f))
            .collect::<Result<Vec<_>>>()?;
        replace.push((link.clone(), resolved));
    }
    let new = files
        .iter()
        .map(|f| resolve_file(f))
        .collect::<Result<Vec<_>>>()?;
    let kept: Vec<String> = old
        .iter()
        .filter(|f| !replace.iter().any(|(link, _)| link == *f))
        .cloned()
        .collect();
    let added: Vec<&String> = replace.iter().flat_map(|(_, fs)| fs).chain(&new).collect();
    let downloaded = added.iter().any(|f| is_download(f));
    let all: Vec<String> = old
        .iter()
        .flat_map(|f| match replace.iter().find(|(link, _)| link == f) {
            Some((_, fs)) => fs.clone(),
            None => vec![f.clone()],
        })
        .chain(new.iter().cloned())
        .collect();
    session.library.set_files(key, &all)?;
    if options.mode != Mode::Link || downloaded {
        let (policy, placement) = policy_for(
            session,
            options.mode,
            &kept,
            &options.copied,
            &options.pool_files,
        );
        let placed = session
            .library
            .rename_entry_files(key, &policy, None, None, true)?;
        placement.check()?;
        return Ok(placed);
    }
    Ok(true)
}

// detach
// ------

/// `{real path of a file: the keys of the entries that link to it}` (`linked_files`).
fn linked_files(library: &Library) -> HashMap<String, Vec<String>> {
    let mut links: HashMap<String, Vec<String>> = HashMap::new();
    for e in library.entries() {
        for f in entry::files(e, Some(&library.relative_to)) {
            links.entry(realpath(&f)).or_default().push(e.key.clone());
        }
    }
    links
}

/// The entries of these keys in the order given, else all the entries of the library.
fn ordered(library: &Library, keys: Option<&[String]>) -> Vec<String> {
    match keys {
        Some(keys) => keys.to_vec(),
        None => library.entries().map(|e| e.key.clone()).collect(),
    }
}

/// The links to remove for `papers file detach FILE` and `papers file rm FILE`: for each entry
/// (of `keys`, else all) that links to a path, its links, or with `recursive` to a file inside a
/// folder. Links are compared by real path, so a broken link can be named too. A path that none
/// of the entries links to is an error, and so is a folder without `recursive`: raised before
/// anything changes.
pub fn links_to_paths(
    library: &Library,
    paths: &[String],
    keys: Option<&[String]>,
    recursive: bool,
) -> Result<Vec<(String, Vec<String>)>> {
    let whom = if keys.is_none() {
        "no entry"
    } else {
        "none of the entries of --key"
    };
    let links: Vec<(String, Vec<String>)> = ordered(library, keys)
        .into_iter()
        .map(|k| {
            let files = library.files(&k);
            (k, files)
        })
        .collect();
    let mut found: Vec<(String, Vec<String>)> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for path in paths {
        let dir = isdir(path) && !islink(path);
        if dir && !recursive {
            errors.push(format!(
                "{path}: is a folder (-r for the linked files inside it)"
            ));
            continue;
        }
        let real = realpath(path);
        let matches = |f: &str| {
            if dir {
                is_inside(f, path)
            } else {
                realpath(f) == real
            }
        };
        let mut hit = false;
        for (key, files) in &links {
            for f in files {
                if matches(f) {
                    hit = true;
                    match found.iter_mut().find(|(k, _)| k == key) {
                        Some((_, fs)) => fs.push(f.clone()),
                        None => found.push((key.clone(), vec![f.clone()])),
                    }
                }
            }
        }
        if !hit {
            errors.push(format!(
                "{path}: {whom} links to {}",
                if dir {
                    "a file in this folder"
                } else {
                    "this file"
                }
            ));
        }
    }
    if !errors.is_empty() {
        return Err(Error::exit(format!(
            "{}\n(nothing was changed)",
            errors.join("\n")
        )));
    }
    Ok(found
        .into_iter()
        .map(|(key, files)| {
            let mut seen = HashSet::new();
            (
                key,
                files
                    .into_iter()
                    .filter(|f| seen.insert(f.clone()))
                    .collect(),
            )
        })
        .collect())
}

/// Remove the links to these files from an entry (`detach_links`): compared by real path.
fn detach_links(library: &mut Library, key: &str, files: &[String]) -> Result<()> {
    let remove: HashSet<String> = files.iter().map(|f| realpath(f)).collect();
    let kept: Vec<String> = library
        .files(key)
        .into_iter()
        .filter(|f| !remove.contains(&realpath(f)))
        .collect();
    library.set_files(key, &kept)
}

/// Remove the links `[(key, files)]` and say which entries lost a link (`_detach`).
fn detach(session: &mut Session, links: &[(String, Vec<String>)]) -> Result<()> {
    let prefix = if session.dry_run() { "DRYRUN: " } else { "" };
    for (key, files) in links {
        detach_links(&mut session.library, key, files)?;
        for f in files {
            session
                .say
                .line(format!("{prefix}{key}: removed the link to {f}"));
        }
    }
    Ok(())
}

/// The broken links of all the entries removed, without a word (`papers filecheck --delete-broken`).
pub fn detach_broken_quietly(session: &mut Session) -> Result<()> {
    let keys: Vec<String> = session.library.entries().map(|e| e.key.clone()).collect();
    for key in keys {
        let broken: Vec<String> = session
            .library
            .files(&key)
            .into_iter()
            .filter(|f| !exists(f))
            .collect();
        detach_links(&mut session.library, &key, &broken)?;
    }
    Ok(())
}

/// `papers file detach` (also `papers list --detach`): remove the links of the entries (`None`:
/// all entries) to the files (all their links without files; only the broken links with
/// `broken`), and save the library.
pub fn detach_entries(
    session: &mut Session,
    keys: Option<&[String]>,
    files: &[String],
    broken: bool,
    recursive: bool,
) -> Result<()> {
    let selected = ordered(&session.library, keys);
    let links: Vec<(String, Vec<String>)> = if broken {
        selected
            .iter()
            .map(|k| {
                (
                    k.clone(),
                    session
                        .library
                        .files(k)
                        .into_iter()
                        .filter(|f| !exists(f))
                        .collect(),
                )
            })
            .collect()
    } else if !files.is_empty() {
        links_to_paths(&session.library, files, keys, recursive)?
    } else {
        selected
            .iter()
            .map(|k| (k.clone(), session.library.files(k)))
            .collect()
    };
    let links: Vec<(String, Vec<String>)> =
        links.into_iter().filter(|(_, fs)| !fs.is_empty()).collect();
    detach(session, &links)?;
    let committed = session.commit()?;
    for line in push_note_lines(&session.library, &committed.notes) {
        session.say.line(line);
    }
    if links.is_empty() {
        session.say.line("no link removed");
    }
    Ok(())
}

// rm
// --

/// The files of removed links that may be deleted (`files_to_delete`): `(inside, outside, kept)`:
/// in the files directory, elsewhere, and still linked by an entry (never deleted). Only
/// existing files, once each.
pub fn files_to_delete(
    library: &Library,
    removed: &[String],
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let still = linked_files(library);
    let (mut inside, mut outside, mut kept) = (Vec::new(), Vec::new(), Vec::new());
    let mut seen = HashSet::new();
    for file in removed {
        let real = realpath(file);
        if !seen.insert(real.clone()) || !exists(file) {
            continue;
        }
        if still.contains_key(&real) {
            kept.push(file.clone());
        } else if library
            .filesdir
            .as_deref()
            .is_some_and(|d| is_inside(file, d))
        {
            inside.push(file.clone());
        } else {
            outside.push(file.clone());
        }
    }
    (inside, outside, kept)
}

/// After `papers file rm -r top`: remove the folders of the deleted files that are left empty,
/// from the file's folder up to `top` included, as `git rm -r` does; the folders in `keep` (the
/// files directory) stay (`remove_empty_folders`).
fn remove_empty_folders(session: &Session, deleted: &[String], top: &str, keep: &[Option<String>]) {
    let dry = session.dry_run();
    let keep: HashSet<String> = keep.iter().flatten().map(|k| realpath(k)).collect();
    let mut gone: HashSet<String> = deleted.iter().map(|f| realpath(f)).collect();
    for file in deleted {
        let mut folder = dirname(&abspath(file));
        while is_inside(&folder, top)
            && !keep.contains(&realpath(&folder))
            && !gone.contains(&realpath(&folder))
        {
            let Ok(read) = std::fs::read_dir(&folder) else {
                break;
            };
            let mut left = false;
            for e in read.flatten() {
                let path = join(&folder, &e.file_name().to_string_lossy());
                if !gone.contains(&realpath(&path)) {
                    left = true;
                    break;
                }
            }
            if !isdir(&folder) || left {
                break;
            }
            session.say.line(format!(
                "{}remove folder {folder}",
                if dry { "DRYRUN: " } else { "" }
            ));
            if !dry {
                let _ = std::fs::remove_dir(&folder);
            }
            gone.insert(realpath(&folder));
            folder = dirname(&folder);
        }
    }
}

/// `papers file rm`: remove the links to the files from the entries (those of `keys`, else all)
/// and delete the files that no entry links to. `-r`: a folder is its linked files, and the
/// folders left empty are removed.
pub fn rm_files(
    session: &mut Session,
    keys: Option<&[String]>,
    files: &[String],
    recursive: bool,
) -> Result<()> {
    let links = links_to_paths(&session.library, files, keys, recursive)?;
    detach(session, &links)?;
    let removed: Vec<String> = links.iter().flat_map(|(_, fs)| fs.clone()).collect();
    let (mut inside, mut outside, kept) = files_to_delete(&session.library, &removed);
    let mut keep = vec![session.config.filesdir.clone()];
    let mut pool_kept: Vec<String> = Vec::new();
    if session.config.shallow() {
        // the links of the pool entries were removed too (the commit): a pool file that no entry of the
        // pool or of the library links to any more is deleted
        let pool_linked = session.pool_linked_files();
        let pool_dirs = session.pool_files_dirs();
        if !pool_dirs.is_empty() {
            keep.extend(pool_dirs.into_iter().map(Some));
        }
        pool_kept = inside
            .iter()
            .chain(&outside)
            .filter(|f| pool_linked.contains(&realpath(f)))
            .cloned()
            .collect();
        inside.retain(|f| !pool_linked.contains(&realpath(f)));
        outside.retain(|f| !pool_linked.contains(&realpath(f)));
    }
    let committed = session.commit()?;
    for line in push_note_lines(&session.library, &committed.notes) {
        session.say.line(line);
    }
    for f in &kept {
        session
            .say
            .line(format!("kept {f}: another entry links to it"));
    }
    for f in &pool_kept {
        session
            .say
            .line(format!("kept {f}: an entry of the pool links to it"));
    }
    let deleted: Vec<String> = inside.into_iter().chain(outside).collect();
    session.delete_files(&deleted, &[])?;
    for path in files {
        if isdir(path) {
            remove_empty_folders(session, &deleted, path, &keep);
        }
    }
    Ok(())
}

// mv
// --

/// One move of `papers file mv`: a file or a folder.
#[derive(Debug, Clone)]
pub struct Move {
    pub src: String,
    /// the real path before the move
    pub real: String,
    pub target: String,
    pub isdir: bool,
}

impl Move {
    fn new(src: &str, target: String) -> Move {
        Move {
            src: src.to_string(),
            real: realpath(src),
            target,
            isdir: isdir(src) && !islink(src),
        }
    }

    /// Where the file of real path `real` is after the move, or `None` if it does not move.
    pub fn new_path(&self, real: &str) -> Option<String> {
        if real == self.real {
            return Some(self.target.clone());
        }
        let prefix = format!("{}/", self.real.trim_end_matches('/'));
        if self.isdir && real.starts_with(&prefix) {
            return Some(join(&self.target, &real[prefix.len()..]));
        }
        None
    }
}

/// The moves of `papers file mv SRC... DEST`, following the rules of `git mv`. All checks are
/// done here, before anything moves: a source file that no entry links to is an error; a source
/// folder must contain a linked file (the whole folder moves); with several sources, the
/// destination must be an existing folder; a destination that exists as a file is an error,
/// unless `force`; and a source folder must not contain one of the `protected` files.
pub fn plan_moves(
    library: &Library,
    sources: &[String],
    dest: &str,
    force: bool,
    protected: &[Option<String>],
) -> Result<Vec<Move>> {
    let links = linked_files(library);
    let dest = abspath(dest);
    let mut errors: Vec<String> = Vec::new();
    if sources.len() > 1 && !isdir(&dest) {
        return Err(Error::exit(format!(
            "{dest}: with several sources, the destination must be an existing folder"
        )));
    }
    let mut moves: Vec<Move> = Vec::new();
    for src in sources {
        let src = abspath(src);
        if !lexists(&src) {
            errors.push(format!("{src}: does not exist"));
            continue;
        }
        let target = if isdir(&dest) {
            join(&dest, &basename(&src))
        } else {
            dest.clone()
        };
        let m = Move::new(&src, target);
        if m.isdir {
            if !links.keys().any(|real| m.new_path(real).is_some()) {
                errors.push(format!("{src}: no entry links to a file in this folder"));
                continue;
            }
            for file in protected.iter().flatten().filter(|f| !f.is_empty()) {
                if m.new_path(&realpath(file)).is_some() {
                    errors.push(format!(
                        "{src}: contains {file}, which cannot be moved with `papers file mv`"
                    ));
                }
            }
        } else if !links.contains_key(&m.real) {
            errors.push(format!("{src}: no entry links to this file"));
            continue;
        }
        let target = m.target.clone();
        if realpath(&target) == m.real {
            errors.push(format!("{src}: source and destination are the same"));
        } else if m.isdir && is_inside(&target, &src) {
            errors.push(format!("{src}: cannot move a folder inside itself"));
        } else if lexists(&target) {
            if isdir(&target) {
                errors.push(format!("{target}: destination exists"));
            } else if m.isdir {
                errors.push(format!("{target}: destination exists as a file"));
            } else if !force {
                errors.push(format!(
                    "{target}: destination exists (-f/--force replaces it)"
                ));
            }
        }
        moves.push(m);
    }
    for (i, a) in moves.iter().enumerate() {
        for b in &moves[i + 1..] {
            if a.real == b.real {
                errors.push(format!("{}: given twice", a.src));
            } else if a.new_path(&b.real).is_some() || b.new_path(&a.real).is_some() {
                errors.push(format!(
                    "{}: inside another source",
                    if a.new_path(&b.real).is_some() {
                        &b.src
                    } else {
                        &a.src
                    }
                ));
            } else if realpath(&a.target) == realpath(&b.target) {
                errors.push(format!(
                    "{}, {}: same destination {}",
                    a.src, b.src, a.target
                ));
            }
        }
    }
    if !errors.is_empty() {
        return Err(Error::exit(format!(
            "{}\nnothing was moved",
            errors.join("\n")
        )));
    }
    Ok(moves)
}

/// `(key, links, real paths of the links)` of each entry, taken before anything moves.
type Snapshot = Vec<(String, Vec<String>, Vec<String>)>;

fn link_snapshot(library: &Library) -> Snapshot {
    library
        .entries()
        .map(|e| {
            let files = library.files(&e.key);
            let reals = files.iter().map(|f| realpath(f)).collect();
            (e.key.clone(), files, reals)
        })
        .collect()
}

/// The new file links of the entries after the moves: `(key, old files, new files)` for the
/// entries that change.
fn moved_links(snapshot: &Snapshot, moves: &[Move]) -> Vec<(String, Vec<String>, Vec<String>)> {
    let mut changes = Vec::new();
    for (key, files, reals) in snapshot {
        let new: Vec<String> = files
            .iter()
            .zip(reals)
            .map(|(file, real)| {
                moves
                    .iter()
                    .find_map(|m| m.new_path(real))
                    .unwrap_or_else(|| file.clone())
            })
            .collect();
        if new != *files {
            changes.push((key.clone(), files.clone(), new));
        }
    }
    changes
}

/// A message for an error of the system, as Python words an `OSError` of a rename.
fn os_error(error: &std::io::Error, src: &str, dst: &str) -> String {
    let message = crate::config::machine::io_message(error);
    match error.raw_os_error() {
        Some(code) => format!(
            "[Errno {code}] {message}: {} -> {}",
            py_repr_str(src),
            py_repr_str(dst)
        ),
        None => message,
    }
}

/// Move the files and folders; the moves done, and the error that stopped them (`execute_moves`).
fn execute_moves(
    session: &mut Session,
    moves: &[Move],
    force: bool,
) -> (Vec<Move>, Option<String>) {
    let mut done = Vec::new();
    for m in moves {
        let parent = dirname(&m.target);
        if !parent.is_empty()
            && !isdir(&parent)
            && let Err(e) = std::fs::create_dir_all(&parent)
        {
            return (done, Some(os_error(&e, &m.src, &m.target)));
        }
        if force
            && isfile(&m.target)
            && !m.isdir
            && let Err(e) = session.remove_file(&m.target)
        {
            return (done, Some(e.message().to_string()));
        }
        log::info(&format!("mv {} {}", m.src, m.target));
        if let Err(e) = crate::history::restore::move_path(&m.src, &m.target) {
            return (done, Some(os_error(&e, &m.src, &m.target)));
        }
        session.record_operation(OpKind::Moved, &m.src, &m.target);
        done.push(m.clone());
    }
    (done, None)
}

/// What `papers file mv` needs of the command line.
#[derive(Debug, Clone, Default)]
pub struct MvOptions {
    pub force: bool,
    /// `--filesdir` was given: the files directory setting does not follow the move.
    pub filesdir_given: bool,
}

/// `papers file mv SRC... DEST`: move files or folders like `mv`, with the rules of `git mv`, and
/// update the links of the entries to the moved files. Moving the files directory also updates
/// the `filesdir` setting.
pub fn move_files(
    session: &mut Session,
    sources: &[String],
    dest: &str,
    options: &MvOptions,
) -> Result<()> {
    let protected = [session.config.bibtex.clone(), session.config.file.clone()];
    let moves = plan_moves(&session.library, sources, dest, options.force, &protected)?;
    let snapshot = link_snapshot(&session.library);

    // -f: a replaced destination that entries link to
    let links = linked_files(&session.library);
    for m in &moves {
        let replaced = if !m.isdir && isfile(&m.target) {
            links.get(&realpath(&m.target))
        } else {
            None
        };
        if let Some(keys) = replaced {
            let one = keys.len() == 1;
            log::warning(&format!(
                "-f replaces {}, which {} link{} to: {} to the moved file",
                m.target,
                keys.join(", "),
                if one { "s" } else { "" },
                if one {
                    "that link leads"
                } else {
                    "these links lead"
                }
            ));
        }
    }

    let new_filesdir = |session: &Session, moves: &[Move]| -> Option<String> {
        let filesdir = session.config.filesdir.as_deref()?;
        if options.filesdir_given {
            return None;
        }
        let real = realpath(filesdir);
        moves.iter().find_map(|m| m.new_path(&real))
    };

    if session.dry_run() {
        for m in &moves {
            session
                .say
                .line(format!("DRYRUN: mv {} {}", m.src, m.target));
        }
        for (key, old, new) in moved_links(&snapshot, &moves) {
            for (a, b) in old.iter().zip(&new) {
                if a != b {
                    session.say.line(format!("DRYRUN: {key}: {a} -> {b}"));
                }
            }
        }
        if let Some(filesdir) = new_filesdir(session, &moves) {
            session.say.line(format!("DRYRUN: filesdir = {filesdir}"));
        }
        return Ok(());
    }

    let (done, error) = execute_moves(session, &moves, options.force);
    let changes = moved_links(&snapshot, &done);
    for (key, _, new) in &changes {
        session.library.set_files(key, new)?;
    }
    let committed = session.commit()?;
    if let Some(filesdir) = new_filesdir(session, &done) {
        session.config.filesdir = Some(filesdir.clone());
        if let Some(file) = session.config.file.clone() {
            session.config.save()?;
            let shown =
                crate::config::paths::relpath(&realpath(&filesdir), &session.config.configdir());
            session.say.line(format!("filesdir = {shown} (in {file})"));
        }
    }
    session.say.line(format!(
        "moved {} of {} file(s) or folder(s), {} entries updated",
        done.len(),
        moves.len(),
        changes.len()
    ));
    for line in push_note_lines(&session.library, &committed.notes) {
        session.say.line(line);
    }
    match error {
        Some(error) => Err(Error::exit(format!(
            "{error}: the links of the files moved before were saved"
        ))),
        None => Ok(()),
    }
}

// check, fix
// ----------

/// The `--search` folders of `papers file check`: their files by name. The folders are searched
/// recursively, hidden files and folders skipped; a file found twice (folders inside one another,
/// symbolic links) counts once.
pub struct SearchFolders {
    folders: Vec<String>,
    names: HashMap<String, Vec<(String, String)>>,
}

impl SearchFolders {
    pub fn new(folders: &[String]) -> Result<SearchFolders> {
        let tops: Vec<String> = folders.iter().map(|d| abspath(&expanduser(d))).collect();
        let mut found = SearchFolders {
            folders: tops.clone(),
            names: HashMap::new(),
        };
        for (top, given) in tops.iter().zip(folders) {
            if !isdir(top) {
                return Err(Error::exit(format!("--search {given}: not a folder")));
            }
            found.walk(top);
        }
        Ok(found)
    }

    fn walk(&mut self, root: &str) {
        let Ok(read) = std::fs::read_dir(root) else {
            return;
        };
        let (mut dirs, mut files): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if isdir(&join(root, &name)) {
                dirs.push(name);
            } else {
                files.push(name);
            }
        }
        dirs.retain(|d| !d.starts_with('.'));
        dirs.sort();
        files.sort();
        for name in files {
            if name.starts_with('.') {
                continue;
            }
            let path = join(root, &name);
            let real = realpath(&path);
            let list = self.names.entry(name).or_default();
            if !list.iter().any(|(r, _)| *r == real) {
                list.push((real, path));
            }
        }
        for d in dirs {
            let path = join(root, &d);
            if !islink(&path) {
                self.walk(&path);
            }
        }
    }

    /// The files of this name.
    pub fn find(&self, name: &str) -> Vec<String> {
        self.names
            .get(name)
            .map(|l| l.iter().map(|(_, p)| p.clone()).collect())
            .unwrap_or_default()
    }

    /// The `--search` folder of a file found.
    pub fn folder(&self, file: &str) -> String {
        self.folders
            .iter()
            .find(|d| is_inside(file, d))
            .cloned()
            .unwrap_or_default()
    }
}

/// The path of a file link as saved by Mendeley, repaired: latex symbols converted, and the
/// leading `/` restored when it was omitted (`home/...` instead of `/home/...`)
/// (`fix_mendeley_path`).
fn fix_mendeley_path(file: &str) -> String {
    let mut file = latex_to_unicode(file, false);
    let directory = dirname(&file);
    let candidate = format!("/{file}");
    if !file.starts_with('/')
        && !directory.is_empty()
        && !exists(&directory)
        && exists(&dirname(&candidate))
    {
        file = candidate;
    }
    file
}

/// For each link of an entry: the repaired path of a broken link saved by Mendeley (missing
/// leading `/`, latex symbols), if that path exists, else `None` (`repaired_paths`).
fn repaired_paths(library: &Library, e: &Entry) -> Vec<Option<String>> {
    let written = parse_file(val(e, "file"));
    entry::files(e, Some(&library.relative_to))
        .into_iter()
        .zip(written)
        .map(|(file, link)| {
            if exists(&file) {
                return None;
            }
            let mut repaired = fix_mendeley_path(&file);
            if !exists(&repaired) && !is_abs(&link) {
                let candidate = fix_mendeley_path(&link);
                if is_abs(&candidate) {
                    repaired = candidate;
                }
            }
            if repaired == file || !exists(&repaired) {
                None
            } else {
                Some(repaired)
            }
        })
        .collect()
}

/// The broken links of an entry, without those that a repair of the path fixes (`broken_links`).
pub(crate) fn broken_links(library: &Library, e: &Entry) -> Vec<String> {
    entry::files(e, Some(&library.relative_to))
        .into_iter()
        .zip(repaired_paths(library, e))
        .filter(|(f, repaired)| repaired.is_none() && !exists(f))
        .map(|(f, _)| f)
        .collect()
}

/// The number of broken links of the entries of the library (`broken_links` of each).
pub fn broken_link_count(library: &Library) -> usize {
    library
        .entries()
        .map(|e| broken_links(library, e).len())
        .sum()
}

fn checksum(file: &str) -> String {
    crate::merge::checksum_hex(file).unwrap_or_default()
}

/// The DOI of a PDF against the entry's (`entry_filecheck_metadata`): the problem, if any.
fn pdf_doi_problem(e: &Entry, file: &str, queries: &Queries) -> Option<String> {
    let key = &e.key;
    let Some(entry_doi) = e.get("doi") else {
        return Some(format!("{key}: no doi, skip PDF parsing"));
    };
    let parsed = queries
        .fetcher()
        .and_then(|fetcher| extract_pdf_doi(&fetcher, file, false));
    let Ok(doi) = parsed else {
        return Some(format!("{key}: failed to parse doi: \"{file}\""));
    };
    if !isvaliddoi(&doi) {
        return Some(format!("{key}: invalid parsed doi: {doi}"));
    }
    if doi.to_lowercase() != entry_doi.to_lowercase() {
        return Some(format!(
            "{key}: doi: entry <=> pdf : {} <=> {}",
            entry_doi.to_lowercase(),
            doi.to_lowercase()
        ));
    }
    None
}

/// The problems of the files of an entry, as lines to print (`check_entry_files`): broken links,
/// links that a repair of the path would fix (links saved by Mendeley), the same file linked
/// twice, identical files (same checksum), and with `metadata` a DOI of the PDF that differs
/// from the entry's.
///
/// `fix`: apply the safe repairs (repaired paths; remove the second link of the same or an
/// identical file, the file itself is kept); with a dry run, the lines of these repairs start
/// with `DRYRUN`. `skip`: broken links not to report. `repairs` receives the lines of the
/// repairs (done with `fix`, possible otherwise).
#[allow(clippy::too_many_arguments)]
fn check_entry_files(
    library: &mut Library,
    key: &str,
    metadata: bool,
    fix: bool,
    skip: &HashSet<String>,
    repairs: &mut Vec<String>,
    dry_run: bool,
    queries: &Queries,
) -> Result<Vec<String>> {
    let Some(e) = library.get(key).cloned() else {
        return Ok(Vec::new());
    };
    let key = e.key.clone();
    let files = library.files(&key);
    let repaired = repaired_paths(library, &e);
    let mut newfiles: Vec<String> = Vec::new();
    let mut issues: Vec<String> = Vec::new();
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut hashes: HashMap<String, String> = HashMap::new();
    let mut repair = |line: String, done: &str, issues: &mut Vec<String>| {
        let line = format!(
            "{}{line}{}",
            if fix && dry_run { "DRYRUN: " } else { "" },
            if fix {
                format!(" ({done})")
            } else {
                String::new()
            }
        );
        issues.push(line.clone());
        repairs.push(line);
    };
    for (file, repaired) in files.iter().zip(repaired) {
        let mut file = file.clone();
        if !exists(&file) {
            match repaired {
                Some(repaired) => {
                    repair(
                        format!("{key}: path can be repaired: {file} -> {repaired}"),
                        "fixed",
                        &mut issues,
                    );
                    if fix {
                        file = repaired;
                    }
                }
                None => {
                    if !skip.contains(&file) {
                        issues.push(format!(
                            "{key}: broken link: {file} (remove it with `papers file detach --broken --key {key}`)"
                        ));
                    }
                    newfiles.push(file);
                    continue;
                }
            }
        }
        let real = realpath(&file);
        if let Some(first) = seen.get(&real) {
            repair(
                format!("{key}: the same file is linked twice: {first} and {file}"),
                "second link removed",
                &mut issues,
            );
            continue;
        }
        seen.insert(real, file.clone());
        if files.len() > 1 && isfile(&file) {
            let hash = checksum(&file);
            if let Some(first) = hashes.get(&hash) {
                repair(
                    format!("{key}: identical files: {first} and {file}"),
                    "second link removed",
                    &mut issues,
                );
                if fix {
                    continue;
                }
            }
            hashes.entry(hash).or_insert_with(|| file.clone());
        }
        if metadata
            && file.to_lowercase().ends_with(".pdf")
            && let Some(problem) = pdf_doi_problem(&e, &file, queries)
        {
            issues.push(problem);
        }
        newfiles.push(file);
    }
    if fix && newfiles != files {
        library.set_files(&key, &newfiles)?;
    }
    Ok(issues)
}

/// The problems of the files of an entry, as `papers filecheck` reports them (the deprecated
/// `--metadata-check`, `--hash-check`, `--fix-mendeley`): see [`check_entry_files`].
pub fn check_entry_lines(
    session: &mut Session,
    key: &str,
    metadata: bool,
    fix: bool,
    queries: &Queries,
) -> Result<Vec<String>> {
    let dry_run = session.dry_run();
    check_entry_files(
        &mut session.library,
        key,
        metadata,
        fix,
        &HashSet::new(),
        &mut Vec::new(),
        dry_run,
        queries,
    )
}

/// The names to look for in the `--search` folders for the broken link `link` of an entry
/// (`rest`: its broken links not found elsewhere): the link's file name, then the name that the
/// library's name template gives the entry, with the link's extension (only when no other broken
/// link has that extension: the template names one file).
fn search_names(nameformat: &Format, e: &Entry, link: &str, rest: &[String]) -> Vec<String> {
    let mut names = vec![basename(link)];
    let ext = splitext_ext(link);
    if rest
        .iter()
        .filter(|f| splitext_ext(f).to_lowercase() == ext.to_lowercase())
        .count()
        == 1
    {
        match nameformat.render(e) {
            Ok(name) => names.push(basename(&fit_name(&format!("{name}{ext}")))),
            Err(error) => log::debug(&format!("{}: no template name: {}", e.key, error.0)),
        }
    }
    names
}

/// Where the file found for a broken link comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// the pool of the library
    Pool,
    /// a library given with `--pool PATH` (how it is shown)
    Library(String),
    /// a `--search` folder
    Folder(String),
}

impl Source {
    pub(crate) fn describe(&self) -> String {
        match self {
            Source::Pool => "from the pool".to_string(),
            Source::Library(shown) => format!("from the library {shown}"),
            Source::Folder(folder) => format!("found in {}", short_home(folder)),
        }
    }
}

/// The files found for the broken links of an entry: `{link: [files]}` in order, and where each
/// comes from.
#[derive(Debug, Default)]
pub(crate) struct Plan {
    pub(crate) key: String,
    pub(crate) found: Vec<(String, Vec<String>)>,
    pub(crate) sources: HashMap<String, Source>,
}

/// The files found for the broken links of the entries, without parsing any file: the pool of the
/// library first, then the libraries of `--pool PATH` (`others`, see [`Exchange::pool_relinks`]), then
/// the `--search` folders (by the link's file name, else by the template name of the entry). Returns the
/// plans, the links that several files match (`(key, link, reason)`), and the number of broken links
/// (`plan_relinks`).
#[allow(clippy::type_complexity)]
fn plan_relinks(
    session: &mut Session,
    keys: &[String],
    search: &SearchFolders,
    nameformat: &Format,
    others: &[PoolIx],
) -> Result<(Vec<Plan>, Vec<(String, String, String)>, usize)> {
    let (mut plans, mut unclear, mut count): (Vec<Plan>, Vec<(String, String, String)>, usize) =
        (Vec::new(), Vec::new(), 0);
    let own = if session.config.shallow() {
        session.exchange().pool(true)
    } else {
        None
    };
    let mut pools: Vec<(PoolIx, Source)> = own.map(|p| (p, Source::Pool)).into_iter().collect();
    if !others.is_empty() {
        let exchange = session.exchange();
        for p in others {
            if Some(*p) != own {
                pools.push((*p, Source::Library(exchange.pools.at(*p).shown.clone())));
            }
        }
    }
    // real path of a file found in a --search folder: [(plan, link)]
    let mut claims: Vec<(String, Vec<(usize, String)>)> = Vec::new();
    for key in keys {
        let Some(e) = session.library.get(key).cloned() else {
            continue;
        };
        let broken = broken_links(&session.library, &e);
        if broken.is_empty() {
            continue;
        }
        count += broken.len();
        let mut plan = Plan {
            key: e.key.clone(),
            ..Plan::default()
        };
        for (pool, source) in &pools {
            let todo: Vec<String> = broken
                .iter()
                .filter(|f| !plan.found.iter().any(|(l, _)| l == *f))
                .cloned()
                .collect();
            let exchange = session.exchange.as_mut().expect("the exchange");
            for (link, files) in exchange.pool_relinks(&session.library, &e, *pool, &todo)? {
                plan.sources.insert(link.clone(), source.clone());
                plan.found.push((link, files));
            }
        }
        let linked: HashSet<String> = session
            .library
            .files(&e.key)
            .into_iter()
            .chain(plan.found.iter().flat_map(|(_, fs)| fs.clone()))
            .filter(|f| exists(f))
            .map(|f| realpath(&f))
            .collect();
        let rest: Vec<String> = broken
            .iter()
            .filter(|f| !plan.found.iter().any(|(l, _)| l == *f))
            .cloned()
            .collect();
        for link in &rest {
            let mut candidates: Vec<String> = Vec::new();
            for name in search_names(nameformat, &e, link, &rest) {
                candidates = search
                    .find(&name)
                    .into_iter()
                    .filter(|f| !linked.contains(&realpath(f)))
                    .collect();
                if !candidates.is_empty() {
                    break;
                }
            }
            if candidates.len() > 1 {
                unclear.push((
                    e.key.clone(),
                    link.clone(),
                    format!(
                        "several files match in the --search folders: {}",
                        candidates
                            .iter()
                            .map(|f| short_home(f))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
            } else if let Some(first) = candidates.first() {
                let real = realpath(first);
                plan.sources
                    .insert(link.clone(), Source::Folder(search.folder(first)));
                plan.found.push((link.clone(), candidates.clone()));
                let at = plans.len();
                match claims.iter_mut().find(|(r, _)| *r == real) {
                    Some((_, c)) => c.push((at, link.clone())),
                    None => claims.push((real, vec![(at, link.clone())])),
                }
            }
        }
        plans.push(plan);
    }
    // a file found for the broken links of several entries goes to none of them
    for (real, links) in &claims {
        if links.len() > 1 {
            let keys: Vec<&str> = links
                .iter()
                .map(|(at, _)| plans[*at].key.as_str())
                .collect();
            let keys = keys.join(", ");
            for (at, link) in links {
                unclear.push((
                    plans[*at].key.clone(),
                    link.clone(),
                    format!("{} matches the broken links of {keys}", short_home(real)),
                ));
            }
            for (at, link) in links {
                plans[*at].found.retain(|(l, _)| l != link);
                plans[*at].sources.remove(link);
            }
        }
    }
    Ok((
        plans.into_iter().filter(|p| !p.found.is_empty()).collect(),
        unclear,
        count,
    ))
}

fn links_count(n: usize) -> String {
    format!("{n} link{}", if n == 1 { "" } else { "s" })
}

/// What `papers file check` and `papers file fix` (also `papers list --fix-files`) are asked.
#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    /// `--search DIR`: where to look for the files of the broken links.
    pub search: Vec<String>,
    /// `--metadata`: compare the DOI of the PDFs with the entry's.
    pub metadata: bool,
    /// `--move`: the files of the `--search` folders are moved.
    pub move_: bool,
    /// `--pool PATH`: the libraries to take the files of the broken links from (also of a library with a pool).
    pub pools: Vec<String>,
    /// The command that applies the repairs, for the hint of `check`.
    pub fix_command: String,
}

/// What the repair of the broken links of entries found (`relink_broken_links`).
pub(crate) struct Relinked {
    /// The files found for the broken links of each entry (the new links, once placed), and where
    /// each comes from.
    pub(crate) relinks: Vec<Plan>,
    /// The links that several files match: `(key, link, reason)`.
    pub(crate) unclear: Vec<(String, String, String)>,
    /// The files that the history brings back to the place of a broken link: `(key, link, when)`.
    pub(crate) restores: Vec<(String, String, String)>,
    /// The number of broken links.
    pub(crate) count: usize,
}

/// The repair of the broken links of the entries by `papers file fix` (also `papers file fetch`,
/// before it downloads; `relink_broken_links`): the links to pool files repaired when a shallow
/// library was loaded, the files found by [`plan_relinks`] (the pool, the libraries `others` of
/// `--pool PATH`, the folders `search` of `--search`), then the files that a command deleted from
/// the place of the link, kept by the history. With `place`, the files found are placed as `papers
/// file attach` does (with the files mode `mode`; `move_`: the files of the `--search` folders are
/// moved), and those of the history copied back.
#[allow(clippy::too_many_arguments)]
pub(crate) fn relink_broken_links(
    session: &mut Session,
    keys: &[String],
    search: &[String],
    nameformat: &Format,
    others: &[PoolIx],
    mode: Option<Mode>,
    place: bool,
    move_: bool,
) -> Result<Relinked> {
    let search = SearchFolders::new(search)?;
    // the links to pool files repaired when a shallow library is loaded (already linked), then the others
    let loaded: Vec<Plan> = session
        .repaired
        .iter()
        .filter(|(key, _)| keys.contains(key))
        .map(|(key, found)| Plan {
            key: key.clone(),
            found: found.clone(),
            sources: found
                .iter()
                .map(|(l, _)| (l.clone(), Source::Pool))
                .collect(),
        })
        .collect();
    let (plans, unclear, count) = plan_relinks(session, keys, &search, nameformat, others)?;
    let count = count + loaded.iter().map(|p| p.found.len()).sum::<usize>();

    let mut relinks: Vec<Plan> = loaded;
    for mut plan in plans {
        if place {
            // placed as `papers file attach` does, in place of the broken links
            let old = session.library.files(&plan.key);
            let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
            let mut i = 0;
            for f in &old {
                let n = plan
                    .found
                    .iter()
                    .find(|(l, _)| l == f)
                    .map_or(1, |(_, fs)| fs.len());
                positions.insert(f.clone(), (i, i + n));
                i += n;
            }
            let from_pool = |link: &String| plan.sources.get(link) == Some(&Source::Pool);
            let pool_files: Vec<String> = plan
                .found
                .iter()
                .filter(|(l, _)| from_pool(l))
                .flat_map(|(_, fs)| fs.clone())
                .collect();
            let copied: Vec<String> = plan
                .found
                .iter()
                .filter(|(l, _)| {
                    !from_pool(l)
                        && !(move_ && matches!(plan.sources.get(l), Some(Source::Folder(_))))
                })
                .flat_map(|(_, fs)| fs.clone())
                .collect();
            let attach = Attach {
                mode: mode.unwrap_or(Mode::Move),
                replace: plan.found.clone(),
                copied,
                pool_files,
            };
            attach_files(session, &plan.key, &[], &attach)?;
            let newfiles = session.library.files(&plan.key);
            plan.found = plan
                .found
                .iter()
                .map(|(link, _)| {
                    let (a, b) = positions[link];
                    (
                        link.clone(),
                        newfiles
                            .get(a..b.min(newfiles.len()))
                            .map(<[String]>::to_vec)
                            .unwrap_or_default(),
                    )
                })
                .collect();
        }
        relinks.push(plan);
    }

    // last, the history: a file that a command deleted from the place of the link comes back there
    // (a copy: the history keeps the file, and the step of the fix keeps it too, see `deleted`)
    let mut found_links: HashSet<String> =
        unclear.iter().map(|(_, link, _)| link.clone()).collect();
    found_links.extend(
        relinks
            .iter()
            .flat_map(|p| p.found.iter().map(|(l, _)| l.clone())),
    );
    let mut restores: Vec<(String, String, String)> = Vec::new();
    let history = History::open(&mut session.config.clone(), false, true)?;
    if let Some(history) = history {
        for key in keys {
            let Some(e) = session.library.get(key) else {
                continue;
            };
            for link in broken_links(&session.library, e) {
                let Some((stored, step)) = (!found_links.contains(&link))
                    .then(|| history.deleted_file(&link))
                    .flatten()
                else {
                    continue;
                };
                let when = match &step {
                    Some(step) => format!("deleted by step {}", short(step)),
                    None => "deleted, its checksum known".to_string(),
                };
                if place {
                    if let Some(parent) = std::path::Path::new(&link).parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    crate::history::restore::copy_file(&stored, &link)
                        .map_err(|e| Error::other(format!("{link}: {e}")))?;
                    session.record_operation(OpKind::Copied, &stored, &link); // undo removes it again
                }
                restores.push((key.clone(), link, when));
            }
        }
    }
    Ok(Relinked {
        relinks,
        unclear,
        restores,
        count,
    })
}

/// `papers file check` (`fix` false: report the problems of the file links, change nothing) and
/// `papers file fix` (`fix`: apply the safe repairs: repaired paths, the second link of a file
/// linked twice or of identical files removed, broken links relinked to the files found in the
/// `--search` folders; with a dry run, list them). A file found is copied (a hard link when
/// possible) into the files directory, never moved, except with `--move` a file of a `--search`
/// folder; in link mode, or without files directory, it is linked where it is.
pub fn check_entries_files(
    session: &mut Session,
    options: &CheckOptions,
    fix: bool,
    keys: &[String],
    queries: &Queries,
) -> Result<()> {
    let dry_run = session.dry_run();
    let place = fix && !dry_run;
    // the library's own name template (a shallow library places files with the pool's)
    let nameformat = session.own_nameformat();
    let mut others: Vec<PoolIx> = Vec::new();
    for path in &options.pools {
        others.push(session.exchange().get_library(path)?);
    }
    let mode = if fix {
        Some(files_mode(false, options.move_, false, &session.config)?)
    } else {
        None
    };
    let Relinked {
        relinks,
        unclear,
        restores,
        count,
    } = relink_broken_links(
        session,
        keys,
        &options.search,
        &nameformat,
        &others,
        mode,
        place,
        options.move_,
    )?;

    let mut lines: Vec<String> = Vec::new();
    for plan in &relinks {
        let shown: Vec<(String, String)> = plan
            .found
            .iter()
            .filter(|(_, fs)| !fs.is_empty())
            .map(|(link, fs)| {
                (
                    link.clone(),
                    fs.iter()
                        .map(|f| short_home(f))
                        .collect::<Vec<_>>()
                        .join(", "),
                )
            })
            .collect();
        let source = |link: &str| plan.sources[link].describe();
        if place {
            lines.push(format!(
                "{}: relinked to {}",
                plan.key,
                shown
                    .iter()
                    .map(|(link, files)| format!("{files} ({})", source(link)))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        } else if fix {
            lines.extend(shown.iter().map(|(link, files)| {
                format!(
                    "DRYRUN: {}: relink {} -> {files} ({})",
                    plan.key,
                    short_home(link),
                    source(link)
                )
            }));
        } else {
            lines.extend(shown.iter().map(|(link, files)| {
                format!(
                    "{}: broken link: {} -> {files} ({})",
                    plan.key,
                    short_home(link),
                    source(link)
                )
            }));
        }
    }
    for (key, link, reason) in &unclear {
        lines.push(format!(
            "{key}: broken link: {}: {reason} (not relinked)",
            short_home(link)
        ));
    }
    for (key, link, when) in &restores {
        lines.push(if place {
            format!("{key}: {}: restored from the history ({when})", short_home(link))
        } else if fix {
            format!("DRYRUN: {key}: restore {} from the history ({when})", short_home(link))
        } else {
            format!(
                "{key}: broken link: {} -> in the history ({when})",
                short_home(link)
            )
        });
    }
    let restored = restores.len();

    let relinked: usize = relinks.iter().map(|p| p.found.len()).sum::<usize>() + restored;
    let mut issues: Vec<String> = lines.clone();
    let mut repairs: Vec<String> = Vec::new();
    let mut skip: HashSet<String> = unclear.iter().map(|(_, link, _)| link.clone()).collect();
    if !place {
        skip.extend(
            relinks
                .iter()
                .flat_map(|p| p.found.iter().map(|(l, _)| l.clone())),
        );
        skip.extend(restores.iter().map(|(_, link, _)| link.clone()));
    }
    for key in keys {
        issues.extend(check_entry_files(
            &mut session.library,
            key,
            options.metadata,
            fix,
            &skip,
            &mut repairs,
            dry_run,
            queries,
        )?);
    }
    for line in &issues {
        session.say.line(line);
    }
    if count > 0 {
        let left = count - relinked;
        let detach = if left > 0 {
            " (papers file detach --broken removes them)"
        } else {
            ""
        };
        if fix {
            session.say.line(format!(
                "{}{} relinked, {left} still broken{detach}",
                if place { "" } else { "DRYRUN: " },
                links_count(relinked)
            ));
        } else {
            session.say.line(format!(
                "{} can be relinked, {left} without a file found{detach}",
                links_count(relinked)
            ));
        }
    }
    let entries = keys.len();
    if place {
        // the issues left: neither relinked nor repaired
        let left = issues.len() - (lines.len() - unclear.len()) - repairs.len();
        if left > 0 {
            session.say.line(format!(
                "{left} issue(s) left in the files of {entries} entries"
            ));
        } else if issues.is_empty() {
            session
                .say
                .line(format!("no issue found in the files of {entries} entries"));
        }
    } else if !issues.is_empty() {
        session.say.line(format!(
            "{} issue(s) in the files of {entries} entries",
            issues.len()
        ));
    } else {
        session
            .say
            .line(format!("no issue found in the files of {entries} entries"));
    }
    if !fix && relinked + repairs.len() > 0 {
        session.say.line(format!(
            "{} can be repaired: `{}`",
            links_count(relinked + repairs.len()),
            options.fix_command
        ));
    }
    if fix && !issues.is_empty() {
        let committed = session.commit()?;
        for line in push_note_lines(&session.library, &committed.notes) {
            session.say.line(line);
        }
    }
    Ok(())
}

// clean
// -----

/// `papers file clean`: remove the files of the files directory that no entry links to, and the
/// folders of versions before 2.19 (`<name>/` with a hidden `.<name>.bib`) that no entry links
/// to. Subfolders are scanned too (name templates may contain `/`); hidden files and folders and
/// `.bib` files are skipped. `ignore_files` are never removed. Returns the files removed (with
/// those of the removed folders, except their hidden `.<name>.bib`).
pub fn clean_filesdir(
    session: &mut Session,
    interactive: bool,
    ignore_files: &[String],
) -> Result<Vec<String>> {
    let Some(filesdir) = session.library.filesdir.clone() else {
        return Err(Error::other("filesdir is not defined, cannot clean"));
    };
    let mut allfiles: HashSet<String> = HashSet::new();
    let mut allfolders: HashSet<String> = HashSet::new();
    for e in session.library.entries() {
        let files = session.library.files(&e.key);
        allfolders.extend(files.iter().map(|f| realpath(&dirname(f))));
        allfiles.extend(files.iter().map(|f| realpath(f)));
    }
    allfiles.extend(ignore_files.iter().map(|f| realpath(f)));
    let mut removed: Vec<String> = Vec::new();
    clean_folder(
        session,
        &filesdir,
        &filesdir,
        interactive,
        &allfiles,
        &allfolders,
        &mut removed,
    )?;
    Ok(removed)
}

fn hidden_bibtex(direc: &str) -> String {
    join(direc, &format!(".{}.bib", basename(direc)))
}

fn clean_folder(
    session: &mut Session,
    filesdir: &str,
    root: &str,
    interactive: bool,
    allfiles: &HashSet<String>,
    allfolders: &HashSet<String>,
    removed: &mut Vec<String>,
) -> Result<()> {
    let Ok(read) = std::fs::read_dir(root) else {
        return Ok(());
    };
    let (mut dirs, mut files): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if isdir(&join(root, &name)) {
            dirs.push(name);
        } else {
            files.push(name);
        }
    }
    dirs.sort();
    files.sort();
    let dry = session.dry_run();
    // Y(es) or N(o) for all files in this folder
    let mut answer_all: Option<String> = None;
    for file in &files {
        let path = abspath(&join(root, file));
        if file.starts_with('.') || file.ends_with(".bib") {
            continue;
        }
        if allfiles.contains(&realpath(&path)) {
            continue;
        }
        let name = crate::config::paths::relpath(&path, filesdir);
        if interactive {
            let answer = match &answer_all {
                None => session.input(&format!(
                    "File: {name} not in library. Remove? [y: yes, n: no, Y: all, N: none] "
                ))?,
                Some(a) => a.clone(),
            };
            let answer = if answer == "Y" || answer == "N" {
                let lower = answer.to_lowercase();
                answer_all = Some(lower.clone());
                lower
            } else {
                answer
            };
            if answer.to_lowercase() == "y" {
                remove_unlinked(session, &path, false)?;
                removed.push(path);
            }
        } else {
            if !dry {
                session
                    .say
                    .line(format!("{name}: removed, not in the library"));
            }
            remove_unlinked(session, &path, false)?;
            removed.push(path);
        }
    }
    let mut subfolders: Vec<String> = Vec::new();
    for direc in &dirs {
        if direc.starts_with('.') {
            continue;
        }
        let direcpath = abspath(&join(root, direc));
        // folders of an entry with several files (versions before 2.19): removed as a whole
        // when no entry links to them, never scanned
        if !exists(&hidden_bibtex(&direcpath)) {
            subfolders.push(direcpath);
            continue;
        }
        if !allfolders.contains(&realpath(&direcpath)) {
            let name = crate::config::paths::relpath(&direcpath, filesdir);
            let answer = if interactive {
                session.input(&format!("Folder: {name} not in library. Remove ? [Y/n] "))?
            } else {
                if !dry {
                    session
                        .say
                        .line(format!("{name}: folder removed, not in the library"));
                }
                "y".to_string()
            };
            if answer.to_lowercase() == "y" {
                removed.extend(files_below(&direcpath));
                remove_unlinked(session, &direcpath, true)?;
            }
        }
    }
    for direcpath in subfolders {
        if !islink(&direcpath) {
            clean_folder(
                session,
                filesdir,
                &direcpath,
                interactive,
                allfiles,
                allfolders,
                removed,
            )?;
        }
    }
    Ok(())
}

/// The files below a folder, but the hidden ones (`os.walk`, sorted by folder and name).
fn files_below(direc: &str) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(read) = std::fs::read_dir(direc) else {
        return found;
    };
    let (mut dirs, mut files): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if isdir(&join(direc, &name)) {
            dirs.push(name);
        } else {
            files.push(name);
        }
    }
    files.sort();
    found.extend(
        files
            .iter()
            .filter(|f| !f.starts_with('.'))
            .map(|f| join(direc, f)),
    );
    dirs.sort();
    for d in dirs {
        let path = join(direc, &d);
        if !islink(&path) {
            found.extend(files_below(&path));
        }
    }
    found
}

fn remove_unlinked(session: &mut Session, path: &str, folder: bool) -> Result<()> {
    if session.dry_run() {
        session.say.line(format!("DRYRUN: remove {path}"));
    } else if folder {
        std::fs::remove_dir_all(path).map_err(|e| Error::other(format!("{path}: {e}")))?;
    } else {
        session.remove_file(path)?;
    }
    Ok(())
}

/// `papers file clean`: the files of the files directory that no entry links to are removed (asked
/// for each one unless `yes`); the bibtex is never removed.
pub fn clean(session: &mut Session, yes: bool) -> Result<()> {
    if session.library.filesdir.is_none() {
        return Err(Error::exit("the library has no files directory"));
    }
    let ignore = [session.config.bibtex.clone().unwrap_or_default()];
    let removed = clean_filesdir(session, !yes, &ignore)?;
    session.say.line(format!(
        "{}removed {} unlinked file(s) from {}",
        if session.dry_run() { "DRYRUN: " } else { "" },
        removed.len(),
        session.config.filesdir.clone().unwrap_or_default()
    ));
    Ok(())
}

// checksum
// --------

/// `papers file checksum`: write (or with `check`, check) the checksums of the files of the
/// entries in `.papers/files.json`.
pub fn checksums(session: &mut Session, keys: &[String], check: bool) -> Result<()> {
    let libdir = dirname(&abspath(
        session.config.bibtex.as_deref().unwrap_or_default(),
    ));
    let Some(files_json) = FilesJson::of(&session.config, &libdir) else {
        return Err(Error::exit(
            "the checksums are kept in a library (.papers/files.json): this bibtex has none",
        ));
    };
    let paths: BTreeSet<String> = keys
        .iter()
        .flat_map(|k| session.library.files(k))
        .map(|f| normpath(&f))
        .collect();
    let mut hashes: BTreeMap<String, String> = files_json.load();
    let (mut written, mut changed, mut missing) = (0usize, Vec::<String>::new(), 0usize);
    for p in &paths {
        if !isfile(p) {
            missing += 1;
            continue;
        }
        let digest = checksum(p);
        if hashes.get(p).is_some_and(|h| *h != digest) {
            changed.push(p.clone());
        }
        if hashes.get(p) != Some(&digest) {
            written += 1;
            hashes.insert(p.clone(), digest);
        }
    }
    for p in &changed {
        session.say.line(format!(
            "{}: changed since its checksum{}",
            short_home(p),
            if check { "" } else { " (updated)" }
        ));
    }
    if missing > 0 {
        session.say.line(format!(
            "{missing} broken link{} (`papers file check` reports them)",
            if missing == 1 { "" } else { "s" }
        ));
    }
    if check {
        session.say.line(format!(
            "{} files checked, {} changed, {} without checksum",
            paths.len() - missing,
            changed.len(),
            written - changed.len()
        ));
        return Ok(());
    }
    if !session.dry_run() {
        files_json.save(&hashes)?;
    }
    session.say.line(format!(
        "{written} checksum{} written ({} changed)",
        if written == 1 { "" } else { "s" },
        changed.len()
    ));
    // the history records the files of the library: a state of its own, whatever the bibtex did
    session.commit_other_changes()?;
    Ok(())
}
