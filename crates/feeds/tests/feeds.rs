//! The feeds of a folder: subscriptions, fetches (conditional, failing,
//! merging), triage, pruning, and the files across a reopening.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use papers_feeds::http::Recorded;
use papers_feeds::normalize::short_id;
use papers_feeds::{
    Access, Error, Feed, FeedSettings, Feeds, Outcome, Reply, SourceKind, SubscriptionSettings,
    Triage,
};

const TC: &str = "https://tc.copernicus.org/xml/rss2_0.xml";
const NGEO: &str = "https://www.nature.com/ngeo.rss";
const ARXIV: &str = "https://rss.arxiv.org/atom/physics.ao-ph";
const FUTURITY: &str = "https://www.futurity.org/feed/";

fn rss(url: &str) -> Feed {
    Feed::Rss {
        url: url.to_string(),
    }
}

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

    fn advance(&self, days: i64) {
        *self.0.lock().unwrap() += TimeDelta::days(days);
    }

    fn open(&self, dir: &std::path::Path) -> Feeds {
        let clock = self.0.clone();
        Feeds::open_with(dir, Box::new(move || *clock.lock().unwrap())).unwrap()
    }
}

/// An Atom feed of entries (id, title, author, DOI).
fn atom(entries: &[(&str, &str, &str, Option<&str>)]) -> Vec<u8> {
    let entries: String = entries
        .iter()
        .map(|(id, title, author, doi)| {
            let doi = doi.map(|d| format!("<arxiv:doi>{d}</arxiv:doi>")).unwrap_or_default();
            format!(
                "<entry><id>{id}</id><title>{title}</title><link href=\"https://example.org/{id}\"/>\
                 <published>2026-10-30T00:00:00Z</published><dc:creator>{author}</dc:creator>{doi}</entry>"
            )
        })
        .collect();
    format!(
        "<?xml version='1.0'?><feed xmlns=\"http://www.w3.org/2005/Atom\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:arxiv=\"http://arxiv.org/schemas/atom\">\
         <title>Test</title><id>feed</id>{entries}</feed>"
    )
    .into_bytes()
}

#[test]
fn a_feed_is_subscribed_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let tc = feeds
        .subscribe(rss(TC), SubscriptionSettings::new(SourceKind::Journal))
        .unwrap();
    assert_eq!(tc.feed, rss(TC));
    for again in [
        "http://tc.copernicus.org/xml/rss2_0.xml",
        "https://TC.Copernicus.ORG/xml/rss2_0.xml#latest",
    ] {
        match feeds.subscribe(rss(again), SubscriptionSettings::new(SourceKind::Journal)) {
            Err(Error::Subscribed { id, .. }) => assert_eq!(id, tc.id),
            other => panic!("{again}: {other:?}"),
        }
    }
    assert!(matches!(
        feeds.subscribe(
            rss("tc.copernicus.org"),
            SubscriptionSettings::new(SourceKind::Journal)
        ),
        Err(Error::Url { .. })
    ));
    assert_eq!(feeds.subscriptions().len(), 1);
    assert_eq!(feeds.unsubscribe(&tc.id).unwrap().id, tc.id);
    assert!(matches!(
        feeds.unsubscribe(&tc.id),
        Err(Error::NoSubscription { .. })
    ));
    assert!(
        feeds
            .subscribe(rss(TC), SubscriptionSettings::new(SourceKind::Journal))
            .is_ok()
    );
}

