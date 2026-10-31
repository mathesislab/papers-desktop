//! The paper behind a post, through the feeds: in the feed's text at fetch
//! time, on the page on request or for a subscription that asks, by the title
//! in Crossref; the paper item made, and "Covered by".

use papers_feeds::http::Recorded;
use papers_feeds::{
    Access, Confidence, Error, Feed, Feeds, ItemKind, Linked, Reply, SourceKind, Via, paragraphs,
};

const FUTURITY: &str = "https://www.futurity.org/feed/";
const PHYSORG: &str = "https://phys.org/rss-feed/earth-news/";
const WORKS: &str = "https://api.crossref.org/works";
const SELECT: &str = "select=DOI,title,author,container-title,abstract,URL,issued,type,relation";

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

/// A Crossref work of a DOI, a title and a journal.
fn work(doi: &str, title: &str, journal: &str) -> String {
    format!(
        r#"{{"status":"ok","message-type":"work","message":{{"DOI":"{doi}","title":["{title}"],
        "container-title":["{journal}"],"type":"journal-article",
        "author":[{{"given":"Wenwen","family":"Tian","sequence":"first"}}],
        "issued":{{"date-parts":[[2026,10,28]]}}}}}}"#
    )
}

fn linked(doi: &str, confidence: Confidence, via: Via) -> Linked {
    Linked {
        doi: doi.to_string(),
        confidence,
        via,
    }
}

#[test]
fn futurity_posts_link_their_papers_from_the_feed() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let futurity = feeds.subscribe(rss(FUTURITY), SourceKind::News).unwrap();
    let plants = format!("{WORKS}/10.1126/sciadv.aei1213");
    let superbug = format!("{WORKS}/10.1126/sciadv.aeh8222");
    let title = "Circadian clock protein TIC condensates under salt stress";
    let http = Recorded::new()
        .reply(FUTURITY, Reply::ok(FUTURITY, fixture("futurity.xml")))
        .reply(
            &plants,
            Reply::ok(
                &plants,
                work("10.1126/sciadv.aei1213", title, "Science Advances"),
            ),
        )
        .reply(&superbug, Reply::status(&superbug, 404));
    let report = feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(report.pages, 0, "no pages unless asked");
    assert_eq!(report.lookups, 2);
    let post = feeds.items()[0].clone();
    assert_eq!(post.kind, ItemKind::News);
    assert_eq!(
        post.links,
        [linked(
            "10.1126/sciadv.aei1213",
            Confidence::High,
            Via::Feed
        )]
    );
    assert_eq!(
        feeds.items()[1].links,
        [linked(
            "10.1126/sciadv.aeh8222",
            Confidence::High,
            Via::Feed
        )]
    );

    // the paper, from Crossref, of the post's source; none for a DOI Crossref lacks
    assert_eq!(feeds.items().len(), 3);
    let paper = feeds.items()[2].clone();
    assert_eq!(paper.kind, ItemKind::Paper);
    assert_eq!(paper.title, title);
    assert_eq!(paper.journal.as_deref(), Some("Science Advances"));
    assert_eq!(paper.sources[0].entry.source, futurity.id);
    let covered: Vec<&str> = feeds
        .covered_by(&paper.id)
        .iter()
        .map(|i| i.id.as_str())
        .collect();
    assert_eq!(covered, [post.id.as_str()]);
    assert!(feeds.covered_by(&post.id).is_empty());
    assert!(matches!(
        feeds.find_papers(&paper.id, &http, &Access::default()),
        Err(Error::NotAPost { .. })
    ));
}

#[test]
fn physorg_pages_read_at_fetch_time_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    // a subscription that asks for the pages (the settings will set it)
    let config = format!(
        r#"{{"subscriptions":[{{"id":"physorg","url":"{PHYSORG}","title":null,"kind":"news",
        "journal":null,"fetch_pages":true}}]}}"#
    );
    std::fs::write(dir.path().join("config.json"), config).unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let erosion = "https://phys.org/news/2026-10-hidden-erosion-reveals-soft-mountain.html";
    let carbon = "https://phys.org/news/2026-10-decade-carbon-dioxide-space.html";
    let paper = format!("{WORKS}/10.1126/sciadv.adw0001");
    let title = "Rock strength sets the pace of mountain erosion";
    let challenge = Reply::status(carbon, 403).with_body("<title>Checking your connection</title>");
    let http = Recorded::new()
        .reply(PHYSORG, Reply::ok(PHYSORG, fixture("physorg-earth.xml")))
        .reply(erosion, Reply::ok(erosion, fixture("page-physorg.html")))
        .reply(carbon, challenge);
    let report = feeds.fetch(&http, &Access::default()).unwrap();
    let post = &feeds.items()[0];
    assert_eq!(
        post.links,
        [linked(
            "10.1126/sciadv.adw0001",
            Confidence::High,
            Via::Page
        )]
    );
    assert!(feeds.items()[1].links.is_empty(), "Cloudflare's challenge");
    // the second page refused, its title searched, which got no answer: no more lookups
    let urls: Vec<String> = http.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(urls[..3], [PHYSORG, erosion, carbon]);
    assert!(urls[3].starts_with(&format!(
        "{WORKS}?query.bibliographic=A%20decade%20of%20carbon"
    )));
    assert_eq!(report.pages, 3);
    assert_eq!(report.lookups, 1);

    // each page once; the paper then made
    let http = Recorded::new()
        .reply(PHYSORG, Reply::status(PHYSORG, 304))
        .reply(
            &paper,
            Reply::ok(
                &paper,
                work("10.1126/sciadv.adw0001", title, "Science Advances"),
            ),
        );
    let report = feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(report.pages, 0);
    let urls: Vec<String> = http.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(urls[..2], [PHYSORG, paper.as_str()]);
    let made = feeds.items().iter().find(|i| i.title == title).unwrap();
    assert_eq!(feeds.covered_by(&made.id)[0].id, feeds.items()[0].id);
}

