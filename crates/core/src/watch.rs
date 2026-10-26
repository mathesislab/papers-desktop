//! Notice when the bibtex file changes, from papers or anything else.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use notify::event::ModifyKind;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Wait this long after the last event before reporting a change: one save
/// comes as several events (e.g. a temporary file renamed over the bibtex).
const QUIET: Duration = Duration::from_secs(1);

/// Watches a bibtex file until dropped.
pub struct BibWatch {
    path: PathBuf,
    _watcher: RecommendedWatcher,
}

impl BibWatch {
    /// Call `on_change` (on a thread of its own) whenever `bibtex` has
    /// changed, once the events have stopped for a second. The directory is
    /// watched, not the file, so a file replaced by a rename is still seen.
    pub fn new(bibtex: PathBuf, on_change: impl Fn() + Send + 'static) -> notify::Result<BibWatch> {
        let dir = bibtex.parent().unwrap_or(Path::new(".")).to_path_buf();
        let name = bibtex.file_name().map(|n| n.to_os_string());
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let Ok(event) = event else { return };
            // reads and metadata changes (access time, extended attributes
            // set by a sync client) do not change the entries
            let content = match event.kind {
                EventKind::Modify(ModifyKind::Metadata(_)) => false,
                kind => kind.is_create() || kind.is_modify() || kind.is_remove(),
            };
            if content && event.paths.iter().any(|p| p.file_name() == name.as_deref()) {
                let _ = tx.send(());
            }
        })?;
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;

        let path = bibtex.clone();
        std::thread::spawn(move || {
            let mut last = modified(&path);
            // ends when the watcher, and with it the sender, is dropped
            while rx.recv().is_ok() {
                while rx.recv_timeout(QUIET).is_ok() {}
                let now = modified(&path);
                if now != last {
                    last = now;
                    on_change();
                }
            }
        });
        Ok(BibWatch {
            path: bibtex,
            _watcher: watcher,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}
