//! Saved views: kept in `config.json`, their names checked, their counts in
//! the view; and the removed items, which a query shows when it names them.

use papers_feeds::http::Recorded;
use papers_feeds::{
    Access, Error, Feed, Feeds, Reply, SavedView, Sort, SourceKind, SubscriptionSettings, Triage,
    ViewRequest,
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

fn view(name: &str, query: &str, sort: Sort) -> SavedView {
    SavedView {
        name: name.to_string(),
        query: query.to_string(),
        sort,
    }
}

/// A journal with three papers, the first removed.
fn feeds(dir: &std::path::Path) -> Feeds {
    let mut feeds = Feeds::open(dir).unwrap();
    feeds
        .subscribe(
            Feed::Rss { url: TC.into() },
            SubscriptionSettings::new(SourceKind::Journal),
        )
        .unwrap();
    let papers = [("a", "Ice sheet melt"), ("b", "Sea ice"), ("c", "Firn")];
    let http = Recorded::new().reply(TC, Reply::ok(TC, atom(&papers)));
    feeds.fetch(&http, &Access::default()).unwrap();
    let first = feeds.items().iter().find(|i| i.title == "Ice sheet melt");
    let id = first.unwrap().id.clone();
    feeds.set_triage(&id, Triage::Removed).unwrap();
    feeds
}

#[test]
fn saved_views_are_kept_and_named_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = feeds(dir.path());
    assert!(feeds.saved_views().is_empty());
    let saved = vec![
        view(" Ice ", "ice kind:paper", Sort::Score),
        view("All", "", Sort::Newest),
    ];
    feeds.set_saved_views(saved).unwrap();
    let feeds = Feeds::open(dir.path()).unwrap();
    assert_eq!(
        feeds.saved_views(),
        [
            view("Ice", "ice kind:paper", Sort::Score),
            view("All", "", Sort::Newest)
        ],
        "in their order, the names trimmed"
    );
    assert_eq!(
        feeds.saved_view("ICE").map(|s| s.query.as_str()),
        Some("ice kind:paper")
    );
    assert!(feeds.saved_view("none").is_none());

    // a blank name, or one given twice in any case: nothing changes
    let mut feeds = feeds;
    let refused = |feeds: &mut Feeds, names: &[&str]| {
        let views = names.iter().map(|n| view(n, "", Sort::Newest)).collect();
        match feeds.set_saved_views(views) {
            Err(Error::View { name, .. }) => name,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(refused(&mut feeds, &["a", "  "]), "");
    assert_eq!(refused(&mut feeds, &["Ice", "ice"]), "ice");
    assert_eq!(feeds.saved_views().len(), 2);

    // a config.json without them, or a view without its sort, under the key of before
    let config = dir.path().join("config.json");
    std::fs::write(
        &config,
        r#"{"searches": [{"name": "x", "query": "is:new"}]}"#,
    )
    .unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    assert_eq!(feeds.saved_views(), [view("x", "is:new", Sort::Newest)]);
    // written under the key of now
    let views = feeds.saved_views().to_vec();
    feeds.set_saved_views(views).unwrap();
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(written["views"][0]["name"], "x");
    assert!(written.get("searches").is_none());
    std::fs::write(&config, "{}").unwrap();
    assert!(Feeds::open(dir.path()).unwrap().saved_views().is_empty());
}

#[test]
fn the_counts_of_the_saved_views_are_what_they_show() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = feeds(dir.path());
    feeds
        .set_saved_views(vec![
            view("Ice", "ice", Sort::Newest),
            view("Removed", "is:removed", Sort::Newest),
            view("Everything", "", Sort::Newest),
            view("None", "glacier", Sort::Newest),
        ])
        .unwrap();
    // whatever the query of the request
    let request = ViewRequest {
        query: "kind:news firn",
        ..ViewRequest::default()
    };
    let counts = feeds.view(&request).counts.views;
    let count = |name: &str| counts.get(name).copied();
    assert_eq!(count("Ice"), Some(1), "the removed one left out");
    assert_eq!(count("Removed"), Some(1));
    assert_eq!(count("Everything"), Some(2));
    assert_eq!(count("None"), None);
}

#[test]
fn a_query_shows_the_removed_items_when_it_names_them() {
    let dir = tempfile::tempdir().unwrap();
    let feeds = feeds(dir.path());
    let rows = |query: &str, removed: bool| {
        let request = ViewRequest {
            query,
            removed,
            ..ViewRequest::default()
        };
        feeds.view(&request).rows.len()
    };
    assert_eq!(rows("", false), 2);
    assert_eq!(rows("ice", false), 1);
    assert_eq!(rows("is:removed", false), 1);
    assert_eq!(rows("ice -IS:seen,removed", false), 1);
    assert_eq!(rows("is:new,\"removed\"", false), 3);
    assert_eq!(rows("ice", true), 2, "asked for whatever the query");
}
