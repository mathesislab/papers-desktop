//! The search of OpenAlex (`fetch::openalex`, an extension of papers-rs: not in papers, so no
//! comparison with it): recorded answers of OpenAlex (`fixtures/openalex.json`, made by
//! `record_openalex.py`) and of crossref (`fixtures/network.json`), replayed. None of the tests
//! needs the network; `--ignored` has one that asks OpenAlex.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use papers_rs::extract::{self, Options};
use papers_rs::fetch::crossref::crossref_to_bibtex;
use papers_rs::fetch::http::{Response, Transport};
use papers_rs::fetch::openalex::{self, OpenAlex, rank_works, work_bibtex, work_doi, work_record};
use papers_rs::fetch::{ErrorKind, Fetcher};
use serde_json::{Value, json};

mod common;
use common::{dir, json_file};

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

/// The network of the tests: OpenAlex answers every semantic search with `openalex`, crossref
/// and the others as recorded in `network.json`; the requests are kept.
struct Replay {
    openalex: Mutex<Value>,
    others: HashMap<String, Value>,
    requests: Mutex<Vec<String>>,
}

struct Shared(Arc<Replay>);

impl Transport for Shared {
    fn get(&self, url: &str, _accept: Option<&str>) -> Result<Response, String> {
        self.0.requests.lock().unwrap().push(url.to_string());
        let response = |status: u16, reason: &str, content_type: &str, body: String| Response {
            status,
            reason: reason.to_string(),
            content_type: content_type.to_string(),
            body: body.into_bytes(),
            url: url.to_string(),
        };
        if url.starts_with(openalex::API) {
            return Ok(response(
                200,
                "OK",
                "application/json",
                self.0.openalex.lock().unwrap().to_string(),
            ));
        }
        let f = self
            .0
            .others
            .get(url)
            .ok_or_else(|| format!("no fixture: {url}"))?;
        Ok(response(
            f["status"].as_u64().unwrap() as u16,
            f["reason"].as_str().unwrap(),
            f["content_type"].as_str().unwrap(),
            f["body"].as_str().unwrap().to_string(),
        ))
    }
}

struct Net {
    replay: Arc<Replay>,
    fetcher: Fetcher,
    cache: tempfile::TempDir,
}

