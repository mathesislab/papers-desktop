//! A full-text search on OpenAlex (<https://api.openalex.org>): an extension of papers-rs, not in
//! papers (the option `--openalex`; the standard has nothing of it).
//!
//! papers has `--scholar`, a full-text search on Google Scholar (HTML scraping with the
//! `scholarly` package, often blocked), for a PDF that has no DOI and that crossref's search does
//! not find. This is the same step with OpenAlex, an open catalogue with an API, in the chain of
//! papers where `--scholar` sits and with the same input (the query that `extract::query_text`
//! makes of the text of the PDF):
//!
//! 1. `GET /works?search.semantic=QUERY`: the works whose title and abstract are closest in
//!    meaning to the query (OpenAlex's semantic search, made for long texts: a request without
//!    a paid key may have at most 1500 characters, the query is cut there at a word; at most 50
//!    works are found). The keyword search (`search=`) cannot
//!    be used: it requires every word of the query, so a query of 200 words finds nothing. The
//!    answers are cached under `openalex/` in the cache directory, by the query (not by the
//!    number of works asked for, a larger answer serves a smaller request; nor by the e-mail
//!    address or the key, which do not change the answer).
//! 2. Each work is read as a crossref record ([`work_record`]), so that papers' own score applies
//!    ([`super::crossref::crossref_score`]: `token_set_ratio` of the family names, the title and the
//!    abstract with the text; the first of equal scores wins), and the hits are ranked by it.
//! 3. The text of the PDF must have the title of the hit (`title_in_text`, as for every result
//!    of papers; but a title of less than 20 letters and digits does not count, where papers lets
//!    those of less than ten pass as too short to tell: OpenAlex has hits whose title is garbled,
//!    or is the name of a journal or a word that any text has). The first hit in the order of the
//!    score that passes is taken, those that are copies of another work (a preprint, the abstract
//!    of a conference: OpenAlex has them with the title of the paper) after the others; if none
//!    passes, the best one is returned, and the caller's own check rejects it (as papers does
//!    for crossref).
//!    Without a text to check (`papers fetch`), the best hit is taken, as papers does.
//! 4. A hit with a DOI gives its BibTeX by papers' usual way (`Fetcher::fetch_bibtex_by_doi`:
//!    crossref, doi.org, arXiv), so the entry is the one that papers makes of that DOI. A DOI that
//!    no registry knows gives the entry made of the OpenAlex work (below), with the DOI.
//! 5. A hit without DOI gives the entry made of the OpenAlex work: the record of [`work_record`]
//!    written by `crossref_to_entry`, as papers writes a crossref record.
//!
//! The entry of a work ([`work_record`]):
//!
//! | BibTeX | OpenAlex |
//! |---|---|
//! | type | `type_crossref`, else the `type` with the type of the source: `article` of a journal `journal-article` (`article`), of a conference `proceedings-article` (`inproceedings`), `book` `book`, `book-chapter` `incollection`, `dissertation` `phdthesis`, `report` `techreport`, the others `misc` |
//! | key | the DOI; the OpenAlex id (`W2741809807`) without one (the key template of the library replaces it in `add`) |
//! | title | `title` |
//! | author | `raw_author_name` of the authorships (the name as printed), else the `display_name` of the author, as `Family, Given`: a name with a comma is read as `Family, Given`, otherwise the last word and the particles before it (`van`, `de`...) are the family name |
//! | year | `publication_year` (`0000` if unknown, as for crossref) |
//! | journal, booktitle | the name of the source (`primary_location`), of a journal article and of a proceedings article |
//! | volume, number, pages | `biblio`: volume, issue, `first_page-last_page` |
//! | publisher | the host organization of the source, for a book |
//! | url | `landing_page_url` of the primary location, else the OpenAlex page |
//! | doi | the DOI without its `https://doi.org/` |
//!
//! OpenAlex needs no key; the e-mail address (`mailto`) is sent when there is one (the "polite
//! pool"), and a key (`api_key`) when there is one. Without a key the use is limited to a small
//! daily budget per IP address (a search costs a hundredth of it: about 100 searches a day, shared
//! with the others on the network), with the free key to ten times as much. A semantic search is
//! limited to one request per second, which is what the requests keep to.

use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use super::cache::{Cached, hashed_key, now};
use super::crossref::{crossref_score, crossref_to_entry};
use super::http::with_params;
use super::{ErrorKind, FetchError, Fetcher};
use crate::bib::{Bib, Block};
use crate::config::json::{dumps_compact, loads, py_repr_str};
use crate::extract::doi::title_in_text_strict;
use crate::log;

