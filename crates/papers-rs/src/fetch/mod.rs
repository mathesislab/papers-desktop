//! Bibliographic metadata over the network (`papers.fetch`): crossref (by DOI and by
//! bibliographic search), doi.org content negotiation, arXiv, and the journal website, with
//! the results cached as papers caches them (`cache`).
//!
//! Not ported: Google Scholar (`scholarly`). An extension of papers-rs instead: the search of
//! OpenAlex (`openalex`).
//!
//! Where papers raises an exception, a [`FetchError`] has its message and what papers
//! does with it: `NotFound` and `Request` are its `DOINotFoundError` and `DOIRequestError`,
//! `Value` a `ValueError`, `Other` what papers does not catch (a traceback there).

pub mod cache;
pub mod crossref;
pub mod download;
pub mod fuzz;
pub mod html;
pub mod http;
pub mod openalex;

use std::fmt;

use serde_json::{Map, Value, json};

use self::cache::{Cache, Cached, ctime, hashed_key, now};
use self::crossref::{crossref_score, crossref_to_bibtex};
use self::http::{Network, Response, Transport, quote, with_params};
use crate::bib::Bib;
use crate::config::json::{dumps_compact, loads, py_float_repr, py_repr_str};
use crate::identity::arxiv_of_doi;
use crate::log;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// `DOINotFoundError`: the DOI does not exist (unknown to doi.org)
    NotFound,
    /// `DOIRequestError`
    Request,
    /// another `ValueError`
    Value,
    /// any other exception
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchError {
    pub kind: ErrorKind,
    pub message: String,
}

