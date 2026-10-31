//! What the feeds hold: the subscriptions, the entries of a feed, the items
//! they make, and the triage of an item.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::normalize::{canonical_issn, canonical_url, feed_key};
use crate::{Error, Result};

/// What a subscription follows, which gives its items their kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Journal,
    Preprints,
    /// news and press releases
    News,
    Blog,
}

impl SourceKind {
    /// The kind of its items, before an item corrects it.
    pub fn item_kind(self) -> ItemKind {
        match self {
            SourceKind::Journal => ItemKind::Paper,
            SourceKind::Preprints => ItemKind::Preprint,
            SourceKind::News => ItemKind::News,
            SourceKind::Blog => ItemKind::Blog,
        }
    }
}

/// What an item is; the order is the preference when sources disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Paper,
    Preprint,
    News,
    Blog,
}

impl ItemKind {
    /// A post (news or blog), about papers rather than one.
    pub fn is_post(self) -> bool {
        matches!(self, ItemKind::News | ItemKind::Blog)
    }
}

/// What a subscription follows: a feed, or a sweep of Crossref or OpenAlex.
/// In `config.json` as the subscription's `url`, `issn` or `openalex`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Feed {
    /// an RSS, Atom or JSON feed; canonical: scheme and host lowercased, no fragment
    Rss { url: String },
    /// the new works of a journal in Crossref, by ISSN (`1994-0424`)
    Crossref { issn: String },
    /// the new works of an OpenAlex filter: `primary_topic.id:T10644`,
    /// `cites:W1|W2`, `authorships.author.id:A123`
    OpenAlex {
        #[serde(rename = "openalex")]
        filter: String,
    },
}

impl Feed {
    /// The feed checked and canonical: an http(s) URL, a valid ISSN, a filter.
    pub fn canonical(&self) -> Result<Feed> {
        Ok(match self {
            Feed::Rss { url } => Feed::Rss {
                url: canonical_url(url)?,
            },
            Feed::Crossref { issn } => Feed::Crossref {
                issn: canonical_issn(issn).ok_or_else(|| Error::Issn { issn: issn.clone() })?,
            },
            Feed::OpenAlex { filter } => match filter.trim() {
                "" => {
                    return Err(Error::Filter {
                        filter: filter.clone(),
                    });
                }
                filter => Feed::OpenAlex {
                    filter: filter.to_string(),
                },
            },
        })
    }

    /// What makes two canonical feeds the same (http and https are one), and
    /// the subscription's id.
    pub fn key(&self) -> String {
        match self {
            Feed::Rss { url } => format!("feed:{}", feed_key(url)),
            Feed::Crossref { issn } => format!("crossref:{issn}"),
            Feed::OpenAlex { filter } => format!("openalex:{filter}"),
        }
    }
}

impl std::fmt::Display for Feed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Feed::Rss { url } => write!(f, "{url}"),
            Feed::Crossref { issn } => write!(f, "Crossref ISSN {issn}"),
            Feed::OpenAlex { filter } => write!(f, "OpenAlex {filter}"),
        }
    }
}

/// A feed followed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    #[serde(flatten)]
    pub feed: Feed,
    /// the feed's own title, until the user names it
    pub title: Option<String>,
    pub kind: SourceKind,
    /// the journal of its items, over what the feed says
    #[serde(default)]
    pub journal: Option<String>,
    /// fetch the page of each item at fetch time (the paper of a post)
    #[serde(default)]
    pub fetch_pages: bool,
    /// added to each interest's score of the items it lists (paper-feeder's
    /// `score_boost`: "cites my work" is partial credit); the strongest counts
    /// for an item several list
    #[serde(default, skip_serializing_if = "is_zero")]
    pub boost: f64,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

