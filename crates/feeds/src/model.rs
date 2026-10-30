//! What the feeds hold: the subscriptions, the entries of a feed, the items
//! they make, and the triage of an item.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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

/// A feed followed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    /// canonical: scheme and host lowercased, no fragment
    pub url: String,
    /// the feed's own title, until the user names it
    pub title: Option<String>,
    pub kind: SourceKind,
    /// the journal of its items, over what the feed says
    #[serde(default)]
    pub journal: Option<String>,
    /// fetch the page of each item at fetch time (the paper of a post)
    #[serde(default)]
    pub fetch_pages: bool,
}

/// One item of one feed, as the feed gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// the subscription's id
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
}

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
    /// every DOI of its sources, preprint or published (Crossref relations later)
    pub versions: Vec<Version>,
    /// the DOIs of the papers a post is about (found later)
    #[serde(default)]
    pub linked: Vec<String>,
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
