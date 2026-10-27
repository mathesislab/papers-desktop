//! Search the library: every term of the query must match (or must not, for
//! an excluded term), and the matches come newest first. Or find the entries
//! closest to a reference. Each hit marks the words that matched, to show them.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::bib::{self, BibEntry};
use crate::query::{self, Field, Pattern, Term};
use crate::reference::Reference;
use crate::text::{Matching, Span, Text, fold};
use crate::{Item, Result, to_csl};

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
    fields: Vec<Fields>,
    /// the items newest first, then by first author and title
    order: Vec<usize>,
    by_id: HashMap<String, usize>,
    /// the entry of each item, as parsed and as written
    entries: Vec<BibEntry>,
    bibs: Vec<PathBuf>,
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

/// The words of an item that matched, as indices of the words of its texts.
#[derive(Debug, Default)]
pub(crate) struct Marks {
    pub title: BTreeSet<usize>,
    /// (author, word)
    pub authors: BTreeSet<(usize, usize)>,
    pub year: bool,
    pub container: BTreeSet<usize>,
}

impl Library {
    /// The entries of these .bib files, in order.
    pub fn load(bibs: &[PathBuf]) -> Result<Library> {
        let mut entries = Vec::new();
        for bib in bibs {
            entries.extend(bib::read(bib)?);
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
        Ok(Library::new(items, entries, bibs.to_vec()))
    }

    fn new(items: Vec<Item>, entries: Vec<BibEntry>, bibs: Vec<PathBuf>) -> Library {
        let fields = items.iter().zip(&entries).map(Fields::new).collect();
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
            fields,
            order,
            by_id,
            entries,
            bibs,
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

    /// The CSL-JSON of an item, for a citation processor.
    pub fn csl(&self, id: &str) -> Option<Value> {
        self.entry(id).map(|e| to_csl::entry_to_csl(&e.entry))
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

    fn entry(&self, id: &str) -> Option<&BibEntry> {
        self.by_id.get(id).map(|&i| &self.entries[i])
    }

    /// The items, newest first.
    pub(crate) fn ordered(&self) -> impl Iterator<Item = &Item> {
        self.order.iter().map(|&i| &self.items[i])
    }

    /// The ids of the items that match a query, newest first: all the items
    /// for an empty query.
    pub fn matching(&self, query: &str, matching: Matching) -> Vec<String> {
        let terms = query::parse(query);
        self.order
            .iter()
            .filter(|&&i| {
                let mut marks = Marks::default();
                terms
                    .iter()
                    .all(|t| self.fields[i].check(t, matching, &mut marks))
            })
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
            let fields = &self.fields[i];
            let mut marks = Marks::default();
            if !terms.iter().all(|t| fields.check(t, matching, &mut marks)) {
                continue;
            }
            if let Some(year) = self.items[i].year {
                *counts.entry(year).or_insert(0) += 1;
            }
            if year_terms
                .iter()
                .all(|t| fields.check(t, matching, &mut marks))
            {
                found.push((i, marks));
            }
        }
        let selected = |year| {
            year_terms
                .iter()
                .all(|t| t.pattern.matches_year(year) != t.negated)
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
            let mut fields = self.fields.iter();
            fields.position(|f| f.doi == *doi || f.key == *doi)
        });
        let mut close: Vec<(f32, usize, Marks)> = match entry_of_doi {
            Some(i) => vec![(0.0, i, Marks::default())],
            None => self
                .order
                .iter()
                .map(|&i| {
                    let mut marks = Marks::default();
                    (self.fields[i].likeness(&reference, &mut marks), i, marks)
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
        let (item, fields) = (&self.items[i], &self.fields[i]);
        let authors = (0..item.authors.len()).map(|a| marks.authors.iter().any(|&(x, _)| x == a));
        Hit {
            title: fields.title.spans(&marks.title),
            subtitle: item.subtitle_spans(marks),
            authors: authors.collect(),
            item: item.clone(),
        }
    }
}

/// The searchable texts of an item.
struct Fields {
    title: Text,
    /// the family name of each author
    authors: Vec<Text>,
    year: Option<i32>,
    container: Text,
    keywords: Vec<Text>,
    /// the keywords, folded, to match as whole tags
    tags: Vec<String>,
    key: String,
    key_words: Text,
    doi: String,
    /// the names of the fields of the entry, and "pdf" with a linked file
    has: BTreeSet<String>,
}

impl Fields {
    fn new((item, entry): (&Item, &BibEntry)) -> Fields {
        let mut has: BTreeSet<String> = entry.entry.fields.keys().map(|f| fold(f)).collect();
        if !item.files.is_empty() {
            has.insert("pdf".into());
        }
        Fields {
            title: Text::new(&item.title),
            authors: item.families.iter().map(|f| Text::new(f)).collect(),
            year: item.year,
            container: Text::new(item.container.as_deref().unwrap_or_default()),
            keywords: item.keywords.iter().map(|k| Text::new(k)).collect(),
            tags: item.keywords.iter().map(|k| fold(k)).collect(),
            key: fold(&item.key),
            key_words: Text::new(&item.key),
            doi: fold(item.doi.as_deref().unwrap_or_default()),
            has,
        }
    }

    /// Whether an item passes a term: matches it, or does not match an
    /// excluded term (whose words are not marked).
    fn check(&self, term: &Term, matching: Matching, marks: &mut Marks) -> bool {
        if term.negated {
            !self.matches(term, matching, &mut Marks::default())
        } else {
            self.matches(term, matching, marks)
        }
    }

    /// Whether a term matches, marking the words it matched.
    fn matches(&self, term: &Term, matching: Matching, marks: &mut Marks) -> bool {
        let find = |text: &Text| match &term.pattern {
            Pattern::Word(w) => text.find(w, matching),
            Pattern::Phrase(ws) => text.find_phrase(ws, matching),
            _ => Vec::new(),
        };
        match (&term.pattern, term.field) {
            (Pattern::Doi(doi), _) => {
                self.doi.contains(doi.as_str()) || self.key.contains(doi.as_str())
            }
            (Pattern::Years(..) | Pattern::YearStart(_), _) => {
                let matched = self.year.is_some_and(|y| term.pattern.matches_year(y));
                self.mark_year(marks, matched)
            }
            (Pattern::Tag(_), _) => self.tags.iter().any(|t| term.pattern.matches_tag(t)),
            (Pattern::Word(w), Field::Key) => {
                self.key.starts_with(w.as_str()) || !self.key_words.find(w, matching).is_empty()
            }
            (_, Field::Title) => mark(&mut marks.title, find(&self.title)),
            (_, Field::Journal) => mark(&mut marks.container, find(&self.container)),
            (Pattern::Word(w), Field::Has) => self.has.contains(w),
            (_, Field::Author) => self.mark_authors(marks, self.authors.len(), find),
            (_, Field::First) => self.mark_authors(marks, 1, find),
            (pattern, Field::Any) => {
                // every field, to mark every match
                let mut found = mark(&mut marks.title, find(&self.title));
                found |= mark(&mut marks.container, find(&self.container));
                found |= self.mark_authors(marks, self.authors.len(), find);
                found |= self.keywords.iter().any(|k| !find(k).is_empty());
                if let Pattern::Word(w) = pattern {
                    found |= self.key.starts_with(w.as_str());
                    found |= !self.key_words.find(w, matching).is_empty();
                    found |= self.mark_year(marks, self.year.is_some_and(|y| y.to_string() == *w));
                    // a DOI fragment like "nclimate1449", not a word like "climate"
                    found |= w.len() >= 5
                        && w.bytes().any(|b| b.is_ascii_digit())
                        && self.doi.contains(w.as_str());
                }
                found
            }
            _ => false,
        }
    }

    /// How much the entry is like a reference: each word of the reference
    /// counts once, for what it is in the entry (an author, a word of the
    /// title or journal), and so does the year.
    fn likeness(&self, reference: &Reference, marks: &mut Marks) -> f32 {
        let matching = Matching::WordStart;
        let mut score = 0.0;
        for w in &reference.words {
            let mut counts: f32 = 0.0;
            for (a, author) in self.authors.iter().enumerate() {
                let words = author.find(w, matching);
                if !words.is_empty() {
                    counts = counts.max(if a == 0 { FIRST_AUTHOR } else { AUTHOR });
                    marks
                        .authors
                        .extend(words.into_iter().map(|word| (a, word)));
                }
            }
            if mark(&mut marks.title, self.title.find(w, matching)) {
                counts = counts.max(TITLE);
            }
            if mark(&mut marks.container, self.container.find(w, matching)) {
                counts = counts.max(JOURNAL);
            }
            score += counts;
        }
        if self.year.is_some_and(|y| reference.years.contains(&y)) {
            marks.year = true;
            score += YEAR;
        }
        score
    }

    fn mark_year(&self, marks: &mut Marks, matched: bool) -> bool {
        marks.year |= matched;
        matched
    }

    /// Match the first `n` authors.
    fn mark_authors(
        &self,
        marks: &mut Marks,
        n: usize,
        find: impl Fn(&Text) -> Vec<usize>,
    ) -> bool {
        let mut found = false;
        for (a, author) in self.authors.iter().take(n).enumerate() {
            let words = find(author);
            found |= !words.is_empty();
            marks.authors.extend(words.into_iter().map(|w| (a, w)));
        }
        found
    }
}

fn mark(marked: &mut BTreeSet<usize>, words: Vec<usize>) -> bool {
    let found = !words.is_empty();
    marked.extend(words);
    found
}
