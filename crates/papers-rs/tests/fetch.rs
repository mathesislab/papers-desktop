//! The queries and the extraction of metadata against papers (`tests/gen_extract_expected.py`,
//! `expected/extract/fetch.json` and `flows.json`): crossref records as bibtex, the score of a
//! search, the cache, the links of a page; and the flows (by DOI, arXiv, doi.org, full text, a text,
//! the PDFs of `tests/fixtures/pdf`) with the answers of the network replayed from the recordings:
//! the same result or error, and the same requests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use papers_rs::config::json::loads;
use papers_rs::extract::{self, Options};
use papers_rs::fetch::cache::{cache_path, hashed_key};
use papers_rs::fetch::crossref::{crossref_score, crossref_to_bibtex};
use papers_rs::fetch::fuzz::token_set_ratio;
use papers_rs::fetch::html::anchor_links;
use papers_rs::fetch::http::{Response, Transport};
use papers_rs::fetch::{ErrorKind, FetchError, Fetcher};
use serde_json::{Value, json};

mod common;
use common::dir;

/// The recording, read as Python reads JSON (the floats are exact: serde_json's are not always).
fn expected(name: &str) -> Value {
    loads(&std::fs::read_to_string(dir("expected").join("extract").join(name)).unwrap()).unwrap()
}