type Result<T> = std::result::Result<T, FetchError>;

pub const API: &str = "https://api.openalex.org/works";

/// The fields of a work that are asked for.
pub const SELECT: &str = "id,doi,title,display_name,type,type_crossref,publication_year,biblio,primary_location,authorships,abstract_inverted_index";

/// How many works are asked for and scored (papers asks crossref for 5; a search costs the same
/// whatever the number, up to the 50 that a semantic search finds).
pub const ROWS: usize = 10;

/// The longest query that a semantic search uses, in characters.
const QUERY_LENGTH: usize = 1500;

/// The least time between two requests (a semantic search is limited to one per second).
const INTERVAL: Duration = Duration::from_millis(1100);

/// How the queries identify themselves to OpenAlex.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenAlex {
    /// `mailto`: the e-mail address of the user, for the polite pool.
    pub mailto: Option<String>,
    /// `api_key`: a key of OpenAlex (free), for a larger daily budget.
    pub api_key: Option<String>,
}

impl OpenAlex {
    pub fn with_mailto(mailto: impl Into<String>) -> OpenAlex {
        OpenAlex {
            mailto: Some(mailto.into()),
            api_key: None,
        }
    }
}

/// An answer that is an error (an HTTP error status: an `OSError` in papers).
fn failed(message: impl Into<String>) -> FetchError {
    FetchError::new(ErrorKind::Network, message)
}

// the work as a record
// --------------------

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// The DOI of a work, without its URL.
pub fn work_doi(work: &Value) -> Option<String> {
    let doi = text(work, "doi")?;
    let doi = [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "doi:",
    ]
    .iter()
    .find_map(|prefix| doi.strip_prefix(prefix))
    .unwrap_or(doi);
    (!doi.is_empty()).then(|| doi.to_string())
}

/// The OpenAlex id of a work: `W2741809807`.
pub fn work_id(work: &Value) -> Option<String> {
    let id = text(work, "id")?;
    Some(id.rsplit('/').next().unwrap_or(id).to_string())
}

/// The title of a work.
pub fn work_title(work: &Value) -> Option<&str> {
    text(work, "title").or_else(|| text(work, "display_name"))
}

