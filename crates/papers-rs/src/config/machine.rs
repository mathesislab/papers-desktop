//! The global configuration: the default library and the default settings of
//! the libraries (`~/Library/Application Support/papers/config.json` on
//! macOS, `~/.config/papers/config.json` on Linux).

use std::fs;

use serde_json::{Map, Value, json};

use super::dirs::machine_config_file;
use super::files::write_atomic;
use super::json::{dumps_pretty, loads, py_repr};
use super::paths::{abspath, dirname, exists, expanduser, getcwd, join, normpath};
use super::settings::{CONFIG_VERSION, DEFAULT_LIBRARY, NO_POOL, check_settings, json_get, json_set, names};
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

/// Read a configuration file (`read_config_file`): its JSON object, read as UTF-8 (a byte order
/// mark is ignored). A file that is not valid UTF-8, not valid JSON, or not a JSON object, is an
/// error that names the file, so that no later save replaces its content.
pub fn read_json_object(file: &str) -> Result<Map<String, Value>> {
    let unreadable = |why: &str| Error::exit(format!("{file} could not be read ({why}): correct the file"));
    let bytes = fs::read(file).map_err(|e| unreadable(&os_error_text(&e, file)))?;
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    let text = std::str::from_utf8(bytes).map_err(|e| unreadable(&utf8_error_text(bytes, e)))?;
    match loads(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(Error::exit(format!("{file} is not a JSON object: correct the file"))),
        Err(error) => Err(unreadable(&error.message)),
    }
}

/// What Python's `str(UnicodeDecodeError)` says of the `bytes` that `error` found not to be UTF-8.
fn utf8_error_text(bytes: &[u8], error: std::str::Utf8Error) -> String {
    let start = error.valid_up_to();
    let (end, why) = match error.error_len() {
        None => (bytes.len(), "unexpected end of data"),
        Some(len) => {
            // a byte that starts no sequence, else the sequence it starts is broken
            let first = bytes[start];
            let invalid_start = matches!(first, 0x80..=0xC1 | 0xF5..=0xFF);
            (start + len, if invalid_start { "invalid start byte" } else { "invalid continuation byte" })
        }
    };
    let at = if end == start + 1 {
        format!("byte 0x{:02x} in position {start}", bytes[start])
    } else {
        format!("bytes in position {start}-{}", end - 1)
    };
    format!("'utf-8' codec can't decode {at}: {why}")
}

/// What Python's `str(OSError)` says of an error on `path`.
fn os_error_text(error: &std::io::Error, path: &str) -> String {
    let text = error.to_string();
    let text = text.split(" (os error").next().unwrap_or(&text);
    match error.raw_os_error() {
        Some(code) => format!("[Errno {code}] {text}: '{path}'"),
        None => text.to_string(),
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
    /// Pool names of earlier v3 builds (`pools.NAME PATH`): not a setting, but written back by
    /// `save`, so that their libraries keep finding their pool.
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

    /// Load the global configuration (`file`, by default the one of the machine). A file that is
    /// not valid JSON, or has a value of the wrong type, is an error.
    pub fn load(file: Option<&str>) -> Result<MachineConfig> {
        let file = file.map(str::to_string).unwrap_or_else(machine_config_file);
        let mut data = if exists(&file) { read_json_object(&file)? } else { Map::new() };
        file_version(&mut data, &file)?;
        data.remove("version");
        match data.remove("pool") {
            None | Some(Value::Null) => {}
            Some(Value::String(s)) if s == NO_POOL => {}
            Some(_) => log::warning(&format!(
                "{file}: the global pool setting is no longer used (ignored); `papers config --global default PATH` sets a default library"
            )),
        }
        let mut checked: Vec<&str> = names();
        checked.push(DEFAULT_LIBRARY);
        check_settings(&data, &file, &checked)?;
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

    /// The absolute path of the default library, or `None` (a relative path is relative to the
    /// folder of this file).
    pub fn default_library_path(&self) -> Option<String> {
        self.default_library().map(|d| self.absolute(&d))
    }

    /// A path of this configuration (`default`, `history-dir`): `~` expanded, a relative path
    /// relative to the folder of the file.
    pub fn absolute(&self, path: &str) -> String {
        let base = match &self.file {
            Some(file) => dirname(&abspath(file)),
            None => getcwd(),
        };
        normpath(&join(&base, &expanduser(path)))
    }

    /// Write the file, atomically (through a temporary file): a crash never leaves half a
    /// configuration. The pool names of earlier builds are written back.
    pub fn save(&self) -> Result<()> {
        let file = self.file.as_deref().ok_or_else(|| Error::other("the global configuration has no file"))?;
        let dir = dirname(file);
        if !dir.is_empty() {
            fs::create_dir_all(&dir).map_err(|e| Error::other(format!("{dir}: {}", io_message(&e))))?;
        }
        let mut data = self.data.clone();
        data.insert("version".into(), json!(CONFIG_VERSION));
        if !self.pool_names.is_empty() {
            data.insert("pools".into(), Value::Object(self.pool_names.clone()));
        }
        write_atomic(file, &dumps_pretty(&Value::Object(data)))
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
    fn a_global_configuration_that_cannot_be_read_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = format!("{}/config.json", dir.path().display());
        fs::write(&file, "{bad").unwrap();
        let error = MachineConfig::load(Some(&file)).unwrap_err();
        assert_eq!(
            error,
            Error::exit(format!(
                "{file} could not be read (Expecting property name enclosed in double quotes: line 1 column 2 (char 1)): correct the file"
            ))
        );
        fs::write(&file, "[1]").unwrap();
        assert_eq!(
            MachineConfig::load(Some(&file)).unwrap_err(),
            Error::exit(format!("{file} is not a JSON object: correct the file"))
        );
        fs::write(&file, r#"{"editor": 3}"#).unwrap();
        assert!(
            MachineConfig::load(Some(&file))
                .unwrap_err()
                .message()
                .contains("invalid value 3 for the setting editor (expected a text)")
        );
        // a byte order mark is ignored
        fs::write(&file, "\u{feff}{\"editor\": \"vim\"}").unwrap();
        assert_eq!(MachineConfig::load(Some(&file)).unwrap().get("editor"), Some(json!("vim")));
    }

    #[test]
    fn a_relative_default_is_relative_to_the_configuration() {
        let machine = MachineConfig::new(Some("/a/b/config.json"), json!({"default": "../lib"}).as_object().unwrap().clone());
        assert_eq!(machine.default_library_path().as_deref(), Some("/a/lib"));
        assert_eq!(machine.absolute("~/x"), format!("{}/x", crate::config::paths::home()));
    }

    #[test]
    fn pool_names_of_earlier_builds_are_read_and_written_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = format!("{}/config.json", dir.path().display());
        fs::write(&file, r#"{"version": 3, "pools": {"a": "/p"}, "pool": "/old", "default": "~/x"}"#).unwrap();
        let machine = MachineConfig::load(Some(&file)).unwrap();
        assert_eq!(machine.pool_names.get("a"), Some(&json!("/p")));
        assert!(!machine.data.contains_key("pool") && !machine.data.contains_key("pools"));
        machine.save().unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "{\n  \"default\": \"~/x\",\n  \"pools\": {\n    \"a\": \"/p\"\n  },\n  \"version\": 3\n}"
        );
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
