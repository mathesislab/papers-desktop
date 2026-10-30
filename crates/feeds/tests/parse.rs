//! The recorded feeds (October 2026, trimmed to a few items) as entries.

use papers_feeds::{Entry, ItemKind, SourceKind, Subscription, parse};

fn subscription(kind: SourceKind) -> Subscription {
    Subscription {
        id: "sub".to_string(),
        url: "https://example.org/feed".to_string(),
        title: None,
        kind,
        journal: None,
        fetch_pages: false,
    }
}

fn entries(fixture: &str, kind: SourceKind) -> (Option<String>, Vec<Entry>) {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let body = std::fs::read(path).unwrap();
    let parsed = parse(&body, None, &subscription(kind)).unwrap();
    (parsed.title, parsed.entries)
}

#[test]
fn copernicus_authors_and_abstract_from_the_description() {
    let (title, entries) = entries("copernicus-tc.xml", SourceKind::Journal);
    assert_eq!(title.as_deref(), Some("TC - recent papers"));
    assert_eq!(entries.len(), 3);
    let e = &entries[0];
    assert_eq!(e.doi.as_deref(), Some("10.5194/tc-20-6181-2026"));
    assert_eq!(
        e.authors,
        [
            "Catherine Taelman",
            "Jack C. Landy",
            "Robbie D. C. Mallett",
            "Polona Itkin"
        ]
    );
    let summary = e.summary.as_deref().unwrap();
    assert!(summary.starts_with("Arctic sea ice thickness is commonly estimated"));
    assert!(summary.contains("“invisible”") || summary.contains("\"invisible\""));
    assert!(!summary.contains("doi.org") && !summary.contains("Taelman"));
    assert_eq!(e.kind, ItemKind::Paper);
    assert_eq!(
        entries[1].authors.last().map(String::as_str),
        Some("Xiao Cheng")
    );
}

#[test]
fn nature_rss1_with_dc_and_prism() {
    let (title, entries) = entries("nature-ngeo.rss", SourceKind::Journal);
    assert_eq!(title.as_deref(), Some("Nature Geoscience"));
    let e = &entries[0];
    assert_eq!(e.doi.as_deref(), Some("10.1038/s41561-026-02107-w"));
    assert_eq!(e.authors.len(), 11);
    assert_eq!(e.authors[0], "Lingcheng Li");
    assert_eq!(e.journal.as_deref(), Some("Nature Geoscience"));
    let summary = e.summary.as_deref().unwrap();
    assert!(
        summary.starts_with("The influence of atmospheric dryness"),
        "{summary}"
    );
    assert!(e.date.is_some());
    // an item without dc:creator
    assert!(entries[1].authors.is_empty());
    assert_eq!(
        entries[1].doi.as_deref(),
        Some("10.1038/s41561-026-02127-6")
    );
}

#[test]
fn wiley_doi_authors_blob_and_dc_description() {
    let (_, entries) = entries("wiley-grl.xml", SourceKind::Journal);
    assert_eq!(entries.len(), 3);
    let e = &entries[0];
    assert_eq!(e.doi.as_deref(), Some("10.1029/2026gl123955"));
    assert_eq!(
        e.authors,
        [
            "Eric Berg",
            "Taiyi A. Wang",
            "Elizabeth Wig",
            "Paul Segall",
            "Kyle Anderson"
        ]
    );
    assert_eq!(e.journal.as_deref(), Some("Geophysical Research Letters"));
    // the abstract of dc:description, not the citation of the description
    let summary = e.summary.as_deref().unwrap();
    assert!(
        summary.starts_with("We utilized geodetic data"),
        "{summary}"
    );
    assert!(!e.editorial);
}

#[test]
fn arxiv_atom_with_dc_creator() {
    let (_, entries) = entries("arxiv-ao-ph.atom", SourceKind::Preprints);
    assert_eq!(entries.len(), 4);
    let e = &entries[0];
    assert_eq!(e.arxiv.as_deref(), Some("2610.38522"));
    assert_eq!(e.doi, None);
    assert_eq!(e.authors, ["Anastassia M. Makarieva", "Andrei V. Nefiodov"]);
    assert_eq!(e.url.as_deref(), Some("https://arxiv.org/abs/2610.38522"));
    let summary = e.summary.as_deref().unwrap();
    assert!(
        summary.starts_with("Atmospheric power is commonly assessed"),
        "{summary}"
    );
    // `&lt;0` of the abstract is text, not a tag
    assert!(summary.contains(r"\simeq-W_P<0$"), "{summary}");
    assert_eq!(e.kind, ItemKind::Preprint);
    assert_eq!(
        entries[2].arxiv.as_deref(),
        Some("2604.27200"),
        "a replacement, v3"
    );
}