impl Net {
    fn new(answer: &Value) -> Net {
        let others = json_file(&dir("fixtures").join("network.json"))
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let replay = Arc::new(Replay {
            openalex: Mutex::new(answer.clone()),
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

    fn requests(&self) -> Vec<String> {
        self.replay.requests.lock().unwrap().clone()
    }

    fn openalex_requests(&self) -> Vec<String> {
        self.requests()
            .into_iter()
            .filter(|u| u.starts_with(openalex::API))
            .collect()
    }
}

fn search_param(url: &str, name: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
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

#[test]
fn the_search_is_a_semantic_search_of_the_query() {
    let r = recorded("fettweis");
    let net = Net::new(&r.body);
    net.fetcher
        .fetch_bibtex_by_fulltext_openalex(&r.query, &OpenAlex::default(), 10, None)
        .unwrap();
    let requests = net.openalex_requests();
    assert_eq!(requests.len(), 1);
    let url = &requests[0];
    assert!(url.starts_with("https://api.openalex.org/works?search.semantic="));
    assert_eq!(search_param(url, "search.semantic").unwrap(), r.query);
    assert_eq!(search_param(url, "per-page").unwrap(), "10");
    assert!(search_param(url, "select").unwrap().contains("authorships"));
    // nothing that identifies the user, unless it is given
    assert_eq!(search_param(url, "mailto"), None);
    assert_eq!(search_param(url, "api_key"), None);
}

#[test]
fn the_address_and_the_key_are_sent_when_there_are() {
    let r = recorded("fettweis");
    let net = Net::new(&r.body);
    let access = OpenAlex {
        mailto: Some("me@example.org".into()),
        api_key: Some("KEY".into()),
    };
    net.fetcher
        .fetch_bibtex_by_fulltext_openalex(&r.query, &access, 10, None)
        .unwrap();
    let url = &net.openalex_requests()[0];
    assert_eq!(search_param(url, "mailto").unwrap(), "me@example.org");
    assert_eq!(search_param(url, "api_key").unwrap(), "KEY");
}

#[test]
fn a_long_query_is_cut_at_a_word_as_openalex_cuts_it() {
    let r = recorded("fettweis");
    let net = Net::new(&r.body);
    let long = std::iter::repeat_n("glacier", 400)
        .collect::<Vec<_>>()
        .join(" ");
    net.fetcher
        .openalex_search(&long, &OpenAlex::default(), 5)
        .unwrap();
    let sent = search_param(&net.openalex_requests()[0], "search.semantic").unwrap();
    assert!(sent.chars().count() <= 1500);
    assert!(sent.ends_with("glacier") && sent.split(' ').count() > 150);
}

#[test]
fn the_answers_are_cached_by_the_query() {
    let r = recorded("fettweis");
    let net = Net::new(&r.body);
    let access = OpenAlex::default();
    let first = net.fetcher.openalex_search(&r.query, &access, 10).unwrap();
    assert_eq!(first.len(), 10);
    // the same query again, and a smaller number of works, from the cache
    assert_eq!(
        net.fetcher.openalex_search(&r.query, &access, 10).unwrap(),
        first
    );
    assert_eq!(
        net.fetcher.openalex_search(&r.query, &access, 3).unwrap(),
        first[..3]
    );
    assert_eq!(net.openalex_requests().len(), 1);
    // more works than the cache has: asked again
    net.fetcher.openalex_search(&r.query, &access, 20).unwrap();
    assert_eq!(net.openalex_requests().len(), 2);
    // the cache is in its own folder
    let dir = net.cache.path().join("openalex");
    assert!(dir.is_dir(), "{}", dir.display());
}

#[test]
fn a_hit_with_a_doi_gives_the_bibtex_of_papers_for_that_doi() {
    let r = recorded("fettweis");
    let net = Net::new(&r.body);
    let bibtex = net
        .fetcher
        .fetch_bibtex_by_fulltext_openalex(&r.query, &OpenAlex::default(), 10, Some(&r.text))
        .unwrap();
    // crossref's record, as papers writes it: the entry that `papers fetch DOI` prints
    assert_eq!(format!("{bibtex}\n"), crossref_bibtex(FETTWEIS_DOI));
    assert!(bibtex.starts_with(&format!("@article{{{FETTWEIS_DOI},")));
    assert!(
        net.requests()
            .contains(&format!("https://api.crossref.org/works/{FETTWEIS_DOI}"))
    );
}

#[test]
fn a_hit_without_a_doi_is_made_of_the_openalex_work() {
    let r = recorded("angelen");
    let net = Net::new(&r.body);
    let bibtex = net
        .fetcher
        .fetch_bibtex_by_fulltext_openalex(&r.query, &OpenAlex::default(), 10, Some(&r.text))
        .unwrap();
    assert_eq!(
        bibtex,
        "@phdthesis{W1506728322,\n author = {van Angelen, J. H.},\n title = {The Greenland ice sheet in a warming climate},\n type = {thesis},\n url = {https://dspace.library.uu.nl/handle/1874/284736},\n year = {2013}\n}"
    );
    // and no request for a DOI that it has not
    assert!(net.requests().iter().all(|u| u.starts_with(openalex::API)));
}

#[test]
fn the_hits_are_scored_as_papers_scores_the_results_of_crossref() {
    let r = recorded("angelen");
    let works = r.body["results"].as_array().unwrap();
    let ranked = rank_works(&r.query, works).unwrap();
    // the thesis is the last of the semantic search; it is the one with its title in the text
    assert_eq!(
        works[9]["title"],
        "The Greenland ice sheet in a warming climate"
    );
    assert_eq!(ranked[0].1["id"], works[9]["id"]);
    assert!(ranked.windows(2).all(|w| w[0].0 >= w[1].0));
    // the score of papers: the family names, the title and the abstract, with the text
    let (score, _) =
        papers_rs::fetch::crossref::crossref_score(&r.query, &work_record(&works[9])).unwrap();
    assert_eq!(ranked[0].0, score);
}

#[test]
fn without_a_text_the_best_hit_is_taken_and_with_one_the_best_that_has_its_title_in_it() {
    // the text of the thesis, and the answer of another search
    let thesis = recorded("angelen");
    let fettweis = recorded("fettweis");
    let mut body = fettweis.body.clone();
    let thesis_work = thesis.body["results"][9].clone();
    body["results"].as_array_mut().unwrap().push(thesis_work);
    let net = Net::new(&body);
    let access = OpenAlex::default();
    let query = &thesis.query;
    // `papers fetch`: the best score (the thesis is among the works, but the text is its)
    let best = net
        .fetcher
        .fetch_bibtex_by_fulltext_openalex(query, &access, 10, None)
        .unwrap();
    assert!(
        best.contains("Greenland ice sheet in a warming climate"),
        "{best}"
    );
    // a text that has the title of another work of the search: that work, though the thesis scores higher
    let text = fettweis.body["results"][0]["title"]
        .as_str()
        .unwrap()
        .to_string();
    let net = Net::new(&body);
    let got = net
        .fetcher
        .fetch_bibtex_by_fulltext_openalex(query, &access, 11, Some(&text))
        .unwrap();
    assert_eq!(format!("{got}\n"), crossref_bibtex(FETTWEIS_DOI));
}

#[test]
fn a_doi_that_no_registry_has_gives_the_entry_of_the_work_with_the_doi() {
    let r = recorded("fettweis");
    let mut body = r.body.clone();
    body["results"][0]["doi"] = json!("https://doi.org/10.9999/not.a.doi");
    let net = Net::new(&body);
    let bibtex = net
        .fetcher
        .fetch_bibtex_by_fulltext_openalex(&r.query, &OpenAlex::default(), 10, Some(&r.text))
        .unwrap();
    assert!(
        bibtex.starts_with("@article{10.9999/not.a.doi,"),
        "{bibtex}"
    );
    assert!(bibtex.contains("journal = {"), "{bibtex}");
    assert!(bibtex.contains("pages = {1015-1033}"), "{bibtex}");
    assert!(bibtex.contains("volume = {11}"), "{bibtex}");
    assert!(bibtex.contains("number = {2}"), "{bibtex}");
    // crossref, then doi.org, were asked
    let requests = net.requests();
    assert!(
        requests
            .iter()
            .any(|u| u.starts_with("https://api.crossref.org/works/10.9999"))
    );
    assert!(
        requests
            .iter()
            .any(|u| u.starts_with("https://doi.org/10.9999"))
    );
}

#[test]
fn the_work_without_authors_year_or_source_is_still_an_entry() {
    let work = json!({"id": "https://openalex.org/W1", "title": "A title", "type": "other"});
    assert_eq!(
        work_bibtex(&work).unwrap(),
        "@misc{W1,\n title = {A title},\n url = {https://openalex.org/W1},\n year = {0000}\n}\n"
    );
    assert_eq!(work_doi(&work), None);
}

#[test]
fn no_results_is_an_error_of_the_search() {
    let net = Net::new(&json!({"meta": {"count": 0}, "results": []}));
    let error = net
        .fetcher
        .fetch_bibtex_by_fulltext_openalex("some words of a text", &OpenAlex::default(), 10, None)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Value);
    assert_eq!(error.message, "openalex fulltext: no results");
}

// in the chain of papers, where `--scholar` sits

fn options(access: Option<OpenAlex>) -> Options {
    Options {
        search_doi: false,
        openalex: access,
        ..Options::default()
    }
}

#[test]
fn the_chain_takes_a_hit_whose_title_is_in_the_text() {
    let r = recorded("fettweis");
    let net = Net::new(&r.body);
    let bibtex = extract::extract_txt_metadata(
        &net.fetcher,
        &r.text,
        &options(Some(OpenAlex::default())),
        "",
    )
    .unwrap();
    assert_eq!(format!("{bibtex}\n"), crossref_bibtex(FETTWEIS_DOI));
    // the query is the one of papers: the words of the text
    let sent = search_param(&net.openalex_requests()[0], "search.semantic").unwrap();
    assert_eq!(sent, r.query);
    // crossref's search was not asked
    assert!(
        net.requests()
            .iter()
            .all(|u| !u.contains("query.bibliographic"))
    );
}

#[test]
fn the_chain_rejects_a_hit_whose_title_is_not_in_the_text() {
    let r = recorded("fettweis");
    let net = Net::new(&r.body);
    // the text of the paper without its title: the hits are about the same, and not it
    let text = r.text.replace(
        "Reconstructions of the 1900-2015 Greenland ice sheet surface mass balance using the regional climate MAR model",
        "",
    );
    let error =
        extract::extract_txt_metadata(&net.fetcher, &text, &options(Some(OpenAlex::default())), "")
            .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Value);
    assert!(
        error.message.starts_with("failed to extract metadata"),
        "{error}"
    );
}

#[test]
fn a_copy_of_the_paper_comes_after_the_paper() {
    // OpenAlex has the abstracts of conferences and the preprints with the title of the paper
    let r = recorded("fettweis");
    let mut body = r.body.clone();
    let mut copy = body["results"][0].clone();
    copy["id"] = json!("https://openalex.org/W1");
    copy["doi"] = Value::Null;
    copy["type"] = json!("conference-abstract");
    body["results"].as_array_mut().unwrap().insert(0, copy);
    let net = Net::new(&body);
    let access = OpenAlex::default();
    let got = net
        .fetcher
        .fetch_bibtex_by_fulltext_openalex(&r.query, &access, 11, Some(&r.text))
        .unwrap();
    assert_eq!(format!("{got}\n"), crossref_bibtex(FETTWEIS_DOI));
    // and where there is only the copy, it is taken
    body["results"] = json!([body["results"][0].clone()]);
    let net = Net::new(&body);
    let got = net
        .fetcher
        .fetch_bibtex_by_fulltext_openalex(&r.query, &access, 11, Some(&r.text))
        .unwrap();
    assert!(got.starts_with("@misc{W1,"), "{got}");
}

#[test]
fn a_title_that_is_too_short_to_tell_is_not_accepted() {
    // papers accepts a title of less than ten letters and digits as too short to tell (the title
    // of one of its results is checked as it is); here, those of less than twenty are not
    let body = json!({"meta": {"count": 2}, "results": [
        {"id": "https://openalex.org/W9", "title": ": () *0 789)()* )(", "type": "article", "publication_year": 2000},
        {"id": "https://openalex.org/W8", "title": "Paleoceanography", "type": "book", "publication_year": 2005}]});
    let net = Net::new(&body);
    let text = "Paleoceanography garbled text *0 789)()* )( of a PDF whose fonts are not decoded, with enough words in it to ask";
    let error =
        extract::extract_txt_metadata(&net.fetcher, text, &options(Some(OpenAlex::default())), "")
            .unwrap_err();
    assert!(
        error.message.starts_with("failed to extract metadata"),
        "{error}"
    );
}

#[test]
fn without_the_option_crossref_is_asked_and_openalex_is_not() {
    let r = recorded("fettweis");
    let net = Net::new(&r.body);
    let _ = extract::extract_txt_metadata(&net.fetcher, &r.text, &options(None), "");
    assert!(net.openalex_requests().is_empty());
    assert!(
        net.requests()
            .iter()
            .any(|u| u.contains("query.bibliographic"))
    );
}

#[test]
#[ignore = "asks OpenAlex (a search of the budget of the day)"]
fn the_real_openalex_finds_the_paper() {
    let r = recorded("fettweis");
    let cache = tempfile::tempdir().unwrap();
    let fetcher = Fetcher::new(cache.path().to_str().unwrap()).unwrap();
    let access = OpenAlex {
        mailto: std::env::var("PAPERS_MAILTO").ok(),
        api_key: std::env::var("OPENALEX_API_KEY").ok(),
    };
    let works = fetcher.openalex_search(&r.query, &access, 10).unwrap();
    assert!(
        works
            .iter()
            .any(|w| work_doi(w).as_deref() == Some(FETTWEIS_DOI))
    );
}
