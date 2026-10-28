//! The cache of the queries (`papers.cache`): one JSON file per key, under the cache
//! directory of papers, in the folder of the query (`doi`, `arxiv`, `crossref-query`). The
//! files are those of papers, which reads them as papers-rs does.
//!
//! Not done: the migration of the single-file caches of the old versions of papers
//! (`crossref.json`, `arxiv.json`); they are ignored.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::http::quote;
use crate::config::json;
use crate::log;

/// How long a DOI that does not exist is remembered (seconds).
pub const NOT_FOUND_TTL: f64 = 24.0 * 3600.0;

/// What the cache has for a key.
pub enum Cached {
    /// the value that was stored
    Value(Value),
    /// the key does not exist, as it was found less than a day ago (`time.time()` then)
    NotFound(f64),
    Nothing,
}

/// `_cache_path`: `<folder>/<prefix>/<rest>.json`, split at the first slash so that the DOIs are
/// grouped by registrant; the characters that are not safe in a file name are quoted.
pub fn cache_path(folder: &str, key: &str) -> String {
    let (head, tail) = match key.split_once('/') {
        Some((h, t)) => (h.to_string(), t.to_string()),
        None => (key.chars().take(2).collect(), key.to_string()),
    };
    let mut name = quote(&tail, "");
    if name.chars().count() > 200 {
        // the file name length limit
        name = hex(&Sha256::digest(tail.as_bytes()));
    }
    format!("{folder}/{}/{name}.json", quote(&head, ""))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The key of a query by its parameters (`hashed_key`): the first 16 characters of the SHA-256.
pub fn hashed_key(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))[..16].to_string()
}

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// `_read_cache_file`
fn read_file(file: &str) -> Option<Value> {
    let text = std::fs::read(file).ok()?;
    match json::loads(&String::from_utf8_lossy(&text)) {
        Ok(v) => Some(v),
        Err(error) => {
            // a corrupt cache file must not break every query
            log::warning(&format!("unreadable cache file {file}: {error} -- ignored"));
            None
        }
    }
}

/// The cache of a folder.
pub struct Cache {
    pub dir: String,
}

impl Cache {
    pub fn new(dir: impl Into<String>) -> Cache {
        Cache { dir: dir.into() }
    }

    fn file(&self, name: &str, key: &str) -> String {
        cache_path(&format!("{}/{name}", self.dir), key)
    }

    /// What the cache has for the key (`now` is the time to compare a not-found with).
    pub fn get(&self, name: &str, key: &str, now: f64, not_found: bool) -> Cached {
        let file = self.file(name, key);
        let Some(Value::Object(data)) = read_file(&file) else {
            // `if data and ...`: not a dict, or an empty one, is nothing
            return Cached::Nothing;
        };
        if let Some(value) = data.get("value") {
            log::debug(&format!(
                "load from cache: ({}, {})",
                json::py_repr_str(name),
                json::py_repr_str(key)
            ));
            return Cached::Value(value.clone());
        }
        if not_found {
            let when = data
                .get("not_found")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            if now - when < NOT_FOUND_TTL {
                return Cached::NotFound(when);
            }
        }
        Cached::Nothing
    }

    /// `_write_cache_file`, atomically. A cache that cannot be written is not an error.
    pub fn put(&self, name: &str, key: &str, data: Value) {
        let file = self.file(name, key);
        let path = Path::new(&file);
        let Some(parent) = path.parent() else { return };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        let tmp = parent.join(format!(
            "{}.{:x}.tmp",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("cache"),
            std::process::id()
        ));
        if std::fs::write(&tmp, json::dumps_compact(&data)).is_ok()
            && std::fs::rename(&tmp, path).is_err()
        {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    pub fn put_value(&self, name: &str, key: &str, value: Value) {
        let mut data = Map::new();
        data.insert("value".into(), value);
        self.put(name, key, Value::Object(data));
    }

    pub fn put_not_found(&self, name: &str, key: &str, when: f64) {
        let mut data = Map::new();
        data.insert("not_found".into(), serde_json::json!(when));
        self.put(name, key, Value::Object(data));
    }
}

/// `time.ctime(t)` in the local time zone: `Tue Oct  8 15:30:00 2026`.
pub fn ctime(t: f64) -> String {
    let secs = t as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    let days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {} {:2} {:02}:{:02}:{:02} {}",
        days[tm.tm_wday as usize % 7],
        months[tm.tm_mon as usize % 12],
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        tm.tm_year + 1900
    )
}