#[test]
fn futurity_post_with_its_text() {
    let (_, entries) = entries("futurity.xml", SourceKind::News);
    let e = &entries[0];
    assert_eq!(e.kind, ItemKind::News);
    assert_eq!(e.doi, None, "the paper's DOI is in the text: found later");
    assert_eq!(
        e.summary.as_deref(),
        Some(
            "New research reveals how a plant's internal clock helps it to respond to dangerous salts in its environment."
        )
    );
    let content = e.content.as_deref().unwrap();
    assert!(content.contains("doi.org/10.1126/sciadv.aei1213"));
    assert_eq!(e.authors, ["Washington U. in St. Louis"]);
}

#[test]
fn physorg_teasers() {
    let (_, entries) = entries("physorg-earth.xml", SourceKind::News);
    assert_eq!(entries.len(), 3);
    let e = &entries[0];
    assert_eq!(e.guid, "news712583101");
    assert!(e.authors.is_empty());
    assert!(
        e.summary
            .as_deref()
            .unwrap()
            .contains("Earth's rocky places")
    );
    assert_eq!(e.content, None);
}

#[test]
fn realclimate_blog_posts() {
    let (_, entries) = entries("realclimate.xml", SourceKind::Blog);
    let e = &entries[0];
    assert_eq!(e.title, "Unforced Variations: Oct 2026");
    assert_eq!(e.kind, ItemKind::Blog);
    assert!(e.content.as_deref().unwrap().contains("<p"));
    assert!(
        e.summary
            .as_deref()
            .unwrap()
            .starts_with("This month’s open thread.")
    );
}

const SMALL: &str = r#"<?xml version="1.0"?>
<rss version="2.0"><channel><title>The Cryosphere</title>
  <item>
    <title>Grounding line retreat in West Antarctica</title>
    <link>https://tc.copernicus.org/articles/18/1/2024/</link>
    <guid>https://doi.org/10.5194/tc-18-1-2024</guid>
    <description>We model ice sheet retreat.</description>
    <pubDate>Mon, 15 Jan 2024 00:00:00 +0000</pubDate>
  </item>
  <item>
    <title>Perspective: the future of ice-sheet modelling</title>
    <link>https://tc.copernicus.org/articles/18/2/2024/</link>
    <guid>10.5194/tc-18-2-2024</guid>
  </item>
  <item>
    <title>A discussion paper</title>
    <link>https://egusphere.copernicus.org/preprints/2025/egusphere-2025-6066/</link>
    <guid>https://doi.org/10.5194/egusphere-2025-6066</guid>
  </item>
</channel></rss>"#;

#[test]
fn doi_from_the_guid_editorials_and_preprints() {
    let mut sub = subscription(SourceKind::Journal);
    sub.journal = Some("The Cryosphere".to_string());
    let entries = parse(SMALL.as_bytes(), None, &sub).unwrap().entries;
    assert_eq!(entries[0].doi.as_deref(), Some("10.5194/tc-18-1-2024"));
    assert_eq!(
        entries[0].summary.as_deref(),
        Some("We model ice sheet retreat.")
    );
    assert_eq!(entries[0].journal.as_deref(), Some("The Cryosphere"));
    assert!(!entries[0].editorial);
    assert_eq!(entries[1].doi.as_deref(), Some("10.5194/tc-18-2-2024"));
    assert!(entries[1].editorial);
    assert_eq!(entries[1].summary, None);
    assert_eq!(
        entries[2].kind,
        ItemKind::Preprint,
        "a journal's feed lists the preprints too"
    );
}

#[test]
fn not_a_feed() {
    let sub = subscription(SourceKind::Journal);
    let error = parse(b"<html><body>Just a moment...</body></html>", None, &sub).unwrap_err();
    assert!(error.to_string().starts_with("https://example.org/feed: "));
}
