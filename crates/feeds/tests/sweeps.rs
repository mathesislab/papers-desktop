//! Sweeps as subscriptions: Crossref by ISSN (October 2026, 3 rows of The
//! Cryosphere), OpenAlex by a filter (hand-written: no key to record with),
//! incremental; and `config.json` of the plain feeds before.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeZone, Utc};
use papers_feeds::http::{CROSSREF, Pacing, Recorded};
use papers_feeds::{Access, Error, Feed, Feeds, ItemKind, Outcome, Reply, SourceKind, Stage};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// A clock that the test moves.
#[derive(Clone)]
struct Clock(Arc<Mutex<DateTime<Utc>>>);

impl Clock {
    fn new() -> Clock {
        Clock(Arc::new(Mutex::new(
            Utc.with_ymd_and_hms(2026, 10, 31, 8, 0, 0).unwrap(),
        )))
    }

    fn open(&self, dir: &std::path::Path) -> Feeds {
        let clock = self.0.clone();
        Feeds::open_with(dir, Box::new(move || *clock.lock().unwrap())).unwrap()
    }
}

fn polite() -> Access {
    Access {
        mailto: Some("papers-desktop@example.org".to_string()),
        openalex_key: None,
    }
}

const SELECT: &str = "select=DOI,title,author,container-title,abstract,URL,issued,type,relation";

fn crossref_list(indexed: &str, created: &str, mailto: bool) -> String {
    let mailto = if mailto {
        "&mailto=papers-desktop@example.org"
    } else {
        ""
    };
    format!(
        "https://api.crossref.org/works?filter=issn:1994-0424,from-index-date:{indexed},\
         from-created-date:{created}&{SELECT}&rows=100&cursor=*{mailto}"
    )
}

#[test]
fn crossref_sweep_of_an_issn_since_the_last() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Clock::new();
    let mut feeds = clock.open(dir.path());
    let issn = Feed::Crossref {
        issn: "1994 0424".to_string(),
    };
    let tc = feeds.subscribe(issn, SourceKind::Journal).unwrap();
    assert_eq!(
        tc.feed,
        Feed::Crossref {
            issn: "1994-0424".to_string()
        }
    );
    // the first time, 14 days back; the works created within 14 days before
    let first = crossref_list("2026-10-17", "2026-10-03", true);
    // then since the last sweep
    let second = crossref_list("2026-10-31", "2026-10-17", true);
    let empty = r#"{"status":"ok","message":{"items":[],"next-cursor":"Zm9v","total-results":0}}"#;
    let http = Recorded::new()
        .reply(&first, Reply::ok(&first, fixture("crossref-tc-works.json")))
        .reply(&second, Reply::ok(&second, empty));
    let report = feeds.fetch(&http, &polite()).unwrap();
    assert_eq!(
        report.feeds[0].outcome,
        Outcome::Fetched { entries: 3, new: 3 }
    );
    assert_eq!(report.lookups, 0, "Crossref's works are whole");
    assert_eq!(http.requests().len(), 1);
    assert_eq!(
        feeds.subscriptions()[0].title.as_deref(),
        Some("The Cryosphere")
    );

    let item = &feeds.items()[0];
    assert_eq!(item.doi.as_deref(), Some("10.5194/tc-20-5813-2026"));
    assert_eq!(item.kind, ItemKind::Paper);
    assert_eq!(item.journal.as_deref(), Some("The Cryosphere"));
    assert_eq!(item.authors[0], "Louis Védrine");
    assert_eq!(
        item.date,
        Some(Utc.with_ymd_and_hms(2026, 10, 12, 0, 0, 0).unwrap())
    );
    let summary = item.summary.as_deref().unwrap();
    assert!(
        summary.starts_with("Snow is a warm, porous material"),
        "{summary}"
    );
    assert!(!summary.contains("jats"));
    let stages: Vec<(&str, Stage)> = item
        .versions
        .iter()
        .map(|v| (v.doi.as_str(), v.stage))
        .collect();
    assert_eq!(
        stages,
        [
            ("10.5194/tc-20-5813-2026", Stage::Published),
            ("10.5194/egusphere-2026-2612", Stage::Preprint)
        ],
        "the review comments are no versions"
    );

    let report = feeds.fetch(&http, &polite()).unwrap();
    assert_eq!(
        report.feeds[0].outcome,
        Outcome::Fetched { entries: 0, new: 0 }
    );
    assert_eq!(feeds.items().len(), 3, "kept, no longer listed");
    assert!(!feeds.items()[0].listed());
}

