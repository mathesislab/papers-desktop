//! `feeds fetch`, `list`, `show`, `find`, `mark` and `prune` on the recorded feeds of the
//! feeds crate (The Cryosphere's, Futurity's).

mod support;

use papers_feeds::Reply;
use papers_feeds::http::Recorded;
use support::{Sandbox, fixture, ok};

const TC: &str = "https://tc.copernicus.org/xml/rss2_0.xml";
const FUTURITY: &str = "https://www.futurity.org/feed/";
const WORKS: &str = "https://api.crossref.org/works";

/// One interest, `Sea ice`: sea ice 3, snow 1, a veto on `retrievability`.
const SEA_ICE: &str = r#"{"interests":[{"name":"Sea ice","include":[{"term":"sea ice","weight":3},
    {"term":"snow","weight":1}],"exclude":[{"term":"retrievability"}],"min_score":3}],
    "rules":[{"when":"kind:news","then":"seen"}]}"#;

/// The Cryosphere and Futurity subscribed and fetched, the interest set.
fn fetched() -> Sandbox {
    let work = format!("{WORKS}/10.1126/sciadv.aei1213");
    let http = Recorded::new()
        .reply(TC, ok(TC, fixture("copernicus-tc.xml")))
        .reply(FUTURITY, ok(FUTURITY, fixture("futurity.xml")))
        .reply(&work, Reply::status(&work, 404));
    let s = Sandbox::new(http);
    s.config(SEA_ICE);
    s.ok(&["subscribe", TC]);
    s.ok(&["subscribe", FUTURITY, "--kind", "news"]);
    s
}

/// The ids of the lines of a list, and the rest of the lines.
fn lines(list: &str) -> Vec<(String, String)> {
    let split = |l: &str| {
        let (id, rest) = l.split_once(' ').unwrap();
        (id.to_string(), rest.trim_start().to_string())
    };
    list.lines().map(split).collect()
}

#[test]
fn fetch_reports_each_subscription() {
    let s = fetched();
    let out = s.ok(&["fetch"]);
    assert_eq!(
        out,
        "3 new  TC - recent papers (3 entries)\n\
         2 new  Futurity (2 entries)\n\
         new: 5; lookups: 2; pages: 0\n"
    );
    // the feed gets no new reply: the second fetch fails, and says so
    let again = s.ok(&["fetch"]);
    assert!(again.starts_with("failed  TC - recent papers: https://tc.copernicus.org/xml/rss2_0.xml: no recorded reply\n"), "{again}");
    // the rule of the folder made the news seen
    assert_eq!(s.ok(&["list", "kind:news"]).matches("  .  ").count(), 2);
}

#[test]
fn a_sweep_that_needs_a_key_fails_without_stopping_the_others() {
    let s = fetched();
    s.ok(&["subscribe", "--openalex", "primary_topic.id:T10644"]);
    let out = s.ok(&["fetch"]);
    let report: Vec<&str> = out.lines().collect();
    assert!(
        report[0].starts_with("3 new") && report[0].ends_with("TC - recent papers (3 entries)"),
        "{out}"
    );
    assert!(
        report[2].starts_with("failed  OpenAlex primary_topic.id:T10644: "),
        "{out}"
    );
    assert!(report[2].to_lowercase().contains("key"), "{out}");
}