/// The abstract of a work, from its inverted index (word to positions).
pub fn work_abstract(work: &Value) -> Option<String> {
    let index = work.get("abstract_inverted_index")?.as_object()?;
    let mut words: Vec<(u64, &str)> = Vec::new();
    for (word, positions) in index {
        for position in positions.as_array()?.iter().filter_map(Value::as_u64) {
            words.push((position, word));
        }
    }
    if words.is_empty() {
        return None;
    }
    words.sort();
    Some(
        words
            .into_iter()
            .map(|(_, w)| w)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

const PARTICLES: [&str; 19] = [
    "van", "von", "der", "den", "de", "del", "della", "di", "da", "dos", "du", "la", "le", "ten",
    "ter", "bin", "ibn", "al", "el",
];

/// A name as (given, family): `Family, Given` is read as it is written, otherwise the family
/// name is the last word with the particles before it. A name of one word is a given name only
/// (it is written as it is: the name of an organization).
pub fn split_name(name: &str) -> (String, String) {
    // the hyphens of the unicode block of punctuation that OpenAlex has in some names
    let name = name.replace(['\u{2010}', '\u{2011}'], "-");
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some((family, given)) = name.split_once(',') {
        return (given.trim().to_string(), family.trim().to_string());
    }
    let words: Vec<&str> = name.split(' ').collect();
    if words.len() < 2 {
        return (name, String::new());
    }
    let mut start = words.len() - 1;
    while start > 1 && PARTICLES.contains(&words[start - 1]) {
        start -= 1;
    }
    (words[..start].join(" "), words[start..].join(" "))
}

/// The type of a work as crossref names it.
fn crossref_type(work: &Value) -> String {
    if let Some(t) = text(work, "type_crossref") {
        return t.to_string();
    }
    let source_type = work
        .pointer("/primary_location/source/type")
        .and_then(Value::as_str);
    match text(work, "type").unwrap_or("") {
        "article" | "review" | "letter" | "editorial" | "erratum" => match source_type {
            None | Some("journal") => "journal-article",
            Some("conference") => "proceedings-article",
            Some(_) => "posted-content",
        },
        "preprint" => "posted-content",
        "dissertation" => "thesis",
        "book" => "book",
        "book-chapter" => "book-chapter",
        "report" => "report",
        other => return other.to_string(),
    }
    .to_string()
}

/// A work that is a copy of another, as OpenAlex has them with the same title: a preprint, an
/// abstract of a conference, a review of a paper, a data set...
fn is_copy(work: &Value) -> bool {
    matches!(
        text(work, "type"),
        Some(
            "preprint"
                | "conference-abstract"
                | "peer-review"
                | "paratext"
                | "dataset"
                | "supplementary-materials"
        )
    )
}

/// The work as a record of crossref (what `crossref_to_bibtex` and the score read).
pub fn work_record(work: &Value) -> Value {
    let mut record = Map::new();
    let kind = crossref_type(work);
    record.insert("type".into(), json!(kind));
    if let Some(doi) = work_doi(work) {
        record.insert("URL".into(), json!(format!("https://doi.org/{doi}")));
        record.insert("DOI".into(), json!(doi));
    } else if let Some(url) = work
        .pointer("/primary_location/landing_page_url")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| text(work, "id"))
    {
        record.insert("URL".into(), json!(url));
    }
    if let Some(title) = work_title(work) {
        record.insert("title".into(), json!([title]));
    }
    let authors: Vec<Value> = work
        .get("authorships")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let name = text(a, "raw_author_name")
                .or_else(|| a.get("author").and_then(|a| text(a, "display_name")))?;
            let (given, family) = split_name(name);
            let mut author = Map::new();
            if !given.is_empty() {
                author.insert("given".into(), json!(given));
            }
            if !family.is_empty() {
                author.insert("family".into(), json!(family));
            }
            Some(Value::Object(author))
        })
        .collect();
    if !authors.is_empty() {
        record.insert("author".into(), Value::Array(authors));
    }
    if let Some(year) = work.get("publication_year").and_then(Value::as_i64) {
        record.insert("issued".into(), json!({"date-parts": [[year]]}));
    }
    let source = work.pointer("/primary_location/source");
    if let Some(name) = source.and_then(|s| text(s, "display_name")) {
        record.insert("container-title".into(), json!([name]));
    }
    if let Some(name) = source.and_then(|s| text(s, "host_organization_name")) {
        record.insert("publisher".into(), json!(name));
    }
    if let Some(biblio) = work.get("biblio") {
        if let Some(volume) = text(biblio, "volume") {
            record.insert("volume".into(), json!(volume));
        }
        if let Some(issue) = text(biblio, "issue") {
            record.insert("issue".into(), json!(issue));
        }
        match (text(biblio, "first_page"), text(biblio, "last_page")) {
            (Some(first), Some(last)) if first != last => {
                record.insert("page".into(), json!(format!("{first}-{last}")));
            }
            (Some(first), _) => {
                record.insert("page".into(), json!(first));
            }
            _ => {}
        }
    }
    if let Some(abstract_) = work_abstract(work) {
        record.insert("abstract".into(), json!(abstract_));
    }
    Value::Object(record)
}

/// The BibTeX made of a work alone: as papers writes a crossref record, the key being the
/// DOI, or the OpenAlex id without one.
pub fn work_bibtex(work: &Value) -> Result<String> {
    let mut entry = crossref_to_entry(&work_record(work))?;
    if work_doi(work).is_none()
        && let Some(id) = work_id(work)
    {
        entry.key = id;
    }
    Ok(Bib {
        blocks: vec![Block::Entry(entry)],
    }
    .format())
}

/// The works with the score of papers, best first (the first of equal scores first): the score
/// is that of a crossref record (the family names, the title and the abstract).
pub fn rank_works<'a>(txt: &str, works: &'a [Value]) -> Result<Vec<(f64, &'a Value)>> {
    let mut ranked = Vec::with_capacity(works.len());
    for work in works {
        let (score, _) = crossref_score(txt, &work_record(work))?;
        ranked.push((score, work));
    }
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    Ok(ranked)
}

