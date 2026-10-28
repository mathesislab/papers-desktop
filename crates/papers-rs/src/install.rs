//! Creating a library (`papers init`): the configuration it starts with,
//! decided from the options, the files of the current directory and the
//! defaults, and the files it creates. Nothing is asked: every setting has a
//! default.

use std::fs;

use serde_json::Value;

use crate::config::paths::{dirname, exists, realpath};
use crate::config::settings::NO_POOL;
use crate::config::{CONFIG_FILE_LOCAL, Config, pool_setting};
use crate::error::{Error, Result};
use crate::log;

/// The words that clear a value on the command line.
const RESET_WORDS: [&str; 6] = ["none", "null", "unset", "undefined", "reset", "delete"];

/// The folders that a new library takes as its files directory when they
/// exist, in this order.
const CHECKDIRS: [&str; 5] = ["files", "pdfs", "pdf", "papers", "bibliography"];

/// What the command line says about the library to create. Paths are
/// relative to the current directory.
#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    pub bibtex: Option<String>,
    pub filesdir: Option<String>,
    /// `relative` or `absolute`
    pub paths: Option<String>,
    /// `--pool PATH|none`
    pub pool: Option<String>,
    pub editor: Option<String>,
    /// The name and key format settings (`name.author-num`), by name.
    pub formats: Vec<(String, Value)>,
}

/// The configuration to write, and the files to create.
#[derive(Debug, Clone)]
pub struct InstallPlan {
    pub config: Config,
    pub create_bibtex: bool,
    pub create_filesdir: bool,
}

/// The `.bib` files of the current directory, sorted.
fn bibtex_files() -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(".")
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".bib"))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

/// Combine the options, the filesystem and the defaults into the
/// configuration of a new library (the current directory is the library
/// directory), to be written to `target` (relative to it).
pub fn resolve_install(mut config: Config, o: &InstallOptions, target: &str) -> Result<InstallPlan> {
    // --- bibtex ---
    let default_bibtex = "papers.bib".to_string();
    let mut seen = vec![realpath(&default_bibtex)];
    let mut candidates = vec![default_bibtex.clone()];
    for file in bibtex_files() {
        let resolved = realpath(&file);
        if !seen.contains(&resolved) {
            seen.push(resolved);
            candidates.push(file);
        }
    }
    candidates.retain(|f| exists(f));

    let bibtex = match o.bibtex.as_deref().filter(|b| !b.is_empty()) {
        None => {
            if candidates.len() > 1 {
                log::warning(&format!("Several bibtex files found: {}", candidates.join(" ")));
            }
            Some(candidates.first().cloned().unwrap_or(default_bibtex))
        }
        Some(b) if RESET_WORDS.contains(&b.to_lowercase().as_str()) => None,
        Some(b) => Some(b.to_string()),
    };

    // --- files directory ---
    let default_filesdir = CHECKDIRS.iter().find(|d| exists(d)).map(|d| d.to_string()).unwrap_or_else(|| "files".to_string());
    let filesdir = match o.filesdir.as_deref().filter(|f| !f.is_empty()) {
        None => Some(default_filesdir),
        Some(f) if RESET_WORDS.contains(&f.to_lowercase().as_str()) => None,
        Some(f) => Some(f.to_string()),
    };

    config.bibtex = bibtex;
    config.filesdir = filesdir;
    config.file = Some(target.to_string());
    config.is_library = true;

    // a new library stores these settings rather than inheriting them later:
    // they determine its content (the file links) and where it looks for entries
    match &o.paths {
        Some(paths) => config.set("paths", Value::String(paths.clone()))?,
        None => {
            let current = config.get("paths");
            config.set("paths", current)?;
        }
    }

    // the pool: --pool PATH|none (a relative path is relative to the library), else none
    match &o.pool {
        Some(pool) => {
            let setting = pool_setting(pool, &config.configdir(), None);
            config.set("pool", Value::String(setting))?;
        }
        None => config.set("pool", Value::String(NO_POOL.to_string()))?,
    }

    if let Some(editor) = o.editor.as_deref().filter(|e| !e.is_empty()) {
        config.set("editor", Value::String(editor.to_string()))?;
    }
    for (name, value) in &o.formats {
        config.set(name, value.clone())?;
    }

    let create_bibtex = config.bibtex.as_deref().is_some_and(|b| !b.is_empty() && !exists(b));
    // a shallow library places its files in the pool: its files directory
    // is created when a file is written there
    let create_filesdir =
        config.filesdir.as_deref().is_some_and(|f| !f.is_empty() && !exists(f)) && !config.has_pool();
    Ok(InstallPlan { config, create_bibtex, create_filesdir })
}

/// Create what the plan says (the empty bibtex, the files directory) and
/// write the configuration.
pub fn apply_install(plan: &mut InstallPlan) -> Result<()> {
    if plan.create_bibtex {
        let bibtex = plan.config.bibtex.clone().unwrap_or_default();
        log::info(&format!("create empty bibliography database: {bibtex}"));
        let parent = dirname(&bibtex);
        if !parent.is_empty() {
            fs::create_dir_all(&parent).map_err(|e| Error::other(format!("{parent}: {e}")))?;
        }
        fs::write(&bibtex, "").map_err(|e| Error::other(format!("{bibtex}: {e}")))?;
    }
    if plan.create_filesdir {
        let filesdir = plan.config.filesdir.clone().unwrap_or_default();
        log::info(&format!("create empty files directory: {filesdir}"));
        fs::create_dir_all(&filesdir).map_err(|e| Error::other(format!("{filesdir}: {e}")))?;
    }
    let file = plan.config.file.clone().unwrap_or_else(|| CONFIG_FILE_LOCAL.to_string());
    log::info(&format!("save config file: {file}"));
    plan.config.save()
}
