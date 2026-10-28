//! What the history needs to know of a library: where its store is, which
//! project and place it is (spec/history.md, sections 2.1, 5.1 and 9.1), and
//! the texts that a state keeps.

use std::fs;

use serde_json::Value;

use super::store::{MB, Store, new_id, sha256};
use crate::config::files::write_atomic;
use crate::config::json::{dumps_pretty, loads};
use crate::config::paths::{abspath, dirname, expanduser, isfile, join, realpath};
use crate::config::{Config, MachineConfig, dirs, library_without_pool};
use crate::error::{Error, Result};

/// The settings and paths of a library that the history uses, taken from its
/// configuration.
#[derive(Debug, Clone)]
pub struct Setup {
    /// The bibtex of the library.
    pub bibtex: String,
    /// The directory of the bibtex: the kept `.papers/` files and the
    /// relative paths of the operations refer to it.
    pub libdir: String,
    /// The library configuration file, for a bibtex of a library (none: a bibtex
    /// without library, whose project is named by its path).
    pub file: Option<String>,
    pub absolute_paths: bool,
    /// The root of the history of the machine (the `history-dir` setting).
    pub root: String,
    /// The `history-separate` setting: a store of its own for the library.
    pub separate: bool,
    /// `history-size` of the library (its own value, else the machine's),
    /// megabytes.
    pub library_size: Option<Value>,
    /// `history-size` of the machine configuration, megabytes.
    pub machine_size: Option<Value>,
    pub keep_deleted_files: bool,
    /// The files directory of the pool of a shallow library: a file moved
    /// there is copied back by undo, not moved.
    pub pool_files_dir: Option<String>,
    /// The pool of the library as the `pool` setting writes it, and its
    /// absolute path: how messages name it.
    pub pool: Option<(String, String)>,
    pub machine: MachineConfig,
    /// The current directory: the paths of the reports are relative to it.
    pub cwd: String,
}

impl Setup {
    pub fn from_config(config: &Config) -> Setup {
        let bibtex = config.bibtex.clone().unwrap_or_default();
        let root = match config.get("history-dir") {
            Value::String(s) if !s.is_empty() => s,
            _ => dirs::history_dir(),
        };
        let pool = match (config.pool_path(), config.pool_label()) {
            (Some(path), Some(label)) => Some((path, label)),
            _ => None,
        };
        Setup {
            libdir: dirname(&abspath(&bibtex)),
            bibtex,
            file: config.file.clone().filter(|_| config.is_library),
            absolute_paths: config.absolute_paths,
            root: expanduser(&root),
            separate: config.get("history-separate") == Value::Bool(true),
            library_size: Some(config.get("history-size")).filter(|v| !v.is_null()),
            machine_size: config.machine.get("history-size"),
            keep_deleted_files: !matches!(config.get("keep-deleted-files"), Value::Bool(false)),
            pool_files_dir: if config.shallow() {
                config
                    .pool_path()
                    .and_then(|p| library_without_pool(&p, &config.machine, "pool").ok())
                    .and_then(|c| c.filesdir)
            } else {
                None
            },
            pool,
            machine: config.machine.clone(),
            cwd: crate::config::paths::getcwd(),
        }
    }

    /// The store of the library `id`: the store shared by the libraries, or
    /// with the `history-separate` setting, a store of its own.
    pub fn store(&self, id: Option<&str>) -> Store {
        match id {
            Some(id) if self.separate => Store::new(join(&join(&self.root, "separate"), id)),
            _ => Store::new(self.root.clone()),
        }
    }

    /// The shared store and the separate ones.
    pub fn all_stores(&self) -> Vec<Store> {
        let mut stores = vec![Store::new(self.root.clone())];
        let separate = join(&self.root, "separate");
        if let Ok(names) = fs::read_dir(&separate) {
            let mut names: Vec<String> = names
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            stores.extend(names.into_iter().map(|n| Store::new(join(&separate, &n))));
        }
        stores
    }

    /// `history-size` in megabytes (0: no limit) of a store: that of the
    /// library for a store of its own, that of the machine for the shared
    /// store.
    pub fn history_size(&self, store: &Store) -> u64 {
        let value = if store.is_separate() {
            self.library_size.as_ref()
        } else {
            self.machine_size.as_ref()
        };
        match value {
            None => 500,
            Some(Value::Number(n)) => n
                .as_i64()
                .or_else(|| n.as_f64().map(|f| f as i64))
                .unwrap_or(0)
                .max(0) as u64,
            Some(Value::Bool(b)) => *b as u64,
            Some(Value::String(s)) => s.trim().parse::<i64>().unwrap_or(0).max(0) as u64,
            Some(_) => 0,
        }
    }

    /// The limit of a store in bytes (0: none).
    pub fn limit(&self, store: &Store) -> u64 {
        self.history_size(store) * MB
    }

    /// The real path of the bibtex: the place of the library.
    pub fn place(&self) -> String {
        realpath(&self.bibtex)
    }
}

