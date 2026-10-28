//! The three-way merge (and the two-way merge of an exchange) as papers does it: base, target and
//! incoming entries, random and hand-made, and what `papers.merge.Merge` makes of them
//! (`expected/merge3.json`, made by `gen_merge3.py`): the fields (status, values, value, the
//! other values, where they changed, removals), the links and the former keys, the result, with
//! each side chosen, with the additions declined. The dialogue and the resolver on the same cases
//! are tested in `papers-rs-cli` (`src/dialogue.rs`). A larger run:
//! `PAPERS_RS_MERGE3_FUZZ=FILE cargo test -p papers-rs --test merge3 fuzz_file`.

use papers_rs::bib::{Bib, Block, Entry};
use papers_rs::merge::{
    Changed, Context, Decision, Merge, OnDuplicate, Resolve, Resolver, Status, apply_entry,
};
use serde_json::{Value, json};

fn entry(v: &Value) -> Entry {
    let mut e = Entry::new(v["type"].as_str().unwrap(), v["key"].as_str().unwrap());
    for f in v["fields"].as_array().unwrap() {
        e.set(f[0].as_str().unwrap(), f[1].as_str().unwrap());
    }
    e
}

fn formatted(e: &Entry) -> String {
    let mut bib = Bib::default();
    bib.blocks.push(Block::Entry(e.clone()));
    bib.format()
}

fn make(case: &Value) -> Merge {
    let target = entry(&case["target"]);
    let other = entry(&case["other"]);
    let rel = case["relative_to"].as_str();
    match case["mode"].as_str().unwrap() {
        "three" => Merge::exchange(target, other, rel, Some(&entry(&case["base"]))),
        "exchange" => Merge::exchange(target, other, rel, None),
        _ => Merge::new(
            &[target, other],
            rel,
            case["keys_to_ids"].as_bool().unwrap(),
        ),
    }
}

fn status(s: Status) -> &'static str {
    match s {
        Status::Same => " ",
        Status::Added => "+",
        Status::Conflict => "!",
    }
}

fn changed(c: Option<Changed>) -> Value {
    match c {
        Some(Changed::Target) => json!("target"),
        Some(Changed::Other) => json!("other"),
        Some(Changed::Both) => json!("both"),
        None => Value::Null,
    }
}

fn names<'a>(fields: impl Iterator<Item = &'a papers_rs::merge::FieldMerge>) -> Vec<String> {
    fields.map(|f| f.name.clone()).collect()
}

fn check(case: &Value) -> Vec<String> {
    let mut failures = Vec::new();
    let mut expect = |what: &str, got: Value, want: &Value| {
        if &got != want {
            failures.push(format!(
                "{what}: papers-rs {got}, papers {want}\n  target {} other {} base {}",
                case["target"], case["other"], case["base"]
            ));
        }
    };
    let m = make(case);
    let model = &case["model"];
    let fields: Vec<Value> = m
        .fields
        .iter()
        .map(|f| {
            json!([
                f.name,
                status(f.status),
                f.values,
                f.value(),
                f.alternatives(),
                changed(f.changed),
                f.removal(),
                f.base
            ])
        })
        .collect();
    expect("fields", Value::Array(fields), &model["fields"]);
    expect("files", json!(m.files), &model["files"]);
    expect("new_files", json!(m.new_files), &model["new_files"]);
    expect("ids", json!(m.ids), &model["ids"]);
    expect("ids_added", json!(m.ids_added), &model["ids_added"]);
    expect("exact", json!(m.is_exact()), &model["exact"]);
    expect("three_way", json!(m.is_three_way()), &model["three_way"]);
    expect("exchange", json!(m.exchange), &model["exchange"]);
    expect(
        "conflicts",
        json!(names(m.conflicts().into_iter())),
        &model["conflicts"],
    );
    expect(
        "additions",
        json!(names(m.additions().into_iter())),
        &model["additions"],
    );
    expect("result", json!(formatted(&m.result())), &model["result"]);

    let v = &case["variants"];
    for choice in 0..2 {
        let mut m = make(case);
        m.choose_all(choice);
        expect(
            &format!("choose{choice}"),
            json!(formatted(&m.result())),
            &v[format!("choose{choice}")],
        );
    }
    let mut m = make(case);
    for f in m.fields.iter_mut().filter(|f| f.status == Status::Added) {
        f.decline();
    }
    expect("declined", json!(formatted(&m.result())), &v["declined"]);
    let mut m = make(case);
    for (i, f) in m.fields.iter_mut().enumerate() {
        if f.status == Status::Conflict {
            f.choose(i % 2);
        } else if f.status == Status::Added && i % 3 == 0 {
            f.decline();
        }
    }
    expect("mixed", json!(formatted(&m.result())), &v["mixed"]);
    expect("target_key", json!(make(case).key()), &v["target_key"]);
    if !v["with_target1"].is_null() {
        expect(
            "with_target1",
            json!(formatted(&make(case).with_target(1).result())),
            &v["with_target1"],
        );
    }
    failures
}

