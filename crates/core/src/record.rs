//! The searchable texts of anything a query finds, a library entry or a feed
//! item, made from plain data: the title, authors, year, journal, abstract,
//! keywords, key, DOI, the `has:` names, and the facets and numbers that a
//! [`Schema`](crate::Schema) declares.

use std::collections::{BTreeMap, BTreeSet};

use crate::query::{Field, Pattern, Query, Term};
use crate::text::{Matching, Span, Text, fold};

/// The searchable texts of an entry or an item.
#[derive(Debug)]
pub struct Record {
    pub(crate) title: Text,
    /// the family name of each author
    pub(crate) authors: Vec<Text>,
    pub(crate) year: Option<i32>,
    /// journal or book title
    pub(crate) container: Text,
    r#abstract: Text,
    keywords: Vec<Text>,
    /// the keywords, folded, to match as whole tags
    tags: Vec<String>,
    pub(crate) key: String,
    key_words: Text,
    pub(crate) doi: String,
    has: BTreeSet<String>,
    /// the folded values of each facet, by its lowercase name
    facets: BTreeMap<String, BTreeSet<String>>,
    /// by lowercase name
    numbers: BTreeMap<String, f64>,
}

/// The words of a record that matched, as indices of the words of its texts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    pub title: BTreeSet<usize>,
    /// (author, word)
    pub authors: BTreeSet<(usize, usize)>,
    pub year: bool,
    pub container: BTreeSet<usize>,
    pub r#abstract: BTreeSet<usize>,
}

impl Record {
    /// A record of a title, the other texts added to it.
    pub fn new(title: &str) -> Record {
        Record {
            title: Text::new(title),
            authors: Vec::new(),
            year: None,
            container: Text::new(""),
            r#abstract: Text::new(""),
            keywords: Vec::new(),
            tags: Vec::new(),
            key: String::new(),
            key_words: Text::new(""),
            doi: String::new(),
            has: BTreeSet::new(),
            facets: BTreeMap::new(),
            numbers: BTreeMap::new(),
        }
    }

    /// The family names of the authors, in order: `first:` takes the first.
    pub fn authors<S: AsRef<str>>(mut self, families: impl IntoIterator<Item = S>) -> Record {
        let families = families.into_iter();
        self.authors.extend(families.map(|f| Text::new(f.as_ref())));
        self
    }

    /// The year: `y:2010-2015`, or a plain word `2012`.
    pub fn year(mut self, year: impl Into<Option<i32>>) -> Record {
        self.year = year.into();
        self
    }

    /// The journal or book title.
    pub fn container(mut self, container: &str) -> Record {
        self.container = Text::new(container);
        self
    }

