//! The search of OpenAlex (`fetch::openalex`, `--openalex`: papers' `tests/test_openalex.py`, papers
//! v3 faeaf4d, #205), with the network replayed: works made in the test, and recorded answers of
//! OpenAlex (`fixtures/openalex.json`, made by `record_openalex.py`) and of crossref
//! (`fixtures/network.json`). None of the tests needs the network; `--ignored` has one that asks
//! OpenAlex.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use papers_rs::bib::{Bib, Entry};
use papers_rs::extract::doi::title_in_text_strict;
use papers_rs::extract::{self, Options};
use papers_rs::fetch::cache::hashed_key;
use papers_rs::fetch::crossref::{crossref_score, crossref_to_bibtex};
use papers_rs::fetch::http::{Response, Transport};
use papers_rs::fetch::openalex::{
    self, OpenAlex, is_copy, trim, work_bibtex, work_doi, work_record,
};
use papers_rs::fetch::{ErrorKind, FetchError, Fetcher};
use serde_json::{Value, json};

mod common;
use common::{dir, json_file};

const PAPER: &str = "Greenland ice sheet mass balance under climate warming";
const TEXT: &str = "Greenland ice sheet mass balance under climate warming\n\nAnn Author, Bo Writer\n\nWe study the mass balance of the Greenland ice sheet.";

/// A work as the search returns it (`tests/test_openalex.py`: `work`).
fn work(title: &str, doi: Option<&str>, id: &str, kind: &str, authors: &[&str]) -> Value {
    let mut w = json!({
        "id": format!("https://openalex.org/{id}"), "type": kind, "publication_year": 2020, "title": title,
        "authorships": authors.iter().map(|a| json!({"raw_author_name": a})).collect::<Vec<_>>(),
        "primary_location": {"landing_page_url": null,
                             "source": {"display_name": "Nature", "type": "journal",
                                        "host_organization_name": "Nature Portfolio"}}});
    if let Some(doi) = doi {
        w["doi"] = json!(doi);
    }
    w
}

/// The work of the paper of `TEXT`.
fn paper() -> Value {
    work(
        PAPER,
        Some("https://doi.org/10.1000/gis"),
        "W1",
        "article",
        &["Ann Author", "Bo Writer"],
    )
}

/// An answer of the network: a status and a body, or no answer (the error of the transport).
#[derive(Clone)]
enum Answer {
    Http(u16, String),
    Fails(String),
}

fn answer(works: &[Value]) -> Answer {
    Answer::Http(200, json!({"results": works}).to_string())
}

/// The network of the tests: OpenAlex gives the answers in turn (the last again), the others as
/// given (`network.json`, the crossref record of 10.1000/gis), else 404; the requests are kept.
struct Replay {
    openalex: Mutex<VecDeque<Answer>>,
    others: HashMap<String, Answer>,
    requests: Mutex<Vec<String>>,
}

struct Shared(Arc<Replay>);

impl Transport for Shared {
    fn get(&self, url: &str, _accept: Option<&str>) -> Result<Response, String> {
        self.0.requests.lock().unwrap().push(url.to_string());
        let answer = if url.starts_with(openalex::API) {
            let mut queue = self.0.openalex.lock().unwrap();
            if queue.len() > 1 {
                queue.pop_front()
            } else {
                queue.front().cloned()
            }
            .expect("an answer")
        } else {
            self.0
                .others
                .get(url)
                .cloned()
                .unwrap_or(Answer::Http(404, String::new()))
        };
        match answer {
            Answer::Fails(error) => Err(error),
            Answer::Http(status, body) => Ok(Response {
                status,
                reason: if status == 200 { "OK" } else { "Error" }.to_string(),
                content_type: "application/json".to_string(),
                body: body.into_bytes(),
                url: url.to_string(),
            }),
        }
    }
}

/// The crossref record of 10.1000/gis.
fn gis_record() -> Value {
    json!({"type": "journal-article", "DOI": "10.1000/gis", "title": [PAPER],
           "author": [{"given": "Ann", "family": "Author"}, {"given": "Bo", "family": "Writer"}],
           "container-title": ["Nature"], "issued": {"date-parts": [[2020]]}})
}

