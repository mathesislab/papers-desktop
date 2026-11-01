//! `feeds views`, and `--saved` of `list` and `digest`, on the recorded feeds of the feeds
//! crate (The Cryosphere's, Futurity's).

mod support;

use papers_feeds::Reply;
use papers_feeds::http::Recorded;
use support::{Sandbox, fixture, ok};

const TC: &str = "https://tc.copernicus.org/xml/rss2_0.xml";
const FUTURITY: &str = "https://www.futurity.org/feed/";
const WORKS: &str = "https://api.crossref.org/works";

/// The Cryosphere and Futurity subscribed and fetched.
fn fetched() -> Sandbox {
    let work = format!("{WORKS}/10.1126/sciadv.aei1213");
    let http = Recorded::new()
        .reply(TC, ok(TC, fixture("copernicus-tc.xml")))
        .reply(FUTURITY, ok(FUTURITY, fixture("futurity.xml")))
        .reply(&work, Reply::status(&work, 404));
    let s = Sandbox::new(http);
    s.config(r#"{"interests":[{"name":"Sea ice","include":[{"term":"sea ice","weight":3}],"min_score":3}]}"#);
    s.ok(&["subscribe", TC]);
    s.ok(&["subscribe", FUTURITY, "--kind", "news"]);
    s.ok(&["fetch"]);
    s
}

/// The ids of the lines of a list.
fn ids(list: &str) -> Vec<String> {
    let id = |l: &str| l.split_once(' ').unwrap().0.to_string();
    list.lines().map(id).collect()
}

#[test]
fn views_are_added_listed_replaced_and_removed() {
    let s = fetched();
    assert_eq!(s.ok(&["views"]), "");
    assert_eq!(s.ok(&["searches"]), "", "the name of before");
    assert_eq!(s.ok(&["views", "add", "News", "kind:news"]), "saved News\n");
    assert_eq!(
        s.ok(&[
            "views",
            "add",
            "Ice",
            "is:new",
            r#"prof:"Sea ice""#,
            "--sort",
            "score"
        ]),
        "saved Ice\n"
    );
    assert_eq!(
        s.ok(&["views"]),
        "News  newest  kind:news\nIce   score   is:new prof:\"Sea ice\"\n"
    );
    assert_eq!(
        s.json(&["views", "--json"])[1],
        serde_json::json!({"name": "Ice", "query": "is:new prof:\"Sea ice\"", "sort": "score"})
    );
    // the same name in another case: in its place
    assert_eq!(
        s.ok(&["views", "add", "news", "kind:news,blog"]),
        "replaced news\n"
    );
    assert_eq!(s.json(&["views", "--json"])[0]["query"], "kind:news,blog");
    assert_eq!(s.ok(&["views", "remove", "ICE"]), "removed Ice\n");
    assert_eq!(s.json(&["views", "--json"]).as_array().unwrap().len(), 1);

    let unknown = s.run(&["views", "remove", "Ice"]);
    assert_eq!(
        (unknown.code, unknown.err.as_str()),
        (1, "feeds: error: Ice: no such saved view\n")
    );
    let blank = s.run(&["views", "add", " ", "x"]);
    assert_eq!(blank.code, 1, "{}", blank.err);
    assert_eq!(s.run(&["views", "add", "x"]).code, 2, "a query is needed");
}

#[test]
fn list_and_digest_take_a_saved_view_and_a_query_too() {
    let s = fetched();
    s.ok(&["views", "add", "Papers", "kind:paper", "--sort", "score"]);
    let papers = ids(&s.ok(&["list", "--saved", "papers"]));
    assert_eq!(papers.len(), 3);
    // in the order of the saved view, unless asked otherwise
    assert_eq!(
        papers,
        ids(&s.ok(&["list", "kind:paper", "--sort", "score"]))
    );
    assert_eq!(
        ids(&s.ok(&["list", "--saved", "Papers", "--sort", "newest"])),
        ids(&s.ok(&["list", "kind:paper"]))
    );
    // with a query: both hold
    let both = ids(&s.ok(&["list", "--saved", "Papers", "sea", "ice"]));
    assert_eq!(
        both,
        ids(&s.ok(&["list", "kind:paper", "sea", "ice", "--sort", "score"]))
    );
    assert!(both.len() < papers.len() && !both.is_empty(), "{both:?}");
    // dismissed items left out, unless asked
    s.ok(&["mark", "dismissed", &papers[0]]);
    assert_eq!(ids(&s.ok(&["list", "--saved", "Papers"])).len(), 2);
    assert_eq!(
        ids(&s.ok(&["list", "--saved", "Papers", "--dismissed"])).len(),
        3
    );
    let unknown = s.run(&["list", "--saved", "None"]);
    assert_eq!(unknown.err, "feeds: error: None: no such saved view\n");

    // the digest: the saved view alone, without the default score:3-, or with --query
    let rss = s.path("feed.xml");
    let rss = rss.to_str().unwrap();
    let digest = |args: &[&str]| {
        let mut all = vec!["digest", "--rss", rss, "--days", "100000"];
        all.extend(args);
        s.ok(&all)
    };
    let count = |args: &[&str]| format!("{} items in ", ids(&s.ok(args)).len());
    assert!(digest(&["--saved", "Papers"]).starts_with("2 items in "));
    let narrowed = count(&["list", "--saved", "Papers", "sea", "ice"]);
    assert!(digest(&["--saved", "Papers", "--query", "sea ice"]).starts_with(&narrowed));
    assert!(digest(&[]).starts_with(&count(&["list", "score:3-"])));
}
