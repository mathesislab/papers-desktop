//! A full-text search on OpenAlex (<https://api.openalex.org>), the open alternative to Google
//! Scholar (`papers.openalex`, papers v3 faeaf4d, #205: the option `--openalex` of `add`,
//! `extract` and `fetch`).
//!
//! OpenAlex is an open catalogue (CC0 data) with an API, where Google Scholar is scraped (and
//! often blocks it). The search takes the text query that papers makes of a PDF
//! (`extract::doi::query_text`) and works like the crossref search:
//!
//! 1. `GET /works?search.semantic=QUERY`: the works whose title and abstract are closest in
//!    meaning to the query. The plain `search=` needs every word of the query, so a query of 200
//!    words finds nothing. A request without a paid key may have at most 1500 characters (HTTP 400
//!    above): the query is cut there at a word ([`truncate`]). A semantic search is limited to one
//!    request per second ([`INTERVAL`]).
//! 2. Each work is read as a crossref record ([`work_record`]), so that the score of papers
//!    applies ([`super::crossref::crossref_score`]: family names, title and abstract with the
//!    text), and the works are ranked by it (the first of equal scores first).
//! 3. With a check of the title (`accept`: the text of the PDF has it, as for every result of
//!    papers), the first work in score order that passes is taken; the copies of a work
//!    ([`is_copy`]: a preprint, the abstract of a conference... that OpenAlex has with the title
//!    of the paper) come after the published works. If none passes, the best one is returned, and
//!    the caller's own check rejects it.
//! 4. A work with a DOI gives its BibTeX the usual way (`Fetcher::fetch_bibtex_by_doi`:
//!    crossref, doi.org, arXiv). A work without DOI, or whose DOI has no metadata, gives an entry
//!    made of the OpenAlex record, as papers makes it of a crossref record (the key is the DOI,
//!    else the OpenAlex id: [`work_bibtex`]).
//!
//! Without a key, OpenAlex allows about 100 searches a day (shared by the network); a free key
//! (<https://openalex.org/settings/api>) has ten times as many. papers reads the key from the
//! environment variable `OPENALEX_API_KEY`, and the email address that identifies the user (the
//! "polite pool") from `PAPERS_MAILTO`, when they are set: the command line does
//! ([`OpenAlex::from_env`]); the library takes them as a parameter ([`OpenAlex`]), so that a
//! program gives its own (Papers Desktop: those of its settings). They are never in a message
//! (`OpenAlexError`). The answers are cached (`openalex/` in the cache
//! directory), by the query, trimmed to what is read ([`trim`]).
//!
//! The wait between two requests is kept by each [`Fetcher`] (papers: by the process; the command
//! line has one fetcher).

use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use super::cache::{Cached, hashed_key, now};
use super::crossref::{crossref_score, crossref_to_entry};
use super::http::{Response, quote_plus, with_params};
use super::{ErrorKind, FetchError, Fetcher};
use crate::bib::{Bib, Block};
use crate::config::json::{
    dumps_compact, loads, py_float_repr, py_repr, py_repr_str, py_str, py_truthy,
};
use crate::log;

type Result<T> = std::result::Result<T, FetchError>;

pub const API: &str = "https://api.openalex.org/works";

/// The fields of a work that are asked for.
pub const SELECT: &str = "id,doi,title,display_name,type,type_crossref,publication_year,biblio,primary_location,authorships,abstract_inverted_index";

/// The works scored (crossref: 5; a search costs the same up to 50).
pub const ROWS: usize = 10;

/// The longest query, in characters.
pub const QUERY_LENGTH: usize = 1500;

/// The time between two requests (the limit of a semantic search is one a second).
pub const INTERVAL: Duration = Duration::from_millis(1100);

/// The environment variable of the API key.
pub const API_KEY_ENV: &str = "OPENALEX_API_KEY";
/// The environment variable of the email address of the user.
pub const MAILTO_ENV: &str = "PAPERS_MAILTO";

/// How the requests identify the user to OpenAlex.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenAlex {
    /// `mailto`: the email address of the user, for the polite pool.
    pub mailto: Option<String>,
    /// `api_key`: a key of OpenAlex (free), for more searches a day.
    pub api_key: Option<String>,
}

impl OpenAlex {
    /// As papers has them: `PAPERS_MAILTO` and `OPENALEX_API_KEY`, when they are set (not empty).
    pub fn from_env() -> OpenAlex {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        OpenAlex {
            mailto: var(MAILTO_ENV),
            api_key: var(API_KEY_ENV),
        }
    }

