//! The files of a feeds folder: `config.json` (the subscriptions),
//! `items.json` (the items and the aliases of merged ones), `state.json` (the
//! triage of each item), `http.json` (the validators of each subscription's
//! last answer, the time of its last sweep), `lookups.json` (what was looked
//! up, and when). Each is read whole, and written whole through a temporary
//! file renamed over it.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::model::{Item, Mark, Subscription};
use crate::{Error, Result};

pub(crate) const CONFIG: &str = "config.json";
pub(crate) const ITEMS: &str = "items.json";
pub(crate) const STATE: &str = "state.json";
pub(crate) const HTTP: &str = "http.json";
pub(crate) const LOOKUPS: &str = "lookups.json";

/// The days a sweep looks back the first time.
pub const LOOKBACK_DAYS: u32 = 14;

/// `config.json`: what the user set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Config {
    #[serde(default)]
    pub subscriptions: Vec<Subscription>,
    /// the days a sweep looks back the first time
    #[serde(default = "lookback_days")]
    pub lookback_days: u32,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            subscriptions: Vec::new(),
            lookback_days: LOOKBACK_DAYS,
        }
    }
}

fn lookback_days() -> u32 {
    LOOKBACK_DAYS
}

/// `items.json`: the items, and the ids of merged items to the item they went into.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Items {
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
}

/// `state.json`: the triage of items, by id.
pub(crate) type State = BTreeMap<String, Mark>;

/// What the next request of a subscription is conditional on: the validators
/// of a feed's last answer, the start of a sweep's last success.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    #[serde(default)]
    pub swept: Option<DateTime<Utc>>,
}

/// `http.json`: the validators, by subscription id.
pub(crate) type Validated = BTreeMap<String, Validators>;

/// `lookups.json`: what was looked up, found or not, and when: `crossref:`
/// and `openalex:` and a DOI, `page:` and an item's id.
pub(crate) type Lookups = BTreeMap<String, DateTime<Utc>>;

/// A file of the folder, or its default if there is none yet.
pub(crate) fn read<T: DeserializeOwned + Default>(dir: &Path, name: &str) -> Result<T> {
    let path = dir.join(name);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(source) => return Err(Error::Read { path, source }),
    };
    serde_json::from_str(&text).map_err(|source| Error::Json { path, source })
}

/// A file of the folder, written whole or not at all.
pub(crate) fn write<T: Serialize>(dir: &Path, name: &str, value: &T) -> Result<()> {
    let path = dir.join(name);
    let failed = |source: std::io::Error| Error::Write {
        path: path.clone(),
        source,
    };
    let json = serde_json::to_vec_pretty(value).map_err(|source| Error::Json {
        path: path.clone(),
        source,
    })?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(failed)?;
    temp.write_all(&json).map_err(failed)?;
    temp.as_file().sync_all().map_err(failed)?;
    temp.persist(&path).map_err(|e| failed(e.error))?;
    Ok(())
}

/// The folder, created if it is not there.
pub(crate) fn folder(dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).map_err(|source| Error::Write {
        path: dir.to_path_buf(),
        source,
    })?;
    Ok(dir.to_path_buf())
}
