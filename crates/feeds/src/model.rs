//! What the feeds hold: the subscriptions, the entries of a feed, the items
//! they make, and the triage of an item.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::normalize::{canonical_issn, canonical_url, feed_key, short_id};
use crate::parse::EDITORIAL;
use crate::{Error, Result};

/// The words of an enum's values, as its JSON writes them: `as_str`,
/// `Display`, and `FromStr` (the error names the words).
macro_rules! words {
    ($enum:ident { $($value:ident => $word:literal),+ $(,)? }) => {
        impl $enum {
            /// Its word, as its JSON writes it.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($enum::$value => $word),+
                }
            }
        }

        impl std::fmt::Display for $enum {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl std::str::FromStr for $enum {
            type Err = String;

            fn from_str(word: &str) -> std::result::Result<$enum, String> {
                match word {
                    $($word => Ok($enum::$value),)+
                    _ => Err(format!("{word:?}: not one of {}", [$($word),+].join(", "))),
                }
            }
        }
    };
}
pub(crate) use words;

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

words!(SourceKind {
    Journal => "journal",
    Preprints => "preprints",
    News => "news",
    Blog => "blog",
});

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

words!(ItemKind {
    Paper => "paper",
    Preprint => "preprint",
    News => "news",
    Blog => "blog",
});

impl ItemKind {
    /// A post (news or blog), about papers rather than one.
    pub fn is_post(self) -> bool {
        matches!(self, ItemKind::News | ItemKind::Blog)
    }

    /// This kind, or a preprint for a paper that is one (a journal's feed
    /// or sweep lists preprints too).
    pub fn promoted(self, preprint: bool) -> ItemKind {
        match self {
            ItemKind::Paper if preprint => ItemKind::Preprint,
            kind => kind,
        }
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

/// A feed followed: what it is, and what the user sets on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    #[serde(flatten)]
    pub feed: Feed,
    #[serde(flatten)]
    pub settings: SubscriptionSettings,
}

impl Subscription {
    /// A subscription of a feed (canonical), by the feed's key, with the
    /// settings of a kind alone.
    pub fn new(feed: Feed, kind: SourceKind) -> Subscription {
        Subscription {
            id: short_id(&feed.key()),
            feed,
            settings: SubscriptionSettings::new(kind),
        }
    }
}

/// What the user names or sets on a subscription.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionSettings {
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

impl SubscriptionSettings {
    /// The settings of a kind alone: the feed's own title, its journal, no
    /// pages fetched, no boost.
    pub fn new(kind: SourceKind) -> SubscriptionSettings {
        SubscriptionSettings {
            title: None,
            kind,
            journal: None,
            fetch_pages: false,
            boost: 0.0,
        }
    }

    /// With the title and the journal trimmed, a blank one none: the feed's
    /// own again (a title at the next fetch).
    pub(crate) fn trimmed(self) -> SubscriptionSettings {
        let trim =
            |text: Option<String>| text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        SubscriptionSettings {
            title: trim(self.title),
            journal: trim(self.journal),
            ..self
        }
    }
}

/// Where an entry's data came from: a feed's text, or a work of Crossref or
/// OpenAlex (a sweep's, a lookup's, a paper of a post, a search's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    #[default]
    Feed,
    Work,
}

words!(Origin {
    Feed => "feed",
    Work => "work",
});

/// One item of one feed, as the feed gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// what it was fetched under: a subscription's id, [`LOOKUP`] for a work
    /// looked up by its DOI, [`crate::FOUND`] for a work searched
    pub source: String,
    /// a feed's entry or a work (those of before were given theirs when
    /// their folder opened)
    #[serde(default)]
    pub origin: Origin,
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
    /// the works that cite it, as OpenAlex counted them when it was found
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cited: Option<u64>,
}

/// The source of the works looked up by DOI (abstracts, authors, versions).
pub const LOOKUP: &str = "lookup";

