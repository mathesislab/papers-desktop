//! A feed (RSS 0.9x, 1.0, 2.0, Atom, JSON Feed) as entries: feed-rs, the
//! elements it leaves out, and paper-feeder's fixes: the DOI from the guid,
//! `dc:identifier`, `prism:doi` or a link; Copernicus authors and abstract from
//! the description; Wiley's and arXiv's authors split; HTML as plain text;
//! editorials flagged.

use std::sync::LazyLock;

use feed_rs::model::{self, Text};
use regex::Regex;

use crate::extras::{Extra, Extras};
use crate::model::{Entry, ItemKind, SourceKind, Subscription};
use crate::normalize::{TAG, arxiv_id, canonical_doi, is_preprint_doi};
use crate::{Error, Result};

/// Separates the authors of a byline: "A, B, and C", "A and B", one per line.
static AUTHOR_SEP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r",\s+and\s+|\s+and\s+|,\s+|\n").unwrap());

static BR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());

/// The label before an abstract: arXiv's announcement, Wiley's heading.
static LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:arXiv:\S+\s+Announce Type:\s*\S+\s*)?(?:Abstract\b[:.]?\s*)?").unwrap()
});

/// A first paragraph that only cites the paper (Nature's "Published online; doi:…").
static CITATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)^\s*<p>(.*?)</p>").unwrap());

pub(crate) static EDITORIAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:news (?:&|and) views|perspective|editorial|comment|correspondence|opinion)s?\b",
    )
    .unwrap()
});

/// A feed as parsed.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    /// the feed's title, plain text
    pub title: Option<String>,
    pub entries: Vec<Entry>,
}

/// The entries of a feed's body, fetched from `base` (relative links), for a
/// subscription.
pub fn parse(body: &[u8], base: Option<&str>, subscription: &Subscription) -> Result<Parsed> {
    let failed = |message: String| Error::Feed {
        url: subscription.feed.to_string(),
        message,
    };
    let feed = feed_rs::parser::Builder::new()
        .base_uri(base)
        .build()
        .parse(body)
        .map_err(|e| failed(e.to_string()))?;
    // a JSON Feed has none of these elements
    let extras = match body.trim_ascii_start().first() {
        Some(b'<') => Extras::read(body).map_err(failed)?,
        _ => Extras::default(),
    };
    let title = feed.title.as_ref().map(text).filter(|t| !t.is_empty());
    let none = Extra::default();
    let entries = feed
        .entries
        .into_iter()
        .map(|e| {
            let links = e.links.iter().map(|l| l.href.as_str());
            let extra = extras.of(&e.id, links).unwrap_or(&none);
            entry(e, extra, subscription, title.as_deref())
        })
        .collect();
    Ok(Parsed { title, entries })
}

/// An entry of feed-rs, with its extras, for a subscription.
fn entry(e: model::Entry, x: &Extra, sub: &Subscription, feed_title: Option<&str>) -> Entry {
    let links: Vec<&str> = e
        .links
        .iter()
        .filter(|l| l.target.is_none() && l.rel.as_deref().is_none_or(|r| r == "alternate"))
        .map(|l| l.href.as_str())
        .collect();
    let doi = std::iter::once(e.id.as_str())
        .chain(x.identifiers.iter().map(String::as_str))
        .chain(x.dois.iter().map(String::as_str))
        .chain(links.iter().copied())
        .find_map(canonical_doi);
    let arxiv = std::iter::once(e.id.as_str())
        .chain(links.iter().copied())
        .find_map(arxiv_id);

    let description = e.summary.as_ref().map(|t| t.content.as_str());
    let mut authors = split_authors(e.authors.iter().filter_map(|p| p.name.as_deref()));
    if authors.is_empty() {
        authors = split_authors(x.creators.iter().map(String::as_str));
    }
    let mut summary = None;
    if authors.is_empty()
        && let Some((byline, abstract_)) = description.and_then(copernicus_byline)
    {
        authors = byline;
        summary = abstract_;
    }
    let content = e.content.as_ref().and_then(|c| c.body.clone());
    let summary = summary.or_else(|| {
        let texts = [description, x.description.as_deref()];
        let found = |t: &str| Some(abstract_text(t)).filter(|s| !s.is_empty());
        let longest = texts
            .into_iter()
            .flatten()
            .filter_map(found)
            .max_by_key(String::len);
        longest.or_else(|| content.as_deref().and_then(found))
    });

    let title = e.title.as_ref().map(text).unwrap_or_default();
    let categories = e
        .categories
        .iter()
        .map(|c| c.label.as_deref().unwrap_or(&c.term));
    let editorial = std::iter::once(title.as_str())
        .chain(categories)
        .any(|t| EDITORIAL.is_match(t));
    let journal = sub
        .journal
        .clone()
        .or_else(|| x.publication.clone())
        .or_else(|| feed_title.map(String::from));
    let kind = match (sub.kind, &doi, &arxiv) {
        (SourceKind::Journal, Some(doi), _) if is_preprint_doi(doi) => ItemKind::Preprint,
        (SourceKind::Journal, None, Some(_)) => ItemKind::Preprint,
        (kind, _, _) => kind.item_kind(),
    };
    let post = matches!(kind, ItemKind::News | ItemKind::Blog);
    Entry {
        source: sub.id.clone(),
        guid: e.id,
        title,
        authors,
        journal,
        date: e.published.or(e.updated),
        doi,
        arxiv,
        url: links.first().map(|l| l.to_string()),
        summary,
        content: content.filter(|_| post),
        editorial,
        kind,
        versions: Vec::new(),
    }
}

