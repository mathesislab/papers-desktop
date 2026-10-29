//! Whether two entries are the same paper, and the merge of those that are: papers-rs against
//! what papers-cli gave (`tests/gen_library_expected.py`, `expected/library/identity.json` and
//! `merge.json`).

use papers_rs::bib::{Bib, Entry};
use papers_rs::identity::{
    Distinct, IdKind, Identifiers, IdentityIndex, Kind, Verdict, arxiv_of_doi, duplicate_groups,
    entry_arxiv, entry_identifiers, family_name_ids, find_duplicates, normalize_arxiv,
    normalize_doi, normalize_isbn, normalize_text, parse_identifier, parse_name, same_paper,
    split_names, strip_doi,
};
use papers_rs::library::Library;
use papers_rs::merge::{Merge, Status, equivalent, names_compatible};
use papers_rs::names::Format;
use serde_json::{Value, json};

mod common;
use common::library_json;
use common::tally::Tally;

fn entries_of(text: &str) -> Vec<Entry> {
    Bib::parse(text).entries().cloned().collect()
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Same => "same",
        Kind::Different => "different",
        Kind::Ask => "ask",
    }
}

fn verdict_json(v: &Verdict) -> Value {
    json!([kind_name(v.kind), v.reason])
}

fn opt(s: &str) -> Option<&str> {
    (!s.is_empty()).then_some(s)
}

#[test]
fn identifiers_as_papers() {
    let j = library_json("identity.json");
    let text = j["bibtex"].as_str().unwrap();
    let entries = entries_of(text);
    let mut tally = Tally::default();
    assert_eq!(entries.len(), j["identifiers"].as_array().unwrap().len());
    for (e, d) in entries.iter().zip(j["identifiers"].as_array().unwrap()) {
        let ids = Identifiers::of(e);
        let own = format!("{} {}", common_text(e), e.key);
        tally.check(
            "type",
            &own,
            ids.entry_type.as_str(),
            d["type"].as_str().unwrap(),
        );
        tally.check("doi", &own, ids.doi.as_str(), d["doi"].as_str().unwrap());
        tally.check(
            "arxiv",
            &own,
            ids.arxiv.as_str(),
            d["arxiv"].as_str().unwrap(),
        );
        tally.check(
            "entry_arxiv",
            &own,
            entry_arxiv(e),
            d["entry_arxiv"].as_str().unwrap().to_string(),
        );
        let isbns: Vec<String> = ids.isbns.iter().cloned().collect();
        tally.check("isbns", &own, isbns, common::strings(&d["isbns"]));
        tally.check(
            "author",
            &own,
            ids.author.as_str(),
            d["author"].as_str().unwrap(),
        );
        tally.check(
            "title",
            &own,
            ids.title.as_str(),
            d["title"].as_str().unwrap(),
        );
        tally.check(
            "authortitle",
            &own,
            ids.author_title.as_str(),
            d["authortitle"].as_str().unwrap(),
        );
        tally.check(
            "identity",
            &own,
            ids.identity().to_json(),
            d["identity"].clone(),
        );
    }
    tally.report("identifiers");
}

