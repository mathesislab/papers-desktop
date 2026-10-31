//! Search the library: every term of the query must match (or must not, for
//! an excluded term), and the matches come newest first. Or find the entries
//! closest to a reference. Each hit marks the words that matched, to show them.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::bib::{self, BibEntry, Snapshot};
use crate::fields::field;
use crate::query::{self, Field, Query, Schema, Term};
use crate::record::{Marks, Record, mark};
use crate::reference::Reference;
use crate::text::{Matching, Span, fold};
use crate::{Item, Result};

/// What a word of a reference counts for, in the entry most like it.
const FIRST_AUTHOR: f32 = 4.0;
const AUTHOR: f32 = 3.0;
const YEAR: f32 = 2.0;
const TITLE: f32 = 1.0;
const JOURNAL: f32 = 0.5;
/// An entry closest to a reference counts at least this, and half of the best.
const CLOSE: f32 = 3.0;

#[derive(Default)]
pub struct Library {
    items: Vec<Item>,
    records: Vec<Record>,
    /// the items newest first, then by first author and title
    order: Vec<usize>,
    by_id: HashMap<String, usize>,
    /// the entry of each item, as parsed and as written
    entries: Vec<BibEntry>,
    bibs: Vec<PathBuf>,
    /// each .bib file as read, for a write to check it is still the same
    snapshots: HashMap<PathBuf, Snapshot>,
}

/// The matches of a search, the first `limit` of them with their marks.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Found {
    pub hits: Vec<Hit>,
    pub total: usize,
    /// the matches of each year, but for the year filters of the query:
    /// from the first year to the last, the years without a match included
    pub years: Vec<YearCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct YearCount {
    pub year: i32,
    pub count: usize,
    /// whether the year filters of the query take this year
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub item: Item,
    pub title: Vec<Span>,
    pub subtitle: Vec<Span>,
    /// for each author, whether it matched
    pub authors: Vec<bool>,
}

impl Library {
    /// The entries of these .bib files, in order.
    pub fn load(bibs: &[PathBuf]) -> Result<Library> {
        let mut entries = Vec::new();
        let mut snapshots = HashMap::new();
        for bib in bibs {
            let file = bib::read(bib)?;
            entries.extend(file.entries);
            snapshots.insert(bib.clone(), file.snapshot);
        }
        // the key is the id, numbered when several entries have it
        let mut count: HashMap<String, usize> = HashMap::new();
        for e in &entries {
            *count.entry(e.entry.key.clone()).or_default() += 1;
        }
        let mut seen: HashMap<String, usize> = HashMap::new();
        let items = entries
            .iter()
            .map(|e| {
                let key = &e.entry.key;
                let n = seen.entry(key.clone()).or_default();
                *n += 1;
                let id = if count[key] > 1 {
                    format!("{key}#{n}")
                } else {
                    key.clone()
                };
                Item::from_entry(e, id)
            })
            .collect();
        Ok(Library::new(items, entries, bibs.to_vec(), snapshots))
    }

    fn new(
        items: Vec<Item>,
        entries: Vec<BibEntry>,
        bibs: Vec<PathBuf>,
        snapshots: HashMap<PathBuf, Snapshot>,
    ) -> Library {
        let records = items.iter().zip(&entries).map(record).collect();
        let mut order: Vec<usize> = (0..items.len()).collect();
        order.sort_by_cached_key(|&i| {
            let it = &items[i];
            let first = it.families.first().map(|f| fold(f)).unwrap_or_default();
            (Reverse(it.year), first, fold(&it.title))
        });
        let by_id = items
            .iter()
            .enumerate()
            .map(|(i, it)| (it.id.clone(), i))
            .collect();
        Library {
            items,
            records,
            order,
            by_id,
            entries,
            bibs,
            snapshots,
        }
    }

