//! The free copy of a paper, from OpenAlex's locations of its DOI, or arXiv.

use chrono::Utc;
use papers_feeds::http::Recorded;
use papers_feeds::{Access, Item, ItemKind, OpenAccess, Pdf, Reply, pdf};

const WORK: &str =
    "https://api.openalex.org/works/doi:10.5194/tc-20-1-2026?select=id,best_oa_location,locations";

fn item(kind: ItemKind, doi: Option<&str>, arxiv: Option<&str>) -> Item {
    Item {
        id: "i".into(),
        kind,
        title: "T".into(),
        authors: Vec::new(),
        journal: None,
        date: Some(Utc::now()),
        doi: doi.map(String::from),
        arxiv: arxiv.map(String::from),
        url: None,
        summary: None,
        content: None,
        editorial: false,
        sources: Vec::new(),
        versions: Vec::new(),
        links: Vec::new(),
    }
}

/// A location: version, PDF, page, licence.
fn location(
    version: &str,
    pdf: Option<&str>,
    page: &str,
    licence: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({"is_oa": true, "version": version, "pdf_url": pdf,
        "landing_page_url": page, "license": licence})
}

fn found(work: serde_json::Value, item: &Item) -> Pdf {
    let http = Recorded::new().reply(WORK, Reply::ok(WORK, work.to_string()));
    pdf(item, &http, &Access::default()).unwrap()
}

#[test]
fn the_published_version_first_then_a_preprint() {
    let paper = item(ItemKind::Paper, Some("10.5194/tc-20-1-2026"), None);
    let submitted = location(
        "submittedVersion",
        Some("https://egusphere.net/p.pdf"),
        "https://egusphere.net/p",
        None,
    );
    let published = location(
        "publishedVersion",
        Some("https://www.tc.net/tc.pdf"),
        "https://tc.net",
        Some("cc-by"),
    );
    let work =
        serde_json::json!({"best_oa_location": submitted, "locations": [submitted, published]});
    let open = found(work, &paper);
    assert_eq!(open.access, OpenAccess::Open);
    assert_eq!(open.url.as_deref(), Some("https://www.tc.net/tc.pdf"));
    assert_eq!(open.licence.as_deref(), Some("cc-by"));
    assert_eq!(open.host.as_deref(), Some("tc.net"));

    let work = serde_json::json!({"best_oa_location": submitted, "locations": [submitted]});
    let preprint = found(work.clone(), &paper);
    assert_eq!(preprint.access, OpenAccess::Preprint);
    assert_eq!(preprint.url.as_deref(), Some("https://egusphere.net/p.pdf"));
    // a preprint's own copy is open
    let own = item(ItemKind::Preprint, Some("10.5194/tc-20-1-2026"), None);
    assert_eq!(found(work, &own).access, OpenAccess::Open);

    let closed = serde_json::json!({"best_oa_location": null,
        "locations": [{"is_oa": false, "landing_page_url": "https://tc.net"}]});
    assert_eq!(found(closed, &paper).access, OpenAccess::Closed);
}

#[test]
fn unknown_without_a_doi_or_in_openalex_and_arxiv_as_the_last_resort() {
    let paper = item(ItemKind::Paper, Some("10.5194/tc-20-1-2026"), None);
    let http = Recorded::new().reply(WORK, Reply::status(WORK, 404));
    let unknown = pdf(&paper, &http, &Access::default()).unwrap();
    assert_eq!(unknown.access, OpenAccess::Unknown);
    let none = item(ItemKind::Paper, None, None);
    assert_eq!(
        pdf(&none, &Recorded::new(), &Access::default())
            .unwrap()
            .access,
        OpenAccess::Unknown
    );

    let preprint = item(ItemKind::Preprint, None, Some("2410.01234"));
    let arxiv = pdf(&preprint, &Recorded::new(), &Access::default()).unwrap();
    assert_eq!(arxiv.access, OpenAccess::Open);
    assert_eq!(
        arxiv.url.as_deref(),
        Some("https://arxiv.org/pdf/2410.01234")
    );
    let both = item(
        ItemKind::Paper,
        Some("10.5194/tc-20-1-2026"),
        Some("2410.01234"),
    );
    let http = Recorded::new().reply(WORK, Reply::status(WORK, 404));
    assert_eq!(
        pdf(&both, &http, &Access::default()).unwrap().access,
        OpenAccess::Preprint
    );
    // no answer
    let http = Recorded::new().fail(WORK, "offline");
    assert!(pdf(&paper, &http, &Access::default()).is_err());
}
