//! The tests of papers-cli about the answers of the network, ported (`test_extract_unit.py`:
//! `TestParseDoiFromInfoFields`, `TestCrossrefToBibtex`, `TestExtractTxtMetadataCandidates`,
//! `TestDoiRequests`; `test_fetch_cmds.py`: the errors of `add --doi`), as of papers 3.0.0b7.dev49
//! (ed8b4dc, issue 189). The answers are made here: no test asks the network. The generated cases
//! (`fetch.rs`, with `gen_extract_expected.py`) check more, against the reference itself.

use std::fs;
use std::sync::{Arc, Mutex};

use papers_rs::command::add::{AddRequest, add};
use papers_rs::command::queries::Queries;
use papers_rs::command::{Options as SessionOptions, Session};
use papers_rs::config::{Config, MachineConfig};
use papers_rs::extract::doi::{doi_from_info_fields, doi_from_xmp, parse_dois};
use papers_rs::extract::{Options, extract_txt_metadata};
use papers_rs::fetch::crossref::crossref_to_bibtex;
use papers_rs::fetch::http::{Response, Transport};
use papers_rs::fetch::{ErrorKind, Fetcher};
use papers_rs::merge::{OnDuplicate, Resolver};
use serde_json::{Map, Value, json};

// the answers
// -----------

/// One answer to a request of this URL: with an `Accept` header (content negotiation) or without.
struct Answer {
    url: &'static str,
    negotiated: bool,
    result: Result<Response, String>,
}

struct Mock {
    answers: Vec<Answer>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl Transport for Mock {
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Response, String> {
        self.calls.lock().unwrap().push(url.to_string());
        self.answers
            .iter()
            .find(|a| a.url == url && a.negotiated == accept.is_some())
            .map(|a| a.result.clone())
            .unwrap_or_else(|| Err(format!("no answer for {url}")))
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "",
    }
}

fn answer(
    url: &'static str,
    negotiated: bool,
    status: u16,
    content_type: &str,
    body: &str,
    final_url: &str,
) -> Answer {
    Answer {
        url,
        negotiated,
        result: Ok(Response {
            status,
            reason: reason(status).to_string(),
            content_type: content_type.to_string(),
            body: body.as_bytes().to_vec(),
            url: final_url.to_string(),
            retry_after: None,
        }),
    }
}

fn fetcher(answers: Vec<Answer>) -> (Fetcher, Arc<Mutex<Vec<String>>>, tempfile::TempDir) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let cache = tempfile::tempdir().unwrap();
    let fetcher = Fetcher::with_transport(
        Box::new(Mock {
            answers,
            calls: calls.clone(),
        }),
        cache.path().to_str().unwrap(),
    );
    (fetcher, calls, cache)
}

const CSL: &str = "application/vnd.citationstyles.csl+json";

// TestParseDoiFromInfoFields
// --------------------------

#[test]
fn the_doi_of_the_info_fields_is_cleaned_as_in_the_text() {
    let subject = "Journal X. doi:10.1000/ABC.123.";
    assert_eq!(
        doi_from_info_fields(subject, "", "").as_deref(),
        Some("10.1000/abc.123")
    );
    assert_eq!(parse_dois(subject, "").unwrap(), ["10.1000/abc.123"]);
    // the DOI of the XMP is kept as it is
    assert_eq!(
        doi_from_xmp("<prism:doi>10.1000/ABC.123.</prism:doi>").as_deref(),
        Some("10.1000/ABC.123.")
    );
}

#[test]
fn a_doi_too_short_is_passed_over_for_the_next_field() {
    assert_eq!(
        doi_from_info_fields("doi:10.1000/.", "", "doi:10.1000/Title-1").as_deref(),
        Some("10.1000/title-1")
    );
    assert_eq!(doi_from_info_fields("doi:10.1000/.", "", ""), None);
}

// TestCrossrefToBibtex
// --------------------

fn base() -> Value {
    json!({"type": "journal-article", "DOI": "10.1/x", "title": ["T"],
        "author": [{"family": "S", "given": "J"}], "issued": {"date-parts": [[2000]]},
        "container-title": ["J"]})
}

/// `{**BASE, **changes}`
fn record(changes: Value) -> Value {
    let mut record = base();
    for (k, v) in changes.as_object().unwrap() {
        record[k] = v.clone();
    }
    record
}

fn bibtex(changes: Value) -> String {
    crossref_to_bibtex(&record(changes)).unwrap()
}