/// The text of a feed-rs text, plain.
fn text(t: &Text) -> String {
    plain(&t.content)
}

/// HTML as plain text: the entities decoded first (escaped tags show), then the
/// tags out, the white space collapsed.
pub(crate) fn plain(html: &str) -> String {
    let decoded = html_escape::decode_html_entities(html);
    let text = TAG.replace_all(&decoded, " ");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An abstract or teaser as plain text, without a citation paragraph before it
/// (Nature) or a label (arXiv's announcement, "Abstract", Copernicus's
/// "Abstract."); JATS's tags go as HTML's.
pub(crate) fn abstract_text(html: &str) -> String {
    let decoded = html_escape::decode_html_entities(html);
    let body = match CITATION.captures(&decoded) {
        Some(c) if canonical_doi(&c[1]).is_some() => &decoded[c.get(0).unwrap().end()..],
        _ => &decoded,
    };
    let text = plain(body);
    LABEL.replace(&text, "").into_owned()
}

/// The authors of names, one or several per name (Wiley's blob, arXiv's list).
fn split_authors<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    names
        .into_iter()
        .flat_map(|name| AUTHOR_SEP.split(name))
        .map(|part| part.split_whitespace().collect::<Vec<_>>().join(" "))
        .map(|name| name.trim_matches(',').trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

/// The authors and abstract of Copernicus's description, whose lines are
/// `<b>title</b><br>authors<br>citation<br>abstract`; None for another layout
/// (the citation line, with its doi.org link, says it is this one).
fn copernicus_byline(description: &str) -> Option<(Vec<String>, Option<String>)> {
    let decoded = html_escape::decode_html_entities(description);
    let lines: Vec<String> = BR.split(&decoded).map(plain).collect();
    if lines.len() < 4 || !lines[2].contains("doi.org/") {
        return None;
    }
    let authors = split_authors([lines[1].as_str()]);
    let abstract_ = plain(&lines[3..].join(" "));
    Some((authors, (!abstract_.is_empty()).then_some(abstract_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiley_blob_is_split() {
        let blob = "Cong Li, \nSusan L. Beck, \nJonathan R. Delph, \nAnne Meltzer";
        let authors = split_authors([blob]);
        assert_eq!(
            authors,
            [
                "Cong Li",
                "Susan L. Beck",
                "Jonathan R. Delph",
                "Anne Meltzer"
            ]
        );
        assert_eq!(split_authors(["Andrew Mitchinson"]), ["Andrew Mitchinson"]);
        assert!(split_authors([""]).is_empty());
    }

    const COPERNICUS: &str = "<b>Temperature-driven shrinkage of a Himalayan glacier</b><br />\n\
        Koji Fujita and Rijan B. Kayastha<br />\n\
        The Cryosphere, 20, 4005–4015, https://doi.org/10.5194/tc-20-4005-2026, 2026<br />\n\
        We measure the glacier and find it is shrinking fast.";

    #[test]
    fn copernicus_byline_gives_authors_and_abstract() {
        let (authors, abstract_) = copernicus_byline(COPERNICUS).unwrap();
        assert_eq!(authors, ["Koji Fujita", "Rijan B. Kayastha"]);
        assert_eq!(
            abstract_.as_deref(),
            Some("We measure the glacier and find it is shrinking fast.")
        );
        assert_eq!(copernicus_byline("Just a normal abstract."), None);
    }

    #[test]
    fn plain_keeps_a_less_than() {
        assert_eq!(
            plain("<p>W &lt; 0 and x<0 &amp; y</p>"),
            "W < 0 and x<0 & y"
        );
        assert_eq!(plain("&lt;b&gt;Bold&lt;/b&gt; text"), "Bold text");
    }

    #[test]
    fn abstract_text_drops_labels_and_citation() {
        let arxiv = "arXiv:2610.38522v1 Announce Type: new \nAbstract: Atmospheric power is…";
        assert_eq!(abstract_text(arxiv), "Atmospheric power is…");
        assert_eq!(
            abstract_text("\nAbstract\nWe utilized data."),
            "We utilized data."
        );
        let nature = "<p>Nature Geoscience, Published online: 28 October 2026; \
            <a href=\"https://www.nature.com/articles/x\">doi:10.1038/s41561-026-02107-w</a></p>\
            The influence of dryness.";
        assert_eq!(abstract_text(nature), "The influence of dryness.");
        assert_eq!(
            abstract_text("<p>First.</p><p>Second.</p>"),
            "First. Second."
        );
    }

    #[test]
    fn editorials() {
        for title in [
            "News & Views: ice",
            "A perspective on melt",
            "Editorial",
            "Comment",
        ] {
            assert!(EDITORIAL.is_match(title), "{title}");
        }
        for title in ["Recommended practice", "Commentless"] {
            assert!(!EDITORIAL.is_match(title), "{title}");
        }
    }
}