struct Net {
    replay: Arc<Replay>,
    fetcher: Fetcher,
    cache: tempfile::TempDir,
}

impl Net {
    fn new(answers: &[Answer]) -> Net {
        Net::with_others(answers, &[])
    }

    /// With answers of the others than those of `network.json`.
    fn with_others(answers: &[Answer], given: &[(&str, Answer)]) -> Net {
        let mut others: HashMap<String, Answer> = json_file(&dir("fixtures").join("network.json"))
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    Answer::Http(
                        v["status"].as_u64().unwrap() as u16,
                        v["body"].as_str().unwrap().to_string(),
                    ),
                )
            })
            .collect();
        others.insert(
            "https://api.crossref.org/works/10.1000/gis".to_string(),
            Answer::Http(200, json!({"message": gis_record()}).to_string()),
        );
        others.extend(
            given
                .iter()
                .map(|(url, answer)| (url.to_string(), answer.clone())),
        );
        let replay = Arc::new(Replay {
            openalex: Mutex::new(answers.iter().cloned().collect()),
            others,
            requests: Mutex::new(Vec::new()),
        });
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::with_transport(
            Box::new(Shared(replay.clone())),
            cache.path().to_str().unwrap(),
        );
        Net {
            replay,
            fetcher,
            cache,
        }
    }

    fn with(works: &[Value]) -> Net {
        Net::new(&[answer(works)])
    }

    fn requests(&self) -> Vec<String> {
        self.replay.requests.lock().unwrap().clone()
    }

    fn openalex_requests(&self) -> Vec<String> {
        self.requests()
            .into_iter()
            .filter(|u| u.starts_with(openalex::API))
            .collect()
    }

    /// `fetch_bibtex_by_fulltext_openalex`
    fn search(
        &self,
        txt: &str,
        access: &OpenAlex,
        accept: Option<&dyn Fn(&str) -> bool>,
    ) -> Result<String, FetchError> {
        self.fetcher
            .fetch_bibtex_by_fulltext_openalex(txt, access, accept, openalex::ROWS)
    }
}

fn params(url: &str) -> HashMap<String, String> {
    reqwest::Url::parse(url)
        .unwrap()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn entry(bibtex: &str) -> Entry {
    Bib::parse(bibtex)
        .entries()
        .next()
        .expect("an entry")
        .clone()
}

// the query
// ---------

#[test]
fn a_long_query_is_cut_at_1500_characters() {
    let net = Net::with(&[paper()]);
    let query = vec!["climate"; 400].join(" "); // 3199 characters
    net.search(&query, &OpenAlex::default(), None).unwrap();
    let sent = &params(&net.openalex_requests()[0])["search.semantic"];
    assert!(sent.chars().count() <= 1500 && sent.chars().count() > 1490);
    assert!(sent.split(' ').all(|w| w == "climate")); // no word is cut
}

#[test]
fn a_semantic_search_of_the_work_fields() {
    let net = Net::with(&[paper()]);
    net.search(PAPER, &OpenAlex::default(), None).unwrap();
    let requests = net.openalex_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("https://api.openalex.org/works?"));
    let sent = params(&requests[0]);
    assert_eq!(sent["search.semantic"], PAPER);
    assert!(!sent.contains_key("search")); // the plain search needs every word
    assert_eq!(sent["per-page"], openalex::ROWS.to_string());
    assert!(sent["select"].contains("abstract_inverted_index"));
}

#[test]
fn the_email_and_the_key_when_they_are_given() {
    let net = Net::with(&[paper()]);
    net.search(PAPER, &OpenAlex::default(), None).unwrap();
    let access = OpenAlex {
        mailto: Some("me@example.org".into()),
        api_key: Some("secret".into()),
    };
    net.search(&format!("{PAPER} again"), &access, None)
        .unwrap();
    let requests = net.openalex_requests();
    let (first, second) = (params(&requests[0]), params(&requests[1]));
    assert!(!first.contains_key("mailto") && !first.contains_key("api_key"));
    assert_eq!(second["mailto"], "me@example.org");
    assert_eq!(second["api_key"], "secret");
}