#[test]
fn fetch_merge_triage_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Clock::new();
    let mut feeds = clock.open(dir.path());
    let tc = feeds
        .subscribe(rss(TC), SubscriptionSettings::new(SourceKind::Journal))
        .unwrap();
    feeds
        .subscribe(rss(NGEO), SubscriptionSettings::new(SourceKind::Journal))
        .unwrap();
    feeds
        .subscribe(rss(ARXIV), SubscriptionSettings::new(SourceKind::Preprints))
        .unwrap();
    feeds
        .subscribe(rss(FUTURITY), SubscriptionSettings::new(SourceKind::News))
        .unwrap();
    let http = Recorded::new()
        .reply(TC, Reply::ok(TC, fixture("copernicus-tc.xml")))
        .reply(NGEO, Reply::ok(NGEO, fixture("nature-ngeo.rss")))
        .reply(ARXIV, Reply::ok(ARXIV, fixture("arxiv-ao-ph.atom")))
        .reply(FUTURITY, Reply::ok(FUTURITY, fixture("futurity.xml")));
    let report = feeds.fetch(&http, &Access::default()).unwrap();
    let outcomes: Vec<&Outcome> = report.feeds.iter().map(|f| &f.outcome).collect();
    assert_eq!(
        outcomes,
        [
            &Outcome::Fetched { entries: 3, new: 3 },
            &Outcome::Fetched { entries: 3, new: 3 },
            &Outcome::Fetched { entries: 4, new: 4 },
            &Outcome::Fetched { entries: 2, new: 2 },
        ]
    );
    assert_eq!(feeds.items().len(), 12);
    assert_eq!(
        feeds.subscriptions()[0].settings.title.as_deref(),
        Some("TC - recent papers")
    );
    let paper = feeds.items()[0].clone();
    assert_eq!(paper.doi.as_deref(), Some("10.5194/tc-20-6181-2026"));
    assert_eq!(paper.sources[0].entry.source, tc.id);
    assert_eq!(feeds.triage(&paper.id), Triage::New);
    clock.advance(1);
    feeds.set_triage(&paper.id, Triage::Starred).unwrap();
    assert!(matches!(
        feeds.set_triage("nope", Triage::Seen),
        Err(Error::NoItem { .. })
    ));

    let reopened = clock.open(dir.path());
    assert_eq!(reopened.subscriptions(), feeds.subscriptions());
    assert_eq!(reopened.items(), feeds.items());
    let mark = reopened.mark(&paper.id).unwrap();
    assert_eq!(mark.triage, Triage::Starred);
    assert_eq!(mark.at, Utc.with_ymd_and_hms(2026, 11, 1, 8, 0, 0).unwrap());
    let mut files: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(
        files,
        [
            "config.json",
            "http.json",
            "items.json",
            "lookups.json",
            "state.json"
        ],
        "no temporary file left"
    );
}

#[test]
fn an_unchanged_feed_is_not_fetched_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    feeds
        .subscribe(rss(TC), SubscriptionSettings::new(SourceKind::Journal))
        .unwrap();
    let modified = "Fri, 30 Oct 2026 21:05:25 GMT";
    let http = Recorded::new()
        .reply(
            TC,
            Reply::ok(TC, fixture("copernicus-tc.xml"))
                .with_validators(Some("\"v1\""), Some(modified)),
        )
        .reply(TC, Reply::status(TC, 304));
    feeds.fetch(&http, &Access::default()).unwrap();
    let items = feeds.items().to_vec();
    // the validators are kept across a reopening
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let report = feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(report.feeds[0].outcome, Outcome::Unchanged);
    assert_eq!(feeds.items(), items);
    // and the lookups of the papers' DOIs, which get no answer
    let requests: Vec<_> = http
        .requests()
        .into_iter()
        .filter(|r| r.url == TC)
        .collect();
    assert_eq!(requests[0].if_none_match, None);
    assert_eq!(requests[1].if_none_match.as_deref(), Some("\"v1\""));
    assert_eq!(requests[1].if_modified_since.as_deref(), Some(modified));
    assert!(
        requests[1]
            .accept
            .as_deref()
            .unwrap()
            .contains("application/rss+xml")
    );
}

