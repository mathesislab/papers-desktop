//! The keys of the merging: DOIs, arXiv ids, titles, families, feed addresses.

use papers_feeds::normalize::*;

#[test]
fn canonical_doi_of_its_spellings() {
    let cases = [
        (
            "https://doi.org/10.1038/s41586-024-01234-5",
            "10.1038/s41586-024-01234-5",
        ),
        ("doi:10.1038/ABC.123", "10.1038/abc.123"),
        ("10.5194/tc-18-1-2024", "10.5194/tc-18-1-2024"),
        ("(10.1038/xyz).", "10.1038/xyz"),
        ("\"10.1038/xyz\";", "10.1038/xyz"),
        (
            "https://doi.org/10.1002%2F2016GL070000",
            "10.1002/2016gl070000",
        ),
        (
            "https://agupubs.onlinelibrary.wiley.com/doi/10.1029/2026GL123955?af=R",
            "10.1029/2026gl123955",
        ),
    ];
    for (text, doi) in cases {
        assert_eq!(canonical_doi(text).as_deref(), Some(doi), "{text}");
    }
    for text in ["", "no doi here", "10.12/too-short-registrant"] {
        assert_eq!(canonical_doi(text), None, "{text}");
    }
}

#[test]
fn preprint_dois() {
    assert!(is_preprint_doi("10.5194/egusphere-2025-6066"));
    assert!(is_preprint_doi("10.22541/essoar.1234"));
    assert!(is_preprint_doi("10.48550/arxiv.2610.38522"));
    assert!(!is_preprint_doi("10.5194/tc-20-5895-2026"));
    assert!(!is_preprint_doi("10.5194/gmd-19-1-2026"));
}

#[test]
fn arxiv_id_without_version() {
    let cases = [
        ("oai:arXiv.org:2610.38522v1", "2610.38522"),
        ("https://arxiv.org/abs/2604.27200v3", "2604.27200"),
        ("https://arxiv.org/pdf/2405.04836", "2405.04836"),
        ("arXiv:2610.38775", "2610.38775"),
        ("10.48550/arXiv.2610.38522", "2610.38522"),
        ("http://arxiv.org/abs/hep-th/9901001v2", "hep-th/9901001"),
    ];
    for (text, id) in cases {
        assert_eq!(arxiv_id(text).as_deref(), Some(id), "{text}");
    }
    assert_eq!(
        arxiv_id("https://www.nature.com/articles/s41561-026-02107-w"),
        None
    );
}

#[test]
fn title_key_folds_tags_accents_punctuation() {
    assert_eq!(
        title_key("Ice-Sheet <i>Dynamics</i>!"),
        "ice sheet dynamics"
    );
    assert_eq!(
        title_key("Élan of the ﬁrst Müller"),
        "elan of the first muller"
    );
    assert_eq!(title_key("CO₂ and H<sub>2</sub>O"), "co2 and h 2 o");
}

#[test]
fn jaccard_of_title_words() {
    let a = title_words("Grounding line retreat of the West Antarctic Ice Sheet");
    let b = title_words("Grounding-line retreat of the West Antarctic ice sheet.");
    assert_eq!(jaccard(&a, &b), 1.0);
    let c = title_words("Grounding line retreat of the West Antarctic Ice Shelf");
    assert!((jaccard(&a, &c) - 0.8).abs() < 1e-9, "8 shared of 10");
    assert_eq!(jaccard(&title_words(""), &title_words("")), 0.0);
}

#[test]
fn family_of_both_orders() {
    assert_eq!(family("Alexander Robinson").as_deref(), Some("robinson"));
    assert_eq!(family("Robinson, A.").as_deref(), Some("robinson"));
    assert_eq!(family("Rónadh Cox").as_deref(), Some("cox"));
    assert_eq!(family("  "), None);
}

#[test]
fn feed_addresses() {
    let url = canonical_url(" HTTPS://TC.Copernicus.org/xml/rss2_0.xml#top ").unwrap();
    assert_eq!(url, "https://tc.copernicus.org/xml/rss2_0.xml");
    let http = canonical_url("http://tc.copernicus.org/xml/rss2_0.xml").unwrap();
    assert_eq!(feed_key(&url), feed_key(&http));
    assert!(canonical_url("ftp://example.org/feed").is_err());
    assert!(canonical_url("not a url").is_err());
}

#[test]
fn short_ids_are_stable() {
    assert_eq!(
        short_id("doi:10.5194/tc-20-5895-2026"),
        short_id("doi:10.5194/tc-20-5895-2026")
    );
    assert_ne!(short_id("doi:10.1/a"), short_id("doi:10.1/b"));
    assert_eq!(short_id("").len(), 12);
}

#[test]
fn review_comments_and_supplements_are_not_papers() {
    for doi in [
        "10.5194/egusphere-2025-6066-rc1",
        "10.5194/egusphere-2025-6066-ac2",
        "10.5194/egusphere-2025-6066-cc1",
        "10.5194/egusphere-2025-6066-ec1",
        "10.5194/egusphere-2025-6066-rc1-supplement",
        "10.5194/tc-20-5895-2026-supplement",
    ] {
        assert!(is_attachment_doi(doi), "{doi}");
    }
    for doi in [
        "10.5194/egusphere-2025-6066",
        "10.5194/tc-20-5895-2026",
        "10.1029/2026gl123955",
    ] {
        assert!(!is_attachment_doi(doi), "{doi}");
    }
}

#[test]
fn news_dois() {
    for doi in [
        "10.1029/2026eo260345",
        "10.64628/aa.abc123",
        "10.1038/d41586-026-01234-5",
    ] {
        assert!(is_news_doi(doi), "{doi}");
    }
    for doi in [
        "10.1029/2026ea005360",
        "10.1029/2021av000469",
        "10.1038/s41561-026-02107-w",
    ] {
        assert!(!is_news_doi(doi), "{doi}");
    }
}

#[test]
fn dois_of_a_text() {
    assert_eq!(
        dois_in("DOI: 10.1126/sciadv.aei1213. See also (doi:10.1029/2026GL1)."),
        ["10.1126/sciadv.aei1213", "10.1029/2026gl1"]
    );
}

#[test]
fn issns_with_their_check_digit() {
    assert_eq!(canonical_issn("1994-0424").as_deref(), Some("1994-0424"));
    assert_eq!(canonical_issn(" 19940424 ").as_deref(), Some("1994-0424"));
    assert_eq!(canonical_issn("0094-8276").as_deref(), Some("0094-8276"));
    assert_eq!(canonical_issn("2169-897x").as_deref(), Some("2169-897X"));
    for wrong in ["1994-0425", "1994-042", "abcd-efgh", ""] {
        assert_eq!(canonical_issn(wrong), None, "{wrong}");
    }
}
