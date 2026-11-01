//! An item in the library: the library's entries of its DOI, or of its
//! title and year, and those of its other versions; by the facts of the
//! library, whose identities core keeps with it.

use std::sync::Arc;

use chrono::{TimeZone, Utc};
use papers_core::Library;
use papers_feeds::{Feeds, Item, ItemKind, LibraryFacts, Stage, Version};

#[test]
fn items_in_the_library_by_doi_or_title_and_year() {
    let dir = tempfile::tempdir().unwrap();
    let bib = dir.path().join("refs.bib");
    std::fs::write(
        &bib,
        "@article{doi2026, title = {Anything}, year = {2026}, doi = {10.5194/TC-20-1-2026}}\n\
         @article{title2026, title = {Hysteresis of the {Greenland} ice sheet}, year = {2026}}\n\
         @article{title2019, title = {Hysteresis of the Greenland ice sheet}, year = {2019}}\n\
         @article{preprint, title = {Old title}, year = {2025}, doi = {10.5194/egusphere-2025-1}}\n",
    )
    .unwrap();
    let library = Library::load(&[bib]).unwrap();
    let mut feeds = Feeds::open(&dir.path().join("feeds")).unwrap();
    let id = |key: &str| {
        let item = library.items().iter().find(|i| i.key == key).unwrap();
        item.id.clone()
    };
    let date = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).single();
    let item = |doi: Option<&str>, title: &str| Item {
        id: "x".into(),
        kind: ItemKind::Paper,
        title: title.into(),
        authors: Vec::new(),
        journal: None,
        date,
        doi: doi.map(String::from),
        arxiv: None,
        url: None,
        summary: None,
        content: None,
        editorial: false,
        sources: Vec::new(),
        versions: Vec::new(),
        links: Vec::new(),
        cited: None,
    };
    let by_doi = item(Some("10.5194/tc-20-1-2026"), "Another title");
    assert!(feeds.in_library(&by_doi).is_empty(), "no facts yet");
    feeds.set_library(Arc::new(LibraryFacts {
        likeness: None,
        identities: library.identities(),
    }));
    assert_eq!(feeds.in_library(&by_doi), [id("doi2026")]);
    let by_title = item(None, "Hysteresis of the Greenland Ice-Sheet");
    assert_eq!(
        feeds.in_library(&by_title),
        [id("title2026")],
        "the same year only"
    );
    assert!(feeds.in_library(&item(None, "Unrelated")).is_empty());

    let mut paper = item(Some("10.5194/tc-20-1-2026"), "Anything");
    paper.versions = vec![
        Version {
            doi: "10.5194/tc-20-1-2026".into(),
            stage: Stage::Published,
        },
        Version {
            doi: "10.5194/egusphere-2025-1".into(),
            stage: Stage::Preprint,
        },
    ];
    let same = feeds.in_library(&paper);
    assert_eq!(same, [id("doi2026")], "its DOI and its title: one entry");
    assert_eq!(feeds.versions_in_library(&paper, &same), [id("preprint")]);
}
