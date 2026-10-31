//! The paper behind a post. In an HTML text (the post in its feed, or its
//! page), labelled blocks first: Futurity's "Original Study", ScienceDaily's
//! "Journal Reference", phys.org's "More information" and `a[data-doi]`, Eos's
//! closing "(Journal, https://doi.org/…, year)". Else the DOIs and article
//! links of the first paragraphs of the body (not reference lists, footnotes,
//! asides, related links, sidebars or comments), ranked by how often, then how
//! early, they come: one that wins is sure, else up to three are candidates.
//! Never the page's own DOI (`citation_doi`, `dc.identifier`) or a news
//! article's. Else the post's title in Crossref, kept if the titles are the
//! same words.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use scraper::{ElementRef, Html, Node, Selector};

use crate::crossref::{self, Message, Work};
use crate::enrich::Budget;
use crate::feeds::Feeds;
use crate::http::{Access, Http, PAGE_ACCEPT, Request, failure, get_json};
use crate::merge::SAME_TITLE;
use crate::model::{Confidence, Item, ItemKind, Linked, Via};
use crate::normalize::{
    arxiv_id, canonical_doi, dois_in, is_attachment_doi, is_news_doi, jaccard, title_words,
};
use crate::{Error, Result};

/// The paragraphs of a body read for its papers.
const FIRST_PARAGRAPHS: usize = 15;

/// The most papers of a post.
const MOST_LINKS: usize = 3;

/// A paper cited by a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cited {
    Doi(String),
    /// a ScienceDirect article, whose DOI Crossref knows
    Pii(String),
}

fn selector(css: &str) -> Selector {
    Selector::parse(css).expect("a valid selector")
}

static META: LazyLock<Selector> = LazyLock::new(|| selector("meta[name][content]"));

/// The blocks that may be labelled.
static BLOCKS: LazyLock<Selector> = LazyLock::new(|| selector("p, li, dd"));

static LINKS: LazyLock<Selector> = LazyLock::new(|| selector("a[href]"));

/// phys.org's link of its "More information".
static DATA_DOI: LazyLock<Selector> = LazyLock::new(|| selector("a[data-doi][href]"));

/// The body of a page, the first found.
static ROOTS: LazyLock<Vec<Selector>> = LazyLock::new(|| {
    [
        "[itemprop=articleBody]",
        ".entry-content",
        ".article-content",
        "#story_text",
        ".article-main",
        "article",
        "main",
    ]
    .map(selector)
    .to_vec()
});

/// The paragraphs of a body, and the headings that may end it.
static TEXT: LazyLock<Selector> = LazyLock::new(|| selector("p, li, h1, h2, h3, h4, h5, h6"));

/// What is not the body: asides, navigation, footers, figures, related links,
/// sidebars, comments, references, footnotes, sharing, newsletters.
static NOT_BODY: LazyLock<Selector> = LazyLock::new(|| {
    selector(
        "aside, nav, footer, header, form, figure, \
         [class*=related], [id*=related], [class*=sidebar], [id*=sidebar], \
         [class*=comment], [id*=comment], [class*=reference], [id*=reference], \
         [class*=footnote], [id*=footnote], [class*=bibliograph], [class*=share], \
         [class*=newsletter], [class*=recommend]",
    )
});

/// The label of a block that names the paper.
static LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(?:original study(?:\s+doi)?|journal references?|more information)\b")
        .unwrap()
});

/// Eos's closing citation of a paragraph: "(Journal, https://doi.org/…, 2026)".
static CLOSING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\(\s*[^()]+?,\s*(https?://(?:dx\.)?doi\.org/10\.\d{4,9}/[^\s,()]+)\s*,\s*\d{4}\s*\)\s*$",
    )
    .unwrap()
});

/// A heading after which come the references, not the body.
static END: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(?:references|sources|bibliography|further reading|literature cited|notes|footnotes)\b")
        .unwrap()
});

