//! `papers config`: get and set the settings of the library, or with
//! `--global` of the global configuration.

use std::fs;

use papers_rs::config::json::loads;
use papers_rs::config::paths::{abspath, exists, expanduser, getcwd, short_home};
use papers_rs::config::settings;
use papers_rs::config::{
    Config, DEFAULT_LIBRARY, LIBRARY_ONLY, MACHINE_ONLY, MachineConfig, NO_POOL, Origin, format_value, json_get,
    library_without_pool, parse_value, pool_setting, removed_setting,
};
use papers_rs::error::{Error, Result};
use papers_rs::log;
use serde_json::Value;

use crate::parse::Leaf;

pub struct ConfigArgs {
    pub key: Option<String>,
    pub values: Vec<String>,
    pub unset: Option<String>,
    pub prune: bool,
    pub is_global: bool,
    pub dry_run: bool,
}

impl ConfigArgs {
    pub fn from_leaf(leaf: &Leaf) -> ConfigArgs {
        let mut key_values = leaf.strings("key_values").into_iter();
        ConfigArgs {
            key: key_values.next(),
            values: key_values.collect(),
            unset: leaf.string("unset"),
            prune: leaf.flag("prune"),
            is_global: leaf.flag("is_global"),
            dry_run: leaf.flag("dry_run"),
        }
    }
}

/// What ends `papers config` other than a result.
pub enum Failure {
    /// An error of the command line (`papers config: error: ...`, status 2).
    Usage(String),
    Error(Error),
}

impl From<Error> for Failure {
    fn from(error: Error) -> Failure {
        Failure::Error(error)
    }
}

type Outcome = std::result::Result<(), Failure>;

/// The error of a command that needs a library, run outside of any library.
pub fn no_library_message(config: &Config) -> String {
    let where_ = match config.machine.default_library_path() {
        Some(default) => format!(" (the default library {} is not a library)", short_home(&default)),
        None => String::new(),
    };
    format!("no library found here{where_}: `papers init`")
}

fn check_setting_name(name: &str, is_global: bool) -> Result<()> {
    if name == DEFAULT_LIBRARY {
        if !is_global {
            return Err(Error::exit(format!(
                "{DEFAULT_LIBRARY} is a setting of the global configuration: `papers config --global {DEFAULT_LIBRARY} PATH|none`"
            )));
        }
        return Ok(());
    }
    if let Some(reason) = removed_setting(name) {
        return Err(Error::exit(format!("the {name} setting is no longer used: {reason}")));
    }
    if settings::kind(name).is_none() {
        let extra = if is_global { format!(" (and with --global: {DEFAULT_LIBRARY})") } else { String::new() };
        return Err(Error::exit(format!(
            "unknown setting: {name}. Settings: {}{extra}",
            settings::names().join(", ")
        )));
    }
    if is_global && name == "pool" {
        return Err(Error::exit(
            "the global configuration has no pool: `papers config pool PATH` sets the pool of a library, and `papers config --global default PATH` the default library (`papers init --shallow` uses it as pool)",
        ));
    }
    if !is_global && MACHINE_ONLY.contains(&name) {
        return Err(Error::exit(format!(
            "{name} is a setting of the machine, not of a library (a library received from someone cannot set it): `papers config --global {name} VALUE`"
        )));
    }
    if is_global && LIBRARY_ONLY.contains(&name) {
        return Err(Error::exit(format!(
            "{name} is a setting of a library: the global configuration holds the default library and the default settings of the libraries (the global library of papers v2 is now the default library, see `papers config --global default`)"
        )));
    }
    Ok(())
}

fn parse_setting(name: &str, values: &[String]) -> Result<Value> {
    parse_value(name, values).map_err(Error::exit)
}

/// The bibtex of a library has file links: changing the `paths` setting
/// rewrites them, which this port does not do yet.
fn has_file_links(bibtex: &str) -> bool {
    let Ok(text) = fs::read_to_string(bibtex) else { return false };
    let lower = text.to_lowercase();
    lower.match_indices("file").any(|(i, _)| {
        let before_ok = lower[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric() && c != '_');
        let after = lower[i + 4..].trim_start();
        before_ok && after.starts_with('=')
    })
}

fn not_implemented(what: &str) -> Error {
    Error::exit(format!("{what}: not implemented in papers-rs yet (the bibtex has file links)"))
}

