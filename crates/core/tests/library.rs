use std::path::{Path, PathBuf};

use papers_core::{Cite, Error, Library, Matching, Span};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn library() -> Library {
    Library::load(&[fixture("library.bib")]).unwrap()
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

    // relative links are relative to the .bib file
    let it = lib.get("robinson2012").unwrap();
    assert_eq!(
        it.files,
        [
            PathBuf::from("/library/robinson_et_al_2012.pdf"),
            fixture("papers/robinson_et_al_2012--supp.pdf"),
        ]
    );
    assert_eq!(it.bib, fixture("library.bib"));
    assert_eq!(lib.get("muller2020").unwrap().authors, ["Anna Müller"]);

    let it = lib.get("vandenbroeke2009").unwrap();
    // a doi.org link is a DOI
    assert_eq!(it.doi.as_deref(), Some("10.1126/science.1178176"));
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
    assert_eq!(keys(&lib, "has:pdf"), ["feldmann2017", "robinson2012"]);
    assert_eq!(keys(&lib, "has:pages"), ["robinson2012"]);
    assert_eq!(keys(&lib, "greenland -has:pdf"), ["calov2015", "vandenbroeke2009"]);
    assert!(keys(&lib, "has:abstract").is_empty());
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
fn excluded() {
    let lib = library();
    assert_eq!(keys(&lib, "greenland -robinson"), ["vandenbroeke2009"]);
    assert_eq!(
        keys(&lib, "greenland -y:2012"),
        ["calov2015", "vandenbroeke2009"]
    );
    assert_eq!(keys(&lib, "a:robinson -f:robinson"), ["calov2015"]);
    assert_eq!(
        keys(&lib, r#"greenland -"ice sheet""#),
        ["vandenbroeke2009"]
    );
    // only exclusions: everything else
    assert_eq!(lib.search("-greenland", Matching::WordStart, 50).total, 5);
    // an excluded word is not marked
    let found = lib.search("calov -multistability", Matching::WordStart, 1);
    assert_eq!(
        marked(&found.hits[0].title),
        "Greenland ice sheet simulations"
    );
}

#[test]
fn year_counts() {
    let lib = library();
    let years = |query: &str| -> Vec<(i32, usize, bool)> {
        let found = lib.search(query, Matching::WordStart, 50);
        found
            .years
            .into_iter()
            .map(|y| (y.year, y.count, y.selected))
            .collect()
    };
    let span = |selected: &dyn Fn(i32) -> bool| -> Vec<(i32, usize, bool)> {
        (2009..=2015)
            .map(|y| (y, usize::from([2009, 2012, 2015].contains(&y)), selected(y)))
            .collect()
    };
    // from the first year to the last, the years without a match included
    assert_eq!(years("greenland"), span(&|_| true));
    // counted before the year filters, which select the years
    assert_eq!(years("greenland y:2015"), span(&|y| y == 2015));
    assert_eq!(years("greenland y:2010-"), span(&|y| y >= 2010));
    assert_eq!(years("greenland -y:2012"), span(&|y| y != 2012));
    assert_eq!(
        lib.search("greenland y:2015", Matching::WordStart, 50)
            .total,
        1
    );
    // no year, no count
    assert!(years("k:smith").is_empty());
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
    // the CSL-JSON as papers wrote it, for a citation processor
    let csl = lib.csl("robinson2012").unwrap();
    assert_eq!(csl["DOI"], "10.1038/nclimate1449");
    assert_eq!(csl["author"][1]["family"], "Calov");
}

#[test]
fn several_files() {
    let lib = Library::load(&[fixture("library.bib"), fixture("other.bib")]).unwrap();
    assert_eq!(lib.len(), 10);
    assert_eq!(lib.bibs(), [fixture("library.bib"), fixture("other.bib")]);
    // a key in two files: numbered ids, in the order of the files
    assert!(lib.get("robinson2012").is_none());
    assert_eq!(
        lib.get("robinson2012#1").unwrap().bib,
        fixture("library.bib")
    );
    let other = lib.get("robinson2012#2").unwrap();
    assert_eq!(other.key, "robinson2012");
    assert_eq!(other.files, [PathBuf::from("/elsewhere/robinson2012.pdf")]);
    assert_eq!(other.container.as_deref(), Some("Nat. Clim. Change"));
    assert_eq!(
        lib.get("beckmann2019").unwrap().subtitle,
        "Beckmann & Perrette · 2019 · EGU General Assembly"
    );
}

#[test]
fn bibtex() {
    let lib = library();
    // as written in the file
    let raw = lib.raw("robinson2012").unwrap();
    assert!(raw.starts_with("@article{robinson2012,\n author = {Robinson, Alexander"));
    assert!(raw.ends_with("year = {2012}\n}"), "{raw:?}");
    // formatted
    let bibtex = lib.bibtex("muller2020").unwrap();
    assert!(bibtex.starts_with("@article{muller2020,"), "{bibtex}");
    assert!(bibtex.contains("Nature"), "{bibtex}");
}

#[test]
fn errors() {
    let missing = Library::load(&[fixture("missing.bib")]);
    assert!(matches!(missing, Err(Error::Read { .. })));
    let broken = Library::load(&[fixture("broken.bib")]).err().unwrap();
    assert!(matches!(broken, Error::Parse { .. }), "{broken}");
    assert!(broken.to_string().contains("broken.bib, line"), "{broken}");
}

/// Reads a real .bib: `PAPERS_BIB=path/to/library.bib cargo test -- --ignored`
#[test]
#[ignore]
fn user_library() {
    let bib = std::env::var("PAPERS_BIB").expect("PAPERS_BIB: the .bib file to read");
    let start = std::time::Instant::now();
    let lib = Library::load(&[PathBuf::from(bib)]).unwrap();
    println!("{} entries in {:?}", lib.len(), start.elapsed());
    assert!(!lib.is_empty());
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
                h.item.id,
                marked(&h.title),
                marked(&h.subtitle)
            );
        }
    }
}