#[test]
fn the_answers_are_cached_whatever_the_key() {
    let net = Net::with(&[paper()]);
    net.search(PAPER, &OpenAlex::default(), None).unwrap();
    let keyed = OpenAlex {
        api_key: Some("secret".into()),
        ..OpenAlex::default()
    };
    net.search(PAPER, &keyed, None).unwrap();
    net.search(&format!("{PAPER} other"), &keyed, None).unwrap();
    assert_eq!(net.openalex_requests().len(), 2);
    // as papers keeps them: by the hash of the parameters, the works trimmed
    let params = format!(
        "{{\"per-page\": 10, \"search.semantic\": \"{PAPER}\", \"select\": \"{}\"}}",
        openalex::SELECT
    );
    let key = hashed_key(&params);
    let file = net
        .cache
        .path()
        .join("openalex")
        .join(&key[..2])
        .join(format!("{key}.json"));
    let kept = json_file(&file);
    assert_eq!(kept, json!({"value": [trim(&paper()).unwrap()]}));
}

#[test]
fn no_cache_for_an_error() {
    let net = Net::new(&[Answer::Http(500, "{}".into()), answer(&[paper()])]);
    let error = net.search(PAPER, &OpenAlex::default(), None).unwrap_err();
    assert!(error.is_os_error());
    net.search(PAPER, &OpenAlex::default(), None).unwrap();
    assert_eq!(net.openalex_requests().len(), 2);
}

// the choice of a work: by the score of papers and the check of the title
// ----------------------------------------------------------------------

/// The work taken (the DOIs are not known: the entry is made of the work, its key the DOI).
fn chosen(works: &[Value], text: &str, accept: bool) -> String {
    let net = Net::with(works);
    let check = |title: &str| title_in_text_strict(title, text);
    let accept: Option<&dyn Fn(&str) -> bool> = if accept { Some(&check) } else { None };
    entry(&net.search(text, &OpenAlex::default(), accept).unwrap()).key
}

#[test]
fn the_best_score_first() {
    let other = work(
        "Antarctic ice shelves in a warming world",
        Some("10.1000/ant"),
        "W2",
        "article",
        &["Zed Zee"],
    );
    assert_eq!(chosen(&[other, paper()], TEXT, false), "10.1000/gis");
}

#[test]
fn the_first_of_equal_scores() {
    let names = ["Ann Author", "Bo Writer"];
    let a = work(PAPER, Some("10.1000/a"), "W1", "article", &names);
    let b = work(PAPER, Some("10.1000/b"), "W2", "article", &names);
    assert_eq!(chosen(&[a.clone(), b.clone()], TEXT, true), "10.1000/a");
    assert_eq!(chosen(&[b, a], TEXT, true), "10.1000/b");
}

#[test]
fn the_title_must_be_in_the_text() {
    // the best by score has a title that the text does not have (a cited paper)
    let cited = work(
        "Mass balance of the Greenland ice sheet from 1992 to 2018",
        Some("10.1000/cited"),
        "W2",
        "article",
        &["Ann Author", "Bo Writer"],
    );
    let text = format!("{TEXT} We use the data of the Greenland ice sheet from 1992 to 2018.");
    assert_eq!(chosen(&[cited, paper()], &text, true), "10.1000/gis");
}

#[test]
fn no_title_in_the_text_gives_the_best() {
    // the caller's own check rejects it
    let other = work(
        "Something else entirely of this kind",
        Some("10.1000/x"),
        "W1",
        "article",
        &["Ann Author"],
    );
    assert_eq!(chosen(&[other], TEXT, true), "10.1000/x");
}

