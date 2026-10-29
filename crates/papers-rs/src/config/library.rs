//! The configuration of a library: `.papers/config.json`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

use super::dirs::config_file;
use super::files::{dry_run, ignore_papers_folder, is_v2_config_name, move_v2_config, write_atomic};
use super::json::{dumps_pretty, py_eq, py_str, py_truthy};
use super::machine::{MachineConfig, file_version, io_message, read_json_object, single_pool};
use super::paths::{
    abspath, basename, dirname, exists, expanduser, getcwd, isdir, join, normpath, realpath, relpath, short_home,
};
use super::settings::{
    CONFIG_VERSION, LIBRARY_ONLY, LIBRARY_SETTINGS, MACHINE_ONLY, NO_POOL, SETTINGS, check_settings, format_value,
    is_known_key, json_get, json_set, names,
};
use crate::error::{Error, Result};
use crate::log;

/// How the configuration that a command acts on was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A library configuration in the directory of the command or above.
    Library,
    /// The global configuration of papers v2, until `papers migrate`.
    V2Global,
    /// The default library of the global configuration.
    Default,
}

/// The directory that the relative paths of a library configuration file
/// refer to, the library directory: the parent of `.papers` for
/// `.papers/config.json`, the directory of the file for the
/// `.papersconfig.json` of papers 2.
pub fn config_root(file: &str) -> String {
    let absolute = file.starts_with('/');
    let parts: Vec<&str> = file.split('/').filter(|c| !c.is_empty() && *c != ".").collect();
    let keep = match parts.as_slice() {
        [.., parent, name] if *name == "config.json" && *parent == ".papers" => parts.len() - 2,
        [] => 0,
        _ => parts.len() - 1,
    };
    let joined = parts[..keep].join("/");
    match (absolute, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".to_string(),
        (false, false) => joined,
    }
}

/// The folder whose `.papers/` holds the files of the library of `bibtex` (`distinct.json`, `origin/`,
/// `files.json`, `lock`): that of its library configuration file `file` (see `config_root`), else the
/// folder of the bibtex (a bibtex without library).
pub fn library_dir(bibtex: Option<&str>, file: Option<&str>) -> String {
    match (file, bibtex) {
        (Some(file), _) => abspath(&config_root(file)),
        (None, Some(bibtex)) => dirname(&abspath(bibtex)),
        (None, None) => getcwd(),
    }
}

/// The files of `.papers/` that earlier v3 builds wrote next to the bibtex, not in the library folder.
const PAPERS_FILES_NEXT_TO_BIBTEX: [&str; 3] = ["distinct.json", "origin", "origin.bib"];

/// `shutil.move`: a rename, else (another file system) a copy and a removal.
fn move_path(src: &str, dst: &str) -> std::io::Result<()> {
    fn copy_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
        if src.is_dir() {
            std::fs::create_dir_all(dst)?;
            for entry in std::fs::read_dir(src)? {
                let entry = entry?;
                copy_all(&entry.path(), &dst.join(entry.file_name()))?;
            }
            Ok(())
        } else {
            std::fs::copy(src, dst).map(|_| ())
        }
    }
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_all(std::path::Path::new(src), std::path::Path::new(dst))?;
    if std::path::Path::new(src).is_dir() { std::fs::remove_dir_all(src) } else { std::fs::remove_file(src) }
}

fn path_value(value: &Value, what: &str) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        other => Err(Error::other(format!("{what}: expected a path, got {}", py_str(other)))),
    }
}

/// The configuration of a library
///
/// Each setting comes from the library's configuration file, else from the
/// global configuration, else from its built-in default; the pool comes only
/// from the library's file (none if it does not set it). The fields hold the
/// resulting values; `explicit` names the settings set in the file. The file
/// keeps the settings that the library set explicitly, and those that differ
/// from the global or default value (see `is_set`).
///
/// A library without pool is self-contained: its own entries and files. A
/// library with a pool is shallow (see `shallow`): it links to the files of
/// the pool entries, and adds its new entries to the pool.
#[derive(Debug, Clone)]
pub struct Config {
    pub file: Option<String>,
    /// False for the global configuration of v2 (v2 compatibility).
    pub is_library: bool,
    pub machine: MachineConfig,
    /// How the configuration was found (see `find_config`).
    pub origin: Option<Origin>,
    pub explicit: BTreeSet<String>,
    pub filesdir: Option<String>,
    pub bibtex: Option<String>,
    /// The identifier of the library: its history.
    pub id: Option<String>,
    pub absolute_paths: bool,
    /// The pool: a path (relative to the library), or `none`.
    pub pool: String,
    /// The other settings, by name (see `SETTINGS`).
    values: BTreeMap<String, Value>,
    /// The content of the configuration file, for the keys that papers does not know (see `tojson`).
    raw: Map<String, Value>,
}

