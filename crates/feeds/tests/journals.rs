//! The journals of the library's papers, counted; OpenAlex's journals of a
//! name or a search (October 2026, recorded without a key), kept; what
//! follows each; and the feed a journal's site links to (pages written here).

use papers_feeds::http::Recorded;
use papers_feeds::{
    Access, Feed, Follow, Journal, Journals, LibraryJournal, MOST_JOURNALS, Reply, SiteFeed,
    journal_key, library_journals, search_journals, site_feed,
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

#[test]
fn journals_searched() {
    let url = search_url("glaciology");
    let http = Recorded::new().reply(
        &url,
        Reply::ok(&url, fixture("openalex-sources-glaciology.json")),
    );
    let found = search_journals(&http, &polite(), "glaciology").unwrap();
    // as the sheet shows them: title, publisher, ISSN, works
    let rows: Vec<String> = found
        .iter()
        .map(|j| {
            format!(
                "{} | {:?} | {:?} | {:?}",
                j.title, j.publisher, j.issn, j.works
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            r#"Journal of Glaciology | Some("Cambridge University Press") | Some("0022-1430") | Some(6886)"#,
            r#"Journal of Glaciology and Geocryology | Some("Science Press") | Some("1000-0240") | Some(1699)"#,
        ]
    );
    assert_eq!(found[0].issns, ["0022-1430", "1727-5652"]);
    let failed = search_url("ice");
    let http = Recorded::new().reply(&failed, Reply::status(&failed, 503));
    assert_eq!(
        search_journals(&http, &polite(), "ice"),
        Err("HTTP 503".to_string())
    );
}

#[test]
fn the_home_page_of_a_journal_by_its_issn_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut journals = Journals::open(dir.path()).unwrap();
    let url = format!(
        "https://api.openalex.org/sources/issn:0022-1430?{SELECT}&mailto=papers-desktop@example.org"
    );
    let source = r#"{"id":"https://openalex.org/S140216511","display_name":"Journal of Glaciology",
        "alternate_titles":[],"issn_l":"0022-1430","issn":["0022-1430","1727-5652"],
        "host_organization_name":"Cambridge University Press",
        "homepage_url":"https://www.cambridge.org/core/journals/journal-of-glaciology","works_count":6886}"#;
    let http = Recorded::new().reply(&url, Reply::ok(&url, source));
    // as the library's papers give it
    let journal = Journal {
        title: "J. Glaciol.".to_string(),
        issn: Some("0022-1430".to_string()),
        issns: vec!["0022-1430".to_string()],
        publisher: None,
        homepage: None,
        works: None,
    };
    let homepage = Some("https://www.cambridge.org/core/journals/journal-of-glaciology");
    for _ in 0..2 {
        let found = journals
            .with_homepage(journal.clone(), &http, &polite())
            .unwrap();
        assert_eq!(found.homepage.as_deref(), homepage);
        assert_eq!(found.title, "J. Glaciol.");
    }
    assert_eq!(http.requests().len(), 1);
    // one with a home page, or without an ISSN, is not asked for
    let none = Journal {
        issn: None,
        issns: Vec::new(),
        ..journal
    };
    assert_eq!(
        journals.with_homepage(none.clone(), &http, &polite()),
        Ok(none)
    );
}

const HOME: &str = "https://journal.example.org/home/";

fn page(head: &str) -> String {
    format!(
        "<!doctype html><html><head><title>J</title>{head}</head><body><p>Welcome</p></body></html>"
    )
}

/// An RSS feed of a title, with one item.
fn rss(title: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>{title}</title>\
         <link>https://journal.example.org/</link><item><title>A paper</title>\
         <link>https://journal.example.org/a</link></item></channel></rss>"
    )
}