#[test]
fn a_failed_sweep_is_asked_again_from_the_same_day() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Clock::new();
    let mut feeds = clock.open(dir.path());
    let issn = Feed::Crossref {
        issn: "1994-0424".to_string(),
    };
    feeds.subscribe(issn, SourceKind::Journal).unwrap();
    feeds.set_lookback_days(7).unwrap();
    let first = crossref_list("2026-10-24", "2026-10-17", false);
    let http = Recorded::new()
        .reply(&first, Reply::status(&first, 503))
        .reply(&first, Reply::ok(&first, fixture("crossref-tc-works.json")));
    let report = feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(
        report.feeds[0].outcome,
        Outcome::Failed("HTTP 503".to_string())
    );
    let report = feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(
        report.feeds[0].outcome,
        Outcome::Fetched { entries: 3, new: 3 }
    );
    assert_eq!(clock.open(dir.path()).lookback_days(), 7);
}

const OPENALEX: &str = "https://api.openalex.org/works?filter=primary_topic.id:T10644,\
    from_created_date:2026-10-17,type:article\
    &select=id,doi,title,publication_date,authorships,primary_location,abstract_inverted_index,type\
    &per-page=100";

#[test]
fn openalex_sweep_needs_a_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Clock::new().open(dir.path());
    let filter = Feed::OpenAlex {
        filter: " primary_topic.id:T10644 ".to_string(),
    };
    feeds.subscribe(filter, SourceKind::Journal).unwrap();
    let http = Recorded::new();
    let report = feeds.fetch(&http, &polite()).unwrap();
    assert_eq!(
        report.feeds[0].outcome,
        Outcome::Failed("OpenAlex: no key".to_string())
    );
    assert!(http.requests().is_empty());
}

#[test]
fn openalex_sweep_page_by_page() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Clock::new().open(dir.path());
    let filter = Feed::OpenAlex {
        filter: "primary_topic.id:T10644".to_string(),
    };
    feeds.subscribe(filter, SourceKind::Journal).unwrap();
    let access = Access {
        mailto: Some("papers-desktop@example.org".to_string()),
        openalex_key: Some("KEY".to_string()),
    };
    let tail = "&api_key=KEY&mailto=papers-desktop@example.org";
    let page1 = format!("{OPENALEX}&cursor=*{tail}");
    let cursor = "IlsxNzkzMDAwMDAwMDAwLCAnaHR0cHM6Ly9vcGVuYWxleC5vcmcvVzQ0MTAwMDAwMDInXSI%3D";
    let page2 = format!("{OPENALEX}&cursor={cursor}{tail}");
    let http = Recorded::new()
        .reply(&page1, Reply::ok(&page1, fixture("openalex-works-1.json")))
        .reply(&page2, Reply::ok(&page2, fixture("openalex-works-2.json")));
    let report = feeds.fetch(&http, &access).unwrap();
    assert_eq!(
        report.feeds[0].outcome,
        Outcome::Fetched { entries: 3, new: 3 }
    );
    let urls: Vec<String> = http.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(urls[..2], [page1, page2]);
    // then the DOIs in Crossref, which does not answer here
    assert!(urls[2].starts_with("https://api.crossref.org/works/10.1029/2026gl120001?"));
    assert_eq!(
        urls.len(),
        3,
        "the lookups stop at the first that gets no answer"
    );

    let items = feeds.items();
    assert_eq!(
        items[0].title,
        "Ocean-driven melt of Thwaites Glacier since 2010"
    );
    assert_eq!(items[0].authors, ["Ana Ruiz", "Ben Okafor"]);
    assert_eq!(
        items[0].journal.as_deref(),
        Some("Geophysical Research Letters")
    );
    assert_eq!(
        items[0].summary.as_deref(),
        Some("Warm water reaches the grounding line the more often.")
    );
    assert_eq!(items[1].doi, None);
    assert_eq!(items[2].kind, ItemKind::Preprint);
    assert_eq!(items[2].arxiv.as_deref(), Some("2610.38522"));
}

