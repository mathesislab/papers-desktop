//! The fixes of `papers check` (`command::check`: `Biblio.fix_entry`) and the diff it shows
//! (`entry_diff`, difflib's `ndiff`), against what papers-cli did (`tests/gen_check.py`,
//! `expected/check.json`): 400 entries with the options of `fix_entry` and a mocked crossref, and
//! the diffs of 150 pairs of entries.

use std::cell::RefCell;

use papers_rs::bib::{Bib, Entry, Field};
use papers_rs::command::check::{
    Encoding, Fetch, FixOptions, entry_diff, fix_entry, update_header,
};
use papers_rs::library::Library;
use papers_rs::names::Format;
use serde_json::Value;

fn entry_of(j: &Value) -> Entry {
    let mut e = Entry::new(j["type"].as_str().unwrap(), j["key"].as_str().unwrap());
    for f in j["fields"].as_array().unwrap() {
        e.fields
            .push(Field::new(f[0].as_str().unwrap(), f[1].as_str().unwrap()));
    }
    e
}

/// The fields of an entry, as (name, value) sorted: the order of the fields is not what is written.
fn shape(e: &Entry) -> (String, String, Vec<(String, String)>) {
    let mut fields: Vec<(String, String)> = e
        .fields
        .iter()
        .map(|f| (f.key.clone(), f.value.clone()))
        .collect();
    fields.sort();
    (e.entry_type.clone(), e.key.clone(), fields)
}

struct Recorded(Option<std::result::Result<String, String>>);

impl Fetch for Recorded {
    fn by_doi(&self, _: &str) -> std::result::Result<String, String> {
        self.0.clone().unwrap_or(Err("none".into()))
    }

    fn by_title_and_authors(&self, _: &str, _: &str) -> std::result::Result<String, String> {
        self.0.clone().unwrap_or(Err("none".into()))
    }
}

fn expected() -> Value {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/expected/check.json"
    ))
    .unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn the_fixes_are_those_of_papers() {
    let all = expected();
    let mut failures = Vec::new();
    for (n, case) in all["fix"].as_array().unwrap().iter().enumerate() {
        let bib = Bib::parse(case["bibtex"].as_str().unwrap());
        let mut library = Library::new(
            "/x/papers.bib",
            None,
            Some("/x"),
            Format::name(),
            Format::key(),
        );
        for e in bib.entries() {
            library.add_unchecked(e.clone());
        }
        let keys: Vec<String> = library.entries().map(|e| e.key.clone()).collect();
        let key = &keys[case["index"].as_u64().unwrap() as usize];
        let o = &case["options"];
        let flag = |name: &str| o[name].as_bool().unwrap();
        let options = FixOptions {
            fix_doi: flag("fix_doi"),
            fetch: flag("fetch"),
            fetch_all: flag("fetch_all"),
            fix_key: flag("fix_key"),
            auto_key: flag("auto_key"),
            key_ascii: flag("key_ascii"),
            encoding: o["encoding"].as_str().and_then(Encoding::parse),
            format_name: flag("format_name"),
            interactive: flag("interactive"),
        };
        let fetched = match case["fetched"].as_object() {
            None => Recorded(None),
            Some(f) if f.contains_key("error") => {
                Recorded(Some(Err(f["error"].as_str().unwrap().to_string())))
            }
            Some(f) => Recorded(Some(Ok(f["text"].as_str().unwrap().to_string()))),
        };
        let answer = case["answer"].as_bool().unwrap();
        let shown: RefCell<String> = RefCell::new(String::new());
        let mut confirm = |old: &Entry, new: &Entry| {
            *shown.borrow_mut() = format!(
                "{}\n{}\n",
                update_header(false),
                entry_diff(old, new, false)
            );
            Ok(answer)
        };
        let want = &case["expected"];
        let result = fix_entry(&mut library, key, &options, Some(&fetched), &mut confirm);
        let got = library
            .get(&result.expect("no error").expect("the entry"))
            .expect("the entry")
            .clone();
        let after = entry_of(&want["after"]);
        if shape(&got) != shape(&after) || *shown.borrow() != want["printed"].as_str().unwrap() {
            failures.push(format!(
                "case {n}: {:?}\n  options {o}\n  fetched {}\n  got  {:?}\n  want {:?}\n  shown {:?}\n  want  {:?}",
                case["bibtex"],
                case["fetched"],
                shape(&got),
                shape(&after),
                shown.borrow(),
                want["printed"]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of 400 differ:\n{}",
        failures.len(),
        failures[..failures.len().min(3)].join("\n")
    );
}

#[test]
fn the_diffs_are_those_of_difflib() {
    let all = expected();
    for (n, case) in all["diffs"].as_array().unwrap().iter().enumerate() {
        let (old, new) = (entry_of(&case["old"]), entry_of(&case["new"]));
        assert_eq!(
            entry_diff(&old, &new, false),
            case["diff"].as_str().unwrap(),
            "diff {n}"
        );
        assert_eq!(
            entry_diff(&old, &new, true),
            case["diff_color"].as_str().unwrap(),
            "colored diff {n}"
        );
    }
}
