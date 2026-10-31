//! The query language over items (facets, numbers, fields), the view
//! (filtering, sorting, marks, counts), rules on new items, and retention.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use papers_core::{Matching, Span};
use papers_feeds::http::Recorded;
use papers_feeds::{
    Access, Action, Feed, Feeds, Interest, ItemKind, Matcher, Reply, Rule, Sort, SourceKind,
    Triage, View, ViewRequest, Weighted,
};

const TC: &str = "https://example.org/tc.atom";
const EGU: &str = "https://example.org/egusphere.atom";
const NEWS: &str = "https://example.org/futurity.atom";

/// An entry: id, title, author, date (day of October 2026), summary, DOI.
type E<'a> = (&'a str, &'a str, &'a str, u32, &'a str, Option<&'a str>);

fn atom(title: &str, entries: &[E]) -> Vec<u8> {
    let entries: String = entries
        .iter()
        .map(|(id, title, author, day, summary, doi)| {
            let doi = doi
                .map(|d| format!("<arxiv:doi>{d}</arxiv:doi>"))
                .unwrap_or_default();
            format!(
                "<entry><id>{id}</id><title>{title}</title><link href=\"https://example.org/{id}\"/>\
                 <published>2026-10-{day:02}T00:00:00Z</published><dc:creator>{author}</dc:creator>\
                 <summary>{summary}</summary>{doi}</entry>"
            )
        })
        .collect();
    format!(
        "<?xml version='1.0'?><feed xmlns=\"http://www.w3.org/2005/Atom\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:arxiv=\"http://arxiv.org/schemas/atom\">\
         <title>{title}</title><id>feed</id>{entries}</feed>"
    )
    .into_bytes()
}

const J1: E = (
    "j1",
    "Hysteresis of the Greenland ice sheet under overshoot",
    "Anne Moreau",
    30,
    "We model the ice-sheet response and GIA.",
    Some("10.5194/tc-20-1-2026"),
);
const J2: E = (
    "j2",
    "Editorial: the year in glaciology",
    "Editors",
    28,
    "",
    None,
);
const J3: E = (
    "j3",
    "Lake ice in Georgia",
    "Smith, Jo",
    27,
    "Not about the ice sheet.",
    None,
);
const P1: E = (
    "p1",
    "Firn densification under extreme melt",
    "Pedersen, K.",
    29,
    "The firn layer of the ice sheet.",
    None,
);
const N1: E = (
    "n1",
    "Greenland melt season starts earlier",
    "Futurity",
    31,
    "A study of the ice sheet.",
    None,
);
const P2: E = (
    "p2",
    "GIA under a thinning ice sheet",
    "Ortiz, M.",
    31,
    "",
    None,
);

fn term(t: &str, weight: f64) -> Weighted {
    Weighted {
        matcher: Matcher::Term(t.to_string()),
        weight,
        label: None,
    }
}

fn interests() -> Vec<Interest> {
    let ice = Interest {
        name: "Ice sheets".to_string(),
        include: vec![
            term("ice sheet", 3.0),
            Weighted {
                matcher: Matcher::Pattern(r"\bGIA\b".to_string()),
                weight: 4.0,
                label: Some("GIA".to_string()),
            },
        ],
        exclude: vec![Matcher::Term("Georgia".to_string())],
        min_score: 3.0,
    };
    let firn = Interest {
        name: "Firn".to_string(),
        include: vec![term("firn", 2.0)],
        exclude: Vec::new(),
        min_score: 3.0,
    };
    vec![ice, firn]
}

/// A clock that the test moves.
#[derive(Clone)]
struct Clock(Arc<Mutex<DateTime<Utc>>>);

impl Clock {
    fn new() -> Clock {
        let start = Utc.with_ymd_and_hms(2026, 10, 31, 8, 0, 0).unwrap();
        Clock(Arc::new(Mutex::new(start)))
    }

    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }

    fn advance(&self, days: i64) {
        *self.0.lock().unwrap() += TimeDelta::days(days);
    }

    fn open(&self, dir: &std::path::Path) -> Feeds {
        let clock = self.0.clone();
        Feeds::open_with(dir, Box::new(move || *clock.lock().unwrap())).unwrap()
    }
}

/// The feeds of a folder subscribed to a journal, a preprint server and a
/// news site, with the interests, `rules` set before the first fetch.
fn feeds(dir: &std::path::Path, clock: &Clock, rules: Vec<Rule>) -> Feeds {
    let mut feeds = clock.open(dir);
    feeds
        .subscribe(Feed::Rss { url: TC.into() }, SourceKind::Journal)
        .unwrap();
    feeds
        .subscribe(Feed::Rss { url: EGU.into() }, SourceKind::Preprints)
        .unwrap();
    feeds
        .subscribe(Feed::Rss { url: NEWS.into() }, SourceKind::News)
        .unwrap();
    feeds.set_interests(interests(), 2.0).unwrap();
    feeds.set_rules(rules).unwrap();
    let http = Recorded::new()
        .reply(TC, Reply::ok(TC, atom("The Cryosphere", &[J1, J2, J3])))
        .reply(EGU, Reply::ok(EGU, atom("EGUsphere", &[P1])))
        .reply(NEWS, Reply::ok(NEWS, atom("Futurity", &[N1])));
    feeds.fetch(&http, &Access::default()).unwrap();
    feeds
}

