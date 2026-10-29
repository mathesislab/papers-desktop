//! The settings of `papers config`: their names, types, built-in defaults,
//! and how they are written in the JSON of a configuration file.

use std::sync::LazyLock;

use serde_json::{Map, Value, json};

use super::json::{dumps_compact, py_repr, py_repr_str, py_str, py_truthy};
use crate::error::Error;

/// The version of the configuration files that papers writes; a file without
/// version was written by papers v2.
pub const CONFIG_VERSION: i64 = 3;
/// The value of the pool setting for no pool.
pub const NO_POOL: &str = "none";
/// The setting of the global configuration that names the default library.
pub const DEFAULT_LIBRARY: &str = "default";
/// The configuration file of a library, relative to the library directory.
pub const CONFIG_FILE_LOCAL: &str = ".papers/config.json";
/// Its name in papers 2 (read, and moved to `CONFIG_FILE_LOCAL` when the
/// configuration is saved).
pub const CONFIG_FILE_LOCAL_V2: &str = ".papersconfig.json";
pub const LIBRARY_CONFIG_FILES: [&str; 2] = [CONFIG_FILE_LOCAL, CONFIG_FILE_LOCAL_V2];

/// The type of a setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Path,
    Pool,
    Bool,
    Int,
    Str,
    Choice(&'static [&'static str]),
}

const FORMAT_FIELDS: [&str; 9] = [
    "template",
    "author-num",
    "author-sep",
    "title-word-num",
    "title-word-size",
    "title-length",
    "title-sep",
    "unknown-template",
    "unknown-strict",
];
const INT_FIELDS: [&str; 4] = ["author-num", "title-word-num", "title-word-size", "title-length"];

/// The settings, in the order of `papers config`.
pub static SETTINGS: LazyLock<Vec<(String, Kind)>> = LazyLock::new(|| {
    let mut settings: Vec<(String, Kind)> = vec![
        ("bibtex".into(), Kind::Path),
        ("filesdir".into(), Kind::Path),
        ("pool".into(), Kind::Pool),
        ("paths".into(), Kind::Choice(&["relative", "absolute"])),
        ("files".into(), Kind::Choice(&["ask", "move", "copy", "link"])),
        ("auto-push".into(), Kind::Bool),
        ("editor".into(), Kind::Str),
        ("checksums".into(), Kind::Bool),
        ("history-dir".into(), Kind::Str),
        ("history-size".into(), Kind::Int),
        ("history-separate".into(), Kind::Bool),
        ("keep-deleted-files".into(), Kind::Bool),
    ];
    for kind in ["name", "key"] {
        for field in FORMAT_FIELDS {
            let setting_kind = if INT_FIELDS.contains(&field) {
                Kind::Int
            } else if field == "unknown-strict" {
                Kind::Bool
            } else {
                Kind::Str
            };
            settings.push((format!("{kind}.{field}"), setting_kind));
        }
    }
    settings
});

/// The names of the settings, in order.
pub fn names() -> Vec<&'static str> {
    SETTINGS.iter().map(|(name, _)| name.as_str()).collect()
}

pub fn kind(name: &str) -> Option<Kind> {
    SETTINGS.iter().find(|(n, _)| n == name).map(|(_, kind)| *kind)
}

/// The settings that only make sense for a library.
pub const LIBRARY_ONLY: [&str; 2] = ["bibtex", "filesdir"];
/// The settings of the machine only: a library does not set them.
pub const MACHINE_ONLY: [&str; 2] = ["history-dir", "keep-deleted-files"];

/// The settings of a library other than its paths: when the library does not
/// set one, the global configuration applies, except for pool.
pub static LIBRARY_SETTINGS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    names().into_iter().filter(|n| !LIBRARY_ONLY.contains(n) && !MACHINE_ONLY.contains(n)).collect()
});

/// Settings of papers before the history: ignored, with their reason.
pub fn removed_setting(name: &str) -> Option<&'static str> {
    match name {
        "git" => Some("papers keeps the history of the libraries without git (see `papers history`)"),
        "git-lfs" => Some("papers keeps the files that its commands delete in its history, without git-lfs (see `papers history`)"),
        "trash-days" | "trash-size" => {
            Some("the history-size setting (global) limits the history, with the files it keeps")
        }
        _ => None,
    }
}

/// The keys of a library configuration file that papers reads, or read in earlier versions; the
/// file keeps the others as they are (the settings of a newer implementation: see `Config::tojson`).
/// The names of the settings are known too (see `is_known_key`).
const KNOWN_KEYS: [&str; 14] = [
    "git", "git-lfs", "trash-days", "trash-size", "absolute_paths", "backup_files", "id", "version", "local", "pools",
    "light", "sync", "gitdir", "gitlfs",
];

