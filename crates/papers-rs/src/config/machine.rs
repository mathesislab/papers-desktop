//! The global configuration: the default library and the default settings of
//! the libraries (`~/Library/Application Support/papers/config.json` on
//! macOS, `~/.config/papers/config.json` on Linux).

use std::fs;

use serde_json::{Map, Value, json};

use super::dirs::machine_config_file;
use super::json::{dumps_pretty, loads, py_repr};
use super::paths::{abspath, dirname, exists, expanduser};
use super::settings::{CONFIG_VERSION, DEFAULT_LIBRARY, NO_POOL, json_get, json_set};
use crate::error::{Error, PAPERS_VERSION, Result};
use crate::log;

/// The format version of a configuration file (2 without version); error if
/// it is too new. The settings of the v2 format are dropped from files of
/// version 3 and later.
pub fn file_version(js: &mut Map<String, Value>, file: &str) -> Result<i64> {
    let version = match js.get("version") {
        None => 2,
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => n.as_i64().unwrap_or(i64::MAX),
        Some(other) => {
            return Err(Error::exit(format!("{file}: invalid configuration version {}", py_repr(other))));
        }
    };
    if version > CONFIG_VERSION {
        return Err(Error::exit(format!(
            "{file} was written by a newer papers (configuration version {version}; papers {PAPERS_VERSION} reads up to version {CONFIG_VERSION}): upgrade papers"
        )));
    }
    if version >= 3 {
        for key in ["absolute_paths", "backup_files"] {
            js.remove(key);
        }
    }
    Ok(version)
}

/// Read a configuration file: its JSON object. Anything else is an error
/// papers does not catch.
pub fn read_json_object(file: &str) -> Result<Map<String, Value>> {
    let text = fs::read_to_string(file).map_err(|e| Error::other(format!("{file}: {}", io_message(&e))))?;
    match loads(&text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(Error::other(format!("{file}: not a JSON object"))),
        Err(error) => Err(Error::other(format!("{file}: {error}"))),
    }
}

pub(crate) fn io_message(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "No such file or directory".to_string(),
        std::io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
        _ => error.to_string(),
    }
}

/// A pool setting written as a list (by papers builds that took several
/// pools): one path is that path; several paths are an error.
pub(crate) fn single_pool(js: &mut Map<String, Value>, file: &str) -> Result<()> {
    let Some(Value::Array(items)) = js.get("pool") else { return Ok(()) };
    let paths: Vec<&str> = items.iter().filter_map(|p| p.as_str()).filter(|p| !p.is_empty()).collect();
    if paths.len() > 1 {
        return Err(Error::exit(format!(
            "{file}: the pool setting lists several pools ({}); papers takes one pool: write one path, or {NO_POOL}, in this file",
            paths.join(", ")
        )));
    }
    let single = paths.first().map(|p| p.to_string()).unwrap_or_else(|| NO_POOL.to_string());
    js.insert("pool".into(), Value::String(single));
    Ok(())
}

/// The global configuration: the default library and the defaults of the
/// library settings. The file has the layout of a library configuration:
///
/// ```json
/// {"default": "~/Documents/papers", "editor": "vim",
///  "nameformat": {"template": "{author}_{year}"}}
/// ```
///
/// `default` (optional) is the default library: a library without pool, used
/// outside of any library, and the pool of `papers init --shallow`.
#[derive(Debug, Clone)]
pub struct MachineConfig {
    pub file: Option<String>,
    pub data: Map<String, Value>,
    /// Pool names of earlier v3 builds (`pools.NAME PATH`): only read, so
    /// that their libraries keep finding their pool.
    pub pool_names: Map<String, Value>,
}

impl MachineConfig {
    pub fn new(file: Option<&str>, mut data: Map<String, Value>) -> MachineConfig {
        let pool_names = match data.remove("pools") {
            Some(Value::Object(names)) => names,
            _ => Map::new(),
        };
        MachineConfig { file: file.map(str::to_string), data, pool_names }
    }

    /// An empty configuration that is not backed by a file.
    pub fn empty() -> MachineConfig {
        MachineConfig { file: None, data: Map::new(), pool_names: Map::new() }
    }

