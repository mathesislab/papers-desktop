//! Search the library: every term of the query must match, and the matches
//! come newest first. Each hit marks the words that matched, to show them.

use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap};

use serde::Serialize;

use crate::query::{self, Field, Pattern, Term};
use crate::text::{Matching, Span, Text, fold};
use crate::{Item, Result, csl};

pub struct Library {
    items: Vec<Item>,
    fields: Vec<Fields>,
    /// the items newest first, then by first author and title
    order: Vec<usize>,
    by_key: HashMap<String, usize>,
}

/// The matches of a search, the first `limit` of them with their marks.
#[derive(Debug, Clone, Serialize)]
pub struct Found {
    pub hits: Vec<Hit>,
    pub total: usize,
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
    pub fn new(items: Vec<Item>) -> Library {
        let fields = items.iter().map(Fields::new).collect();
        let mut order: Vec<usize> = (0..items.len()).collect();
        order.sort_by_cached_key(|&i| {
            let it = &items[i];
            let first = it.families.first().map(|f| fold(f)).unwrap_or_default();
            (Reverse(it.year), first, fold(&it.title))
        });
        let by_key = items
            .iter()
            .enumerate()
            .map(|(i, it)| (it.key.clone(), i))
            .collect();
        Library {
            items,
            fields,
            order,
            by_key,
        }
    }

    pub fn from_csl_json(json: &str) -> Result<Library> {
        let entries = csl::parse(json)?;
        Ok(Library::new(
            entries.into_iter().map(Item::from_csl).collect(),
        ))
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&Item> {
        self.by_key.get(key).map(|&i| &self.items[i])
    }

    pub fn search(&self, query: &str, matching: Matching, limit: usize) -> Found {
        let terms = query::parse(query);
        if terms.is_empty() {
            return Found {
                hits: Vec::new(),
                total: 0,
            };
        }
        let found: Vec<(usize, Marks)> = self
            .order
            .iter()
            .filter_map(|&i| {
                let mut marks = Marks::default();
                let all = terms
                    .iter()
                    .all(|t| self.fields[i].matches(t, matching, &mut marks));
                all.then_some((i, marks))
            })
            .collect();
        let total = found.len();
        let hits = found
            .into_iter()
            .take(limit)
            .map(|(i, marks)| self.hit(i, &marks))
            .collect();
        Found { hits, total }
    }

    fn hit(&self, i: usize, marks: &Marks) -> Hit {
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
    key: String,
    key_words: Text,
    doi: String,
}

impl Fields {
    fn new(item: &Item) -> Fields {
        Fields {
            title: Text::new(&item.title),
            authors: item.families.iter().map(|f| Text::new(f)).collect(),
            year: item.year,
            container: Text::new(item.container.as_deref().unwrap_or_default()),
            keywords: item.keywords.iter().map(|k| Text::new(k)).collect(),
            key: fold(&item.key),
            key_words: Text::new(&item.key),
            doi: fold(item.doi.as_deref().unwrap_or_default()),
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
            (Pattern::Years(from, to), _) => {
                let within = |y: i32| from.is_none_or(|f| y >= f) && to.is_none_or(|t| y <= t);
                self.mark_year(marks, self.year.is_some_and(within))
            }
            (Pattern::YearStart(start), _) => {
                let starts = self
                    .year
                    .is_some_and(|y| y.to_string().starts_with(start.as_str()));
                self.mark_year(marks, starts)
            }
            (Pattern::Word(w), Field::Key) => {
                self.key.starts_with(w.as_str()) || !self.key_words.find(w, matching).is_empty()
            }
            (_, Field::Title) => mark(&mut marks.title, find(&self.title)),
            (_, Field::Journal) => mark(&mut marks.container, find(&self.container)),
            (_, Field::Tag) => self.keywords.iter().any(|k| !find(k).is_empty()),
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