    /// The .bib files of the library.
    pub fn bibs(&self) -> &[PathBuf] {
        &self.bibs
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The items, in the order of the files.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn get(&self, id: &str) -> Option<&Item> {
        self.by_id.get(id).map(|&i| &self.items[i])
    }

    /// The item of key `key` in .bib file `bib`, which stays the same when
    /// the ids change with the files of the library.
    pub fn find(&self, bib: &Path, key: &str) -> Option<&Item> {
        self.items.iter().find(|it| it.bib == bib && it.key == key)
    }

    /// An item as a hit without marks: one not found by a search.
    pub fn hit(&self, id: &str) -> Option<Hit> {
        self.by_id
            .get(id)
            .map(|&i| self.marked_hit(i, &Marks::default()))
    }

    /// The BibTeX of an item, formatted.
    pub fn bibtex(&self, id: &str) -> Option<String> {
        let e = self.entry(id)?;
        Some(e.entry.to_bibtex_string().unwrap_or_else(|_| e.raw.clone()))
    }

    /// The BibTeX of items, formatted, one after another: a .bib file of
    /// their own (the strings expanded).
    pub fn export(&self, ids: &[String]) -> String {
        let entries: Vec<String> = ids.iter().filter_map(|id| self.bibtex(id)).collect();
        entries.join("\n\n") + "\n"
    }

    /// The BibTeX of an item as written in its .bib file.
    pub fn raw(&self, id: &str) -> Option<&str> {
        self.entry(id).map(|e| e.raw.as_str())
    }

    /// The entry of an item, as parsed and as written.
    pub(crate) fn entry(&self, id: &str) -> Option<&BibEntry> {
        self.by_id.get(id).map(|&i| &self.entries[i])
    }

    /// The entries of a .bib file of the library.
    pub(crate) fn entries_in(&self, bib: &Path) -> impl Iterator<Item = &BibEntry> {
        self.entries.iter().filter(move |e| e.bib == bib)
    }

    /// A .bib file of the library as it was read.
    pub(crate) fn snapshot(&self, bib: &Path) -> Option<Snapshot> {
        self.snapshots.get(bib).copied()
    }

    /// The items, newest first.
    pub(crate) fn ordered(&self) -> impl Iterator<Item = &Item> {
        self.order.iter().map(|&i| &self.items[i])
    }

    /// The ids of the items that match a query, newest first: all the items
    /// for an empty query.
    pub fn matching(&self, query: &str, matching: Matching) -> Vec<String> {
        let query = Query::parse(query, Schema::default());
        self.order
            .iter()
            .filter(|&&i| self.records[i].check(&query, matching).is_some())
            .map(|&i| self.items[i].id.clone())
            .collect()
    }

    pub fn search(&self, query: &str, matching: Matching, limit: usize) -> Found {
        let terms = query::parse(query);
        if terms.is_empty() {
            return Found::default();
        }
        // the years are counted before the year filters apply
        let (year_terms, terms): (Vec<&Term>, Vec<&Term>) =
            terms.iter().partition(|t| t.field == Field::Year);
        let mut counts = BTreeMap::new();
        let mut found = Vec::new();
        for &i in &self.order {
            let record = &self.records[i];
            let mut marks = Marks::default();
            if !terms.iter().all(|t| record.passes(t, matching, &mut marks)) {
                continue;
            }
            if let Some(year) = self.items[i].year {
                *counts.entry(year).or_insert(0) += 1;
            }
            if year_terms
                .iter()
                .all(|t| record.passes(t, matching, &mut marks))
            {
                found.push((i, marks));
            }
        }
        let selected = |year| {
            year_terms
                .iter()
                .all(|t| t.patterns.iter().any(|p| p.matches_year(year)) != t.negated)
        };
        let years = match (counts.keys().next(), counts.keys().next_back()) {
            (Some(&first), Some(&last)) => (first..=last)
                .map(|year| YearCount {
                    year,
                    count: counts.get(&year).copied().unwrap_or(0),
                    selected: selected(year),
                })
                .collect(),
            _ => Vec::new(),
        };
        Found {
            total: found.len(),
            hits: found
                .into_iter()
                .take(limit)
                .map(|(i, marks)| self.marked_hit(i, &marks))
                .collect(),
            years,
        }
    }

    /// The entries closest to a reference, the best first: the entry of its
    /// DOI, else those that have most of its words, names counting most.
    pub fn closest(&self, reference: &str, limit: usize) -> Found {
        let reference = Reference::parse(reference);
        let entry_of_doi = reference.doi.as_ref().and_then(|doi| {
            let mut records = self.records.iter();
            records.position(|r| r.doi == *doi || r.key == *doi)
        });
        let mut close: Vec<(f32, usize, Marks)> = match entry_of_doi {
            Some(i) => vec![(0.0, i, Marks::default())],
            None => self
                .order
                .iter()
                .map(|&i| {
                    let mut marks = Marks::default();
                    (likeness(&self.records[i], &reference, &mut marks), i, marks)
                })
                .collect(),
        };
        if entry_of_doi.is_none() {
            // stable: the equally close ones newest first
            close.sort_by(|a, b| b.0.total_cmp(&a.0));
            let best = close.first().map_or(0.0, |c| c.0);
            close.retain(|c| c.0 >= CLOSE && c.0 >= best / 2.0);
        }
        Found {
            total: close.len(),
            hits: close
                .into_iter()
                .take(limit)
                .map(|(_, i, marks)| self.marked_hit(i, &marks))
                .collect(),
            years: Vec::new(),
        }
    }

    fn marked_hit(&self, i: usize, marks: &Marks) -> Hit {
        let (item, record) = (&self.items[i], &self.records[i]);
        let authors = (0..item.authors.len()).map(|a| marks.authors.iter().any(|&(x, _)| x == a));
        Hit {
            title: record.title_spans(marks),
            subtitle: item.subtitle_spans(marks),
            authors: authors.collect(),
            item: item.clone(),
        }
    }
}

/// The record of an item, its abstract from its entry, and what it has: the
/// fields of the entry, and "pdf" with a linked file.
fn record((item, entry): (&Item, &BibEntry)) -> Record {
    let pdf = (!item.files.is_empty()).then_some("pdf");
    Record::new(&item.title)
        .authors(&item.families)
        .year(item.year)
        .container(item.container.as_deref().unwrap_or_default())
        .r#abstract(&field(&entry.entry, &["abstract"]).unwrap_or_default())
        .keywords(&item.keywords)
        .key(&item.key)
        .doi(item.doi.as_deref().unwrap_or_default())
        .has(entry.entry.fields.keys().map(String::as_str).chain(pdf))
}

/// How much a record is like a reference: each word of the reference counts
/// once, for what it is in the record (an author, a word of the title or
/// journal), and so does the year.
fn likeness(record: &Record, reference: &Reference, marks: &mut Marks) -> f32 {
    let matching = Matching::WordStart;
    let mut score = 0.0;
    for w in &reference.words {
        let mut counts: f32 = 0.0;
        for (a, author) in record.authors.iter().enumerate() {
            let words = author.find(w, matching);
            if !words.is_empty() {
                counts = counts.max(if a == 0 { FIRST_AUTHOR } else { AUTHOR });
                marks
                    .authors
                    .extend(words.into_iter().map(|word| (a, word)));
            }
        }
        if mark(&mut marks.title, record.title.find(w, matching)) {
            counts = counts.max(TITLE);
        }
        if mark(&mut marks.container, record.container.find(w, matching)) {
            counts = counts.max(JOURNAL);
        }
        score += counts;
    }
    if record.year.is_some_and(|y| reference.years.contains(&y)) {
        marks.year = true;
        score += YEAR;
    }
    score
}
