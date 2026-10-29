//! The configuration of papers: the library configuration
//! (`.papers/config.json`), the global configuration (the default library and
//! the default settings), the settings of `papers config` with their sources
//! (library, else global, else built-in default), and how the library of a
//! directory is found.
//!
//! The files written are byte-identical to those of papers (key order,
//! indentation, no trailing newline); papers' paths are strings with
//! Python's `os.path` semantics (see `paths`).

pub mod dirs;
pub mod files;
pub mod json;
pub mod library;
pub mod machine;
pub mod paths;
pub mod pools;
pub mod settings;

pub use files::{
    check_legacy_global_config, dry_run, find_config, find_library_config, ignore_papers_folder, is_v2_config_name,
    library_config_file, move_v2_config, set_dry_run, write_atomic, as_dry_run,
};
pub use library::{Config, Origin, config_root, library_dir};
pub use machine::MachineConfig;
pub use paths::short_home;
pub use pools::{library_without_pool, not_a_library, pool_setting};
pub use settings::{
    CONFIG_FILE_LOCAL, CONFIG_FILE_LOCAL_V2, CONFIG_VERSION, DEFAULT_LIBRARY, Kind, LIBRARY_ONLY, LIBRARY_SETTINGS,
    MACHINE_ONLY, NO_POOL, default_value, format_value, json_get, json_set, parse_value, removed_setting,
};
