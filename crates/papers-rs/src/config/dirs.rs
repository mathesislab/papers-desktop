//! Where papers keeps its files on the machine: the paths that `platformdirs`
//! gives it on macOS and Linux.

use super::paths::{expanduser, join};

fn xdg(variable: &str, default: &str) -> String {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => expanduser(default),
    }
}

/// `platformdirs.user_config_dir()`.
fn user_config_dir() -> String {
    if cfg!(target_os = "macos") {
        expanduser("~/Library/Application Support")
    } else {
        xdg("XDG_CONFIG_HOME", "~/.config")
    }
}

/// `platformdirs.user_data_dir()`.
fn user_data_dir() -> String {
    if cfg!(target_os = "macos") {
        expanduser("~/Library/Application Support")
    } else {
        xdg("XDG_DATA_HOME", "~/.local/share")
    }
}

/// `platformdirs.user_cache_dir()`.
fn user_cache_dir() -> String {
    if cfg!(target_os = "macos") {
        expanduser("~/Library/Caches")
    } else {
        xdg("XDG_CACHE_HOME", "~/.cache")
    }
}

/// The global configuration of papers v2 (read for v2 compatibility).
pub fn config_file() -> String {
    join(&user_config_dir(), "papersconfig.json")
}

/// The global configuration: the default library and the default settings.
pub fn machine_config_file() -> String {
    join(&join(&user_config_dir(), "papers"), "config.json")
}

/// The data of papers (its history, unless the history-dir setting says
/// otherwise).
pub fn data_dir() -> String {
    join(&user_data_dir(), "papers")
}

/// The history of the libraries.
pub fn history_dir() -> String {
    join(&data_dir(), "history")
}

/// The cache of papers.
pub fn cache_dir() -> String {
    join(&user_cache_dir(), "papers")
}

fn legacy_config_home() -> String {
    std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| join(&expanduser("~"), ".config"))
}

fn legacy_data_home() -> String {
    std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| join(&join(&expanduser("~"), ".local"), "share"))
}

/// A location of the global configuration used by previous versions
/// (XDG conventions on every platform).
pub fn config_file_legacy() -> String {
    join(&legacy_data_home(), "config.json")
}

/// The other location of the global configuration used by previous versions.
pub fn config_file_legacy_xdg() -> String {
    join(&legacy_config_home(), "papersconfig.json")
}
