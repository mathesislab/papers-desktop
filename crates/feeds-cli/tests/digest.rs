//! `feeds digest`: paper-feeder's RSS feed and HTML page of the items that reach an interest.

mod support;

use papers_feeds::http::Recorded;
use support::{Sandbox, ok};

const URL: &str = "https://example.org/journal.atom";

/// An Atom feed: a matching paper whose title needs escaping, one that matches in the
/// abstract only, one that does not match, one of long ago, one to dismiss.
fn atom() -> String {
    let names = [
        "Anne Moreau",
        "B. Ito",
        "C. Diaz",
        "D. Eze",
        "E. Fox",
        "F. Gil",
        "G. Han",
        "H. Ivo",
    ];
    let authors: String = names
        .iter()
        .map(|n| format!("<author><name>{n}</name></author>"))
        .collect();
    format!(
        "<?xml version='1.0'?><feed xmlns=\"http://www.w3.org/2005/Atom\" \
         xmlns:arxiv=\"http://arxiv.org/schemas/atom\"><title>Journal</title><id>feed</id>\
         <entry><id>a</id><title>Ice sheet &amp; \"melt\" in Greenland's fjords</title>\
          <link href=\"https://example.org/a\"/><published>2026-10-30T00:00:00Z</published>{authors}\
          <summary>A study of the Greenland ice sheet.</summary><arxiv:doi>10.5194/tc-1-2026</arxiv:doi></entry>\
         <entry><id>b</id><title>Marine terraces</title><link href=\"https://example.org/b\"/>\
          <published>2026-10-29T00:00:00Z</published><author><name>Ola Berg</name></author>\
          <summary>On the Greenland ice sheet too.</summary></entry>\
         <entry><id>c</id><title>Protein folding</title><link href=\"https://example.org/c\"/>\
          <published>2026-10-29T00:00:00Z</published><summary>Proteins.</summary></entry>\
         <entry><id>d</id><title>The ice sheet of long ago</title><link href=\"https://example.org/d\"/>\
          <published>2026-08-01T00:00:00Z</published><summary>Old.</summary></entry>\
         <entry><id>e</id><title>Ice sheet dismissed</title><link href=\"https://example.org/e\"/>\
          <published>2026-10-30T00:00:00Z</published><summary>Dismissed.</summary></entry>\
         </feed>"
    )
}

fn digest_sandbox() -> Sandbox {
    let s = Sandbox::new(Recorded::new().reply(URL, ok(URL, atom())));
    s.config(
        r#"{"interests":[{"name":"Ice","include":[{"term":"ice sheet","weight":3},
        {"term":"Greenland","weight":2}],"min_score":3}]}"#,
    );
    s.ok(&["subscribe", URL, "--journal", "The Cryosphere"]);
    s.ok(&["fetch"]);
    let dismissed = s.json(&["list", "Ice sheet dismissed", "--json"])[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.ok(&["mark", "dismissed", &dismissed]);
    s
}

#[test]
fn the_rss_feed_has_paper_feeders_items() {
    let s = digest_sandbox();
    let rss = s.path("feed.xml");
    let said = s.ok(&[
        "digest",
        "--rss",
        rss.to_str().unwrap(),
        "--title",
        "Ice & papers",
        "--link",
        "https://example.org/feed/",
    ]);
    assert_eq!(said, format!("2 items in {}\n", rss.display()));
    let xml = std::fs::read_to_string(&rss).unwrap();
    assert!(
        xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\""),
        "{xml}"
    );
    assert!(
        xml.contains("<lastBuildDate>Sat, 31 Oct 2026 08:00:00 +0000</lastBuildDate>"),
        "{xml}"
    );

    let feed = feed_rs::parser::parse(xml.as_bytes()).unwrap();
    assert_eq!(feed.title.unwrap().content, "Ice & papers");
    assert_eq!(feed.links[0].href, "https://example.org/feed/");
    // the best score first; the old, the dismissed and the unmatched are not there
    let titles: Vec<&str> = feed
        .entries
        .iter()
        .map(|e| e.title.as_ref().unwrap().content.as_str())
        .collect();
    assert_eq!(
        titles,
        [
            "Ice sheet & \"melt\" in Greenland's fjords",
            "Marine terraces"
        ]
    );
    let first = &feed.entries[0];
    assert_eq!(first.id, "10.5194/tc-1-2026", "the guid is the DOI");
    assert_eq!(first.links[0].href, "https://doi.org/10.5194/tc-1-2026");
    let authors: Vec<&str> = first
        .authors
        .iter()
        .map(|a| a.name.as_deref().unwrap())
        .collect();
    assert_eq!(
        authors,
        ["Anne Moreau, B. Ito, C. Diaz, D. Eze, E. Fox, F. Gil, … (+2)"]
    );
    assert_eq!(first.categories[0].term, "The Cryosphere");
    assert_eq!(
        first.published.unwrap().to_rfc3339(),
        "2026-10-30T00:00:00+00:00"
    );
    let summary = first.summary.as_ref().unwrap().content.as_str();
    assert_eq!(
        summary,
        "The Cryosphere — matched: ice sheet, Greenland\n\nA study of the Greenland ice sheet."
    );
    // no DOI: the guid is the link
    assert_eq!(feed.entries[1].id, "https://example.org/b");
    assert_eq!(
        feed.entries[1].summary.as_ref().unwrap().content,
        "The Cryosphere — matched: ice sheet, Greenland\n\nOn the Greenland ice sheet too."
    );
}

