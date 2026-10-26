//! A library entry as the panel shows and searches it.

use std::path::PathBuf;

use serde::Serialize;

use crate::csl;

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub key: String,
    /// CSL type, e.g. `article-journal`
    pub kind: String,
    pub title: String,
    /// display names, "Given Family"
    pub authors: Vec<String>,
    pub year: Option<i32>,
    /// journal or book title
    pub container: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
    pub keywords: Vec<String>,
    pub files: Vec<PathBuf>,
    /// one line under the title: "Feldmann & Levermann · 2017 · Cryosphere"
    pub subtitle: String,
    /// family names with their particle ("van den Broeke"), in author order
    #[serde(skip)]
    pub(crate) families: Vec<String>,
}

impl Item {
    pub(crate) fn from_csl(e: csl::Entry) -> Item {
        let names = if e.author.is_empty() {
            &e.editor
        } else {
            &e.author
        };
        let families: Vec<String> = names.iter().filter_map(family).collect();
        let authors = names.iter().filter_map(display_name).collect();
        let year = e.issued.as_ref().and_then(csl::Date::year);
        let container = e
            .container_title
            .as_deref()
            .map(clean)
            .filter(|s| !s.is_empty());
        let key = e.citation_key.unwrap_or(e.id);
        let title = e.title.as_deref().map(clean).unwrap_or_default();
        let keywords = e
            .keyword
            .as_deref()
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .map(String::from)
            .collect();
        let subtitle = subtitle(&families, year, container.as_deref());
        Item {
            title: if title.is_empty() { key.clone() } else { title },
            key,
            kind: e.kind,
            authors,
            year,
            container,
            doi: e.doi.filter(|s| !s.is_empty()),
            url: e.url.filter(|s| !s.is_empty()),
            keywords,
            files: e.custom.file.into_iter().map(PathBuf::from).collect(),
            subtitle,
            families,
        }
    }
}

fn family(n: &csl::Name) -> Option<String> {
    let family = match (&n.particle, &n.family, &n.literal) {
        (Some(p), Some(f), _) => format!("{p} {f}"),
        (None, Some(f), _) => f.clone(),
        (_, None, Some(l)) => l.clone(),
        _ => return None,
    };
    Some(clean(&family)).filter(|s| !s.is_empty())
}

fn display_name(n: &csl::Name) -> Option<String> {
    let family = family(n)?;
    Some(match n.given.as_deref().map(clean) {
        Some(given) if !given.is_empty() => format!("{given} {family}"),
        _ => family,
    })
}

fn subtitle(families: &[String], year: Option<i32>, container: Option<&str>) -> String {
    let names = match families {
        [] => None,
        [a] => Some(a.clone()),
        [a, b] => Some(format!("{a} & {b}")),
        [a, ..] => Some(format!("{a} et al.")),
    };
    [
        names,
        year.map(|y| y.to_string()),
        container.map(String::from),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}

/// Strip the bibtex braces left in a field and collapse whitespace.
fn clean(s: &str) -> String {
    s.replace(['{', '}'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
