//! The pool setting: how a pool path is written in a configuration, and the
//! check that a path is a library without pool.

use super::files::library_config_file;
use super::library::Config;
use super::machine::MachineConfig;
use super::paths::{abspath, dirname, exists, expanduser, is_abs, isdir, join, relpath, short_home};
use super::settings::{CONFIG_FILE_LOCAL, NO_POOL};
use crate::error::{Error, Result};

/// How the pool setting of a library in `libdir` writes a pool path given on
/// the command line (`base`: the directory a relative path is relative to, by
/// default `libdir`): relative to the library when it is written relative or
/// is under the library (`./` or `../` first), else absolute (`~` for the
/// home directory). `none` stays none.
pub fn pool_setting(path: &str, libdir: &str, base: Option<&str>) -> String {
    if path == NO_POOL {
        return path.to_string();
    }
    let expanded = expanduser(path);
    let full = abspath(&join(base.unwrap_or(libdir), &expanded));
    let libdir = abspath(libdir);
    let inside = full == libdir || full.starts_with(&format!("{}/", libdir.trim_end_matches('/')));
    if !is_abs(&expanded) || inside {
        let rel = relpath(&full, &libdir);
        if rel.starts_with('.') { rel } else { join(".", &rel) }
    } else {
        short_home(&full)
    }
}

/// The error for a pool, or a library given on the command line, that is not
/// a library: `papers init PATH` creates it (a command given a pool never
/// creates it). A missing parent directory may be a disk not mounted.
pub fn not_a_library(what: &str, path: &str) -> String {
    let shown = short_home(path);
    let parent = dirname(&abspath(path));
    let reason = if exists(path) {
        format!("{shown} is not a library (no {CONFIG_FILE_LOCAL})")
    } else if !isdir(&parent) {
        format!("{shown}: {} does not exist (disk not mounted?)", short_home(&parent))
    } else {
        format!("{shown} does not exist")
    };
    format!("{what} {reason}: create it first with `papers init {shown}`")
}

/// The configuration of the library in directory `path`, which must exist and
/// have no pool: the pool of a shallow library, or the default library
/// (`what` names it in the errors).
pub fn library_without_pool(path: &str, machine: &MachineConfig, what: &str) -> Result<Config> {
    let Some(configfile) = library_config_file(path)? else {
        return Err(Error::exit(format!("{} (nothing was changed)", not_a_library(what, path))));
    };
    let config = Config::load(&configfile, machine.clone(), None)?;
    if config.has_pool() {
        return Err(Error::exit(format!(
            "{what} {} has a pool itself ({}): give a library without pool (nothing was changed)",
            short_home(path),
            config.pool_label().unwrap_or_default()
        )));
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_setting_is_relative_or_absolute() {
        assert_eq!(pool_setting("none", "/lib", None), "none");
        assert_eq!(pool_setting("../A", "/x/lib", None), "../A");
        assert_eq!(pool_setting("sub", "/x/lib", None), "./sub");
        assert_eq!(pool_setting("/x/lib/sub", "/x/lib", None), "./sub");
        assert_eq!(pool_setting("/y/other", "/x/lib", None), "/y/other");
        assert_eq!(pool_setting("../other", "/x/lib", Some("/y/z")), "../../y/other");
    }
}
