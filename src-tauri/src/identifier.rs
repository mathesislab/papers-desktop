//! The identifier of the app (tauri.conf.json), which names its folders: it
//! was `io.github.alex-robinson.papers-desktop` until the repository moved to
//! the mathesislab organization. The folders of the old one are moved to the
//! new one once, at start, before anything reads or makes them: the plugins
//! (the window state), the webviews (their local storage), the setup.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const OLD: &str = "io.github.alex-robinson.papers-desktop";

/// Move the folders of the old identifier to the new one. A folder of the
/// new one there already is kept, and the old one with it.
pub fn migrate(identifier: &str) {
    for parent in parents() {
        let (old, new) = (parent.join(OLD), parent.join(identifier));
        if let Err(e) = move_folder(&old, &new) {
            eprintln!(
                "papers-desktop: cannot move {} to {}: {e}",
                old.display(),
                new.display()
            );
        }
    }
}

/// Where the folders named by the identifier are: Tauri's
/// (`app_config_dir`, `app_data_dir`, `app_local_data_dir`: one folder on
/// macOS, two on Linux, with the data of the webviews in the local one, as on
/// Windows) and, on macOS, WebKit's, with the local storage of the pages.
/// The caches are not moved: nothing of the app is kept in them.
fn parents() -> Vec<PathBuf> {
    let mut parents: Vec<PathBuf> = [dirs::config_dir(), dirs::data_dir(), dirs::data_local_dir()]
        .into_iter()
        .flatten()
        .collect();
    #[cfg(target_os = "macos")]
    parents.extend(dirs::home_dir().map(|home| home.join("Library/WebKit")));
    parents.dedup();
    parents
}

/// `old` renamed `new`, when there is an `old` and no `new` yet: whether it was.
fn move_folder(old: &Path, new: &Path) -> io::Result<bool> {
    if new.symlink_metadata().is_ok() || !old.is_dir() {
        return Ok(false);
    }
    fs::rename(old, new)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_the_old_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (old, new) = (dir.path().join(OLD), dir.path().join("new"));
        fs::create_dir(&old).unwrap();
        fs::write(old.join("settings.json"), "{}").unwrap();
        assert!(move_folder(&old, &new).unwrap());
        assert!(!old.exists());
        assert_eq!(fs::read_to_string(new.join("settings.json")).unwrap(), "{}");
    }

    #[test]
    fn keeps_both_when_the_new_one_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let (old, new) = (dir.path().join(OLD), dir.path().join("new"));
        fs::create_dir(&old).unwrap();
        fs::create_dir(&new).unwrap();
        fs::write(new.join("settings.json"), "new").unwrap();
        assert!(!move_folder(&old, &new).unwrap());
        assert!(old.exists());
        assert_eq!(
            fs::read_to_string(new.join("settings.json")).unwrap(),
            "new"
        );
    }

    #[test]
    fn nothing_without_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let (old, new) = (dir.path().join(OLD), dir.path().join("new"));
        assert!(!move_folder(&old, &new).unwrap());
        assert!(!new.exists());
    }
}
