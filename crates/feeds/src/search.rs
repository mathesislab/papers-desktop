//! Searching the works of OpenAlex, as a search engine of papers does: a
//! query of the feeds' language (core's) as OpenAlex's parameters. Its words
//! and phrases are the full-text `search` (`-` a NOT); its fields filters:
//! `t:` the title, `abs:` the abstract, `a:` an author's name as written,
//! `j:` a journal (looked up by its name), `y:` the years, `doi:` DOIs, and
//! the facets `cites:` (the works that cite a work, by its OpenAlex id) and
//! `citedby:` (the works a work cites). What OpenAlex cannot be asked (a
//! field it has not, a title that excludes) is refused, said. Sorted by
//! relevance (without words, the most cited first), the most cited or the
//! newest; 25 a page, the next from a cursor.
//!
//! The works found go into a folder of feeds of their own (the app's
//! `search`), an item each: the searches before go, but for the items saved
//! for later (shortlisted) or added.

use papers_core::{Query, Schema};
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::feeds::Feeds;
use crate::http::{Access, Http, get_json, with_query};
use crate::journals::{self, journal_key};
use crate::model::{Entry, ItemKind, Triage};
use crate::normalize::canonical_doi;
use crate::openalex::{self, List};

/// The source of the entries of the works found.
pub const FOUND: &str = "search";

/// The facets of a search: the works around a work.
pub const SEARCH_FACETS: &[&str] = &["cites", "citedby"];

/// The works of a page of a search.
pub const PER_SEARCH: usize = 25;

const SCHEMA: Schema = Schema {
    facets: SEARCH_FACETS,
    numbers: &[],
};

/// The order of the works found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Order {
    /// the best match first; without words, the most cited
    #[default]
    Relevance,
    Cited,
    Newest,
}

/// What a query asks of OpenAlex: its full-text search, its filters, and
/// the journals to look up by name (any of them; excluded with `true`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Asked {
    pub search: Vec<String>,
    pub filters: Vec<String>,
    pub journals: Vec<(Vec<String>, bool)>,
}

/// A page of a search: the works found, how many there are in all, the
/// cursor of the next page (none after the last).
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub entries: Vec<Entry>,
    pub total: u64,
    pub next: Option<String>,
}

/// What a query asks of OpenAlex; what it cannot ask, said.
pub fn ask(query: &str) -> std::result::Result<Asked, String> {
    let mut asked = Asked::default();
    for term in Query::written(query, SCHEMA) {
        let values: Vec<String> = term
            .values
            .iter()
            .filter(|v| !v.text.trim().is_empty())
            .map(|v| match v.quoted {
                true => format!("\"{}\"", v.text.trim()),
                false => v.text.trim().to_string(),
            })
            .collect();
        if values.is_empty() {
            continue;
        }
        let name = term.field.as_deref();
        let refused = |why: &str| {
            let field = name.map_or(String::new(), |n| format!("{n}:"));
            Err(format!(
                "{}{field}: {why}",
                if term.negated { "-" } else { "" }
            ))
        };
        let filter =
            |field: &str, values: &[String], joint: &str| format!("{field}:{}", values.join(joint));
        match name {
            None => asked
                .search
                .extend(values.into_iter().map(|v| match term.negated {
                    true => format!("NOT {v}"),
                    false => v,
                })),
            Some("title" | "abstract" | "author") if term.negated => {
                return refused("OpenAlex cannot leave out by it");
            }
            Some("title") => asked.filters.push(filter("title.search", &values, " OR ")),
            Some("abstract") => asked
                .filters
                .push(filter("abstract.search", &values, " OR ")),
            Some("author") => asked
                .filters
                .push(filter("raw_author_name.search", &values, " OR ")),
            Some("year") => {
                let years: Vec<String> = values
                    .iter()
                    .map(|v| years(v))
                    .collect::<Option<_>>()
                    .ok_or_else(|| format!("y:{}: not years", values.join(",")))?;
                if term.negated && years.iter().any(|y| !y.chars().all(|c| c.is_ascii_digit())) {
                    return refused("OpenAlex cannot leave out a span of years");
                }
                let not = if term.negated { "!" } else { "" };
                let years: Vec<String> = years.iter().map(|y| format!("{not}{y}")).collect();
                let joint = if term.negated { "+" } else { "|" };
                asked
                    .filters
                    .push(filter("publication_year", &years, joint));
            }
            Some("journal") => {
                let names = values.iter().map(|v| v.trim_matches('"').to_string());
                asked.journals.push((names.collect(), term.negated));
            }
            Some("doi") => {
                let dois: Vec<String> = values
                    .iter()
                    .map(|v| canonical_doi(v.trim_matches('"')))
                    .collect::<Option<_>>()
                    .ok_or_else(|| format!("doi:{}: not a DOI", values.join(",")))?;
                let not = if term.negated { "!" } else { "" };
                asked.filters.push(format!("doi:{not}{}", dois.join("|")));
            }
            Some(facet @ ("cites" | "citedby")) => {
                let field = if facet == "cites" {
                    "cites"
                } else {
                    "cited_by"
                };
                let not = if term.negated { "!" } else { "" };
                let works: Vec<String> = values.iter().map(|v| work_id(v)).collect();
                asked
                    .filters
                    .push(format!("{field}:{not}{}", works.join("|")));
            }
            Some(_) => return refused("not a field of OpenAlex's works"),
        }
    }
    Ok(asked)
}

