//! The view of the items, for the app and the CLI: those that a query
//! passes, sorted, each with its score and its parts, its scores in the
//! interests and its texts marked (the query's words and its interests' terms), and the counts
//! of the sidebar.
//!
//! The counts are those of the library's sidebar: over every live item (not
//! removed), whatever the query; a saved search's are the items it shows.
//! The rows, as a saved search's, leave the removed items out unless the
//! query names them (`is:removed`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use chrono::{DateTime, Utc};
use papers_core::{Marks, Matching, Span};
use serde::{Deserialize, Serialize};

use crate::feeds::Feeds;
use crate::interest::{Score, joined};
use crate::model::{Item, ItemKind, Reminder, Triage, words};
use crate::query::{Seen, names_removed, parse};
use crate::rating::Rating;

/// The order of the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    /// by date, else by when first fetched
    #[default]
    Newest,
    /// the best score first, then the newest; `like` (the sort by likeness
    /// to the library of before, now a part of the score) read as it
    #[serde(alias = "like")]
    Score,
}

words!(Sort {
    Newest => "newest",
    Score => "score",
});

/// What a view shows.
#[derive(Clone, Copy, Default)]
pub struct ViewRequest<'a> {
    /// a query of the feeds
    pub query: &'a str,
    pub matching: Matching,
    pub sort: Sort,
    /// the removed items too, whatever the query
    pub removed: bool,
}

/// The rows a request shows, and the counts of the sidebar.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct View {
    pub rows: Vec<Row>,
    pub counts: Counts,
}

/// An item shown.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    pub id: String,
    /// its score and its parts
    pub rating: Rating,
    /// the interests it matched a term of, the best first (by their scaled
    /// scores, as the keywords part takes them)
    pub interests: Vec<Score>,
    /// the words of the query and the terms of its interests marked
    pub title: Vec<Span>,
    /// the family name of each author, the words of the query marked
    pub authors: Vec<Vec<Span>>,
    /// the summary, marked as the title
    pub r#abstract: Vec<Span>,
    /// snoozed, or due: first in the list
    pub reminder: Option<Reminder>,
    /// the library's entries of the same paper (its DOI, or its title and
    /// year), newest first; none without the library's facts
    pub in_library: Vec<String>,
    /// the posts about it (linking a DOI of its versions)
    pub covered_by: usize,
}

/// The live items (not removed) of each group of the sidebar; a value no
/// live item has is left out.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Counts {
    pub all: usize,
    /// by `is:` value: triage, `editorial`, `inlibrary` (`unread` is `new`)
    pub is: BTreeMap<String, usize>,
    pub total: Groups,
    /// those new (`is:new`: not snoozed)
    pub unread: Groups,
    /// the items each saved view shows (removed ones unless it asks), by name
    pub views: BTreeMap<String, usize>,
}

/// Items by kind, interest and subscription.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Groups {
    pub kinds: BTreeMap<ItemKind, usize>,
    /// the items of each interest (`prof:`), by name
    pub interests: BTreeMap<String, usize>,
    /// by subscription id
    pub sources: BTreeMap<String, usize>,
}

/// One more of a key.
fn count(map: &mut BTreeMap<String, usize>, key: &str) {
    *map.entry(key.to_string()).or_default() += 1;
}

impl Counts {
    fn add(&mut self, item: &Item, seen: &Seen) {
        self.all += 1;
        seen.is.iter().for_each(|v| count(&mut self.is, v));
        self.total.add(item, seen);
        if seen.unread() {
            self.unread.add(item, seen);
        }
    }
}

impl Groups {
    fn add(&mut self, item: &Item, seen: &Seen) {
        *self.kinds.entry(item.kind).or_default() += 1;
        let reached = seen.scores.iter().filter(|s| s.reached);
        reached.for_each(|s| count(&mut self.interests, &s.interest));
        seen.sources
            .iter()
            .for_each(|id| count(&mut self.sources, id));
    }
}

impl Feeds {
    /// The items a request shows, and the counts of the sidebar.
    pub fn view(&self, request: &ViewRequest) -> View {
        let query = parse(request.query);
        let removed = request.removed || names_removed(&query);
        let views: Vec<_> = (self.saved_views().iter())
            .map(|v| (&v.name, parse(&v.query)))
            .map(|(name, query)| (name, names_removed(&query), query))
            .collect();
        let mut counts = Counts::default();
        let mut rows = Vec::new();
        let covering = self.covering();
        for item in self.items() {
            let seen = self.seen(item);
            let live = seen.triage() != Triage::Removed;
            if live {
                counts.add(item, &seen);
            }
            for (name, removed, view) in &views {
                if (live || *removed) && seen.record.check(view, request.matching).is_some() {
                    *counts.views.entry(name.to_string()).or_default() += 1;
                }
            }
            if !live && !removed {
                continue;
            }
            if let Some(marks) = seen.record.check(&query, request.matching) {
                let covered_by = covered_by(&covering, item);
                rows.push((newest(item), self.row(item, seen, &marks, covered_by)));
            }
        }
        match request.sort {
            Sort::Newest => rows.sort_by_key(|r| std::cmp::Reverse(r.0)),
            Sort::Score => {
                let score = |row: &Row| row.rating.score;
                rows.sort_by(|a, b| score(&b.1).total_cmp(&score(&a.1)).then(b.0.cmp(&a.0)))
            }
        }
        // the reminders due first, in that order
        rows.sort_by_key(|(_, row)| row.reminder != Some(Reminder::Due));
        let rows = rows.into_iter().map(|(_, row)| row).collect();
        View { rows, counts }
    }

