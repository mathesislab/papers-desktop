//! Editing a subscription: what the user names or sets stays, a blank title or journal is none, its
//! id and feed do not change.

use papers_feeds::{Error, Feed, Feeds, SourceKind, SubscriptionSettings};

#[test]
fn a_subscription_is_named_and_keeps_its_feed() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let feed = Feed::Crossref {
        issn: "1994-0424".to_string(),
    };
    let made = feeds
        .subscribe(feed.clone(), SubscriptionSettings::new(SourceKind::Journal))
        .unwrap();
    let settings = SubscriptionSettings {
        title: Some(" TC ".to_string()),
        journal: Some("The Cryosphere".to_string()),
        fetch_pages: true,
        ..made.settings.clone()
    };
    let edited = feeds.edit_subscription(&made.id, settings).unwrap();
    assert_eq!(
        (edited.id.as_str(), &edited.feed),
        (made.id.as_str(), &feed)
    );
    assert_eq!(edited.settings.title.as_deref(), Some("TC"), "trimmed");
    let reopened = Feeds::open(dir.path()).unwrap();
    assert_eq!(reopened.subscriptions(), std::slice::from_ref(&edited));
    assert_eq!(
        reopened.subscriptions()[0].settings.journal.as_deref(),
        Some("The Cryosphere")
    );
    let blank = SubscriptionSettings {
        title: Some("  ".to_string()),
        ..edited.settings
    };
    let edited = feeds.edit_subscription(&made.id, blank).unwrap();
    assert_eq!(edited.settings.title, None, "the feed's own again");
    assert!(matches!(
        feeds.edit_subscription("nope", made.settings),
        Err(Error::NoSubscription { .. })
    ));
}