impl Config {
    /// A configuration without library: the inherited settings only.
    pub fn new(machine: MachineConfig) -> Config {
        let mut config = Config {
            file: None,
            is_library: false,
            machine,
            origin: None,
            explicit: BTreeSet::new(),
            filesdir: None,
            bibtex: None,
            id: None,
            absolute_paths: false,
            pool: NO_POOL.to_string(),
            values: BTreeMap::new(),
            raw: Map::new(),
        };
        for (name, _) in SETTINGS.iter() {
            if !["bibtex", "filesdir", "pool", "paths"].contains(&name.as_str()) {
                let value = config.inherited(name);
                config.values.insert(name.clone(), value);
            }
        }
        config.absolute_paths = py_eq(&config.inherited("paths"), &json!("absolute"));
        config
    }

    // settings by name (see SETTINGS)
    // -------------------------------

    /// The value of a setting when the library does not set it: the global
    /// configuration, else the built-in default. The pool is none: the global
    /// configuration has no pool.
    pub fn inherited(&self, name: &str) -> Value {
        if name == "pool" {
            return Value::String(NO_POOL.to_string());
        }
        self.machine.get(name).unwrap_or_else(|| super::settings::default_value(name))
    }

    /// The current value of a setting (`Null`: none).
    pub fn get(&self, name: &str) -> Value {
        match name {
            "bibtex" => self.bibtex.clone().map(Value::String).unwrap_or(Value::Null),
            "filesdir" => self.filesdir.clone().map(Value::String).unwrap_or(Value::Null),
            "pool" => Value::String(self.pool.clone()),
            "paths" => json!(if self.absolute_paths { "absolute" } else { "relative" }),
            _ => self.values.get(name).cloned().unwrap_or(Value::Null),
        }
    }

    /// Set a setting of the library (saved by `save`).
    pub fn set(&mut self, name: &str, value: Value) -> Result<()> {
        match name {
            "bibtex" => self.bibtex = path_value(&value, "bibtex")?,
            "filesdir" => self.filesdir = path_value(&value, "filesdir")?,
            "pool" => match value {
                Value::String(s) => self.pool = s,
                other => return Err(Error::other(format!("pool: expected a path, got {}", py_str(&other)))),
            },
            "paths" => self.absolute_paths = py_eq(&value, &json!("absolute")),
            _ => {
                self.values.insert(name.to_string(), value);
            }
        }
        self.explicit.insert(name.to_string());
        Ok(())
    }

    /// Remove a setting from the library: the global or default value applies
    /// again (pool: none).
    pub fn unset(&mut self, name: &str) -> Result<()> {
        self.explicit.remove(name);
        let value = if LIBRARY_ONLY.contains(&name) { Value::Null } else { self.inherited(name) };
        self.set(name, value)?;
        self.explicit.remove(name);
        Ok(())
    }

    /// The library sets this setting: a setting given explicitly (in its configuration file, by
    /// `papers init` or `papers config NAME VALUE`), even with the global or default value, and a
    /// setting whose value differs from it; the others are inherited (not saved).
    pub fn is_set(&self, name: &str) -> bool {
        if LIBRARY_ONLY.contains(&name) {
            return !self.get(name).is_null();
        }
        if MACHINE_ONLY.contains(&name) {
            return false;
        }
        self.explicit.contains(name) || !py_eq(&self.get(name), &self.inherited(name))
    }

