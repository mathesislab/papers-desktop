//! The library as the main window shows it: a row per item, the tags, and
//! the items that may be the same paper.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bib;
use crate::item::Item;
use crate::search::Library;
use crate::tag::branch;
use crate::text::{fold, words};
use crate::write::given_entries;
use crate::{Error, Result};

/// Whether an item links a file, and whether the files are there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileState {
    None,
    Found,
    /// one of the files linked is not there
    Missing,
}

/// An item as a row of the table.
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub id: String,
    pub key: String,
    /// bibtex entry type, e.g. `article`
    pub kind: String,
    /// family names: "Robinson", "Robinson & Calov", "Robinson et al."
    pub authors: String,
    pub year: Option<i32>,
    pub title: String,
    /// journal or book title
    pub container: Option<String>,
    pub tags: Vec<String>,
    pub doi: Option<String>,
    pub file: FileState,
    /// the .bib file of the entry
    pub bib: PathBuf,
}

/// A tag, and how many items have it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagCount {
    pub tag: String,
    /// the items that have the tag itself
    pub count: usize,
    /// the items that have the tag or a tag under it, each once
    pub under: usize,
}

impl TagCount {
    fn new(tag: &str) -> TagCount {
        TagCount {
            tag: tag.to_string(),
            count: 0,
            under: 0,
        }
    }
}

impl Library {
    /// The rows of all items, newest first. The linked files are looked for
    /// now: they change without the .bib.
    pub fn rows(&self) -> Vec<Row> {
        self.ordered().map(row).collect()
    }

    /// The tags in alphabetical order, a tag before the tags under it, with
    /// the number of items of each. A parent that no item has itself comes
    /// too, with a count of 0.
    pub fn tags(&self) -> Vec<TagCount> {
        // a tag written differently ("AMOC", "amoc") is one tag, named as
        // written first
        let mut tags: BTreeMap<String, TagCount> = BTreeMap::new();
        for item in self.ordered() {
            let mut seen = BTreeSet::new();
            for tag in &item.keywords {
                if seen.insert(fold(tag)) {
                    tags.entry(fold(tag))
                        .or_insert_with(|| TagCount::new(tag))
                        .count += 1;
                }
            }
        }
        // after the tags written, whose spelling a parent must not take
        for item in self.ordered() {
            let mut seen = BTreeSet::new();
            for tag in &item.keywords {
                for name in branch(tag) {
                    if seen.insert(fold(name)) {
                        tags.entry(fold(name))
                            .or_insert_with(|| TagCount::new(name))
                            .under += 1;
                    }
                }
            }
        }
        let mut tags: Vec<TagCount> = tags.into_values().collect();
        // by the parts between the `/`: "climate-review" sorts before
        // "climate/amoc" as a string, but the tags under "climate" come together
        tags.sort_by_cached_key(|t| {
            fold(&t.tag)
                .split('/')
                .map(String::from)
                .collect::<Vec<_>>()
        });
        tags
    }

    /// The items that may be the same paper, in groups: those with the same
    /// DOI, or with the same title and year. The groups come newest first,
    /// and so do the items of a group.
    pub fn duplicates(&self) -> Vec<Vec<String>> {
        let items: Vec<&Item> = self.ordered().collect();
        let mut groups = Groups::new(items.len());
        let mut doi: HashMap<String, usize> = HashMap::new();
        let mut title: HashMap<String, usize> = HashMap::new();
        for (i, item) in items.iter().enumerate() {
            let Identity { doi: d, title: t } = item.identity();
            if let Some(d) = d {
                let first = *doi.entry(d).or_insert(i);
                groups.join(first, i);
            }
            if let Some(t) = t {
                let first = *title.entry(t).or_insert(i);
                groups.join(first, i);
            }
        }
        let mut members: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (i, item) in items.iter().enumerate() {
            members
                .entry(groups.root(i))
                .or_default()
                .push(item.id.clone());
        }
        let mut found: Vec<(usize, Vec<String>)> = members
            .into_iter()
            .filter(|(_, ids)| ids.len() > 1)
            .collect();
        // a group comes where its newest item does
        found.sort_by_key(|(root, _)| groups.first[*root]);
        found.into_iter().map(|(_, ids)| ids).collect()
    }