    /// Load the global configuration (`file`, by default the one of the
    /// machine). A file that cannot be read is ignored, with a warning.
    pub fn load(file: Option<&str>) -> Result<MachineConfig> {
        let file = file.map(str::to_string).unwrap_or_else(machine_config_file);
        let mut data = Map::new();
        if exists(&file) {
            let read = fs::read_to_string(&file)
                .map_err(|e| io_message(&e))
                .and_then(|text| loads(&text).map_err(|e| e.message));
            match read {
                Ok(Value::Object(map)) => data = map,
                Ok(_) => return Err(Error::other(format!("{file}: not a JSON object"))),
                Err(message) => {
                    log::warning(&format!("unreadable global configuration {file}: {message} -- ignored"));
                }
            }
        }
        file_version(&mut data, &file)?;
        data.remove("version");
        match data.remove("pool") {
            None | Some(Value::Null) => {}
            Some(Value::String(s)) if s == NO_POOL => {}
            Some(_) => log::warning(&format!(
                "{file}: the global pool setting is no longer used (ignored); `papers config --global default PATH` sets a default library"
            )),
        }
        Ok(MachineConfig::new(Some(&file), data))
    }

    pub fn exists(&self) -> bool {
        self.file.as_deref().is_some_and(exists)
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        json_get(&self.data, name)
    }

    pub fn set(&mut self, name: &str, value: Option<Value>) {
        json_set(&mut self.data, name, value);
    }

    /// The default library as written (with `~`), or `None`.
    pub fn default_library(&self) -> Option<String> {
        match self.data.get(DEFAULT_LIBRARY) {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        }
    }

    /// The absolute path of the default library, or `None`.
    pub fn default_library_path(&self) -> Option<String> {
        self.default_library().map(|d| abspath(&expanduser(&d)))
    }

    pub fn save(&self) -> Result<()> {
        let file = self.file.as_deref().ok_or_else(|| Error::other("the global configuration has no file"))?;
        let dir = dirname(file);
        if !dir.is_empty() {
            fs::create_dir_all(&dir).map_err(|e| Error::other(format!("{dir}: {}", io_message(&e))))?;
        }
        let mut data = self.data.clone();
        data.insert("version".into(), json!(CONFIG_VERSION));
        fs::write(file, dumps_pretty(&Value::Object(data)))
            .map_err(|e| Error::other(format!("{file}: {}", io_message(&e))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_global_configuration_is_saved_as_papers_saves_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = format!("{}/papers/config.json", dir.path().display());
        let mut machine = MachineConfig::load(Some(&file)).unwrap();
        assert!(!machine.exists());
        machine.set("editor", Some(json!("vim")));
        machine.set("default", Some(json!("~/papers")));
        machine.set("name.author-num", Some(json!(1)));
        machine.save().unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "{\n  \"default\": \"~/papers\",\n  \"editor\": \"vim\",\n  \"nameformat\": {\n    \"author_num\": 1\n  },\n  \"version\": 3\n}"
        );
        // the version is not a setting
        let loaded = MachineConfig::load(Some(&file)).unwrap();
        assert!(loaded.exists() && !loaded.data.contains_key("version"));
        assert_eq!(loaded.default_library().as_deref(), Some("~/papers"));
        assert_eq!(loaded.get("name.author-num"), Some(json!(1)));
    }

    #[test]
    fn a_global_configuration_that_cannot_be_read_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let file = format!("{}/config.json", dir.path().display());
        fs::write(&file, "{bad").unwrap();
        let machine = MachineConfig::load(Some(&file)).unwrap();
        assert!(machine.data.is_empty());
    }

    #[test]
    fn pool_names_of_earlier_builds_are_read_and_dropped_when_saved() {
        let dir = tempfile::tempdir().unwrap();
        let file = format!("{}/config.json", dir.path().display());
        fs::write(&file, r#"{"version": 3, "pools": {"a": "/p"}, "pool": "/old", "default": "~/x"}"#).unwrap();
        let machine = MachineConfig::load(Some(&file)).unwrap();
        assert_eq!(machine.pool_names.get("a"), Some(&json!("/p")));
        assert!(!machine.data.contains_key("pool") && !machine.data.contains_key("pools"));
        machine.save().unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "{\n  \"default\": \"~/x\",\n  \"version\": 3\n}");
    }

    #[test]
    fn a_global_configuration_of_papers_2_has_no_version() {
        let mut js = json!({"absolute_paths": true, "bibtex": "x"}).as_object().unwrap().clone();
        assert_eq!(file_version(&mut js, "f").unwrap(), 2);
        assert!(js.contains_key("absolute_paths"));
        let mut js = json!({"version": 3, "absolute_paths": true}).as_object().unwrap().clone();
        assert_eq!(file_version(&mut js, "f").unwrap(), 3);
        assert!(!js.contains_key("absolute_paths"));
    }
}