/// The key of a configuration file is one that papers reads or wrote in earlier versions.
pub fn is_known_key(key: &str) -> bool {
    KNOWN_KEYS.contains(&key) || kind(key).is_some()
}

/// Raise an error if a setting of the JSON of a configuration file has a value of the wrong type
/// (`null` is unset): `names` are the settings to check.
pub fn check_settings(js: &Map<String, Value>, file: &str, names: &[&str]) -> crate::error::Result<()> {
    for group in ["nameformat", "keyformat"] {
        if let Some(value) = js.get(group).filter(|v| !v.is_null())
            && !value.is_object()
        {
            return Err(Error::exit(format!(
                "{file}: invalid {group} {} (expected an object): correct the file",
                py_repr(value)
            )));
        }
    }
    for name in names {
        let Some(value) = json_get(js, name) else { continue };
        let (ok, expected) = match kind(name).unwrap_or(Kind::Str) {
            Kind::Choice(choices) => (
                value.as_str().is_some_and(|v| choices.contains(&v)),
                format!("one of {}", choices.join(", ")),
            ),
            Kind::Bool => (value.is_boolean(), "true or false".to_string()),
            Kind::Int => (value.is_i64() || value.is_u64(), "an integer".to_string()),
            Kind::Path | Kind::Pool | Kind::Str => (value.is_string(), "a text".to_string()),
        };
        if !ok {
            return Err(Error::exit(format!(
                "{file}: invalid value {} for the setting {name} (expected {expected}): correct the file",
                dumps_compact(&value)
            )));
        }
    }
    Ok(())
}

/// The built-in default of a setting (`Null`: none).
pub fn default_value(name: &str) -> Value {
    match name {
        "name.template" => json!("{authorX}_{year}_{title}"),
        "key.template" => json!("{author}{year}"),
        "name.unknown-template" => json!("unknown_{doi_or_id}"),
        "key.unknown-template" => json!("{doi_}"),
        "name.author-sep" | "key.author-sep" => json!("_"),
        "name.title-sep" | "key.title-sep" => json!("-"),
        "name.author-num" | "key.author-num" => json!(2),
        "name.title-word-num" | "key.title-word-num" => json!(100),
        "name.title-word-size" | "key.title-word-size" => json!(1),
        "name.title-length" | "key.title-length" => json!(100),
        "name.unknown-strict" | "key.unknown-strict" => json!(false),
        "paths" => json!("relative"),
        "files" => json!("move"),
        "auto-push" => json!(false),
        "history-size" => json!(500),
        "history-separate" => json!(false),
        "keep-deleted-files" => json!(true),
        "checksums" => json!(true),
        _ => Value::Null,
    }
}

/// The key of the group (`nameformat` or `keyformat`) of a `name.*` or
/// `key.*` setting, and its key in it (`author-num` is `author_num`).
fn group_and_attr(name: &str) -> Option<(&'static str, String)> {
    let (kind, field) = name.split_once('.')?;
    match kind {
        "name" => Some(("nameformat", field.replace('-', "_"))),
        "key" => Some(("keyformat", field.replace('-', "_"))),
        _ => None,
    }
}

/// The value of a setting in the JSON of a configuration file, or `None`
/// (`null` also means unset).
pub fn json_get(js: &Map<String, Value>, name: &str) -> Option<Value> {
    let found = if let Some((group, attr)) = group_and_attr(name) {
        js.get(group).and_then(|g| g.as_object()).and_then(|g| g.get(&attr)).cloned()
    } else if name == "paths" {
        match js.get("paths") {
            Some(v) if !v.is_null() => Some(v.clone()),
            _ => match js.get("absolute_paths") {
                // files of v2
                Some(v) if !v.is_null() => Some(json!(if py_truthy(v) { "absolute" } else { "relative" })),
                _ => None,
            },
        }
    } else {
        js.get(name).cloned()
    };
    found.filter(|v| !v.is_null())
}

/// Set (with `None`, remove) a setting in the JSON of a configuration file.
pub fn json_set(js: &mut Map<String, Value>, name: &str, value: Option<Value>) {
    if let Some((group, attr)) = group_and_attr(name) {
        let entry = js.entry(group.to_string()).or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        let map = entry.as_object_mut().expect("an object");
        match value {
            None => {
                map.remove(&attr);
            }
            Some(v) => {
                map.insert(attr, v);
            }
        }
        if map.is_empty() {
            js.remove(group);
        }
        return;
    }
    if name == "paths" {
        js.remove("absolute_paths");
    }
    match value {
        None => {
            js.remove(name);
        }
        Some(v) => {
            js.insert(name.to_string(), v);
        }
    }
}

