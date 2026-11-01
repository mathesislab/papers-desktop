//! The subscriptions as the settings edit them: OPML out and in, and the
//! suggested list.

use std::collections::HashSet;

use papers_feeds::{Error, Feed, Feeds, SourceKind, SubscriptionSettings, suggested};

fn rss(url: &str) -> Feed {
    Feed::Rss {
        url: url.to_string(),
    }
}

#[test]
fn opml_out_and_back_in() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let eos = SubscriptionSettings {
        title: Some("Eos & \"friends\"".to_string()),
        ..SubscriptionSettings::new(SourceKind::News)
    };
    feeds
        .subscribe(rss("https://eos.org/feed?a=1&b=2"), eos)
        .unwrap();
    let issn = Feed::Crossref {
        issn: "0022-1430".into(),
    };
    feeds
        .subscribe(issn, SubscriptionSettings::new(SourceKind::Journal))
        .unwrap();
    let filter = Feed::OpenAlex {
        filter: "primary_topic.id:T10644".into(),
    };
    feeds
        .subscribe(filter, SubscriptionSettings::new(SourceKind::Journal))
        .unwrap();
    let opml = feeds.opml();
    assert!(
        opml.contains("xmlUrl=\"https://eos.org/feed?a=1&amp;b=2\""),
        "{opml}"
    );
    assert!(opml.contains("category=\"news\""), "{opml}");

    let other = tempfile::tempdir().unwrap();
    let mut imported = Feeds::open(other.path()).unwrap();
    assert_eq!(imported.import_opml(&opml).unwrap(), 3);
    let of = |feeds: &Feeds| -> Vec<(Feed, SourceKind)> {
        let subscriptions = feeds.subscriptions().iter();
        subscriptions
            .map(|s| (s.feed.clone(), s.settings.kind))
            .collect()
    };
    assert_eq!(of(&imported), of(&feeds));
    assert_eq!(
        imported.subscriptions()[0].settings.title.as_deref(),
        Some("Eos & \"friends\"")
    );
    // an untitled one is named as it shows
    let title = imported.subscriptions()[1].settings.title.as_deref();
    assert_eq!(title, Some("Crossref ISSN 0022-1430"));
    assert_eq!(
        imported.import_opml(&opml).unwrap(),
        0,
        "subscribed already"
    );
}

#[test]
fn opml_of_another_reader() {
    let opml = r#"<?xml version="1.0"?>
        <opml version="1.0"><head><title>Mine</title></head><body>
          <outline text="Journals">
            <outline type="rss" text="The Cryosphere" xmlUrl="https://tc.copernicus.org/xml/rss2_0.xml"/>
          </outline>
          <outline title="Science news">
            <outline type="rss" text="Eos" xmlUrl="https://eos.org/feed"></outline>
            <outline type="rss" text="Bad" xmlUrl="ftp://example.org/feed"/>
          </outline>
          <outline text="Blogs"><outline text="RealClimate" xmlUrl="https://www.realclimate.org/index.php/feed/"/></outline>
          <outline type="rss" text="Loose" xmlUrl="https://example.org/feed"/>
        </body></opml>"#;
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    assert_eq!(
        feeds.import_opml(opml).unwrap(),
        4,
        "the bad address passed over"
    );
    let got: Vec<(Option<&str>, SourceKind)> = feeds
        .subscriptions()
        .iter()
        .map(|s| (s.settings.title.as_deref(), s.settings.kind))
        .collect();
    assert_eq!(
        got,
        [
            (Some("The Cryosphere"), SourceKind::Journal),
            (Some("Eos"), SourceKind::News),
            (Some("RealClimate"), SourceKind::Blog),
            (Some("Loose"), SourceKind::Journal),
        ]
    );
    assert!(matches!(
        feeds.import_opml("<rss></rss>"),
        Err(Error::Opml { .. })
    ));
}

#[test]
fn the_suggested_subscriptions_are_valid_and_distinct() {
    let list = suggested();
    assert!(list.len() > 25);
    let keys: HashSet<String> = list
        .iter()
        .map(|s| s.feed.canonical().unwrap().key())
        .collect();
    assert_eq!(keys.len(), list.len());
    let groups: HashSet<&str> = list.iter().map(|s| s.group.as_str()).collect();
    for group in ["Copernicus", "Nature", "AGU", "News", "Blogs", "Preprints"] {
        assert!(groups.contains(group), "{group}");
    }
    let egusphere = list.iter().find(|s| s.title == "EGUsphere").unwrap();
    assert_eq!(egusphere.kind, SourceKind::Preprints);
    // subscribed as they are
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    for s in &list {
        let settings = SubscriptionSettings::new(s.kind);
        feeds.subscribe(s.feed.clone(), settings).unwrap();
    }
}

#[test]
fn another_kind_moves_the_items_and_reads_the_feed_whole_again() {
    use papers_feeds::http::Recorded;
    use papers_feeds::{Access, ItemKind, Reply};

    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let url = "https://xkcd.com/rss.xml";
    let comic = feeds
        .subscribe(rss(url), SubscriptionSettings::new(SourceKind::Journal))
        .unwrap();
    let body = "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>xkcd.com</title>\
        <item><title>Earth Sized Planets</title><link>https://xkcd.com/3000/</link>\
        <description>&lt;img src=\"https://imgs.xkcd.com/comics/planets.png\" /&gt;</description>\
        </item></channel></rss>";
    let http = Recorded::new()
        .reply(
            url,
            Reply::ok(url, body).with_validators(Some("\"v1\""), None),
        )
        .reply(
            url,
            Reply::ok(url, body).with_validators(Some("\"v1\""), None),
        );
    feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(feeds.items()[0].kind, ItemKind::Paper);
    assert_eq!(feeds.items()[0].content, None, "a paper keeps no HTML");

    let blog = SubscriptionSettings::new(SourceKind::Blog);
    feeds.edit_subscription(&comic.id, blog).unwrap();
    assert_eq!(feeds.items()[0].kind, ItemKind::Blog, "at once");
    feeds.fetch(&http, &Access::default()).unwrap();
    let asked = &http.requests()[1];
    assert_eq!(asked.if_none_match, None, "the feed read whole again");
    let content = feeds.items()[0].content.as_deref().unwrap();
    assert!(
        content.contains("<img src=\"https://imgs.xkcd.com/comics/planets.png\""),
        "{content}"
    );
}