/// The titles of the rows a query shows, newest first.
fn titles(feeds: &Feeds, query: &str) -> Vec<String> {
    let request = ViewRequest {
        query,
        ..ViewRequest::default()
    };
    titles_of(&feeds.view(&request))
}

fn titles_of(view: &View) -> Vec<String> {
    let text = |spans: &[Span]| spans.iter().map(|s| s.text.as_str()).collect::<String>();
    view.rows.iter().map(|r| text(&r.title)).collect()
}

fn id_of(feeds: &Feeds, title: &str) -> String {
    let item = feeds.items().iter().find(|i| i.title == title).unwrap();
    item.id.clone()
}

#[test]
fn facets_numbers_and_fields() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = feeds(dir.path(), &Clock::new(), Vec::new());
    let (j1, j2, j3, p1, n1) = (J1.1, J2.1, J3.1, P1.1, N1.1);
    assert_eq!(titles(&feeds, ""), [n1, j1, p1, j2, j3], "newest first");
    assert_eq!(titles(&feeds, "kind:preprint"), [p1]);
    assert_eq!(titles(&feeds, "kind:news"), [n1]);
    assert_eq!(titles(&feeds, "-kind:paper"), [n1, p1]);
    assert_eq!(titles(&feeds, "is:unread").len(), 5);
    assert_eq!(titles(&feeds, "is:editorial"), [j2]);
    assert_eq!(titles(&feeds, r#"src:"the cryosphere""#), [j1, j2, j3]);
    let egu = &feeds.subscriptions()[1].id;
    assert_eq!(titles(&feeds, &format!("src:{egu}")), [p1]);
    assert_eq!(titles(&feeds, "src:*sphere"), [j1, p1, j2, j3]);
    assert_eq!(titles(&feeds, r#"prof:"ice sheets""#), [n1, j1, p1]);
    assert_eq!(titles(&feeds, "prof:firn"), [p1]);
    assert_eq!(titles(&feeds, "score:5-"), [j1, p1]);
    assert_eq!(titles(&feeds, "score:-0"), [j2, j3], "vetoed: 0");
    assert_eq!(titles(&feeds, "has:doi"), [j1]);
    assert_eq!(titles(&feeds, "-has:abstract"), [j2]);
    assert_eq!(titles(&feeds, "doi:tc-20-1"), [j1]);
    assert_eq!(titles(&feeds, "a:moreau j:cryosphere y:2026"), [j1]);
    assert_eq!(titles(&feeds, "firn"), [p1], "the title");
    assert_eq!(titles(&feeds, "abs:firn"), [p1]);
    assert!(
        titles(&feeds, "layer").is_empty(),
        "plain words: not the abstract"
    );
    assert_eq!(titles(&feeds, "abs:layer"), [p1]);

    feeds
        .set_triage(&id_of(&feeds, n1), Triage::Shortlisted)
        .unwrap();
    feeds
        .set_triage(&id_of(&feeds, j3), Triage::Dismissed)
        .unwrap();
    assert_eq!(titles(&feeds, "is:shortlisted"), [n1]);
    assert_eq!(titles(&feeds, "is:new").len(), 3);
    assert_eq!(titles(&feeds, "-is:dismissed").len(), 4);

    // the caller's facets
    let j1_id = id_of(&feeds, j1);
    let in_library = |item: &papers_feeds::Item| match item.id == j1_id {
        true => vec!["inlibrary".to_string()],
        false => Vec::new(),
    };
    let request = ViewRequest {
        query: "is:inlibrary",
        is: Some(&in_library),
        ..ViewRequest::default()
    };
    assert_eq!(titles_of(&feeds.view(&request)), [j1]);
}

#[test]
fn rows_are_sorted_scored_and_marked() {
    let dir = tempfile::tempdir().unwrap();
    let feeds = feeds(dir.path(), &Clock::new(), Vec::new());
    let request = ViewRequest {
        query: "hysteresis",
        ..ViewRequest::default()
    };
    let view = feeds.view(&request);
    let row = &view.rows[0];
    assert_eq!(row.score, 3.0 * 2.0 + 3.0 + 4.0);
    assert_eq!(row.interests.len(), 1);
    assert_eq!(row.interests[0].terms, ["ice sheet", "GIA"]);
    let marked = |spans: &[Span]| -> Vec<String> {
        let marked = spans.iter().filter(|s| s.mark);
        marked.map(|s| s.text.clone()).collect()
    };
    assert_eq!(marked(&row.title), ["Hysteresis", "ice sheet"]);
    assert_eq!(marked(&row.r#abstract), ["ice-sheet", "GIA"]);
    assert_eq!(row.authors.len(), 1);
    assert_eq!(
        row.authors[0][0].text, "Moreau",
        "the family name as written"
    );

    let by = |sort, kinds: &[ItemKind]| {
        let request = ViewRequest {
            sort,
            kinds,
            ..ViewRequest::default()
        };
        titles_of(&feeds.view(&request))
    };
    let (j1, j2, j3, p1, n1) = (J1.1, J2.1, J3.1, P1.1, N1.1);
    assert_eq!(
        by(Sort::Score, &[]),
        [j1, p1, n1, j2, j3],
        "the best score, then the newest"
    );
    assert_eq!(
        by(Sort::Newest, &[ItemKind::Paper, ItemKind::Preprint]),
        [j1, p1, j2, j3]
    );
    let view = feeds.view(&ViewRequest {
        query: "firn",
        ..ViewRequest::default()
    });
    let names: Vec<(&str, f64)> = view.rows[0]
        .interests
        .iter()
        .map(|s| (s.interest.as_str(), s.score))
        .collect();
    assert_eq!(
        names,
        [("Firn", 6.0), ("Ice sheets", 3.0)],
        "the best first"
    );
}

#[test]
fn an_item_alone_is_marked_whether_the_query_matches_or_not() {
    let dir = tempfile::tempdir().unwrap();
    let feeds = feeds(dir.path(), &Clock::new(), Vec::new());
    let id = id_of(&feeds, J1.1);
    let marked = |query: &str| -> Vec<String> {
        let row = feeds.row_of(&id, query, Matching::WordStart).unwrap();
        let marked = row.title.iter().filter(|s| s.mark);
        marked.map(|s| s.text.clone()).collect()
    };
    assert_eq!(marked("hysteresis"), ["Hysteresis", "ice sheet"]);
    assert_eq!(marked("firn"), ["ice sheet"], "the interests' terms only");
    assert!(feeds.row_of("none", "", Matching::WordStart).is_none());
}

#[test]
fn counts_are_of_live_items_whatever_the_query() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = feeds(dir.path(), &Clock::new(), Vec::new());
    feeds
        .set_triage(&id_of(&feeds, N1.1), Triage::Dismissed)
        .unwrap();
    feeds
        .set_triage(&id_of(&feeds, P1.1), Triage::Shortlisted)
        .unwrap();
    let j1 = id_of(&feeds, J1.1);
    let in_library = |item: &papers_feeds::Item| match item.id == j1 {
        true => vec!["inlibrary".to_string()],
        false => Vec::new(),
    };
    let request = ViewRequest {
        query: "kind:preprint",
        kinds: &[ItemKind::Preprint],
        is: Some(&in_library),
        ..ViewRequest::default()
    };
    let view = feeds.view(&request);
    assert_eq!(view.rows.len(), 1);
    let counts = view.counts;
    assert_eq!(counts.all, 4, "not the dismissed");
    let pairs = |map: &std::collections::BTreeMap<String, usize>| -> Vec<(String, usize)> {
        map.iter().map(|(k, &v)| (k.clone(), v)).collect()
    };
    assert_eq!(
        pairs(&counts.is),
        [
            ("editorial".to_string(), 1),
            ("inlibrary".to_string(), 1),
            ("new".to_string(), 3),
            ("shortlisted".to_string(), 1),
        ]
    );
    assert_eq!(
        counts.kinds.into_iter().collect::<Vec<_>>(),
        [(ItemKind::Paper, 3), (ItemKind::Preprint, 1)]
    );
    assert_eq!(
        pairs(&counts.interests),
        [("Firn".to_string(), 1), ("Ice sheets".to_string(), 2)]
    );
    let subscriptions = feeds.subscriptions();
    assert_eq!(counts.sources[&subscriptions[0].id], 3);
    assert_eq!(counts.sources[&subscriptions[1].id], 1);
    assert!(!counts.sources.contains_key(&subscriptions[2].id));
    let json = serde_json::to_value(feeds.view(&ViewRequest::default()).counts).unwrap();
    assert_eq!(json["kinds"]["paper"], 3);
}

#[test]
fn rules_triage_the_new_items_only() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Clock::new();
    let rule = |when: &str, then| Rule {
        when: when.to_string(),
        then,
    };
    let rules = vec![
        rule("prof:firn score:5-", Action::Shortlist),
        rule("kind:preprint", Action::Dismiss),
        rule("is:editorial", Action::Dismiss),
        rule("kind:news -src:futurity", Action::Dismiss),
        rule("kind:news", Action::Seen),
    ];
    let mut feeds = feeds(dir.path(), &clock, rules.clone());
    let triage = |feeds: &Feeds, e: E| feeds.triage(&id_of(feeds, e.1));
    assert_eq!(triage(&feeds, P1), Triage::Shortlisted, "the first rule");
    assert_eq!(triage(&feeds, J2), Triage::Dismissed);
    assert_eq!(triage(&feeds, N1), Triage::Seen);
    assert_eq!(triage(&feeds, J1), Triage::New);
    assert_eq!(feeds.mark(&id_of(&feeds, P1.1)).unwrap().at, clock.now());

    // a rule on items the user has seen, or left new: none changes
    feeds
        .set_triage(&id_of(&feeds, J1.1), Triage::Seen)
        .unwrap();
    let mut rules = rules;
    rules.push(rule("prof:\"ice sheets\"", Action::Shortlist));
    rules.push(rule("", Action::Dismiss));
    feeds.set_rules(rules.clone()).unwrap();
    let http = Recorded::new()
        .reply(TC, Reply::ok(TC, atom("The Cryosphere", &[J1, J2, J3])))
        .reply(EGU, Reply::ok(EGU, atom("EGUsphere", &[P1, P2])))
        .reply(NEWS, Reply::ok(NEWS, atom("Futurity", &[N1])));
    clock.advance(1);
    feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(triage(&feeds, J1), Triage::Seen);
    assert_eq!(triage(&feeds, J3), Triage::New);
    assert_eq!(
        triage(&feeds, P2),
        Triage::Dismissed,
        "kind:preprint before prof:\"ice sheets\""
    );
    let reopened = clock.open(dir.path());
    assert_eq!(reopened.rules(), rules);
    assert_eq!(triage(&reopened, P2), Triage::Dismissed, "written");
}

#[test]
fn the_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let config = r#"{
      "subscriptions": [],
      "interests": [
        {"name": "AMOC", "min_score": 4,
         "include": [{"term": "AMOC", "weight": 4},
                     {"pattern": "overturning\\s+circulation", "weight": 3, "label": "overturning"}],
         "exclude": [{"term": "market"}]}
      ],
      "title_boost": 2.5,
      "rules": [{"when": "prof:amoc score:6-", "then": "shortlist"},
                {"when": "t:editorial", "then": "dismiss"}],
      "keep_days": 30
    }"#;
    std::fs::write(dir.path().join("config.json"), config).unwrap();
    let feeds = Feeds::open(dir.path()).unwrap();
    assert_eq!(feeds.interests()[0].min_score, 4.0);
    assert_eq!(feeds.title_boost(), 2.5);
    assert_eq!(feeds.rules()[1].then, Action::Dismiss);
    assert_eq!(feeds.keep_days(), 30);
    let empty = tempfile::tempdir().unwrap();
    let defaults = Feeds::open(empty.path()).unwrap();
    assert_eq!((defaults.title_boost(), defaults.keep_days()), (2.0, 90));

    let broken = config.replace("overturning\\\\s+", "(overturning");
    std::fs::write(dir.path().join("config.json"), broken).unwrap();
    assert!(matches!(
        Feeds::open(dir.path()),
        Err(papers_feeds::Error::Interest { .. })
    ));
}

#[test]
fn old_items_go_on_opening_and_fetching() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Clock::new();
    let mut feeds = feeds(dir.path(), &clock, Vec::new());
    feeds
        .set_triage(&id_of(&feeds, J2.1), Triage::Shortlisted)
        .unwrap();
    // the journal now lists J1 only
    let http = Recorded::new()
        .reply(TC, Reply::ok(TC, atom("The Cryosphere", &[J1])))
        .reply(EGU, Reply::status(EGU, 304))
        .reply(NEWS, Reply::status(NEWS, 304))
        .reply(TC, Reply::status(TC, 304))
        .reply(EGU, Reply::ok(EGU, atom("EGUsphere", &[])))
        .reply(NEWS, Reply::status(NEWS, 304));
    clock.advance(11);
    feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(feeds.items().len(), 5, "kept 90 days");
    feeds.set_keep_days(10).unwrap();
    let mut feeds = clock.open(dir.path());
    let left =
        |feeds: &Feeds| -> Vec<String> { feeds.items().iter().map(|i| i.title.clone()).collect() };
    assert_eq!(left(&feeds), [J1.1, J2.1, P1.1, N1.1], "J2 shortlisted");
    let items = std::fs::read_to_string(dir.path().join("items.json")).unwrap();
    assert!(!items.contains(J3.1), "written");
    feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(left(&feeds), [J1.1, J2.1, N1.1], "no longer listed");
}