#[test]
fn a_failing_feed_does_not_stop_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let broken = "https://example.org/gone.xml";
    let blocked = "https://agupubs.onlinelibrary.wiley.com/feed/19448007/most-recent";
    let challenge = "https://example.org/challenge.xml";
    for url in [broken, blocked, challenge, TC] {
        feeds
            .subscribe(rss(url), SubscriptionSettings::new(SourceKind::Journal))
            .unwrap();
    }
    let mut throttled = Reply::status(blocked, 429);
    throttled.retry_after = Some("120".to_string());
    let page = Reply::ok(challenge, "<html><body>Just a moment</body></html>")
        .with_validators(Some("\"c\""), None);
    let http = Recorded::new()
        .fail(broken, "connection refused")
        .reply(blocked, throttled)
        .reply(challenge, page)
        .reply(TC, Reply::ok(TC, fixture("copernicus-tc.xml")))
        .reply(challenge, Reply::status(challenge, 404));
    let report = feeds.fetch(&http, &Access::default()).unwrap();
    let outcomes: Vec<Outcome> = report.feeds.into_iter().map(|f| f.outcome).collect();
    assert_eq!(
        outcomes[0],
        Outcome::Failed("connection refused".to_string())
    );
    assert_eq!(
        outcomes[1],
        Outcome::Failed("HTTP 429, retry after 120".to_string())
    );
    assert!(
        matches!(&outcomes[2], Outcome::Failed(m) if m.starts_with(challenge)),
        "{:?}",
        outcomes[2]
    );
    assert_eq!(outcomes[3], Outcome::Fetched { entries: 3, new: 3 });
    // a feed that did not parse is asked for again whole
    feeds.fetch(&http, &Access::default()).unwrap();
    let again = http
        .requests()
        .into_iter()
        .filter(|r| r.url == challenge)
        .nth(1)
        .unwrap();
    assert_eq!(again.if_none_match, None);
}

#[test]
fn triage_follows_merged_items() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let preprints = "https://example.org/preprints.atom";
    let journal = "https://example.org/journal.atom";
    feeds
        .subscribe(
            rss(preprints),
            SubscriptionSettings::new(SourceKind::Preprints),
        )
        .unwrap();
    feeds
        .subscribe(rss(journal), SubscriptionSettings::new(SourceKind::Journal))
        .unwrap();
    let preprint = [(
        "oai:arXiv.org:2610.00001v1",
        "Ice sheet tipping, a preprint",
        "A. Robinson",
        None,
    )];
    let published = [(
        "10.1000/ice",
        "Tipping points of ice sheets",
        "A. Robinson",
        None,
    )];
    let with_doi = [(
        "oai:arXiv.org:2610.00001v1",
        "Ice sheet tipping, a preprint",
        "A. Robinson",
        Some("10.1000/ice"),
    )];
    let http = Recorded::new()
        .reply(preprints, Reply::ok(preprints, atom(&preprint)))
        .reply(journal, Reply::ok(journal, atom(&published)))
        .reply(preprints, Reply::ok(preprints, atom(&with_doi)))
        .reply(journal, Reply::ok(journal, atom(&published)));
    feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(feeds.items().len(), 2, "two titles, no shared key yet");
    let (pre, paper) = (feeds.items()[0].id.clone(), feeds.items()[1].id.clone());
    feeds.set_triage(&pre, Triage::Seen).unwrap();
    feeds.set_triage(&paper, Triage::Starred).unwrap();

    // arXiv now gives the journal's DOI: one item, the older id
    let report = feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(
        report.feeds[0].outcome,
        Outcome::Fetched { entries: 1, new: 0 }
    );
    assert_eq!(feeds.items().len(), 1);
    let item = &feeds.items()[0];
    assert_eq!(item.id, pre);
    assert_eq!(
        item.title, "Tipping points of ice sheets",
        "the published version's"
    );
    assert_eq!(item.arxiv.as_deref(), Some("2610.00001"));
    assert_eq!(feeds.resolve(&paper), pre);
    assert_eq!(feeds.item(&paper).unwrap().id, pre);
    assert_eq!(
        feeds.triage(&pre),
        Triage::Starred,
        "the strongest decision"
    );
    assert_eq!(feeds.triage(&paper), Triage::Starred);
}

