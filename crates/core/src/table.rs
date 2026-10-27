//! The library as the main window shows it: a row per item, the tags, and
//! the items that may be the same paper.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use serde::Serialize;

use crate::item::Item;
use crate::search::Library;
use crate::tag::branch;
use crate::text::fold;

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
        let mut title: HashMap<(String, Option<i32>), usize> = HashMap::new();
        for (i, item) in items.iter().enumerate() {
            if let Some(d) = &item.doi {
                let first = *doi.entry(d.to_lowercase()).or_insert(i);
                groups.join(first, i);
            }
            let t = fold(&item.title);
            if !t.is_empty() {
                let first = *title.entry((t, item.year)).or_insert(i);
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
