//! Finding the configuration of a library, and writing configuration files.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::sync::atomic::{AtomicBool, Ordering};

use super::dirs::{config_file, config_file_legacy, config_file_legacy_xdg};
use super::library::{Config, Origin};
use super::machine::{MachineConfig, io_message};
use super::paths::{abspath, basename, dirname, exists, isdir, isfile, join, normpath, realpath, short_home};
use super::settings::{CONFIG_FILE_LOCAL, CONFIG_FILE_LOCAL_V2, LIBRARY_CONFIG_FILES};
use crate::error::{Error, Result};
use crate::log;

/// What `.papers/.gitignore` holds: the folder has the internals of papers,
/// which a library under git does not track (they may change between
/// versions).
const GITIGNORE: &str = "*\n";
/// The one that papers wrote before (the git backup in the library).
const LEGACY_GITIGNORES: [&str; 1] = ["backup/\n"];

static DRYRUN: AtomicBool = AtomicBool::new(false);

/// A dry run is in force (`--dry-run`): papers writes nothing that it does not need to
/// say, not even the files that it moves when it loads a library. Set before the library is loaded.
pub fn set_dry_run(on: bool) {
    DRYRUN.store(on, Ordering::Relaxed);
}

pub fn dry_run() -> bool {
    DRYRUN.load(Ordering::Relaxed)
}

fn random_hex() -> String {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(std::process::id() as u64);
    format!("{:016x}", hasher.finish())
}

/// Write `text` to `path` through a temporary file next to it, then replaced
/// in one step: a crash, or a program reading the file meanwhile (a sync
/// client), never sees a partial file. A symbolic link is followed (its
/// target is written), and the permissions of the file are kept.
pub fn write_atomic(path: &str, text: &str) -> Result<()> {
    let path = realpath(path);
    let directory = dirname(&path);
    let mode = match fs::metadata(&path) {
        Ok(meta) => meta.permissions().mode() & 0o7777,
        Err(_) => {
            // SAFETY: umask only reads and sets the process' file mode creation mask
            let umask = unsafe {
                let mask = libc::umask(0);
                libc::umask(mask);
                mask
            };
            0o666 & !(umask as u32)
        }
    };
    let name = basename(&path);
    let mut attempt = 0;
    let (tmp, mut file) = loop {
        let tmp = format!("{directory}/.{name}.{}.tmp", random_hex());
        match fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp) {
            Ok(file) => break (tmp, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 100 => attempt += 1,
            Err(e) => return Err(Error::other(format!("{path}: {}", io_message(&e)))),
        }
    };
    let result = (|| -> std::io::Result<()> {
        file.write_all(text.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
        fs::rename(&tmp, &path)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(Error::other(format!("{path}: {}", io_message(&e))));
    }
    Ok(())
}

/// Write `.papers/.gitignore` (everything ignored), unless the user wrote
/// one; the one that earlier versions wrote is replaced.
pub fn ignore_papers_folder(folder: &str) {
    if basename(&normpath(folder)) != ".papers" {
        return;
    }
    let path = join(folder, ".gitignore");
    let current = if exists(&path) {
        match fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) => {
                log::debug(&format!("{path}: not written ({})", io_message(&e)));
                return;
            }
        }
    } else {
        None
    };
    if current.as_deref().is_none_or(|c| LEGACY_GITIGNORES.contains(&c))
        && let Err(e) = write_atomic(&path, GITIGNORE)
    {
        log::debug(&format!("{path}: not written ({e})"));
    }
}

/// The configuration file of the library in directory `path`, or `None`. A
/// directory with both names (`.papers/config.json` and `.papersconfig.json`
/// of papers 2) is an error: the user keeps one.
pub fn library_config_file(path: &str) -> Result<Option<String>> {
    let found: Vec<String> =
        LIBRARY_CONFIG_FILES.iter().map(|name| join(path, name)).filter(|file| isfile(file)).collect();
    if found.len() > 1 {
        return Err(Error::exit(format!(
            "{} has two library configurations, {CONFIG_FILE_LOCAL} and {CONFIG_FILE_LOCAL_V2} (of papers 2): keep one of them and remove the other (nothing was changed)",
            short_home(&abspath(path))
        )));
    }
    Ok(found.into_iter().next())
}