/// The identifier of a library, which names its history: a library without
/// one gets one, saved into its configuration file (the identifier only: the
/// settings in memory may be those of the command), unless `create` is false.
pub fn library_id(config: &mut Config, create: bool, dry_run: bool) -> Result<Option<String>> {
    if config.id.is_none() && create && config.is_library {
        let Some(file) = config.file.clone() else {
            return Ok(config.id.clone());
        };
        let id = new_id();
        config.id = Some(id.clone());
        if isfile(&file) && !dry_run {
            let text =
                fs::read_to_string(&file).map_err(|e| Error::other(format!("{file}: {e}")))?;
            let mut js = match loads(&text) {
                Ok(Value::Object(map)) => map,
                Ok(_) | Err(_) => return Err(Error::other(format!("{file}: not a JSON object"))),
            };
            js.insert("id".into(), Value::String(id));
            write_atomic(&file, &dumps_pretty(&Value::Object(js)))?;
        }
    }
    Ok(config.id.clone())
}

/// The project id of a bibtex without library: the first 32 characters of
/// the sha256 of the bytes of its place.
pub fn path_id(place: &str) -> String {
    sha256(place.as_bytes())[..32].to_string()
}

// the texts of a state
// ====================

/// The files of `.papers/` that belong to the states of a library, relative
/// to the directory of the bibtex: those of these that exist as files.
pub fn kept_names(libdir: &str) -> Vec<String> {
    let mut names: Vec<String> = [
        ".papers/distinct.json",
        ".papers/origin.bib",
        ".papers/files.json",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Ok(entries) = fs::read_dir(join(libdir, ".papers/origin")) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".bib") {
                names.push(format!(".papers/origin/{name}"));
            }
        }
    }
    names.sort();
    names.dedup();
    names
        .into_iter()
        .filter(|n| isfile(&join(libdir, n)))
        .collect()
}

fn read_text(path: &str) -> Result<String> {
    let bytes = fs::read(path).map_err(|e| Error::other(format!("{path}: {e}")))?;
    String::from_utf8(bytes).map_err(|_| {
        Error::other(format!(
            "{path}: not UTF-8 text: it cannot be recorded in the history"
        ))
    })
}

/// The text of the bibtex and of the kept `.papers/` files, as they are (no
/// translation of the line ends).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub bibtex: String,
    /// Name (relative to the directory of the bibtex) and text, by name.
    pub files: Vec<(String, String)>,
}

impl Snapshot {
    /// The library as it is on disk: the bibtex (empty when it does not exist) and the kept files.
    pub fn read(bibtex: &str, libdir: &str) -> Result<Snapshot> {
        Ok(Snapshot {
            bibtex: read_bibtex(bibtex)?,
            files: read_kept(libdir)?,
        })
    }
}

/// The text of the bibtex as it is on disk; empty when it does not exist.
pub fn read_bibtex(bibtex: &str) -> Result<String> {
    if crate::config::paths::exists(bibtex) {
        read_text(bibtex)
    } else {
        Ok(String::new())
    }
}

/// The kept `.papers/` files as they are on disk.
pub fn read_kept(libdir: &str) -> Result<Vec<(String, String)>> {
    kept_names(libdir)
        .into_iter()
        .map(|n| Ok((n.clone(), read_text(&join(libdir, &n))?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        assert!(kept_names(&root).is_empty());
        for (name, text) in [
            (".papers/distinct.json", "[]"),
            (".papers/config.json", "{}"),
            (".papers/lock", "x"),
            (".papers/origin/b.bib", "@a{k}"),
            (".papers/origin/a.bib", "@a{k}"),
            (".papers/origin/note.txt", "x"),
            (".papers/files.json", "{}"),
        ] {
            fs::create_dir_all(dirname(&join(&root, name))).unwrap();
            fs::write(join(&root, name), text).unwrap();
        }
        assert_eq!(
            kept_names(&root),
            vec![
                ".papers/distinct.json",
                ".papers/files.json",
                ".papers/origin/a.bib",
                ".papers/origin/b.bib"
            ]
        );
    }

    #[test]
    fn the_id_of_a_bibtex_without_library() {
        let id = path_id("/home/u/papers.bib");
        assert_eq!(id.len(), 32);
        assert_eq!(id, sha256(b"/home/u/papers.bib")[..32]);
    }

    #[test]
    fn history_size_defaults_and_scope() {
        let mut setup = Setup::from_config(&Config::new(MachineConfig::empty()));
        let shared = Store::new("/h");
        let separate = Store::new("/h/separate/abc");
        assert_eq!(setup.history_size(&shared), 500);
        setup.machine_size = Some(Value::from(10));
        setup.library_size = Some(Value::from(3));
        assert_eq!(
            (setup.history_size(&shared), setup.history_size(&separate)),
            (10, 3)
        );
        setup.machine_size = Some(Value::from(-4));
        assert_eq!(setup.limit(&shared), 0);
    }
}
