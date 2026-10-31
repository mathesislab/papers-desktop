//! The boost of a subscription: added to each interest's score of the items it
//! lists, as paper-feeder's `score_boost`.

use papers_feeds::http::Recorded;
use papers_feeds::{
    Access, Feed, Feeds, Interest, Matcher, Reply, SourceKind, Subscription, Weighted,
};

const A: &str = "https://example.org/a.atom";
const B: &str = "https://example.org/b.atom";

/// An Atom feed of entries: id, title, summary, DOI.
fn atom(entries: &[(&str, &str, &str, Option<&str>)]) -> Vec<u8> {
    let entries: String = entries
        .iter()
        .map(|(id, title, summary, doi)| {
            let doi = doi
                .map(|d| format!("<arxiv:doi>{d}</arxiv:doi>"))
                .unwrap_or_default();
            format!(
                "<entry><id>{id}</id><title>{title}</title><link href=\"https://example.org/{id}\"/>\
                 <published>2026-10-30T00:00:00Z</published><dc:creator>Moreau, A.</dc:creator>\
                 <summary>{summary}</summary>{doi}</entry>"
            )
        })
        .collect();
    format!(
        "<?xml version='1.0'?><feed xmlns=\"http://www.w3.org/2005/Atom\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:arxiv=\"http://arxiv.org/schemas/atom\">\
         <title>t</title><id>feed</id>{entries}</feed>"
    )
    .into_bytes()
}

/// An interest of the terms `firn` (2) and the veto `Georgia`, from score 3.
fn firn() -> Interest {
    Interest {
        name: "Firn".to_string(),
        include: vec![Weighted {
            matcher: Matcher::Term("firn".to_string()),
            weight: 2.0,
            label: None,
        }],
        exclude: vec![Matcher::Term("Georgia".to_string())],
        min_score: 3.0,
    }
}

/// Feeds A (boost `a`) and B (boost `b`) fetched: A lists a weak match, a
/// vetoed one, no match, and a paper that B lists too.
fn fetched(dir: &std::path::Path, a: f64, b: f64) -> Feeds {
    let mut feeds = Feeds::open(dir).unwrap();
    let mut boosted = |url: &str, boost: f64| {
        let made = feeds
            .subscribe(Feed::Rss { url: url.into() }, SourceKind::Journal)
            .unwrap();
        feeds
            .edit_subscription(&made.id, |s| s.boost = boost)
            .unwrap();
    };
    boosted(A, a);
    boosted(B, b);
    feeds.set_interests(vec![firn()], 2.0).unwrap();
    let both = Some("10.5194/tc-1-1");
    let http = Recorded::new()
        .reply(
            A,
            Reply::ok(
                A,
                atom(&[
                    ("weak", "Melt", "The firn layer.", None),
                    ("vetoed", "Firn in Georgia", "", None),
                    ("none", "Unrelated", "Nothing here.", None),
                    ("both", "Shared", "The firn.", both),
                ]),
            ),
        )
        .reply(
            B,
            Reply::ok(B, atom(&[("both", "Shared", "The firn.", both)])),
        );
    feeds.fetch(&http, &Access::default()).unwrap();
    feeds
}

/// The score and whether it reaches the interest, of the item of a title.
fn scored(feeds: &Feeds, title: &str) -> (f64, bool) {
    let item = feeds.items().iter().find(|i| i.title == title).unwrap();
    let score = &feeds.scores(item)[0];
    (score.score, score.reached)
}

#[test]
fn a_boost_adds_to_the_score_of_the_items_a_subscription_lists() {
    let dir = tempfile::tempdir().unwrap();
    let feeds = fetched(dir.path(), 0.0, 0.0);
    assert_eq!(scored(&feeds, "Melt"), (2.0, false));
    assert_eq!(scored(&feeds, "Unrelated"), (0.0, false));
    let dir = tempfile::tempdir().unwrap();
    let feeds = fetched(dir.path(), 2.0, 0.0);
    assert_eq!(
        scored(&feeds, "Melt"),
        (4.0, true),
        "reaches with the boost"
    );
    assert_eq!(
        scored(&feeds, "Unrelated"),
        (2.0, false),
        "a boost counts with no term matching"
    );
    assert_eq!(
        scored(&feeds, "Firn in Georgia"),
        (0.0, false),
        "a veto stays 0"
    );
    let dir = tempfile::tempdir().unwrap();
    let feeds = fetched(dir.path(), 3.0, 0.0);
    assert_eq!(scored(&feeds, "Unrelated"), (3.0, true));
}

#[test]
fn the_strongest_boost_counts_for_a_merged_item() {
    let dir = tempfile::tempdir().unwrap();
    let feeds = fetched(dir.path(), 1.0, 3.0);
    let both = feeds.items().iter().find(|i| i.title == "Shared").unwrap();
    assert_eq!(both.sources.len(), 2, "listed by A and B");
    assert_eq!(scored(&feeds, "Shared"), (2.0 + 3.0, true), "not 1 + 3");
    assert_eq!(scored(&feeds, "Melt"), (3.0, true), "A's alone");
    let dir = tempfile::tempdir().unwrap();
    let feeds = fetched(dir.path(), -1.0, -2.0);
    assert_eq!(scored(&feeds, "Shared"), (1.0, false));
}

#[test]
fn the_boost_is_in_the_queries_and_in_config_json() {
    let dir = tempfile::tempdir().unwrap();
    let feeds = fetched(dir.path(), 2.0, 0.0);
    let titles = |feeds: &Feeds, query: &str| -> Vec<String> {
        let request = papers_feeds::ViewRequest {
            query,
            ..papers_feeds::ViewRequest::default()
        };
        let rows = feeds.view(&request).rows;
        let mut titles: Vec<String> = rows
            .iter()
            .map(|r| feeds.item(&r.id).unwrap().title.clone())
            .collect();
        titles.sort();
        titles
    };
    assert_eq!(titles(&feeds, "prof:Firn"), ["Melt", "Shared"]);
    assert_eq!(titles(&feeds, "score:4-"), ["Melt", "Shared"]);

    // zero is not written, the other is, and both come back
    let text = std::fs::read_to_string(dir.path().join("config.json")).unwrap();
    assert_eq!(text.matches("\"boost\"").count(), 1, "{text}");
    let reopened = Feeds::open(dir.path()).unwrap();
    let boosts: Vec<f64> = reopened.subscriptions().iter().map(|s| s.boost).collect();
    assert_eq!(boosts, [2.0, 0.0]);
    // a subscription as written before the boost
    let old: Subscription = serde_json::from_str(
        r#"{"id":"x","url":"https://example.org/x","title":null,"kind":"blog"}"#,
    )
    .unwrap();
    assert_eq!(old.boost, 0.0);
}
