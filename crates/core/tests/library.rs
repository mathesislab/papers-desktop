use std::path::PathBuf;

use papers_core::{Cite, Library, Matching, Papers, Span};

fn library() -> Library {
    Library::from_csl_json(include_str!("fixtures/library.json")).unwrap()
}

fn keys(lib: &Library, query: &str) -> Vec<String> {
    keys_with(lib, query, Matching::WordStart)
}

fn keys_with(lib: &Library, query: &str, matching: Matching) -> Vec<String> {
    let found = lib.search(query, matching, 50);
    found.hits.into_iter().map(|h| h.item.key).collect()
}

/// Spans written out, the marked ones in *stars*.
fn marked(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| {
            if s.mark {
                format!("*{}*", s.text)
            } else {
                s.text.clone()
            }
        })
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
fn every_term_must_match() {
    let lib = library();
    assert_eq!(keys(&lib, "robinson 2012"), ["robinson2012"]);
    assert_eq!(keys(&lib, "greenland 2009"), ["vandenbroeke2009"]);
    assert!(keys(&lib, "robinson nonexistent").is_empty());
    assert!(keys(&lib, "  ").is_empty());
}

#[test]
fn newest_first() {
    let lib = library();
    assert_eq!(keys(&lib, "robinson"), ["calov2015", "robinson2012"]);
    assert_eq!(
        keys(&lib, "greenland"),
        ["calov2015", "robinson2012", "vandenbroeke2009"]
    );
    assert_eq!(
        keys(&lib, "climate"),
        ["ipcc2021", "calov2015", "robinson2012"]
    );
}

#[test]
fn word_start() {
    let lib = library();
    assert_eq!(keys(&lib, "broeke"), ["vandenbroeke2009"]);
    assert_eq!(keys(&lib, "van den broeke"), ["vandenbroeke2009"]);
    assert_eq!(keys(&lib, "muller"), ["muller2020"]);
    assert_eq!(keys(&lib, "Müller"), ["muller2020"]);
    assert_eq!(keys(&lib, "feld"), ["feldmann2017"]);
    assert_eq!(keys(&lib, "heinrich-like"), ["feldmann2017"]);
    assert_eq!(keys(&lib, "mass balance"), ["vandenbroeke2009"]);
    // not inside words, unless asked
    assert!(keys(&lib, "stability").is_empty());
    assert_eq!(
        keys_with(&lib, "stability", Matching::InsideWords),
        ["robinson2012"]
    );
    assert!(keys_with(&lib, "ab", Matching::InsideWords).is_empty());
}

#[test]
fn fields() {
    let lib = library();
    assert_eq!(keys(&lib, "author:robinson"), ["calov2015", "robinson2012"]);
    assert_eq!(keys(&lib, "first:robinson"), ["robinson2012"]);
    assert_eq!(keys(&lib, "f:calov"), ["calov2015"]);
    assert_eq!(
        keys(&lib, "a:robinson a:calov"),
        ["calov2015", "robinson2012"]
    );
    assert_eq!(
        keys(&lib, "title:greenland"),
        ["calov2015", "robinson2012", "vandenbroeke2009"]
    );
    assert_eq!(keys(&lib, "t:climate"), ["ipcc2021"]);
    assert_eq!(keys(&lib, "journal:nature"), ["muller2020", "robinson2012"]);
    assert_eq!(keys(&lib, "j:nature y:2012"), ["robinson2012"]);
    assert_eq!(keys(&lib, "tag:mass"), ["vandenbroeke2009"]);
    assert!(keys(&lib, "tag:greenland a:robinson").is_empty());
    assert_eq!(keys(&lib, "key:robinson"), ["robinson2012"]);
    assert_eq!(keys(&lib, "k:smith"), ["smith_handbook"]);
    // the field of a filter only
    assert!(keys(&lib, "author:greenland").is_empty());
    assert!(keys(&lib, "title:robinson").is_empty());
}

