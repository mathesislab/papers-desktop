//! The rows of the list and the details of an item, as the page shows them:
//! the crate's, with the titles of the subscriptions, the library's papers
//! like it, the items of its versions and links.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use papers_core::{Library, Matching, Span};
use papers_feeds::{
    Confidence, Feeds, Groups, Item, ItemKind, Rating, Reminder, Score, Sort, SourceKind, Stage,
    Stages, Triage, Via, ViewRequest as CrateRequest, safe_html,
};
use serde::{Deserialize, Serialize};

use super::title_of;

/// What the page asks a view for.
#[derive(Debug, Deserialize)]
pub struct ViewRequest {
    query: String,
    sort: Sort,
    /// when the list was last brought up to date: what was fetched since is held back
    #[serde(default)]
    shown: Option<DateTime<Utc>>,
}

/// A row of the list.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedRow {
    id: String,
    kind: ItemKind,
    title: Vec<Span>,
    authors: Vec<Vec<Span>>,
    journal: Option<String>,
    /// the titles of the subscriptions that listed it
    sources: Vec<String>,
    date: Option<DateTime<Utc>>,
    /// its score and its parts: `score`, `keywords`, `library`, `excluded`
    #[serde(flatten)]
    rating: Rating,
    interests: Vec<Score>,
    triage: Triage,
    /// snoozed, or due (first in the list)
    reminder: Option<Reminder>,
    /// when it is to be reminded of
    remind: Option<DateTime<Utc>>,
    in_library: Vec<String>,
    /// the stages of it known: a preprint, published, or both
    stages: Stages,
    covered_by: usize,
    links: usize,
    /// the works that cite it, as OpenAlex counted them when it was found
    cited: Option<u64>,
}

/// The counts of the sidebar, the subscriptions by title, the saved views by name.
#[derive(Debug, Serialize)]
pub struct FeedCounts {
    all: usize,
    is: BTreeMap<String, usize>,
    total: FeedGroups,
    /// those new
    unread: FeedGroups,
    views: BTreeMap<String, usize>,
}

/// Items by kind, interest and subscription (by title).
#[derive(Debug, Serialize)]
pub struct FeedGroups {
    kinds: BTreeMap<ItemKind, usize>,
    interests: BTreeMap<String, usize>,
    sources: BTreeMap<String, usize>,
}

impl FeedGroups {
    /// The crate's groups, the subscriptions by title.
    fn of(groups: Groups, titles: &HashMap<&str, String>) -> FeedGroups {
        let mut sources: BTreeMap<String, usize> = BTreeMap::new();
        for (id, count) in groups.sources {
            if let Some(title) = titles.get(id.as_str()) {
                *sources.entry(title.clone()).or_default() += count;
            }
        }
        FeedGroups {
            kinds: groups.kinds,
            interests: groups.interests,
            sources,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FeedView {
    rows: Vec<FeedRow>,
    counts: FeedCounts,
    /// the items fetched since the list was shown, held back
    held: usize,
}

/// A paper behind a post, and its item.
#[derive(Debug, Serialize)]
pub struct Linked {
    doi: String,
    confidence: Confidence,
    via: Via,
    item: Option<String>,
    title: Option<String>,
}

/// An item, for the details pane.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedDetail {
    id: String,
    kind: ItemKind,
    title: Vec<Span>,
    authors: Vec<String>,
    journal: Option<String>,
    date: Option<DateTime<Utc>>,
    doi: Option<String>,
    arxiv: Option<String>,
    url: Option<String>,
    r#abstract: Vec<Span>,
    /// the abstract is the text of a feed (a teaser, a summary), not Crossref's or OpenAlex's
    abstract_from_feed: bool,
    /// its lookups may still find its abstract: the reader asks (`feeds_look_up`)
    abstract_pending: bool,
    /// a post's HTML, safe to show (`safe_html`): its text, links and images
    content: Option<String>,
    editorial: bool,
    sources: Vec<SourceOf>,
    versions: Vec<VersionOf>,
    links: Vec<Linked>,
    covered_by: Vec<Post>,
    interests: Vec<Score>,
    triage: Triage,
    in_library: Vec<String>,
    version_in_library: Vec<String>,
    /// its score and its parts, as its row's
    #[serde(flatten)]
    rating: Rating,
    /// the papers of the library most like it, the nearest first
    nearest: Vec<Nearest>,
    /// the works that cite it, as OpenAlex counted them when it was found
    cited: Option<u64>,
    /// its OpenAlex id (`W…`), if an entry is OpenAlex's: the works around it are searched by it,
    /// else by its DOI
    openalex: Option<String>,
}

/// A paper of the library like an item.
#[derive(Debug, Serialize)]
pub struct Nearest {
    id: String,
    key: String,
    title: String,
    /// "Feldmann & Levermann · 2017 · Cryosphere"
    subtitle: String,
    /// of the item's vector and the paper's
    cosine: f64,
}

#[derive(Debug, Serialize)]
pub struct SourceOf {
    id: String,
    title: String,
    kind: SourceKind,
}

#[derive(Debug, Serialize)]
pub struct VersionOf {
    doi: String,
    stage: Stage,
    item: Option<String>,
}

/// A post about a paper.
#[derive(Debug, Serialize)]
pub struct Post {
    id: String,
    title: String,
    source: String,
}

/// The item of a DOI, its own or one of its versions', but `not` this one.
fn item_of_doi<'a>(feeds: &'a Feeds, doi: &str, not: &str) -> Option<&'a Item> {
    let has = |i: &&Item| i.doi.as_deref() == Some(doi) || i.versions.iter().any(|v| v.doi == doi);
    feeds.items().iter().filter(|i| i.id != not).find(has)
}