#[test]
fn short_titles_do_not_pass() {
    // a journal name, or a word of every text, is in the text (title_in_text lets it pass too)
    let journal = work(
        "Nature",
        Some("10.1000/nature"),
        "W2",
        "article",
        &["Ann Author", "Bo Writer"],
    );
    assert!(title_in_text_strict(PAPER, &format!("x {PAPER}")));
    assert!(!title_in_text_strict("Nature", "published in Nature"));
    assert!(!title_in_text_strict(
        "abcde fghij klmno pqrs",
        "abcde fghij klmno pqrs"
    )); // 19 letters
    assert!(title_in_text_strict(
        "abcde fghij klmno pqrst",
        "abcde fghij klmno pqrst"
    )); // 20
    assert_eq!(
        chosen(&[journal, paper()], &format!("{TEXT} Nature"), true),
        "10.1000/gis"
    );
}

#[test]
fn the_published_works_before_their_copies() {
    let mut preprint = work(
        PAPER,
        Some("10.31223/preprint"),
        "W2",
        "preprint",
        &["Ann Author", "Bo Writer"],
    );
    preprint["abstract_inverted_index"] = json!({"We": [0], "study": [1], "the": [2, 5], "mass": [3], "balance": [4],
        "Greenland": [6], "ice": [7], "sheet.": [8]});
    assert_eq!(
        chosen(&[preprint.clone(), paper()], TEXT, true),
        "10.1000/gis"
    );
    // copies only: the best of them
    assert_eq!(
        chosen(std::slice::from_ref(&preprint), TEXT, true),
        "10.31223/preprint"
    );
    // without check, the best is taken
    assert_eq!(
        chosen(&[preprint, paper()], TEXT, false),
        "10.31223/preprint"
    );
    assert!(is_copy(&work(
        PAPER,
        None,
        "W3",
        "conference-abstract",
        &[]
    )));
    assert!(!is_copy(&work(PAPER, None, "W3", "book-chapter", &[])));
}

#[test]
fn no_results() {
    let net = Net::with(&[]);
    let error = net.search(TEXT, &OpenAlex::default(), None).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Value);
    assert_eq!(error.message, "openalex fulltext: no results");
}

// the entry
// ---------

#[test]
fn the_entry_of_the_doi() {
    let net = Net::with(&[paper()]);
    let bibtex = net.search(TEXT, &OpenAlex::default(), None).unwrap();
    assert_eq!(
        format!("{bibtex}\n"),
        crossref_to_bibtex(&gis_record()).unwrap()
    );
    // the DOI without its URL
    assert!(
        net.requests()
            .contains(&"https://api.crossref.org/works/10.1000/gis".to_string())
    );
}

#[test]
fn a_doi_without_metadata_gives_the_openalex_entry() {
    let mut w = paper();
    w["doi"] = json!("https://doi.org/10.1000/unknown");
    let net = Net::with(&[w]);
    let bibtex = net.search(TEXT, &OpenAlex::default(), None).unwrap();
    assert!(bibtex.starts_with("@article{10.1000/unknown,"), "{bibtex}");
    assert!(bibtex.contains(PAPER));
    // crossref, then doi.org, were asked
    let requests = net.requests();
    assert!(requests.contains(&"https://api.crossref.org/works/10.1000/unknown".to_string()));
    assert!(requests.contains(&"https://doi.org/10.1000/unknown".to_string()));
}

#[test]
fn a_network_error_of_the_doi_is_raised() {
    let down = (
        "https://api.crossref.org/works/10.1000/gis",
        Answer::Fails("down".into()),
    );
    let net = Net::with_others(&[answer(&[paper()])], &[down]);
    let error = net.search(TEXT, &OpenAlex::default(), None).unwrap_err();
    assert!(error.is_os_error(), "{error:?}");
}