pub fn config_cmd(o: &ConfigArgs, mut config: Config) -> Outcome {
    let truthy = |s: &Option<String>| s.as_deref().filter(|s| !s.is_empty()).map(str::to_string);
    if truthy(&o.unset).is_some() && truthy(&o.key).is_some() {
        return Err(Failure::Usage("--unset takes the setting name: papers config --unset KEY".to_string()));
    }
    let name = truthy(&o.unset).or_else(|| truthy(&o.key));
    if let Some(name) = &name {
        check_setting_name(name, o.is_global)?;
    }
    if o.is_global {
        return machine_config_cmd(o, &mut config.machine, name.as_deref());
    }

    let Some(file) = config.file.clone() else {
        return Err(Error::exit(format!("{}, or use `papers config --global`", no_library_message(&config))).into());
    };

    if o.prune {
        if name.is_some() {
            return Err(Failure::Usage("--prune takes no setting: papers config --prune".to_string()));
        }
        return prune(&mut config, o.dry_run);
    }

    let Some(name) = name else {
        log::println(&format!("# {file}"));
        log::println(&config.settings_lines(None).join("\n"));
        return Ok(());
    };

    let unset = truthy(&o.unset).is_some();
    if !unset && o.values.is_empty() {
        log::println(&format_value(&config.get(&name)));
        return Ok(());
    }

    if config.origin == Some(Origin::V2Global) {
        return Err(Error::exit(format!(
            "{file} is the global configuration of papers v2: run `papers migrate` first, to turn it into a library"
        ))
        .into());
    }

    if name == "pool" {
        return set_pool(o, config, unset);
    }

    if unset {
        config.unset(&name)?;
    } else {
        let mut value = parse_setting(&name, &o.values)?;
        if name == "bibtex" || name == "filesdir" {
            let path = value.as_str().unwrap_or_default().to_string();
            if !exists(&path) {
                log::warning(&format!("{path} does not exist"));
            }
            value = Value::String(abspath(&path));
        }
        config.set(&name, value)?;
    }

    if o.dry_run {
        log::println(&format!(
            "DRYRUN: {name} = {} not saved to {file}",
            format_value(&config.get(&name))
        ));
        return Ok(());
    }
    // the file links of the bibtex follow the paths setting
    if name == "paths"
        && let Some(bibtex) = config.bibtex.clone()
        && exists(&bibtex)
        && has_file_links(&bibtex)
    {
        return Err(not_implemented("config paths").into());
    }
    config.save()?;

    if name == "paths"
        && let Some(bibtex) = config.bibtex.as_deref()
        && exists(bibtex)
    {
        // the file links are written as the setting says: none to change
        log::info(&format!("{bibtex}: nothing changed, not saved"));
    }
    if name == "checksums" {
        let checksums = config.get("checksums");
        log::println(if papers_rs::config::json::py_truthy(&checksums) {
            "the files get a checksum when a command moves, copies, adds or links them: `papers file checksum` writes those of all the files"
        } else {
            "the commands write no checksum (undo finds the files through the journal of the commands)"
        });
    }
    Ok(())
}

/// `papers config --prune`: save the library configuration without the
/// settings equal to their inherited value.
fn prune(config: &mut Config, dry_run: bool) -> Outcome {
    let file = config.file.clone().unwrap_or_default();
    if config.origin == Some(Origin::V2Global) {
        return Err(Error::exit(format!("{file} is the global configuration of papers v2: run `papers migrate` first")).into());
    }
    let before = match fs::read_to_string(&file).map_err(|e| e.to_string()).and_then(|t| loads(&t).map_err(|e| e.message)) {
        Ok(Value::Object(map)) => map,
        Ok(_) => return Err(Error::other(format!("{file}: not a JSON object")).into()),
        Err(message) => return Err(Error::other(format!("{file}: {message}")).into()),
    };
    let after = match config.tojson() {
        Value::Object(map) => map,
        _ => unreachable!("a configuration is an object"),
    };
    let mut removed: Vec<String> = settings::names()
        .into_iter()
        .filter(|name| json_get(&before, name).is_some() && json_get(&after, name).is_none())
        .map(str::to_string)
        .collect();
    // the settings of papers before the history (git, git-lfs, trash-days...), no longer used
    removed.extend(
        ["git", "gitlfs", "backup_files", "gitdir", "trash-days", "trash-size"]
            .into_iter()
            .filter(|name| before.contains_key(*name))
            .map(str::to_string),
    );
    if removed.is_empty() {
        log::println(&format!("nothing to remove from {}", short_home(&file)));
        return Ok(());
    }
    if dry_run {
        log::println(&format!("DRYRUN: {} not removed from {}", removed.join(", "), short_home(&file)));
        return Ok(());
    }
    config.save()?;
    // a library configuration of papers 2 is moved when saved: the message names the new file
    let saved = config.file.clone().unwrap_or_default();
    log::println(&format!(
        "removed from {} (equal to the global or default value): {}",
        short_home(&saved),
        removed.join(", ")
    ));
    Ok(())
}

