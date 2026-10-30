//! The Downloads panel of the main window: the PDFs of one folder outside the library (by default
//! ~/Downloads), listed to drag into it. Only the names, sizes and dates of the files of that one
//! folder are read (and the contents of those with the size of a file of the library, to tell that
//! the paper is there already); nothing leaves the computer. A PDF moved into the library stays in
//! the list, marked, until the app quits.

use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File};
use std::hash::Hasher;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use notify::event::ModifyKind;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use crate::AppState;
use crate::settings::Settings;

/// Wait this long after the last event before telling the page: a download is written in pieces,
/// then renamed.
const QUIET: Duration = Duration::from_millis(500);
/// A PDF that left the folder is looked for in the library this long: the library is read again
/// after the file has moved.
const LEFT: Duration = Duration::from_secs(60);

/// What tells a file apart on its disk, whatever its name: its device and inode, which a move
/// (a rename) keeps.
type FileId = (u64, u64);

#[cfg(unix)]
fn file_id(meta: &fs::Metadata) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;
    Some((meta.dev(), meta.ino()))
}

#[cfg(not(unix))]
fn file_id(_: &fs::Metadata) -> Option<FileId> {
    None
}

/// A PDF of the folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Download {
    pub path: PathBuf,
    pub name: String,
    /// bytes
    pub size: u64,
    /// when it came: the later of its modification and creation, ms since the epoch
    pub added: i64,
    /// the papers of the library that link this file, or a file with the same content: empty when
    /// it is not in the library
    pub matches: Vec<Match>,
    /// moved into the library: no longer in the folder, still listed (the papers that link it are
    /// the matches)
    pub imported: bool,
    /// the file on its disk, to find it in the library once moved there
    #[serde(skip)]
    id: Option<FileId>,
}

/// A paper of the library that has a PDF of the folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Match {
    pub id: String,
    pub key: String,
    /// the file the paper links, relative to the folder of its .bib as the library has it (its full
    /// path when elsewhere)
    pub file: String,
    /// not the same file, but a file with the same contents
    pub same_contents: bool,
}

/// A file a paper of the library links.
#[derive(Debug, Clone)]
pub struct Linked {
    pub id: String,
    pub key: String,
    pub bib: PathBuf,
    pub file: PathBuf,
}

impl Linked {
    fn to_match(&self, same_contents: bool) -> Match {
        let relative = self
            .bib
            .parent()
            .and_then(|dir| self.file.strip_prefix(dir).ok());
        Match {
            id: self.id.clone(),
            key: self.key.clone(),
            file: relative
                .unwrap_or(&self.file)
                .to_string_lossy()
                .into_owned(),
            same_contents,
        }
    }
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

fn millis(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_millis() as i64,
        Err(before) => -(before.duration().as_millis() as i64),
    }
}

/// The PDFs of the folder (not those of the folders in it, nor hidden files), the newest first, the
/// name of equal times in order.
pub fn list(dir: &Path) -> Result<Vec<Download>, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files: Vec<Download> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if name.starts_with('.') || !is_pdf(&path) {
                return None;
            }
            // what a link points to; a folder named x.pdf, or a link that leads nowhere, is no file
            let meta = fs::metadata(&path).ok().filter(|m| m.is_file())?;
            let modified = meta.modified().ok()?;
            let added = millis(meta.created().map_or(modified, |c| c.max(modified)));
            Some(Download {
                path,
                name,
                size: meta.len(),
                added,
                matches: Vec::new(),
                imported: false,
                id: file_id(&meta),
            })
        })
        .collect();
    newest_first(&mut files);
    Ok(files)
}

/// A hash of the contents of a file.
fn content(path: &Path) -> Option<u64> {
    let mut file = File::open(path).ok()?;
    let mut hasher = DefaultHasher::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buffer).ok()? {
            0 => return Some(hasher.finish()),
            n => hasher.write(&buffer[..n]),
        }
    }
}