impl FetchError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> FetchError {
        FetchError {
            kind,
            message: message.into(),
        }
    }

    /// a `DOIRequestError` (the not-found one is a kind of it)
    pub fn is_request_error(&self) -> bool {
        matches!(self.kind, ErrorKind::NotFound | ErrorKind::Request)
    }

    /// a `ValueError`: what the commands report as an error of the file or the query
    pub fn is_value_error(&self) -> bool {
        self.kind != ErrorKind::Other
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FetchError {}

type Result<T> = std::result::Result<T, FetchError>;

fn request_error(message: impl Into<String>) -> FetchError {
    FetchError::new(ErrorKind::Request, message)
}

/// The record fields that `crossref_to_bibtex` and the score use.
pub const CROSSREF_SELECT: &str = "DOI,URL,type,title,author,editor,container-title,volume,issue,page,publisher,publisher-location,ISBN,issued,published-print,published-online,abstract";

/// CSL types (doi.org content negotiation) as crossref types.
fn csl_type(t: &str) -> &str {
    match t {
        "article-journal" => "journal-article",
        "chapter" => "book-chapter",
        "paper-conference" => "proceedings-article",
        other => other,
    }
}

/// The queries of papers: a transport, and the cache of their results.
pub struct Fetcher {
    transport: Box<dyn Transport>,
    cache: Cache,
    /// when the last request to OpenAlex was made (`openalex`)
    openalex_last: std::sync::Mutex<Option<std::time::Instant>>,
}

impl Fetcher {
    /// Over the network, with the cache of papers in `cache_dir`.
    pub fn new(cache_dir: &str) -> Result<Fetcher> {
        let network = Network::new().map_err(|e| FetchError::new(ErrorKind::Other, e))?;
        Ok(Fetcher::with_transport(Box::new(network), cache_dir))
    }

    pub fn with_transport(transport: Box<dyn Transport>, cache_dir: &str) -> Fetcher {
        Fetcher {
            transport,
            cache: Cache::new(cache_dir),
            openalex_last: std::sync::Mutex::new(None),
        }
    }

    /// `http_get`
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Response> {
        self.transport
            .get(url, accept)
            .map_err(|e| FetchError::new(ErrorKind::Other, e))
    }

    /// `fetch_crossref_by_doi`: the record of a DOI (the "message" of the API response).
    pub fn fetch_crossref_by_doi(&self, doi: &str) -> Result<Value> {
        let response = self.get(
            &format!("https://api.crossref.org/works/{}", quote(doi, "/")),
            None,
        )?;
        if response.status == 404 {
            return Err(FetchError::new(
                ErrorKind::NotFound,
                format!("{doi}: not found on crossref"),
            ));
        }
        let failure = |error: String| request_error(format!("{}: {error}", py_repr_str(doi)));
        if let Some(message) = response.status_error() {
            return Err(failure(format!("HTTPError({})", py_repr_str(&message))));
        }
        let body = loads(&response.text())
            .map_err(|e| failure(format!("JSONDecodeError({})", py_repr_str(&e.message))))?;
        match body {
            Value::Object(mut map) => map
                .remove("message")
                .ok_or_else(|| failure("KeyError('message')".to_string())),
            _ => Err(failure(
                "TypeError('list indices must be integers or slices, not str')".to_string(),
            )),
        }
    }

    /// `fetch_csl_by_doi`: the record of a DOI of any registration agency (crossref, DataCite...),
    /// by content negotiation on doi.org, as a crossref-like record.
    pub fn fetch_csl_by_doi(&self, doi: &str) -> Result<Value> {
        let response = self.get(
            &format!("https://doi.org/{}", quote(doi, "/")),
            Some("application/vnd.citationstyles.csl+json"),
        )?;
        if response.status == 404 {
            return Err(FetchError::new(
                ErrorKind::NotFound,
                format!("{doi}: unknown to doi.org"),
            ));
        }
        if !response.ok() || !response.content_type.to_lowercase().contains("json") {
            // the DOI exists but its metadata are not available (yet)
            return Err(request_error(format!(
                "{doi}: no metadata on doi.org (HTTP {})",
                response.status
            )));
        }
        let mut message =
            loads(&response.text()).map_err(|e| FetchError::new(ErrorKind::Value, e.message))?;
        let Value::Object(map) = &mut message else {
            // `message.get(...)` of what is not a dict
            return Err(FetchError::new(
                ErrorKind::Other,
                format!(
                    "'{}' object has no attribute 'get'",
                    crossref::type_name(&message)
                ),
            ));
        };
        for field in ["title", "container-title"] {
            if let Some(Value::String(s)) = map.get(field) {
                let list = Value::Array(vec![Value::String(s.clone())]);
                map.insert(field.to_string(), list);
            }
        }
        let t = match map.get("type") {
            Some(Value::String(s)) => Value::String(csl_type(s).to_string()),
            Some(other) => other.clone(),
            None => Value::Null,
        };
        map.insert("type".into(), t);
        Ok(message)
    }

    /// `fetch_doi_metadata`: the crossref-like record of a DOI, from the crossref API, or from
    /// doi.org for the DOIs registered elsewhere. A DOI unknown to doi.org is remembered as not
    /// found for a day (a new paper's DOI may be printed before it is registered). The cache
    /// has the DOI in lower case; the queries use it as given.
    pub fn fetch_doi_metadata(&self, doi: &str) -> Result<Value> {
        let key = doi.to_lowercase();
        match self.cache.get("doi", &key, now(), true) {
            Cached::Value(v) => return Ok(v),
            Cached::NotFound(when) => {
                return Err(FetchError::new(
                    ErrorKind::NotFound,
                    format!(
                        "{doi}: not found (cached until {})",
                        ctime(when + cache::NOT_FOUND_TTL)
                    ),
                ));
            }
            Cached::Nothing => {}
        }
        let fetched = match self.fetch_crossref_by_doi(doi) {
            Ok(v) => Ok(v),
            Err(error) if error.is_request_error() => {
                log::debug(&format!("{error}: try doi.org"));
                self.fetch_csl_by_doi(doi)
            }
            Err(error) => Err(error),
        };
        match fetched {
            Ok(mut record) => {
                // the cited references are most of the record's size, and are not used
                if let Value::Object(map) = &mut record {
                    map.remove("reference");
                }
                self.cache.put_value("doi", &key, record.clone());
                Ok(record)
            }
            Err(error) => {
                if error.kind == ErrorKind::NotFound {
                    self.cache.put_not_found("doi", &key, now());
                }
                Err(error)
            }
        }
    }

    /// `fetch_bibtex_by_arxiv`
    pub fn fetch_bibtex_by_arxiv(&self, arxiv_id: &str) -> Result<String> {
        if let Cached::Value(Value::String(text)) = self.cache.get("arxiv", arxiv_id, now(), false)
        {
            return Ok(text);
        }
        let response = self.get(&format!("https://arxiv.org/bibtex/{arxiv_id}"), None)?;
        if response.status != 200 {
            return Err(request_error(format!(
                "{arxiv_id}: unable to fetch BibTeX from arxiv (HTTP {})",
                response.status
            )));
        }
        let text = response.text();
        self.cache
            .put_value("arxiv", arxiv_id, Value::String(text.clone()));
        Ok(text)
    }

    /// `fetch_bibtex_by_doi`: the bibtex of a DOI (an arXiv DOI: from arXiv).
    pub fn fetch_bibtex_by_doi(&self, doi: &str) -> Result<String> {
        if let Some(arxiv) = arxiv_of_doi(doi) {
            return self.fetch_bibtex_by_arxiv(&arxiv);
        }
        match self.fetch_doi_metadata(doi) {
            Ok(record) => return crossref_to_bibtex(&record),
            Err(error) if error.kind == ErrorKind::NotFound => return Err(error),
            Err(error) if error.is_request_error() => log::debug(&error.message),
            Err(error) => return Err(error),
        }
        // registered but without metadata (yet): the publisher's page may link a bibtex
        match self.fetch_bibtex_on_journal_website(doi) {
            Ok(bibtex) => return Ok(bibtex),
            Err(error) => log::debug(&format!("{doi}: no bibtex on the journal website: {error}")),
        }
        Err(request_error(format!(
            "Unable to fetch BibTeX for DOI {doi}"
        )))
    }

    /// `_crossref_query`: the items of a search (the cache has them by the parameters).
    fn crossref_query(&self, params: &str) -> Result<Vec<Value>> {
        let key = hashed_key(params);
        if let Cached::Value(Value::Array(items)) =
            self.cache.get("crossref-query", &key, now(), false)
        {
            return Ok(items);
        }
        let Value::Object(map) = loads(params).expect("the parameters are JSON") else {
            unreachable!()
        };
        let query: Vec<(&str, String)> = map
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str(),
                    match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    },
                )
            })
            .collect();
        let response = self.get(&with_params("https://api.crossref.org/works", &query), None)?;
        if let Some(message) = response.status_error() {
            return Err(FetchError::new(ErrorKind::Other, message));
        }
        let body =
            loads(&response.text()).map_err(|e| FetchError::new(ErrorKind::Value, e.message))?;
        let items = body
            .get("message")
            .and_then(|m| m.get("items"))
            .and_then(|i| i.as_array())
            .cloned()
            .ok_or_else(|| FetchError::new(ErrorKind::Other, "'items'"))?;
        self.cache
            .put_value("crossref-query", &key, Value::Array(items.clone()));
        Ok(items)
    }

    /// `fetch_bibtex_by_fulltext_crossref`: the best match of a crossref bibliographic search
    /// (title, authors, other text), as bibtex.
    pub fn fetch_bibtex_by_fulltext_crossref(
        &self,
        txt: &str,
        title: Option<&str>,
        author: Option<&str>,
        rows: i64,
    ) -> Result<String> {
        let truthy = |x: &Option<&str>| x.filter(|s| !s.is_empty()).is_some();
        let query = [title, Some(txt)]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let mut params = Map::new();
        params.insert("rows".into(), json!(rows));
        params.insert("select".into(), json!(CROSSREF_SELECT));
        let mut shown = format!(
            "{{'rows': {rows}, 'select': {}",
            py_repr_str(CROSSREF_SELECT)
        );
        if !query.is_empty() {
            params.insert("query.bibliographic".into(), json!(query));
            shown.push_str(&format!(", 'query.bibliographic': {}", py_repr_str(&query)));
        }
        if truthy(&author) {
            params.insert("query.author".into(), json!(author.unwrap()));
            shown.push_str(&format!(
                ", 'query.author': {}",
                py_repr_str(author.unwrap())
            ));
        }
        shown.push('}');
        log::debug(&format!("crossref fulltext search: {shown}"));
        let results = self.crossref_query(&dumps_compact(&Value::Object(params)))?;
        if results.is_empty() {
            return Err(FetchError::new(
                ErrorKind::Value,
                "crossref fulltext: no results",
            ));
        }
        let scoretxt = [Some(query.as_str()), author]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        // the best, the first of equals
        let mut best: Option<(f64, bool, &Value)> = None;
        for result in &results {
            let (score, float) = crossref_score(&scoretxt, result)?;
            if best.is_none_or(|(b, _, _)| score > b) {
                best = Some((score, float, result));
            }
        }
        let (score, float, result) = best.expect("results is not empty");
        log::debug(&format!(
            "score: {}",
            if float {
                py_float_repr(score)
            } else {
                "0".to_string()
            }
        ));
        Ok(crossref_to_bibtex(result)?.trim().to_string())
    }

    /// `fetch_bibtex_on_journal_website`: for the DOIs registered without metadata yet (the ESD
    /// journal prints DOIs before crossref has them), a link to a `.bib` file on the page the DOI
    /// resolves to, and the entry with that DOI in it.
    pub fn fetch_bibtex_on_journal_website(&self, doi: &str) -> Result<String> {
        let base_url = format!("https://doi.org/{doi}");
        let page = self.get(&base_url, None)?;
        if let Some(message) = page.status_error() {
            return Err(FetchError::new(ErrorKind::Other, message));
        }
        let base = reqwest::Url::parse(&base_url).ok();
        for href in html::anchor_links(&page.text()) {
            if !href.ends_with(".bib") {
                continue;
            }
            let url = match &base {
                Some(base) => base.join(&href).map(|u| u.to_string()).unwrap_or(href),
                None => href,
            };
            let file = self.get(&url, None)?;
            if let Some(message) = file.status_error() {
                return Err(FetchError::new(ErrorKind::Other, message));
            }
            let content = file.text();
            let bib = Bib::parse(&content);
            let found = bib
                .entries()
                .any(|e| e.get("doi").unwrap_or("").to_lowercase() == doi.to_lowercase());
            if found {
                return Ok(content);
            }
        }
        Err(request_error(
            "No matching BibTeX entry found for the given DOI.",
        ))
    }
}