static PII: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)sciencedirect\.com/science/article/(?:abs/)?pii/([0-9a-z]{16,17})").unwrap()
});

/// A Copernicus paper (`tc.copernicus.org/articles/20/5895/2026/`) or discussion.
static COPERNICUS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^https?://([a-z0-9-]+)\.copernicus\.org/(?:articles/(\d+)/(\d+)/(\d{4})|preprints/(?:\d+/)?([a-z]+-\d{4}-\d+))/?(?:$|[?#])")
        .unwrap()
});

static NATURE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^https?://(?:www\.)?nature\.com/articles/([a-z0-9.-]+)/?(?:$|[?#])").unwrap()
});

/// The page of a DOI after it in a publisher's link: `/full`, `/pdf`, ….
static SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"/(?:full|abstract|abs|pdf|epdf|meta|html|fulltext|summary)/?$").unwrap()
});

/// The paper of a link: a DOI's, a publisher's article (Nature, Copernicus,
/// Wiley, Science, PNAS, IOP, Springer, …), an arXiv abstract, a
/// ScienceDirect PII.
fn cited_by_link(href: &str) -> Option<Cited> {
    let href = href.trim();
    if !href.starts_with("http://") && !href.starts_with("https://") {
        return None;
    }
    let host = url::Url::parse(href).ok()?.host_str()?.to_lowercase();
    if host == "arxiv.org" || host.ends_with(".arxiv.org") {
        return arxiv_id(href).map(|id| Cited::Doi(format!("10.48550/arxiv.{id}")));
    }
    if let Some(c) = PII.captures(href) {
        return Some(Cited::Pii(c[1].to_uppercase()));
    }
    if let Some(c) = COPERNICUS.captures(href) {
        let journal = c[1].to_lowercase();
        let doi = match c.get(5) {
            Some(preprint) => format!("10.5194/{}", preprint.as_str().to_lowercase()),
            None => format!("10.5194/{journal}-{}-{}-{}", &c[2], &c[3], &c[4]),
        };
        return Some(Cited::Doi(doi));
    }
    if let Some(c) = NATURE.captures(href) {
        return Some(Cited::Doi(format!("10.1038/{}", c[1].to_lowercase())));
    }
    let doi = canonical_doi(href)?;
    Some(Cited::Doi(SUFFIX.replace(&doi, "").into_owned()))
}

