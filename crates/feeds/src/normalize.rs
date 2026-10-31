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

/// The review comments, replies and supplements of Copernicus's papers and
/// discussions: `-RC1`, `-AC2`, `-CC1`, `-EC1`, `-RC1-supplement`, `-supplement`.
static ATTACHMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^10\.5194/.+-(?:(?:rc|ac|cc|ec)\d+(?:-supplement)?|supplement)$").unwrap()
});

/// The DOIs of news and magazine articles: Eos (`10.1029/2026EO260345`), The
/// Conversation (`10.64628/…`), Nature's news (`10.1038/d41586-…`).
static NEWS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:10\.1029/\d{4}eo\d+$|10\.64628/|10\.1038/d41586-)").unwrap());

/// Whether a canonical DOI is of a paper's review comment, reply or
/// supplement, not of a paper.
pub fn is_attachment_doi(doi: &str) -> bool {
    ATTACHMENT.is_match(doi)
}

/// Whether a canonical DOI is of a news article, not of a paper.
pub fn is_news_doi(doi: &str) -> bool {
    NEWS.is_match(doi)
}

/// Every DOI in a text, canonical, in order.
pub fn dois_in(text: &str) -> Vec<String> {
    DOI.find_iter(text)
        .filter_map(|m| canonical_doi(m.as_str()))
        .collect()
}

/// An ISSN as `NNNN-NNNC` (C a digit or X), if its check digit is right.
pub fn canonical_issn(text: &str) -> Option<String> {
    let chars: Vec<char> = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let [digits @ .., check] = chars.as_slice() else {
        return None;
    };
    if digits.len() != 7 || !digits.iter().all(char::is_ascii_digit) {
        return None;
    }
    let sum: u32 = (2..=8)
        .rev()
        .zip(digits)
        .map(|(weight, d)| weight * d.to_digit(10).unwrap_or(0))
        .sum();
    let expected = match (11 - sum % 11) % 11 {
        10 => 'X',
        n => char::from_digit(n, 10)?,
    };
    (*check == expected).then(|| {
        let digits: String = digits.iter().collect();
        format!("{}-{}{check}", &digits[..4], &digits[4..])
    })
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

/// The family name of an author, folded: before the comma of "Family, Given",
/// else the last word of "Given Family".
pub fn family(author: &str) -> Option<String> {
    let name = match author.split_once(',') {
        Some((family, _)) => family.to_string(),
        None => author.split_whitespace().last()?.to_string(),
    };
    let name = fold(&name);
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