fn common_text(e: &Entry) -> String {
    e.fields
        .iter()
        .map(|f| f.value.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn distinct_of(j: &Value) -> Distinct {
    let mut d = Distinct::new();
    for p in j["distinct_pairs"].as_array().unwrap() {
        d.pairs.push([p[0].clone(), p[1].clone()]);
    }
    d
}

#[test]
fn same_paper_as_papers() {
    let j = library_json("identity.json");
    let entries = entries_of(j["bibtex"].as_str().unwrap());
    let distinct = distinct_of(&j);
    let mut tally = Tally::default();
    let mut kinds = std::collections::HashSet::new();
    for (key, d) in [("pairs", None), ("pairs_distinct", Some(&distinct))] {
        for case in j[key].as_array().unwrap() {
            let (a, b) = (
                case[0].as_u64().unwrap() as usize,
                case[1].as_u64().unwrap() as usize,
            );
            let got = same_paper(&entries[a], &entries[b], d);
            let text = format!("{} {}", common_text(&entries[a]), common_text(&entries[b]));
            tally.check(key, &text, verdict_json(&got), case[2].clone());
            kinds.insert(kind_name(got.kind));
        }
    }
    tally.report("same_paper");
    assert_eq!(kinds.len(), 3);
    // distinct.json is written as papers writes it
    let mut written = distinct.clone();
    written.changed = true;
    tally.check(
        "distinct",
        "",
        written.to_text(),
        j["distinct_text"].as_str().unwrap().to_string(),
    );
}

#[test]
fn finding_duplicates_as_papers() {
    let j = library_json("identity.json");
    let text = j["bibtex"].as_str().unwrap();
    let entries = entries_of(text);
    let distinct = distinct_of(&j);
    let index = IdentityIndex::of(&entries);
    let mut tally = Tally::default();
    for (name, d) in [("find", None), ("find_distinct", Some(&distinct))] {
        for (i, expected) in j[name].as_array().unwrap().iter().enumerate() {
            let found = find_duplicates(&entries[i], &index, d, Some(i as u64));
            let got: Vec<Value> = found
                .iter()
                .map(|(h, v)| json!([h, kind_name(v.kind), v.reason]))
                .collect();
            tally.check(name, text, Value::Array(got), expected.clone());
            // the result is that of comparing with every entry
            let mut all: Vec<(usize, Verdict)> = (0..entries.len())
                .filter(|&k| k != i)
                .map(|k| (k, same_paper(&entries[k], &entries[i], d)))
                .filter(|(_, v)| v.kind != Kind::Different)
                .collect();
            all.sort_by_key(|(k, v)| (v.kind != Kind::Same, *k));
            let by_all: Vec<Value> = all
                .iter()
                .map(|(k, v)| json!([k, kind_name(v.kind), v.reason]))
                .collect();
            let by_index: Vec<Value> = found
                .iter()
                .map(|(h, v)| json!([h, kind_name(v.kind), v.reason]))
                .collect();
            assert_eq!(
                by_index, by_all,
                "the index finds what comparing with every entry finds"
            );
        }
    }
    let refs: Vec<&Entry> = entries.iter().collect();
    for (name, d) in [("groups", None), ("groups_distinct", Some(&distinct))] {
        let got: Vec<Value> = duplicate_groups(&refs, d)
            .iter()
            .map(|g| json!([g.entries, kind_name(g.kind), g.reason]))
            .collect();
        tally.check(name, text, Value::Array(got.clone()), j[name].clone());
        assert!(!got.is_empty());
    }
    tally.report("duplicates");
}

#[test]
fn normalization_as_papers() {
    let j = library_json("identity.json");
    let mut tally = Tally::default();
    for c in j["norm"].as_array().unwrap() {
        let t = c[0].as_str().unwrap();
        tally.check(
            "normalize_text",
            t,
            normalize_text(t),
            c[1].as_str().unwrap().to_string(),
        );
    }
    for c in j["split_names"].as_array().unwrap() {
        let t = c[0].as_str().unwrap();
        assert_eq!(split_names(t), common::strings(&c[1]), "{t:?}");
    }
    for c in j["parse_name"].as_array().unwrap() {
        let t = c[0].as_str().unwrap();
        let (family, given) = parse_name(t);
        assert_eq!(json!([family, given]), c[1], "{t:?}");
    }
    for c in j["family_name_ids"].as_array().unwrap() {
        let t = c[0].as_str().unwrap();
        tally.check(
            "family_name_ids",
            t,
            family_name_ids(t),
            common::strings(&c[1]),
        );
    }
    for c in j["doi"].as_array().unwrap() {
        let t = c[0].as_str().unwrap();
        assert_eq!(strip_doi(t).as_deref(), c[1].as_str(), "{t:?}");
        assert_eq!(normalize_doi(t).as_deref(), c[2].as_str(), "{t:?}");
        assert_eq!(arxiv_of_doi(t).as_deref(), c[3].as_str(), "{t:?}");
    }
    for c in j["arxiv"].as_array().unwrap() {
        let t = c[0].as_str().unwrap();
        assert_eq!(normalize_arxiv(t).as_deref(), c[1].as_str(), "{t:?}");
    }
    for c in j["isbn"].as_array().unwrap() {
        let t = c[0].as_str().unwrap();
        assert_eq!(normalize_isbn(t).as_deref(), c[1].as_str(), "{t:?}");
    }
    for c in j["parse_identifier"].as_array().unwrap() {
        let t = c[0].as_str().unwrap();
        match parse_identifier(t) {
            Ok((kind, v)) => {
                let kind = match kind {
                    IdKind::Key => "key",
                    IdKind::Doi => "doi",
                    IdKind::Arxiv => "arxiv",
                    IdKind::Isbn => "isbn",
                };
                assert_eq!(json!([kind, v]), c[1], "{t:?}");
            }
            Err(e) => assert_eq!(e, c[1]["error"].as_str().unwrap(), "{t:?}"),
        }
    }
    tally.report("normalization");
    assert!(opt("x").is_some());
}

#[test]
fn naming_entries_as_papers() {
    let j = library_json("identity.json");
    let entries = entries_of(j["bibtex"].as_str().unwrap());
    for (e, c) in entries
        .iter()
        .zip(j["entry_identifiers"].as_array().unwrap())
    {
        let mut got: Vec<Value> = entry_identifiers(e)
            .into_iter()
            .map(|(k, v)| {
                let k = match k {
                    IdKind::Key => "key",
                    IdKind::Doi => "doi",
                    IdKind::Arxiv => "arxiv",
                    IdKind::Isbn => "isbn",
                };
                json!([k, v])
            })
            .collect();
        got.sort_by_key(|v| v.to_string());
        let mut expected = c[1].as_array().unwrap().clone();
        expected.sort_by_key(|v| v.to_string());
        assert_eq!(got, expected, "{}", e.key);
    }
    let mut library = Library::new(
        "/lib/papers.bib",
        None,
        Some("/lib"),
        Format::name(),
        Format::key(),
    );
    let mut resolver = papers_rs::merge::Resolver::new(papers_rs::merge::OnDuplicate::Skip);
    library
        .add_entries(
            entries,
            None,
            &[],
            "bibtex",
            &Default::default(),
            &mut resolver,
        )
        .unwrap();
    for c in j["select"].as_array().unwrap() {
        let args = common::strings(&c[0]);
        match library.select_some(&args) {
            Ok((found, missing)) => {
                assert_eq!(json!([found, missing]), json!([c[1], c[2]]), "{args:?}");
            }
            Err(e) => assert_eq!(e.message(), c[1]["error"].as_str().unwrap(), "{args:?}"),
        }
    }
}

#[test]
fn merge_as_papers() {
    let j = library_json("merge.json");
    let mut tally = Tally::default();
    let status = |s: Status| match s {
        Status::Same => " ",
        Status::Added => "+",
        Status::Conflict => "!",
    };
    let formatted = |entries: &[Entry]| {
        let mut bib = Bib::default();
        bib.blocks
            .extend(entries.iter().cloned().map(papers_rs::bib::Block::Entry));
        bib.format()
    };
    for case in j["cases"].as_array().unwrap() {
        let text = case["bibtex"].as_str().unwrap();
        let entries = entries_of(text);
        let rel = case["relative_to"].as_str();
        let keys_to_ids = case["keys_to_ids"].as_bool().unwrap();
        let merge = Merge::new(&entries, rel, keys_to_ids);
        let fields: Vec<Value> = merge
            .fields
            .iter()
            .map(|f| {
                json!([
                    f.name,
                    status(f.status),
                    f.values,
                    f.value(),
                    f.alternatives()
                ])
            })
            .collect();
        tally.check("fields", text, Value::Array(fields), case["fields"].clone());
        assert_eq!(merge.files, common::strings(&case["files"]), "{text}");
        assert_eq!(merge.new_files, common::strings(&case["new_files"]));
        assert_eq!(merge.ids, common::strings(&case["ids"]));
        assert_eq!(merge.ids_added, common::strings(&case["ids_added"]));
        assert_eq!(merge.is_exact(), case["exact"].as_bool().unwrap());
        tally.check(
            "result",
            text,
            formatted(&[merge.result()]),
            case["result"].as_str().unwrap().to_string(),
        );
        for choice in 0..2 {
            let mut m = Merge::new(&entries, rel, keys_to_ids);
            m.choose_all(choice);
            tally.check(
                "choose",
                text,
                formatted(&[m.result()]),
                case[format!("choose{choice}")]
                    .as_str()
                    .unwrap()
                    .to_string(),
            );
        }
        if entries.len() > 1 {
            let m = Merge::new(&entries, rel, keys_to_ids).with_target(1);
            tally.check(
                "with_target",
                text,
                formatted(&[m.result()]),
                case["with_target1"].as_str().unwrap().to_string(),
            );
        }
    }
    for c in j["names"].as_array().unwrap() {
        let (a, b) = (c[0].as_str().unwrap(), c[1].as_str().unwrap());
        assert_eq!(
            names_compatible(a, b),
            c[2].as_bool().unwrap(),
            "{a:?} {b:?}"
        );
        assert_eq!(equivalent("author", a, b), c[4].as_bool().unwrap());
    }
    for c in j["equivalent"].as_array().unwrap() {
        let (name, a, b) = (
            c[0].as_str().unwrap(),
            c[1].as_str().unwrap(),
            c[2].as_str().unwrap(),
        );
        tally.check(
            "equivalent",
            &format!("{a} {b}"),
            equivalent(name, a, b),
            c[3].as_bool().unwrap(),
        );
    }
    tally.report("merge");
}