/// The text of an element, its white space collapsed.
fn text_of(element: ElementRef) -> String {
    let text: String = element.text().collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The elements that end a paragraph of text.
const BLOCK_NAMES: &[&str] = &[
    "p",
    "div",
    "li",
    "br",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "ul",
    "ol",
    "table",
    "tr",
    "figure",
    "figcaption",
    "section",
    "article",
    "pre",
    "dd",
    "dt",
    "hr",
];

/// The text of an HTML text (a post's content) as plain paragraphs: a block
/// (`p`, `li`, `h2`, `br`, ...) ends one, white space is collapsed, scripts
/// and styles are left out.
pub fn paragraphs(html: &str) -> Vec<String> {
    fn flush(out: &mut Vec<String>, buffer: &mut String) {
        let text = buffer.split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.is_empty() {
            out.push(text);
        }
        buffer.clear();
    }
    fn walk(element: ElementRef, out: &mut Vec<String>, buffer: &mut String) {
        for child in element.children() {
            match (ElementRef::wrap(child), child.value()) {
                (Some(e), _) if matches!(e.value().name(), "script" | "style") => {}
                (Some(e), _) if BLOCK_NAMES.contains(&e.value().name()) => {
                    flush(out, buffer);
                    walk(e, out, buffer);
                    flush(out, buffer);
                }
                (Some(e), _) => walk(e, out, buffer),
                (None, Node::Text(text)) => buffer.push_str(text),
                _ => {}
            }
        }
    }
    let fragment = Html::parse_fragment(html);
    let (mut out, mut buffer) = (Vec::new(), String::new());
    walk(fragment.root_element(), &mut out, &mut buffer);
    flush(&mut out, &mut buffer);
    out
}

/// The papers an element cites, in order: its links' (or a link's text, if
/// its address is not a paper's), and the DOIs of its text.
fn cited_in(element: ElementRef) -> Vec<Cited> {
    let mut found = Vec::new();
    for child in element.children() {
        match (ElementRef::wrap(child), child.value()) {
            (Some(link), _) if link.value().name() == "a" => {
                match link.value().attr("href").and_then(cited_by_link) {
                    Some(cited) => found.push(cited),
                    None => found.extend(dois_in(&text_of(link)).into_iter().map(Cited::Doi)),
                }
            }
            (Some(inner), _) => found.extend(cited_in(inner)),
            (None, Node::Text(text)) => found.extend(dois_in(text).into_iter().map(Cited::Doi)),
            _ => {}
        }
    }
    found
}

/// The DOIs a page gives as its own: `citation_doi`, `dc.identifier`, `prism.doi`.
fn own_dois(document: &Html) -> HashSet<String> {
    let names = [
        "citation_doi",
        "dc.identifier",
        "dc.identifier.doi",
        "prism.doi",
    ];
    document
        .select(&META)
        .filter(|m| {
            let name = m.value().attr("name").unwrap_or_default().to_lowercase();
            names.contains(&name.as_str())
        })
        .filter_map(|m| m.value().attr("content").and_then(canonical_doi))
        .collect()
}

/// The papers of the labelled blocks, in order: a block that starts with a
/// label ("Original Study DOI: …", "More information: …"), the block after a
/// label alone ("Journal Reference:"), a paragraph's closing citation, a
/// link with a DOI attribute or a label for text.
fn labelled(document: &Html) -> Vec<Cited> {
    let mut found = Vec::new();
    for block in document.select(&BLOCKS) {
        let text = text_of(block);
        if let Some(label) = LABEL.find(&text) {
            let rest = text[label.end()..].trim_matches(|c: char| c.is_whitespace() || c == ':');
            match rest.is_empty() {
                true => {
                    let next = block.next_siblings().find_map(ElementRef::wrap);
                    found.extend(next.map(cited_in).unwrap_or_default());
                }
                false => found.extend(cited_in(block)),
            }
        } else if let Some(closing) = CLOSING.captures(&text) {
            found.extend(canonical_doi(&closing[1]).map(Cited::Doi));
        }
    }
    let links = document
        .select(&LINKS)
        .filter(|a| LABEL.is_match(&text_of(*a)));
    for link in document.select(&DATA_DOI).chain(links) {
        found.extend(link.value().attr("href").and_then(cited_by_link));
    }
    found
}

/// Whether an element is in what is not the body, below the body's root.
fn outside_body(element: ElementRef, root: ElementRef) -> bool {
    std::iter::once(element)
        .chain(element.ancestors().filter_map(ElementRef::wrap))
        .take_while(|e| e.id() != root.id())
        .any(|e| NOT_BODY.matches(&e))
}

/// Whether an element is within a paragraph or an item of a list, below the
/// body's root (read with it).
fn within_block(element: ElementRef, root: ElementRef) -> bool {
    element
        .ancestors()
        .filter_map(ElementRef::wrap)
        .take_while(|e| e.id() != root.id())
        .any(|e| matches!(e.value().name(), "p" | "li"))
}

/// The papers of the first paragraphs of the body, every mention in order;
/// the whole text if it has no paragraphs.
fn body(document: &Html) -> Vec<Cited> {
    let root = ROOTS
        .iter()
        .find_map(|s| document.select(s).next())
        .unwrap_or_else(|| document.root_element());
    let mut found = Vec::new();
    let mut read = 0;
    for element in root.select(&TEXT) {
        if read == FIRST_PARAGRAPHS {
            break;
        }
        if outside_body(element, root) || within_block(element, root) {
            continue;
        }
        let text = text_of(element);
        if element.value().name().starts_with('h') {
            if END.is_match(&text) {
                break;
            }
            continue;
        }
        if !text.is_empty() {
            read += 1;
            found.extend(cited_in(element));
        }
    }
    if read == 0 {
        found.extend(cited_in(root));
    }
    found
}

/// The papers cited, each once, with how often.
fn counted(cited: impl IntoIterator<Item = Cited>) -> Vec<(Cited, usize)> {
    let mut counted: Vec<(Cited, usize)> = Vec::new();
    for c in cited {
        match counted.iter_mut().find(|(known, _)| *known == c) {
            Some((_, n)) => *n += 1,
            None => counted.push((c, 1)),
        }
    }
    counted
}

/// The papers an HTML text cites, best first: those of its labelled blocks
/// (high confidence); else the most mentioned of the first paragraphs if it
/// wins (high), else the first three (low). A page (`document`) has its own
/// DOIs, left out with those of news articles.
pub(crate) fn cited(html: &str, document: bool) -> Vec<(Cited, Confidence)> {
    let parsed = match document {
        true => Html::parse_document(html),
        false => Html::parse_fragment(html),
    };
    let own = own_dois(&parsed);
    let keep = |c: &Cited| match c {
        Cited::Doi(doi) => !own.contains(doi) && !is_news_doi(doi) && !is_attachment_doi(doi),
        Cited::Pii(_) => true,
    };
    let labelled = counted(labelled(&parsed).into_iter().filter(keep));
    if !labelled.is_empty() {
        let labelled = labelled.into_iter().take(MOST_LINKS);
        return labelled.map(|(c, _)| (c, Confidence::High)).collect();
    }
    let mut ranked = counted(body(&parsed).into_iter().filter(keep));
    // stable: the earlier first among equals
    ranked.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    match ranked.as_slice() {
        [] => Vec::new(),
        [(one, _)] => vec![(one.clone(), Confidence::High)],
        [(first, n), (_, m), ..] if n > m => vec![(first.clone(), Confidence::High)],
        _ => ranked
            .into_iter()
            .take(MOST_LINKS)
            .map(|(c, _)| (c, Confidence::Low))
            .collect(),
    }
}

/// Whether links have a sure one.
pub(crate) fn sure(links: &[Linked]) -> bool {
    links.iter().any(|l| l.confidence == Confidence::High)
}

/// How far to look for the papers of a post.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reach {
    /// its text in its feed
    Feed,
    /// its page and its title in Crossref too
    Web,
}