/// The years of a value as OpenAlex's `publication_year` takes them:
/// `2000-2010`, `2000-` (>1999), `-2010` (<2011), `2005`, `201` (the 2010s).
fn years(value: &str) -> Option<String> {
    let year = |s: &str| s.parse::<i32>().ok();
    match value.split_once('-') {
        Some((from, "")) => Some(format!(">{}", year(from)? - 1)),
        Some(("", to)) => Some(format!("<{}", year(to)? + 1)),
        Some((from, to)) => Some(format!("{}-{}", year(from)?, year(to)?)),
        None if value.len() == 3 => {
            let decade = year(value)? * 10;
            Some(format!("{decade}-{}", decade + 9))
        }
        None => year(value).map(|y| y.to_string()),
    }
}

/// An OpenAlex id (`W2741809807`) of a value, its address or the id.
fn work_id(value: &str) -> String {
    let value = value.trim_matches('"').trim();
    let id = value.rsplit('/').next().unwrap_or(value);
    id.to_ascii_uppercase()
}

/// The request of a page of a search, its journals' ids found: from the
/// first page, or from a cursor.
pub fn search_url(
    asked: &Asked,
    journals: &[(Vec<String>, bool)],
    order: Order,
    cursor: Option<&str>,
    access: &Access,
) -> std::result::Result<String, String> {
    let mut filters = asked.filters.clone();
    for (ids, excluded) in journals {
        let field = "primary_location.source.id";
        filters.push(match excluded {
            true => format!(
                "{field}:{}",
                ids.iter()
                    .map(|i| format!("!{i}"))
                    .collect::<Vec<_>>()
                    .join("+")
            ),
            false => format!("{field}:{}", ids.join("|")),
        });
    }
    let search = asked.search.join(" ");
    if search.is_empty() && filters.is_empty() {
        return Err("Nothing to search for".to_string());
    }
    let sort = match order {
        Order::Relevance if !search.is_empty() => "relevance_score:desc",
        Order::Relevance | Order::Cited => "cited_by_count:desc",
        Order::Newest => "publication_date:desc",
    };
    let filter = filters.join(",");
    let per_page = PER_SEARCH.to_string();
    let select = format!("{},cited_by_count", openalex::SELECT);
    let mut pairs = vec![];
    if !search.is_empty() {
        pairs.push(("search", search.as_str()));
    }
    if !filter.is_empty() {
        pairs.push(("filter", filter.as_str()));
    }
    pairs.extend([
        ("sort", sort),
        ("select", select.as_str()),
        ("per-page", per_page.as_str()),
        ("cursor", cursor.unwrap_or("*")),
    ]);
    Ok(with_query(
        openalex::WORKS,
        &openalex::access_pairs(pairs, access),
    ))
}

/// A page of the works of a query, in an order: from the first, or from a
/// cursor. The journals of `j:` are looked up by name first: the one whose
/// title is the name, else OpenAlex's best.
pub fn search_works(
    http: &dyn Http,
    access: &Access,
    query: &str,
    order: Order,
    cursor: Option<&str>,
) -> std::result::Result<Page, String> {
    let asked = ask(query)?;
    let mut journals = Vec::new();
    for (names, excluded) in &asked.journals {
        let mut ids = Vec::new();
        for name in names {
            let found = journals::sources(http, access, name)?;
            let key = journal_key(name);
            let best = (found.iter().find(|s| journal_key(&s.display_name) == key))
                .or(found.first())
                .ok_or_else(|| format!("j:{name}: no such journal in OpenAlex"))?;
            ids.push(work_id(&best.id));
        }
        journals.push((ids, *excluded));
    }
    let url = search_url(&asked, &journals, order, cursor, access)?;
    let list: List = get_json(http, &url)?.unwrap_or_default();
    let entries = list
        .results
        .iter()
        .map(|w| w.entry(FOUND, ItemKind::Paper))
        .collect();
    Ok(Page {
        entries,
        total: list.meta.count,
        next: list
            .meta
            .next_cursor
            .filter(|_| list.results.len() == PER_SEARCH),
    })
}

impl Feeds {
    /// The works found, into the items, an item each (merged as a feed's
    /// entries are); their ids, in their order. With `fresh` (another
    /// search), the items of the searches before go first, but for those
    /// saved for later (shortlisted) or added.
    pub fn found(&mut self, entries: Vec<Entry>, fresh: bool) -> Result<Vec<String>> {
        if fresh {
            let kept = |t: Triage| matches!(t, Triage::Shortlisted | Triage::Added);
            let gone: Vec<String> = (self.items().iter())
                .filter(|i| !kept(self.triage(&i.id)))
                .map(|i| i.id.clone())
                .collect();
            self.forget(&gone);
        }
        let guids: Vec<String> = entries.iter().map(|e| e.guid.clone()).collect();
        self.add(entries, false);
        let of_guid = |guid: &String| {
            let item = self.items().iter().find(|i| {
                (i.sources.iter()).any(|s| s.entry.source == FOUND && &s.entry.guid == guid)
            });
            item.map(|i| i.id.clone())
        };
        let mut ids: Vec<String> = Vec::new();
        for id in guids.iter().filter_map(of_guid) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        self.save()?;
        Ok(ids)
    }
}