    fn mailto(&self) -> Option<&str> {
        self.mailto.as_deref().filter(|m| !m.is_empty())
    }

    fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref().filter(|k| !k.is_empty())
    }

    /// The parameters of a request: `mailto` and `api_key`, those that are set.
    fn params(&self) -> Vec<(&'static str, String)> {
        let mut params = Vec::new();
        if let Some(mailto) = self.mailto() {
            params.push(("mailto", mailto.to_string()));
        }
        if let Some(api_key) = self.api_key() {
            params.push(("api_key", api_key.to_string()));
        }
        params
    }

    /// `_redact`: the text without the key and the email address (as they are, and as a URL
    /// has them).
    fn redact(&self, text: &str) -> String {
        let mut text = text.to_string();
        for secret in [self.api_key(), self.mailto()].into_iter().flatten() {
            text = text
                .replace(secret, "***")
                .replace(&quote_plus(secret), "***");
        }
        text
    }
}

/// `OpenAlexError`: an answer of OpenAlex that is an error, or no answer (the message without the
/// key). An `OSError` in papers, as the errors of the network: a clean exit of the commands.
fn openalex_error(message: impl Into<String>) -> FetchError {
    FetchError::new(ErrorKind::Network, message)
}

/// `_http_error`: the status, what OpenAlex says of it (`message`, else `error`), and for a
/// request without key that is over the limit of the day, the key.
fn http_error(response: &Response, access: &OpenAlex) -> FetchError {
    let body = loads(&response.text()).ok();
    let said = body
        .as_ref()
        .and_then(|b| ["message", "error"].iter().find_map(|k| b.get(k)?.as_str()))
        .unwrap_or("");
    let mut message = format!("OpenAlex: HTTP {}", response.status);
    if !said.is_empty() {
        message.push_str(&format!(": {said}"));
    }
    if response.status == 429 && access.api_key().is_none() {
        message.push_str(&format!(
            " (without a free API key OpenAlex allows about 100 searches a day: set {API_KEY_ENV})"
        ));
    }
    openalex_error(access.redact(&message))
}

// the query
// ---------

/// The first `length` characters of a text, up to a space (a word is not cut).
pub fn truncate(text: &str, length: usize) -> &str {
    let Some((end, next)) = text.char_indices().nth(length) else {
        return text;
    };
    let head = &text[..end];
    if next.is_whitespace() {
        return head.trim_end();
    }
    match head.rfind([' ', '\n', '\t']) {
        Some(cut) if cut > 0 => head[..cut].trim_end(),
        _ => head,
    }
}

/// The wait before a request, `INTERVAL` after the last one.
fn wait(last: Option<Instant>, now: Instant) -> Option<Duration> {
    INTERVAL
        .checked_sub(now.duration_since(last?))
        .filter(|w| !w.is_zero())
}

// the works
// ---------

/// A value that Python takes as true, as a string (what papers reads of a field).
fn truthy_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

/// A value that Python takes as true.
fn truthy<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|v| py_truthy(v))
}

/// The work as it is kept and read (`_trim`): the abstract as text, no affiliations and other
/// pages of the record. `None` for what is not a work (an unexpected answer).
pub fn trim(work: &Value) -> Option<Value> {
    let object = work.as_object()?;
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
    ] {
        if let Some(v) = object.get(key).filter(|v| !v.is_null()) {
            out.insert(key.into(), v.clone());
        }
    }
    let abstract_ = abstract_text(object.get("abstract_inverted_index"))?;
    if !abstract_.is_empty() {
        out.insert("abstract".into(), json!(abstract_));
    }
    let mut authorships = Vec::new();
    for a in iterate(object.get("authorships"))? {
        a.as_object()?;
        let author = match truthy(a, "author") {
            Some(author) => Some(author.as_object()?),
            None => None,
        };
        let mut kept = Map::new();
        if let Some(name) = truthy_str(a, "raw_author_name") {
            kept.insert("raw_author_name".into(), json!(name));
        }
        if let Some(name) = author
            .and_then(|a| a.get("display_name")?.as_str())
            .filter(|s| !s.is_empty())
        {
            kept.insert("display_name".into(), json!(name));
        }
        authorships.push(Value::Object(kept));
    }
    out.insert("authorships".into(), Value::Array(authorships));
    let empty = Value::Object(Map::new());
    let location = match truthy(work, "primary_location") {
        Some(location) => location.as_object().map(|_| location)?,
        None => &empty,
    };
    let source = match truthy(location, "source") {
        Some(source) => source.as_object().map(|_| source)?,
        None => &empty,
    };
    let field = |v: &Value, key: &str| v.get(key).cloned().unwrap_or(Value::Null);
    out.insert(
        "primary_location".into(),
        json!({
            "landing_page_url": field(location, "landing_page_url"),
            "source": {
                "display_name": field(source, "display_name"),
                "type": field(source, "type"),
                "host_organization_name": field(source, "host_organization_name"),
            },
        }),
    );
    Some(Value::Object(out))
}