/// The configuration file of the library of `start_dir`: in `start_dir` or a
/// directory above it (the root directory excluded), else `None`.
pub fn find_library_config(start_dir: &str) -> Result<Option<String>> {
    let mut current = abspath(start_dir);
    loop {
        let parent = dirname(&current);
        if parent == current {
            // the root directory
            return Ok(None);
        }
        if let Some(file) = library_config_file(&current)? {
            return Ok(Some(file));
        }
        current = parent;
    }
}

/// A library configuration file with the name of papers 2
/// (`.papersconfig.json`).
pub fn is_v2_config_name(file: &str) -> bool {
    basename(file) == CONFIG_FILE_LOCAL_V2
}

/// Move the library configuration of papers 2 `file` (`.papersconfig.json`)
/// to `.papers/config.json` in the same directory, and return the new path
/// (with `dryrun`: only say so). Error if both files exist.
pub fn move_v2_config(file: &str, dryrun: bool) -> Result<String> {
    let libdir = dirname(&abspath(file));
    let new = join(&libdir, CONFIG_FILE_LOCAL);
    if exists(&new) {
        library_config_file(&libdir)?; // the error of two configurations
    }
    if dryrun {
        log::println(&format!("DRYRUN: {} not moved to {}", short_home(file), short_home(&new)));
        return Ok(file.to_string());
    }
    let dir = dirname(&new);
    fs::create_dir_all(&dir).map_err(|e| Error::other(format!("{dir}: {}", io_message(&e))))?;
    fs::rename(file, &new).map_err(|e| Error::other(format!("{file}: {}", io_message(&e))))?;
    log::println(&format!("moved {} to {}", short_home(file), short_home(&new)));
    Ok(new)
}

/// Move the global configuration from the locations used by previous
/// versions (`~/.local/share/papers/config.json` on every platform, and the
/// XDG-style location on macOS where platformdirs now differs).
pub fn check_legacy_global_config() {
    let current = config_file();
    for legacy in [config_file_legacy(), config_file_legacy_xdg()] {
        if realpath(&legacy) == realpath(&current) || !exists(&legacy) {
            continue;
        }
        if !exists(&current) {
            log::warning(&format!("Move legacy config file {legacy} to {current}"));
            let dir = dirname(&current);
            let _ = fs::create_dir_all(&dir);
            if fs::rename(&legacy, &current).is_err() && fs::copy(&legacy, &current).is_ok() {
                let _ = fs::remove_file(&legacy);
            }
        } else {
            log::warning(&format!("Legacy config file found: {legacy}. Delete to remove this warning:  rm -f '{legacy}'"));
        }
    }
}

