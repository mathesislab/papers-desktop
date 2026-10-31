//! Feeds for papers-desktop: the new papers of the journals, preprint servers,
//! news sites and blogs followed.
//!
//! A [`Subscription`] follows a [`Feed`]: an RSS or Atom feed, fetched by a
//! conditional GET and parsed into [`Entry`]s (with paper-feeder's fixes:
//! Copernicus bylines, Wiley author blobs, the DOI from the guid,
//! `prism:doi` or the link), or a sweep of Crossref (a journal's ISSN) or
//! OpenAlex (a filter) for the works new since the last. Requests go through
//! an [`Http`] (paced per host), the APIs' with an [`Access`]. The entries of
//! all sources are merged into [`Item`]s, one per paper or post; the works of
//! DOIs are looked up in Crossref (abstracts, authors, a preprint and its
//! paper as one item), and the papers behind posts found ([`Linked`]).
//! [`Feeds`] keeps them in a folder of JSON files with their [`Triage`].
//! [`Interest`]s score the items, as paper-feeder does; [`Rule`]s triage new
//! items by a [`query`] (core's language over a `Record` of an item);
//! [`Feeds::view`] gives the items a query shows and the counts of the
//! sidebar. The subscriptions go out and come in as OPML; [`suggested`]
//! lists some to start with; [`pdf`] tells the free copy of a paper. Nothing
//! here knows of .bib files.

mod crossref;
mod enrich;
mod extras;
mod feeds;
pub mod http;
mod interest;
mod links;
mod merge;
mod model;
pub mod normalize;
mod openalex;
mod opml;
mod parse;
mod pdf;
pub mod query;
mod rules;
mod store;
mod suggested;
mod sweep;
mod view;

use std::path::PathBuf;

pub use enrich::{LOOKUPS_PER_FETCH, PAGES_PER_FETCH, TEASER};
pub use feeds::{FeedReport, Feeds, FetchReport, Now, Outcome, Patch};
pub use http::{Access, Http, Network, Reply, Request};
pub use interest::{Interest, Lexicon, Matcher, Score, Weighted};
pub use links::{Found, paragraphs};
pub use model::{
    Confidence, Entry, Feed, Item, ItemKind, LOOKUP, Linked, Mark, Source, SourceKind, Stage,
    Subscription, Triage, Version, Via,
};
pub use opml::Outline;
pub use parse::{Parsed, parse};
pub use pdf::{OpenAccess, Pdf, pdf};
pub use rules::{Action, Rule};
pub use store::LOOKBACK_DAYS;
pub use suggested::{Suggested, suggested};
pub use sweep::SWEEP_PAGES;
pub use view::{Counts, IsValues, Row, Sort, View, ViewRequest};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// a subscription's address that is not an http(s) URL
    #[error("{url}: not a feed address: {message}")]
    Url { url: String, message: String },
    #[error("{feed}: already subscribed ({id})")]
    Subscribed { feed: String, id: String },
    /// an ISSN of a wrong form or check digit
    #[error("{issn}: not an ISSN")]
    Issn { issn: String },
    #[error("{filter:?}: not an OpenAlex filter")]
    Filter { filter: String },
    #[error("{id}: no such subscription")]
    NoSubscription { id: String },
    #[error("{id}: no such item")]
    NoItem { id: String },
    /// an interest whose term or pattern is no regex, or whose name is taken
    #[error("interest {name}: {message}")]
    Interest { name: String, message: String },
    /// the paper behind an item that is not a post (news or blog)
    #[error("{id}: not a post")]
    NotAPost { id: String },
    /// a feed that does not parse
    #[error("{url}: {message}")]
    Feed { url: String, message: String },
    /// a list of subscriptions that does not parse
    #[error("not an OPML list: {message}")]
    Opml { message: String },
    #[error("{}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{}: {source}", path.display())]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{}: {source}", path.display())]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