/// What the cache keeps of a work: the fields that are read, not the affiliations and
/// the other pages of the record.
fn trim(work: &Value) -> Value {
    let mut out = Map::new();
    for key in [
        "id",
        "doi",
        "title",
        "display_name",
        "type",
        "type_crossref",
        "publication_year",
        "biblio",
        "abstract_inverted_index",
    ] {
        if let Some(v) = work.get(key).filter(|v| !v.is_null()) {
            out.insert(key.into(), v.clone());
        }
    }
    let authorships: Vec<Value> = work
        .get("authorships")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|a| {
            let mut author = Map::new();
            if let Some(name) = text(a, "raw_author_name") {
                author.insert("raw_author_name".into(), json!(name));
            }
            if let Some(name) = a.get("author").and_then(|a| text(a, "display_name")) {
                author.insert("author".into(), json!({"display_name": name}));
            }
            Value::Object(author)
        })
        .collect();
    out.insert("authorships".into(), Value::Array(authorships));
    if let Some(location) = work.get("primary_location").filter(|v| v.is_object()) {
        let mut loc = Map::new();
        if let Some(url) = text(location, "landing_page_url") {
            loc.insert("landing_page_url".into(), json!(url));
        }
        if let Some(source) = location.get("source").filter(|v| v.is_object()) {
            let mut s = Map::new();
            for key in ["display_name", "type", "host_organization_name"] {
                if let Some(v) = text(source, key) {
                    s.insert(key.into(), json!(v));
                }
            }
            loc.insert("source".into(), Value::Object(s));
        }
        out.insert("primary_location".into(), Value::Object(loc));
    }
    Value::Object(out)
}

/// The fields of a work that the lookup of a free PDF asks for.
const OPEN_SELECT: &str = "id,doi,open_access,best_oa_location,locations";

/// A link to a free PDF of a work, if OpenAlex lists one: the `pdf_url` of its best open-access
/// location, else of another open one, else the `oa_url` of its `open_access` (which may be the
/// page of the article, not the PDF: a download checks).
pub fn open_pdf_url(work: &Value) -> Option<String> {
    let pdf = |location: &Value| text(location, "pdf_url").map(str::to_string);
    work.get("best_oa_location")
        .and_then(pdf)
        .or_else(|| {
            work.get("locations")
                .and_then(Value::as_array)?
                .iter()
                .filter(|l| l.get("is_oa").and_then(Value::as_bool).unwrap_or(false))
                .find_map(pdf)
        })
        .or_else(|| {
            work.get("open_access")
                .and_then(|o| text(o, "oa_url"))
                .map(str::to_string)
        })
}

// the queries
// -----------

/// The first `length` characters of a text, up to a space (a word is not cut).
fn truncate(text: &str, length: usize) -> &str {
    match text.char_indices().nth(length) {
        None => text,
        Some((end, _)) => match text[..end].rfind(char::is_whitespace) {
            Some(space) => text[..space].trim_end(),
            None => &text[..end],
        },
    }
}

impl Fetcher {
    /// Wait until the last request to OpenAlex is `INTERVAL` ago.
    fn openalex_throttle(&self) {
        let mut last = self
            .openalex_last
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(when) = *last {
            let since = when.elapsed();
            if since < INTERVAL {
                std::thread::sleep(INTERVAL - since);
            }
        }
        *last = Some(Instant::now());
    }

    /// The works that a semantic search finds (at most `rows`, the closest first), trimmed to
    /// what is read. The cache has them by the query: an answer of at least `rows` works serves.
    pub fn openalex_search(
        &self,
        query: &str,
        access: &OpenAlex,
        rows: usize,
    ) -> Result<Vec<Value>> {
        let query = truncate(query, QUERY_LENGTH);
        let key = hashed_key(&dumps_compact(
            &json!({"search.semantic": query, "select": SELECT}),
        ));
        if let Cached::Value(Value::Object(cached)) = self.cache.get("openalex", &key, now(), false)
            && cached.get("per-page").and_then(Value::as_u64) >= Some(rows as u64)
            && let Some(Value::Array(works)) = cached.get("works")
        {
            return Ok(works.iter().take(rows).cloned().collect());
        }
        let mut params: Vec<(&str, String)> = vec![
            ("search.semantic", query.to_string()),
            ("per-page", rows.to_string()),
            ("select", SELECT.to_string()),
        ];
        log::debug(&format!("openalex search: {}", with_params(API, &params)));
        if let Some(mailto) = access.mailto.as_deref().filter(|m| !m.is_empty()) {
            params.push(("mailto", mailto.to_string()));
        }
        if let Some(api_key) = access.api_key.as_deref().filter(|k| !k.is_empty()) {
            params.push(("api_key", api_key.to_string()));
        }
        self.openalex_throttle();
        let response = self.get(&with_params(API, &params), None)?;
        if response.status >= 400 {
            // the answer says what is wrong (the budget of the day, a key that is not active)
            let body = loads(&response.text()).ok();
            let said = body.as_ref().and_then(|b| {
                ["message", "error"]
                    .iter()
                    .find_map(|k| b.get(k).and_then(Value::as_str))
            });
            return Err(failed(match said {
                Some(said) => format!("OpenAlex: HTTP {}: {said}", response.status),
                None => format!(
                    "OpenAlex: HTTP {} {}",
                    response.status,
                    py_repr_str(&response.reason)
                ),
            }));
        }
        let body = loads(&response.text())
            .map_err(|e| FetchError::new(ErrorKind::Value, format!("OpenAlex: {}", e.message)))?;
        let works: Vec<Value> = body
            .get("results")
            .and_then(Value::as_array)
            .ok_or_else(|| failed("OpenAlex: no 'results' in the answer"))?
            .iter()
            .map(trim)
            .collect();
        self.cache
            .put_value("openalex", &key, json!({"per-page": rows, "works": works}));
        Ok(works)
    }