    /// Where the value of a setting comes from: `library`, `global` or
    /// `default`.
    pub fn source(&self, name: &str) -> &'static str {
        if self.file.is_some() && self.is_set(name) {
            return "library";
        }
        if !["bibtex", "filesdir", "pool"].contains(&name) && self.machine.get(name).is_some() {
            return "global";
        }
        "default"
    }

    // pool
    // ----

    /// The directory that the relative paths of the configuration file refer
    /// to.
    pub fn configdir(&self) -> String {
        match &self.file {
            Some(file) => realpath(&config_root(file)),
            None => getcwd(),
        }
    }

    /// The folder whose `.papers/` holds the files of the library (see `library_dir`).
    pub fn libdir(&self) -> String {
        let file = if self.is_library { self.file.as_deref() } else { None };
        library_dir(self.bibtex.as_deref(), file)
    }

    /// The absolute path of the pool of the library (the pool setting,
    /// relative to the library), or `None` (pool none, or no library).
    pub fn pool_path(&self) -> Option<String> {
        if self.file.is_none() || self.pool == NO_POOL {
            return None;
        }
        Some(abspath(&join(&self.configdir(), &expanduser(&self.pool))))
    }

    /// The pool as the pool setting writes it (relative to the library, or
    /// with `~`), or `None`.
    pub fn pool_label(&self) -> Option<String> {
        self.pool_path().map(|_| self.pool.clone())
    }

    /// The library is its own pool (e.g. pool `.`), so it has none.
    pub fn is_pool_library(&self) -> bool {
        match self.pool_path() {
            Some(path) if self.file.is_some() => realpath(&path) == realpath(&self.configdir()),
            _ => false,
        }
    }

    pub fn has_pool(&self) -> bool {
        self.pool_path().is_some() && !self.is_pool_library()
    }

    /// A library with a pool: its entries link to the files of the pool
    /// entries, and its new entries go to the pool.
    pub fn shallow(&self) -> bool {
        self.file.is_some() && self.has_pool()
    }

    /// For a command on a shallow library: its files go to the files directory of its pool,
    /// named with the pool's `name.*` settings (`use_pool_files`). Only this configuration in memory
    /// changes: the settings are not set in the library.
    pub fn use_pool_files(&mut self, pool: &Config) {
        self.filesdir = pool.filesdir.clone();
        for (name, value) in &pool.values {
            if name.starts_with("name.") {
                self.values.insert(name.clone(), value.clone());
            }
        }
    }

    /// The settings that a shallow library does not use: its files are in the
    /// pool, named with the pool's template (filesdir is where
    /// `papers config pool none` copies them).
    pub fn unused_while_shallow(&self, name: &str) -> bool {
        self.shallow() && (name == "filesdir" || name.starts_with("name."))
    }

    /// The lines `name = value (source)` of `papers config`: bibtex and
    /// filesdir as absolute paths.
    pub fn settings_lines(&self, names: Option<&[&str]>) -> Vec<String> {
        let all: Vec<&str> = super::settings::names();
        let names = names.unwrap_or(&all);
        let width = names.iter().map(|n| n.chars().count()).max().unwrap_or(0);
        names
            .iter()
            .map(|name| {
                let value = self.get(name);
                let note = if self.unused_while_shallow(name) { ", not used: shallow library" } else { "" };
                format!("{name:<width$} = {}  ({}{note})", format_value(&value), self.source(name))
            })
            .collect()
    }

    // paths
    // -----

    fn relative_to_configdir(&self, path: &str) -> String {
        if !self.is_library {
            // v2 compatibility: absolute
            return realpath(path);
        }
        // relative to the directory of the configuration file, with `..`
        // components if needed; a relative path is relative to the current
        // directory
        relpath(&realpath(path), &self.configdir())
    }

    fn absolute_from(&self, path: &str, root: Option<&str>) -> String {
        if !self.is_library {
            return realpath(path);
        }
        // normpath, not realpath: `..` is removed, the symbolic links are kept; `~` is the home directory
        let base = match root {
            Some(root) => realpath(root),
            None => self.configdir(),
        };
        normpath(&join(&base, &expanduser(path)))
    }

    // file
    // ----

    /// The content of the configuration file: the paths, and the settings
    /// that the library sets (the others come from the global configuration
    /// when loaded), over the keys of the file that papers does not know (kept
    /// as they are, also in `nameformat` and `keyformat`).
    pub fn tojson(&self) -> Value {
        let path = |p: &Option<String>| p.as_deref().map(|p| Value::String(self.relative_to_configdir(p)));
        let mut js: Map<String, Value> =
            self.raw.iter().filter(|(key, _)| !is_known_key(key)).map(|(k, v)| (k.clone(), v.clone())).collect();
        js.insert("filesdir".into(), path(&self.filesdir).unwrap_or(Value::Null));
        js.insert("bibtex".into(), path(&self.bibtex).unwrap_or(Value::Null));
        for name in LIBRARY_SETTINGS.iter() {
            let value = if self.is_set(name) { Some(self.get(name)).filter(|v| !v.is_null()) } else { None };
            json_set(&mut js, name, value);
        }
        if self.is_library
            && let Some(id) = self.id.as_deref().filter(|id| !id.is_empty())
        {
            js.insert("id".into(), json!(id));
        }
        if self.is_library {
            js.insert("version".into(), json!(CONFIG_VERSION));
        } else {
            // v2 compatibility: the file format of the global configuration of v2
            js.insert("local".into(), json!(false));
        }
        Value::Object(js)
    }

    /// Write the configuration file; a library configuration of papers 2
    /// (`.papersconfig.json`) is moved to `.papers/config.json` first.
    pub fn save(&mut self) -> Result<()> {
        let Some(file) = self.file.clone() else {
            return Err(Error::other("the configuration has no file"));
        };
        let mut file = file;
        if self.is_library && is_v2_config_name(&file) {
            file = move_v2_config(&file, false)?;
            self.file = Some(file.clone());
        }
        let dir = dirname(&file);
        if !dir.is_empty() {
            std::fs::create_dir_all(&dir).map_err(|e| Error::other(format!("{dir}: {}", super::machine::io_message(&e))))?;
        }
        write_atomic(&file, &dumps_pretty(&self.tojson()))?;
        if self.is_library {
            ignore_papers_folder(&dir);
        }
        Ok(())
    }

    /// Read a configuration file. `is_library` is false for the global
    /// configuration of v2 (by default the file content or name tells them
    /// apart).
    pub fn load(file: &str, machine: MachineConfig, is_library: Option<bool>) -> Result<Config> {
        let mut js = read_json_object(file)?;
        let raw = js.clone();
        let version = file_version(&mut js, file)?;
        let mut is_library = is_library;
        if is_library.is_none() && version >= 3 {
            is_library = Some(true); // v3 has no global configuration
        }
        if version < 3 {
            // v2 compatibility
            let v2_local = js.remove("local");
            if is_library.is_none() {
                is_library = v2_local.filter(|v| !v.is_null()).map(|v| py_truthy(&v));
            }
        }
        let is_library = is_library.unwrap_or_else(|| basename(file) != basename(&config_file()));
        read_earlier_v3(&mut js, &machine, file)?;
        check_settings(&js, file, &names())?;
        let mut config = Config::new(machine);
        config.raw = raw;
        config.file = Some(file.to_string());
        config.is_library = is_library;
        config.bibtex = path_value(js.get("bibtex").unwrap_or(&Value::Null), "bibtex")?;
        config.filesdir = path_value(js.get("filesdir").unwrap_or(&Value::Null), "filesdir")?;
        // an identifier that papers did not write (e.g. a path, in a library received from someone) is ignored
        if is_library
            && let Some(id) = js.get("id").and_then(|v| v.as_str())
            && id.len() == 32
            && id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
        {
            config.id = Some(id.to_string());
        }
        if !is_library && json_get(&js, "paths").is_none() {
            // v2 compatibility: the default of v2 for a global configuration
            js.insert("paths".into(), json!("absolute"));
        }
        for name in LIBRARY_SETTINGS.iter() {
            if let Some(value) = json_get(&js, name) {
                config.set(name, value)?;
            }
        }
        config.update_paths_to_absolute();
        config.move_papers_files();
        Ok(config)
    }

    /// Move `distinct.json` and `origin/` (`origin.bib`) from the `.papers/` next to the bibtex,
    /// where earlier v3 builds wrote them, to the `.papers/` of the library folder: when the two
    /// folders differ, for a file the library folder has not, and unless the folder of the bibtex
    /// is another library. Not with a dry run (`config::dry_run`): the command line of the program
    /// says if it is one, after the library is loaded, and the program calls this then.
    pub fn move_papers_files(&self) {
        let Some(bibtex) = self.bibtex.as_deref().filter(|_| self.file.is_some() && self.is_library) else { return };
        if dry_run() {
            return;
        }
        let old = join(&dirname(&abspath(bibtex)), ".papers");
        let new = join(&self.libdir(), ".papers");
        if realpath(&old) == realpath(&new) || !isdir(&old) || exists(&join(&old, "config.json")) {
            return;
        }
        let mut moved = Vec::new();
        for name in PAPERS_FILES_NEXT_TO_BIBTEX {
            let (src, dst) = (join(&old, name), join(&new, name));
            if exists(&src) && !exists(&dst) {
                let result = std::fs::create_dir_all(&new).and_then(|()| move_path(&src, &dst));
                match result {
                    Ok(()) => moved.push(name),
                    Err(error) => {
                        log::warning(&format!("{src}: not moved to {dst} ({})", io_message(&error)));
                    }
                }
            }
        }
        if !moved.is_empty() {
            log::info(&format!("moved {} from {} to {}", moved.join(", "), short_home(&old), short_home(&new)));
        }
    }

    fn update_paths_to_absolute(&mut self) {
        let Some(file) = self.file.clone() else { return };
        let root = config_root(&file);
        if let Some(bibtex) = self.bibtex.take() {
            self.bibtex = Some(self.absolute_from(&bibtex, Some(&root)));
        }
        if let Some(filesdir) = self.filesdir.take() {
            self.filesdir = Some(self.absolute_from(&filesdir, Some(&root)));
        }
    }
}