#[test]
fn prune_drops_old_items_no_feed_lists() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Clock::new();
    let mut feeds = clock.open(dir.path());
    let blog = "https://example.org/blog.atom";
    feeds
        .subscribe(rss(blog), SubscriptionSettings::new(SourceKind::Blog))
        .unwrap();
    let all = [
        ("post-1", "First post", "Group", None),
        ("post-2", "Second post", "Group", None),
        ("post-3", "Third post", "Group", None),
        ("post-4", "Fourth post", "Group", None),
    ];
    let http = Recorded::new()
        .reply(blog, Reply::ok(blog, atom(&all)))
        .reply(blog, Reply::ok(blog, atom(&all[3..])));
    feeds.fetch(&http, &Access::default()).unwrap();
    let ids: Vec<String> = feeds.items().iter().map(|i| i.id.clone()).collect();
    feeds.set_triage(&ids[0], Triage::Starred).unwrap();
    feeds.set_triage(&ids[1], Triage::Removed).unwrap();
    feeds.set_triage(&ids[2], Triage::Seen).unwrap();
    clock.advance(10);
    // the feed now lists the fourth post only
    feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(feeds.keep_days(), 90);
    feeds
        .set_settings(FeedSettings {
            keep_days: 30,
            ..feeds.settings().clone()
        })
        .unwrap();
    assert_eq!(feeds.prune().unwrap(), 0, "not old enough");
    feeds
        .set_settings(FeedSettings {
            keep_days: 7,
            ..feeds.settings().clone()
        })
        .unwrap();
    assert_eq!(feeds.prune().unwrap(), 2);
    let left: Vec<&str> = feeds.items().iter().map(|i| i.title.as_str()).collect();
    assert_eq!(
        left,
        ["First post", "Fourth post"],
        "starred, and still listed"
    );
    assert_eq!(feeds.mark(&ids[1]), None);
    let reopened = clock.open(dir.path());
    assert_eq!(reopened.items().len(), 2);
}

#[test]
fn an_article_of_two_feeds_is_one_item_and_its_duplicates_collapse() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let earth = "https://www.sciencedaily.com/rss/earth_climate.xml";
    let climate = "https://www.sciencedaily.com/rss/earth_climate/climate.xml";
    let earth_id = feeds
        .subscribe(rss(earth), SubscriptionSettings::new(SourceKind::News))
        .unwrap()
        .id;
    let climate_id = feeds
        .subscribe(rss(climate), SubscriptionSettings::new(SourceKind::News))
        .unwrap()
        .id;
    let page = "https://www.sciencedaily.com/releases/2026/09/260919031037.htm";
    let body = format!(
        "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>ScienceDaily</title>\
         <item><title>Glaciers retreat</title><link>{page}</link>\
         <guid isPermaLink=\"true\">{page}</guid><pubDate>Sat, 19 Sep 2026 08:00:00 EDT</pubDate>\
         </item></channel></rss>"
    );
    let http = Recorded::new()
        .reply(earth, Reply::ok(earth, body.clone()))
        .reply(climate, Reply::ok(climate, body.clone()))
        .reply(earth, Reply::ok(earth, body.clone()))
        .reply(climate, Reply::ok(climate, body));
    feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(feeds.items().len(), 1);
    let sources: Vec<&str> = (feeds.items()[0].sources.iter())
        .map(|s| s.entry.source.as_str())
        .collect();
    assert_eq!(sources, [earth_id.as_str(), climate_id.as_str()]);

    // items.json as it was before pages were compared: an item per feed
    let path = dir.path().join("items.json");
    let mut items: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let mut duplicate = items["items"][0].clone();
    let climates = items["items"][0]["sources"]
        .as_array_mut()
        .unwrap()
        .split_off(1);
    duplicate["sources"] = climates.into();
    let duplicate_id = short_id(&format!("entry:{climate_id}\n{page}"));
    duplicate["id"] = duplicate_id.clone().into();
    items["items"].as_array_mut().unwrap().push(duplicate);
    std::fs::write(&path, serde_json::to_vec(&items).unwrap()).unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    assert_eq!(feeds.items().len(), 2);
    let kept = feeds.items()[0].id.clone();
    feeds.set_triage(&duplicate_id, Triage::Starred).unwrap();

    feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(feeds.items().len(), 1);
    assert_eq!(feeds.items()[0].id, kept, "the older id stays");
    assert_eq!(feeds.items()[0].sources.len(), 2);
    assert_eq!(feeds.resolve(&duplicate_id), kept);
    assert_eq!(feeds.triage(&kept), Triage::Starred);
}

#[test]
fn opening_writes_nothing_that_did_not_change() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    assert_eq!(feeds.prune().unwrap(), 0);
    let written: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert!(written.is_empty(), "{written:?}");
}