#[test]
fn years() {
    let lib = library();
    assert_eq!(keys(&lib, "year:2012"), ["robinson2012"]);
    assert_eq!(keys(&lib, "year:2010-2016"), ["calov2015", "robinson2012"]);
    assert_eq!(keys(&lib, "year:2020-"), ["ipcc2021", "muller2020"]);
    assert_eq!(keys(&lib, "y:-2009"), ["vandenbroeke2009"]);
    assert_eq!(
        keys(&lib, "y:201"),
        ["feldmann2017", "calov2015", "robinson2012"]
    );
    assert!(keys(&lib, "year:abc").is_empty());
}

#[test]
fn phrases() {
    let lib = library();
    assert_eq!(
        keys(&lib, r#"title:"ice sheet""#),
        ["muller2020", "calov2015", "robinson2012"]
    );
    assert!(keys(&lib, r#""sheet ice""#).is_empty());
    assert_eq!(keys(&lib, r#"a:"van den""#), ["vandenbroeke2009"]);
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
    assert_eq!(keys(&lib, "doi:nclimate"), ["robinson2012"]);
    assert_eq!(keys(&lib, "10.1002/xyz.123"), ["10.1002/xyz.123"]);
    assert_eq!(keys(&lib, "nclimate1449"), ["robinson2012"]);
}

#[test]
fn marks() {
    let lib = library();
    let hit = |query: &str| lib.search(query, Matching::WordStart, 1).hits.remove(0);

    let h = hit("greenland robinson 2012");
    assert_eq!(
        marked(&h.title),
        "Multistability and critical thresholds of the *Greenland* ice sheet"
    );
    assert_eq!(
        marked(&h.subtitle),
        "*Robinson* et al. · *2012* · Nature Climate Change"
    );
    assert_eq!(h.authors, [true, false, false]);

    // a co-author behind "et al."
    let h = hit("first:robinson calov");
    assert_eq!(
        marked(&h.subtitle),
        "*Robinson* *et al.* · 2012 · Nature Climate Change"
    );
    assert_eq!(h.authors, [true, true, false]);

    // whole words, and both of two authors
    let h = hit("feldm leverm");
    assert_eq!(
        marked(&h.subtitle),
        "*Feldmann* & *Levermann* · 2017 · The Cryosphere"
    );
    let h = hit("j:clim y:2012");
    assert_eq!(
        marked(&h.subtitle),
        "Robinson et al. · *2012* · Nature *Climate* Change"
    );

    let h = hit("muller");
    assert_eq!(marked(&h.subtitle), "*Müller* · 2020 · Nature");
    let h = hit(r#""ice sheet" y:2020"#);
    assert_eq!(marked(&h.title), "*Ice* *sheet* hysteresis");
}

#[test]
fn total_and_limit() {
    let lib = library();
    let found = lib.search("greenland", Matching::WordStart, 1);
    assert_eq!((found.hits.len(), found.total), (1, 3));
}

#[test]
fn cite_and_link() {
    let lib = library();
    let it = lib.get("robinson2012").unwrap();
    assert_eq!(it.cite(Cite::Key), "robinson2012");
    assert_eq!(it.cite(Cite::Pandoc), "[@robinson2012]");
    assert_eq!(it.cite(Cite::Latex), "\\cite{robinson2012}");
    assert_eq!(it.link().unwrap(), "https://doi.org/10.1038/nclimate1449");
    assert_eq!(lib.get("smith_handbook").unwrap().link(), None);
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
    // exact key: robinson2012 only, not robinson2012a or robinson2012-supp
    let key = lib.search("robinson", Matching::WordStart, 1).hits[0]
        .item
        .key
        .clone();
    let bibtex = papers.bibtex(&key).unwrap();
    assert!(bibtex.starts_with('@'));
    assert_eq!(bibtex.matches("\n@").count(), 0);
    assert!(bibtex.contains(&format!("{{{key},")));
    for query in [
        "greenland ice sheet",
        "robinson",
        "f:robinson y:2010-2015",
        "10.5194",
    ] {
        let found = lib.search(query, Matching::WordStart, 5);
        println!("{query}: {} matches", found.total);
        for h in found.hits {
            println!(
                "  {} | {} | {}",
                h.item.key,
                marked(&h.title),
                marked(&h.subtitle)
            );
        }
    }
}
