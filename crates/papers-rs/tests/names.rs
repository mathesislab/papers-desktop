//! The names papers gives (keys, file names), tags: papers-rs against what papers-cli gave
//! (`tests/gen_library_expected.py`, `expected/library/names.json`). Where the transliteration
//! of papers (Unidecode) and of papers-rs (deunicode) differ, the difference is counted, not
//! failed (`expected/library/translit.json`).

use papers_rs::entry::{
    add_tags, fnmatch, remove_tags, rename_tag, tag_counts, tag_matches, tag_parents, tags,
};
use papers_rs::names::{
    Format, FormatKind, append_abc, ascii, attachment_names, check_key, list_tag, path_safe,
    slugify, target_in,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

mod common;
use common::translit::{Tally, translit};
use common::{entry_of, library_json, strings};

#[test]
fn the_transliteration_differences_are_the_recorded_ones() {
    let j = library_json("translit.json");
    let mut differing = 0;
    for d in j["differences_below_3000"].as_array().unwrap() {
        let c = char::from_u32(d[0].as_u64().unwrap() as u32).unwrap();
        let (unidecode, ours) = (d[1].as_str().unwrap(), d[2].as_str().unwrap());
        assert_eq!(
            ascii(&c.to_string()),
            ours,
            "{c:?}: the table of deunicode changed"
        );
        assert_ne!(ours, unidecode);
        differing += 1;
    }
    let mut digest = Sha256::new();
    let mut agree = 0;
    for cp in 0x80u32..0x110000 {
        let Some(c) = char::from_u32(cp) else {
            continue;
        };
        if translit().differs(c) {
            if cp >= 0x3000 {
                differing += 1;
            }
            continue;
        }
        let out = ascii(&c.to_string());
        if !out.is_empty() {
            digest.update(format!("{cp:x}:{out}\n"));
            agree += 1;
        }
    }
    assert_eq!(agree, j["agree_count"].as_u64().unwrap());
    assert_eq!(differing as u64, j["difference_count"].as_u64().unwrap());
    let hex: String = digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        hex,
        j["agree_digest"].as_str().unwrap(),
        "the transliteration changed where it agreed with Unidecode"
    );
    // the letters of the Latin scripts that names are made of agree
    for c in "ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏÐÑÒÓÔÕÖØÙÚÛÜÝÞßàáâãäåæçèéêëìíîïðñòóôõöøùúûüýþÿĀāĂăĄąĆćČčĎďĐđĒēĘęĚěĞğĪīİıŁłŃńŇňŌōŐőŒœŘřŚśŞşŠšŢţŤťŪūŮůŰűŹźŻżŽž".chars() {
        assert!(!translit().differs(c), "{c}");
    }
}

#[test]
fn slugify_as_papers() {
    let mut tally = Tally::default();
    for case in library_json("names.json")["slugify"].as_array().unwrap() {
        let (text, expected) = (case[0].as_str().unwrap(), case[1].as_str().unwrap());
        tally.check("slugify", text, slugify(text), expected.to_string());
    }
    tally.report("slugify");
    assert!(tally.cases > 1000 && tally.known < tally.cases / 4);
}

#[test]
fn small_functions_as_papers() {
    let j = library_json("names.json");
    for case in j["path_safe"].as_array().unwrap() {
        assert_eq!(
            path_safe(case[0].as_str().unwrap()),
            case[1].as_str().unwrap()
        );
    }
    for case in j["listtag"].as_array().unwrap() {
        let words = strings(&case[0]);
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        let n = |i: usize| case[i].as_i64().unwrap();
        assert_eq!(
            list_tag(&words, n(1), n(2), n(3), case[4].as_str().unwrap()),
            case[5].as_str().unwrap(),
            "{case}"
        );
    }
    for case in j["append_abc"].as_array().unwrap() {
        let keys = strings(&case[1]);
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        let got = append_abc(case[0].as_str().unwrap(), &keys);
        match &case[2] {
            Value::String(s) => assert_eq!(got.unwrap(), *s, "{case}"),
            e => assert_eq!(got.unwrap_err(), e["error"].as_str().unwrap(), "{case}"),
        }
    }
    for case in j["valid_key"].as_array().unwrap() {
        let key = case[0].as_str().unwrap();
        assert_eq!(
            check_key(key).is_none(),
            case[1].as_bool().unwrap(),
            "{key:?}"
        );
    }
    for case in j["attachment_names"].as_array().unwrap() {
        assert_eq!(
            attachment_names(&strings(&case[0]), case[1].as_str().unwrap()),
            strings(&case[2]),
            "{case}"
        );
    }
    for case in j["target_in"].as_array().unwrap() {
        let got = target_in(case[0].as_str().unwrap(), case[1].as_str().unwrap());
        assert_eq!(got.as_deref(), case[2].as_str(), "{case}");
    }
}