#[test]
fn openalex_sweep_of_words_in_titles_and_abstracts() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Clock::new().open(dir.path());
    let words = Feed::OpenAlex {
        filter: "title_and_abstract.search:ice sheet AND (tipping OR \"tipping point\")"
            .to_string(),
    };
    feeds.subscribe(words, SourceKind::Journal).unwrap();
    let access = Access {
        mailto: None,
        openalex_key: Some("KEY".to_string()),
    };
    let http = Recorded::new();
    feeds.fetch(&http, &access).unwrap();
    let url = &http.requests()[0].url;
    let filter = "filter=title_and_abstract.search:ice%20sheet%20AND%20(tipping%20OR%20%22tipping%20point%22),\
        from_created_date:2026-10-17,type:article&";
    assert!(url.contains(filter), "{url}");
}

#[test]
fn subscriptions_of_three_kinds_in_config_json() {
    let dir = tempfile::tempdir().unwrap();
    // as written before sweeps: a plain url
    let before = r#"{"subscriptions":[{"id":"abc","url":"https://tc.copernicus.org/xml/rss2_0.xml",
        "title":"TC","kind":"journal","journal":null,"fetch_pages":false}]}"#;
    std::fs::write(dir.path().join("config.json"), before).unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    assert_eq!(
        feeds.subscriptions()[0].feed,
        Feed::Rss {
            url: "https://tc.copernicus.org/xml/rss2_0.xml".to_string()
        }
    );
    assert_eq!(feeds.lookback_days(), 14);
    let tc = Feed::Rss {
        url: "http://tc.copernicus.org/xml/rss2_0.xml".to_string(),
    };
    assert!(matches!(
        feeds.subscribe(tc, SourceKind::Journal),
        Err(Error::Subscribed { id, .. }) if id == "abc"
    ));
    let issn = |issn: &str| Feed::Crossref {
        issn: issn.to_string(),
    };
    feeds
        .subscribe(issn("1994-0424"), SourceKind::Journal)
        .unwrap();
    assert!(matches!(
        feeds.subscribe(issn("19940424"), SourceKind::Journal),
        Err(Error::Subscribed { .. })
    ));
    assert!(matches!(
        feeds.subscribe(issn("1994-0425"), SourceKind::Journal),
        Err(Error::Issn { .. })
    ));
    let filter = |filter: &str| Feed::OpenAlex {
        filter: filter.to_string(),
    };
    feeds
        .subscribe(filter("cites:W1|W2"), SourceKind::Journal)
        .unwrap();
    assert!(matches!(
        feeds.subscribe(filter("  "), SourceKind::Journal),
        Err(Error::Filter { .. })
    ));

    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("config.json")).unwrap()).unwrap();
    let subscriptions = json["subscriptions"].as_array().unwrap();
    assert_eq!(
        subscriptions[0]["url"],
        "https://tc.copernicus.org/xml/rss2_0.xml"
    );
    assert_eq!(subscriptions[1]["issn"], "1994-0424");
    assert_eq!(subscriptions[2]["openalex"], "cites:W1|W2");
    let reopened = Feeds::open(dir.path()).unwrap();
    assert_eq!(reopened.subscriptions(), feeds.subscriptions());
}

#[test]
fn crossref_is_paced_to_three_a_second_with_an_address() {
    let interval = |access: &Access| Pacing::polite(access).interval(CROSSREF);
    assert_eq!(interval(&polite()).as_millis(), 334);
    assert_eq!(interval(&Access::default()).as_secs(), 1);
}