#[test]
fn a_work_without_doi() {
    let mut w = work(
        PAPER,
        None,
        "W2741809807",
        "article",
        &["Ann Author", "van der Berg, Bo"],
    );
    w["publication_year"] = json!(2019);
    w["biblio"] =
        json!({"volume": "579", "issue": "7798", "first_page": "233", "last_page": "239"});
    w["abstract_inverted_index"] = json!({"We": [0], "study": [1]});
    w["primary_location"]["landing_page_url"] = json!("https://example.org/paper");
    let net = Net::with(&[w]);
    let bibtex = net.search(TEXT, &OpenAlex::default(), None).unwrap();
    // no DOI to ask for
    assert!(net.requests().iter().all(|u| u.starts_with(openalex::API)));
    let entry = entry(&bibtex);
    assert_eq!(entry.entry_type, "article");
    assert_eq!(entry.key, "W2741809807");
    for (field, value) in [
        ("title", PAPER),
        ("author", "Author, Ann and van der Berg, Bo"),
        ("year", "2019"),
        ("journal", "Nature"),
        ("volume", "579"),
        ("number", "7798"),
        ("pages", "233-239"),
        ("url", "https://example.org/paper"),
    ] {
        assert_eq!(entry.get(field), Some(value), "{field}");
    }
    assert_eq!(entry.get("doi"), None);
}