/// The settings of the files written by earlier v3 builds, read as the
/// current ones: `pools` (a list of names and paths) is the pool (its first
/// entry; the name `default` is the default library). The settings `light`
/// and `local` are gone: a library with a pool is shallow.
fn read_earlier_v3(js: &mut Map<String, Value>, machine: &MachineConfig, file: &str) -> Result<()> {
    let pools = js.remove("pools");
    for name in ["light", "local", "sync"] {
        js.remove(name);
    }
    single_pool(js, file)?;
    if let Some(Value::Array(pools)) = pools
        && !js.contains_key("pool")
    {
        let first = match pools.first() {
            Some(Value::String(s)) => s.clone(),
            Some(_) => return Ok(()),
            None => NO_POOL.to_string(),
        };
        if first == "default" {
            js.insert("pool".into(), json!(machine.default_library().unwrap_or_else(|| NO_POOL.to_string())));
        } else if first == NO_POOL || first.contains('/') || first.starts_with(['.', '~']) {
            js.insert("pool".into(), json!(first));
        } else if let Some(path) = machine.pool_names.get(&first) {
            js.insert("pool".into(), path.clone());
        } else {
            log::warning(&format!("{file}: unknown pool name {first}: the library has no pool"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::json::loads;

    #[test]
    fn config_root_of_the_files() {
        assert_eq!(config_root("/a/b/.papers/config.json"), "/a/b");
        assert_eq!(config_root(".papers/config.json"), ".");
        assert_eq!(config_root("./.papers/config.json"), ".");
        assert_eq!(config_root("/a/b/.papersconfig.json"), "/a/b");
        assert_eq!(config_root("/a/config.json"), "/a");
        assert_eq!(config_root("/.papers/config.json"), "/");
    }

    fn machine(data: Value) -> MachineConfig {
        MachineConfig::new(Some("/machine/config.json"), data.as_object().unwrap().clone())
    }

    fn canonical(dir: &std::path::Path) -> String {
        std::fs::canonicalize(dir).unwrap().to_string_lossy().into_owned()
    }

    fn library(dir: &std::path::Path, js: &str) -> String {
        let file = format!("{}/.papers/config.json", canonical(dir));
        std::fs::create_dir_all(dirname(&file)).unwrap();
        std::fs::write(&file, js).unwrap();
        file
    }

    #[test]
    fn settings_come_from_the_library_then_the_global_configuration_then_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let file = library(
            dir.path(),
            r#"{"bibtex": "lib.bib", "keyformat": {"template": "{author}"}, "paths": "absolute", "version": 3}"#,
        );
        let machine = machine(json!({
            "editor": "vim", "keyformat": {"template": "{year}"}, "nameformat": {"template": "{title}"}, "paths": "relative"
        }));
        let config = Config::load(&file, machine, None).unwrap();
        assert_eq!(config.get("key.template"), json!("{author}"));
        assert_eq!(config.source("key.template"), "library");
        assert!(config.absolute_paths);
        assert_eq!(config.source("paths"), "library");
        assert_eq!(config.get("name.template"), json!("{title}"));
        assert_eq!(config.source("name.template"), "global");
        assert_eq!(config.get("editor"), json!("vim"));
        assert_eq!(config.source("editor"), "global");
        assert_eq!(config.get("name.author-num"), json!(2));
        assert_eq!(config.source("name.author-num"), "default");
        // the pool comes only from the library: without pool setting, none
        assert_eq!(config.pool, "none");
        assert_eq!(config.source("pool"), "default");
        assert!(config.pool_path().is_none());
        // the paths are absolute once loaded
        assert_eq!(config.bibtex.as_deref(), Some(format!("{}/lib.bib", config_root(&file)).as_str()));
        assert!(config.filesdir.is_none());
    }

    #[test]
    fn saving_keeps_the_inherited_settings_out() {
        let dir = tempfile::tempdir().unwrap();
        let file = library(
            dir.path(),
            r#"{"bibtex": "lib.bib", "keyformat": {"template": "{author}"}, "paths": "absolute"}"#,
        );
        let machine = machine(json!({"editor": "vim", "keyformat": {"template": "{year}"}, "nameformat": {"template": "{title}"}}));
        let mut config = Config::load(&file, machine, None).unwrap();
        config.save().unwrap();
        let saved = loads(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(saved["keyformat"], json!({"template": "{author}"}));
        assert!(saved.get("nameformat").is_none());
        assert!(saved.get("editor").is_none());
        assert_eq!(saved["version"], json!(3));
        // a changed setting is saved, and the folder of the library ignores its internals
        config.set("name.author-num", json!(1)).unwrap();
        config.save().unwrap();
        let saved = loads(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(saved["nameformat"], json!({"author_num": 1}));
        assert_eq!(std::fs::read_to_string(dirname(&file) + "/.gitignore").unwrap(), "*\n");
    }

    #[test]
    fn the_file_is_written_as_papers_writes_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = library(dir.path(), "{}");
        let mut config = Config::load(&file, MachineConfig::empty(), None).unwrap();
        config.bibtex = Some(format!("{}/papers.bib", config_root(&file)));
        config.filesdir = Some(format!("{}/../pdfs", config_root(&file)));
        config.set("pool", json!("../pool")).unwrap();
        config.set("editor", json!("subl -w")).unwrap();
        config.set("name.unknown-strict", json!(true)).unwrap();
        config.save().unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "{\n  \"bibtex\": \"papers.bib\",\n  \"editor\": \"subl -w\",\n  \"filesdir\": \"../pdfs\",\n  \"nameformat\": {\n    \"unknown_strict\": true\n  },\n  \"pool\": \"../pool\",\n  \"version\": 3\n}"
        );
    }

    #[test]
    fn unset_gives_back_the_inherited_value() {
        let dir = tempfile::tempdir().unwrap();
        let file = library(dir.path(), r#"{"keyformat": {"template": "{author}"}}"#);
        let mut config = Config::load(&file, machine(json!({"keyformat": {"template": "{year}"}})), None).unwrap();
        config.unset("key.template").unwrap();
        assert_eq!(config.get("key.template"), json!("{year}"));
        assert_eq!(config.source("key.template"), "global");
        assert!(config.tojson().get("keyformat").is_none());
    }

    #[test]
    fn libraries_of_papers_2_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = library(
            dir.path(),
            r#"{"bibtex": "lib.bib", "local": true, "absolute_paths": true, "backup_files": true, "git": true,
               "nameformat": {"template": "{author}", "author_num": 3}}"#,
        );
        let mut config = Config::load(&file, MachineConfig::empty(), None).unwrap();
        assert!(config.is_library);
        assert_eq!(config.get("paths"), json!("absolute"));
        assert_eq!(config.get("name.author-num"), json!(3));
        // self-contained: no pool
        assert!(!config.shallow());
        config.save().unwrap();
        let saved = loads(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(saved["paths"], json!("absolute"));
        assert!(saved.get("absolute_paths").is_none() && saved.get("git").is_none() && saved.get("local").is_none());
        assert_eq!(saved["version"], json!(3));
    }

    #[test]
    fn the_v2_library_file_moves_when_saved() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        let old = format!("{root}/.papersconfig.json");
        std::fs::write(&old, r#"{"bibtex": "lib.bib", "local": true}"#).unwrap();
        let mut config = Config::load(&old, MachineConfig::empty(), None).unwrap();
        config.save().unwrap();
        assert!(!std::path::Path::new(&old).exists());
        assert_eq!(config.file.as_deref(), Some(format!("{root}/.papers/config.json").as_str()));
        assert!(std::path::Path::new(&format!("{root}/.papers/config.json")).exists());
    }

    #[test]
    fn the_global_configuration_of_papers_2_has_absolute_paths() {
        let dir = tempfile::tempdir().unwrap();
        let file = format!("{}/papersconfig.json", canonical(dir.path()));
        std::fs::write(&file, r#"{"bibtex": "/x/y/papers.bib", "filesdir": "/x/y/files"}"#).unwrap();
        let config = Config::load(&file, MachineConfig::empty(), Some(false)).unwrap();
        assert!(!config.is_library);
        assert_eq!(config.get("paths"), json!("absolute"));
        assert_eq!(config.bibtex.as_deref(), Some("/x/y/papers.bib"));
        assert_eq!(config.tojson()["local"], json!(false));
    }

    #[test]
    fn a_pool_makes_a_shallow_library() {
        let dir = tempfile::tempdir().unwrap();
        let file = library(dir.path(), r#"{"bibtex": "lib.bib", "pool": "../pool", "version": 3}"#);
        let config = Config::load(&file, MachineConfig::empty(), None).unwrap();
        assert!(config.shallow());
        assert_eq!(config.pool_label().as_deref(), Some("../pool"));
        assert!(config.unused_while_shallow("filesdir") && config.unused_while_shallow("name.template"));
        // a library that is its own pool has none
        let own = library(dir.path(), r#"{"bibtex": "lib.bib", "pool": ".", "version": 3}"#);
        let config = Config::load(&own, MachineConfig::empty(), None).unwrap();
        assert!(config.is_pool_library() && !config.has_pool());
    }

    #[test]
    fn files_of_earlier_v3_builds_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut machine = machine(json!({"default": "/the/default"}));
        machine.pool_names.insert("shared".into(), json!("/shared/pool"));
        let pool_of = |js: &str| {
            let file = library(dir.path(), js);
            let config = Config::load(&file, machine.clone(), None).unwrap();
            config.explicit.contains("pool").then(|| config.pool.clone())
        };
        assert_eq!(pool_of(r#"{"version": 3, "pools": ["default"], "light": true}"#).as_deref(), Some("/the/default"));
        assert_eq!(pool_of(r#"{"version": 3, "pools": ["../x", "default"], "sync": "push"}"#).as_deref(), Some("../x"));
        assert_eq!(pool_of(r#"{"version": 3, "pools": []}"#).as_deref(), Some("none"));
        assert_eq!(pool_of(r#"{"version": 3, "pools": ["shared"]}"#).as_deref(), Some("/shared/pool"));
        assert_eq!(pool_of(r#"{"version": 3, "pools": ["unknown"]}"#), None);
        assert_eq!(pool_of(r#"{"version": 3, "pool": ["a"]}"#).as_deref(), Some("a"));
        let file = library(dir.path(), r#"{"version": 3, "pool": ["a", "b"]}"#);
        let error = Config::load(&file, machine.clone(), None).unwrap_err();
        assert!(error.message().contains("the pool setting lists several pools (a, b)"), "{error}");
    }

    #[test]
    fn newer_and_invalid_versions_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = library(dir.path(), r#"{"version": 4}"#);
        let error = Config::load(&file, MachineConfig::empty(), None).unwrap_err();
        assert!(error.message().contains("was written by a newer papers (configuration version 4;"), "{error}");
        let file = library(dir.path(), r#"{"version": true}"#);
        let error = Config::load(&file, MachineConfig::empty(), None).unwrap_err();
        assert!(error.message().ends_with("invalid configuration version True"), "{error}");
    }

    #[test]
    fn only_ids_that_papers_writes_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = library(dir.path(), r#"{"id": "0123456789abcdef0123456789abcdef"}"#);
        assert!(Config::load(&file, MachineConfig::empty(), None).unwrap().id.is_some());
        let file = library(dir.path(), r#"{"id": "NOTANID"}"#);
        assert!(Config::load(&file, MachineConfig::empty(), None).unwrap().id.is_none());
    }

    fn read_back(file: &str) -> Value {
        crate::config::json::loads(&std::fs::read_to_string(file).unwrap()).unwrap()
    }

    #[test]
    fn a_setting_given_is_saved_even_with_the_default_value() {
        let dir = tempfile::tempdir().unwrap();
        let file = library(dir.path(), r#"{"bibtex": "lib.bib", "files": "move", "history-size": 500, "version": 3}"#);
        let mut config = Config::load(&file, machine(json!({"editor": "vim"})), None).unwrap();
        config.set("editor", json!("vim")).unwrap();
        config.set("name.author-num", json!(2)).unwrap();
        config.save().unwrap();
        let saved = read_back(&file);
        assert_eq!(saved["files"], json!("move"));
        assert_eq!(saved["history-size"], json!(500));
        assert_eq!(saved["editor"], json!("vim"));
        assert_eq!(saved["nameformat"], json!({"author_num": 2}));
        assert_eq!(config.source("files"), "library");
        // unset: inherited again (not written); the paths of a library are written as null
        config.unset("files").unwrap();
        config.unset("name.author-num").unwrap();
        config.unset("filesdir").unwrap();
        config.save().unwrap();
        let saved = read_back(&file);
        assert!(saved.get("files").is_none() && saved.get("nameformat").is_none());
        assert_eq!(saved["filesdir"], Value::Null);
        assert_eq!(config.source("files"), "default");
        assert_eq!(saved["history-size"], json!(500));
    }

    #[test]
    fn keys_that_papers_does_not_know_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = library(
            dir.path(),
            r#"{"bibtex": "lib.bib", "version": 3, "editor": "vim", "pools": ["old"], "future": {"a": [1]},
                "nameformat": {"template": "{author}", "future-field": 2}, "gitdir": "x", "light": true}"#,
        );
        let mut config = Config::load(&file, MachineConfig::empty(), None).unwrap();
        config.unset("editor").unwrap();
        config.set("name.template", json!("{year}")).unwrap();
        config.save().unwrap();
        let saved = read_back(&file);
        assert_eq!(saved["future"], json!({"a": [1]}));
        assert_eq!(saved["nameformat"], json!({"template": "{year}", "future-field": 2}));
        // the known keys are those of papers: unset, or read and no longer written
        for key in ["editor", "pools", "gitdir", "light"] {
            assert!(saved.get(key).is_none(), "{key}");
        }
        // with no setting of the group left, the unknown member keeps it
        config.unset("name.template").unwrap();
        config.save().unwrap();
        assert_eq!(read_back(&file)["nameformat"], json!({"future-field": 2}));
    }

    #[test]
    fn a_value_of_the_wrong_type_is_an_error_that_names_the_file_and_the_setting() {
        let dir = tempfile::tempdir().unwrap();
        for (js, name) in [
            (json!({"auto-push": "no"}), "auto-push"),
            (json!({"pool": 3}), "pool"),
            (json!({"bibtex": ["a.bib"]}), "bibtex"),
            (json!({"paths": "relativ"}), "paths"),
            (json!({"keyformat": {"author_num": 2.5}}), "key.author-num"),
            (json!({"history-size": true}), "history-size"),
        ] {
            let file = library(dir.path(), &js.to_string());
            let error = Config::load(&file, MachineConfig::empty(), None).unwrap_err();
            assert!(
                error.message().starts_with(&format!("{file}: invalid value ")) && error.message().contains(&format!(" for the setting {name} (expected ")),
                "{error}"
            );
            assert!(matches!(error, Error::Exit(_)));
        }
        let file = library(dir.path(), r#"{"version": 3, "nameformat": "x"}"#);
        let error = Config::load(&file, MachineConfig::empty(), None).unwrap_err();
        assert_eq!(error.message(), format!("{file}: invalid nameformat 'x' (expected an object): correct the file"));
        // null is unset
        let file = library(dir.path(), r#"{"version": 3, "auto-push": null, "nameformat": null}"#);
        assert!(Config::load(&file, MachineConfig::empty(), None).is_ok());
        // a file that is not JSON, or not an object
        for text in ["{bad", "[]", ""] {
            let file = library(dir.path(), text);
            let error = Config::load(&file, MachineConfig::empty(), None).unwrap_err();
            assert!(error.message().starts_with(&file) && error.message().ends_with("correct the file"), "{error}");
        }
    }

    #[test]
    fn a_library_configuration_is_read_as_utf8_and_a_byte_order_mark_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let file = library(dir.path(), "\u{feff}{\"version\":3,\"bibtex\":\"r\u{e9}f.bib\",\"editor\":\"\u{e9}\",\"filesdir\":null}");
        let config = Config::load(&file, MachineConfig::empty(), None).unwrap();
        assert_eq!(config.get("editor"), json!("\u{e9}"));
        assert_eq!(config.bibtex.as_deref(), Some(format!("{}/r\u{e9}f.bib", config_root(&file)).as_str()));
    }

    #[test]
    fn a_configuration_that_is_not_utf8_is_an_error_that_names_the_file() {
        // library.md, Reading 1 (test_spec_library.py::TestReading::test_invalid_utf8), with what Python says of the bytes
        let dir = tempfile::tempdir().unwrap();
        for (bytes, why) in [
            (&b"{\"version\": 3, \"editor\": \"\xff\"}"[..], "byte 0xff in position 26: invalid start byte"),
            (b"\xef\xbb\xbf{\"version\": 3, \"editor\": \"\xff\"}", "byte 0xff in position 26: invalid start byte"),
            (b"{\"version\": 3, \"editor\": \"\xc3(\"}", "byte 0xc3 in position 26: invalid continuation byte"),
            (b"{\"version\": 3, \"editor\": \"\xe2\x82(\"}", "bytes in position 26-27: invalid continuation byte"),
            (b"{\"version\": 3, \"editor\": \"\xed\xa0\x80\"}", "byte 0xed in position 26: invalid continuation byte"),
            (b"{\"version\": 3, \"editor\": \"\xf0\x9f", "bytes in position 26-27: unexpected end of data"),
            (b"{\"version\": 3, \"editor\": \"\xe2", "byte 0xe2 in position 26: unexpected end of data"),
        ] {
            let file = library(dir.path(), "");
            std::fs::write(&file, bytes).unwrap();
            let error = Config::load(&file, MachineConfig::empty(), None).unwrap_err();
            assert_eq!(
                error.message(),
                format!("{file} could not be read ('utf-8' codec can't decode {why}): correct the file")
            );
        }
    }

    #[test]
    fn the_home_directory_is_expanded_in_bibtex_and_filesdir() {
        let dir = tempfile::tempdir().unwrap();
        let file = library(dir.path(), r#"{"bibtex": "~/refs.bib", "filesdir": "~/pdfs", "version": 3}"#);
        let config = Config::load(&file, MachineConfig::empty(), None).unwrap();
        let home = crate::config::paths::home();
        assert_eq!(config.bibtex.as_deref(), Some(format!("{home}/refs.bib").as_str()));
        assert_eq!(config.filesdir.as_deref(), Some(format!("{home}/pdfs").as_str()));
    }

    #[test]
    fn the_files_of_the_library_are_in_the_folder_of_its_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        let file = library(dir.path(), r#"{"bibtex": "bib/papers.bib", "version": 3}"#);
        let config = Config::load(&file, MachineConfig::empty(), None).unwrap();
        assert_eq!(config.libdir(), root);
        // a bibtex without library: the folder of the bibtex
        assert_eq!(library_dir(Some(&format!("{root}/x/refs.bib")), None), format!("{root}/x"));
        assert_eq!(library_dir(Some("refs.bib"), None), getcwd());
        // a configuration of papers 2: the folder of the file
        assert_eq!(library_dir(None, Some(&format!("{root}/.papersconfig.json"))), root);
        // the global configuration of papers 2 is not a library: the folder of its bibtex
        let mut v2 = Config::new(MachineConfig::empty());
        v2.bibtex = Some(format!("{root}/b/p.bib"));
        v2.file = Some(format!("{root}/papersconfig.json"));
        assert_eq!(v2.libdir(), format!("{root}/b"));
    }

    #[test]
    fn the_files_that_earlier_builds_wrote_next_to_the_bibtex_move_to_the_library_folder() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        let write = |path: &str, text: &str| {
            std::fs::create_dir_all(dirname(&format!("{root}/{path}"))).unwrap();
            std::fs::write(format!("{root}/{path}"), text).unwrap();
        };
        // lib/ has its bibtex in lib/bib/
        write("lib/.papers/config.json", r#"{"bibtex": "bib/papers.bib", "version": 3}"#);
        write("lib/bib/.papers/distinct.json", "{\"distinct\": []}\n");
        write("lib/bib/.papers/origin/pool-123456.bib", "");
        write("lib/bib/.papers/origin.bib", "");
        write("lib/bib/.papers/files.json", "{}");
        Config::load(&format!("{root}/lib/.papers/config.json"), MachineConfig::empty(), None).unwrap();
        for name in ["distinct.json", "origin/pool-123456.bib", "origin.bib"] {
            assert!(!std::path::Path::new(&format!("{root}/lib/bib/.papers/{name}")).exists(), "{name}");
            assert!(std::path::Path::new(&format!("{root}/lib/.papers/{name}")).exists(), "{name}");
        }
        // the others stay
        assert!(std::path::Path::new(&format!("{root}/lib/bib/.papers/files.json")).exists());
        // not over a file that the library folder has
        write("b/.papers/config.json", r#"{"bibtex": "s/papers.bib", "version": 3}"#);
        write("b/.papers/distinct.json", "mine");
        write("b/s/.papers/distinct.json", "old");
        Config::load(&format!("{root}/b/.papers/config.json"), MachineConfig::empty(), None).unwrap();
        assert_eq!(std::fs::read_to_string(format!("{root}/b/.papers/distinct.json")).unwrap(), "mine");
        assert_eq!(std::fs::read_to_string(format!("{root}/b/s/.papers/distinct.json")).unwrap(), "old");
        // not from the folder of another library
        write("c/.papers/config.json", r#"{"bibtex": "../o/papers.bib", "version": 3}"#);
        write("o/.papers/config.json", r#"{"bibtex": "papers.bib", "version": 3}"#);
        write("o/.papers/distinct.json", "o");
        Config::load(&format!("{root}/c/.papers/config.json"), MachineConfig::empty(), None).unwrap();
        assert!(std::path::Path::new(&format!("{root}/o/.papers/distinct.json")).exists());
        assert!(!std::path::Path::new(&format!("{root}/c/.papers/distinct.json")).exists());
    }

    #[test]
    fn the_lines_of_papers_config() {
        let config = Config::new(MachineConfig::empty());
        let lines = config.settings_lines(None);
        assert_eq!(lines.len(), 30);
        assert_eq!(lines[0], "bibtex                =   (default)");
        assert_eq!(lines[3], "paths                 = relative  (default)");
        assert_eq!(lines[13], "name.author-num       = 2  (default)");
    }
}
