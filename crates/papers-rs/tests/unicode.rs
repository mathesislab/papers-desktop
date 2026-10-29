//! The LaTeX of the values as unicode, as papers does it with pylatexenc, against what the
//! reference recorded in `tests/expected/unicode.json` (`tests/gen_unicode.py`).

use papers_rs::bib::Bib;
use papers_rs::bib::unicode::{
    convert_entry_to_unicode, entry_unicode_fields, latex_to_unicode_library, value_to_unicode,
};
use serde_json::{Value, json};

mod common;
use common::{dir, dump};

/// The recorded results, `{TODAY}` (the date of `\maketitle` without `\date`, which the
/// generator replaces) as today's date.
fn expected() -> Value {
    let text = std::fs::read_to_string(dir("expected").join("unicode.json")).unwrap();
    serde_json::from_str(&text.replace("{TODAY}", &latexenc::latex2text::latex_today())).unwrap()
}

#[test]
fn libraries_decoded_as_papers() {
    let expected = expected();
    let cases = expected["libraries"].as_array().unwrap();
    assert!(cases.len() > 300);
    let mut errors = 0;
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let text = case["in"].as_str().unwrap();
        let bib = latex_to_unicode_library(Bib::parse(text));
        let got = dump(&bib);
        assert_eq!(got["blocks"], case["blocks"], "{name}: blocks of {text:?}");
        assert_eq!(
            got["unreadable"], case["unreadable"],
            "{name}: unreadable of {text:?}"
        );
        assert_eq!(
            bib.to_string(),
            case["saved"].as_str().unwrap(),
            "{name}: saved text of {text:?}"
        );
        errors += case["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|b| b["class"] == "MiddlewareErrorBlock")
            .count();
    }
    assert!(errors >= 10, "{errors}");
    println!(
        "libraries: {} cases ({errors} blocks with an error)",
        cases.len()
    );
}

#[test]
fn entries_converted_as_papers() {
    let expected = expected();
    let cases = expected["entries"].as_array().unwrap();
    assert!(cases.len() > 200);
    let mut errors = 0;
    for case in cases {
        let text = case["in"].as_str().unwrap();
        let mut bib = Bib::parse(text);
        let entry = bib.entries_mut().next().unwrap();
        let result = convert_entry_to_unicode(entry);
        assert_eq!(
            result.is_err(),
            case.get("error").is_some(),
            "error of {text:?}"
        );
        errors += usize::from(result.is_err());
        let fields: Vec<Value> = entry
            .fields
            .iter()
            .map(|f| json!([f.key, f.value, f.verbatim]))
            .collect();
        assert_eq!(json!(fields), case["fields"], "fields of {text:?}");
    }
    println!("entries: {} cases ({errors} errors)", cases.len());
}

/// Both use the converter of the process, whose state stays from one call to the next as in
/// papers: the sequence is that of the generator, and nothing else in this file uses it.
#[test]
fn values_and_fields_as_papers() {
    let expected = expected();
    let values = expected["values"].as_array().unwrap();
    assert!(values.len() > 1000);
    for case in values {
        let value = case["in"].as_str().unwrap();
        assert_eq!(
            value_to_unicode(value),
            case["out"].as_str().unwrap(),
            "value_to_unicode({value:?})"
        );
    }
    let cases = expected["fields"].as_array().unwrap();
    assert!(cases.len() > 100);
    for case in cases {
        let text = case["in"].as_str().unwrap();
        let bib = Bib::parse(text);
        let entry = bib
            .entries()
            .nth(case["index"].as_u64().unwrap() as usize)
            .unwrap();
        let got: Vec<Value> = entry_unicode_fields(entry)
            .iter()
            .map(|(k, v)| json!([k, v]))
            .collect();
        assert_eq!(json!(got), case["out"], "entry_unicode_fields of {text:?}");
    }
    println!(
        "values: {}, entries of fields: {}",
        values.len(),
        cases.len()
    );
}
