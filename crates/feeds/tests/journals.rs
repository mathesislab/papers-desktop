//! The journals of the library's papers, counted; OpenAlex's journals of a
//! name (October 2026, recorded without a key), kept; and what follows each.

use papers_feeds::http::Recorded;
use papers_feeds::{
    Access, Feed, Follow, Journal, Journals, LibraryJournal, MOST_JOURNALS, Reply, journal_key,
    library_journals,
};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

const SELECT: &str = "select=id,display_name,alternate_titles,issn_l,issn,host_organization_name,\
    homepage_url,works_count";

fn search_url(search: &str) -> String {
    format!(
        "https://api.openalex.org/sources?search={search}&filter=type:journal&{SELECT}&per-page=10\
         &mailto=papers-desktop@example.org"
    )
}

fn polite() -> Access {
    Access {
        mailto: Some("papers-desktop@example.org".to_string()),
        openalex_key: None,
    }
}

#[test]
fn titles_folded_for_counting() {
    assert_eq!(journal_key("{The} Cryosphere"), "the cryosphere");
    assert_eq!(
        journal_key("Journal of Geophysical Research: Oceans"),
        journal_key("journal of geophysical research  oceans")
    );
    assert_eq!(journal_key("Earth's  Future"), "earth s future");
    assert_eq!(journal_key("{N}ature {G}eoscience"), "nature geoscience");
}

#[test]
fn the_journals_of_the_library_with_three_papers_or_more() {
    let papers = [
        ("Journal of Glaciology", Some("0022-1430")),
        ("J. Glaciol.", None),
        ("Climate of the Past", None),
        ("{Journal} of {Glaciology}", None),
        ("journal of glaciology", Some("00221430, 1727-5652")),
        ("Climate of the Past", None),
        ("Climate of the past", None),
        ("Nature", None),
        ("Nature", Some("not an ISSN")),
        ("Nature", None),
        ("Nature", None),
        ("", None),
        ("Science", None),
        ("Science", None),
    ];
    let journals = library_journals(papers);
    let titles: Vec<(&str, usize)> = journals
        .iter()
        .map(|j| (j.title.as_str(), j.papers))
        .collect();
    assert_eq!(
        titles,
        [
            ("Nature", 4),
            ("Climate of the Past", 3),
            ("Journal of Glaciology", 3)
        ],
        "most papers first, then by title; each written as most papers write it"
    );
    assert_eq!(journals[2].issns, ["0022-1430", "1727-5652"]);
    assert!(journals[0].issns.is_empty());
}

#[test]
fn at_most_fifteen_journals() {
    let titles: Vec<String> = (0..20).map(|i| format!("Journal {i:02}")).collect();
    let papers = titles
        .iter()
        .flat_map(|t| std::iter::repeat_n((t.as_str(), None::<&str>), 3));
    let journals = library_journals(papers);
    assert_eq!(journals.len(), MOST_JOURNALS);
    assert_eq!(journals[0].title, "Journal 00");
}

