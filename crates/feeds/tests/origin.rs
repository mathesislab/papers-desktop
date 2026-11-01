//! Where an entry's data came from: a feed's text, or a work of Crossref or
//! OpenAlex, whatever it was fetched under; the entries of before told apart
//! when their folder opens (and the pages read, kept by their post's id, kept
//! by their URL); `src:` and the sidebar's sources only the subscriptions.

use papers_feeds::{
    Entry, FOUND, Feed, Feeds, ItemKind, LOOKUP, Origin, SourceKind, SubscriptionSettings, Version,
    ViewRequest,
};
use serde_json::Value;

/// An entry of a source, its DOI and an abstract long enough not to be a teaser.
fn entry(source: &str, guid: &str, doi: Option<&str>, origin: Origin) -> Entry {
    Entry {
        source: source.to_string(),
        origin,
        guid: guid.to_string(),
        title: format!("Paper {guid}"),
        authors: vec![format!("Author {guid}")],
        journal: None,
        date: None,
        doi: doi.map(String::from),
        arxiv: None,
        url: Some(format!("https://example.org/{guid}")),
        summary: Some("Ice melts. ".repeat(40)),
        content: None,
        editorial: false,
        kind: ItemKind::Paper,
        versions: Vec::new(),
        cited: None,
    }
}

/// The origin of the entry of a guid.
fn origin_of(feeds: &Feeds, guid: &str) -> Origin {
    let entries = feeds.items().iter().flat_map(|i| &i.sources);
    let entry = entries.map(|s| &s.entry).find(|e| e.guid == guid);
    entry.unwrap().origin
}

#[test]
fn a_work_found_by_a_search_is_a_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let work = entry(
        FOUND,
        "https://openalex.org/W1",
        Some("10.1/w1"),
        Origin::Work,
    );
    let ids = feeds.found(vec![work], true).unwrap();
    let item = feeds.item(&ids[0]).unwrap();
    assert!(
        feeds.abstract_from_work(item),
        "OpenAlex's abstract, not a feed's"
    );
}

#[test]
fn a_folder_of_before_is_brought_up_to_date_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let rss = Feed::Rss {
        url: "https://example.org/news.rss".to_string(),
    };
    let news = feeds
        .subscribe(rss, SubscriptionSettings::new(SourceKind::News))
        .unwrap()
        .id;
    let issn = Feed::Crossref {
        issn: "1994-0424".to_string(),
    };
    let sweep = feeds
        .subscribe(issn, SubscriptionSettings::new(SourceKind::Journal))
        .unwrap()
        .id;
    let mut post = entry(&news, "post", None, Origin::Feed);
    post.kind = ItemKind::News;
    // the paper of the post, from Crossref: its own DOI among its versions
    let mut paper = entry(&news, "10.1/paper", Some("10.1/paper"), Origin::Work);
    paper.versions = vec![Version {
        doi: "10.1/paper".to_string(),
        stage: papers_feeds::Stage::Published,
    }];
    let openalex = entry(
        &news,
        "https://openalex.org/W2",
        Some("10.1/w2"),
        Origin::Work,
    );
    let lookup = entry(LOOKUP, "10.1/lookup", Some("10.1/lookup"), Origin::Work);
    let found = entry(FOUND, "found", Some("10.1/found"), Origin::Work);
    let swept = entry(&sweep, "10.1/swept", Some("10.1/swept"), Origin::Work);
    let entries = vec![post, paper, openalex, lookup, found, swept];
    feeds.found(entries, false).unwrap();

    // as written before entries had an origin, and the page read of the post
    // kept by its id; one by its URL, as a migration cut short left it
    let post = feeds.items().iter().find(|i| i.title == "Paper post");
    let read = format!(
        r#"{{"page:{}": "2026-10-30T00:00:00Z",
            "page:https://example.org/other": "2026-10-30T00:00:00Z"}}"#,
        post.unwrap().id
    );
    std::fs::write(dir.path().join("lookups.json"), read).unwrap();
    let path = dir.path().join("items.json");
    let mut json: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    json.as_object_mut().unwrap().remove("format");
    for item in json["items"].as_array_mut().unwrap() {
        for source in item["sources"].as_array_mut().unwrap() {
            source["entry"].as_object_mut().unwrap().remove("origin");
        }
    }
    std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();

    let mut feeds = Feeds::open(dir.path()).unwrap();
    assert_eq!(origin_of(&feeds, "post"), Origin::Feed);
    for work in [
        "10.1/paper",
        "https://openalex.org/W2",
        "10.1/lookup",
        "found",
        "10.1/swept",
    ] {
        assert_eq!(origin_of(&feeds, work), Origin::Work, "{work}");
    }
    let written: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(written["format"], 3, "once");
    let lookups = std::fs::read_to_string(dir.path().join("lookups.json")).unwrap();
    let lookups: Value = serde_json::from_str(&lookups).unwrap();
    let keys: Vec<&String> = lookups.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        [
            "page:https://example.org/other",
            "page:https://example.org/post"
        ]
    );

    // a sweep's item stays a work once the sweep is unsubscribed
    feeds.unsubscribe(&sweep).unwrap();
    let swept = (feeds.items().iter())
        .find(|i| i.doi.as_deref() == Some("10.1/swept"))
        .unwrap();
    assert!(feeds.abstract_from_work(swept));
}

#[test]
fn src_and_the_sources_are_the_subscriptions() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let rss = Feed::Rss {
        url: "https://example.org/tc.rss".to_string(),
    };
    let tc = feeds
        .subscribe(rss, SubscriptionSettings::new(SourceKind::Journal))
        .unwrap()
        .id;
    let listed = entry(&tc, "a", Some("10.1/a"), Origin::Feed);
    let looked_up = entry(LOOKUP, "10.1/a", Some("10.1/a"), Origin::Work);
    feeds.found(vec![listed, looked_up], false).unwrap();
    let view = |query: &str| {
        feeds.view(&ViewRequest {
            query,
            ..ViewRequest::default()
        })
    };
    assert_eq!(view(&format!("src:{tc}")).rows.len(), 1);
    assert!(
        view("src:lookup").rows.is_empty(),
        "a lookup is no subscription"
    );
    let sources = view("").counts.total.sources;
    assert_eq!(sources.keys().collect::<Vec<_>>(), [&tc]);
}