    /// An item alone, as the details show it: its texts marked where the
    /// query matched (if it does) and its interests' terms; by its id or an alias.
    pub fn row_of(&self, id: &str, query: &str, matching: Matching) -> Option<Row> {
        let item = self.item(id)?;
        let seen = self.seen(item);
        let marks = seen
            .record
            .check(&parse(query), matching)
            .unwrap_or_default();
        let covered_by = covered_by(&self.covering(), item);
        Some(self.row(item, seen, &marks, covered_by))
    }

    /// The items starred, the latest first seen first.
    pub fn starred(&self) -> Vec<&Item> {
        let mut starred: Vec<&Item> = (self.items().iter())
            .filter(|i| self.triage(&i.id) == Triage::Starred)
            .collect();
        starred.sort_by_key(|i| std::cmp::Reverse(i.first_seen()));
        starred
    }

    /// The posts that link each DOI, by their ids.
    fn covering(&self) -> HashMap<&str, HashSet<&str>> {
        let mut posts: HashMap<&str, HashSet<&str>> = HashMap::new();
        for post in self.items().iter().filter(|i| !i.links.is_empty()) {
            for link in &post.links {
                posts.entry(&link.doi).or_default().insert(&post.id);
            }
        }
        posts
    }

    /// An item shown: the query's marks, its interests' terms marked too.
    fn row(&self, item: &Item, seen: Seen, marks: &Marks, covered_by: usize) -> Row {
        let summary = item.summary.as_deref().unwrap_or("");
        let matched = || {
            let lexicons = self.lexicons().iter().zip(&seen.scores);
            lexicons
                .filter(|(_, s)| !s.terms.is_empty())
                .map(|(l, _)| l)
        };
        let terms = |text: &str| joined(matched().flat_map(|l| l.ranges(text)).collect());
        let record = &seen.record;
        let title = overlay(record.title_spans(marks), &terms(&item.title));
        let r#abstract = overlay(record.abstract_spans(marks), &terms(summary));
        let authors = (0..item.authors.len())
            .map(|a| record.author_spans(a, marks))
            .collect();
        let mut interests: Vec<Score> = seen
            .scores
            .iter()
            .filter(|s| !s.terms.is_empty())
            .cloned()
            .collect();
        interests.sort_by(|a, b| b.scaled.total_cmp(&a.scaled));
        Row {
            id: item.id.clone(),
            rating: seen.rating,
            interests,
            title,
            authors,
            r#abstract,
            reminder: seen.reminder,
            in_library: seen.in_library,
            covered_by,
        }
    }
}

/// The number of posts about an item, of those that link each DOI: of any of
/// its versions, the item itself left out.
fn covered_by(covering: &HashMap<&str, HashSet<&str>>, item: &Item) -> usize {
    let dois = item.doi.iter().chain(item.versions.iter().map(|v| &v.doi));
    let posts = dois.filter_map(|d| covering.get(d.as_str())).flatten();
    let posts: HashSet<&&str> = posts.filter(|&&id| id != item.id).collect();
    posts.len()
}

/// When an item is of: its date, else when it was first fetched.
fn newest(item: &Item) -> DateTime<Utc> {
    item.date.unwrap_or_else(|| item.first_seen())
}

/// Spans with the text in `ranges` (of their text joined) marked too, the
/// neighbouring spans of a kind joined.
fn overlay(spans: Vec<Span>, ranges: &[Range<usize>]) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut at = 0;
    for span in spans {
        let end = at + span.text.len();
        let inside = ranges
            .iter()
            .flat_map(|r| [r.start, r.end])
            .filter(|&b| at < b && b < end);
        let mut cuts: Vec<usize> = [at, end].into_iter().chain(inside).collect();
        cuts.sort_unstable();
        cuts.dedup();
        for piece in cuts.windows(2) {
            let (from, to) = (piece[0], piece[1]);
            let mark = span.mark || ranges.iter().any(|r| r.start <= from && to <= r.end);
            let text = &span.text[from - at..to - at];
            match out.last_mut() {
                Some(last) if last.mark == mark => last.text.push_str(text),
                _ => out.push(Span {
                    text: text.to_string(),
                    mark,
                }),
            }
        }
        at = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(pieces: &[(&str, bool)]) -> Vec<Span> {
        let span = |&(text, mark): &(&str, bool)| Span {
            text: text.to_string(),
            mark,
        };
        pieces.iter().map(span).collect()
    }

    #[test]
    fn overlay_marks_ranges_across_spans() {
        // "Ice-sheet melt in Greenland", "melt" marked by the query
        let query = spans(&[
            ("Ice-sheet ", false),
            ("melt", true),
            (" in Greenland", false),
        ]);
        assert_eq!(
            overlay(query.clone(), &[0..9, 18..27]),
            spans(&[
                ("Ice-sheet", true),
                (" ", false),
                ("melt", true),
                (" in ", false),
                ("Greenland", true)
            ])
        );
        assert_eq!(overlay(query.clone(), &[]), query);
        assert_eq!(
            overlay(query, &[6..16, 40..44]),
            spans(&[
                ("Ice-sh", false),
                ("eet melt i", true),
                ("n Greenland", false)
            ])
        );
        assert_eq!(overlay(Vec::new(), &[0..3, 5..6]), []);
    }
}