#[test]
fn the_types_as_crossref_names_them() {
    let kind = |w: Value| {
        work_record(&trim(&w).unwrap())["type"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let article = || paper();
    let with = |key: &str, value: Value| {
        let mut w = article();
        w[key] = value;
        w
    };
    assert_eq!(
        kind(with("type_crossref", json!("book-chapter"))),
        "book-chapter"
    );
    assert_eq!(kind(article()), "journal-article");
    assert_eq!(
        kind(with(
            "primary_location",
            json!({"source": {"type": "conference", "display_name": "Proc"}})
        )),
        "proceedings-article"
    );
    assert_eq!(
        kind(with(
            "primary_location",
            json!({"source": {"type": "repository"}})
        )),
        "posted-content"
    );
    assert_eq!(kind(with("type", json!("preprint"))), "posted-content");
    assert_eq!(kind(with("type", json!("dissertation"))), "thesis");
    assert_eq!(kind(with("type", json!("book"))), "book");
    assert_eq!(kind(with("type", json!("dataset"))), "dataset");
}

#[test]
fn the_cache_has_the_abstract_as_text() {
    let mut w = paper();
    w["abstract_inverted_index"] = json!({"Hello": [0], "world": [1]});
    w["authorships"] = json!([{"raw_author_name": "A B", "institutions": [{"x": 1}]}]);
    let net = Net::with(&[w]);
    let works = net
        .fetcher
        .openalex_search("q", 10, &OpenAlex::default())
        .unwrap();
    assert_eq!(works[0]["abstract"], "Hello world");
    assert!(works[0].get("abstract_inverted_index").is_none());
    assert_eq!(works[0]["authorships"], json!([{"raw_author_name": "A B"}]));
}

#[test]
fn a_work_without_authors_year_or_source_is_still_an_entry() {
    let work = trim(&json!({"id": "https://openalex.org/W1", "title": "A title", "type": "other"}))
        .unwrap();
    assert_eq!(
        work_bibtex(&work).unwrap(),
        "@misc{W1,\n title = {A title},\n url = {https://openalex.org/W1},\n year = {0000}\n}\n"
    );
    assert_eq!(work_doi(&work), None);
}

// the errors
// ----------

fn error_of(answers: &[Answer], access: &OpenAlex) -> FetchError {
    let net = Net::new(answers);
    let error = net.search(TEXT, access, None).unwrap_err();
    assert!(error.is_os_error(), "{error:?}"); // as the errors of the network: a clean exit of the commands
    error
}

#[test]
fn http_400() {
    let body =
        json!({"error": "Invalid query parameters error", "message": "search.semantic too long"});
    let error = error_of(&[Answer::Http(400, body.to_string())], &OpenAlex::default());
    assert_eq!(
        error.message,
        "OpenAlex: HTTP 400: search.semantic too long"
    );
}

#[test]
fn http_429_names_the_key() {
    let limit = Answer::Http(429, json!({"message": "Rate limit exceeded"}).to_string());
    let error = error_of(std::slice::from_ref(&limit), &OpenAlex::default());
    assert_eq!(
        error.message,
        "OpenAlex: HTTP 429: Rate limit exceeded (without a free API key OpenAlex allows about 100 searches a day: set OPENALEX_API_KEY)"
    );
    // with a key, it is another limit
    let keyed = OpenAlex {
        api_key: Some("secret".into()),
        ..OpenAlex::default()
    };
    assert_eq!(
        error_of(&[limit], &keyed).message,
        "OpenAlex: HTTP 429: Rate limit exceeded"
    );
}

#[test]
fn an_http_error_without_a_message() {
    let error = error_of(
        &[Answer::Http(503, "not json".into())],
        &OpenAlex::default(),
    );
    assert_eq!(error.message, "OpenAlex: HTTP 503");
}

#[test]
fn a_network_error_does_not_show_the_key() {
    let access = OpenAlex {
        mailto: Some("me@example.org".into()),
        api_key: Some("secret".into()),
    };
    let down = Answer::Fails(
        "Max retries exceeded with url: /works?api_key=secret&mailto=me@example.org".into(),
    );
    assert_eq!(
        error_of(&[down], &access).message,
        "OpenAlex: Max retries exceeded with url: /works?api_key=***&mailto=***"
    );
}

#[test]
fn an_unexpected_answer() {
    for body in [
        r#"{"meta": {}}"#,
        "not json",
        r#"["x"]"#,
        r#"{"results": ["x"]}"#,
        r#"{"results": null}"#,
    ] {
        let error = error_of(&[Answer::Http(200, body.into())], &OpenAlex::default());
        assert_eq!(error.message, "OpenAlex: unexpected answer", "{body}");
    }
}

// in the chain of papers (`extract_txt_metadata` with `openalex`)
// ----------------------------------------------------------------

fn options() -> Options {
    Options {
        search_doi: false,
        search_fulltext: true,
        openalex: Some(OpenAlex::default()),
        ..Options::default()
    }
}

#[test]
fn accepted_only_with_its_title_in_the_text() {
    let net = Net::with(&[paper()]);
    let text = format!("A paper\n\n{TEXT} {}", vec!["word"; 20].join(" "));
    let bibtex = extract::extract_txt_metadata(&net.fetcher, &text, &options(), "").unwrap();
    assert!(bibtex.contains("10.1000/gis"));
}

#[test]
fn another_title_is_rejected() {
    let net = Net::with(&[work(
        "A quite different title of a paper",
        Some("10.1000/x"),
        "W1",
        "article",
        &[],
    )]);
    let error =
        extract::extract_txt_metadata(&net.fetcher, &format!("{TEXT} word word"), &options(), "")
            .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Value);
    assert!(
        error.message.starts_with("failed to extract metadata"),
        "{error}"
    );
}

#[test]
fn a_failure_of_the_search_is_no_metadata() {
    let net = Net::new(&[Answer::Http(
        429,
        json!({"message": "Rate limit exceeded"}).to_string(),
    )]);
    let error = extract::extract_txt_metadata(&net.fetcher, TEXT, &options(), "").unwrap_err();
    assert!(
        error.message.starts_with("failed to extract metadata"),
        "{error}"
    );
}

#[test]
fn crossref_is_not_searched() {
    let net = Net::with(&[paper()]);
    extract::extract_txt_metadata(&net.fetcher, &format!("{TEXT} word word"), &options(), "")
        .unwrap();
    assert!(
        net.requests()
            .iter()
            .all(|u| !u.contains("query.bibliographic"))
    );
}

#[test]
fn without_the_option_crossref_is_searched_and_openalex_is_not() {
    let net = Net::with(&[paper()]);
    let options = Options {
        openalex: None,
        ..options()
    };
    let _ = extract::extract_txt_metadata(&net.fetcher, &format!("{TEXT} word word"), &options, "");
    assert!(net.openalex_requests().is_empty());
    assert!(
        net.requests()
            .iter()
            .any(|u| u.contains("query.bibliographic"))
    );
}

// recorded answers of OpenAlex
// ----------------------------

/// A text of `fixtures/openalex.json` and the answer of OpenAlex to its search.
struct Recorded {
    text: String,
    query: String,
    body: Value,
}

fn recorded(name: &str) -> Recorded {
    let file = json_file(&dir("fixtures").join("openalex.json"));
    let answer = file["answers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == name)
        .unwrap();
    Recorded {
        text: file["texts"][name].as_str().unwrap().to_string(),
        query: answer["search"].as_str().unwrap().to_string(),
        body: answer["body"].clone(),
    }
}

impl Recorded {
    fn net(&self) -> Net {
        Net::new(&[Answer::Http(200, self.body.to_string())])
    }
}

const FETTWEIS_DOI: &str = "10.5194/tc-11-1015-2017";

fn crossref_bibtex(doi: &str) -> String {
    let network = json_file(&dir("fixtures").join("network.json"));
    let body = network[format!("https://api.crossref.org/works/{doi}")]["body"]
        .as_str()
        .unwrap();
    let record = papers_rs::config::json::loads(body).unwrap();
    crossref_to_bibtex(&record["message"]).unwrap()
}

fn accept_in(text: &str) -> impl Fn(&str) -> bool + '_ {
    move |title| title_in_text_strict(title, text)
}

#[test]
fn a_recorded_work_with_a_doi_gives_the_bibtex_of_papers_for_that_doi() {
    let r = recorded("fettweis");
    let net = r.net();
    let accept = accept_in(&r.text);
    let bibtex = net
        .search(&r.query, &OpenAlex::default(), Some(&accept))
        .unwrap();
    // crossref's record, as papers writes it: the entry that `papers fetch DOI` prints
    assert_eq!(format!("{bibtex}\n"), crossref_bibtex(FETTWEIS_DOI));
}

#[test]
fn a_recorded_work_without_a_doi_is_made_of_the_openalex_work() {
    let r = recorded("angelen");
    let net = r.net();
    let accept = accept_in(&r.text);
    let bibtex = net
        .search(&r.query, &OpenAlex::default(), Some(&accept))
        .unwrap();
    assert_eq!(
        bibtex,
        "@phdthesis{W1506728322,\n author = {van Angelen, J. H.},\n title = {The Greenland ice sheet in a warming climate},\n type = {thesis},\n url = {https://dspace.library.uu.nl/handle/1874/284736},\n year = {2013}\n}"
    );
    assert!(net.requests().iter().all(|u| u.starts_with(openalex::API)));
}

#[test]
fn the_recorded_works_are_scored_as_papers_scores_the_records_of_crossref() {
    let r = recorded("angelen");
    let works: Vec<Value> = r.body["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| trim(w).unwrap())
        .collect();
    let scores: Vec<f64> = works
        .iter()
        .map(|w| crossref_score(&r.query, &work_record(w)).unwrap().0)
        .collect();
    // the thesis is the last of the semantic search, and the first by the score
    assert_eq!(
        works[9]["title"],
        "The Greenland ice sheet in a warming climate"
    );
    assert!(scores[..9].iter().all(|s| *s < scores[9]), "{scores:?}");
    // without a check of the title, it is taken
    let bibtex = r
        .net()
        .search(&r.query, &OpenAlex::default(), None)
        .unwrap();
    assert!(bibtex.starts_with("@phdthesis{W1506728322,"), "{bibtex}");
}

#[test]
fn the_chain_takes_a_recorded_work_whose_title_is_in_the_text() {
    let r = recorded("fettweis");
    let net = r.net();
    let bibtex = extract::extract_txt_metadata(&net.fetcher, &r.text, &options(), "").unwrap();
    assert_eq!(format!("{bibtex}\n"), crossref_bibtex(FETTWEIS_DOI));
    // the query is the one of papers: the words of the text
    assert_eq!(
        params(&net.openalex_requests()[0])["search.semantic"],
        r.query
    );
}

#[test]
#[ignore = "asks OpenAlex (a search of the budget of the day)"]
fn the_real_openalex_finds_the_paper() {
    let r = recorded("fettweis");
    let cache = tempfile::tempdir().unwrap();
    let fetcher = Fetcher::new(cache.path().to_str().unwrap()).unwrap();
    let works = fetcher
        .openalex_search(&r.query, 10, &OpenAlex::from_env())
        .unwrap();
    assert!(
        works
            .iter()
            .any(|w| work_doi(w).as_deref() == Some(FETTWEIS_DOI))
    );
}
