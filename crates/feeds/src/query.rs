//! The query language over items: the library's (core's), with the facets
//! `kind:` (paper, preprint, news, blog), `is:` (new or unread, seen,
//! shortlisted, dismissed, added, editorial, snoozed (not new till its day),
//! reminder (new again, due), inlibrary (with the library's facts)), `src:` (a
//! subscription's title or id), `prof:` (an interest the item reaches the
//! min_score of), and the numbers `score:` (its best score in an interest)
//! and `like:` (how like the library it is, 0–100: none without the index of
//! the library, which then matches nothing). A query leaves the dismissed
//! items out unless it names them (`is:dismissed`, `-is:seen,dismissed`).

use papers_core::{Like, Query, Record, Schema};

use crate::feeds::Feeds;
use crate::interest::Score;
use crate::model::{Item, Mark, Reminder, Triage};
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

/// Whether a query shows the dismissed items: those it names.
pub(crate) fn names_dismissed(query: &Query) -> bool {
    query.names("is", Triage::Dismissed.as_str())
}

/// The best of the scores; 0 without any.
pub fn best(scores: &[Score]) -> f64 {
    scores.iter().map(|s| s.score).fold(0.0, f64::max)
}

/// An item as a query sees it: its record, and the values counted.
pub(crate) struct Seen {
    pub record: Record,
    /// its triage and when it was set; none: new
    pub mark: Option<Mark>,
    /// where its reminder is now
    pub reminder: Option<Reminder>,
    pub scores: Vec<Score>,
    /// how like the library it is, with an index
    pub like: Option<Like>,
    /// its `is:` values, `unread` left out
    pub is: Vec<&'static str>,
    /// the library's entries of the same paper
    pub in_library: Vec<String>,
    /// the ids of its subscriptions (not a lookup's or a search's source)
    pub sources: Vec<String>,
}

impl Seen {
    pub fn triage(&self) -> Triage {
        self.mark.map_or(Triage::New, |m| m.triage)
    }

    /// Whether it is new and not snoozed (`is:unread`).
    pub fn unread(&self) -> bool {
        unread(self.triage(), self.reminder)
    }
}

/// Whether an item is new and not snoozed.
fn unread(triage: Triage, reminder: Option<Reminder>) -> bool {
    triage == Triage::New && reminder != Some(Reminder::Snoozed)
}

impl Feeds {
    /// An item as a query sees it.
    pub(crate) fn seen(&self, item: &Item) -> Seen {
        let scores = self.scores(item);
        let mark = self.mark(&item.id).copied();
        let reminder = mark.and_then(|m| m.reminder((self.now)()));
        let triage = mark.map_or(Triage::New, |m| m.triage);
        let in_library = self.in_library(item);
        let is = [
            Some(match reminder {
                Some(Reminder::Snoozed) => Reminder::Snoozed.as_str(),
                _ => triage.as_str(),
            }),
            (reminder == Some(Reminder::Due)).then_some("reminder"),
            item.editorial.then_some("editorial"),
            (!in_library.is_empty()).then_some("inlibrary"),
        ];
        let is: Vec<&'static str> = is.into_iter().flatten().collect();
        let subscriptions = self.subscriptions_of(item);
        let sources: Vec<String> = subscriptions.iter().map(|s| s.id.clone()).collect();
        let titles = subscriptions
            .iter()
            .filter_map(|s| s.settings.title.as_deref());
        let src: Vec<&str> = sources.iter().map(String::as_str).chain(titles).collect();
        let prof = scores.iter().filter(|s| s.reached).map(|s| &s.interest);
        let summary = item.summary.as_deref().unwrap_or("");
        let unread = unread(triage, reminder).then_some("unread");
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
            .facet("kind", [item.kind.as_str()])
            .facet("is", is.iter().copied().chain(unread))
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
            mark,
            reminder,
            scores,
            like,
            is,
            in_library,
            sources,
        }
    }
}
