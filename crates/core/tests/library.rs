use std::path::PathBuf;

use papers_core::{Library, Papers};

fn library() -> Library {
    Library::from_csl_json(include_str!("fixtures/library.json")).unwrap()
}

fn keys(lib: &Library, query: &str) -> Vec<String> {
    lib.search(query, 50)
        .into_iter()
        .map(|it| it.key.clone())
        .collect()
}

#[test]
fn items() {
    let lib = library();
    assert_eq!(lib.len(), 8);

    let it = lib.get("feldmann2017").unwrap();
    assert_eq!(
        it.title,
        "From cyclic ice streaming to Heinrich-like events"
    );
    assert_eq!(it.authors, ["Johannes Feldmann", "Anders Levermann"]);
    assert_eq!(it.subtitle, "Feldmann & Levermann · 2017 · The Cryosphere");
    assert_eq!(
        it.files,
        [PathBuf::from("/library/feldmann_and_levermann_2017.pdf")]
    );

    let it = lib.get("vandenbroeke2009").unwrap();
    assert_eq!(it.authors[0], "Michiel van den Broeke");
    assert_eq!(it.subtitle, "van den Broeke et al. · 2009 · Science");
    assert_eq!(it.keywords, ["greenland", "mass balance"]);

    assert_eq!(lib.get("muller2020").unwrap().year, Some(2020));
    assert_eq!(lib.get("ipcc2021").unwrap().subtitle, "IPCC · 2021");

    // editors stand in for missing authors; "None" is no year
    let it = lib.get("smith_handbook").unwrap();
    assert_eq!(it.subtitle, "Smith");
    assert_eq!(it.year, None);

    // no title: the key stands in
    let it = lib.get("10.1002/xyz.123").unwrap();
    assert_eq!(it.title, "10.1002/xyz.123");
    assert_eq!(it.subtitle, "");
}

#[test]
fn every_word_must_match() {
    let lib = library();
    assert_eq!(keys(&lib, "robinson 2012"), ["robinson2012"]);
    assert_eq!(keys(&lib, "greenland 2009"), ["vandenbroeke2009"]);
    assert!(keys(&lib, "robinson nonexistent").is_empty());
    assert!(keys(&lib, "  ").is_empty());
}

#[test]
fn first_author_ranks_first() {
    let lib = library();
    assert_eq!(keys(&lib, "robinson"), ["robinson2012", "calov2015"]);
    assert_eq!(keys(&lib, "calov"), ["calov2015", "robinson2012"]);
}

#[test]
fn author_names() {
    let lib = library();
    assert_eq!(keys(&lib, "broeke"), ["vandenbroeke2009"]);
    assert_eq!(keys(&lib, "van den broeke"), ["vandenbroeke2009"]);
    assert_eq!(keys(&lib, "muller"), ["muller2020"]);
    assert_eq!(keys(&lib, "Müller"), ["muller2020"]);
    assert_eq!(keys(&lib, "ipcc"), ["ipcc2021"]);
    assert_eq!(keys(&lib, "feld"), ["feldmann2017"]);
}

#[test]
fn title_journal_keyword() {
    let lib = library();
    assert_eq!(keys(&lib, "hysteresis"), ["muller2020"]);
    assert_eq!(keys(&lib, "heinrich-like"), ["feldmann2017"]);
    assert_eq!(keys(&lib, "cryosphere"), ["feldmann2017"]);
    assert_eq!(keys(&lib, "mass balance"), ["vandenbroeke2009"]);
    // title words outrank journal words
    assert_eq!(
        keys(&lib, "climate"),
        ["ipcc2021", "calov2015", "robinson2012"]
    );
}

#[test]
fn doi() {
    let lib = library();
    assert_eq!(keys(&lib, "10.5194/tc-11"), ["feldmann2017"]);
    assert_eq!(keys(&lib, "10.5194"), ["feldmann2017"]);
    assert_eq!(
        keys(&lib, "https://doi.org/10.1038/nclimate1449"),
        ["robinson2012"]
    );
    assert_eq!(keys(&lib, "10.1002/xyz.123"), ["10.1002/xyz.123"]);
    assert_eq!(keys(&lib, "nclimate1449"), ["robinson2012"]);
}

#[test]
fn limit() {
    let lib = library();
    assert_eq!(lib.search("greenland", 50).len(), 3);
    assert_eq!(lib.search("greenland", 1).len(), 1);
}

/// Loads the real library through the papers CLI: `cargo test -- --ignored`
#[test]
#[ignore]
fn user_library() {
    let papers = Papers::new(Papers::locate().expect("papers not found on the shell PATH"));
    assert!(papers.bibtex_path().unwrap().exists());
    let lib = papers.load().unwrap();
    assert!(!lib.is_empty());
    println!("{} entries", lib.len());
    for query in ["greenland ice sheet", "robinson", "10.5194"] {
        let hits = lib.search(query, 5);
        println!("{query}:");
        for it in hits {
            println!("  {} | {} | {}", it.key, it.title, it.subtitle);
        }
    }
}
