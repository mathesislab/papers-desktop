//! Crossref's works: the requests (a journal's works by ISSN, a work by its
//! DOI, a title searched, a ScienceDirect PII), their JSON, and a work as an
//! entry: its abstract (JATS) as plain text, its relations as versions.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::Deserialize;

use crate::http::{Access, doi_path, with_query};
use crate::model::{Entry, ItemKind, Stage, Version};
use crate::normalize::{arxiv_id, canonical_doi, is_attachment_doi, is_preprint_doi};
use crate::parse::{EDITORIAL, abstract_text, plain};

const WORKS: &str = "https://api.crossref.org/works";

/// The fields of a work that a list asks for.
const SELECT: &str = "DOI,title,author,container-title,abstract,URL,issued,type,relation";

/// The works of a page of a list.
pub(crate) const ROWS: usize = 100;

/// The works a title search ranks.
const SEARCH_ROWS: usize = 5;

/// An answer of the API.
#[derive(Debug, Deserialize)]
pub(crate) struct Message<T> {
    pub message: T,
}

/// A page of a list of works.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct List {
    pub items: Vec<Work>,
    #[serde(rename = "next-cursor")]
    pub next_cursor: Option<String>,
}

/// A work, with the fields used.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Work {
    #[serde(rename = "DOI")]
    pub doi: String,
    pub title: Vec<String>,
    pub author: Vec<Author>,
    #[serde(rename = "container-title")]
    pub container_title: Vec<String>,
    /// JATS
    #[serde(rename = "abstract")]
    pub abstract_: Option<String>,
    #[serde(rename = "URL")]
    pub url: Option<String>,
    pub issued: Option<Dated>,
    /// `journal-article`, `posted-content`, …
    #[serde(rename = "type")]
    pub kind: String,
    /// `preprint` of a `posted-content`
    pub subtype: Option<String>,
    /// `has-preprint`, `is-preprint-of`, `is-version-of`, `has-review`, …
    pub relation: BTreeMap<String, Vec<Relation>>,
}

/// An author: a person's given and family names, or an organisation's name.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Author {
    pub given: Option<String>,
    pub family: Option<String>,
    pub name: Option<String>,
}

/// A date as Crossref gives it: `[[year, month, day]]`, the month and day optional.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Dated {
    #[serde(rename = "date-parts")]
    pub parts: Vec<Vec<Option<i32>>>,
}

/// A work related to another.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Relation {
    #[serde(rename = "id-type")]
    pub id_type: String,
    pub id: String,
}

/// The parameters of every request: the address of the polite pool, if set.
fn polite<'a>(mut pairs: Vec<(&'a str, &'a str)>, access: &'a Access) -> Vec<(&'a str, &'a str)> {
    if let Some(mailto) = &access.mailto {
        pairs.push(("mailto", mailto));
    }
    pairs
}

/// A page of a journal's works indexed since a day and created since
/// another, from a cursor (`*` the first).
pub(crate) fn list_url(
    issn: &str,
    indexed: NaiveDate,
    created: NaiveDate,
    cursor: &str,
    access: &Access,
) -> String {
    let filter = format!("issn:{issn},from-index-date:{indexed},from-created-date:{created}");
    let rows = ROWS.to_string();
    let pairs = vec![
        ("filter", filter.as_str()),
        ("select", SELECT),
        ("rows", &rows),
        ("cursor", cursor),
    ];
    with_query(WORKS, &polite(pairs, access))
}

/// The work of a DOI.
pub(crate) fn work_url(doi: &str, access: &Access) -> String {
    with_query(
        &format!("{WORKS}/{}", doi_path(doi)),
        &polite(vec![], access),
    )
}

/// The works best matching a title.
pub(crate) fn search_url(title: &str, access: &Access) -> String {
    let rows = SEARCH_ROWS.to_string();
    let pairs = vec![
        ("query.bibliographic", title),
        ("rows", &rows),
        ("select", SELECT),
    ];
    with_query(WORKS, &polite(pairs, access))
}

