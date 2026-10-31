//! The export formats against what the reference recorded in
//! `tests/expected/formats.json` (`tests/gen_formats.py`): names, dates, pages, DOIs, and the
//! CSL-JSON, RIS and BibJSON texts of random and hand-written entries.

use papers_rs::bib::Bib;
use papers_rs::formats::common::{
    clean_doi, date_parts, pages_to_bibtex, parse_month, split_pages, usable_key,
};
use papers_rs::formats::names::{
    Name, bibtex_name_of_ris, format_bibtex_names, parse_bibtex_names,
};
use papers_rs::formats::{Format, dumps, no_versions};
use serde_json::{Value, json};

mod common;
use common::{dir, json_file};

fn expected() -> Value {
    json_file(&dir("expected").join("formats.json"))
}

fn name_json(n: &Name) -> Value {
    let mut m = serde_json::Map::new();
    for (k, v) in [
        ("literal", &n.literal),
        ("family", &n.family),
        ("given", &n.given),
        ("non-dropping-particle", &n.non_dropping_particle),
        ("suffix", &n.suffix),
    ] {
        if let Some(v) = v {
            m.insert(k.to_string(), json!(v));
        }
    }
    Value::Object(m)
}

#[test]
fn names_as_papers() {
    let expected = expected();
    for case in expected["names"].as_array().unwrap() {
        let input = case["in"].as_str().unwrap();
        let names = parse_bibtex_names(input);
        let got: Vec<Value> = names.iter().map(name_json).collect();
        assert_eq!(Value::Array(got), case["names"], "names of {input:?}");
        assert_eq!(
            format_bibtex_names(&names),
            case["bibtex"].as_str().unwrap(),
            "bibtex of {input:?}"
        );
    }
    for case in expected["ris_names"].as_array().unwrap() {
        let input = case["in"].as_str().unwrap();
        assert_eq!(
            bibtex_name_of_ris(input),
            case["out"].as_str().unwrap(),
            "{input:?}"
        );
    }
}

#[test]
fn helpers_as_papers() {
    let expected = expected();
    let h = &expected["helpers"];
    for c in h["dates"].as_array().unwrap() {
        let got: Vec<i64> = date_parts(c["in"].as_str().unwrap())
            .iter()
            .map(|&n| n as i64)
            .collect();
        assert_eq!(json!(got), c["out"], "date {:?}", c["in"]);
    }
    for c in h["months"].as_array().unwrap() {
        let got = parse_month(c["in"].as_str().unwrap()).map_or(Value::Null, |n| json!(n as i64));
        assert_eq!(got, c["out"], "month {:?}", c["in"]);
    }
    for c in h["pages"].as_array().unwrap() {
        let input = c["in"].as_str().unwrap();
        assert_eq!(
            pages_to_bibtex(input),
            c["bibtex"].as_str().unwrap(),
            "pages {input:?}"
        );
        let (a, b) = split_pages(input);
        assert_eq!(json!([a, b]), c["split"], "split of {input:?}");
    }
    for c in h["dois"].as_array().unwrap() {
        assert_eq!(
            clean_doi(c["in"].as_str().unwrap()),
            c["out"].as_str().unwrap(),
            "doi {:?}",
            c["in"]
        );
    }
    for c in h["keys"].as_array().unwrap() {
        assert_eq!(
            usable_key(c["in"].as_str().unwrap()),
            c["out"].as_str().unwrap(),
            "key {:?}",
            c["in"]
        );
    }
}

