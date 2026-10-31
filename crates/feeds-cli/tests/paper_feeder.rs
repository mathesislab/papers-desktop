//! `feeds import-paper-feeder`: a setup modelled on paper-feeder's examples becomes
//! subscriptions, the lookback and one interest; what has no equivalent is reported.

mod support;

use papers_feeds::http::Recorded;
use support::{Sandbox, own};

/// The `(kind, journal, feed)` of each subscription.
fn subscriptions(s: &Sandbox) -> Vec<(String, String, String)> {
    let json = s.json(&["subscriptions", "--json"]);
    let text = |j: &serde_json::Value, key: &str| j[key].as_str().map(String::from);
    let feed = |j: &serde_json::Value| ["url", "issn", "openalex"].iter().find_map(|k| text(j, k));
    let rows = json.as_array().unwrap().iter();
    rows.map(|j| {
        (
            text(j, "kind").unwrap(),
            text(j, "journal").unwrap_or_default(),
            feed(j).unwrap(),
        )
    })
    .collect()
}

fn import(s: &Sandbox, extra: &[&str]) -> support::Ran {
    let (sources, scoring) = (own("pf-sources.yaml"), own("pf-scoring.yaml"));
    let mut args = vec![
        "import-paper-feeder",
        sources.to_str().unwrap(),
        scoring.to_str().unwrap(),
    ];
    args.extend(extra);
    s.run(&args)
}

#[test]
fn sources_become_subscriptions_and_the_lexicon_one_interest() {
    let s = Sandbox::new(Recorded::new());
    let ran = import(&s, &[]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        ran.out,
        "subscriptions: 9 added, 1 there already, 1 failed\n\
         lookback: 21 days\n\
         interest Interests: 4 terms, 3 vetoes, from score 4, title boost 3\n\
         mailto me@example.org: not stored; set FEEDS_MAILTO or pass --mailto\n"
    );
    // the look-around of Python's `re` is no regex here (the message is the regex crate's)
    let (skipped, rest) = ran
        .err
        .split_once("feeds: warning: always_include")
        .unwrap();
    assert!(skipped.starts_with(
        "feeds: warning: skipped ftp://example.org/feed: ftp://example.org/feed: not a feed address: \
         not an http(s) URL\n\
         feeds: warning: skipped Pattern(\"(?=ice)shelf\"): interest Interests: "
    ), "{skipped}");
    assert!(
        skipped.ends_with("feeds: warning: skipped an include with neither term nor pattern\n"),
        "{skipped}"
    );
    assert_eq!(
        rest,
        " has no equivalent yet, skipped for rss:tc, openalex:cites-me\n\
         feeds: warning: score_boost has no equivalent yet, skipped for openalex:cites-me (2)\n\
         feeds: warning: lookback_days of a query has no equivalent yet, skipped for openalex:cites-me (180 days)\n\
         feeds: warning: not used: window_days, retention_days, render, serendipity_count, llm_rerank\n"
    );
    let rows = subscriptions(&s);
    let row = |kind: &str, journal: &str, feed: &str| {
        (kind.to_string(), journal.to_string(), feed.to_string())
    };
    assert_eq!(
        rows,
        [
            row(
                "journal",
                "Nature Geoscience",
                "https://www.nature.com/ngeo.rss"
            ),
            row(
                "journal",
                "The Cryosphere",
                "https://tc.copernicus.org/xml/rss2_0.xml"
            ),
            row(
                "preprints",
                "arXiv physics.ao-ph",
                "http://arxiv.org/rss/physics.ao-ph"
            ),
            row(
                "journal",
                "",
                "https://agupubs.onlinelibrary.wiley.com/feed/19448007/most-recent"
            ),
            row("journal", "", "primary_topic.id:T10644"),
            row("journal", "", "cites:W123|W456"),
            row("journal", "", "0022-1430"),
            row("journal", "", "0894-8755"),
            row("journal", "", "1994-0424"),
        ]
    );
    // the query is named by its name
    assert_eq!(
        s.json(&["subscriptions", "--json"])[4]["title"],
        "cryosphere"
    );
    assert_eq!(
        s.ok(&["interests"]),
        "Interests (from score 4; a term in the title counts 3 times)\n\
         \x20     3  ice sheet\n\
         \x20     5  /ice[- ]sheet model(l)?ing/ as ice-sheet modelling\n\
         \x20     4  /\\bGIA\\b/ as GIA\n\
         \x20     1  sea ice\n\
         \x20  veto  Georgia\n\
         \x20  veto  clinical trial\n\
         \x20  veto  /stock\\s+market/\n"
    );
    assert!(s.ok(&["config"]).contains("lookback days 21\n"));
    // the scores are paper-feeder's: a term in the title counts the boost
    let json = s.json(&["interests", "--json"]);
    assert_eq!(json["title_boost"], 3.0);
    assert_eq!(json["interests"][0]["min_score"], 4.0);
    assert_eq!(
        json["interests"][0]["include"][3]["weight"], 1.0,
        "a term's weight is 1 if none"
    );
}

#[test]
fn an_import_again_adds_nothing_and_replaces_the_interest() {
    let s = Sandbox::new(Recorded::new());
    import(&s, &[]);
    let again = import(&s, &[]);
    assert!(
        again
            .out
            .starts_with("subscriptions: 0 added, 10 there already, 1 failed\n"),
        "{}",
        again.out
    );
    assert!(
        again
            .out
            .contains("title boost 3, replacing the one of this name\n"),
        "{}",
        again.out
    );
    assert_eq!(
        s.json(&["interests", "--json"])["interests"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // another name: a second interest, beside the first
    import(&s, &["--interest", "Ice"]);
    let names: Vec<String> = s.json(&["interests", "--json"])["interests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["Interests", "Ice"]);
}

#[test]
fn the_issns_of_openalex_can_stay_openalex_filters() {
    let s = Sandbox::new(Recorded::new());
    let ran = import(&s, &["--issn-via", "openalex"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(
        ran.out
            .starts_with("subscriptions: 10 added, 0 there already, 1 failed\n"),
        "{}",
        ran.out
    );
    let feeds: Vec<String> = subscriptions(&s).into_iter().map(|r| r.2).collect();
    assert_eq!(
        feeds[6..],
        [
            "primary_location.source.issn:0022-1430",
            "primary_location.source.issn:0894-8755",
            "0022-1430",
            "1994-0424"
        ]
    );
}

#[test]
fn a_file_that_is_missing_or_not_yaml_is_an_error() {
    let s = Sandbox::new(Recorded::new());
    let scoring = own("pf-scoring.yaml");
    let missing = s.run(&[
        "import-paper-feeder",
        "nope.yaml",
        scoring.to_str().unwrap(),
    ]);
    assert_eq!(missing.code, 1);
    assert!(
        missing.err.starts_with("feeds: error: nope.yaml: "),
        "{}",
        missing.err
    );
    std::fs::write(s.path("bad.yaml"), "rss: [unclosed\n").unwrap();
    let bad = s.run(&[
        "import-paper-feeder",
        s.path("bad.yaml").to_str().unwrap(),
        scoring.to_str().unwrap(),
    ]);
    assert_eq!(bad.code, 1);
    assert!(bad.err.contains("bad.yaml: "), "{}", bad.err);
    // nothing was imported
    assert_eq!(s.ok(&["subscriptions"]), "");
}