    /// The link to a free PDF of the work with this DOI, if OpenAlex has the work and lists one
    /// ([`open_pdf_url`]). One request; a DOI that OpenAlex does not know has none.
    pub fn openalex_free_pdf(&self, doi: &str, access: &OpenAlex) -> Result<Option<String>> {
        let mut params: Vec<(&str, String)> = vec![("select", OPEN_SELECT.to_string())];
        if let Some(mailto) = access.mailto.as_deref().filter(|m| !m.is_empty()) {
            params.push(("mailto", mailto.to_string()));
        }
        if let Some(api_key) = access.api_key.as_deref().filter(|k| !k.is_empty()) {
            params.push(("api_key", api_key.to_string()));
        }
        let url = with_params(
            &format!("{API}/doi:{}", super::http::quote(doi, "/")),
            &params,
        );
        log::debug(&format!("openalex work: {url}"));
        self.openalex_throttle();
        let response = self.get(&url, None)?;
        if response.status == 404 {
            return Ok(None);
        }
        if response.status >= 400 {
            return Err(failed(format!(
                "OpenAlex: HTTP {} {}",
                response.status,
                py_repr_str(&response.reason)
            )));
        }
        let work = loads(&response.text())
            .map_err(|e| FetchError::new(ErrorKind::Value, format!("OpenAlex: {}", e.message)))?;
        Ok(open_pdf_url(&work))
    }

    /// The BibTeX of a work: that of its DOI, as papers makes it (crossref, doi.org, arXiv), or,
    /// for a work without DOI or whose DOI no registry has metadata of, made of the work.
    pub fn bibtex_of_work(&self, work: &Value) -> Result<String> {
        if let Some(doi) = work_doi(work) {
            match self.fetch_bibtex_by_doi(&doi) {
                Ok(bibtex) => return Ok(bibtex),
                Err(error) if error.is_request_error() => log::info(&format!(
                    "{doi}: no metadata ({error}), the entry is made of the OpenAlex record"
                )),
                Err(error) => return Err(error),
            }
        }
        work_bibtex(work)
    }