#[test]
fn an_empty_title_and_container_title() {
    let bib = bibtex(json!({"title": [], "container-title": []}));
    assert!(!bib.contains("title"), "{bib}");
    assert!(!bib.contains("journal"), "{bib}");
    assert!(bib.contains("year = {2000}"), "{bib}");
    assert!(!bibtex(json!({"type": "book", "ISBN": []})).contains("isbn"));
}

#[test]
fn an_institution_that_is_a_list() {
    assert!(
        bibtex(json!({"type": "report", "institution": [{"name": "A"}]}))
            .contains("institution = {A}")
    );
    assert!(
        bibtex(json!({"type": "thesis", "institution": [{"name": "A"}]})).contains("school = {A}")
    );
    assert!(
        bibtex(json!({"type": "report", "institution": {"name": "A"}}))
            .contains("institution = {A}")
    );
    assert!(!bibtex(json!({"type": "report", "institution": []})).contains("institution"));
}

#[test]
fn a_date_without_year() {
    for issued in [
        json!({"date-parts": [[null]]}),
        json!(null),
        json!({"date-parts": []}),
    ] {
        assert!(
            bibtex(json!({ "issued": issued })).contains("year = {0000}"),
            "{issued}"
        );
    }
    // a date without year is passed over for the next one
    assert!(bibtex(json!({"published-print": {"date-parts": [[null]]}})).contains("year = {2000}"));
}

// TestExtractTxtMetadataCandidates
// --------------------------------

#[test]
fn a_crossref_record_with_an_empty_title_is_skipped() {
    // crossref_to_bibtex used to raise IndexError, which stopped the whole extraction
    let untitled =
        json!({"message": {"DOI": "10.1000/untitled", "type": "journal-article", "title": []}});
    let titled = json!({"message": {"DOI": "10.1000/titled", "type": "journal-article", "title": ["The Titled Paper"]}});
    let (fetcher, _, _cache) = fetcher(vec![
        answer(
            "https://api.crossref.org/works/10.1000/untitled",
            false,
            200,
            "application/json",
            &untitled.to_string(),
            "",
        ),
        answer(
            "https://api.crossref.org/works/10.1000/titled",
            false,
            200,
            "application/json",
            &titled.to_string(),
            "",
        ),
    ]);
    let text = "cites https://doi.org/10.1000/untitled then https://doi.org/10.1000/titled\nThe Titled Paper";
    let options = Options {
        search_doi: true,
        search_fulltext: false,
        ..Options::default()
    };
    let bibtex = extract_txt_metadata(&fetcher, text, &options, "").unwrap();
    assert!(bibtex.contains("10.1000/titled"), "{bibtex}");
}

// TestDoiRequests
// ---------------

#[test]
fn invalid_json_on_doi_org_tries_the_website() {
    let page = r#"<a href="cite/x.bib">BibTeX</a>"#;
    let bib = "@article{x, doi = {10.1000/badjson}, title = {From Website}}";
    let (fetcher, calls, _cache) = fetcher(vec![
        answer(
            "https://api.crossref.org/works/10.1000/badjson",
            false,
            404,
            "text/plain",
            "",
            "",
        ),
        // doi.org claims JSON and sends HTML
        answer(
            "https://doi.org/10.1000/badjson",
            true,
            200,
            "application/json",
            "<html>",
            "",
        ),
        answer(
            "https://doi.org/10.1000/badjson",
            false,
            200,
            "text/html",
            page,
            "https://journal.example.org/a/x",
        ),
        answer(
            "https://journal.example.org/a/cite/x.bib",
            false,
            200,
            "text/plain",
            bib,
            "",
        ),
    ]);
    assert_eq!(fetcher.fetch_bibtex_by_doi("10.1000/badjson").unwrap(), bib);
    assert_eq!(calls.lock().unwrap().len(), 4);
    // the error says what doi.org sent, as a DOI without metadata
    let (fetcher, _, _cache) = fetcher_of_invalid_json();
    let error = fetcher.fetch_doi_metadata("10.1000/badjson").unwrap_err();
    assert_eq!(error.kind, ErrorKind::Request);
    assert_eq!(
        error.message,
        "10.1000/badjson: invalid metadata on doi.org (Expecting value: line 1 column 1 (char 0))"
    );
}

fn fetcher_of_invalid_json() -> (Fetcher, Arc<Mutex<Vec<String>>>, tempfile::TempDir) {
    fetcher(vec![
        answer(
            "https://api.crossref.org/works/10.1000/badjson",
            false,
            404,
            "text/plain",
            "",
            "",
        ),
        answer(
            "https://doi.org/10.1000/badjson",
            true,
            200,
            CSL,
            "<html>",
            "",
        ),
    ])
}

