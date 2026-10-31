//! Saved searches: kept in `config.json`, their names checked, their counts
//! in the view; and a query made live.

use papers_feeds::http::Recorded;
use papers_feeds::query::live;
use papers_feeds::{
    Access, Error, Feed, Feeds, Reply, Search, Sort, SourceKind, Triage, ViewRequest,
};

const TC: &str = "https://example.org/tc.atom";

fn atom(entries: &[(&str, &str)]) -> Vec<u8> {
    let entries: String = entries
        .iter()
        .map(|(id, title)| {
            format!(
                "<entry><id>{id}</id><title>{title}</title><link href=\"https://example.org/{id}\"/>\
                 <published>2026-10-30T00:00:00Z</published></entry>"
            )
        })
        .collect();
    format!(
        "<?xml version='1.0'?><feed xmlns=\"http://www.w3.org/2005/Atom\">\
         <title>The Cryosphere</title><id>feed</id>{entries}</feed>"
    )
    .into_bytes()
}

fn search(name: &str, query: &str, sort: Sort) -> Search {
    Search {
        name: name.to_string(),
        query: query.to_string(),
        sort,
    }
}

/// A journal with three papers, the first dismissed.
fn feeds(dir: &std::path::Path) -> Feeds {
    let mut feeds = Feeds::open(dir).unwrap();
    feeds
        .subscribe(Feed::Rss { url: TC.into() }, SourceKind::Journal)
        .unwrap();
    let papers = [("a", "Ice sheet melt"), ("b", "Sea ice"), ("c", "Firn")];
    let http = Recorded::new().reply(TC, Reply::ok(TC, atom(&papers)));
    feeds.fetch(&http, &Access::default()).unwrap();
    let first = feeds.items().iter().find(|i| i.title == "Ice sheet melt");
    let id = first.unwrap().id.clone();
    feeds.set_triage(&id, Triage::Dismissed).unwrap();
    feeds
}

#[test]
fn saved_searches_are_kept_and_named_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = feeds(dir.path());
    assert!(feeds.searches().is_empty());
    let saved = vec![
        search(" Ice ", "ice kind:paper", Sort::Score),
        search("All", "", Sort::Newest),
    ];
    feeds.set_searches(saved).unwrap();
    let feeds = Feeds::open(dir.path()).unwrap();
    assert_eq!(
        feeds.searches(),
        [
            search("Ice", "ice kind:paper", Sort::Score),
            search("All", "", Sort::Newest)
        ],
        "in their order, the names trimmed"
    );
    assert_eq!(
        feeds.saved_search("ICE").map(|s| s.query.as_str()),
        Some("ice kind:paper")
    );
    assert!(feeds.saved_search("none").is_none());

    // a blank name, or one given twice in any case: nothing changes
    let mut feeds = feeds;
    let refused = |feeds: &mut Feeds, names: &[&str]| {
        let searches = names.iter().map(|n| search(n, "", Sort::Newest)).collect();
        match feeds.set_searches(searches) {
            Err(Error::Search { name, .. }) => name,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(refused(&mut feeds, &["a", "  "]), "");
    assert_eq!(refused(&mut feeds, &["Ice", "ice"]), "ice");
    assert_eq!(feeds.searches().len(), 2);

    // a config.json without them, or a search without its sort
    let config = dir.path().join("config.json");
    std::fs::write(
        &config,
        r#"{"searches": [{"name": "x", "query": "is:new"}]}"#,
    )
    .unwrap();
    let feeds = Feeds::open(dir.path()).unwrap();
    assert_eq!(feeds.searches(), [search("x", "is:new", Sort::Newest)]);
    std::fs::write(&config, "{}").unwrap();
    assert!(Feeds::open(dir.path()).unwrap().searches().is_empty());
}

#[test]
fn the_counts_of_the_saved_searches_are_what_they_show() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = feeds(dir.path());
    feeds
        .set_searches(vec![
            search("Ice", "ice", Sort::Newest),
            search("Dismissed", "is:dismissed", Sort::Newest),
            search("Everything", "", Sort::Newest),
            search("None", "glacier", Sort::Newest),
        ])
        .unwrap();
    // whatever the query and the kinds of the request
    let request = ViewRequest {
        query: "firn",
        kinds: &[papers_feeds::ItemKind::News],
        ..ViewRequest::default()
    };
    let counts = feeds.view(&request).counts.searches;
    let count = |name: &str| counts.get(name).copied();
    assert_eq!(count("Ice"), Some(1), "the dismissed one left out");
    assert_eq!(count("Dismissed"), Some(1));
    assert_eq!(count("Everything"), Some(2));
    assert_eq!(count("None"), None);
}

#[test]
fn a_live_query_leaves_dismissed_items_out_unless_it_asks() {
    assert_eq!(live(""), "-is:dismissed");
    assert_eq!(live("ice"), "ice -is:dismissed");
    assert_eq!(live("is:dismissed"), "is:dismissed");
    assert_eq!(live("x -IS:seen,dismissed"), "x -IS:seen,dismissed");
    assert_eq!(live("is:new,\"dismissed\""), "is:new,\"dismissed\"");
    assert_eq!(live("dismissed is:new"), "dismissed is:new -is:dismissed");
}
