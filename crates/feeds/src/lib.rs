//! Feeds for papers-desktop: the new papers of the journals, preprint servers,
//! news sites and blogs followed.
//!
//! A [`Subscription`] is fetched (conditional GET, paced per host, through an
//! [`Http`]), its feed parsed into [`Entry`]s (with paper-feeder's fixes:
//! Copernicus bylines, Wiley author blobs, the DOI from the guid,
//! `prism:doi` or the link), and the entries of all sources merged into
//! [`Item`]s: one per paper or post. [`Feeds`] keeps them in a folder of
//! JSON files with their [`Triage`]. Nothing here knows of .bib files.

mod extras;
mod feeds;
pub mod http;
mod merge;
mod model;
pub mod normalize;
mod parse;
mod store;

use std::path::PathBuf;

pub use feeds::{FeedReport, Feeds, FetchReport, Now, Outcome};
pub use http::{Http, Network, Reply, Request};
pub use model::{
    Entry, Item, ItemKind, Mark, Source, SourceKind, Stage, Subscription, Triage, Version,
};
pub use parse::{Parsed, parse};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// a subscription's address that is not an http(s) URL
    #[error("{url}: not a feed address: {message}")]
    Url { url: String, message: String },
    #[error("{url}: already subscribed ({id})")]
    Subscribed { url: String, id: String },
    #[error("{id}: no such subscription")]
    NoSubscription { id: String },
    #[error("{id}: no such item")]
    NoItem { id: String },
    /// a feed that does not parse
    #[error("{url}: {message}")]
    Feed { url: String, message: String },
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