impl Entry {
    /// An entry of a source, of an origin, by its guid: its title (an
    /// editorial if the title says so) and its kind, nothing else known yet.
    pub fn new(source: &str, origin: Origin, guid: String, title: String, kind: ItemKind) -> Entry {
        Entry {
            source: source.to_string(),
            origin,
            guid,
            editorial: EDITORIAL.is_match(&title),
            title,
            authors: Vec::new(),
            journal: None,
            date: None,
            doi: None,
            arxiv: None,
            url: None,
            summary: None,
            content: None,
            kind,
            versions: Vec::new(),
            cited: None,
        }
    }
}

impl Origin {
    /// The origin of an entry kept before entries had one: a work if it is
    /// a lookup's or a search's, if it has versions (Crossref's work has its
    /// own DOI among them, a feed's entry none) or an OpenAlex id, or if its
    /// subscription is a sweep.
    pub(crate) fn of_old(entry: &Entry, subscription: Option<&Subscription>) -> Origin {
        let work = [LOOKUP, crate::FOUND].contains(&entry.source.as_str())
            || !entry.versions.is_empty()
            || entry.guid.starts_with("https://openalex.org/")
            || subscription.is_some_and(|s| !matches!(s.feed, Feed::Rss { .. }));
        match work {
            true => Origin::Work,
            false => Origin::Feed,
        }
    }
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

words!(Stage {
    Preprint => "preprint",
    Published => "published",
});

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
    /// the abstract looked up by its DOI, else the longest of its sources
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
    /// the works that cite it, the most its sources counted
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cited: Option<u64>,
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

words!(Confidence {
    High => "high",
    Low => "low",
});

/// Where a linked paper was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    /// the post's text in its feed
    Feed,
    /// the post's page
    Page,
    /// the post's title in Crossref (`search` before)
    #[serde(alias = "search")]
    Title,
}

words!(Via {
    Feed => "feed",
    Page => "page",
    Title => "title",
});

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

    /// Whether it is a preprint, or has one among its versions.
    pub fn has_versions(&self) -> bool {
        self.kind == ItemKind::Preprint || self.versions.iter().any(|v| v.stage == Stage::Preprint)
    }

    /// Its OpenAlex id (`W…`), if an entry is OpenAlex's work.
    pub fn openalex_id(&self) -> Option<&str> {
        (self.sources.iter())
            .find_map(|s| s.entry.guid.strip_prefix("https://openalex.org/"))
            .filter(|id| id.starts_with('W'))
    }
}

/// Where the user is with an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Triage {
    #[default]
    New,
    Seen,
    /// `shortlisted` in the state of before
    #[serde(alias = "shortlisted")]
    Starred,
    Dismissed,
    /// added to the library
    Added,
}

words!(Triage {
    New => "new",
    Seen => "seen",
    Starred => "starred",
    Dismissed => "dismissed",
    Added => "added",
});

impl Triage {
    /// Which of two states an item keeps when its versions merge: the user's
    /// strongest decision.
    fn weight(self) -> u8 {
        match self {
            Triage::New => 0,
            Triage::Seen => 1,
            Triage::Dismissed => 2,
            Triage::Starred => 3,
            Triage::Added => 4,
        }
    }

    /// Kept by a prune, however old.
    pub fn kept(self) -> bool {
        matches!(self, Triage::Starred | Triage::Added)
    }
}

/// A triage state and when it was set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub triage: Triage,
    pub at: DateTime<Utc>,
    /// a new item to be reminded of then (snoozed till then, due after); any
    /// other triage set clears it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remind: Option<DateTime<Utc>>,
}

/// Where an item to be reminded of is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reminder {
    /// out of the unread items until its day (`is:snoozed`)
    Snoozed,
    /// new, first in the list, and not seen by being shown (`is:reminder`)
    Due,
}

words!(Reminder {
    Snoozed => "snoozed",
    Due => "due",
});

impl Mark {
    /// A triage set now, no reminder.
    pub fn new(triage: Triage, at: DateTime<Utc>) -> Mark {
        Mark {
            triage,
            at,
            remind: None,
        }
    }

    /// Where its reminder is at `now`, if it has one.
    pub fn reminder(&self, now: DateTime<Utc>) -> Option<Reminder> {
        let remind = self.remind.filter(|_| self.triage == Triage::New)?;
        Some(match remind > now {
            true => Reminder::Snoozed,
            false => Reminder::Due,
        })
    }