fn files_of(case: &Value) -> Vec<String> {
    case["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn entries_as_papers() {
    let expected = expected();
    let cases = expected["entries"].as_array().unwrap();
    assert!(cases.len() > 500);
    let mut written = 0;
    for case in cases {
        let text = case["in"].as_str().unwrap();
        let bib = Bib::parse(text);
        let entries: Vec<_> = bib.entries().collect();
        assert_eq!(
            entries.len() as u64,
            case["count"].as_u64().unwrap(),
            "{text:?}"
        );
        if entries.is_empty() {
            continue;
        }
        let files = files_of(case);
        let one = &entries[..1];
        for (format, key) in [
            (Format::CslJson, "csl"),
            (Format::BibJson, "bibjson"),
            (Format::Ris, "ris"),
        ] {
            let got = dumps(one, format, &|_| files.clone(), &no_versions);
            assert_eq!(got, case[key].as_str().unwrap(), "{key} of {text:?}");
        }
        written += 1;
    }
    println!("{written} entries");
}

#[test]
fn several_and_none() {
    let expected = expected();
    let several = &expected["several"];
    let texts: Vec<Bib> = several["in"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| Bib::parse(t.as_str().unwrap()))
        .collect();
    let entries: Vec<_> = texts.iter().map(|b| b.entries().next().unwrap()).collect();
    for (format, key) in [
        (Format::CslJson, "csl"),
        (Format::BibJson, "bibjson"),
        (Format::Ris, "ris"),
    ] {
        let got = dumps(
            &entries,
            format,
            &|_| vec!["a/b.pdf".to_string()],
            &no_versions,
        );
        assert_eq!(got, several[key].as_str().unwrap(), "{key}");
        let none = dumps(&[], format, &|_| vec![], &no_versions);
        assert_eq!(
            none,
            expected["empty"][key].as_str().unwrap(),
            "{key} of no entry"
        );
    }
}

#[test]
fn imports_as_papers() {
    use papers_rs::formats::import::loads;
    let expected = expected();
    let cases = expected["imports"].as_array().unwrap();
    assert!(cases.len() > 1000);
    let (mut read, mut errors, mut crashes) = (0, 0, 0);
    for case in cases {
        let format = Format::from_id(case["fmt"].as_str().unwrap()).unwrap();
        let text = case["in"].as_str().unwrap();
        let got = loads(text, format);
        match case["class"].as_str() {
            None => {
                let entries = got.unwrap_or_else(|e| panic!("{format:?} {text:?}: {e}"));
                let dumped: Vec<Value> = entries
                    .iter()
                    .map(|e| {
                        json!({
                            "type": e.entry_type, "key": e.key,
                            "fields": e.fields.iter().map(|f| json!([f.key, f.value])).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                assert_eq!(Value::Array(dumped), case["entries"], "{format:?} {text:?}");
                read += 1;
            }
            // the errors that papers reports: the message is the same
            Some("ValueError" | "KeyError") => {
                let message = got
                    .err()
                    .unwrap_or_else(|| panic!("{format:?} {text:?}: an error was expected"));
                assert_eq!(
                    message,
                    case["error"].as_str().unwrap(),
                    "{format:?} {text:?}"
                );
                errors += 1;
            }
            // papers crashes (a name that is not a text...): nothing to follow
            Some(_) => crashes += 1,
        }
    }
    println!("{read} texts read, {errors} errors, {crashes} crashes of papers");
}

#[test]
fn fuzzy_scores_as_rapidfuzz() {
    use papers_rs::search::token_set_ratio;
    let expected = expected();
    for c in expected["scores"].as_array().unwrap() {
        let (a, b) = (c["a"].as_str().unwrap(), c["b"].as_str().unwrap());
        let cutoff = c["cutoff"].as_f64().unwrap_or(0.0);
        let want = c["score"].as_f64().unwrap();
        let got = token_set_ratio(a, b, cutoff);
        // rapidfuzz computes in single precision
        assert!(
            (got - want).abs() < 1e-4,
            "{a:?} {b:?} cutoff {cutoff}: {got} against {want}"
        );
    }
}

#[test]
fn valid_dois_as_papers() {
    use papers_rs::search::is_valid_doi;
    let expected = expected();
    for c in expected["dois"].as_array().unwrap() {
        let input = c["in"].as_str().unwrap();
        assert_eq!(
            is_valid_doi(input),
            c["out"].as_bool().unwrap(),
            "{input:?}"
        );
    }
}