    /// `fetch_bibtex_by_fulltext_scholar` with OpenAlex: the best match of a search of the
    /// query, as BibTeX. With `text` (the text of the PDF, of which the query is a part), the
    /// best of the hits whose title is in the text; if none has, the best one.
    pub fn fetch_bibtex_by_fulltext_openalex(
        &self,
        query: &str,
        access: &OpenAlex,
        rows: usize,
        text: Option<&str>,
    ) -> Result<String> {
        let works = self.openalex_search(query, access, rows)?;
        if works.is_empty() {
            return Err(FetchError::new(
                ErrorKind::Value,
                "openalex fulltext: no results",
            ));
        }
        let ranked = rank_works(query, &works)?;
        let (score, best) = ranked[0];
        // the hits with their title in the text, the published works before the copies of them
        let chosen = text
            .and_then(|txt| {
                let accepted: Vec<&(f64, &Value)> = ranked
                    .iter()
                    .filter(|(_, w)| work_title(w).is_some_and(|t| title_in_text_strict(t, txt)))
                    .collect();
                accepted
                    .iter()
                    .find(|(_, w)| !is_copy(w))
                    .or(accepted.first())
                    .map(|hit| **hit)
            })
            .unwrap_or((score, best));
        log::debug(&format!("score: {}", chosen.0));
        self.bibtex_of_work(chosen.1).map(|b| b.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_as_given_and_family() {
        let pair = |g: &str, f: &str| (g.to_string(), f.to_string());
        assert_eq!(
            split_name("Alexander Robinson"),
            pair("Alexander", "Robinson")
        );
        assert_eq!(split_name("Jason E. Box"), pair("Jason E.", "Box"));
        assert_eq!(split_name("Dirk van As"), pair("Dirk", "van As"));
        assert_eq!(
            split_name("Ludwig van der Waals"),
            pair("Ludwig", "van der Waals")
        );
        assert_eq!(split_name("Mahé  Perrette"), pair("Mahé", "Perrette"));
        assert_eq!(split_name("Perrette, Mahé"), pair("Mahé", "Perrette"));
        assert_eq!(
            split_name("Marie\u{2010}Luise Kapsch"),
            pair("Marie-Luise", "Kapsch")
        );
        // a name of one word is not split
        assert_eq!(split_name("IPCC"), pair("IPCC", ""));
        // the particle is not a name by itself
        assert_eq!(split_name("van Angelen"), pair("van", "Angelen"));
    }

    #[test]
    fn types_as_crossref_names_them() {
        let kind = |w: Value| crossref_type(&w);
        assert_eq!(
            kind(json!({"type_crossref": "book-chapter", "type": "article"})),
            "book-chapter"
        );
        assert_eq!(kind(json!({"type": "article"})), "journal-article");
        let source =
            |t: &str| json!({"type": "article", "primary_location": {"source": {"type": t}}});
        assert_eq!(kind(source("journal")), "journal-article");
        assert_eq!(kind(source("conference")), "proceedings-article");
        assert_eq!(kind(source("repository")), "posted-content");
        assert_eq!(kind(json!({"type": "preprint"})), "posted-content");
        assert_eq!(kind(json!({"type": "dissertation"})), "thesis");
        assert_eq!(kind(json!({"type": "book"})), "book");
        assert_eq!(kind(json!({"type": "dataset"})), "dataset");
    }

    #[test]
    fn dois_without_their_url() {
        let doi = |d: &str| work_doi(&json!({"doi": d}));
        assert_eq!(doi("https://doi.org/10.1/x"), Some("10.1/x".to_string()));
        assert_eq!(doi("10.1/x"), Some("10.1/x".to_string()));
        assert_eq!(doi(""), None);
        assert_eq!(work_doi(&json!({"doi": null})), None);
    }

    #[test]
    fn the_abstract_from_its_inverted_index() {
        let work = json!({"abstract_inverted_index": {"is": [1], "This": [0], "it": [2, 3]}});
        assert_eq!(work_abstract(&work).as_deref(), Some("This is it it"));
        assert_eq!(
            work_abstract(&json!({"abstract_inverted_index": null})),
            None
        );
    }

    #[test]
    fn the_free_pdf_of_a_work() {
        let url = |w: Value| open_pdf_url(&w);
        let best = json!({"best_oa_location": {"pdf_url": "https://a.org/best.pdf"},
            "open_access": {"oa_url": "https://a.org/page"}});
        assert_eq!(url(best).as_deref(), Some("https://a.org/best.pdf"));
        // another open location, then the open-access link
        let other = json!({"best_oa_location": {"pdf_url": null}, "locations": [
            {"is_oa": false, "pdf_url": "https://a.org/closed.pdf"},
            {"is_oa": true, "pdf_url": "https://a.org/open.pdf"}]});
        assert_eq!(url(other).as_deref(), Some("https://a.org/open.pdf"));
        let landing =
            json!({"best_oa_location": null, "open_access": {"oa_url": "https://a.org/page"}});
        assert_eq!(url(landing).as_deref(), Some("https://a.org/page"));
        assert_eq!(
            url(json!({"open_access": {"oa_url": null}, "locations": []})),
            None
        );
    }

    #[test]
    fn queries_are_cut_at_a_word() {
        assert_eq!(truncate("one two three", 100), "one two three");
        assert_eq!(truncate("one two three", 9), "one two");
        assert_eq!(truncate("onetwothree", 5), "onetw");
        assert_eq!(truncate("é é é", 3), "é");
    }
}

// the locations of a work (`papers file fetch`)
// ---------------------------------------------

/// The fields of a work that [`Fetcher::work_locations`] asks for (`WORK_SELECT`).
pub const WORK_SELECT: &str = "id,doi,open_access,best_oa_location,locations";

/// How long the locations of a work are kept (seconds): open access changes (an embargo ends), so
/// they are asked again after a month.
pub const WORK_MAX_AGE: f64 = 30.0 * 24.0 * 3600.0;

/// A place where a work can be read, as kept of a location of OpenAlex (`_trim_location`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Location {
    pub pdf_url: Option<String>,
    pub landing_page_url: Option<String>,
    /// `publishedVersion`, `acceptedVersion`, `submittedVersion`
    pub version: Option<String>,
    pub is_oa: bool,
    /// the name of the source
    pub source: Option<String>,
}

impl Location {
    /// A location as OpenAlex gives it, or as the cache has it.
    fn of(value: &Value) -> Option<Location> {
        let map = value.as_object()?;
        let string = |key: &str| map.get(key).and_then(Value::as_str).map(str::to_string);
        let truthy = match map.get("is_oa") {
            None | Some(Value::Null) | Some(Value::Bool(false)) => false,
            Some(Value::Number(n)) => n.as_f64() != Some(0.0),
            Some(Value::String(s)) => !s.is_empty(),
            Some(Value::Array(a)) => !a.is_empty(),
            Some(Value::Object(o)) => !o.is_empty(),
            Some(Value::Bool(true)) => true,
        };
        let source = match map.get("source") {
            Some(Value::Object(source)) => source
                .get("display_name")
                .and_then(Value::as_str)
                .map(str::to_string),
            // as the cache keeps it
            Some(Value::String(name)) => Some(name.clone()),
            _ => None,
        };
        Some(Location {
            pdf_url: string("pdf_url"),
            landing_page_url: string("landing_page_url"),
            version: string("version"),
            is_oa: truthy,
            source,
        })
    }