/// Python's `int(text)`.
pub fn py_int(text: &str) -> Option<i64> {
    let t = text.trim();
    let (negative, digits) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') || digits.contains("__") {
        return None;
    }
    if !digits.chars().all(|c| c.is_ascii_digit() || c == '_') {
        return None;
    }
    let clean: String = digits.chars().filter(|c| *c != '_').collect();
    let n: i64 = clean.parse().ok()?;
    Some(if negative { -n } else { n })
}

/// Check and convert the command-line value(s) of a setting.
pub fn parse_value(name: &str, values: &[String]) -> Result<Value, String> {
    let kind = kind(name).expect("a known setting");
    if values.len() != 1 {
        return Err(format!("{name} takes one value, got {}", values.len()));
    }
    let value = &values[0];
    match kind {
        Kind::Bool => match value.to_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Ok(Value::Bool(true)),
            "false" | "no" | "off" | "0" => Ok(Value::Bool(false)),
            _ => Err(format!("{name}: expected true or false, got {}", py_repr_str(value))),
        },
        Kind::Int => py_int(value)
            .map(|n| json!(n))
            .ok_or_else(|| format!("{name}: expected an integer, got {}", py_repr_str(value))),
        Kind::Choice(choices) if !choices.contains(&value.as_str()) => {
            Err(format!("{name}: expected one of {}, got {}", choices.join(", "), py_repr_str(value)))
        }
        _ => Ok(Value::String(value.clone())),
    }
}

/// A setting's value as `papers config` prints it.
pub fn format_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(true) => "true".into(),
        Value::Bool(false) => "false".into(),
        other => py_str(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_in_papers_order() {
        let names = names();
        assert_eq!(names.len(), 30);
        assert_eq!(&names[..4], ["bibtex", "filesdir", "pool", "paths"]);
        assert_eq!(names[12], "name.template");
        assert_eq!(names[29], "key.unknown-strict");
        assert_eq!(LIBRARY_SETTINGS.len(), 26);
    }

    #[test]
    fn the_types_of_the_settings_in_a_file_are_checked() {
        let check = |js: Value| check_settings(js.as_object().unwrap(), "config.json", &names());
        assert!(check(json!({"auto-push": true, "history-size": 3, "nameformat": {"author_num": 2}, "editor": null})).is_ok());
        let message = |js: Value| check(js).unwrap_err().message().to_string();
        assert_eq!(
            message(json!({"auto-push": "no"})),
            "config.json: invalid value \"no\" for the setting auto-push (expected true or false): correct the file"
        );
        assert!(message(json!({"history-size": true})).contains("(expected an integer)"));
        assert!(message(json!({"files": "x"})).contains("(expected one of ask, move, copy, link)"));
        assert!(message(json!({"nameformat": {"author_num": "2"}})).contains("setting name.author-num"));
        assert!(message(json!({"keyformat": [1]})).ends_with("invalid keyformat [1] (expected an object): correct the file"));
        assert!(message(json!({"pool": 3})).contains("invalid value 3 for the setting pool (expected a text)"));
    }

    #[test]
    fn parses_values() {
        let one = |s: &str| vec![s.to_string()];
        assert_eq!(parse_value("auto-push", &one("Yes")), Ok(json!(true)));
        assert_eq!(parse_value("history-size", &one(" 1_2 ")), Ok(json!(12)));
        assert_eq!(
            parse_value("history-size", &one("12x")),
            Err("history-size: expected an integer, got '12x'".to_string())
        );
        assert_eq!(
            parse_value("paths", &one("foo")),
            Err("paths: expected one of relative, absolute, got 'foo'".to_string())
        );
        assert_eq!(parse_value("files", &[]), Err("files takes one value, got 0".to_string()));
    }

    #[test]
    fn json_get_and_set_groups() {
        let mut js = Map::new();
        json_set(&mut js, "name.author-num", Some(json!(3)));
        assert_eq!(js.get("nameformat"), Some(&json!({"author_num": 3})));
        assert_eq!(json_get(&js, "name.author-num"), Some(json!(3)));
        json_set(&mut js, "name.author-num", None);
        assert!(js.get("nameformat").is_none());
        let v2: Map<String, Value> = json!({"absolute_paths": true}).as_object().unwrap().clone();
        assert_eq!(json_get(&v2, "paths"), Some(json!("absolute")));
    }
}