/// The configuration that commands act on, with `origin` telling where it
/// was found:
/// - `Library`: a library configuration in `start_dir` or above;
/// - `V2Global`: the global configuration of papers v2, until `papers migrate`;
/// - `Default`: the default library of the global configuration;
/// - none: no library (a `Config` with the inherited settings only).
pub fn find_config(start_dir: &str, machine: MachineConfig) -> Result<Config> {
    let mut configfile = find_library_config(start_dir)?;
    let mut origin = Origin::Library;
    if configfile.is_none() && exists(&config_file()) {
        configfile = Some(config_file());
        origin = Origin::V2Global;
    }
    if configfile.is_none()
        && let Some(default) = machine.default_library_path()
        && isdir(&default)
    {
        configfile = library_config_file(&default)?;
        origin = Origin::Default;
    }
    let Some(configfile) = configfile else {
        return Ok(Config::new(machine));
    };
    let is_library = if origin == Origin::V2Global { Some(false) } else { None };
    let mut config = Config::load(&configfile, machine, is_library)?;
    config.origin = Some(origin);
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canonical(dir: &std::path::Path) -> String {
        fs::canonicalize(dir).unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn the_library_is_found_from_a_directory_above() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        fs::create_dir_all(format!("{root}/lib/.papers")).unwrap();
        fs::write(format!("{root}/lib/.papers/config.json"), "{}").unwrap();
        fs::create_dir_all(format!("{root}/lib/a/b")).unwrap();
        fs::create_dir_all(format!("{root}/other")).unwrap();
        assert_eq!(
            find_library_config(&format!("{root}/lib/a/b")).unwrap().as_deref(),
            Some(format!("{root}/lib/.papers/config.json").as_str())
        );
        assert_eq!(find_library_config(&format!("{root}/other")).unwrap(), None);
    }

    #[test]
    fn two_library_configurations_are_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        fs::create_dir_all(format!("{root}/.papers")).unwrap();
        fs::write(format!("{root}/.papers/config.json"), "{}").unwrap();
        fs::write(format!("{root}/.papersconfig.json"), "{}").unwrap();
        let error = library_config_file(&root).unwrap_err();
        assert!(error.message().contains("has two library configurations, .papers/config.json and .papersconfig.json (of papers 2)"));
    }

    #[test]
    fn the_default_library_is_found_outside_of_libraries() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        fs::create_dir_all(format!("{root}/default/.papers")).unwrap();
        fs::write(format!("{root}/default/.papers/config.json"), r#"{"bibtex": "d.bib", "version": 3}"#).unwrap();
        fs::create_dir_all(format!("{root}/elsewhere")).unwrap();
        let mut machine = MachineConfig::empty();
        machine.set("default", Some(serde_json::json!(format!("{root}/default"))));
        let config = find_config(&format!("{root}/elsewhere"), machine).unwrap();
        // a v2 configuration in the real home would take the place of it: only check what the default gives
        if config.origin == Some(Origin::Default) {
            assert_eq!(config.bibtex.as_deref(), Some(format!("{root}/default/d.bib").as_str()));
        }
    }

    #[test]
    fn write_atomic_keeps_the_mode_and_follows_links() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        let target = format!("{root}/target.json");
        fs::write(&target, "old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        std::os::unix::fs::symlink(&target, format!("{root}/link.json")).unwrap();
        write_atomic(&format!("{root}/link.json"), "new").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert!(fs::symlink_metadata(format!("{root}/link.json")).unwrap().file_type().is_symlink());
        assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o640);
        let leftovers: Vec<_> = fs::read_dir(&root).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(leftovers.len(), 2, "{leftovers:?}");
    }

    #[test]
    fn the_ignore_file_of_the_papers_folder() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        let ignore = format!("{root}/.papers/.gitignore");
        fs::create_dir_all(format!("{root}/.papers")).unwrap();
        ignore_papers_folder(&format!("{root}/.papers"));
        assert_eq!(fs::read_to_string(&ignore).unwrap(), "*\n");
        // one that the user wrote stays, the one of earlier versions is replaced
        fs::write(&ignore, "mine\n").unwrap();
        ignore_papers_folder(&format!("{root}/.papers"));
        assert_eq!(fs::read_to_string(&ignore).unwrap(), "mine\n");
        fs::write(&ignore, "backup/\n").unwrap();
        ignore_papers_folder(&format!("{root}/.papers"));
        assert_eq!(fs::read_to_string(&ignore).unwrap(), "*\n");
        // another folder is none of papers' business
        fs::create_dir_all(format!("{root}/other")).unwrap();
        ignore_papers_folder(&format!("{root}/other"));
        assert!(!std::path::Path::new(&format!("{root}/other/.gitignore")).exists());
    }
}
