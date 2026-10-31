//! The lookups by DOI after a fetch: abstracts for teasers, a preprint and its
//! paper as one item (Crossref, October 2026: tc-20-5895-2026 and
//! egusphere-2025-6066), review comments left out, each DOI once, OpenAlex for
//! what Crossref lacks.

use papers_feeds::http::Recorded;
use papers_feeds::{Access, Feed, Feeds, ItemKind, LOOKUP, Reply, SourceKind, Stage, Triage};

const TC: &str = "https://tc.copernicus.org/xml/rss2_0.xml";
const EGUSPHERE: &str = "https://egusphere.copernicus.org/xml/rss2_0.xml";
const WORKS: &str = "https://api.crossref.org/works/";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn rss(url: &str) -> Feed {
    Feed::Rss {
        url: url.to_string(),
    }
}

/// An RSS feed of items (title, DOI, description).
fn feed(items: &[(&str, &str, &str)]) -> Vec<u8> {
    let items: String = items
        .iter()
        .map(|(title, doi, description)| {
            format!(
                "<item><title>{title}</title><link>https://doi.org/{doi}</link>\
                 <guid>https://doi.org/{doi}</guid><description>{description}</description>\
                 <pubDate>Mon, 26 Oct 2026 00:00:00 +0000</pubDate></item>"
            )
        })
        .collect();
    format!("<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>Feed</title>{items}</channel></rss>")
        .into_bytes()
}

const TITLE: &str =
    "Propagating meteorological uncertainty in physically based mountain snow modeling";
const TC_DOI: &str = "10.5194/tc-20-5895-2026";
const EGU_DOI: &str = "10.5194/egusphere-2025-6066";

#[test]
fn a_preprint_and_its_paper_become_one_item() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    feeds
        .subscribe(rss(EGUSPHERE), SourceKind::Preprints)
        .unwrap();
    feeds.subscribe(rss(TC), SourceKind::Journal).unwrap();
    let egu_feed = feed(&[
        (
            "Propagating Meteorological Uncertainty",
            EGU_DOI,
            "A teaser.",
        ),
        (
            "Comment on egusphere-2025-6066",
            "10.5194/egusphere-2025-6066-RC1",
            "A referee's comment.",
        ),
    ]);
    let tc_feed = feed(&[(TITLE, TC_DOI, "Snow estimation is uncertain.")]);
    let (egu, tc) = (format!("{WORKS}{EGU_DOI}"), format!("{WORKS}{TC_DOI}"));
    let http = Recorded::new()
        .reply(EGUSPHERE, Reply::ok(EGUSPHERE, egu_feed.clone()))
        .reply(TC, Reply::ok(TC, tc_feed.clone()))
        .fail(&egu, "offline")
        .reply(EGUSPHERE, Reply::status(EGUSPHERE, 304))
        .reply(TC, Reply::status(TC, 304))
        .reply(
            &egu,
            Reply::ok(&egu, fixture("crossref-egusphere-2025-6066.json")),
        )
        .reply(
            &tc,
            Reply::ok(&tc, fixture("crossref-tc-20-5895-2026.json")),
        );

    // offline: two items, and no item for the review comment
    let report = feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(report.lookups, 1);
    assert_eq!(feeds.items().len(), 2);
    let (pre, paper) = (feeds.items()[0].id.clone(), feeds.items()[1].id.clone());
    feeds.set_triage(&pre, Triage::Shortlisted).unwrap();
    feeds.set_triage(&paper, Triage::Seen).unwrap();

    // Crossref relates them: one item, the published version first
    let report = feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(report.lookups, 2);
    assert_eq!(feeds.items().len(), 1);
    let item = &feeds.items()[0];
    assert_eq!(item.id, pre, "the older id");
    assert_eq!(feeds.resolve(&paper), pre);
    assert_eq!(item.kind, ItemKind::Paper);
    assert_eq!(item.title, TITLE);
    assert_eq!(item.doi.as_deref(), Some(TC_DOI));
    let stages: Vec<(&str, Stage)> = item
        .versions
        .iter()
        .map(|v| (v.doi.as_str(), v.stage))
        .collect();
    assert_eq!(
        stages,
        [(EGU_DOI, Stage::Preprint), (TC_DOI, Stage::Published)]
    );
    assert_eq!(
        item.authors[0], "David R. Casson",
        "Crossref's, the feeds had none"
    );
    let summary = item.summary.as_deref().unwrap();
    assert!(
        summary.starts_with("Snow estimation in mountainous regions"),
        "{summary}"
    );
    assert_eq!(
        feeds.triage(&paper),
        Triage::Shortlisted,
        "the strongest decision"
    );
    let lookups = item
        .sources
        .iter()
        .filter(|s| s.entry.source == LOOKUP)
        .count();
    assert_eq!(lookups, 2);
    assert!(
        item.sources
            .iter()
            .filter(|s| s.entry.source == LOOKUP)
            .all(|s| !s.listed)
    );

    // each DOI once
    let before = http.requests().len();
    let http = Recorded::new()
        .reply(EGUSPHERE, Reply::ok(EGUSPHERE, egu_feed))
        .reply(TC, Reply::ok(TC, tc_feed));
    feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(http.requests().len(), 2, "the feeds only ({before} before)");
    assert_eq!(feeds.items().len(), 1);
}