#[test]
fn templates_as_papers() {
    let mut tally = Tally::default();
    for case in library_json("names.json")["format"].as_array().unwrap() {
        let kind = if case["kind"] == "key" {
            FormatKind::Key
        } else {
            FormatKind::Name
        };
        let mut f = if case["defaults"].as_bool().unwrap_or(false) {
            if kind == FormatKind::Key {
                Format::key()
            } else {
                Format::name()
            }
        } else {
            // `Format(template, **options)`: the defaults of the class, not those of the settings
            let mut f = Format::name();
            f.kind = kind;
            f.unknown_template = None;
            f
        };
        f.template = case["template"].as_str().unwrap().to_string();
        let o = &case["options"];
        if let Some(v) = o["author_num"].as_i64() {
            f.author_num = v;
        }
        if let Some(v) = o["author_sep"].as_str() {
            f.author_sep = v.to_string();
        }
        if let Some(v) = o["title_word_num"].as_i64() {
            f.title_word_num = v;
        }
        if let Some(v) = o["title_word_size"].as_i64() {
            f.title_word_size = v;
        }
        if let Some(v) = o["title_length"].as_i64() {
            f.title_length = v;
        }
        if let Some(v) = o["title_sep"].as_str() {
            f.title_sep = v.to_string();
        }
        if let Some(v) = o["unknown_strict"].as_bool() {
            f.unknown_strict = v;
        }
        if o.get("unknown_template").is_some() {
            f.unknown_template = o["unknown_template"].as_str().map(str::to_string);
        }
        let entry = entry_of(&case["entry"]);
        let got = f.render(&entry);
        let text: String = entry
            .fields
            .iter()
            .map(|x| x.value.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        match (case["out"].as_str(), case["error"].as_str()) {
            (Some(out), None) => match got {
                Ok(got) => tally.check("template", &text, got, out.to_string()),
                Err(e) => panic!("{case}: {e}"),
            },
            (None, Some(error)) if error.starts_with("PYTHON") => {
                assert!(got.is_err(), "{case}: papers fails, we gave {got:?}");
            }
            (None, Some(error)) => {
                let got = got.expect_err(&case.to_string());
                assert_eq!(got.0, error, "{case}");
            }
            _ => unreachable!(),
        }
    }
    tally.report("templates");
}

#[test]
fn tags_as_papers() {
    let j = library_json("names.json");
    for case in j["tag_matches"].as_array().unwrap() {
        let (tag, pattern) = (case[0].as_str().unwrap(), case[1].as_str().unwrap());
        assert_eq!(
            tag_matches(tag, pattern),
            case[2].as_bool().unwrap(),
            "{tag:?} {pattern:?}"
        );
    }
    for case in j["tag_parents"].as_array().unwrap() {
        assert_eq!(tag_parents(case[0].as_str().unwrap()), strings(&case[1]));
    }
    let make = |kw: &Value| {
        let mut e = papers_rs::bib::Entry::new("article", "k");
        if let Some(k) = kw.as_str() {
            e.set("keywords", k);
        }
        e
    };
    let text = |e: &papers_rs::bib::Entry| e.get("keywords").map(str::to_string);
    for case in j["tag_ops"].as_array().unwrap() {
        let mut e = make(&case["keywords"]);
        rename_tag(
            &mut e,
            case["rename"][0].as_str().unwrap(),
            case["rename"][1].as_str().unwrap(),
        );
        assert_eq!(text(&e).as_deref(), case["rename"][2].as_str(), "{case}");
        let mut e = make(&case["keywords"]);
        add_tags(&mut e, &strings(&case["add"][0]));
        assert_eq!(text(&e).as_deref(), case["add"][1].as_str(), "{case}");
        let mut e = make(&case["add"][1]);
        remove_tags(&mut e, &strings(&case["remove"][0]));
        assert_eq!(text(&e).as_deref(), case["remove"][1].as_str(), "{case}");
    }
    let entries: Vec<_> = j["tag_counts"]["keywords"]
        .as_array()
        .unwrap()
        .iter()
        .map(make)
        .collect();
    let mut got: Vec<(String, usize)> = tag_counts(&entries);
    got.sort();
    let expected: Vec<(String, usize)> = j["tag_counts"]["counts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c[0].as_str().unwrap().to_string(),
                c[1].as_u64().unwrap() as usize,
            )
        })
        .collect();
    assert_eq!(got, expected);
    assert!(tags(&make(&Value::Null)).is_empty());
    assert!(fnmatch("a/b", "a/*"));
}