/// One item of one feed, as the feed gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// the subscription's id; [`LOOKUP`] for a work looked up by its DOI
    pub source: String,
    /// the feed's id of the item (its guid, Atom id, or feed-rs's hash of the link)
    pub guid: String,
    pub title: String,
    /// display names, as the feed writes them
    pub authors: Vec<String>,
    pub journal: Option<String>,
    pub date: Option<DateTime<Utc>>,
    /// canonical, lowercase
    pub doi: Option<String>,
    /// without its version
    pub arxiv: Option<String>,
    pub url: Option<String>,
    /// the abstract or teaser, plain text
    pub summary: Option<String>,
    /// the full text of a post, HTML
    pub content: Option<String>,
    /// news & views, perspective, editorial, comment, correspondence, opinion
    pub editorial: bool,
    /// the subscription's kind, corrected by the entry's identifiers
    pub kind: ItemKind,
    /// its DOIs as Crossref relates them: its own and those of its other
    /// versions (`has-preprint`, `is-preprint-of`, `is-version-of`); none from a feed
    #[serde(default)]
    pub versions: Vec<Version>,
}

/// The source of the works looked up by DOI (abstracts, authors, versions).
pub const LOOKUP: &str = "lookup";

/// An entry of an item, and when it was first fetched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub entry: Entry,
    pub first_seen: DateTime<Utc>,
    /// in the feed as last fetched: such an item is not pruned (it would come back)
    pub listed: bool,
}

/// Whether a DOI is of a preprint or of the published paper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Preprint,
    Published,
}

/// A DOI of an item: its preprint or its published version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub doi: String,
    pub stage: Stage,
}

/// One paper or post, from every source that listed it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// stable: from the first key of its first entry; merged items leave an alias
    pub id: String,
    pub kind: ItemKind,
    pub title: String,
    pub authors: Vec<String>,
    pub journal: Option<String>,
    pub date: Option<DateTime<Utc>>,
    /// the published version's, else the preprint's
    pub doi: Option<String>,
    pub arxiv: Option<String>,
    pub url: Option<String>,
    /// the longest of its sources
    pub summary: Option<String>,
    /// the longest of its sources
    pub content: Option<String>,
    pub editorial: bool,
    pub sources: Vec<Source>,
    /// every DOI of its sources and of their relations in Crossref, preprint or published
    pub versions: Vec<Version>,
    /// the papers a post is about, best first
    #[serde(default)]
    pub links: Vec<Linked>,
}

/// A paper a post is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Linked {
    pub doi: String,
    pub confidence: Confidence,
    pub via: Via,
}

/// Whether a linked paper is the one: high when it is labelled or wins
/// clearly, low when it is one of several candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Low,
}

/// Where a linked paper was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    /// the post's text in its feed
    Feed,
    /// the post's page
    Page,
    /// the post's title in Crossref
    Search,
}

impl Item {
    /// When it was first fetched, from any source.
    pub fn first_seen(&self) -> DateTime<Utc> {
        self.sources
            .iter()
            .map(|s| s.first_seen)
            .min()
            .expect("an item has a source")
    }

    /// Whether a feed still lists it.
    pub fn listed(&self) -> bool {
        self.sources.iter().any(|s| s.listed)
    }
}

/// Where the user is with an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Triage {
    #[default]
    New,
    Seen,
    Shortlisted,
    Dismissed,
    /// added to the library
    Added,
}

impl Triage {
    /// Which of two states an item keeps when its versions merge: the user's
    /// strongest decision.
    fn weight(self) -> u8 {
        match self {
            Triage::New => 0,
            Triage::Seen => 1,
            Triage::Dismissed => 2,
            Triage::Shortlisted => 3,
            Triage::Added => 4,
        }
    }

    /// Kept by a prune, however old.
    pub fn kept(self) -> bool {
        matches!(self, Triage::Shortlisted | Triage::Added)
    }
}

/// A triage state and when it was set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub triage: Triage,
    pub at: DateTime<Utc>,
}

impl Mark {
    /// The mark of two merged items: the strongest decision, the latest if equal.
    pub(crate) fn stronger(self, other: Mark) -> Mark {
        match self.triage.weight().cmp(&other.triage.weight()) {
            std::cmp::Ordering::Greater => self,
            std::cmp::Ordering::Less => other,
            std::cmp::Ordering::Equal if self.at >= other.at => self,
            std::cmp::Ordering::Equal => other,
        }
    }
}
