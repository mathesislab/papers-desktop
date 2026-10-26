//! In-memory search over the library. Every query word must match a field
//! (author, year, key, title, keyword, journal or DOI); the fields that
//! identify a paper best score highest, and the first author above the others.

use std::collections::HashMap;

use deunicode::deunicode;

use crate::{Item, Result, csl};

pub struct Library {
    items: Vec<Item>,
    fields: Vec<Fields>,
    by_key: HashMap<String, usize>,
}

impl Library {
    pub fn new(items: Vec<Item>) -> Library {
        let fields = items.iter().map(Fields::new).collect();
        let by_key = items
            .iter()
            .enumerate()
            .map(|(i, it)| (it.key.clone(), i))
            .collect();
        Library {
            items,
            fields,
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

    /// The best `limit` matches, best first; newer papers first among equals.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&Item> {
        let words = query_words(query);
        if words.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<(u32, usize)> = self
            .fields
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                words
                    .iter()
                    .try_fold(0, |sum, w| match f.score(w) {
                        0 => None,
                        s => Some(sum + s),
                    })
                    .map(|score| (score, i))
            })
            .collect();
        hits.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| self.items[b.1].year.cmp(&self.items[a.1].year))
        });
        hits.into_iter()
            .take(limit)
            .map(|(_, i)| &self.items[i])
            .collect()
    }
}

/// The searchable text of an item, folded to lowercase ASCII.
struct Fields {
    key: String,
    /// the words of each family name, in author order
    authors: Vec<Vec<String>>,
    year: Option<String>,
    title: String,
    title_words: Vec<String>,
    keyword_words: Vec<String>,
    container_words: Vec<String>,
    doi: String,
}

impl Fields {
    fn new(item: &Item) -> Fields {
        let title = fold(&item.title);
        Fields {
            key: fold(&item.key),
            authors: item.families.iter().map(|f| words(&fold(f))).collect(),
            year: item.year.map(|y| y.to_string()),
            title_words: words(&title),
            title,
            keyword_words: item.keywords.iter().flat_map(|k| words(&fold(k))).collect(),
            container_words: words(&fold(item.container.as_deref().unwrap_or_default())),
            doi: fold(item.doi.as_deref().unwrap_or_default()),
        }
    }

    /// How well one query word matches the item, 0 for not at all.
    fn score(&self, w: &str) -> u32 {
        if w.contains('/') || w.starts_with("10.") {
            // a DOI, or a key made of a DOI
            return if self.doi.contains(w) || self.key.contains(w) {
                50
            } else {
                0
            };
        }
        let mut best = 0;
        let mut take = |s: u32| best = best.max(s);
        if self.key == w {
            take(50);
        } else if self.key.starts_with(w) {
            take(35);
        } else if self.key.contains(w) {
            take(15);
        }
        for (i, family) in self.authors.iter().enumerate() {
            let first = if i == 0 { 15 } else { 0 };
            if family.iter().any(|f| f == w) {
                take(40 + first);
            } else if family.iter().any(|f| f.starts_with(w)) {
                take(30 + first);
            }
        }
        if self.year.as_deref() == Some(w) {
            take(35);
        }
        if self.title_words.iter().any(|t| t == w) {
            take(25);
        } else if self.title_words.iter().any(|t| t.starts_with(w)) {
            take(18);
        } else if w.len() >= 3 && self.title.contains(w) {
            take(8);
        }
        if self.keyword_words.iter().any(|k| k == w) {
            take(25);
        } else if self.keyword_words.iter().any(|k| k.starts_with(w)) {
            take(15);
        }
        if self.container_words.iter().any(|c| c == w) {
            take(12);
        } else if self.container_words.iter().any(|c| c.starts_with(w)) {
            take(9);
        }
        // a DOI fragment, e.g. "nclimate1449": not a plain word like "climate"
        if w.len() >= 5 && w.bytes().any(|b| b.is_ascii_digit()) && self.doi.contains(w) {
            take(20);
        }
        best
    }
}

/// The words of a query; a DOI (with a slash, or starting with "10.") is kept
/// whole, and a pasted https://doi.org/ link is reduced to its DOI.
fn query_words(query: &str) -> Vec<String> {
    fold(query)
        .split_whitespace()
        .flat_map(|t| match t.rsplit_once("doi.org/") {
            Some((_, doi)) => vec![doi.to_string()],
            None if t.contains('/') || t.starts_with("10.") => vec![t.to_string()],
            None => words(t),
        })
        .collect()
}

fn fold(s: &str) -> String {
    deunicode(s).to_lowercase()
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}