    /// The mark of two merged items: the strongest decision, the latest if
    /// equal; a new one keeps the earliest reminder of the two.
    pub(crate) fn stronger(self, other: Mark) -> Mark {
        let stronger = match self.triage.weight().cmp(&other.triage.weight()) {
            std::cmp::Ordering::Greater => self,
            std::cmp::Ordering::Less => other,
            std::cmp::Ordering::Equal if self.at >= other.at => self,
            std::cmp::Ordering::Equal => other,
        };
        let pending = |m: Mark| m.remind.filter(|_| m.triage == Triage::New);
        let remind = [pending(self), pending(other)].into_iter().flatten().min();
        Mark {
            remind: remind.filter(|_| stronger.triage == Triage::New),
            ..stronger
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each word as JSON writes it, and back.
    fn same_as_json<T>(values: &[T])
    where
        T: Copy + PartialEq + std::fmt::Debug + Serialize + std::str::FromStr + std::fmt::Display,
    {
        for &value in values {
            let json = serde_json::to_value(value).unwrap();
            assert_eq!(json.as_str(), Some(value.to_string().as_str()));
            assert!(value.to_string().parse::<T>().is_ok_and(|v| v == value));
        }
    }

    #[test]
    fn merged_marks_keep_the_earliest_reminder_of_a_new_item() {
        use chrono::TimeZone;
        let day = |d| Utc.with_ymd_and_hms(2026, 11, d, 0, 0, 0).unwrap();
        let snoozed = |at, till| Mark {
            remind: Some(day(till)),
            ..Mark::new(Triage::New, day(at))
        };
        let new = Mark::new(Triage::New, day(5));
        assert_eq!(
            snoozed(1, 9).stronger(new).remind,
            Some(day(9)),
            "the later one's"
        );
        assert_eq!(new.stronger(snoozed(1, 9)).remind, Some(day(9)));
        let earliest = snoozed(1, 9).stronger(snoozed(2, 7));
        assert_eq!((earliest.at, earliest.remind), (day(2), Some(day(7))));
        let seen = Mark::new(Triage::Seen, day(3));
        assert_eq!(
            snoozed(1, 9).stronger(seen),
            seen,
            "another triage clears it"
        );
    }

    #[test]
    fn words_are_those_of_json() {
        use SourceKind::{Blog, Journal, News, Preprints};
        same_as_json(&[Journal, Preprints, News, Blog]);
        let kinds = [ItemKind::Paper, ItemKind::Preprint];
        same_as_json(&[kinds[0], kinds[1], ItemKind::News, ItemKind::Blog]);
        let triage = [Triage::New, Triage::Seen, Triage::Starred];
        same_as_json(&[
            triage[0],
            triage[1],
            triage[2],
            Triage::Dismissed,
            Triage::Added,
        ]);
        same_as_json(&[Reminder::Snoozed, Reminder::Due]);
        same_as_json(&[Stage::Preprint, Stage::Published]);
        same_as_json(&[Confidence::High, Confidence::Low]);
        same_as_json(&[Via::Feed, Via::Page, Via::Title]);
        same_as_json(&[Origin::Feed, Origin::Work]);
        let refused = "podcast".parse::<ItemKind>().unwrap_err();
        assert_eq!(
            refused,
            r#""podcast": not one of paper, preprint, news, blog"#
        );
    }

    #[test]
    fn a_star_of_before_reads_as_starred() {
        let triage: Triage = serde_json::from_str(r#""shortlisted""#).unwrap();
        assert_eq!(triage, Triage::Starred);
        assert_eq!(serde_json::to_string(&triage).unwrap(), r#""starred""#);
        assert!("shortlisted".parse::<Triage>().is_err(), "its word no more");
    }

    #[test]
    fn a_paper_found_by_a_title_of_before_reads_as_by_title() {
        let via: Via = serde_json::from_str(r#""search""#).unwrap();
        assert_eq!(via, Via::Title);
        assert_eq!(serde_json::to_string(&via).unwrap(), r#""title""#);
    }
}
