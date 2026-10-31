//! The view of the items, for the app and the CLI: those that a query and
//! the kinds pass, sorted, each with its scores and its texts marked (the
//! query's words and its interests' terms), and the counts of the sidebar.
//!
//! The counts are those of the library's sidebar: over every live item (not
//! dismissed), whatever the query and the kinds shown; a saved search's are
//! the items it shows. The rows are what the query passes, dismissed items
//! too: the app asks `-is:dismissed`.

use std::collections::BTreeMap;
use std::ops::Range;

use chrono::{DateTime, Utc};
use papers_core::{Marks, Matching, Span};
use serde::{Deserialize, Serialize};

use crate::feeds::Feeds;
use crate::interest::{Score, joined};
use crate::model::{Item, ItemKind, Triage};
use crate::query::{Seen, best, live, parse};

/// The order of the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    /// by date, else by when first fetched
    #[default]
    Newest,
    /// the best score first, then the newest
    Score,
}

/// What a view shows.
#[derive(Clone, Copy, Default)]
pub struct ViewRequest<'a> {
    /// a query of the feeds
    pub query: &'a str,
    pub matching: Matching,
    pub sort: Sort,
    /// the kinds shown; none: every kind
    pub kinds: &'a [ItemKind],
    pub is: Option<IsValues<'a>>,
}

/// The caller's `is:` values of an item, such as `inlibrary`.
pub type IsValues<'a> = &'a dyn Fn(&Item) -> Vec<String>;

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
    /// its best score in an interest; 0 without any
    pub score: f64,
    /// the interests it matched a term of, the best first
    pub interests: Vec<Score>,
    /// the words of the query and the terms of its interests marked
    pub title: Vec<Span>,
    /// the family name of each author, the words of the query marked
    pub authors: Vec<Vec<Span>>,
    /// the summary, marked as the title
    pub r#abstract: Vec<Span>,
}

/// The live items (not dismissed) of each group of the sidebar; a value no
/// live item has is left out.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Counts {
    pub all: usize,
    /// by `is:` value: triage, `editorial`, the caller's (`unread` is `new`)
    pub is: BTreeMap<String, usize>,
    pub kinds: BTreeMap<ItemKind, usize>,
    /// the items of each interest (`prof:`), by name
    pub interests: BTreeMap<String, usize>,
    /// by subscription id
    pub sources: BTreeMap<String, usize>,
    /// the items each saved search shows (dismissed ones unless it asks), by name
    pub searches: BTreeMap<String, usize>,
}

impl Counts {
    fn add(&mut self, item: &Item, seen: &Seen) {
        let count = |map: &mut BTreeMap<String, usize>, key: &str| {
            *map.entry(key.to_string()).or_default() += 1;
        };
        self.all += 1;
        *self.kinds.entry(item.kind).or_default() += 1;
        seen.is.iter().for_each(|v| count(&mut self.is, v));
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
        let searches: Vec<_> = (self.searches().iter())
            .map(|s| (&s.name, parse(&live(&s.query))))
            .collect();
        let mut counts = Counts::default();
        let mut rows = Vec::new();
        for item in self.items() {
            let extra = request.is.map_or_else(Vec::new, |is| is(item));
            let seen = self.seen(item, extra);
            if seen.triage != Triage::Dismissed {
                counts.add(item, &seen);
            }
            for (name, search) in &searches {
                if seen.record.check(search, request.matching).is_some() {
                    *counts.searches.entry(name.to_string()).or_default() += 1;
                }
            }
            if !request.kinds.is_empty() && !request.kinds.contains(&item.kind) {
                continue;
            }
            if let Some(marks) = seen.record.check(&query, request.matching) {
                rows.push((newest(item), self.row(item, seen, &marks)));
            }
        }
        match request.sort {
            Sort::Newest => rows.sort_by_key(|r| std::cmp::Reverse(r.0)),
            Sort::Score => rows.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then(b.0.cmp(&a.0))),
        }
        let rows = rows.into_iter().map(|(_, row)| row).collect();
        View { rows, counts }
    }

    /// An item alone, as the details show it: its texts marked where the
    /// query matched (if it does) and its interests' terms; by its id or an alias.
    pub fn row_of(&self, id: &str, query: &str, matching: Matching) -> Option<Row> {
        let item = self.item(id)?;
        let seen = self.seen(item, Vec::new());
        let marks = seen
            .record
            .check(&parse(query), matching)
            .unwrap_or_default();
        Some(self.row(item, seen, &marks))
    }

    /// An item shown: the query's marks, its interests' terms marked too.
    fn row(&self, item: &Item, seen: Seen, marks: &Marks) -> Row {
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
        interests.sort_by(|a, b| b.score.total_cmp(&a.score));
        Row {
            id: item.id.clone(),
            score: best(&seen.scores),
            interests,
            title,
            authors,
            r#abstract,
        }
    }
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
