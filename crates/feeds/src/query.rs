//! The query language over items: the library's (core's), with the facets
//! `kind:` (paper, preprint, news, blog), `is:` (new or unread, seen,
//! shortlisted, dismissed, added, editorial, and the caller's), `src:` (a
//! subscription's title or id), `prof:` (an interest the item reaches the
//! min_score of), and the numbers `score:` (its best score in an interest)
//! and `like:` (how like the library it is, 0–100: none without the index of
//! the library, which then matches nothing).

use papers_core::{Like, Query, Record, Schema};

use crate::feeds::Feeds;
use crate::interest::Score;
use crate::model::{Item, ItemKind, Triage};
use crate::normalize::family_name;

/// The facets of an item.
pub const FACETS: &[&str] = &["kind", "is", "src", "prof"];

/// The numbers of an item.
pub const NUMBERS: &[&str] = &["score", "like"];

/// What the feeds' queries take besides the library's fields.
pub const SCHEMA: Schema = Schema {
    facets: FACETS,
    numbers: NUMBERS,
};

/// A query of the feeds.
pub fn parse(text: &str) -> Query {
    Query::parse(text, SCHEMA)
}

/// The value of `kind:`.
pub fn kind_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Paper => "paper",
        ItemKind::Preprint => "preprint",
        ItemKind::News => "news",
        ItemKind::Blog => "blog",
    }
}

/// The value of `is:`; new is `is:unread` too.
pub fn triage_name(triage: Triage) -> &'static str {
    match triage {
        Triage::New => "new",
        Triage::Seen => "seen",
        Triage::Shortlisted => "shortlisted",
        Triage::Dismissed => "dismissed",
        Triage::Added => "added",
    }
}

/// A query as the app and the CLI show it: dismissed items left out (`-is:dismissed`
/// added) unless it has an `is:` term on them (`is:dismissed`, `-is:seen,dismissed`).
pub fn live(query: &str) -> String {
    let asks = query.split_whitespace().any(|token| {
        let token = token.strip_prefix('-').unwrap_or(token).to_lowercase();
        token
            .strip_prefix("is:")
            .is_some_and(|values| (values.split(',')).any(|v| v.trim_matches('"') == "dismissed"))
    });
    match (asks, query.trim().is_empty()) {
        (true, _) => query.to_string(),
        (false, true) => "-is:dismissed".to_string(),
        (false, false) => format!("{query} -is:dismissed"),
    }
}

/// The best of the scores; 0 without any.
pub fn best(scores: &[Score]) -> f64 {
    scores.iter().map(|s| s.score).fold(0.0, f64::max)
}

/// An item as a query sees it: its record, and the values counted.
pub(crate) struct Seen {
    pub record: Record,
    pub triage: Triage,
    pub scores: Vec<Score>,
    /// how like the library it is, with an index
    pub like: Option<Like>,
    /// its `is:` values, `unread` left out
    pub is: Vec<String>,
    /// the ids of its subscriptions
    pub sources: Vec<String>,
}

impl Feeds {
    /// An item as a query sees it, with the caller's `is:` values.
    pub(crate) fn seen(&self, item: &Item, extra: Vec<String>) -> Seen {
        let scores = self.scores(item);
        let triage = self.triage(&item.id);
        let mut is = vec![triage_name(triage).to_string()];
        if item.editorial {
            is.push("editorial".to_string());
        }
        is.extend(extra);
        let mut sources: Vec<String> = item
            .sources
            .iter()
            .map(|s| s.entry.source.clone())
            .collect();
        sources.sort();
        sources.dedup();
        let titles = sources.iter().filter_map(|id| {
            let subscription = self.subscriptions().iter().find(|s| &s.id == id);
            subscription.and_then(|s| s.title.as_deref())
        });
        let src: Vec<&str> = sources.iter().map(String::as_str).chain(titles).collect();
        let prof = scores.iter().filter(|s| s.reached).map(|s| &s.interest);
        let summary = item.summary.as_deref().unwrap_or("");
        let unread = (triage == Triage::New).then_some("unread");
        let has = [
            ("abstract", !summary.is_empty()),
            ("doi", item.doi.is_some()),
        ];
        let record = Record::new(&item.title)
            .authors(item.authors.iter().map(|a| family_name(a).unwrap_or(a)))
            .year(item.date.map(|d| chrono::Datelike::year(&d)))
            .container(item.journal.as_deref().unwrap_or(""))
            .r#abstract(summary)
            .doi(item.doi.as_deref().unwrap_or(""))
            .has(has.iter().filter(|(_, x)| *x).map(|(name, _)| name))
            .facet("kind", [kind_name(item.kind)])
            .facet("is", is.iter().map(String::as_str).chain(unread))
            .facet("src", src)
            .facet("prof", prof)
            .number("score", best(&scores));
        let like = self.like(item);
        let record = match &like {
            Some(like) => record.number("like", like.score),
            None => record,
        };
        Seen {
            record,
            triage,
            scores,
            like,
            is,
            sources,
        }
    }
}