/// What looking for the papers behind a post found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// the papers, best first, as the post now has them
    pub links: Vec<Linked>,
    /// why the post's page could not be read, if it could not
    pub page: Option<String>,
}

/// The text of a page, or why not.
fn read_page(http: &dyn Http, url: &str) -> std::result::Result<String, String> {
    let request = Request {
        url: url.to_string(),
        accept: Some(PAGE_ACCEPT.to_string()),
        ..Request::default()
    };
    let reply = http.get(&request)?;
    match reply.status {
        200..=299 => Ok(String::from_utf8_lossy(&reply.body).into_owned()),
        _ => Err(format!("{url}: {}", failure(&reply))),
    }
}

impl Feeds {
    /// The papers behind a post, looked for again: in its text in its feed,
    /// else on its page (unless the feed's text named one surely), else its
    /// title in Crossref. They are kept on the post, and each that is not an
    /// item yet becomes a paper of the post's source, from Crossref.
    pub fn find_papers(&mut self, id: &str, http: &dyn Http, access: &Access) -> Result<Found> {
        let item = self
            .item(id)
            .ok_or_else(|| Error::NoItem { id: id.to_string() })?;
        if !item.kind.is_post() {
            return Err(Error::NotAPost { id: id.to_string() });
        }
        let id = item.id.clone();
        let mut budget = Budget::new(crate::LOOKUPS_PER_FETCH, 1);
        let found = self.find(&id, Reach::Web, http, access, &mut budget);
        self.papers_of(&[id], http, access, &mut budget);
        self.save()?;
        Ok(found)
    }

