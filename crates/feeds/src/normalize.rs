//! The keys that items are matched by: canonical DOIs, arXiv ids, title
//! words, first-author family names; and the canonical address of a feed.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use percent_encoding::percent_decode_str;
use regex::Regex;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;
use url::Url;

use crate::{Error, Result};

static DOI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"10\.\d{4,9}/[^\s"<>]+"#).unwrap());

static ARXIV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:arxiv\.org/(?:abs|pdf)/|oai:arxiv\.org:|arxiv[:.]\s*)([a-z][a-z.-]*/\d{7}|\d{4}\.\d{4,5})(?:v\d+)?",
    )
    .unwrap()
});

/// An HTML tag: `<` and a tag name, so that `x<0` in an abstract stays.
pub(crate) static TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"</?[A-Za-z][A-Za-z0-9:-]*(?:\s[^<>]*)?/?>|<!--[\s\S]*?-->").unwrap()
});

/// The prefixes of the DOIs of preprint servers and discussion papers.
const PREPRINT_DOIS: [&str; 9] = [
    "10.5194/egusphere-",
    "10.48550/",      // arXiv
    "10.31223/",      // EarthArXiv
    "10.1002/essoar", // ESS Open Archive
    "10.22541/essoar",
    "10.22541/au.", // Authorea
    "10.21203/rs.", // Research Square
    "10.2139/ssrn.",
    "10.1101/", // bioRxiv, medRxiv
];

/// The DOI in a text (a doi.org link, `doi:10.…`, a publisher's link, a bare
/// DOI), URL-decoded, without trailing punctuation, lowercase.
pub fn canonical_doi(text: &str) -> Option<String> {
    let text = text.trim();
    // the query and fragment of a link are not the DOI's (Wiley's `?af=R`)
    let text = if text.starts_with("http://") || text.starts_with("https://") {
        text.split(['?', '#']).next().unwrap_or(text)
    } else {
        text
    };
    let decoded = percent_decode_str(text).decode_utf8_lossy();
    let doi = DOI.find(&decoded)?.as_str();
    let doi = doi.trim_end_matches([')', '.', ',', ';', '"', '\'']);
    Some(doi.to_lowercase())
}

/// Whether a DOI is of a preprint (EGUsphere, arXiv, EarthArXiv, ESSOAr, …).
pub fn is_preprint_doi(doi: &str) -> bool {
    PREPRINT_DOIS.iter().any(|p| doi.starts_with(p))
}

/// The arXiv id in a text (`oai:arXiv.org:2610.38522v1`, an abs or pdf link,
/// `arXiv:…`, an arXiv DOI), without its version.
pub fn arxiv_id(text: &str) -> Option<String> {
    let found = ARXIV.captures(text)?;
    Some(found[1].to_lowercase())
}

/// A title as compared: tags out, letters decomposed (NFKD) without their
/// accents, lowercase, words of letters and digits separated by one space.
pub fn title_key(title: &str) -> String {
    fold(&TAG.replace_all(title, " "))
}

/// The words of a text folded as [`title_key`].
pub fn fold(text: &str) -> String {
    let folded: String = text
        .nfkd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The words of a title.
pub fn title_words(title: &str) -> BTreeSet<String> {
    title_key(title)
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// The Jaccard index of two sets of words: shared over all; 0 for two empty sets.
pub fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// The family name of an author, folded: see [`family_name`].
pub fn family(author: &str) -> Option<String> {
    let name = fold(family_name(author)?);
    (!name.is_empty()).then_some(name)
}

/// The family name of an author, as written: before the comma of "Family,
/// Given", else the last word of "Given Family".
pub fn family_name(author: &str) -> Option<&str> {
    let name = match author.split_once(',') {
        Some((family, _)) => family.trim(),
        None => author.split_whitespace().last()?,
    };
    (!name.is_empty()).then_some(name)
}

/// The canonical address of a feed: an http(s) URL, scheme and host
/// lowercased, no fragment.
pub fn canonical_url(text: &str) -> Result<String> {
    let invalid = |message: String| Error::Url {
        url: text.to_string(),
        message,
    };
    let mut url = Url::parse(text.trim()).map_err(|e| invalid(e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err(invalid("not an http(s) URL".to_string()));
    }
    url.set_fragment(None);
    Ok(url.to_string())
}

/// What makes two feed addresses the same feed: the canonical address
/// without its scheme (http and https are one).
pub fn feed_key(canonical: &str) -> &str {
    canonical
        .split_once("://")
        .map_or(canonical, |(_, rest)| rest)
}

/// A short stable id of a key: 12 hex digits of its FNV-1a hash.
pub fn short_id(key: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{:012x}", hash >> 16)
}