#[test]
fn the_links_of_the_website_are_relative_to_the_page() {
    // doi.org redirects to the journal page: its relative links are relative to that page
    let bib = "@article{x, doi = {10.1000/web}, title = {From Website}}";
    let (fetcher, calls, _cache) = fetcher(vec![
        answer(
            "https://doi.org/10.1000/web",
            false,
            200,
            "text/html",
            r#"<a href="cite/x.bib">BibTeX</a>"#,
            "https://journal.example.org/articles/x",
        ),
        answer(
            "https://journal.example.org/articles/cite/x.bib",
            false,
            200,
            "text/plain",
            bib,
            "",
        ),
    ]);
    assert_eq!(
        fetcher
            .fetch_bibtex_on_journal_website("10.1000/web")
            .unwrap(),
        bib
    );
    assert_eq!(
        *calls.lock().unwrap(),
        [
            "https://doi.org/10.1000/web",
            "https://journal.example.org/articles/cite/x.bib"
        ]
    );
}

// the errors of `add --doi`, test_fetch_cmds.py
// ---------------------------------------------

/// What `add --doi DOI` says when the queries are answered by this fetcher.
fn add_doi_error(fetcher: Fetcher, doi: &str) -> (String, String) {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let path = |rel: &str| format!("{}/{rel}", root.display());
    fs::create_dir_all(path("lib/.papers")).unwrap();
    fs::write(
        path("lib/.papers/config.json"),
        "{\n  \"bibtex\": \"papers.bib\",\n  \"filesdir\": \"files\",\n  \"pool\": \"none\",\n  \"version\": 3\n}",
    )
    .unwrap();
    fs::write(path("lib/papers.bib"), "").unwrap();
    let mut machine = Map::new();
    machine.insert("history-dir".into(), json!(path("history")));
    let config = Config::load(
        &path("lib/.papers/config.json"),
        MachineConfig::new(None, machine),
        None,
    )
    .unwrap();
    let mut session = Session::open(
        config,
        SessionOptions {
            message: format!("papers add --doi {doi}"),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    let request = AddRequest {
        doi: Some(doi.to_string()),
        queries: Queries::with(fetcher),
        ..AddRequest::default()
    };
    let mut resolver = Resolver::new(OnDuplicate::Error);
    let error = add(&mut session, &request, &mut resolver).unwrap_err();
    session.finish();
    (
        error.message().to_string(),
        fs::read_to_string(path("lib/papers.bib")).unwrap(),
    )
}

#[test]
fn an_unknown_doi_is_a_clean_error() {
    let (fetcher, _, _cache) = fetcher(vec![
        answer(
            "https://api.crossref.org/works/10.1000/zz",
            false,
            404,
            "text/plain",
            "",
            "",
        ),
        answer("https://doi.org/10.1000/zz", true, 404, "text/html", "", ""),
    ]);
    let (message, bib) = add_doi_error(fetcher, "10.1000/zz");
    assert_eq!(message, "10.1000/zz: unknown to doi.org");
    assert_eq!(bib, "");
}

#[test]
fn a_network_error_names_the_doi() {
    // the transport has no answer: the message of the error follows the DOI
    let (fetcher, _, _cache) = fetcher(vec![Answer {
        url: "https://api.crossref.org/works/10.1000/nw",
        negotiated: false,
        result: Err("no proxy".to_string()),
    }]);
    let (message, bib) = add_doi_error(fetcher, "10.1000/nw");
    assert_eq!(message, "10.1000/nw: no proxy");
    assert_eq!(bib, "");
}

#[test]
fn a_doi_without_metadata_is_the_error_of_the_doi() {
    // registered without metadata and no bibtex on the page: papers' own message names the DOI
    let (fetcher, _, _cache) = fetcher(vec![
        answer(
            "https://api.crossref.org/works/10.1000/none",
            false,
            404,
            "text/plain",
            "",
            "",
        ),
        answer(
            "https://doi.org/10.1000/none",
            true,
            200,
            "text/html",
            "",
            "",
        ),
        answer(
            "https://doi.org/10.1000/none",
            false,
            200,
            "text/html",
            "<p>no link</p>",
            "https://doi.org/10.1000/none",
        ),
    ]);
    let (message, _) = add_doi_error(fetcher, "10.1000/none");
    assert_eq!(message, "Unable to fetch BibTeX for DOI 10.1000/none");
}
