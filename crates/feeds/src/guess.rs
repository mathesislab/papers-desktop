//! The kind of a feed subscribed to by its address, guessed from what it
//! lists: a preprint server's (its host, or items that are preprints), a
//! journal's (items with DOIs, or its title a journal of OpenAlex), else
//! general (blogs, comics, anything else; news is not told from a blog).

use serde::Serialize;

use crate::http::{Access, FEED_ACCEPT, Http, get_ok};
use crate::journals::{best_named, sources};
use crate::model::{Feed, ItemKind, SourceKind, Subscription};
use crate::parse::parse;

/// The hosts of preprint servers, and their subdomains.
const PREPRINT_HOSTS: &[&str] = &[
    "arxiv.org",
    "biorxiv.org",
    "medrxiv.org",
    "chemrxiv.org",
    "eartharxiv.org",
    "essopenarchive.org",
    "egusphere.copernicus.org",
    "preprints.org",
    "researchsquare.com",
    "ssrn.com",
];

/// A kind guessed, and what tells it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Guess {
    pub kind: SourceKind,
    /// "its items have DOIs"
    pub reason: String,
}

/// Whether an address is of a preprint server (its host, or a subdomain).
pub fn preprint_host(url: &str) -> bool {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_lowercase))
        .unwrap_or_default();
    (PREPRINT_HOSTS.iter()).any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

/// The kind of the feed at an address: read (as a subscription fetches it),
/// its items' identifiers counted; its title looked for among OpenAlex's
/// journals if they have none. An error if the feed cannot be read.
pub fn guess_kind(http: &dyn Http, access: &Access, url: &str) -> Result<Guess, String> {
    let guess = |kind, reason: &str| {
        Ok(Guess {
            kind,
            reason: reason.to_string(),
        })
    };
    if preprint_host(url) {
        return guess(SourceKind::Preprints, "a preprint server");
    }
    let reply = get_ok(http, url, FEED_ACCEPT)?;
    let feed = Feed::Rss {
        url: url.to_string(),
    };
    let probe = Subscription::new(feed, SourceKind::Journal);
    let parsed = parse(&reply.body, Some(&reply.url), &probe).map_err(|e| e.to_string())?;
    let n = parsed.entries.len();
    let count =
        |f: &dyn Fn(&crate::model::Entry) -> bool| parsed.entries.iter().filter(|e| f(e)).count();
    if n > 0 && 2 * count(&|e| e.kind == ItemKind::Preprint) >= n {
        return guess(SourceKind::Preprints, "its items are preprints");
    }
    if n > 0 && 2 * count(&|e| e.doi.is_some()) >= n {
        return guess(SourceKind::Journal, "its items have DOIs");
    }
    if let Some(title) = &parsed.title {
        let journals = sources(http, access, title).unwrap_or_default();
        if best_named(&journals, title).is_some() {
            return guess(SourceKind::Journal, "a journal in OpenAlex");
        }
    }
    guess(
        SourceKind::Blog,
        "no DOIs in its items, and not a journal in OpenAlex",
    )
}
