//! The Downloads panel of the main window: the PDFs of one folder outside the library (by default
//! ~/Downloads), listed to drag into it. Only the names, sizes and dates of the files of that one
//! folder are read (and the contents of those with the size of a file of the library, to tell that
//! the paper is there already); nothing leaves the computer.

use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File};
use std::hash::Hasher;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

/// A PDF of the folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Download {
    pub path: PathBuf,
    pub name: String,
    /// bytes
    pub size: u64,
    /// when it came: the later of its modification and creation, ms since the epoch
    pub added: i64,
    /// a paper of the library links this file, or has a file with the same content
    pub in_library: bool,
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
                in_library: false,
            })
        })
        .collect();
    files.sort_by(|a, b| b.added.cmp(&a.added).then_with(|| a.name.cmp(&b.name)));
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

/// Mark the files that are in the library already: one of the `linked` files of its papers is that
/// file (the same path), or has the same contents (compared only among files of the same size, so
/// that few are read).
pub fn mark_in_library(files: &mut [Download], linked: &[PathBuf]) {
    let canonical = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let linked: Vec<&PathBuf> = linked.iter().filter(|p| is_pdf(p)).collect();
    // by path: only the linked files with the name of the download are looked at
    for file in files.iter_mut() {
        let name = file.path.file_name();
        let mut same_name = linked.iter().filter(|p| p.file_name() == name);
        if same_name.any(|p| canonical(p) == canonical(&file.path)) {
            file.in_library = true;
        }
    }
    if files.iter().all(|f| f.in_library) {
        return;
    }
    // by contents
    let mut sizes: Vec<(u64, &PathBuf)> = linked
        .iter()
        .filter_map(|p| Some((fs::metadata(p).ok()?.len(), *p)))
        .collect();
    sizes.sort();
    for file in files.iter_mut().filter(|f| !f.in_library) {
        let start = sizes.partition_point(|(size, _)| *size < file.size);
        let same: Vec<&PathBuf> = sizes[start..]
            .iter()
            .take_while(|(size, _)| *size == file.size)
            .map(|(_, p)| *p)
            .collect();
        if same.is_empty() {
            continue;
        }
        if let Some(hash) = content(&file.path) {
            file.in_library = same.iter().any(|p| content(p) == Some(hash));
        }
    }
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

/// The folder being watched, if the panel is shown.
#[derive(Default)]
pub struct Downloads {
    watching: Mutex<Option<(PathBuf, DirWatch)>>,
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
            let linked: Vec<PathBuf> = {
                let library = state.library.read().unwrap();
                library
                    .items()
                    .iter()
                    .flat_map(|i| i.files.iter().cloned())
                    .collect()
            };
            mark_in_library(&mut files, &linked);
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
        assert!(files.iter().all(|f| !f.in_library));
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

    #[test]
    fn marked_when_linked_by_path_or_the_same_content() {
        let downloads = tempfile::tempdir().unwrap();
        let library = tempfile::tempdir().unwrap();
        let put = |dir: &Path, name: &str, bytes: &[u8]| {
            let path = dir.join(name);
            fs::write(&path, bytes).unwrap();
            path
        };
        let linked_here = put(downloads.path(), "linked.pdf", b"%PDF linked");
        put(downloads.path(), "copied.pdf", b"%PDF copied");
        put(downloads.path(), "same-size.pdf", b"%PDF AAAAAA");
        put(downloads.path(), "other.pdf", b"%PDF other, longer");
        let renamed = put(library.path(), "Smith2020.pdf", b"%PDF copied");
        let different = put(library.path(), "Jones2021.pdf", b"%PDF BBBBBB");
        // not a PDF: its contents do not count
        let note = put(library.path(), "note.txt", b"%PDF other, longer");

        let mut files = list(downloads.path()).unwrap();
        mark_in_library(&mut files, &[linked_here, renamed, different, note]);
        let mut marked: Vec<(&str, bool)> = files
            .iter()
            .map(|f| (f.name.as_str(), f.in_library))
            .collect();
        marked.sort();
        assert_eq!(
            marked,
            [
                ("copied.pdf", true),
                ("linked.pdf", true),
                ("other.pdf", false),
                ("same-size.pdf", false)
            ]
        );
    }

    #[test]
    fn a_path_spelled_another_way_is_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.pdf"), "x").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        let mut files = list(dir.path()).unwrap();
        mark_in_library(&mut files, &[dir.path().join("sub/../a.pdf")]);
        assert!(files[0].in_library);
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
