//! The kind of a feed guessed when it is subscribed to by its address.

use papers_feeds::http::Recorded;
use papers_feeds::{Access, Reply, SourceKind, guess_kind};

const SELECT: &str = "select=id,display_name,alternate_titles,issn_l,issn,host_organization_name,\
    homepage_url,works_count";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// An RSS feed of a title and items (title, link).
fn feed(title: &str, items: &[(&str, &str)]) -> Vec<u8> {
    let items: String = items
        .iter()
        .map(|(t, link)| {
            format!("<item><title>{t}</title><link>{link}</link><guid>{link}</guid></item>")
        })
        .collect();
    format!("<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>{title}</title>{items}</channel></rss>")
        .into_bytes()
}

fn search(text: &str) -> String {
    format!(
        "https://api.openalex.org/sources?search={text}&filter=type:journal&{SELECT}&per-page=10"
    )
}

#[test]
fn a_comic_is_general() {
    let url = "https://xkcd.com/rss.xml";
    let body = feed(
        "xkcd.com",
        &[("Earth Sized Planets", "https://xkcd.com/3000/")],
    );
    let found = search("xkcd.com");
    let empty = r#"{"results":[]}"#;
    let http = Recorded::new()
        .reply(url, Reply::ok(url, body))
        .reply(&found, Reply::ok(&found, empty));
    let guess = guess_kind(&http, &Access::default(), url).unwrap();
    assert_eq!(guess.kind, SourceKind::Blog, "{}", guess.reason);
}

#[test]
fn a_journal_by_its_dois_or_its_title() {
    let url = "https://example.org/jgr.rss";
    let body = feed(
        "JGR",
        &[
            ("A", "https://doi.org/10.1029/2026jc1"),
            ("B", "https://doi.org/10.1029/2026jc2"),
        ],
    );
    let http = Recorded::new().reply(url, Reply::ok(url, body));
    let guess = guess_kind(&http, &Access::default(), url).unwrap();
    assert_eq!(
        (guess.kind, guess.reason.as_str()),
        (SourceKind::Journal, "its items have DOIs")
    );

    // ScienceDirect's items have no DOI: the title is a journal's
    let url = "https://rss.sciencedirect.com/publication/science/00221694";
    let body = feed(
        "Journal of Glaciology",
        &[("A", "https://www.sciencedirect.com/science/article/pii/S1")],
    );
    let found = search("Journal%20of%20Glaciology");
    let http = Recorded::new().reply(url, Reply::ok(url, body)).reply(
        &found,
        Reply::ok(&found, fixture("openalex-sources-glaciology.json")),
    );
    let guess = guess_kind(&http, &Access::default(), url).unwrap();
    assert_eq!(
        (guess.kind, guess.reason.as_str()),
        (SourceKind::Journal, "a journal in OpenAlex")
    );
}

#[test]
fn a_preprint_server_by_its_host() {
    let http = Recorded::new();
    let url = "https://export.arxiv.org/rss/physics.ao-ph";
    let guess = guess_kind(&http, &Access::default(), url).unwrap();
    assert_eq!(guess.kind, SourceKind::Preprints);
    assert!(http.requests().is_empty(), "not read");
}