/// A link of a page to a feed.
fn link(title: &str, href: &str) -> String {
    format!(r#"<link rel="alternate" type="application/rss+xml" title="{title}" href="{href}">"#)
}

const CLIMATE: &str = "Journal of Climate";

#[test]
fn the_feed_a_site_links_to() {
    let site = |reply: Reply| {
        let http = Recorded::new().reply(HOME, reply);
        site_feed(&http, &http, HOME, CLIMATE)
    };
    // no feed: none linked, or another kind of alternate
    let other = r#"<link rel="alternate" hreflang="de" href="/de/">
        <link rel="alternate" type="application/json" href="/oembed">"#;
    assert_eq!(site(Reply::ok(HOME, page(other))), SiteFeed::NoFeed);
    assert_eq!(site(Reply::ok(HOME, page(""))), SiteFeed::NoFeed);
    // refused
    assert_eq!(
        site(Reply::status(HOME, 403)),
        SiteFeed::Refused {
            reason: "HTTP 403".to_string()
        }
    );
    let unreachable = Recorded::new().fail(HOME, "dns error: no such host");
    assert_eq!(
        site_feed(&unreachable, &unreachable, HOME, CLIMATE),
        SiteFeed::Refused {
            reason: "dns error: no such host".to_string()
        }
    );
}

#[test]
fn the_feed_whose_link_names_the_journal_read() {
    // the second link names it: read, the first not
    let feed = "https://journal.example.org/clim.xml";
    let head = [
        link("AMS News", "https://journal.example.org/newsrss"),
        r#"<link rel="stylesheet" href="/s.css">"#.to_string(),
        link("Journal of Climate: Latest issue", "/clim.xml"),
    ];
    let http = Recorded::new()
        .reply(HOME, Reply::ok(HOME, page(&head.concat())))
        .reply(feed, Reply::ok(feed, rss("J Clim")));
    assert_eq!(
        site_feed(&http, &http, HOME, CLIMATE),
        SiteFeed::Found {
            url: feed.to_string(),
            title: Some("J Clim".to_string())
        }
    );
    let urls: Vec<String> = http.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(urls, [HOME, feed]);
    // refused, or not a feed: said so, no other read
    let refused = || {
        Recorded::new().reply(
            HOME,
            Reply::ok(HOME, page(&link("journal of climate", "/clim.xml"))),
        )
    };
    let http = refused().reply(feed, Reply::status(feed, 403));
    assert_eq!(
        site_feed(&http, &http, HOME, CLIMATE),
        SiteFeed::FeedRefused {
            url: feed.to_string(),
            reason: "HTTP 403".to_string()
        }
    );
    let html = refused().reply(feed, Reply::ok(feed, page("")));
    assert_eq!(
        site_feed(&html, &html, HOME, CLIMATE),
        SiteFeed::NotAFeed {
            url: feed.to_string()
        }
    );
    // the feed read as it will be fetched: one the page's agent gets, the fetch's not
    let page_agent = refused().reply(feed, Reply::ok(feed, rss("Journal of Climate")));
    let fetch_agent = Recorded::new().reply(feed, Reply::status(feed, 403));
    assert_eq!(
        site_feed(&page_agent, &fetch_agent, HOME, CLIMATE),
        SiteFeed::FeedRefused {
            url: feed.to_string(),
            reason: "HTTP 403".to_string()
        }
    );
}

#[test]
fn the_feed_whose_own_title_names_the_journal() {
    // relative to where the page was, after its redirects; Atom, in any case
    let moved = "https://journal.example.org/j/current/";
    let head = [
        link("Latest", "https://feeds.example.org/latest.xml"),
        r#"<LINK REL="Alternate" TYPE="Application/Atom+XML" HREF="../feed/atom">"#.to_string(),
    ];
    let latest = "https://feeds.example.org/latest.xml";
    let atom = "https://journal.example.org/j/feed/atom";
    let atom_feed = "<?xml version=\"1.0\"?><feed xmlns=\"http://www.w3.org/2005/Atom\">\
        <title>{The} Journal of Climate</title><id>j</id></feed>";
    let http = Recorded::new()
        .reply(HOME, Reply::ok(moved, page(&head.concat())))
        .reply(latest, Reply::status(latest, 404))
        .reply(atom, Reply::ok(atom, atom_feed));
    assert_eq!(
        site_feed(&http, &http, HOME, CLIMATE),
        SiteFeed::Found {
            url: atom.to_string(),
            title: Some("{The} Journal of Climate".to_string())
        }
    );
}

#[test]
fn a_feed_that_may_not_be_the_journal_s() {
    // AMS: its news first, then others, none naming the journal; 3 read at most
    let feeds =
        ["newsrss", "a.xml", "b.xml", "c.xml"].map(|f| format!("https://journal.example.org/{f}"));
    let head: String = feeds.iter().map(|f| link("", f)).collect();
    let http = Recorded::new()
        .reply(HOME, Reply::ok(HOME, page(&head)))
        .reply(&feeds[0], Reply::ok(&feeds[0], rss("AMS News")))
        .reply(&feeds[1], Reply::status(&feeds[1], 403))
        .reply(
            &feeds[2],
            Reply::ok(&feeds[2], rss("Weather and Forecasting")),
        )
        .reply(&feeds[3], Reply::ok(&feeds[3], rss(CLIMATE)));
    assert_eq!(
        site_feed(&http, &http, HOME, CLIMATE),
        SiteFeed::Other {
            url: feeds[0].clone(),
            title: Some("AMS News".to_string())
        }
    );
    assert_eq!(http.requests().len(), 4, "the page and three feeds");
    // none read: the first failure
    let http = Recorded::new()
        .reply(HOME, Reply::ok(HOME, page(&head)))
        .fail(&feeds[0], "timed out")
        .reply(&feeds[1], Reply::status(&feeds[1], 403));
    assert_eq!(
        site_feed(&http, &http, HOME, CLIMATE),
        SiteFeed::FeedRefused {
            url: feeds[0].clone(),
            reason: "timed out".to_string()
        }
    );
}