    /// The posts about a paper (linking any DOI of its versions), by an id or
    /// an alias.
    pub fn covered_by(&self, paper: &str) -> Vec<&Item> {
        let Some(paper) = self.item(paper) else {
            return Vec::new();
        };
        let dois: Vec<&str> = paper.versions.iter().map(|v| v.doi.as_str()).collect();
        self.items()
            .iter()
            .filter(|i| i.id != paper.id && i.links.iter().any(|l| dois.contains(&l.doi.as_str())))
            .collect()
    }

    /// Looks for the papers behind a post by its id: in its feed's text; with
    /// the web's reach (and the budget for it), on its page unless the feed's
    /// text named one surely, then its title in Crossref if neither named
    /// any. The links are set on the post; the works of a search or a PII are
    /// merged as its papers.
    pub(crate) fn find(
        &mut self,
        id: &str,
        reach: Reach,
        http: &dyn Http,
        access: &Access,
        budget: &mut Budget,
    ) -> Found {
        let Some(item) = self.item(id) else {
            return Found::default();
        };
        let text = item.content.clone().or_else(|| item.summary.clone());
        let (url, title) = (item.url.clone(), item.title.clone());
        let source = item.sources[0].entry.source.clone();
        let mut works = Vec::new();
        let feed = text.map_or_else(Vec::new, |t| cited(&t, false));
        let mut found = Found {
            links: self.links_of(feed, Via::Feed, http, access, budget, &mut works),
            page: None,
        };
        if reach == Reach::Web && !sure(&found.links) && budget.page() {
            self.lookups.insert(format!("page:{id}"), (self.now)());
            match url.map(|url| read_page(http, &url)) {
                Some(Ok(html)) => {
                    let page = cited(&html, true);
                    let page = self.links_of(page, Via::Page, http, access, budget, &mut works);
                    if sure(&page) || found.links.is_empty() {
                        found.links = page;
                    }
                }
                Some(Err(message)) => found.page = Some(message),
                None => found.page = Some("no page".to_string()),
            }
            if found.links.is_empty()
                && let Some((link, work)) = self.search(&title, http, access, budget)
            {
                found.links.push(link);
                works.push(work);
            }
        }
        let now = (self.now)();
        let papers: Vec<_> = works
            .iter()
            .filter_map(|w| w.entry(&source, ItemKind::Paper))
            .collect();
        for doi in papers.iter().filter_map(|e| e.doi.as_ref()) {
            self.lookups.insert(format!("crossref:{doi}"), now);
        }
        let id = self.resolve(id).to_string();
        if let Some(post) = self.items.items.iter_mut().find(|i| i.id == id) {
            post.links.clone_from(&found.links);
        }
        self.add(papers, false);
        found
    }

    /// Cited papers as links: a PII by its work in Crossref (kept in `works`),
    /// once; each DOI once.
    fn links_of(
        &mut self,
        cited: Vec<(Cited, Confidence)>,
        via: Via,
        http: &dyn Http,
        access: &Access,
        budget: &mut Budget,
        works: &mut Vec<Work>,
    ) -> Vec<Linked> {
        let mut links: Vec<Linked> = Vec::new();
        for (cited, confidence) in cited {
            let doi = match cited {
                Cited::Doi(doi) => doi,
                Cited::Pii(pii) => match self.by_pii(&pii, http, access, budget) {
                    Some(work) => match work.doi() {
                        Some(doi) => {
                            works.push(work);
                            doi
                        }
                        None => continue,
                    },
                    None => continue,
                },
            };
            if !links.iter().any(|l| l.doi == doi) {
                links.push(Linked {
                    doi,
                    confidence,
                    via,
                });
            }
        }
        links
    }