#[test]
fn crossref_records_as_bibtex_as_papers() {
    let j = expected("fetch.json");
    let cases = j["crossref"].as_array().unwrap();
    let mut failures = Vec::new();
    for case in cases {
        let got = crossref_to_bibtex(&case["record"]);
        match (case.get("ok"), got) {
            (Some(want), Ok(got)) if want.as_str() == Some(got.as_str()) => {}
            (None, Err(_)) => {} // papers raises: the message is Python's
            (want, got) => failures.push(format!(
                "{}\n   papers    {want:?}\n   papers-rs {got:?}",
                case["record"]
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} differ:\n{}",
        failures.len(),
        cases.len(),
        failures[..failures.len().min(8)].join("\n")
    );
}

#[test]
fn token_set_ratio_as_rapidfuzz() {
    let j = expected("fetch.json");
    for case in j["token_set_ratio"].as_array().unwrap() {
        let got = token_set_ratio(case[0].as_str().unwrap(), case[1].as_str().unwrap());
        assert_eq!(got, case[2].as_f64().unwrap(), "{case:?}");
    }
}

#[test]
fn the_score_of_a_result_as_papers() {
    let j = expected("fetch.json");
    for case in j["score"].as_array().unwrap() {
        let got = crossref_score(case["text"].as_str().unwrap(), &case["record"]);
        match case.get("score") {
            Some(want) => {
                let (score, float) = got.unwrap_or_else(|e| panic!("{e}: {case}"));
                assert_eq!(score, want.as_f64().unwrap(), "{case}");
                assert_eq!(float, case["float"].as_bool().unwrap(), "{case}");
            }
            None => assert!(got.is_err(), "{case}"),
        }
    }
}

#[test]
fn the_files_of_the_cache_as_papers() {
    let j = expected("fetch.json");
    for case in j["cache"]["paths"].as_array().unwrap() {
        assert_eq!(
            cache_path("/c/doi", case[0].as_str().unwrap()),
            case[1].as_str().unwrap(),
            "{:?}",
            case[0]
        );
    }
    for case in j["cache"]["hashes"].as_array().unwrap() {
        assert_eq!(
            hashed_key(case[0].as_str().unwrap()),
            case[1].as_str().unwrap()
        );
    }
}

#[test]
fn the_links_of_a_page_as_beautifulsoup() {
    let j = expected("fetch.json");
    for case in j["html"].as_array().unwrap() {
        let want: Vec<&str> = case[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        assert_eq!(
            anchor_links(case[0].as_str().unwrap()),
            want,
            "{:?}",
            case[0]
        );
    }
}

// the flows
// ---------

/// The answers by URL: a list is the answers to the successive requests of that URL (the last one
/// goes on), `url` the URL that answered, after the redirects.
struct Replay {
    fixtures: HashMap<String, Value>,
    requests: Mutex<Vec<(String, Option<String>)>>,
}

struct Shared(Arc<Replay>);

impl Transport for Shared {
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Response, String> {
        let n = {
            let mut requests = self.0.requests.lock().unwrap();
            requests.push((url.to_string(), accept.map(str::to_string)));
            requests.iter().filter(|(u, _)| u == url).count() - 1
        };
        let f = self
            .0
            .fixtures
            .get(url)
            .ok_or_else(|| format!("no fixture: {url}"))?;
        let f = match f.as_array() {
            Some(answers) => &answers[n.min(answers.len() - 1)],
            None => f,
        };
        Ok(Response {
            status: f["status"].as_u64().unwrap() as u16,
            reason: f["reason"].as_str().unwrap().to_string(),
            content_type: f["content_type"].as_str().unwrap().to_string(),
            body: f["body"].as_str().unwrap().as_bytes().to_vec(),
            url: f["url"].as_str().unwrap_or(url).to_string(),
            retry_after: None,
        })
    }
}

fn kind_name(k: ErrorKind) -> &'static str {
    match k {
        ErrorKind::NotFound => "not_found",
        ErrorKind::Request => "request",
        ErrorKind::Value => "value",
        ErrorKind::Network => "network",
        ErrorKind::Other => "other",
    }
}

/// What a call gave, as the recording has it.
fn outcome(r: Result<Value, FetchError>) -> Value {
    match r {
        Ok(v) => json!({"ok": v}),
        Err(e) => json!({"err": [kind_name(e.kind), e.message]}),
    }
}

fn options_of(o: &Value, search_fulltext: bool) -> Options {
    Options {
        search_doi: o["search_doi"].as_bool().unwrap_or(true),
        search_fulltext: o["search_fulltext"].as_bool().unwrap_or(search_fulltext),
        max_query_words: o["max_query_words"].as_i64().unwrap_or(200),
        ..Options::default()
    }
}

fn call(fetcher: &Fetcher, c: &Value, pdfs: &std::path::Path) -> Value {
    let args = c["args"].as_array().unwrap();
    let s = |i: usize| args[i].as_str().unwrap();
    match c["fn"].as_str().unwrap() {
        "fetch_bibtex_by_doi" => outcome(fetcher.fetch_bibtex_by_doi(s(0)).map(Value::String)),
        "fetch_doi_metadata" => outcome(fetcher.fetch_doi_metadata(s(0))),
        "fetch_bibtex_on_journal_website" => outcome(
            fetcher
                .fetch_bibtex_on_journal_website(s(0))
                .map(Value::String),
        ),
        "fetch_bibtex_by_fulltext_crossref" => {
            let optional = |i: usize| args.get(i).and_then(|a| a.as_str());
            let rows = args.get(3).and_then(|a| a.as_i64()).unwrap_or(5);
            outcome(
                fetcher
                    .fetch_bibtex_by_fulltext_crossref(s(0), optional(1), optional(2), rows)
                    .map(Value::String),
            )
        }
        "extract_txt_metadata" => {
            let options = options_of(&args[2], false);
            outcome(extract::extract_txt_metadata(fetcher, s(0), &options, s(1)).map(Value::String))
        }
        "confirm_doi" => {
            let dois: Vec<String> = args[0]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d.as_str().unwrap().to_string())
                .collect();
            outcome(extract::confirm_doi(fetcher, &dois, s(1)).map(|o| match o {
                Some((doi, bibtex)) => json!([doi, bibtex]),
                None => Value::Null,
            }))
        }
        "extract_pdf_doi" => {
            let path = pdfs.join(s(0));
            outcome(
                extract::extract_pdf_doi(fetcher, path.to_str().unwrap(), false).map(Value::String),
            )
        }
        "extract_pdf_metadata" => {
            let path = pdfs.join(s(0));
            let options = options_of(&args[1], true);
            outcome(
                extract::extract_pdf_metadata(fetcher, path.to_str().unwrap(), &options)
                    .map(Value::String),
            )
        }
        other => panic!("{other}"),
    }
}

/// The message of an error, but for the time of a remembered "not found".
fn comparable(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(m) = v["err"][1].as_str()
        && let Some(i) = m.find("(cached until")
    {
        v["err"][1] = json!(&m[..i]);
    }
    v
}

#[test]
fn the_queries_as_papers_with_the_network_replayed() {
    let scenarios = expected("flows.json");
    let pdfs = dir("fixtures").join("pdf");
    let mut failures = Vec::new();
    for scenario in scenarios.as_array().unwrap() {
        let fixtures: HashMap<String, Value> = scenario["fixtures"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let replay = Arc::new(Replay {
            fixtures,
            requests: Mutex::new(Vec::new()),
        });
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::with_transport(
            Box::new(Shared(replay.clone())),
            cache.path().to_str().unwrap(),
        );
        for c in scenario["calls"].as_array().unwrap() {
            let before = replay.requests.lock().unwrap().len();
            let got = call(&fetcher, c, &pdfs);
            let requests: Vec<Value> = replay.requests.lock().unwrap()[before..]
                .iter()
                .map(|(u, a)| json!([u, a]))
                .collect();
            let want = if c.get("ok").is_some() {
                json!({"ok": c["ok"]})
            } else {
                json!({"err": c["err"]})
            };
            // a cached error says until when
            if comparable(&got) != comparable(&want)
                || Value::Array(requests.clone()) != c["requests"]
            {
                failures.push(format!(
                    "{} {} {}\n   papers    {:.300} {}\n   papers-rs {:.300} {}",
                    scenario["name"],
                    c["fn"],
                    c["args"],
                    want,
                    c["requests"],
                    got,
                    Value::Array(requests)
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} calls differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The cache of papers-rs is the cache of papers: a record that papers cached is read, and one that
/// papers-rs cached is readable by papers (the same file name and `{"value": ...}`).
#[test]
fn the_cache_is_shared_with_papers() {
    let cache = tempfile::tempdir().unwrap();
    let record = json!({"DOI": "10.1234/Cached.1", "title": ["From the cache"], "type": "journal-article", "issued": {"date-parts": [[2001]]}});
    // what papers writes: <cache>/doi/10.1234/cached.1.json
    let file = cache
        .path()
        .join("doi")
        .join("10.1234")
        .join("cached.1.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, format!("{{\"value\": {record}}}")).unwrap();
    let replay = Arc::new(Replay {
        fixtures: HashMap::new(),
        requests: Mutex::new(Vec::new()),
    });
    let fetcher = Fetcher::with_transport(
        Box::new(Shared(replay.clone())),
        cache.path().to_str().unwrap(),
    );
    let bibtex = fetcher.fetch_bibtex_by_doi("10.1234/CACHED.1").unwrap();
    assert!(bibtex.contains("title = {From the cache}"), "{bibtex}");
    assert!(replay.requests.lock().unwrap().is_empty());
    // a not-found that papers remembered
    let gone = cache.path().join("doi").join("10.1234").join("gone.json");
    std::fs::write(
        &gone,
        format!(
            "{{\"not_found\": {}}}",
            papers_rs::fetch::cache::now() - 60.0
        ),
    )
    .unwrap();
    let error = fetcher.fetch_bibtex_by_doi("10.1234/gone").unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert!(
        error
            .message
            .starts_with("10.1234/gone: not found (cached until "),
        "{error}"
    );
    assert!(replay.requests.lock().unwrap().is_empty());
    // an old one is asked again
    std::fs::write(
        &gone,
        format!(
            "{{\"not_found\": {}}}",
            papers_rs::fetch::cache::now() - 2.0 * 86400.0
        ),
    )
    .unwrap();
    let error = fetcher.fetch_bibtex_by_doi("10.1234/gone").unwrap_err();
    assert_eq!(error.kind, ErrorKind::Network); // no fixture: the request was made
    assert_eq!(replay.requests.lock().unwrap().len(), 1);
}

/// What papers-rs writes in the cache is what papers writes.
#[test]
fn the_cache_files_are_written_as_papers_writes_them() {
    let cache = tempfile::tempdir().unwrap();
    let body = json!({"status": "ok", "message": {"DOI": "10.1234/W.1", "title": ["\u{e9}"], "reference": [{"key": "x"}], "type": "journal-article"}});
    let url = "https://api.crossref.org/works/10.1234/W.1".to_string();
    let fixtures = HashMap::from([(
        url,
        json!({"status": 200, "reason": "OK", "content_type": "application/json", "body": body.to_string()}),
    )]);
    let replay = Arc::new(Replay {
        fixtures,
        requests: Mutex::new(Vec::new()),
    });
    let fetcher = Fetcher::with_transport(Box::new(Shared(replay)), cache.path().to_str().unwrap());
    fetcher.fetch_doi_metadata("10.1234/W.1").unwrap();
    let written =
        std::fs::read_to_string(cache.path().join("doi").join("10.1234").join("w.1.json")).unwrap();
    // json.dump: ASCII, the references dropped (the keys in the order of the map)
    let escaped = "\\u00e9";
    assert_eq!(
        written,
        format!(
            r#"{{"value": {{"DOI": "10.1234/W.1", "title": ["{escaped}"], "type": "journal-article"}}}}"#
        )
    );
}