/// The work of a ScienceDirect PII (Elsevier deposits it as an alternative id).
pub(crate) fn pii_url(pii: &str, access: &Access) -> String {
    let filter = format!("alternative-id:{pii}");
    let pairs = vec![
        ("filter", filter.as_str()),
        ("rows", "1"),
        ("select", SELECT),
    ];
    with_query(WORKS, &polite(pairs, access))
}

impl Work {
    /// The canonical DOI.
    pub fn doi(&self) -> Option<String> {
        canonical_doi(&self.doi)
    }

    /// The title, plain text.
    pub fn title(&self) -> String {
        self.title.first().map(|t| plain(t)).unwrap_or_default()
    }

    /// Whether it is a preprint: posted content, a preprint's relation or DOI.
    fn preprint(&self, doi: &str) -> bool {
        self.kind == "posted-content"
            || self.subtype.as_deref() == Some("preprint")
            || self.relation.contains_key("is-preprint-of")
            || is_preprint_doi(doi)
    }

    /// Its DOI and those of its other versions, with their stages.
    fn versions(&self, doi: &str, preprint: bool) -> Vec<Version> {
        let stage = |preprint: bool| match preprint {
            true => Stage::Preprint,
            false => Stage::Published,
        };
        let mut versions = vec![Version {
            doi: doi.to_string(),
            stage: stage(preprint),
        }];
        let related = [
            ("has-preprint", Some(Stage::Preprint)),
            ("is-preprint-of", Some(Stage::Published)),
            ("is-version-of", None),
            ("has-version", None),
        ];
        for (name, known) in related {
            let ids = self.relation.get(name).into_iter().flatten();
            for id in ids.filter(|r| r.id_type == "doi") {
                let Some(other) = canonical_doi(&id.id) else {
                    continue;
                };
                if is_attachment_doi(&other) || versions.iter().any(|v| v.doi == other) {
                    continue;
                }
                let stage = known.unwrap_or_else(|| stage(is_preprint_doi(&other)));
                versions.push(Version { doi: other, stage });
            }
        }
        versions
    }

    /// The date it was issued (online or in print, the earlier).
    fn date(&self) -> Option<DateTime<Utc>> {
        let parts = self.issued.as_ref()?.parts.first()?;
        let part = |i: usize, default: u32| {
            parts
                .get(i)
                .copied()
                .flatten()
                .map_or(Some(default), |n| u32::try_from(n).ok())
        };
        let year = parts.first().copied().flatten()?;
        let date = NaiveDate::from_ymd_opt(year, part(1, 1)?, part(2, 1)?)?;
        Some(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?))
    }

    /// As an entry of a source (a sweep's subscription, [`crate::LOOKUP`], a
    /// post's subscription), of a kind unless it is a preprint; None without
    /// a DOI.
    pub fn entry(&self, source: &str, kind: ItemKind) -> Option<Entry> {
        let doi = self.doi()?;
        let preprint = self.preprint(&doi);
        let title = self.title();
        let authors = self
            .author
            .iter()
            .filter_map(|a| match (&a.given, &a.family, &a.name) {
                (Some(given), Some(family), _) => Some(format!("{given} {family}")),
                (None, Some(family), _) => Some(family.clone()),
                (_, None, name) => name.clone(),
            })
            .collect();
        let summary = self
            .abstract_
            .as_deref()
            .map(abstract_text)
            .filter(|s| !s.is_empty());
        let kind = match kind {
            ItemKind::Paper if preprint => ItemKind::Preprint,
            kind => kind,
        };
        Some(Entry {
            source: source.to_string(),
            guid: doi.clone(),
            editorial: EDITORIAL.is_match(&title),
            title,
            authors,
            journal: self.container_title.first().map(|t| plain(t)),
            date: self.date(),
            arxiv: arxiv_id(&doi),
            url: self.url.clone(),
            summary,
            content: None,
            kind,
            versions: self.versions(&doi, preprint),
            doi: Some(doi),
        })
    }
}