/// Say which papers have each file that is in the library already: those that link that file (the
/// same path), else those that link a file with the same contents (compared only among files of
/// the same size, so that few are read). A paper once, by the first of its files that matches.
pub fn mark_in_library(files: &mut [Download], linked: &[Linked]) {
    let canonical = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let linked: Vec<&Linked> = linked.iter().filter(|l| is_pdf(&l.file)).collect();
    let once = |matches: &mut Vec<Match>, m: Match| {
        if !matches.iter().any(|x| x.id == m.id) {
            matches.push(m);
        }
    };
    // by path: only the linked files with the name of the download are looked at
    for file in files.iter_mut() {
        let name = file.path.file_name();
        let path = canonical(&file.path);
        for l in linked.iter().filter(|l| l.file.file_name() == name) {
            if canonical(&l.file) == path {
                once(&mut file.matches, l.to_match(false));
            }
        }
    }
    if files.iter().all(|f| !f.matches.is_empty()) {
        return;
    }
    // by contents
    let mut sizes: Vec<(u64, &Linked)> = linked
        .iter()
        .filter_map(|l| Some((fs::metadata(&l.file).ok()?.len(), *l)))
        .collect();
    sizes.sort_by_key(|(size, _)| *size);
    for file in files.iter_mut().filter(|f| f.matches.is_empty()) {
        let start = sizes.partition_point(|(size, _)| *size < file.size);
        let same: Vec<&Linked> = sizes[start..]
            .iter()
            .take_while(|(size, _)| *size == file.size)
            .map(|(_, l)| *l)
            .collect();
        if same.is_empty() {
            continue;
        }
        if let Some(hash) = content(&file.path) {
            for l in same.into_iter().filter(|l| content(&l.file) == Some(hash)) {
                once(&mut file.matches, l.to_match(true));
            }
        }
    }
}

/// The newest first, the name of equal times in order.
fn newest_first(files: &mut [Download]) {
    files.sort_by(|a, b| b.added.cmp(&a.added).then_with(|| a.name.cmp(&b.name)));
}

/// What the panel listed of a folder this session: to keep the PDFs moved into the library.
#[derive(Debug, Default)]
pub struct Listed {
    folder: PathBuf,
    /// the list shown last
    shown: Vec<Download>,
    /// the PDFs that left the folder and are not found in the library yet, since when
    left: Vec<(Download, Instant)>,
}

impl Listed {
    /// The list to show: the PDFs of the folder now (`files`, marked), and those that left it for
    /// the library this session, in their place, marked imported. A PDF that left is imported when
    /// a paper links the same file, moved: its device and inode, which a rename keeps (a move
    /// that copies to another disk, then deletes, is not seen), and its size; one deleted is not.
    /// It is looked for while the library is read again (`LEFT`), then forgotten; one that comes
    /// back (undo) is in the folder again.
    pub fn arrange(
        &mut self,
        folder: &Path,
        files: Vec<Download>,
        linked: &[Linked],
        now: Instant,
    ) -> Vec<Download> {
        if self.folder != folder {
            *self = Listed {
                folder: folder.to_path_buf(),
                ..Listed::default()
            };
        }
        let here: Vec<&Path> = files.iter().map(|f| f.path.as_path()).collect();
        let shown = std::mem::take(&mut self.shown);
        let (imported, listed): (Vec<Download>, Vec<Download>) =
            shown.into_iter().partition(|f| f.imported);
        self.left.extend(
            listed
                .into_iter()
                .filter(|f| !here.contains(&f.path.as_path()))
                .map(|f| (f, now)),
        );
        self.left.retain(|(f, since)| {
            !here.contains(&f.path.as_path()) && now.duration_since(*since) < LEFT
        });
        let mut moved: Vec<Download> = Vec::new();
        let candidates: Vec<Download> = imported
            .into_iter()
            .filter(|f| !here.contains(&f.path.as_path()))
            .chain(self.left.iter().map(|(f, _)| f.clone()))
            .collect();
        if !candidates.is_empty() {
            let ids = linked_ids(linked);
            for mut file in candidates {
                let mut matches: Vec<Match> = Vec::new();
                let same = |(id, size, _): &&(FileId, u64, &Linked)| {
                    file.id == Some(*id) && *size == file.size
                };
                for (_, _, l) in ids.iter().filter(same) {
                    if !matches.iter().any(|m| m.id == l.id) {
                        matches.push(l.to_match(false));
                    }
                }
                if !matches.is_empty() {
                    self.left.retain(|(f, _)| f.path != file.path);
                    file.imported = true;
                    file.matches = matches;
                    moved.push(file);
                }
            }
        }
        let mut shown: Vec<Download> = files.into_iter().chain(moved).collect();
        newest_first(&mut shown);
        self.shown = shown.clone();
        shown
    }
}

