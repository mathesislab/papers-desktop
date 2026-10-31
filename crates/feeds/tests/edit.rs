//! Editing a subscription: what the user names stays, its id and feed do not change.

use papers_feeds::{Error, Feed, Feeds, SourceKind};

#[test]
fn a_subscription_is_named_and_keeps_its_feed() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let feed = Feed::Crossref {
        issn: "1994-0424".to_string(),
    };
    let made = feeds.subscribe(feed.clone(), SourceKind::Journal).unwrap();
    let edited = feeds
        .edit_subscription(&made.id, |s| {
            s.title = Some("TC".to_string());
            s.journal = Some("The Cryosphere".to_string());
            s.fetch_pages = true;
            s.id = "other".to_string();
            s.feed = Feed::Crossref {
                issn: "0000-0000".to_string(),
            };
        })
        .unwrap();
    assert_eq!((edited.id.as_str(), &edited.feed), (made.id.as_str(), &feed));
    let reopened = Feeds::open(dir.path()).unwrap();
    assert_eq!(reopened.subscriptions(), [edited]);
    assert_eq!(
        reopened.subscriptions()[0].journal.as_deref(),
        Some("The Cryosphere")
    );
    assert!(matches!(
        feeds.edit_subscription("nope", |_| {}),
        Err(Error::NoSubscription { .. })
    ));
}