/// A paper behind a post, with its item if there is one.
pub(super) fn linked(feeds: &Feeds, link: &papers_feeds::Linked) -> Linked {
    let item = item_of_doi(feeds, &link.doi, "");
    Linked {
        doi: link.doi.clone(),
        confidence: link.confidence,
        via: link.via,
        item: item.map(|i| i.id.clone()),
        title: item.map(|i| i.title.clone()),
    }
}

/// The view of a request: the rows, each in the library or not, and the counts.
pub fn view(feeds: &Feeds, matching: Matching, request: &ViewRequest) -> FeedView {
    let view = feeds.view(&CrateRequest {
        query: &request.query,
        matching,
        sort: request.sort,
        shown: request.shown,
        ..CrateRequest::default()
    });
    let rows = Rows::new(feeds);
    FeedView {
        held: view.held,
        counts: FeedCounts {
            all: view.counts.all,
            is: view.counts.is,
            total: FeedGroups::of(view.counts.total, &rows.titles),
            unread: FeedGroups::of(view.counts.unread, &rows.titles),
            views: view.counts.views,
        },
        rows: view.rows.into_iter().map(|row| rows.row(row)).collect(),
    }
}

/// What the rows of the list show besides the crate's rows: the titles of the
/// subscriptions.
pub struct Rows<'a> {
    feeds: &'a Feeds,
    titles: HashMap<&'a str, String>,
}

impl<'a> Rows<'a> {
    pub fn new(feeds: &'a Feeds) -> Rows<'a> {
        let titles = (feeds.subscriptions().iter())
            .map(|s| (s.id.as_str(), title_of(s)))
            .collect();
        Rows { feeds, titles }
    }

    /// The row of the list of a crate's row.
    pub fn row(&self, row: papers_feeds::Row) -> FeedRow {
        let feeds = self.feeds;
        let item = feeds.item(&row.id).expect("a row's item");
        FeedRow {
            kind: item.kind,
            journal: item.journal.clone(),
            sources: (feeds.subscriptions_of(item).into_iter())
                .map(title_of)
                .collect(),
            date: item.date,
            triage: feeds.triage(&item.id),
            reminder: row.reminder,
            remind: feeds.mark(&item.id).and_then(|m| m.remind),
            in_library: row.in_library,
            stages: item.stages(),
            covered_by: row.covered_by,
            links: item.links.len(),
            cited: item.cited,
            id: row.id,
            title: row.title,
            authors: row.authors,
            rating: row.rating,
            interests: row.interests,
        }
    }
}

/// An item, for the details: its texts marked by the query and its interests.
pub fn detail(
    feeds: &Feeds,
    library: &Library,
    matching: Matching,
    id: &str,
    query: &str,
) -> Result<FeedDetail, String> {
    let item = feeds
        .item(id)
        .ok_or_else(|| format!("{id}: no such item"))?;
    let row = feeds.row_of(id, query, matching).expect("an item");
    let first_source = |item: &Item| {
        let first = feeds.subscriptions_of(item).into_iter().next();
        first.map(title_of).unwrap_or_default()
    };
    let in_library = row.in_library;
    let like = feeds.like(item);
    let nearest = (like.iter().flat_map(|l| &l.nearest))
        .filter_map(|(id, cosine)| {
            library.get(id).map(|entry| Nearest {
                id: entry.id.clone(),
                key: entry.key.clone(),
                title: entry.title.clone(),
                subtitle: entry.subtitle.clone(),
                cosine: *cosine,
            })
        })
        .collect();
    Ok(FeedDetail {
        id: item.id.clone(),
        kind: item.kind,
        title: row.title,
        authors: item.authors.clone(),
        journal: item.journal.clone(),
        date: item.date,
        doi: item.doi.clone(),
        arxiv: item.arxiv.clone(),
        url: item.url.clone(),
        cited: item.cited,
        openalex: item.openalex_id().map(String::from),
        r#abstract: row.r#abstract,
        abstract_from_feed: item.summary.is_some() && !feeds.abstract_from_work(item),
        abstract_pending: feeds.pending(item).is_some(),
        content: (item.content.as_deref()).map(|html| safe_html(html, item.url.as_deref())),
        editorial: item.editorial,
        sources: (feeds.subscriptions_of(item).into_iter())
            .map(|s| SourceOf {
                id: s.id.clone(),
                title: title_of(s),
                kind: s.settings.kind,
            })
            .collect(),
        versions: (item.versions.iter())
            .map(|v| VersionOf {
                doi: v.doi.clone(),
                stage: v.stage,
                item: item_of_doi(feeds, &v.doi, &item.id).map(|i| i.id.clone()),
            })
            .collect(),
        links: item.links.iter().map(|l| linked(feeds, l)).collect(),
        covered_by: (feeds.covered_by(&item.id).into_iter())
            .map(|post| Post {
                id: post.id.clone(),
                title: post.title.clone(),
                source: first_source(post),
            })
            .collect(),
        interests: row.interests,
        triage: feeds.triage(&item.id),
        version_in_library: feeds.versions_in_library(item, &in_library),
        in_library,
        rating: row.rating,
        nearest,
    })
}