/// The PDFs the papers of the library link that are there, with their file on the disk and size.
fn linked_ids(linked: &[Linked]) -> Vec<(FileId, u64, &Linked)> {
    linked
        .iter()
        .filter(|l| is_pdf(&l.file))
        .filter_map(|l| {
            let meta = fs::metadata(&l.file).ok()?;
            Some((file_id(&meta)?, meta.len(), l))
        })
        .collect()
}

/// Watches a folder for PDFs that come, change or go, until dropped.
pub struct DirWatch {
    _watcher: RecommendedWatcher,
}

impl DirWatch {
    /// Call `on_change` (on a thread of its own) when PDFs of `dir` came, changed or went, once the
    /// events have stopped for `quiet`.
    pub fn new(
        dir: &Path,
        quiet: Duration,
        on_change: impl Fn() + Send + 'static,
    ) -> notify::Result<DirWatch> {
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let Ok(event) = event else { return };
            let content = match event.kind {
                EventKind::Modify(ModifyKind::Metadata(_)) => false,
                kind => kind.is_create() || kind.is_modify() || kind.is_remove(),
            };
            if content && event.paths.iter().any(|p| is_pdf(p)) {
                let _ = tx.send(());
            }
        })?;
        watcher.watch(dir, RecursiveMode::NonRecursive)?;
        // ends when the watcher, and with it the sender, is dropped
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                while rx.recv_timeout(quiet).is_ok() {}
                on_change();
            }
        });
        Ok(DirWatch { _watcher: watcher })
    }
}

/// The folder being watched, if the panel is shown, and what was listed of it.
#[derive(Default)]
pub struct Downloads {
    watching: Mutex<Option<(PathBuf, DirWatch)>>,
    listed: Mutex<Listed>,
}

impl Downloads {
    /// Watch the folder of the settings, or nothing when the panel is hidden; the watch of the same
    /// folder stays. The page is told (`downloads`) when PDFs change, and now.
    pub fn apply(&self, app: &AppHandle, settings: &Settings) {
        let dir = settings.downloads_panel.then(|| settings.downloads_dir());
        {
            let mut watching = self.watching.lock().unwrap();
            if watching.as_ref().map(|(d, _)| d) != dir.as_ref() {
                *watching = dir.and_then(|dir| {
                    let handle = app.clone();
                    match DirWatch::new(&dir, QUIET, move || {
                        let _ = handle.emit("downloads", ());
                    }) {
                        Ok(watch) => Some((dir, watch)),
                        Err(e) => {
                            eprintln!("papers-desktop: cannot watch {}: {e}", dir.display());
                            None
                        }
                    }
                });
            }
        }
        let _ = app.emit("downloads", ());
    }
}

/// What the panel shows.
#[derive(Serialize)]
pub struct DownloadsView {
    shown: bool,
    folder: PathBuf,
    /// why the folder cannot be read
    error: Option<String>,
    files: Vec<Download>,
}

/// The PDFs of the folder, with those in the library marked. Async: the folder and the files are
/// read.
#[tauri::command]
pub async fn downloads_list(state: State<'_, AppState>) -> Result<DownloadsView, String> {
    let settings = Settings::load(&state.settings_path)?;
    let folder = settings.downloads_dir();
    if !settings.downloads_panel {
        return Ok(DownloadsView {
            shown: false,
            folder,
            error: None,
            files: Vec::new(),
        });
    }
    let (files, error) = match list(&folder) {
        Ok(mut files) => {
            let linked: Vec<Linked> = {
                let library = state.library.read().unwrap();
                library
                    .items()
                    .iter()
                    .flat_map(|i| {
                        i.files.iter().map(|file| Linked {
                            id: i.id.clone(),
                            key: i.key.clone(),
                            bib: i.bib.clone(),
                            file: file.clone(),
                        })
                    })
                    .collect()
            };
            mark_in_library(&mut files, &linked);
            let files = state.downloads.listed.lock().unwrap().arrange(
                &folder,
                files,
                &linked,
                Instant::now(),
            );
            (files, None)
        }
        Err(e) => (Vec::new(), Some(e)),
    };
    Ok(DownloadsView {
        shown: true,
        folder,
        error,
        files,
    })
}

