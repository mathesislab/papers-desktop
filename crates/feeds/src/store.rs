//! The files of a feeds folder: `config.json` (the subscriptions, interests,
//! rules, saved searches, the days kept), `items.json` (the items and the aliases of merged
//! ones), `state.json` (the triage of each item), `http.json` (the
//! validators of each subscription's last answer, the time of its last
//! sweep), `lookups.json` (what was looked up, and when), `journals.json`
//! (the journals looked up in OpenAlex, [`crate::Journals`]). Each is read whole,
//! and written whole through a temporary
//! file renamed over it.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::interest::Interest;
use crate::model::{Item, Mark, Subscription};
use crate::rules::Rule;
use crate::searches::Search;
use crate::{Error, Result};

pub(crate) const CONFIG: &str = "config.json";
pub(crate) const ITEMS: &str = "items.json";
pub(crate) const STATE: &str = "state.json";
pub(crate) const HTTP: &str = "http.json";
pub(crate) const LOOKUPS: &str = "lookups.json";
pub(crate) const JOURNALS: &str = "journals.json";

/// The days a sweep looks back the first time.
pub const LOOKBACK_DAYS: u32 = 14;

/// `config.json`: what the user set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Config {
    #[serde(default)]
    pub subscriptions: Vec<Subscription>,
    #[serde(flatten)]
    pub settings: FeedSettings,
    /// the saved searches, in the order of the sidebar
    #[serde(default)]
    pub searches: Vec<Search>,
}

/// The settings of the feeds, set together (`Feeds::set_settings`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedSettings {
    #[serde(default)]
    pub interests: Vec<Interest>,
    /// the factor of a term's weight in a title, for every interest
    #[serde(default = "title_boost")]
    pub title_boost: f64,
    /// tried in order on the items a fetch brings
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// the days an item no feed lists is kept, unless shortlisted or added
    #[serde(default = "keep_days")]
    pub keep_days: u32,
    /// the days a sweep looks back the first time
    #[serde(default = "lookback_days")]
    pub lookback_days: u32,
}

impl Default for FeedSettings {
    fn default() -> FeedSettings {
        FeedSettings {
            interests: Vec::new(),
            title_boost: title_boost(),
            rules: Vec::new(),
            keep_days: keep_days(),
            lookback_days: LOOKBACK_DAYS,
        }
    }
}

fn keep_days() -> u32 {
    90
}

/// paper-feeder's: a term in the title counts twice.
fn title_boost() -> f64 {
    2.0
}

fn lookback_days() -> u32 {
    LOOKBACK_DAYS
}

/// The format of the folder, in `items.json`: since 1, each entry has its
/// origin; since 2, the pages read are kept by their URL.
pub(crate) const FORMAT: u32 = 2;

/// `items.json`: the items, and the ids of merged items to the item they went into.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Items {
    /// the format it was written in, 0 for one before formats; brought up to
    /// [`FORMAT`] when the folder opens
    #[serde(default)]
    pub format: u32,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
}

impl Default for Items {
    fn default() -> Items {
        Items {
            format: FORMAT,
            items: Vec::new(),
            aliases: BTreeMap::new(),
        }
    }
}

/// `state.json`: the triage of items, by id.
pub(crate) type State = BTreeMap<String, Mark>;

/// What a feed's next request is conditional on: the validators of its last
/// answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

/// What a subscription's next fetch starts from: its feed's validators, the
/// start of its sweep's last success.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Since {
    #[serde(flatten)]
    pub validators: Validators,
    #[serde(default)]
    pub swept: Option<DateTime<Utc>>,
}

/// `http.json`: what each subscription's next fetch starts from, by its id.
pub(crate) type Validated = BTreeMap<String, Since>;

/// `lookups.json`: what was looked up, found or not, and when, by its
/// [`Lookup::key`].
pub(crate) type Lookups = BTreeMap<String, DateTime<Utc>>;

/// What is asked for once (again only once its asking is pruned).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Lookup<'a> {
    /// the work of a DOI in Crossref
    Crossref(&'a str),
    /// the work of a DOI in OpenAlex
    OpenAlex(&'a str),
    /// a post's page, by its URL: the same whichever item the post merges into
    Page(&'a str),
    /// the work of a ScienceDirect PII in Crossref
    Pii(&'a str),
}

impl Lookup<'_> {
    /// Its key in `lookups.json`: `crossref:`, `openalex:`, `page:` or
    /// `pii:` and what is looked up.
    pub fn key(self) -> String {
        match self {
            Lookup::Crossref(doi) => format!("crossref:{doi}"),
            Lookup::OpenAlex(doi) => format!("openalex:{doi}"),
            Lookup::Page(url) => format!("page:{url}"),
            Lookup::Pii(pii) => format!("pii:{pii}"),
        }
    }
}

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