/// The items of a list of an answer (`x or []`); `None` for what is not one.
fn iterate(value: Option<&Value>) -> Option<&[Value]> {
    match value {
        Some(Value::Array(items)) => Some(items),
        Some(v) if py_truthy(v) => None,
        _ => Some(&[]),
    }
}

/// `_abstract`: the abstract from its inverted index (word to positions); empty without one,
/// `None` for an index that is not one.
fn abstract_text(index: Option<&Value>) -> Option<String> {
    let Some(Value::Object(index)) = index else {
        return Some(String::new());
    };
    let mut words: Vec<(i64, &str)> = Vec::new();
    for (word, positions) in index {
        for position in positions.as_array()? {
            words.push((position.as_i64()?, word));
        }
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

/// The DOI of a work, without its URL.
pub fn work_doi(work: &Value) -> Option<String> {
    let doi = work.get("doi").and_then(Value::as_str).unwrap_or("").trim();
    let lower = doi.to_ascii_lowercase();
    let prefix = [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "http://dx.doi.org/",
        "doi:",
    ]
    .iter()
    .find(|p| lower.starts_with(*p))
    .map_or(0, |p| p.len());
    let doi = &doi[prefix..];
    (!doi.is_empty()).then(|| doi.to_string())
}

/// The title of a work (its `display_name` without one).
pub fn work_title(work: &Value) -> &str {
    truthy_str(work, "title")
        .or_else(|| truthy_str(work, "display_name"))
        .unwrap_or("")
        .trim()
}

/// A work that is a copy of another, as OpenAlex has them with the same title: a preprint, the
/// abstract of a conference, a review, a data set...
pub fn is_copy(work: &Value) -> bool {
    matches!(
        work.get("type").and_then(Value::as_str),
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

const PARTICLES: [&str; 19] = [
    "van", "von", "der", "den", "de", "del", "della", "di", "da", "dos", "du", "la", "le", "ten",
    "ter", "bin", "ibn", "al", "el",
];

/// (given, family) of a name: `Family, Given` as it is written, else the family name is the last
/// word with the particles before it; a name of one word (an organization) is a given name.
pub fn split_name(name: &str) -> (String, String) {
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

/// `_crossref_type`: the type of a work as crossref names it.
fn crossref_type(work: &Value) -> String {
    if let Some(t) = truthy(work, "type_crossref") {
        return py_str(t);
    }
    let kind = truthy_str(work, "type").unwrap_or("");
    let source_type = work
        .pointer("/primary_location/source/type")
        .and_then(Value::as_str);
    match kind {
        "article" | "review" | "letter" | "editorial" | "erratum" => match source_type {
            None | Some("journal") => "journal-article",
            Some("conference") => "proceedings-article",
            Some(_) => "posted-content",
        },
        "preprint" => "posted-content",
        "dissertation" => "thesis",
        other => other,
    }
    .to_string()
}

/// The work as a crossref record (what `crossref_to_bibtex` and the score read).
pub fn work_record(work: &Value) -> Value {
    let empty = Value::Object(Map::new());
    let mut record = Map::new();
    record.insert("type".into(), json!(crossref_type(work)));
    let location = truthy(work, "primary_location").unwrap_or(&empty);
    let source = truthy(location, "source").unwrap_or(&empty);
    if let Some(doi) = work_doi(work) {
        record.insert("URL".into(), json!(format!("https://doi.org/{doi}")));
        record.insert("DOI".into(), json!(doi));
    } else {
        let url = truthy(location, "landing_page_url")
            .or_else(|| truthy(work, "id"))
            .cloned()
            .unwrap_or_else(|| json!(""));
        record.insert("URL".into(), url);
    }
    let title = work_title(work);
    if !title.is_empty() {
        record.insert("title".into(), json!([title]));
    }
    let mut authors = Vec::new();
    for authorship in work
        .get("authorships")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = truthy_str(authorship, "raw_author_name")
            .or_else(|| truthy_str(authorship, "display_name"));
        if let Some(name) = name {
            let (given, family) = split_name(name);
            let mut author = Map::new();
            if !given.is_empty() {
                author.insert("given".into(), json!(given));
            }
            if !family.is_empty() {
                author.insert("family".into(), json!(family));
            }
            authors.push(Value::Object(author));
        }
    }
    if !authors.is_empty() {
        record.insert("author".into(), Value::Array(authors));
    }
    if let Some(year) = work.get("publication_year").filter(|v| !v.is_null()) {
        record.insert("issued".into(), json!({"date-parts": [[year]]}));
    }
    if let Some(name) = truthy(source, "display_name") {
        record.insert("container-title".into(), json!([name]));
    }
    if let Some(name) = truthy(source, "host_organization_name") {
        record.insert("publisher".into(), name.clone());
    }
    let biblio = truthy(work, "biblio").unwrap_or(&empty);
    if let Some(volume) = truthy(biblio, "volume") {
        record.insert("volume".into(), volume.clone());
    }
    if let Some(issue) = truthy(biblio, "issue") {
        record.insert("issue".into(), issue.clone());
    }
    if let Some(first) = truthy(biblio, "first_page") {
        let page = match truthy(biblio, "last_page") {
            Some(last) if last != first => json!(format!("{}-{}", py_str(first), py_str(last))),
            _ => first.clone(),
        };
        record.insert("page".into(), page);
    }
    if let Some(abstract_) = truthy(work, "abstract") {
        record.insert("abstract".into(), abstract_.clone());
    }
    Value::Object(record)
}

/// The BibTeX made of a work alone: the key is its DOI, else its OpenAlex id (`W2741809807`).
pub fn work_bibtex(work: &Value) -> Result<String> {
    let mut entry = crossref_to_entry(&work_record(work))?;
    let id = work.get("id").and_then(Value::as_str).unwrap_or("");
    let key = id.rsplit('/').next().unwrap_or("");
    if work_doi(work).is_none() && !key.is_empty() {
        entry.key = key.to_string();
    }
    Ok(Bib {
        blocks: vec![Block::Entry(entry)],
    }
    .format())
}

/// A string field, trimmed, if it is not empty.
fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
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

// the search
// ----------

/// The works of an answer, trimmed; `None` if it is not an answer of a search.
fn answer_works(body: &Value) -> Option<Vec<Value>> {
    let results = body.as_object()?.get("results")?;
    match results {
        Value::Array(works) => works.iter().map(trim).collect(),
        // what Python iterates to nothing
        Value::Object(m) if m.is_empty() => Some(Vec::new()),
        Value::String(s) if s.is_empty() => Some(Vec::new()),
        _ => None,
    }
}

impl Fetcher {
    /// `_throttle`: wait until the last request to OpenAlex is `INTERVAL` ago.
    fn openalex_throttle(&self) {
        let mut last = self
            .openalex_last
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(wait) = wait(*last, Instant::now()) {
            std::thread::sleep(wait);
        }
        *last = Some(Instant::now());
    }

    /// `_search`: the works of a semantic search of the query (at most `rows`), trimmed
    /// ([`trim`]). The cache has them by the query and the number of works (not by the email
    /// address or the key, which do not change the answer); an error is not cached.
    pub fn openalex_search(
        &self,
        query: &str,
        rows: usize,
        access: &OpenAlex,
    ) -> Result<Vec<Value>> {
        let params = json!({"search.semantic": query, "per-page": rows, "select": SELECT});
        let key = hashed_key(&dumps_compact(&params));
        if let Cached::Value(Value::Array(works)) = self.cache.get("openalex", &key, now(), false) {
            return Ok(works);
        }
        let mut request: Vec<(&str, String)> = vec![
            ("per-page", rows.to_string()),
            ("search.semantic", query.to_string()),
            ("select", SELECT.to_string()),
        ];
        request.extend(access.params());
        log::debug(&format!("openalex search: {}", py_repr(&params)));
        self.openalex_throttle();
        let response = self
            .get(&with_params(API, &request), None)
            .map_err(|error| {
                openalex_error(access.redact(&format!("OpenAlex: {}", error.message)))
            })?;
        if response.status >= 400 {
            return Err(http_error(&response, access));
        }
        let works = loads(&response.text())
            .ok()
            .and_then(|body| answer_works(&body))
            .ok_or_else(|| openalex_error("OpenAlex: unexpected answer"))?;
        self.cache
            .put_value("openalex", &key, Value::Array(works.clone()));
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
            return Err(openalex_error(format!(
                "OpenAlex: HTTP {} {}",
                response.status,
                py_repr_str(&response.reason)
            )));
        }
        let work = loads(&response.text())
            .map_err(|e| FetchError::new(ErrorKind::Value, format!("OpenAlex: {}", e.message)))?;
        Ok(open_pdf_url(&work))
    }

    /// The BibTeX of the DOI of a work, as papers makes it (crossref, doi.org, arXiv), else made
    /// of the work (a work without DOI, or whose DOI has no metadata).
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

    /// The best match of an OpenAlex search of the text, as BibTeX (the query is the text cut at
    /// [`QUERY_LENGTH`] characters, the score is that of the whole text).
    ///
    /// `accept`: the check of a title (true if it is that of the paper of the text): the best
    /// work that passes is taken, the published ones before their copies.
    pub fn fetch_bibtex_by_fulltext_openalex(
        &self,
        txt: &str,
        access: &OpenAlex,
        accept: Option<&dyn Fn(&str) -> bool>,
        rows: usize,
    ) -> Result<String> {
        let query = truncate(txt, QUERY_LENGTH);
        let works = self.openalex_search(query, rows, access)?;
        if works.is_empty() {
            return Err(FetchError::new(
                ErrorKind::Value,
                "openalex fulltext: no results",
            ));
        }
        let scores = works
            .iter()
            .map(|work| crossref_score(txt, &work_record(work)))
            .collect::<Result<Vec<_>>>()?;
        // the first of equal scores first
        let mut order: Vec<usize> = (0..works.len()).collect();
        order.sort_by(|&a, &b| scores[b].0.total_cmp(&scores[a].0));
        let mut chosen = order[0];
        if let Some(accept) = accept {
            let accepted: Vec<usize> = order
                .iter()
                .copied()
                .filter(|&i| {
                    let title = work_title(&works[i]);
                    !title.is_empty() && accept(title)
                })
                .collect();
            if let Some(&first) = accepted.first() {
                chosen = accepted
                    .iter()
                    .copied()
                    .find(|&i| !is_copy(&works[i]))
                    .unwrap_or(first);
            }
        }
        let (score, float) = scores[chosen];
        log::debug(&format!(
            "score: {}",
            if float {
                py_float_repr(score)
            } else {
                "0".to_string()
            }
        ));
        self.bibtex_of_work(&works[chosen])
            .map(|b| b.trim().to_string())
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
    fn dois_without_their_url() {
        let doi = |d: &str| work_doi(&json!({"doi": d}));
        assert_eq!(doi("https://doi.org/10.1/x"), Some("10.1/x".to_string()));
        assert_eq!(doi("HTTP://DX.DOI.ORG/10.1/x"), Some("10.1/x".to_string()));
        assert_eq!(doi("doi:10.1/x"), Some("10.1/x".to_string()));
        assert_eq!(doi("10.1/x"), Some("10.1/x".to_string()));
        assert_eq!(doi(""), None);
        assert_eq!(doi("https://doi.org/"), None);
        assert_eq!(work_doi(&json!({"doi": null})), None);
    }

    #[test]
    fn the_abstract_from_its_inverted_index() {
        let index = json!({"is": [1], "This": [0], "it": [2, 3]});
        assert_eq!(
            abstract_text(Some(&index)).as_deref(),
            Some("This is it it")
        );
        assert_eq!(abstract_text(None).as_deref(), Some(""));
        assert_eq!(abstract_text(Some(&Value::Null)).as_deref(), Some(""));
        // not an index: an unexpected answer
        assert_eq!(abstract_text(Some(&json!({"is": 1}))), None);
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
        assert_eq!(truncate("one two three", 7), "one two"); // the word ends there
        assert_eq!(truncate("onetwothree", 5), "onetw");
        assert_eq!(truncate("é é é", 2), "é");
        assert_eq!(truncate(" onetwo", 4), " one"); // a space at the start does not count
    }

    #[test]
    fn one_request_a_second() {
        let start = Instant::now();
        // the first request does not wait
        assert_eq!(wait(None, start), None);
        // 0.3 s later
        let later = start + Duration::from_millis(300);
        assert_eq!(
            wait(Some(start), later),
            Some(INTERVAL - Duration::from_millis(300))
        );
        // long after
        assert_eq!(wait(Some(start), start + Duration::from_secs(5)), None);
        assert!(INTERVAL >= Duration::from_secs(1));
    }

    #[test]
    fn the_key_and_the_address_are_not_in_a_message() {
        let access = OpenAlex {
            mailto: Some("me@example.org".into()),
            api_key: Some("secret".into()),
        };
        assert_eq!(
            access.redact("url: /works?api_key=secret&mailto=me@example.org"),
            "url: /works?api_key=***&mailto=***"
        );
        assert_eq!(access.redact("mailto=me%40example.org"), "mailto=***");
        assert_eq!(OpenAlex::default().redact("api_key="), "api_key=");
    }
}
