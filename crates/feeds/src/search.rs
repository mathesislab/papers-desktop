//! Searching the works of OpenAlex, as a search engine of papers does: a
//! query of the feeds' language (core's) as OpenAlex's parameters. Its words
//! and phrases are the full-text `search` (`-` a NOT); its fields filters:
//! `t:` the title, `abs:` the abstract, `a:` an author's name as written,
//! `j:` a journal (looked up by its name), `y:` the years (core's
//! [`year_range`], the grammar of the library's `year:`), `doi:` DOIs, and
//! the facets `cites:` (the works that cite a work, by its OpenAlex id or
//! its DOI) and `citedby:` (the works a work cites). What OpenAlex cannot be asked (a
//! field it has not, a title that excludes) is refused, said. Sorted by
//! relevance (without words, the most cited first), the most cited or the
//! newest; 25 a page, the next from a cursor.
//!
//! The works found go into a folder of feeds of their own (the app's
//! `search`), an item each: the searches before go, but for the items saved
//! for later (shortlisted) or added. A search followed is a sweep of the
//! feeds: its words searched in the titles and abstracts.

use std::ops::RangeInclusive;

use papers_core::{Query, Schema, year_range};
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::feeds::Feeds;
use crate::http::{Access, Http, get_json, with_query};
use crate::journals::{self, journal_key};
use crate::model::{Entry, ItemKind, Triage};
use crate::normalize::canonical_doi;
use crate::openalex::{self, List, Work};

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

/// What a query asks of OpenAlex: its full-text search, its filters, the
/// journals to look up by name and the works around a work (by its id, or
/// by its DOI to look up), any of each; excluded with `true`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Asked {
    pub search: Vec<String>,
    pub filters: Vec<String>,
    pub journals: Vec<(Vec<String>, bool)>,
    /// `cites` or `cited_by`, the works
    pub around: Vec<(&'static str, Vec<String>, bool)>,
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
                let years: Vec<RangeInclusive<i32>> = values
                    .iter()
                    .map(|v| year_range(v))
                    .collect::<Option<_>>()
                    .ok_or_else(|| format!("y:{}: not years", values.join(",")))?;
                if term.negated && years.iter().any(|y| y.start() != y.end()) {
                    return refused("OpenAlex cannot leave out a span of years");
                }
                let not = if term.negated { "!" } else { "" };
                let years: Vec<String> = (years.iter())
                    .map(|y| format!("{not}{}", publication_years(y)))
                    .collect();
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
                let works = values.iter().map(|v| {
                    let v = v.trim_matches('"');
                    canonical_doi(v).unwrap_or_else(|| work_id(v))
                });
                asked.around.push((field, works.collect(), term.negated));
            }
            Some(_) => return refused("not a field of OpenAlex's works"),
        }
    }
    Ok(asked)
}

/// Years of core's [`year_range`] as OpenAlex's `publication_year` takes
/// them: `2000-2010`, `>1999` (from 2000), `<2011` (to 2010), `2005`.
fn publication_years(years: &RangeInclusive<i32>) -> String {
    match (*years.start(), *years.end()) {
        (i32::MIN, to) => format!("<{}", to + 1),
        (from, i32::MAX) => format!(">{}", from - 1),
        (from, to) if from == to => from.to_string(),
        (from, to) => format!("{from}-{to}"),
    }
}

/// An OpenAlex id (`W2741809807`) of a value, its address or the id.
fn work_id(value: &str) -> String {
    let value = value.trim_matches('"').trim();
    let id = value.rsplit('/').next().unwrap_or(value);
    id.to_ascii_uppercase()
}

/// A filter of any of some ids, or of none of them.
fn any_of(field: &str, ids: &[String], excluded: bool) -> String {
    match excluded {
        true => {
            let not: Vec<String> = ids.iter().map(|i| format!("!{i}")).collect();
            format!("{field}:{}", not.join("+"))
        }
        false => format!("{field}:{}", ids.join("|")),
    }
}

/// The filters of what a query asks, its journals and works looked up: a
/// journal by its name (the one whose title is the name, else OpenAlex's
/// best), a work by its DOI.
pub fn filters(
    http: &dyn Http,
    access: &Access,
    asked: &Asked,
) -> std::result::Result<Vec<String>, String> {
    let mut filters = asked.filters.clone();
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
        filters.push(any_of("primary_location.source.id", &ids, *excluded));
    }
    for (field, works, excluded) in &asked.around {
        let mut ids = Vec::new();
        for work in works {
            if !work.starts_with("10.") {
                ids.push(work.clone());
                continue;
            }
            let found: Option<Work> = get_json(http, &openalex::work_url(work, access))?;
            let found = found.ok_or_else(|| format!("{work}: not in OpenAlex"))?;
            ids.push(work_id(&found.id));
        }
        filters.push(any_of(field, &ids, *excluded));
    }
    Ok(filters)
}

/// The request of a page of a search, of its words and its filters: from
/// the first page, or from a cursor.
pub fn search_url(
    words: &[String],
    filters: &[String],
    order: Order,
    cursor: Option<&str>,
    access: &Access,
) -> std::result::Result<String, String> {
    let search = words.join(" ");
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
/// cursor (its journals and works looked up again: `filters`).
pub fn search_works(
    http: &dyn Http,
    access: &Access,
    query: &str,
    order: Order,
    cursor: Option<&str>,
) -> std::result::Result<Page, String> {
    let asked = ask(query)?;
    let filters = filters(http, access, &asked)?;
    let url = search_url(&asked.search, &filters, order, cursor, access)?;
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

/// The OpenAlex filter of a query followed as a sweep of the feeds: its
/// words searched in the titles and abstracts (their commas, which would end
/// the filter, dropped), then its filters.
pub fn follow_filter(
    http: &dyn Http,
    access: &Access,
    query: &str,
) -> std::result::Result<String, String> {
    let asked = ask(query)?;
    let mut all = Vec::new();
    let words = asked.search.join(" ").replace(',', " ");
    let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
    if !words.is_empty() {
        all.push(format!("title_and_abstract.search:{words}"));
    }
    all.extend(filters(http, access, &asked)?);
    if all.is_empty() {
        return Err("Nothing to follow".to_string());
    }
    Ok(all.join(","))
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