/// `papers config pool PATH|none` and `--unset pool`. In a shallow library,
/// pool none (the library becomes self-contained) copies the pool files that
/// it links to into its files directory. A pool PATH (an existing library
/// without pool) needs each PDF of the library in the pool entry of the same
/// paper: the library then links to the pool's copies, and its own copies are
/// deleted. Only a library whose entries have no files is handled here: for
/// the others, the files are the work of the exchanges with the pool, which
/// this port does not have yet.
fn set_pool(o: &ConfigArgs, mut config: Config, unset: bool) -> Outcome {
    let file = config.file.clone().unwrap_or_default();
    let was_shallow = config.shallow();
    let mut new = config.clone();
    let value = if unset {
        None
    } else {
        let value = parse_setting("pool", &o.values)?;
        Some(pool_setting(value.as_str().unwrap_or_default(), &config.configdir(), Some(&getcwd())))
    };
    match &value {
        None => new.unset("pool")?,
        Some(v) => new.set("pool", Value::String(v.clone()))?,
    }
    if new.has_pool() {
        let what = format!("pool {}:", o.values.first().cloned().unwrap_or_default());
        let pool_path = new.pool_path().unwrap_or_default();
        library_without_pool(&pool_path, &config.machine, &what)?;
    }

    let to_self = was_shallow && !new.has_pool();
    let to_pool = new.has_pool()
        && (!was_shallow
            || papers_rs::config::paths::realpath(&config.pool_path().unwrap_or_default())
                != papers_rs::config::paths::realpath(&new.pool_path().unwrap_or_default()));

    let done = |config: &mut Config| -> Outcome {
        if o.dry_run {
            let shown = format_value(&new.get("pool"));
            log::println(&format!("DRYRUN: pool = {shown} not saved to {}", short_home(&file)));
            return Ok(());
        }
        match &value {
            None => config.unset("pool")?,
            Some(v) => config.set("pool", Value::String(v.clone()))?,
        }
        config.save()?;
        let saved = config.file.clone().unwrap_or_default();
        log::println(&format!("pool = {} (in {})", format_value(&config.get("pool")), short_home(&saved)));
        if to_pool && !was_shallow {
            log::println(
                "the library is now shallow: it links to the PDFs of the pool entries, and `papers add` adds its new entries to the pool",
            );
        } else if to_self {
            log::println("the library is now self-contained: its own entries and files");
        }
        Ok(())
    };

    if !to_self && !to_pool {
        return done(&mut config);
    }

    // the library as `get_biblio` loads it: the files of its entries are what the change moves
    let Some(bibtex) = config.bibtex.clone() else {
        return Err(Error::other("bibtex is not initialized").into());
    };
    if exists(&bibtex) && has_file_links(&bibtex) {
        return Err(not_implemented(&format!("config pool {}", o.values.first().cloned().unwrap_or_default())).into());
    }
    if to_self {
        if config.filesdir.is_none() {
            return Err(Error::exit(
                "the library has no files directory: set it with `papers config filesdir DIR` first",
            )
            .into());
        }
        // no file to copy
        log::println("pool none: the library becomes self-contained, with its own files");
    }
    done(&mut config)
}

/// `papers config --global`.
fn machine_config_cmd(o: &ConfigArgs, machine: &mut MachineConfig, name: Option<&str>) -> Outcome {
    let file = machine.file.clone().unwrap_or_default();
    let Some(name) = name else {
        log::println(&format!("# {file}{}", if machine.exists() { "" } else { " (does not exist)" }));
        let mut settings_names: Vec<&str> = vec![DEFAULT_LIBRARY];
        settings_names.extend(settings::names());
        for setting in settings_names {
            if let Some(value) = machine.get(setting) {
                log::println(&format!("{setting} = {}", format_value(&value)));
            }
        }
        return Ok(());
    };
    let unset = o.unset.as_deref().is_some_and(|s| !s.is_empty());
    if !unset && o.values.is_empty() {
        if let Some(value) = machine.get(name) {
            log::println(&format_value(&value));
        }
        return Ok(());
    }

    let value = if unset {
        None
    } else if name == DEFAULT_LIBRARY {
        if o.values.len() != 1 {
            return Err(Error::exit(format!("{DEFAULT_LIBRARY} takes one value: PATH or none")).into());
        }
        let value = &o.values[0];
        if value == NO_POOL {
            None
        } else {
            let path = abspath(&expanduser(value));
            library_without_pool(&path, machine, "the default library")?;
            Some(Value::String(short_home(&path)))
        }
    } else {
        Some(parse_setting(name, &o.values)?)
    };
    machine.set(name, value);
    if o.dry_run {
        log::println(&format!("DRYRUN: {file} not saved"));
        return Ok(());
    }
    machine.save()?;
    Ok(())
}