    fn to_value(&self) -> Value {
        json!({
            "pdf_url": self.pdf_url,
            "landing_page_url": self.landing_page_url,
            "version": self.version,
            "is_oa": self.is_oa,
            "source": self.source,
        })
    }
}

/// The error of an answer of OpenAlex (`_http_error`): its status, what it says, and for the limit
/// of the day without a key, how to get more.
fn work_http_error(status: u16, body: &str, access: &OpenAlex) -> FetchError {
    let said = loads(body).ok().and_then(|b| {
        ["message", "error"]
            .iter()
            .find_map(|k| b.get(k).and_then(Value::as_str).map(str::to_string))
    });
    let mut message = format!("OpenAlex: HTTP {status}");
    if let Some(said) = said.filter(|s| !s.is_empty()) {
        message.push_str(&format!(": {said}"));
    }
    if status == 429 && access.api_key.as_deref().is_none_or(str::is_empty) {
        message.push_str(" (without a free API key OpenAlex allows about 100 searches a day: set OPENALEX_API_KEY)");
    }
    failed(redact(&message, access))
}

/// A text without the key and the e-mail address (`_redact`).
fn redact(text: &str, access: &OpenAlex) -> String {
    let mut text = text.to_string();
    for secret in [&access.api_key, &access.mailto].into_iter().flatten() {
        if !secret.is_empty() {
            text = text.replace(secret.as_str(), "***");
        }
    }
    text
}

/// The locations of a work, the best open-access location first, then the others of `locations`
/// (those equal to it left out); `None` for an answer that is not a work.
fn work_locations_of(work: &Value) -> Option<Vec<Value>> {
    let work = work.as_object()?;
    let truthy = |v: &Value| match v {
        Value::Null | Value::Bool(false) => false,
        Value::Object(o) => !o.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::String(s) => !s.is_empty(),
        _ => true,
    };
    let mut locations: Vec<Value> = Vec::new();
    if let Some(best) = work.get("best_oa_location").filter(|b| truthy(b)) {
        locations.push(best.clone());
    }
    match work.get("locations") {
        None | Some(Value::Null) => {}
        Some(Value::Array(all)) => {
            for x in all {
                if truthy(x) && !locations.contains(x) {
                    locations.push(x.clone());
                }
            }
        }
        Some(other) if !truthy(other) => {}
        Some(_) => return None,
    }
    locations.iter().all(Value::is_object).then_some(locations)
}

impl Fetcher {
    /// The locations of the work of a DOI (`work_locations`: `GET /works/doi:<DOI>`, one
    /// request), the best open-access location first. A DOI that OpenAlex does not have is an
    /// error of kind `NotFound` (`WorkNotFound`), remembered for a day; the locations are kept in
    /// the cache (`openalex-work/`, by the DOI in lower case) for [`WORK_MAX_AGE`].
    pub fn work_locations(&self, doi: &str, access: &OpenAlex) -> Result<Vec<Location>> {
        const NAME: &str = "openalex-work";
        let key = doi.to_lowercase();
        let time = now();
        match self
            .cache
            .get_within(NAME, &key, time, true, Some(WORK_MAX_AGE))
        {
            Cached::Value(Value::Array(locations)) => {
                return Ok(locations.iter().filter_map(Location::of).collect());
            }
            Cached::NotFound(when) => {
                return Err(FetchError::new(
                    ErrorKind::NotFound,
                    format!(
                        "{doi}: not found (cached until {})",
                        super::cache::ctime(when + super::cache::NOT_FOUND_TTL)
                    ),
                ));
            }
            _ => {}
        }
        let mut params: Vec<(&str, String)> = vec![("select", WORK_SELECT.to_string())];
        if let Some(mailto) = access.mailto.as_deref().filter(|m| !m.is_empty()) {
            params.push(("mailto", mailto.to_string()));
        }
        if let Some(api_key) = access.api_key.as_deref().filter(|k| !k.is_empty()) {
            params.push(("api_key", api_key.to_string()));
        }
        log::debug(&format!("openalex work: {doi}"));
        self.openalex_throttle();
        let url = with_params(
            &format!("{API}/doi:{}", super::http::quote(doi, "/")),
            &params,
        );
        let response = self
            .get(&url, None)
            .map_err(|e| failed(redact(&format!("OpenAlex: {}", e.message), access)))?;
        if response.status == 404 {
            self.cache.put_not_found(NAME, &key, now());
            return Err(FetchError::new(
                ErrorKind::NotFound,
                format!("{doi}: not found on OpenAlex"),
            ));
        }
        if response.status >= 400 {
            return Err(work_http_error(response.status, &response.text(), access));
        }
        let unexpected = || failed("OpenAlex: unexpected answer");
        let work = loads(&response.text()).map_err(|_| unexpected())?;
        let locations = work_locations_of(&work).ok_or_else(unexpected)?;
        let locations: Vec<Location> = locations.iter().filter_map(Location::of).collect();
        let stored = Value::Array(locations.iter().map(Location::to_value).collect());
        self.cache.put_value_at(NAME, &key, stored, now());
        Ok(locations)
    }
}

#[cfg(test)]
mod work_tests {
    use super::*;