/// Show or hide the panel; the other settings stay.
#[tauri::command]
pub fn set_downloads_panel(
    app: AppHandle,
    state: State<AppState>,
    shown: bool,
) -> Result<(), String> {
    change(&app, &state, |s| s.downloads_panel = shown)
}

/// The folder of the panel; the other settings stay.
#[tauri::command]
pub fn set_downloads_folder(
    app: AppHandle,
    state: State<AppState>,
    folder: PathBuf,
) -> Result<(), String> {
    change(&app, &state, |s| s.downloads_folder = folder)
}

fn change(
    app: &AppHandle,
    state: &AppState,
    set: impl FnOnce(&mut Settings),
) -> Result<(), String> {
    let mut settings = Settings::load(&state.settings_path)?;
    set(&mut settings);
    settings.save(&state.settings_path)?;
    state.downloads.apply(app, &settings);
    Ok(())
}

/// The folder, in the file manager.
#[tauri::command]
pub fn open_downloads(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let folder = Settings::load(&state.settings_path)?.downloads_dir();
    app.opener()
        .open_path(folder.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    fn names(files: &[Download]) -> Vec<&str> {
        files.iter().map(|f| f.name.as_str()).collect()
    }

    fn set_time(path: &Path, secs: u64) {
        let file = File::options().write(true).open(path).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn only_the_pdfs_of_the_folder_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        // modified in the future of their creation: the modification is the later of the two
        let now = millis(SystemTime::now()) as u64 / 1000;
        let put = |name: &str, bytes: &[u8], secs: u64| {
            let path = dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            set_time(&path, now + secs);
        };
        put("old.pdf", b"%PDF-1", 1000);
        put("new.PDF", b"%PDF-22", 3000);
        put("middle.Pdf", b"%PDF-333", 2000);
        put("notes.txt", b"text", 4000);
        put("paper.pdf.part", b"x", 4000);
        put(".hidden.pdf", b"x", 4000);
        fs::create_dir(dir.path().join("folder.pdf")).unwrap();
        fs::write(dir.path().join("folder.pdf/inside.pdf"), "x").unwrap();

        let files = list(dir.path()).unwrap();
        assert_eq!(names(&files), ["new.PDF", "middle.Pdf", "old.pdf"]);
        assert_eq!(files.iter().map(|f| f.size).collect::<Vec<_>>(), [7, 8, 6]);
        assert_eq!(files[0].path, dir.path().join("new.PDF"));
        assert!(files.iter().all(|f| f.matches.is_empty()));
    }

    #[test]
    fn equal_times_by_name_and_a_missing_folder_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let now = millis(SystemTime::now()) as u64 / 1000 + 1000;
        for name in ["b.pdf", "a.pdf"] {
            fs::write(dir.path().join(name), "x").unwrap();
            set_time(&dir.path().join(name), now);
        }
        assert_eq!(names(&list(dir.path()).unwrap()), ["a.pdf", "b.pdf"]);
        assert!(list(&dir.path().join("none")).is_err());
    }

    fn linked(id: &str, bib: &Path, file: PathBuf) -> Linked {
        Linked {
            id: id.to_string(),
            key: format!("key-{id}"),
            bib: bib.to_path_buf(),
            file,
        }
    }

    /// (id, file, same contents) of a match
    type Seen<'a> = (&'a str, &'a str, bool);

    /// (name, matches) of each file, by name
    fn marked(files: &[Download]) -> Vec<(&str, Vec<Seen<'_>>)> {
        let mut marked: Vec<_> = files
            .iter()
            .map(|f| {
                let matches = f
                    .matches
                    .iter()
                    .map(|m| (m.id.as_str(), m.file.as_str(), m.same_contents))
                    .collect();
                (f.name.as_str(), matches)
            })
            .collect();
        marked.sort();
        marked
    }

    #[test]
    fn marked_with_the_papers_that_link_it_or_a_file_of_the_same_content() {
        let downloads = tempfile::tempdir().unwrap();
        let library = tempfile::tempdir().unwrap();
        let put = |dir: &Path, name: &str, bytes: &[u8]| {
            let path = dir.join(name);
            fs::write(&path, bytes).unwrap();
            path
        };
        let bib = library.path().join("refs.bib");
        let linked_here = put(downloads.path(), "linked.pdf", b"%PDF linked");
        put(downloads.path(), "copied.pdf", b"%PDF copied");
        put(downloads.path(), "same-size.pdf", b"%PDF AAAAAA");
        put(downloads.path(), "other.pdf", b"%PDF other, longer");
        fs::create_dir(library.path().join("files")).unwrap();
        let renamed = put(
            &library.path().join("files"),
            "smith_2020.pdf",
            b"%PDF copied",
        );
        let copy = put(library.path(), "jones_2021.pdf", b"%PDF copied");
        let different = put(library.path(), "other-one.pdf", b"%PDF BBBBBB");
        // not a PDF: its contents do not count
        let note = put(library.path(), "note.txt", b"%PDF other, longer");

        let mut files = list(downloads.path()).unwrap();
        mark_in_library(
            &mut files,
            &[
                // two papers link the same file
                linked("1", &bib, linked_here.clone()),
                linked("2", &bib, linked_here.clone()),
                linked("3", &bib, renamed),
                linked("4", &bib, copy),
                linked("5", &bib, different),
                linked("6", &bib, note),
            ],
        );
        assert_eq!(
            marked(&files),
            [
                (
                    "copied.pdf",
                    vec![
                        ("3", "files/smith_2020.pdf", true),
                        ("4", "jones_2021.pdf", true)
                    ]
                ),
                (
                    "linked.pdf",
                    // outside the folder of the .bib: the full path
                    vec![
                        ("1", linked_here.to_str().unwrap(), false),
                        ("2", linked_here.to_str().unwrap(), false)
                    ]
                ),
                ("other.pdf", vec![]),
                ("same-size.pdf", vec![]),
            ]
        );
    }

    #[test]
    fn a_paper_once_even_with_several_files_that_match() {
        let dir = tempfile::tempdir().unwrap();
        let bib = dir.path().join("refs.bib");
        fs::create_dir(dir.path().join("downloads")).unwrap();
        fs::write(dir.path().join("downloads/a.pdf"), "same").unwrap();
        fs::write(dir.path().join("a1.pdf"), "same").unwrap();
        fs::write(dir.path().join("a2.pdf"), "same").unwrap();
        let mut files = list(&dir.path().join("downloads")).unwrap();
        mark_in_library(
            &mut files,
            &[
                linked("1", &bib, dir.path().join("a1.pdf")),
                linked("1", &bib, dir.path().join("a2.pdf")),
            ],
        );
        assert_eq!(marked(&files), [("a.pdf", vec![("1", "a1.pdf", true)])]);
    }

    #[test]
    fn a_path_spelled_another_way_is_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.pdf"), "x").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        let mut files = list(dir.path()).unwrap();
        let bib = dir.path().join("refs.bib");
        mark_in_library(
            &mut files,
            &[linked("1", &bib, dir.path().join("sub/../a.pdf"))],
        );
        assert_eq!(files[0].matches.len(), 1);
        assert!(!files[0].matches[0].same_contents);
    }

    /// (name, imported, keys of the matches) of each file shown, in order
    fn shown(files: &[Download]) -> Vec<(&str, bool, Vec<&str>)> {
        files
            .iter()
            .map(|f| {
                (
                    f.name.as_str(),
                    f.imported,
                    f.matches.iter().map(|m| m.key.as_str()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn a_pdf_moved_into_the_library_stays_listed_one_deleted_does_not() {
        let root = tempfile::tempdir().unwrap();
        let (downloads, library) = (root.path().join("Downloads"), root.path().join("lib"));
        fs::create_dir_all(&downloads).unwrap();
        fs::create_dir_all(library.join("files")).unwrap();
        let bib = library.join("papers.bib");
        let now = millis(SystemTime::now()) as u64 / 1000;
        for (name, secs) in [
            ("old.pdf", 1000),
            ("moved.pdf", 2000),
            ("deleted.pdf", 3000),
        ] {
            fs::write(downloads.join(name), name).unwrap();
            set_time(&downloads.join(name), now + secs);
        }
        let mut listed = Listed::default();
        let start = Instant::now();
        let arrange = |listed: &mut Listed, linked: &[Linked], after: u64| {
            let mut files = list(&downloads).unwrap();
            mark_in_library(&mut files, linked);
            listed.arrange(
                &downloads,
                files,
                linked,
                start + Duration::from_secs(after),
            )
        };
        arrange(&mut listed, &[], 0);
        let target = library.join("files/smith_2020.pdf");
        fs::rename(downloads.join("moved.pdf"), &target).unwrap();
        fs::remove_file(downloads.join("deleted.pdf")).unwrap();
        // the library not read again yet: the PDF that left is looked for
        assert_eq!(
            shown(&arrange(&mut listed, &[], 1)),
            [("old.pdf", false, vec![])]
        );
        // read again: in its place, marked; the one deleted is not in the library
        let links = [linked("1", &bib, target.clone())];
        let files = arrange(&mut listed, &links, 2);
        assert_eq!(
            shown(&files),
            [
                ("moved.pdf", true, vec!["key-1"]),
                ("old.pdf", false, vec![])
            ]
        );
        assert_eq!(files[0].matches[0].file, "files/smith_2020.pdf");
        assert_eq!(files[0].path, downloads.join("moved.pdf"));
        // kept while the paper links it, after the time to look for those that left
        assert_eq!(shown(&arrange(&mut listed, &links, 200)).len(), 2);
        // taken back (undo): in the folder again, once
        fs::rename(&target, downloads.join("moved.pdf")).unwrap();
        assert_eq!(
            shown(&arrange(&mut listed, &[], 201)),
            [("moved.pdf", false, vec![]), ("old.pdf", false, vec![])]
        );
        // moved again, then its paper removed: no longer listed
        fs::rename(downloads.join("moved.pdf"), &target).unwrap();
        assert_eq!(
            shown(&arrange(&mut listed, &links, 202))[0],
            ("moved.pdf", true, vec!["key-1"])
        );
        assert_eq!(
            shown(&arrange(&mut listed, &[], 203)),
            [("old.pdf", false, vec![])]
        );
        // another folder: nothing kept
        fs::rename(downloads.join("old.pdf"), library.join("old.pdf")).unwrap();
        let links = [
            linked("1", &bib, target),
            linked("2", &bib, library.join("old.pdf")),
        ];
        let other = root.path().join("Other");
        assert!(
            listed
                .arrange(&other, Vec::new(), &links, start)
                .is_empty()
        );
    }

    #[test]
    fn a_pdf_that_left_is_looked_for_a_minute() {
        let root = tempfile::tempdir().unwrap();
        let downloads = root.path().join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        fs::write(downloads.join("a.pdf"), "a").unwrap();
        let mut listed = Listed::default();
        let start = Instant::now();
        listed.arrange(&downloads, list(&downloads).unwrap(), &[], start);
        let target = root.path().join("a_2020.pdf");
        fs::rename(downloads.join("a.pdf"), &target).unwrap();
        listed.arrange(&downloads, Vec::new(), &[], start);
        let links = [linked("1", &root.path().join("refs.bib"), target)];
        let later = start + LEFT + Duration::from_secs(1);
        assert!(
            listed
                .arrange(&downloads, Vec::new(), &links, later)
                .is_empty()
        );
    }

    /// Wait for `condition`, a few seconds at most.
    fn within_seconds(condition: impl Fn() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        condition()
    }

    #[test]
    fn the_watch_tells_of_pdfs_arriving_and_leaving_not_of_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let _watch = DirWatch::new(dir.path(), Duration::from_millis(50), move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(300));

        // other files: nothing
        fs::write(dir.path().join("notes.txt"), "x").unwrap();
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // a download: written under another name, then renamed
        fs::write(dir.path().join("paper.pdf.part"), "x").unwrap();
        fs::rename(
            dir.path().join("paper.pdf.part"),
            dir.path().join("paper.pdf"),
        )
        .unwrap();
        assert!(within_seconds(|| calls.load(Ordering::SeqCst) >= 1));
        std::thread::sleep(Duration::from_millis(300));
        let after_arrival = calls.load(Ordering::SeqCst);

        // a PDF that goes
        fs::remove_file(dir.path().join("paper.pdf")).unwrap();
        assert!(within_seconds(
            || calls.load(Ordering::SeqCst) > after_arrival
        ));
    }

    #[test]
    fn a_folder_that_is_not_there_cannot_be_watched() {
        let dir = tempfile::tempdir().unwrap();
        assert!(DirWatch::new(&dir.path().join("none"), QUIET, || {}).is_err());
    }
}