fn run(cases: &[Value]) {
    let failures: Vec<String> = cases.iter().flat_map(check).collect();
    assert!(
        failures.is_empty(),
        "{} of {} cases differ; the first:\n{}",
        failures.len(),
        cases.len(),
        failures
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn load(path: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap()
}

#[test]
fn the_merge_as_papers_makes_it() {
    let path = format!("{}/tests/expected/merge3.json", env!("CARGO_MANIFEST_DIR"));
    let cases = load(&path);
    let by_mode = |m: &str| cases.iter().filter(|c| c["mode"] == m).count();
    assert!(by_mode("three") > 100 && by_mode("exchange") > 10 && by_mode("two") > 10);
    run(&cases);
}

#[test]
fn fuzz_file() {
    let Ok(path) = std::env::var("PAPERS_RS_MERGE3_FUZZ") else {
        return;
    };
    run(&load(&path));
}

fn e(key: &str, fields: &[(&str, &str)]) -> Entry {
    let mut e = Entry::new("article", key);
    for (k, v) in fields {
        e.set(k, *v);
    }
    e
}

/// What `test_merge3.py` of papers checks.
#[test]
fn the_cases_of_the_papers_tests() {
    let base = [
        ("author", "Smith, John"),
        ("title", "Climate sensitivity revisited"),
        ("year", "2020"),
        ("journal", "Nat."),
        ("keywords", "climate"),
    ];
    let with = |more: &[(&'static str, &'static str)]| -> Vec<(&str, &str)> {
        let mut f: Vec<(&str, &str)> = base
            .iter()
            .filter(|(k, _)| !more.iter().any(|(m, _)| m == k))
            .copied()
            .collect();
        f.extend(more.iter().filter(|(_, v)| !v.is_empty()).copied());
        f
    };
    let three = |lib: &[(&'static str, &'static str)], pool: &[(&'static str, &'static str)]| {
        Merge::exchange(
            e("smith2020climate", &with(lib)),
            e("Smith_2020", &with(pool)),
            None,
            Some(&e("smith2020climate", &base)),
        )
    };
    let st = |m: &Merge, n: &str| {
        let f = m.fields.iter().find(|f| f.name == n).unwrap();
        (f.status, f.changed)
    };

    assert!(three(&[], &[]).is_exact());
    // changed in the pool only: offered
    let m = three(&[], &[("journal", "Nature"), ("volume", "10")]);
    assert_eq!(st(&m, "journal"), (Status::Added, Some(Changed::Other)));
    assert_eq!(st(&m, "volume"), (Status::Added, Some(Changed::Other)));
    assert!(m.conflicts().is_empty());
    assert_eq!(m.result().get("volume"), Some("10"));
    assert_eq!(m.result().key, "smith2020climate");
    // changed in the library only: kept
    let m = three(&[("keywords", "climate, mine")], &[]);
    assert_eq!(st(&m, "keywords"), (Status::Same, Some(Changed::Target)));
    assert!(m.is_exact());
    // both sides
    let mut m = three(
        &[("journal", "Nature")],
        &[("journal", "Nature Climate Change")],
    );
    assert_eq!(st(&m, "journal"), (Status::Conflict, Some(Changed::Both)));
    assert_eq!(m.result().get("journal"), Some("Nature"));
    m.choose_all(1);
    assert_eq!(m.result().get("journal"), Some("Nature Climate Change"));
    // the same change on both sides
    assert!(three(&[("journal", "Nature")], &[("journal", "nature")]).is_exact());
    // removed in the pool: an addition that is a removal, which can be declined
    let mut m = three(&[], &[("keywords", "")]);
    assert_eq!(st(&m, "keywords"), (Status::Added, Some(Changed::Other)));
    assert!(m.additions()[0].removal());
    assert_eq!(m.result().get("keywords"), None);
    m.fields
        .iter_mut()
        .find(|f| f.name == "keywords")
        .unwrap()
        .decline();
    assert_eq!(m.result().get("keywords"), Some("climate"));
    // removed in the pool, changed in the library: a conflict, the library's value until decided
    let mut m = three(&[("keywords", "climate, mine")], &[("keywords", "")]);
    assert_eq!(st(&m, "keywords"), (Status::Conflict, Some(Changed::Both)));
    assert_eq!(m.result().get("keywords"), Some("climate, mine"));
    m.choose_all(1);
    assert_eq!(m.result().get("keywords"), None);
    // removed in the library, changed in the pool
    let mut m = three(&[("keywords", "")], &[("keywords", "climate, pool")]);
    assert_eq!(st(&m, "keywords"), (Status::Conflict, Some(Changed::Both)));
    assert_eq!(m.result().get("keywords"), None);
    m.choose_all(1);
    assert_eq!(m.result().get("keywords"), Some("climate, pool"));
    // removed on both sides, or in the library only: stays removed
    assert!(three(&[("keywords", "")], &[("keywords", "")]).is_exact());
    assert!(three(&[("keywords", "")], &[]).is_exact());
    // added differently on both sides
    let m = three(&[("volume", "9")], &[("volume", "10")]);
    assert_eq!(st(&m, "volume"), (Status::Conflict, Some(Changed::Both)));
    // a declined addition: the target's value
    let mut m = three(&[], &[("journal", "Nature"), ("volume", "10")]);
    for f in m.fields.iter_mut().filter(|f| f.status == Status::Added) {
        f.decline();
    }
    assert_eq!(m.result().get("journal"), Some("Nat."));
    assert_eq!(m.result().get("volume"), None);
    // without a base: one side only is an addition (the pool's), or kept (the library's)
    let no_keywords = with(&[("keywords", "")]);
    let m = Merge::exchange(e("a", &no_keywords), e("b", &base), None, None);
    assert_eq!(m.additions().len(), 1);
    assert_eq!(m.result().get("keywords"), Some("climate"));
    let m = Merge::exchange(e("a", &base), e("b", &no_keywords), None, None);
    assert!(m.is_exact());
    assert_eq!(m.result().get("keywords"), Some("climate"));
    // the former keys: the target's only
    let mut a = e("a", &base);
    a.set("ids", "old");
    let mut b = e("b", &base);
    b.set("ids", "poolold");
    for base in [None, Some(e("a", &base))] {
        let m = Merge::exchange(a.clone(), b.clone(), None, base.as_ref());
        assert_eq!(
            (m.result().get("ids"), m.result().key.as_str()),
            (Some("old"), "a")
        );
    }
}

#[test]
fn decisions_and_applying_them() {
    let base = e("a", &[("title", "T"), ("journal", "Nat.")]);
    let library = e("a", &[("title", "T"), ("journal", "Nature")]);
    let pool = e(
        "A_pool",
        &[("title", "T"), ("journal", "Science"), ("volume", "2")],
    );
    let merge = || Merge::exchange(library.clone(), pool.clone(), None, Some(&base));
    let verdict = papers_rs::identity::Verdict {
        kind: papers_rs::identity::Kind::Same,
        reason: "pool default".into(),
    };
    // keep: entry 1, update: entry 2; the additions are taken in every mode
    for (mode, journal) in [
        (OnDuplicate::Keep, "Nature"),
        (OnDuplicate::Update, "Science"),
    ] {
        match Resolver::new(mode)
            .decide(merge(), Context::Pull, &verdict)
            .unwrap()
        {
            Decision::Merge(_, merged) => {
                assert_eq!(merged.get("journal"), Some(journal));
                assert_eq!(merged.get("volume"), Some("2"));
                let mut target = library.clone();
                apply_entry(&mut target, &merged);
                assert_eq!(target.get("journal"), Some(journal));
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(matches!(
        Resolver::new(OnDuplicate::Skip).decide(merge(), Context::Pull, &verdict),
        Ok(Decision::Skip)
    ));
    let error = Resolver::new(OnDuplicate::Error)
        .decide(merge(), Context::Pull, &verdict)
        .unwrap_err();
    assert_eq!(
        error.0,
        "a: changed in the library and in pool default (A_pool; 1 conflict)\nPass --on-duplicate with one of: ask, keep, update, skip (ask is the default in a terminal), see `papers pull --help`."
    );
    // push: entry 1 is the pool entry, so keep keeps the pool's value
    let push = Merge::exchange(pool.clone(), library.clone(), None, Some(&base));
    match Resolver::new(OnDuplicate::Keep)
        .decide(push.clone(), Context::Push, &verdict)
        .unwrap()
    {
        Decision::Merge(_, merged) => assert_eq!(merged.get("journal"), Some("Science")),
        other => panic!("{other:?}"),
    }
    let error = Resolver::new(OnDuplicate::Error)
        .decide(push, Context::Push, &verdict)
        .unwrap_err();
    assert!(
        error
            .0
            .starts_with("a: changed in the library and in pool default (A_pool; 1 conflict)\n")
    );
    assert!(error.0.ends_with("see `papers push --help`."));
    // a revert names `history`
    let error = Resolver::new(OnDuplicate::Error)
        .decide(merge(), Context::Revert, &verdict)
        .unwrap_err();
    assert_eq!(
        error.0,
        "a: changed since pool default too (1 conflict)\nPass --on-duplicate with one of: ask, keep, update, skip (ask is the default in a terminal), see `papers history --help`."
    );
}
