//! `papers migrate`: the one-time upgrades from papers v2 (`papers.migrate`,
//! `papers.cli.library_cmds.migratecmd`).
//!
//! The global configuration of v2 defined a library for the whole user account. In v3 it becomes
//! an ordinary library, where it is: its settings are written to a library configuration
//! (`.papers/config.json` in the directory of its bibtex), it is registered as the default
//! library (the `default` setting of the global configuration: the library used outside of any
//! library), and the old file is renamed to `<name>.v2.bak`. It is self-contained: its own files,
//! no pool. The library configurations of v2 (`.papersconfig.json`) are read as self-contained
//! libraries (no pool); they are moved to `.papers/config.json` when papers saves them, or by
//! `papers migrate` in the library.
//!
//! What these functions say goes to the standard output as it comes (they are about the
//! configuration, not a library that is written: the changes of a library are for a [`Session`]).

use serde_json::json;

use crate::config::dirs::{config_file, config_file_legacy, config_file_legacy_xdg};
use crate::config::paths::{dirname, exists, expanduser, isdir, isfile, join, realpath};
use crate::config::{
    CONFIG_FILE_LOCAL, Config, DEFAULT_LIBRARY, LIBRARY_SETTINGS, MachineConfig, NO_POOL, Origin,
    is_v2_config_name, library_config_file, move_v2_config, short_home,
};
use crate::error::{Error, Result};
use crate::library::{Action, Policy};
use crate::log;

use super::session::Session;

/// The global configuration files of v2 that exist, the current location first.
pub fn v2_global_configs() -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for file in [
        config_file(),
        config_file_legacy(),
        config_file_legacy_xdg(),
    ] {
        if isfile(&file) && !seen.contains(&realpath(&file)) {
            seen.push(realpath(&file));
            found.push(file);
        }
    }
    found
}

fn same_path(a: &str, b: &str) -> bool {
    realpath(&expanduser(a)) == realpath(&expanduser(b))
}

/// Migrate the global configuration(s) of v2: how many files were migrated (0: nothing to do).
///
/// `relative`: store the file links relative to the bibtex (by default the library keeps its
/// previous setting, absolute in most cases); `convert_links` converts those of the bibtex of
/// the new library, which it is given the configuration of.
pub fn migrate_v2_global(
    machine: &mut MachineConfig,
    relative: bool,
    dryrun: bool,
    convert_links: &mut dyn FnMut(Config) -> Result<()>,
) -> Result<usize> {
    let files = v2_global_configs();
    let prefix = if dryrun { "DRYRUN: " } else { "" };
    for file in &files {
        let old = Config::load(file, machine.clone(), Some(false))?;
        log::println(&format!("global configuration of papers v2: {file}"));
        match old.bibtex.clone().filter(|b| !b.is_empty()) {
            None => log::println(&format!(
                "  {prefix}it defines no bibtex: no library to create"
            )),
            Some(bibtex) => {
                let libdir = dirname(&bibtex);
                let target = join(&libdir, CONFIG_FILE_LOCAL);
                let existing = if isdir(&libdir) {
                    library_config_file(&libdir)?
                } else {
                    None
                };
                if let Some(existing) = existing {
                    log::println(&format!(
                        "  a library configuration already exists: {existing} (kept as is)"
                    ));
                } else {
                    let mut new = Config::new(machine.clone());
                    new.file = Some(target.clone());
                    new.is_library = true;
                    new.bibtex = Some(bibtex.clone());
                    new.filesdir = old.filesdir.clone();
                    for name in LIBRARY_SETTINGS.iter() {
                        if old.explicit.contains(*name) {
                            new.set(name, old.get(name))?;
                        }
                    }
                    new.set("pool", json!(NO_POOL))?; // self-contained
                    new.set(
                        "paths",
                        if relative {
                            json!("relative")
                        } else {
                            old.get("paths")
                        },
                    )?;
                    let paths = new.get("paths");
                    log::println(&format!(
                        "  {prefix}create the library configuration {target} (paths: {})",
                        paths.as_str().unwrap_or("")
                    ));
                    if !dryrun {
                        new.save()?;
                        if relative && old.absolute_paths && exists(&bibtex) {
                            convert_links(new)?;
                            log::println(&format!(
                                "  the file links of {bibtex} are now relative to its directory"
                            ));
                        }
                    }
                }

                let current = machine.default_library();
                if current.as_deref().is_none_or(|c| same_path(c, &libdir)) {
                    log::println(&format!(
                        "  {prefix}register {libdir} as the default library in {}",
                        machine.file.as_deref().unwrap_or("")
                    ));
                    if !dryrun {
                        machine.set(DEFAULT_LIBRARY, Some(json!(short_home(&libdir))));
                        machine.save()?;
                    }
                } else {
                    log::println(&format!(
                        "  the default library is already {}: not changed (change it with `papers config --global default PATH`)",
                        current.unwrap_or_default()
                    ));
                }
            }
        }

        let backup = format!("{file}.v2.bak");
        log::println(&format!("  {prefix}rename {file} to {backup}"));
        if !dryrun {
            std::fs::rename(file, &backup).map_err(|e| Error::other(format!("{file}: {e}")))?;
        }
    }
    if !files.is_empty() {
        log::info(&format!(
            "migrated {} global configuration file(s)",
            files.len()
        ));
    }
    Ok(files.len())
}

