//! OpenAlex's works: the requests (the articles of a filter created since a
//! day, a work by its DOI; each with the key), their JSON, and a work as an
//! entry, its abstract rebuilt from the inverted index.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::Deserialize;

use crate::http::{Access, doi_path, with_query};
use crate::model::{Entry, ItemKind};
use crate::normalize::{arxiv_id, canonical_doi, is_preprint_doi};
use crate::parse::{EDITORIAL, plain};

const WORKS: &str = "https://api.openalex.org/works";

/// The fields of a work asked for.
const SELECT: &str =
    "id,doi,title,publication_date,authorships,primary_location,abstract_inverted_index,type";

/// The works of a page of a list.
pub(crate) const PER_PAGE: usize = 100;

/// A page of a list of works.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct List {
    pub meta: Meta,
    pub results: Vec<Work>,
}

/// What a page says of the list.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Meta {
    pub next_cursor: Option<String>,
}

/// A work, with the fields used.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Work {
    /// `https://openalex.org/W…`
    pub id: String,
    /// `https://doi.org/…`
    pub doi: Option<String>,
    pub title: Option<String>,
    pub publication_date: Option<String>,
    pub authorships: Vec<Authorship>,
    pub primary_location: Option<Location>,
    /// each word of the abstract and its positions
    pub abstract_inverted_index: Option<BTreeMap<String, Vec<usize>>>,
    /// `article`, `preprint`, `dataset`, …
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

/// An author of a work.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Authorship {
    pub author: Named,
}

/// Something with a name.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Named {
    pub display_name: Option<String>,
}

/// Where a work is: its journal or repository, its page.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Location {
    pub source: Option<Named>,
    pub landing_page_url: Option<String>,
}

/// The parameters of every request: the key, and the address, if set.
fn access_pairs<'a>(
    mut pairs: Vec<(&'a str, &'a str)>,
    access: &'a Access,
) -> Vec<(&'a str, &'a str)> {
    if let Some(key) = &access.openalex_key {
        pairs.push(("api_key", key));
    }
    if let Some(mailto) = &access.mailto {
        pairs.push(("mailto", mailto));
    }
    pairs
}

/// A page of the articles of a filter created since a day, from a cursor
/// (`*` the first).
pub(crate) fn list_url(filter: &str, created: NaiveDate, cursor: &str, access: &Access) -> String {
    let filter = format!("{filter},from_created_date:{created},type:article");
    let per_page = PER_PAGE.to_string();
    let pairs = vec![
        ("filter", filter.as_str()),
        ("select", SELECT),
        ("per-page", &per_page),
        ("cursor", cursor),
    ];
    with_query(WORKS, &access_pairs(pairs, access))
}

/// The work of a DOI.
pub(crate) fn work_url(doi: &str, access: &Access) -> String {
    let base = format!("{WORKS}/doi:{}", doi_path(doi));
    with_query(&base, &access_pairs(vec![("select", SELECT)], access))
}

impl Work {
    /// The abstract, its words put back in their places.
    pub fn abstract_text(&self) -> Option<String> {
        let index = self.abstract_inverted_index.as_ref()?;
        let mut words: BTreeMap<usize, &str> = BTreeMap::new();
        for (word, positions) in index {
            for &p in positions {
                words.insert(p, word);
            }
        }
        let text = words.into_values().collect::<Vec<_>>().join(" ");
        (!text.is_empty()).then_some(text)
    }

    /// The date it was published.
    fn date(&self) -> Option<DateTime<Utc>> {
        let date = NaiveDate::parse_from_str(self.publication_date.as_deref()?, "%Y-%m-%d").ok()?;
        Some(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?))
    }

    /// As an entry of a source, of a kind unless it is a preprint.
    pub fn entry(&self, source: &str, kind: ItemKind) -> Entry {
        let doi = self.doi.as_deref().and_then(canonical_doi);
        let preprint =
            self.kind.as_deref() == Some("preprint") || doi.as_deref().is_some_and(is_preprint_doi);
        let title = self.title.as_deref().map(plain).unwrap_or_default();
        let location = self.primary_location.as_ref();
        Entry {
            source: source.to_string(),
            guid: self.id.clone(),
            editorial: EDITORIAL.is_match(&title),
            title,
            authors: self
                .authorships
                .iter()
                .filter_map(|a| a.author.display_name.clone())
                .collect(),
            journal: location
                .and_then(|l| l.source.as_ref())
                .and_then(|s| s.display_name.clone()),
            date: self.date(),
            arxiv: doi.as_deref().and_then(arxiv_id),
            url: location.and_then(|l| l.landing_page_url.clone()),
            summary: self.abstract_text(),
            content: None,
            kind: match kind {
                ItemKind::Paper if preprint => ItemKind::Preprint,
                kind => kind,
            },
            versions: Vec::new(),
            doi,
        }
    }
}
