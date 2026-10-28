//! papers-rs reads and writes bibtex as papers-cli does: the fixtures of
//! `tests/fixtures/` against what the reference recorded in `tests/expected/`
//! (`tests/gen_expected.py`).

use std::path::PathBuf;

use papers_rs::bib::encoding::{
    family_names, format_file, parse_file, parse_keywords, standard_name,
};
use papers_rs::bib::latex_table::{latex_to_unicode, protect_uppercase, string_to_latex};
use papers_rs::bib::{Bib, read_file};
use serde_json::{Value, json};

mod common;
use common::{dir, dump, json_file};

/// The edits of `edit` in the generator.
fn edit(bib: &mut Bib) {
    for e in bib.entries_mut() {
        e.set("x-edited", "yes {braced} & \"quoted\"");
        e.remove("abstract");
        if let Some(title) = e.get("title").map(str::to_string) {
            e.set("title", format!("{title} (edited)"));
        }
    }
}

fn saved(bib: &Bib) -> String {
    bib.to_string()
}

#[test]
fn fixtures_read_and_written_as_papers() {
    let mut names = Vec::new();
    for f in std::fs::read_dir(dir("fixtures")).unwrap() {
        let path = f.unwrap().path();
        if path.extension().is_some_and(|e| e == "bib") {
            names.push(path.file_stem().unwrap().to_string_lossy().into_owned());
        }
    }
    names.sort();
    assert!(names.len() >= 10, "{names:?}");
    for name in names {
        let text = read_file(&dir("fixtures").join(format!("{name}.bib"))).unwrap();
        let expected = json_file(&dir("expected").join(format!("{name}.json")));
        let bib = Bib::parse(&text);

        let got = dump(&bib);
        assert_eq!(got["blocks"], expected["blocks"], "{name}: the blocks read");
        assert_eq!(
            got["unreadable"], expected["unreadable"],
            "{name}: the unreadable blocks"
        );

        assert_eq!(
            bib.format(),
            expected["formatted"].as_str().unwrap(),
            "{name}: format"
        );
        let saved_text =
            std::fs::read_to_string(dir("expected").join(format!("{name}.saved.bib"))).unwrap();
        assert_eq!(saved(&bib), saved_text, "{name}: the text of a save");

        let mut edited = bib.clone();
        edit(&mut edited);
        assert_eq!(
            saved(&edited),
            expected["edited_saved"].as_str().unwrap(),
            "{name}: edited"
        );

        // what papers saved is read and saved again as papers does (the same text, but for the unreadable)
        let twice = saved(&Bib::parse(&saved_text));
        assert_eq!(
            twice,
            expected["saved_twice"].as_str().unwrap(),
            "{name}: saved twice"
        );
        if bib.unreadable().is_empty() {
            assert_eq!(twice, saved_text, "{name}: a saved text is stable");
        }
    }
}

#[test]
fn varied_is_the_expected_of_papers_tests() {
    // tests/sortformat/varied.expected.bib of papers
    let text = read_file(&dir("fixtures").join("varied.bib")).unwrap();
    let expected = std::fs::read_to_string(dir("expected").join("varied.saved.bib")).unwrap();
    assert_eq!(Bib::parse(&text).to_string(), expected);
    assert!(expected.starts_with(
        "% a comment at the top of the file, before anything\n\n@string{jgr = {Journal of"
    ));
}

fn fuzz_cases() -> Vec<Value> {
    let path = std::env::var("PAPERS_RS_FUZZ")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dir("expected").join("fuzz.json"));
    json_file(&path).as_array().unwrap().clone()
}

#[test]
fn edited_texts_read_and_written_as_papers() {
    let cases = fuzz_cases();
    assert!(cases.len() >= 100);
    for (i, case) in cases.iter().enumerate() {
        // (papers reads a text piece by piece with its cache: a text with a line that starts
        // with @ and is not a block can be cut in two comments there, `cache_agrees` is false)
        let text = case["text"].as_str().unwrap();
        let bib = Bib::parse(text);
        let got = dump(&bib);
        let entries: Vec<&Value> = got["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|b| b["kind"] == "entry")
            .collect();
        assert_eq!(
            json!(entries),
            case["entries"],
            "case {i}: entries of {text:?}"
        );
        assert_eq!(
            got["unreadable"], case["unreadable"],
            "case {i}: unreadable of {text:?}"
        );
        assert_eq!(
            saved(&bib),
            case["saved"].as_str().unwrap(),
            "case {i}: saved text of {text:?}"
        );
    }
}

#[test]
fn latex_and_unicode_as_papers() {
    let cases = json_file(&dir("expected").join("latexenc.json"));
    let cases = cases.as_array().unwrap();
    assert!(cases.len() > 500);
    for c in cases {
        let s = c["in"].as_str().unwrap();
        assert_eq!(
            string_to_latex(s),
            c["to_latex"].as_str().unwrap(),
            "string_to_latex({s:?})"
        );
        assert_eq!(
            latex_to_unicode(s),
            c["to_unicode"].as_str().unwrap(),
            "latex_to_unicode({s:?})"
        );
        assert_eq!(
            protect_uppercase(s),
            c["protect"].as_str().unwrap(),
            "protect_uppercase({s:?})"
        );
    }
}

#[test]
fn names_keywords_and_files_as_papers() {
    let cases = json_file(&dir("expected").join("encoding.json"));
    let strs = |v: &Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect()
    };
    for c in cases["names"].as_array().unwrap() {
        let s = c["in"].as_str().unwrap();
        assert_eq!(
            standard_name(s),
            c["standard"].as_str().unwrap(),
            "standard_name({s:?})"
        );
        assert_eq!(family_names(s), strs(&c["family"]), "family_names({s:?})");
    }
    for c in cases["keywords"].as_array().unwrap() {
        let s = c["in"].as_str().unwrap();
        assert_eq!(
            parse_keywords(s),
            strs(&c["parse"]),
            "parse_keywords({s:?})"
        );
    }
    for c in cases["files"].as_array().unwrap() {
        let s = c["in"].as_str().unwrap();
        assert_eq!(parse_file(s), strs(&c["parse"]), "parse_file({s:?})");
    }
    for c in cases["format_file"].as_array().unwrap() {
        let l = strs(&c["in"]);
        assert_eq!(
            format_file(&l),
            c["out"].as_str().unwrap(),
            "format_file({l:?})"
        );
    }
}