fn library(title: &str, issns: &[&str]) -> LibraryJournal {
    LibraryJournal {
        title: title.to_string(),
        papers: 5,
        issns: issns.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn a_journal_of_the_suggested_list_by_title_or_issn() {
    let dir = tempfile::tempdir().unwrap();
    let journals = Journals::open(dir.path()).unwrap();
    let Follow::Suggested(tc) = journals.follow(&library("the cryosphere", &[])) else {
        panic!("The Cryosphere is suggested");
    };
    assert_eq!(
        tc.feed,
        Feed::Rss {
            url: "https://tc.copernicus.org/xml/rss2_0.xml".to_string()
        }
    );
    // JGR Oceans by the electronic ISSN of Wiley's feed
    let Follow::Suggested(oceans) =
        journals.follow(&library("J. Geophys. Res. Oceans", &["2169-9291"]))
    else {
        panic!("JGR: Oceans is suggested");
    };
    assert_eq!(oceans.title, "JGR: Oceans");
    // an ISSN of the papers: a sweep of it, nothing asked
    let Follow::Journal(journal) = journals.follow(&library(
        "Earth and Planetary Science Letters",
        &["0012-821X"],
    )) else {
        panic!("by its ISSN");
    };
    assert_eq!(journal.issn.as_deref(), Some("0012-821X"));
    assert_eq!(journal.homepage, None);
    assert_eq!(
        journals.follow(&library("Earth and Planetary Science Letters", &[])),
        Follow::Pending
    );
}

#[test]
fn a_journal_looked_up_by_its_name_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut journals = Journals::open(dir.path()).unwrap();
    let epsl = search_url("Earth%20and%20Planetary%20Science%20Letters");
    let http = Recorded::new().reply(
        &epsl,
        Reply::ok(&epsl, fixture("openalex-sources-epsl.json")),
    );
    let name = "Earth and Planetary Science Letters";
    let found = journals.look_up(name, &http, &polite()).unwrap();
    let expected = Journal {
        title: "Earth and Planetary Science Letters".to_string(),
        issn: Some("0012-821X".to_string()),
        issns: vec!["0012-821X".to_string(), "1385-013X".to_string()],
        publisher: Some("Elsevier BV".to_string()),
        homepage: Some(
            "http://www.elsevier.com/wps/find/journaldescription.cws_home/503328/description"
                .to_string(),
        ),
        works: Some(21964),
    };
    assert_eq!(found, Follow::Journal(expected.clone()));
    // kept: not asked again, also once opened again
    let again = journals.look_up("earth and planetary science letters", &http, &polite());
    assert_eq!(again.unwrap(), Follow::Journal(expected.clone()));
    let reopened = Journals::open(dir.path()).unwrap();
    assert_eq!(
        reopened.follow(&library(name, &[])),
        Follow::Journal(expected)
    );
    assert_eq!(http.requests().len(), 1);
}

#[test]
fn a_journal_found_by_an_alternate_title_and_one_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut journals = Journals::open(dir.path()).unwrap();
    let bingchuan = search_url("Bingchuan%20Dongtu");
    let glaciology = search_url("Glaciology");
    let failed = search_url("Annals%20of%20Nowhere");
    let body = fixture("openalex-sources-glaciology.json");
    let http = Recorded::new()
        .reply(&bingchuan, Reply::ok(&bingchuan, body.clone()))
        .reply(&glaciology, Reply::ok(&glaciology, body))
        .fail(&failed, "connection refused");
    let Follow::Journal(found) = journals
        .look_up("Bingchuan Dongtu", &http, &polite())
        .unwrap()
    else {
        panic!("found by its alternate title");
    };
    assert_eq!(found.title, "Journal of Glaciology and Geocryology");
    assert_eq!(found.issn.as_deref(), Some("1000-0240"));
    assert_eq!(found.homepage, None);
    // no title of the search is the name: none found, and kept
    let none = journals.look_up("Glaciology", &http, &polite()).unwrap();
    assert_eq!(none, Follow::Unknown);
    assert_eq!(
        journals.follow(&library("glaciology", &[])),
        Follow::Unknown
    );
    // no answer: an error, not kept
    let error = journals.look_up("Annals of Nowhere", &http, &polite());
    assert_eq!(error, Err("connection refused".to_string()));
    assert_eq!(
        journals.follow(&library("Annals of Nowhere", &[])),
        Follow::Pending
    );
}

#[test]
fn a_journal_of_the_suggested_list_found_by_its_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let mut journals = Journals::open(dir.path()).unwrap();
    let url = search_url("J%20Glaciology");
    let mut body: serde_json::Value =
        serde_json::from_slice(&fixture("openalex-sources-glaciology.json")).unwrap();
    body["results"][0]["alternate_titles"] = serde_json::json!(["J Glaciology"]);
    let http = Recorded::new().reply(&url, Reply::ok(&url, body.to_string()));
    let Follow::Suggested(suggested) = journals.look_up("J Glaciology", &http, &polite()).unwrap()
    else {
        panic!("the Journal of Glaciology is suggested");
    };
    assert_eq!(
        suggested.feed,
        Feed::Crossref {
            issn: "0022-1430".to_string()
        }
    );
}
