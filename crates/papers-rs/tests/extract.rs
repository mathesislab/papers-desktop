//! The DOIs of a text (`extract::doi`) against papers (`tests/gen_extract_expected.py`,
//! `expected/extract/doi_text.json` and `doi_misc.json`): the same DOIs, in the same order,
//! for the unit tests of papers and for generated texts; and the pieces they are made of.
//!
//! A larger run: `python3 tests/gen_extract_expected.py --count 100000 --seed 7 --out /tmp/x`
//! then `PAPERS_RS_EXTRACT_FUZZ=/tmp/x/doi_text.json cargo test -p papers-rs --test extract
//! fuzz_file`.

use papers_rs::extract::doi::{
    DoiParsingError, doi_from_info_fields, doi_from_xmp, isvaliddoi, normalize_doi_text,
    parse_dois, py_split, query_text, title_in_text,
};
use serde_json::Value;

mod common;
use common::{dir, json_file};

fn expected(name: &str) -> Value {
    json_file(&dir("expected").join("extract").join(name))
}

fn check_cases(cases: &[Value]) -> Vec<String> {
    let mut failures = Vec::new();
    for case in cases {
        let text = case["text"].as_str().unwrap();
        let tail = case["tail"].as_str().unwrap();
        let got: Result<Vec<String>, DoiParsingError> = parse_dois(text, tail);
        let want: Result<Vec<String>, String> = match case.get("dois") {
            Some(d) => Ok(d
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap().to_string())
                .collect()),
            None => Err(case["error"].as_str().unwrap().to_string()),
        };
        let got = got.map_err(|e| e.0);
        if got != want {
            failures.push(format!(
                "{text:?} tail {tail:?}\n   papers    {want:?}\n   papers-rs {got:?}"
            ));
        }
    }
    failures
}

#[test]
fn the_dois_of_a_text_as_papers() {
    let cases = expected("doi_text.json");
    let cases = cases.as_array().unwrap();
    let failures = check_cases(cases);
    assert!(
        failures.is_empty(),
        "{} of {} differ:\n{}",
        failures.len(),
        cases.len(),
        failures[..failures.len().min(10)].join("\n")
    );
}

/// A bigger recording, made by the generator (see the top of this file).
#[test]
fn fuzz_file() {
    let Ok(path) = std::env::var("PAPERS_RS_EXTRACT_FUZZ") else {
        return;
    };
    let cases = json_file(std::path::Path::new(&path));
    let cases = cases.as_array().unwrap();
    let failures = check_cases(cases);
    assert!(
        failures.is_empty(),
        "{} of {} differ:\n{}",
        failures.len(),
        cases.len(),
        failures[..failures.len().min(10)].join("\n")
    );
}

#[test]
fn the_text_is_normalized_as_papers_does() {
    let j = expected("doi_misc.json");
    for pair in j["normalize"].as_array().unwrap() {
        assert_eq!(
            normalize_doi_text(pair[0].as_str().unwrap()),
            pair[1].as_str().unwrap(),
            "{:?}",
            pair[0]
        );
    }
}

#[test]
fn isvaliddoi_as_papers() {
    let j = expected("doi_misc.json");
    for pair in j["isvaliddoi"].as_array().unwrap() {
        assert_eq!(
            isvaliddoi(pair[0].as_str().unwrap()),
            pair[1].as_bool().unwrap(),
            "{:?}",
            pair[0]
        );
    }
}

#[test]
fn the_doi_of_the_metadata_as_papers() {
    let j = expected("doi_misc.json");
    for case in j["xmp"].as_array().unwrap() {
        assert_eq!(
            doi_from_xmp(case[0].as_str().unwrap()).as_deref(),
            case[1].as_str(),
            "{:?}",
            case[0]
        );
    }
    for case in j["info"].as_array().unwrap() {
        let [subject, keywords, title] = [0, 1, 2].map(|i| case[i].as_str().unwrap());
        assert_eq!(
            doi_from_info_fields(subject, keywords, title).as_deref(),
            case[3].as_str(),
            "{case:?}"
        );
    }
}

#[test]
fn the_title_in_the_text_as_papers() {
    let j = expected("doi_misc.json");
    let mut failures = Vec::new();
    for case in j["title_in_text"].as_array().unwrap() {
        let got = title_in_text(case[0].as_str().unwrap(), case[1].as_str().unwrap());
        if got != case[2].as_bool().unwrap() {
            failures.push(format!("{case:?}: papers-rs {got}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_query_text_as_papers() {
    let j = expected("doi_misc.json");
    for case in j["query_text"].as_array().unwrap() {
        let got = query_text(case[0].as_str().unwrap(), case[1].as_i64().unwrap());
        match (&case[2]["ok"], &case[2]["err"]) {
            (Value::String(s), _) => assert_eq!(got.as_deref(), Ok(s.as_str()), "{case:?}"),
            (_, Value::String(e)) => assert_eq!(got, Err(e.clone()), "{case:?}"),
            _ => unreachable!(),
        }
    }
}

#[test]
fn split_as_python() {
    let j = expected("doi_misc.json");
    for case in j["split"].as_array().unwrap() {
        let want: Vec<&str> = case[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        assert_eq!(py_split(case[0].as_str().unwrap()), want, "{:?}", case[0]);
    }
}

// the unit tests of papers (tests/test_extract_unit.py), as they are
mod papers_unit {
    use super::*;
    use papers_rs::extract::doi::parse_doi;

    #[test]
    fn parse_doi_cases() {
        assert_eq!(
            parse_doi("doi:10.5194/bg-8-515-2011").unwrap(),
            "10.5194/bg-8-515-2011"
        );
        assert_eq!(
            parse_doi("DOI:10.5194/test-2020").unwrap(),
            "10.5194/test-2020"
        );
        assert_eq!(
            parse_doi("doi:10.5194/ESD-4-11-2013").unwrap(),
            "10.5194/esd-4-11-2013"
        );
        assert!(parse_doi("not a doi").is_err());
        let arxiv = parse_doi("arxiv:1234.5678").unwrap();
        assert!(arxiv.contains("10.48550") && arxiv.contains("1234.5678"));
        // the parenthesis that closes the text is not the DOI's
        assert_eq!(
            parse_doi("(doi:10.1007/s00382-010-0904-1)").unwrap(),
            "10.1007/s00382-010-0904-1"
        );
        assert_eq!(
            parse_doi("http://dx.doi.org/10.1007/s00382-010-0904-1)").unwrap(),
            "10.1007/s00382-010-0904-1"
        );
        // balanced ones are
        assert_eq!(
            parse_doi("doi:10.1234/foo(bar)baz").unwrap(),
            "10.1234/foo(bar)baz"
        );
        assert_eq!(
            parse_doi("doi:10.1234/foo(bar)").unwrap(),
            "10.1234/foo(bar)"
        );
    }

    #[test]
    fn validity() {
        assert!(isvaliddoi("10.5194/bg-8-515-2011"));
        assert!(!isvaliddoi("invalid"));
    }
}