/// A feed of one post: its title, its page, its text.
fn post(title: &str, page: &str, text: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>Blog</title>\
         <item><title>{title}</title><link>{page}</link><guid>{page}</guid>\
         <description><![CDATA[{text}]]></description></item></channel></rss>"
    )
    .into_bytes()
}

#[test]
fn eos_page_with_its_own_doi_then_a_search_that_does_not_match() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let eos = "https://eos.org/feed";
    feeds.subscribe(rss(eos), SourceKind::News).unwrap();
    let page = "https://eos.org/articles/u-s-river-sediment-transport-is-becoming-burstier";
    let title = "U.S. River Sediment Transport Is Becoming “Burstier”";
    let search = format!(
        "{WORKS}?query.bibliographic=U.S.%20River%20Sediment%20Transport%20Is%20Becoming%20\
         %E2%80%9CBurstier%E2%80%9D&rows=5&{SELECT}"
    );
    let http = Recorded::new()
        .reply(
            eos,
            Reply::ok(eos, post(title, page, "Rivers move sediment in bursts.")),
        )
        .reply(page, Reply::ok(page, fixture("page-eos-article.html")))
        .reply(
            &search,
            Reply::ok(&search, fixture("crossref-search-eos.json")),
        );
    feeds.fetch(&http, &Access::default()).unwrap();
    assert_eq!(
        http.requests().len(),
        1,
        "nothing in the text, no page unless asked"
    );
    let id = feeds.items()[0].id.clone();
    let found = feeds.find_papers(&id, &http, &Access::default()).unwrap();
    assert!(found.links.is_empty(), "{found:?}");
    assert_eq!(found.page, None);
    let urls: Vec<String> = http.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(urls[1..], [page, search.as_str()]);
    assert_eq!(
        feeds.items().len(),
        1,
        "the book chapters on sediment are not it"
    );
}

#[test]
fn a_blog_post_titled_as_its_paper_is_found_by_its_title() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let blog = "https://example.org/blog.rss";
    let sub = feeds.subscribe(rss(blog), SourceKind::Blog).unwrap();
    let page = "https://example.org/blog/snow-creep";
    let title = "Temperature sensitivity of snow viscoplasticity: evidence from controlled creep experiments";
    let search = format!(
        "{WORKS}?query.bibliographic=Temperature%20sensitivity%20of%20snow%20viscoplasticity:\
         %20evidence%20from%20controlled%20creep%20experiments&rows=5&{SELECT}"
    );
    let http = Recorded::new()
        .reply(
            blog,
            Reply::ok(blog, post(title, page, "Our new paper is out.")),
        )
        .reply(page, Reply::status(page, 404))
        .reply(
            &search,
            Reply::ok(&search, fixture("crossref-tc-works.json")),
        );
    feeds.fetch(&http, &Access::default()).unwrap();
    let id = feeds.items()[0].id.clone();
    let found = feeds.find_papers(&id, &http, &Access::default()).unwrap();
    assert_eq!(
        found.links,
        [linked(
            "10.5194/tc-20-5813-2026",
            Confidence::High,
            Via::Search
        )]
    );
    assert_eq!(
        found.page.as_deref(),
        Some(format!("{page}: HTTP 404").as_str())
    );
    assert_eq!(
        http.requests().len(),
        3,
        "the search's work, not looked up again"
    );
    let paper = &feeds.items()[1];
    assert_eq!(paper.doi.as_deref(), Some("10.5194/tc-20-5813-2026"));
    assert_eq!(paper.sources[0].entry.source, sub.id);
    assert!(
        paper
            .summary
            .as_deref()
            .unwrap()
            .starts_with("Snow is a warm")
    );
    assert_eq!(feeds.items()[0].links, found.links, "kept on the post");
    let reopened = Feeds::open(dir.path()).unwrap();
    assert_eq!(reopened.items(), feeds.items());
}

#[test]
fn a_posts_text_as_paragraphs() {
    let html = "<p>Ice <em>sheets</em> melt.</p><ul><li>One</li><li>Two<br>lines</li></ul>\
                <script>var x;</script><div>Last\n   words</div> tail";
    assert_eq!(
        paragraphs(html),
        [
            "Ice sheets melt.",
            "One",
            "Two",
            "lines",
            "Last words",
            "tail"
        ]
    );
    assert_eq!(paragraphs("plain text"), ["plain text"]);
    assert!(paragraphs("  ").is_empty());
}