#[test]
fn the_window_and_the_query_choose_the_items() {
    let s = digest_sandbox();
    let rss = s.path("feed.xml");
    let rss = rss.to_str().unwrap();
    // a longer window brings the old paper; a query narrows
    assert!(
        s.ok(&["digest", "--rss", rss, "--days", "120"])
            .starts_with("3 items in ")
    );
    assert!(
        s.ok(&["digest", "--rss", rss, "--query", "score:9-"])
            .starts_with("1 items in ")
    );
    assert!(
        s.ok(&["digest", "--rss", rss, "--query", "terraces", "--days", "1"])
            .starts_with("0 items in ")
    );
    let xml = std::fs::read_to_string(rss).unwrap();
    assert!(!xml.contains("<item>"), "{xml}");
    assert_eq!(s.run(&["digest"]).code, 2, "an output is needed");
}

#[test]
fn the_html_page_links_the_feed() {
    let s = digest_sandbox();
    let (rss, html) = (s.path("out/feed.xml"), s.path("out/index.html"));
    std::fs::create_dir_all(s.path("out")).unwrap();
    s.ok(&[
        "digest",
        "--rss",
        rss.to_str().unwrap(),
        "--html",
        html.to_str().unwrap(),
        "--title",
        "Ice & papers",
    ]);
    let page = std::fs::read_to_string(&html).unwrap();
    assert!(page.starts_with("<!doctype html><html lang=en>"), "{page}");
    assert!(page.contains("<link rel=\"alternate\" type=\"application/rss+xml\" title=\"Ice &amp; papers\" href=\"feed.xml\">"), "{page}");
    assert!(page.contains("<h1>Ice &amp; papers</h1>"), "{page}");
    assert!(page.contains("2 papers (2 new) · generated 2026-10-31 · <a class=\"rss\" href=\"feed.xml\">RSS feed</a>"), "{page}");
    assert!(page.contains("<h2><a href=\"https://doi.org/10.5194/tc-1-2026\" target=\"_blank\" rel=\"noopener noreferrer\">Ice sheet &amp; &quot;melt&quot; in Greenland&#39;s fjords</a></h2>"), "{page}");
    assert!(
        page.contains(
            "<div class=\"meta\">Anne Moreau, B. Ito, C. Diaz, D. Eze, E. Fox, F. Gil, … (+2)</div>"
        ),
        "{page}"
    );
    assert!(
        page.contains(
            "<div class=\"meta\">The Cryosphere · 2026-10-30 <span class=\"new\">new</span></div>"
        ),
        "{page}"
    );
    assert!(
        page.contains("<div class=\"why\"><span class=\"score\">"),
        "{page}"
    );
    assert!(
        page.contains("matched: ice sheet, Greenland</div>"),
        "{page}"
    );
    assert!(page.contains("<details><summary>abstract</summary><p>A study of the Greenland ice sheet.</p></details>"), "{page}");
    assert!(!page.contains("Protein"), "{page}");
    // without the feed written, no link to it
    let alone = s.path("alone.html");
    s.ok(&["digest", "--html", alone.to_str().unwrap()]);
    let page = std::fs::read_to_string(&alone).unwrap();
    assert!(
        !page.contains("RSS") && !page.contains("alternate"),
        "{page}"
    );
}