/// Move the configuration of the library `config` from `.papersconfig.json` (papers 2) to
/// `.papers/config.json`: whether there was one to move.
pub fn migrate_library_config(config: &mut Config, dryrun: bool) -> Result<bool> {
    let Some(file) = config.file.clone() else {
        return Ok(false);
    };
    if !(config.is_library && is_v2_config_name(&file)) {
        return Ok(false);
    }
    config.file = Some(move_v2_config(&file, dryrun)?);
    Ok(true)
}

/// `papers migrate` without `--folders`: the library configuration of the directory, and the
/// global configuration of v2. What was found is said; the last line says when there was nothing.
pub fn migrate(
    config: &mut Config,
    relative: bool,
    dryrun: bool,
    convert_links: &mut dyn FnMut(Config) -> Result<()>,
) -> Result<()> {
    let in_library = matches!(config.origin, Some(Origin::Library | Origin::Default));
    let moved = in_library && migrate_library_config(config, dryrun)?;
    let mut machine = config.machine.clone();
    let migrated = migrate_v2_global(&mut machine, relative, dryrun, convert_links)?;
    config.machine = machine;
    if migrated == 0 && !moved {
        log::println(&format!(
            "nothing to migrate: no global configuration of papers v2 was found{}",
            if in_library {
                format!(", and the library configuration is already {CONFIG_FILE_LOCAL}")
            } else {
                String::new()
            }
        ));
    }
    Ok(())
}

/// The directory of the files of an entry kept in a folder of papers < 2.19 (`<name>/` with a
/// hidden `.<name>.bib`), if one is.
fn entry_folder(files: &[String]) -> bool {
    files.iter().any(|f| {
        let direc = dirname(f);
        let name = direc.rsplit('/').next().unwrap_or("").to_string();
        isfile(&join(&direc, &format!(".{name}.bib")))
    })
}

/// `papers migrate --folders`: the files of the entries kept in folders of papers < 2.19 are moved
/// out of them, to the flat layout of the files directory. How many entries were moved; the
/// library is saved by the caller if there were some.
pub fn migrate_folders(session: &mut Session) -> Result<usize> {
    let keys: Vec<String> = session.library.entries().map(|e| e.key.clone()).collect();
    let mut count = 0;
    for key in keys {
        let linked = session.library.files(&key);
        if entry_folder(&linked) {
            log::info(&format!("{key}: move files out of {}", dirname(&linked[0])));
            session.library.rename_entry_files(
                &key,
                &Policy::all(Action::Move),
                None,
                None,
                true,
            )?;
            count += 1;
        }
    }
    Ok(count)
}

/// The line that `papers migrate --folders` ends with.
pub fn folders_line(count: usize) -> String {
    format!("{count} entries moved out of the folders of papers < 2.19")
}