    #[test]
    fn the_best_location_first_without_its_copy() {
        let best = json!({"pdf_url": "https://a.org/b.pdf", "version": "publishedVersion", "is_oa": true,
            "landing_page_url": null, "source": {"display_name": "A"}});
        let other =
            json!({"pdf_url": null, "version": "acceptedVersion", "is_oa": 1, "source": null});
        let work = json!({"best_oa_location": best, "locations": [other, best, null]});
        let locations: Vec<Location> = work_locations_of(&work)
            .unwrap()
            .iter()
            .filter_map(Location::of)
            .collect();
        assert_eq!(locations.len(), 2);
        assert_eq!(locations[0].source.as_deref(), Some("A"));
        assert!(locations[1].is_oa);
        assert_eq!(locations[1].version.as_deref(), Some("acceptedVersion"));
        // as the cache keeps it
        assert_eq!(
            Location::of(&locations[0].to_value()).as_ref(),
            Some(&locations[0])
        );
        assert!(work_locations_of(&json!([1])).is_none());
        assert!(work_locations_of(&json!({"locations": ["x"]})).is_none());
        assert_eq!(work_locations_of(&json!({"locations": null})), Some(vec![]));
    }

    #[test]
    fn errors_without_the_key() {
        let access = OpenAlex {
            mailto: Some("me@x.org".into()),
            api_key: None,
        };
        let error = work_http_error(429, r#"{"error": "slow down me@x.org"}"#, &access);
        assert_eq!(
            error.message,
            "OpenAlex: HTTP 429: slow down *** (without a free API key OpenAlex allows about 100 searches a day: set OPENALEX_API_KEY)"
        );
        assert_eq!(
            work_http_error(500, "", &access).message,
            "OpenAlex: HTTP 500"
        );
    }
}
