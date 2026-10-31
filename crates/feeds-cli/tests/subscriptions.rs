//! `feeds subscribe`, `unsubscribe` and `subscriptions`: the kinds, the sweeps, the errors.

mod support;

use papers_feeds::http::Recorded;
use support::Sandbox;

const TC: &str = "https://tc.copernicus.org/xml/rss2_0.xml";
const ARXIV: &str = "https://rss.arxiv.org/atom/physics.ao-ph";

#[test]
fn subscribe_guesses_the_kind_and_lists_the_subscriptions() {
    let s = Sandbox::new(Recorded::new());
    let said = s.ok(&["subscribe", TC]);
    let tc = s.json(&["subscriptions", "--json"])[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(said, format!("subscribed {tc} (journal) {TC}\n"));
    assert!(s.ok(&["subscribe", ARXIV]).contains("(preprints)"));
    s.ok(&[
        "subscribe",
        "--issn",
        "1994-0424",
        "--journal",
        "The Cryosphere",
    ]);
    s.ok(&[
        "subscribe",
        "--openalex",
        "primary_topic.id:T10644",
        "--title",
        "Cryosphere",
        "--kind",
        "preprints",
    ]);
    s.ok(&[
        "subscribe",
        "https://example.org/blog.xml",
        "--kind",
        "blog",
        "--fetch-pages",
    ]);

    let json = s.json(&["subscriptions", "--json"]);
    let rows: Vec<(&str, &str, &str)> = json
        .as_array()
        .unwrap()
        .iter()
        .map(|j| {
            let feed = j["url"]
                .as_str()
                .or(j["issn"].as_str())
                .or(j["openalex"].as_str());
            (
                j["kind"].as_str().unwrap(),
                j["title"].as_str().unwrap_or(""),
                feed.unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("journal", "", TC),
            ("preprints", "", ARXIV),
            ("journal", "", "1994-0424"),
            ("preprints", "Cryosphere", "primary_topic.id:T10644"),
            ("blog", "", "https://example.org/blog.xml"),
        ]
    );
    assert_eq!(json[2]["journal"], "The Cryosphere");
    assert_eq!(json[4]["fetch_pages"], true);

    let table = s.ok(&["subscriptions"]);
    let lines: Vec<&str> = table.lines().collect();
    assert_eq!(lines.len(), 5);
    assert!(
        lines[0].starts_with(&format!("{tc}  journal    ")) && lines[0].ends_with(TC),
        "{table}"
    );
    assert!(
        lines[2].ends_with("The Cryosphere  Crossref ISSN 1994-0424"),
        "{table}"
    );
}

#[test]
fn unsubscribe_and_the_errors_of_subscribing() {
    let s = Sandbox::new(Recorded::new());
    s.ok(&["subscribe", TC]);
    // http and https are one feed
    let again = s.run(&["subscribe", "http://tc.copernicus.org/xml/rss2_0.xml"]);
    assert_eq!(again.code, 1);
    assert!(
        again.err.starts_with(
            "feeds: error: http://tc.copernicus.org/xml/rss2_0.xml: already subscribed ("
        ),
        "{}",
        again.err
    );
    assert_eq!(again.out, "");
    let issn = s.run(&["subscribe", "--issn", "1994-0425"]);
    assert_eq!(
        (issn.code, issn.err.as_str()),
        (1, "feeds: error: 1994-0425: not an ISSN\n")
    );
    let url = s.run(&["subscribe", "ftp://example.org/feed"]);
    assert_eq!(url.code, 1);
    // exactly one of a URL, --issn, --openalex: a usage error
    let neither = s.run(&["subscribe"]);
    assert_eq!(neither.code, 2);
    assert!(
        neither
            .err
            .contains("<URL|--issn <ISSN>|--openalex <FILTER>>"),
        "{}",
        neither.err
    );
    assert_eq!(s.run(&["subscribe", TC, "--issn", "1994-0424"]).code, 2);
    assert_eq!(s.run(&["subscribe", TC, "--kind", "podcast"]).code, 2);

    let id = s.json(&["subscriptions", "--json"])[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        s.ok(&["unsubscribe", &id]),
        format!("unsubscribed {id} {TC}\n")
    );
    assert_eq!(s.ok(&["subscriptions"]), "");
    let gone = s.run(&["unsubscribe", &id]);
    assert_eq!(
        (gone.code, gone.err),
        (1, format!("feeds: error: {id}: no such subscription\n"))
    );
}

#[test]
fn the_boost_is_set_when_subscribing_and_edited_after() {
    let s = Sandbox::new(Recorded::new());
    s.ok(&["subscribe", TC]);
    s.ok(&["subscribe", "--openalex", "cites:W1", "--boost", "2"]);
    let json = s.json(&["subscriptions", "--json"]);
    assert!(json[0].get("boost").is_none(), "none when 0");
    assert_eq!(json[1]["boost"], 2.0);
    let table = s.ok(&["subscriptions"]);
    let lines: Vec<&str> = table.lines().collect();
    assert!(
        lines[0].ends_with(TC) && !lines[0].contains("boost"),
        "{table}"
    );
    assert!(lines[1].ends_with("boost +2  OpenAlex cites:W1"), "{table}");

    let id = json[0]["id"].as_str().unwrap();
    assert_eq!(
        s.ok(&["edit", id, "--boost", "-1.5"]),
        format!("edited {id} {TC}: boost -1.5\n")
    );
    assert_eq!(s.json(&["subscriptions", "--json"])[0]["boost"], -1.5);
    s.ok(&["edit", id, "--boost", "0"]);
    assert!(
        s.json(&["subscriptions", "--json"])[0]
            .get("boost")
            .is_none()
    );

    assert_eq!(s.run(&["edit", id]).code, 2, "--boost is required");
    let gone = s.run(&["edit", "nope", "--boost", "1"]);
    assert_eq!(
        (gone.code, gone.err.as_str()),
        (1, "feeds: error: nope: no such subscription\n")
    );
}

#[test]
fn opml_out_and_back_in() {
    let s = Sandbox::new(Recorded::new());
    s.ok(&["subscribe", TC, "--title", "The Cryosphere"]);
    s.ok(&["subscribe", "--issn", "1994-0424"]);
    s.ok(&["subscribe", ARXIV, "--kind", "preprints"]);

    let opml = s.ok(&["opml", "export"]);
    assert!(opml.starts_with("<?xml"), "{opml}");
    assert!(opml.contains(&format!("xmlUrl=\"{TC}\"")), "{opml}");
    assert!(opml.contains("issn=\"1994-0424\""), "{opml}");
    let file = s.path("feeds.opml");
    let said = s.ok(&["opml", "export", file.to_str().unwrap()]);
    assert_eq!(said, format!("exported 3 to {}\n", file.display()));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), opml);

    // into another folder: the same subscriptions, titled and kinded as they were
    let other = Sandbox::new(Recorded::new());
    let said = other.ok(&["opml", "import", file.to_str().unwrap()]);
    assert_eq!(said, "subscribed to 3 new\n");
    assert_eq!(other.ok(&["opml", "export"]), opml);
    assert_eq!(
        other.ok(&["opml", "import", file.to_str().unwrap()]),
        "subscribed to 0 new\n",
        "those followed already are passed over"
    );
}

#[test]
fn opml_errors_name_the_file_or_the_problem() {
    let s = Sandbox::new(Recorded::new());
    let missing = s.run(&["opml", "import", "nope.opml"]);
    assert_eq!(missing.code, 1);
    assert!(
        missing.err.starts_with("feeds: error: nope.opml: "),
        "{}",
        missing.err
    );
    let not = s.path("not.opml");
    std::fs::write(&not, "<html></html>").unwrap();
    let bad = s.run(&["opml", "import", not.to_str().unwrap()]);
    assert_eq!(
        (bad.code, bad.err.as_str()),
        (1, "feeds: error: not an OPML list: not an OPML document\n")
    );
    let into = s.path("no-such-folder/feeds.opml");
    let unwritable = s.run(&["opml", "export", into.to_str().unwrap()]);
    assert_eq!(unwritable.code, 1);
    assert!(
        unwritable.err.starts_with("feeds: error: "),
        "{}",
        unwritable.err
    );
    assert_eq!(s.run(&["opml"]).code, 2);
}