    /// The abstract, which `abs:` searches and plain words do not.
    pub fn r#abstract(mut self, text: &str) -> Record {
        self.r#abstract = Text::new(text);
        self
    }

    /// Keywords: their words for plain words, each whole for `tag:`.
    pub fn keywords<S: AsRef<str>>(mut self, keywords: impl IntoIterator<Item = S>) -> Record {
        for k in keywords {
            self.keywords.push(Text::new(k.as_ref()));
            self.tags.push(fold(k.as_ref()));
        }
        self
    }

    /// The key: `key:` takes its start or one of its words.
    pub fn key(mut self, key: &str) -> Record {
        self.key = fold(key);
        self.key_words = Text::new(key);
        self
    }

    /// The DOI: `doi:`, or a plain word that is a DOI or part of one.
    pub fn doi(mut self, doi: &str) -> Record {
        self.doi = fold(doi);
        self
    }

    /// Names that `has:` takes: `has:abstract`, `has:pdf`.
    pub fn has<S: AsRef<str>>(mut self, names: impl IntoIterator<Item = S>) -> Record {
        self.has.extend(names.into_iter().map(|n| fold(n.as_ref())));
        self
    }

    /// Values of a facet: `kind` and `paper`, for `kind:paper`.
    pub fn facet<S: AsRef<str>>(
        mut self,
        name: &str,
        values: impl IntoIterator<Item = S>,
    ) -> Record {
        let facet = self.facets.entry(name.to_ascii_lowercase()).or_default();
        facet.extend(values.into_iter().map(|v| fold(v.as_ref())));
        self
    }

    /// A number: `score` and 3.5, for `score:3-`.
    pub fn number(mut self, name: &str, value: f64) -> Record {
        self.numbers.insert(name.to_ascii_lowercase(), value);
        self
    }

    /// The marks of a query that matches the record, every term of it; none
    /// when a term does not.
    pub fn check(&self, query: &Query, matching: Matching) -> Option<Marks> {
        let mut marks = Marks::default();
        let mut terms = query.terms.iter();
        terms
            .all(|t| self.passes(t, matching, &mut marks))
            .then_some(marks)
    }

    /// The title in spans, the matched words marked.
    pub fn title_spans(&self, marks: &Marks) -> Vec<Span> {
        self.title.spans(&marks.title)
    }

    /// The family name of author `a` in spans, the matched words marked.
    pub fn author_spans(&self, a: usize, marks: &Marks) -> Vec<Span> {
        let words = marks.authors.iter().filter(|(x, _)| *x == a);
        self.authors[a].spans(&words.map(|(_, w)| *w).collect())
    }

    /// The journal or book title in spans, the matched words marked.
    pub fn container_spans(&self, marks: &Marks) -> Vec<Span> {
        self.container.spans(&marks.container)
    }

    /// The abstract in spans, the matched words marked.
    pub fn abstract_spans(&self, marks: &Marks) -> Vec<Span> {
        self.r#abstract.spans(&marks.r#abstract)
    }

    /// Whether the record passes a term: matches it, or does not match an
    /// excluded term (whose words are not marked).
    pub(crate) fn passes(&self, term: &Term, matching: Matching, marks: &mut Marks) -> bool {
        if term.negated {
            !self.matches(term, matching, &mut Marks::default())
        } else {
            self.matches(term, matching, marks)
        }
    }

    /// Whether a term matches, any of its patterns, marking the words of every
    /// one that matched.
    fn matches(&self, term: &Term, matching: Matching, marks: &mut Marks) -> bool {
        let mut found = false;
        for pattern in &term.patterns {
            found |= self.takes(&term.field, pattern, matching, marks);
        }
        found
    }

    /// Whether a pattern of a field matches, marking the words it matched.
    fn takes(
        &self,
        field: &Field,
        pattern: &Pattern,
        matching: Matching,
        marks: &mut Marks,
    ) -> bool {
        let find = |text: &Text| match pattern {
            Pattern::Word(w) => text.find(w, matching),
            Pattern::Phrase(ws) => text.find_phrase(ws, matching),
            _ => Vec::new(),
        };
        match (pattern, field) {
            (Pattern::Doi(doi), _) => {
                self.doi.contains(doi.as_str()) || self.key.contains(doi.as_str())
            }
            (Pattern::Years(..) | Pattern::YearStart(_), _) => {
                let matched = self.year.is_some_and(|y| pattern.matches_year(y));
                self.mark_year(marks, matched)
            }
            (Pattern::Tag(_), Field::Facet(name)) => self
                .facets
                .get(name)
                .is_some_and(|values| values.iter().any(|v| pattern.matches_whole(v))),
            (Pattern::Tag(_), _) => self.tags.iter().any(|t| pattern.matches_tag(t)),
            (Pattern::Numbers(..), Field::Number(name)) => self
                .numbers
                .get(name)
                .is_some_and(|&n| pattern.matches_number(n)),
            (Pattern::Word(w), Field::Key) => {
                self.key.starts_with(w.as_str()) || !self.key_words.find(w, matching).is_empty()
            }
            (_, Field::Title) => mark(&mut marks.title, find(&self.title)),
            (_, Field::Journal) => mark(&mut marks.container, find(&self.container)),
            (_, Field::Abstract) => mark(&mut marks.r#abstract, find(&self.r#abstract)),
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

/// Mark the words: whether there are any.
pub(crate) fn mark(marked: &mut BTreeSet<usize>, words: Vec<usize>) -> bool {
    let found = !words.is_empty();
    marked.extend(words);
    found
}