#[test]
fn list_sorts_filters_and_limits() {
    let s = fetched();
    s.ok(&["fetch"]);
    let titles = |args: &[&str]| -> Vec<String> {
        let list = s.ok(args);
        lines(&list)
            .into_iter()
            .map(|(_, rest)| {
                rest.split("  ")
                    .last()
                    .unwrap()
                    .split(" — ")
                    .next()
                    .unwrap()
                    .to_string()
            })
            .collect()
    };
    let newest = titles(&["list"]);
    assert_eq!(newest.len(), 5);
    assert!(
        newest[4].starts_with("Propagating meteorological"),
        "the oldest is last: {newest:?}"
    );
    let by_score = titles(&["list", "--sort", "score"]);
    assert!(
        by_score[0].starts_with("Empirical evidence"),
        "{by_score:?}"
    );
    assert!(by_score[1].starts_with("Propagating"), "{by_score:?}");

    // one line: id, state, score, kind, date, title — journal
    let one = s.run(&["list", "--sort", "score", "--limit", "1"]);
    assert_eq!(one.err, "feeds: showing 1 of 5 items\n");
    let line = one.out;
    let id = line.split(' ').next().unwrap();
    assert_eq!(
        line,
        format!(
            "{id}     10.0  paper  2026-10-30  Empirical evidence of overestimated Ku-band sea ice \
             radar freeboards in satellite altimetry — TC - recent papers\n"
        )
    );
    let limited = s.run(&["list", "--limit", "2"]);
    assert_eq!(
        (limited.out.lines().count(), limited.err.as_str()),
        (2, "feeds: showing 2 of 5 items\n")
    );

    // the query's language: score ranges, kinds, words
    assert_eq!(titles(&["list", "score:3-"]).len(), 2);
    assert_eq!(titles(&["list", "--kind", "news"]).len(), 2);
    assert_eq!(
        titles(&["list", "--kind", "paper,news", "snow", "mountain"]),
        ["Propagating meteorological uncertainty in physically based mountain snow modeling"]
    );
    assert_eq!(s.run(&["list", "--kind", "podcast"]).code, 2);

    let json: serde_json::Value = serde_json::from_str(
        &s.run(&["list", "--sort", "score", "--json", "--limit", "1"])
            .out,
    )
    .unwrap();
    assert_eq!(json[0]["id"], id);
    assert_eq!(json[0]["score"], 10.0);
    assert_eq!(json[0]["interests"][0]["terms"][0], "sea ice");
    assert_eq!(json[0]["triage"], "new");
    assert_eq!(json[0]["authors"][0], "Catherine Taelman");
    assert!(json[0].get("sources").is_none());
}

#[test]
fn mark_shows_in_the_list_and_dismissed_items_are_left_out() {
    let s = fetched();
    s.ok(&["fetch"]);
    let list = s.ok(&["list", "--sort", "score"]);
    let rows = lines(&list);
    let (first, second, third) = (&rows[0].0, &rows[1].0, &rows[2].0);
    let said = s.ok(&["mark", "shortlisted", first, second]);
    assert!(
        said.starts_with(&format!("shortlisted {first} Empirical evidence")),
        "{said}"
    );
    assert_eq!(said.lines().count(), 2);
    s.ok(&["mark", "dismissed", third]);
    let after = s.ok(&["list", "--sort", "score"]);
    assert_eq!(after.lines().count(), 4);
    assert!(
        after
            .lines()
            .take(2)
            .all(|l| l.split("  ").nth(1) == Some("*")),
        "{after}"
    );
    assert!(!after.contains(third.as_str()));
    // asked for, or named by the query
    let all = s.ok(&["list", "--dismissed", "--sort", "score"]);
    assert_eq!(all.lines().count(), 5);
    assert!(
        all.lines().any(|l| l.starts_with(&format!("{third}  x  "))),
        "{all}"
    );
    assert_eq!(s.ok(&["list", "is:dismissed"]).lines().count(), 1);
    assert_eq!(s.ok(&["list", "is:shortlisted"]).lines().count(), 2);

    // an unknown id changes nothing
    let bad = s.run(&["mark", "added", first, "nope"]);
    assert_eq!(
        (bad.code, bad.err.as_str(), bad.out.as_str()),
        (1, "feeds: error: nope: no such item\n", "")
    );
    assert_eq!(s.json(&["show", first, "--json"])["triage"], "shortlisted");
    s.ok(&["mark", "new", first]);
    assert_eq!(s.json(&["show", first, "--json"])["triage"], "new");
    assert_eq!(s.run(&["mark", "starred", first]).code, 2);
    assert_eq!(s.run(&["mark", "seen"]).code, 2);
}