    /// The entries of a BibTeX text to add, each with the items of the
    /// library that are the same paper: with the same DOI, or the same title
    /// and year. The text must parse, as for `add_entries`.
    pub fn incoming(&self, text: &str) -> Result<Vec<Incoming>> {
        let text = text.trim();
        if given_entries(text)?.is_empty() {
            return Err(Error::NoEntry);
        }
        let entries = bib::parse(Path::new(""), text).map_err(|(at, message)| Error::Bibtex {
            line: text[..at.min(text.len())].matches('\n').count() + 1,
            message,
        })?;
        let mut doi: HashMap<String, Vec<&Item>> = HashMap::new();
        let mut title: HashMap<String, Vec<&Item>> = HashMap::new();
        for item in self.ordered() {
            let Identity { doi: d, title: t } = item.identity();
            if let Some(d) = d {
                doi.entry(d).or_default().push(item);
            }
            if let Some(t) = t {
                title.entry(t).or_default().push(item);
            }
        }
        Ok(entries
            .iter()
            .map(|e| {
                let new = Item::from_entry(e, String::new());
                let identity = new.identity();
                let by_doi = identity.doi.and_then(|d| doi.get(&d)).into_iter();
                let by_title = identity.title.and_then(|t| title.get(&t));
                let mut same: Vec<String> = Vec::new();
                for item in by_doi.chain(by_title).flatten() {
                    if !same.contains(&item.id) {
                        same.push(item.id.clone());
                    }
                }
                Incoming {
                    key: new.key.clone(),
                    same,
                    item: new,
                }
            })
            .collect())
    }
}

/// An entry of a BibTeX text to add, and the items that are the same paper.
#[derive(Debug, Clone, Serialize)]
pub struct Incoming {
    pub key: String,
    /// the ids of the items of the library, newest first; none: a new paper
    pub same: Vec<String>,
    /// the entry as the library would show it (its `id` is empty)
    pub item: Item,
}

/// What tells a paper apart, as the possible duplicates and the papers to add are compared: the
/// same DOI, or the same title and year, is the same paper. It stays when the key changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// in lower case
    pub doi: Option<String>,
    /// the words of the title (case, accents, punctuation and white space are gone), and the year:
    /// `ice sheet stability (2019)`; none without a title
    pub title: Option<String>,
}

impl Item {
    pub fn identity(&self) -> Identity {
        let words = words(&fold(&self.title)).join(" ");
        Identity {
            doi: self.doi.as_ref().map(|d| d.to_lowercase()),
            title: (!words.is_empty()).then(|| match self.year {
                Some(year) => format!("{words} ({year})"),
                None => words,
            }),
        }
    }
}

impl Identity {
    /// Whether it is the same paper: the same DOI, or the same title and year.
    pub fn same_paper(&self, other: &Identity) -> bool {
        let same = |a: &Option<String>, b: &Option<String>| a.is_some() && a == b;
        same(&self.doi, &other.doi) || same(&self.title, &other.title)
    }
}

fn row(item: &Item) -> Row {
    let file = if item.files.is_empty() {
        FileState::None
    } else if item.files.iter().all(|f| f.exists()) {
        FileState::Found
    } else {
        FileState::Missing
    };
    Row {
        id: item.id.clone(),
        key: item.key.clone(),
        kind: item.kind.clone(),
        authors: short_names(&item.families),
        year: item.year,
        title: item.title.clone(),
        container: item.container.clone(),
        tags: item.keywords.clone(),
        doi: item.doi.clone(),
        file,
        bib: item.bib.clone(),
    }
}

/// The authors as the subtitle of the panel names them.
fn short_names(families: &[String]) -> String {
    match families {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} & {second}"),
        [first, ..] => format!("{first} et al."),
    }
}

/// Items joined in groups (union-find), each group under the item of its
/// root, which knows the first item of the group.
struct Groups {
    parent: Vec<usize>,
    /// of a root: its first item
    first: Vec<usize>,
}

impl Groups {
    fn new(n: usize) -> Groups {
        Groups {
            parent: (0..n).collect(),
            first: (0..n).collect(),
        }
    }

    fn root(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.root(a), self.root(b));
        if a != b {
            self.parent[b] = a;
            self.first[a] = self.first[a].min(self.first[b]);
        }
    }
}