    /// The work of a ScienceDirect PII in Crossref, asked once.
    fn by_pii(
        &mut self,
        pii: &str,
        http: &dyn Http,
        access: &Access,
        budget: &mut Budget,
    ) -> Option<Work> {
        let key = format!("pii:{pii}");
        if self.lookups.contains_key(&key) || !budget.lookup() {
            return None;
        }
        match get_json::<Message<crossref::List>>(http, &crossref::pii_url(pii, access)) {
            Ok(list) => {
                self.lookups.insert(key, (self.now)());
                list.and_then(|l| l.message.items.into_iter().next())
            }
            Err(_) => {
                budget.stop();
                None
            }
        }
    }

    /// The work of a post's title in Crossref, if its title is the same words.
    fn search(
        &mut self,
        title: &str,
        http: &dyn Http,
        access: &Access,
        budget: &mut Budget,
    ) -> Option<(Linked, Work)> {
        if !budget.lookup() {
            return None;
        }
        let list =
            match get_json::<Message<crossref::List>>(http, &crossref::search_url(title, access)) {
                Ok(list) => list?.message,
                Err(_) => {
                    budget.stop();
                    return None;
                }
            };
        let words = title_words(title);
        let (similarity, work) = list
            .items
            .into_iter()
            .map(|w| (jaccard(&words, &title_words(&w.title())), w))
            .max_by(|a, b| a.0.total_cmp(&b.0))?;
        let doi = work.doi().filter(|_| similarity >= SAME_TITLE)?;
        let link = Linked {
            doi,
            confidence: Confidence::High,
            via: Via::Search,
        };
        Some((link, work))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(path).unwrap()
    }

    fn doi(d: &str) -> Cited {
        Cited::Doi(d.to_string())
    }

    fn high(d: &str) -> (Cited, Confidence) {
        (doi(d), Confidence::High)
    }

    fn low(d: &str) -> (Cited, Confidence) {
        (doi(d), Confidence::Low)
    }

    #[test]
    fn futurity_original_study() {
        let page = cited(&fixture("page-futurity.html"), true);
        assert_eq!(page, [high("10.1126/sciadv.aei1213")]);
    }

    #[test]
    fn sciencedaily_journal_reference() {
        let page = cited(&fixture("page-sciencedaily.html"), true);
        assert_eq!(page, [high("10.1126/sciadv.aeg2217")]);
    }

    #[test]
    fn physorg_more_information_over_the_body_and_related_stories() {
        let page = cited(&fixture("page-physorg.html"), true);
        assert_eq!(page, [high("10.1126/sciadv.adw0001")]);
    }

    #[test]
    fn eos_closing_citation_not_its_own_doi_nor_others_of_agu() {
        let page = cited(&fixture("page-eos-spotlight.html"), true);
        assert_eq!(page, [high("10.1029/2026ea005360")]);
    }

    #[test]
    fn eos_article_with_its_own_doi_only() {
        assert_eq!(cited(&fixture("page-eos-article.html"), true), []);
    }

    #[test]
    fn futurity_feed_text() {
        let feed = fixture("futurity.xml");
        let start = feed.find("<content:encoded><![CDATA[").unwrap();
        let end = feed[start..].find("]]></content:encoded>").unwrap();
        let content = &feed[start + 26..start + end];
        assert_eq!(cited(content, false), [high("10.1126/sciadv.aei1213")]);
    }

    #[test]
    fn body_without_labels_gives_candidates_in_order() {
        let page = fixture("page-physorg.html")
            .replace("<strong>More information:</strong> ", "")
            .replace("data-doi=\"1\" ", "");
        let pii = (Cited::Pii("0012821X9190220C".to_string()), Confidence::Low);
        assert_eq!(
            cited(&page, true),
            [
                low("10.1038/nature09043"),
                pii,
                low("10.1126/sciadv.adw0001")
            ],
            "one mention each, not the related stories' DOI"
        );
    }