#[test]
fn show_gives_every_field() {
    let s = fetched();
    s.ok(&["fetch"]);
    let list = s.ok(&["list", "--sort", "score"]);
    let id = list.split(' ').next().unwrap();
    s.ok(&["mark", "shortlisted", id]);
    let sub = s.json(&["subscriptions", "--json"])[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let shown = s.ok(&["show", id]);
    let top = "Empirical evidence of overestimated Ku-band sea ice radar freeboards in satellite altimetry";
    assert!(
        shown.starts_with(&format!(
            "{top}\n\
         authors  Catherine Taelman, Jack C. Landy, Robbie D. C. Mallett, Polona Itkin\n\
         journal  TC - recent papers\n\
         date     2026-10-30\n\
         kind     paper\n\
         state    shortlisted (since 2026-10-31)\n\
         id       {id}\n\
         doi      10.5194/tc-20-6181-2026\n\
         url      https://doi.org/10.5194/tc-20-6181-2026\n\
         interests:\n  Sea ice: 10.0 (reached) sea ice, snow\n\
         versions:\n  10.5194/tc-20-6181-2026 (published)\n\
         sources:\n  TC - recent papers  first seen 2026-10-31, listed\n\
         abstract:\n  Arctic sea ice thickness is commonly estimated"
        )),
        "{shown}"
    );
    assert!(
        shown
            .lines()
            .filter(|l| l.starts_with("  "))
            .all(|l| l.len() <= 80),
        "{shown}"
    );

    let json = s.json(&["show", id, "--json"]);
    assert_eq!(json["doi"], "10.5194/tc-20-6181-2026");
    assert_eq!(json["triage"], "shortlisted");
    assert_eq!(json["sources"][0]["entry"]["source"], sub);
    assert_eq!(json["scores"][0]["score"], 10.0);
    assert_eq!(json["covered_by"], serde_json::json!([]));
    let gone = s.run(&["show", "nope"]);
    assert_eq!(
        (gone.code, gone.err.as_str()),
        (1, "feeds: error: nope: no such item\n")
    );
}

#[test]
fn a_post_links_its_paper_and_find_looks_again() {
    let work = format!("{WORKS}/10.1126/sciadv.aei1213");
    let title = "Circadian clock protein TIC condensates under salt stress";
    let crossref = format!(
        r#"{{"status":"ok","message-type":"work","message":{{"DOI":"10.1126/sciadv.aei1213","title":["{title}"],
        "container-title":["Science Advances"],"type":"journal-article",
        "author":[{{"given":"Wenwen","family":"Tian","sequence":"first"}}],
        "issued":{{"date-parts":[[2026,10,28]]}}}}}}"#
    );
    let superbug = format!("{WORKS}/10.1126/sciadv.aeh8222");
    let http = Recorded::new()
        .reply(FUTURITY, ok(FUTURITY, fixture("futurity.xml")))
        .reply(&work, ok(&work, crossref))
        .reply(&superbug, Reply::status(&superbug, 404));
    let s = Sandbox::new(http);
    s.ok(&["subscribe", FUTURITY, "--kind", "news"]);
    s.ok(&["fetch"]);
    let news = s.json(&["list", "--kind", "news", "--json"]);
    let post = news
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["title"].as_str().unwrap().starts_with("Internal clock"))
        .unwrap();
    let post = post["id"].as_str().unwrap();
    let paper = s.json(&["list", "--kind", "paper", "--json"])[0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let shown = s.ok(&["show", post]);
    assert!(
        shown.contains(&format!(
            "papers:\n  10.1126/sciadv.aei1213  high, via feed  {title} [{paper}]\n"
        )),
        "{shown}"
    );
    let shown = s.ok(&["show", &paper]);
    assert!(
        shown.contains(&format!(
            "covered by:\n  Internal clock helps plants respond to dangerous stress [{post}]\n"
        )),
        "{shown}"
    );
    let covered = s.json(&["show", &paper, "--json"])["covered_by"].clone();
    assert_eq!(covered[0]["id"], post);

    // `find` asks again: a post's papers, never a paper's
    let found = s.ok(&["find", post]);
    assert_eq!(
        found,
        format!("10.1126/sciadv.aei1213  high, via feed  {title} [{paper}]\n")
    );
    let bad = s.run(&["find", &paper]);
    assert_eq!(
        (bad.code, bad.err),
        (1, format!("feeds: error: {paper}: not a post\n"))
    );
}

#[test]
fn prune_keeps_what_a_feed_lists() {
    let s = fetched();
    s.ok(&["fetch"]);
    let out = s.ok(&["prune"]);
    assert!(out.starts_with("5 items kept"), "{out}");
}