#[test]
fn teasers_first_and_openalex_for_what_crossref_lacks() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let journal = "https://example.org/journal.rss";
    feeds.subscribe(rss(journal), SourceKind::Journal).unwrap();
    let whole = "An abstract long enough to be whole. ".repeat(10);
    let items = feed(&[
        ("A paper with its abstract", "10.1000/whole", &whole),
        (
            "Snow model ensembles",
            "10.5281/zenodo.17740654",
            "Ensembles.",
        ),
    ]);
    let key = Access {
        mailto: None,
        openalex_key: Some("KEY".to_string()),
    };
    let zenodo = format!("{WORKS}10.5281/zenodo.17740654");
    let openalex = "https://api.openalex.org/works/doi:10.5281/zenodo.17740654?select=id,doi,title,\
        publication_date,authorships,primary_location,abstract_inverted_index,type&api_key=KEY";
    let whole_doi = format!("{WORKS}10.1000/whole");
    let http = Recorded::new()
        .reply(journal, Reply::ok(journal, items))
        .reply(&zenodo, Reply::status(&zenodo, 404))
        .reply(
            openalex,
            Reply::ok(openalex, fixture("openalex-work-doi.json")),
        )
        .reply(&whole_doi, Reply::status(&whole_doi, 404));
    let report = feeds.fetch(&http, &key).unwrap();
    let urls: Vec<String> = http.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(
        urls,
        [journal, &zenodo, openalex, &whole_doi],
        "the teaser first"
    );
    assert_eq!(report.lookups, 3);
    let dataset = &feeds.items()[1];
    assert_eq!(
        dataset.summary.as_deref(),
        Some("Ensembles of snow water of equivalent for three basins.")
    );
    assert_eq!(dataset.title, "Snow model ensembles", "the feed's");
    // not found is remembered
    let again = Recorded::new().reply(journal, Reply::status(journal, 304));
    feeds.fetch(&again, &key).unwrap();
    assert_eq!(again.requests().len(), 1);
}

#[test]
fn no_openalex_without_a_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let journal = "https://example.org/journal.rss";
    feeds.subscribe(rss(journal), SourceKind::Journal).unwrap();
    let items = feed(&[(
        "Snow model ensembles",
        "10.5281/zenodo.17740654",
        "Ensembles.",
    )]);
    let zenodo = format!("{WORKS}10.5281/zenodo.17740654?mailto=papers-desktop@example.org");
    let http = Recorded::new()
        .reply(journal, Reply::ok(journal, items))
        .reply(&zenodo, Reply::status(&zenodo, 404));
    let access = Access {
        mailto: Some("papers-desktop@example.org".to_string()),
        openalex_key: None,
    };
    feeds.fetch(&http, &access).unwrap();
    assert_eq!(http.requests().len(), 2);
    assert_eq!(feeds.items()[0].summary.as_deref(), Some("Ensembles."));
}