    #[test]
    fn a_doi_mentioned_twice_wins() {
        let html = "<p>The study <a href=\"https://doi.org/10.1000/a\">in GRL</a> builds on \
            <a href=\"https://www.science.org/doi/10.1000/b\">older work</a>.</p>\
            <p>Its data (doi:10.1000/a) are open.</p>";
        assert_eq!(cited(html, false), [high("10.1000/a")]);
        let tie = "<p><a href=\"https://doi.org/10.1000/a\">One</a> and \
            <a href=\"https://pnas.org/doi/10.1000/b/full\">two</a>.</p>";
        assert_eq!(cited(tie, false), [low("10.1000/a"), low("10.1000/b")]);
    }

    #[test]
    fn reference_lists_footnotes_and_asides_are_not_the_body() {
        let html = "<html><body><article>\
            <p>Melt is up, a <a href=\"https://doi.org/10.1000/paper\">new study</a> finds.</p>\
            <aside><p><a href=\"https://doi.org/10.1000/aside\">Also</a></p></aside>\
            <div class=\"footnotes\"><p>1. doi:10.1000/note doi:10.1000/note</p></div>\
            <h2>References</h2>\
            <ol><li>doi:10.1000/ref1</li><li>doi:10.1000/ref1</li><li>doi:10.1000/ref2</li></ol>\
            </article></body></html>";
        assert_eq!(cited(html, true), [high("10.1000/paper")]);
    }

    #[test]
    fn the_pages_own_doi_and_news_dois_are_left_out() {
        // as The Conversation: its own DOI in citation_doi
        let html = "<html><head><meta name=\"citation_doi\" content=\"10.64628/AA.abc123\">\
            <meta name=\"DC.identifier\" content=\"doi:10.1000/self\"></head><body><article>\
            <p>Our <a href=\"https://doi.org/10.64628/aa.abc123\">article</a> and \
            <a href=\"https://doi.org/10.1000/self\">this</a>, an \
            <a href=\"https://doi.org/10.1029/2026EO260345\">Eos story</a>, and \
            <a href=\"https://www.nature.com/articles/d41586-026-01234-5\">Nature's news</a>, \
            about <a href=\"https://www.nature.com/articles/s41561-026-02107-w\">the paper</a>.</p>\
            </article></body></html>";
        assert_eq!(cited(html, true), [high("10.1038/s41561-026-02107-w")]);
    }

    #[test]
    fn publisher_links() {
        let cases = [
            (
                "https://tc.copernicus.org/articles/20/5895/2026/",
                "10.5194/tc-20-5895-2026",
            ),
            (
                "https://egusphere.copernicus.org/preprints/2025/egusphere-2025-6066/",
                "10.5194/egusphere-2025-6066",
            ),
            (
                "https://www.nature.com/articles/s41561-026-02107-w?utm=x",
                "10.1038/s41561-026-02107-w",
            ),
            (
                "https://agupubs.onlinelibrary.wiley.com/doi/full/10.1029/2026GL123955",
                "10.1029/2026gl123955",
            ),
            (
                "https://www.science.org/doi/10.1126/science.adx1234",
                "10.1126/science.adx1234",
            ),
            (
                "https://www.pnas.org/doi/10.1073/pnas.2500000123",
                "10.1073/pnas.2500000123",
            ),
            (
                "https://iopscience.iop.org/article/10.1088/1748-9326/ad1234/pdf",
                "10.1088/1748-9326/ad1234",
            ),
            (
                "https://link.springer.com/article/10.1007/s00382-026-01234-5",
                "10.1007/s00382-026-01234-5",
            ),
            (
                "https://arxiv.org/abs/2610.38522v2",
                "10.48550/arxiv.2610.38522",
            ),
        ];
        for (href, expected) in cases {
            assert_eq!(cited_by_link(href), Some(doi(expected)), "{href}");
        }
        assert_eq!(
            cited_by_link("https://www.sciencedirect.com/science/article/pii/S0012821X26001234"),
            Some(Cited::Pii("S0012821X26001234".to_string()))
        );
        for href in [
            "/news/2026-10-x.html",
            "https://phys.org/news/x.html",
            "mailto:a@b",
        ] {
            assert_eq!(cited_by_link(href), None, "{href}");
        }
    }
}
