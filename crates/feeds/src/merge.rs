//! Entries into items: one item per paper or post, from every source that
//! listed it. An entry joins the item of the same feed item, else of its DOI,
//! else of its arXiv id, else of a title of the same words (Jaccard ≥ 0.9) by
//! the same first author within a year; an entry that joins two items merges
//! them, the newer becoming an alias of the older.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Datelike, Utc};

use crate::model::{Entry, Item, Source, Stage, Version};
use crate::normalize::{family, is_preprint_doi, jaccard, short_id, title_words};

/// The least Jaccard index of the title words of one paper.
pub(crate) const SAME_TITLE: f64 = 0.9;

/// The items, indexed by their keys, as entries come in.
pub(crate) struct Merger<'a> {
    items: &'a mut Vec<Item>,
    aliases: &'a mut BTreeMap<String, String>,
    /// absorbed by another item, removed at the end
    gone: Vec<bool>,
    by_doi: HashMap<String, usize>,
    by_arxiv: HashMap<String, usize>,
    by_source: HashMap<(String, String), usize>,
    by_family: HashMap<String, Vec<usize>>,
    /// (absorbed, kept) ids, in order
    merged: Vec<(String, String)>,
}

impl<'a> Merger<'a> {
    pub fn new(items: &'a mut Vec<Item>, aliases: &'a mut BTreeMap<String, String>) -> Merger<'a> {
        let count = items.len();
        let mut merger = Merger {
            items,
            aliases,
            gone: vec![false; count],
            by_doi: HashMap::new(),
            by_arxiv: HashMap::new(),
            by_source: HashMap::new(),
            by_family: HashMap::new(),
            merged: Vec::new(),
        };
        (0..count).for_each(|i| merger.index(i));
        merger
    }

    /// The keys of an item's entries, to the item.
    fn index(&mut self, i: usize) {
        for source in &self.items[i].sources {
            let e = &source.entry;
            if let Some(doi) = &e.doi {
                self.by_doi.insert(doi.clone(), i);
            }
            if let Some(arxiv) = &e.arxiv {
                self.by_arxiv.insert(arxiv.clone(), i);
            }
            self.by_source.insert((e.source.clone(), e.guid.clone()), i);
            if let Some(family) = first_family(e) {
                let items = self.by_family.entry(family).or_default();
                if !items.contains(&i) {
                    items.push(i);
                }
            }
        }
    }

    /// An entry fetched at `now`, into its item or a new one; true if new.
    pub fn add(&mut self, entry: Entry, now: DateTime<Utc>) -> bool {
        let own = self
            .by_source
            .get(&(entry.source.clone(), entry.guid.clone()))
            .copied();
        let doi = entry.doi.as_ref().and_then(|d| self.by_doi.get(d));
        let arxiv = entry.arxiv.as_ref().and_then(|a| self.by_arxiv.get(a));
        let mut found: BTreeSet<usize> = doi.into_iter().chain(arxiv).copied().collect();
        if own.is_none() && found.is_empty() {
            found.extend(self.same_title(&entry));
        }
        found.extend(own);
        // the oldest item stays
        let Some(&target) = found.first() else {
            self.create(entry, now);
            return true;
        };
        for &other in found.iter().skip(1) {
            self.absorb(target, other);
        }
        let item = &mut self.items[target];
        let same = |s: &&mut Source| s.entry.source == entry.source && s.entry.guid == entry.guid;
        match item.sources.iter_mut().find(same) {
            Some(source) => {
                source.entry = entry;
                source.listed = true;
            }
            None => item.sources.push(Source {
                entry,
                first_seen: now,
                listed: true,
            }),
        }
        refresh(item);
        self.index(target);
        false
    }

    /// A new item of an entry, its id from the entry's first key.
    fn create(&mut self, entry: Entry, now: DateTime<Utc>) {
        let key = match (&entry.doi, &entry.arxiv) {
            (Some(doi), _) => format!("doi:{doi}"),
            (None, Some(arxiv)) => format!("arxiv:{arxiv}"),
            (None, None) => format!("entry:{}\n{}", entry.source, entry.guid),
        };
        let source = Source {
            entry,
            first_seen: now,
            listed: true,
        };
        self.items
            .push(assemble(short_id(&key), vec![source], Vec::new()));
        self.gone.push(false);
        self.index(self.items.len() - 1);
    }

    /// Moves an item's sources into another; its id becomes an alias.
    fn absorb(&mut self, target: usize, other: usize) {
        let sources = std::mem::take(&mut self.items[other].sources);
        let linked = std::mem::take(&mut self.items[other].linked);
        self.gone[other] = true;
        let old = self.items[other].id.clone();
        let new = self.items[target].id.clone();
        for kept in self.aliases.values_mut().filter(|kept| **kept == old) {
            kept.clone_from(&new);
        }
        self.aliases.insert(old.clone(), new.clone());
        self.merged.push((old, new));
        let item = &mut self.items[target];
        item.sources.extend(sources);
        for doi in linked {
            if !item.linked.contains(&doi) {
                item.linked.push(doi);
            }
        }
    }

    /// The item of a title of the same words, by the same first author, within
    /// a year, with no other DOI or arXiv id: the closest.
    fn same_title(&self, entry: &Entry) -> Option<usize> {
        let family = first_family(entry)?;
        let year = entry.date?.year();
        let words = title_words(&entry.title);
        let score = |e: &Entry| {
            let near = e.date.is_some_and(|d| (d.year() - year).abs() <= 1);
            let same_author = first_family(e).as_deref() == Some(family.as_str());
            (near && same_author).then(|| jaccard(&words, &title_words(&e.title)))
        };
        self.by_family
            .get(&family)?
            .iter()
            .filter(|&&i| !self.gone[i] && !conflicts(entry, &self.items[i]))
            .filter_map(|&i| {
                let entries = self.items[i].sources.iter().map(|s| &s.entry);
                let best = entries.filter_map(score).max_by(f64::total_cmp)?;
                (best >= SAME_TITLE).then_some((best, i))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, i)| i)
    }

    /// The (absorbed, kept) ids of the items merged, the absorbed removed.
    pub fn finish(self) -> Vec<(String, String)> {
        let mut gone = self.gone.into_iter();
        self.items.retain(|_| !gone.next().unwrap_or(false));
        self.merged
    }
}

/// The first author's family name of an entry.
fn first_family(entry: &Entry) -> Option<String> {
    entry.authors.first().and_then(|a| family(a))
}

/// Whether an entry and an item are two works: another DOI, another arXiv id.
fn conflicts(entry: &Entry, item: &Item) -> bool {
    let entries = || item.sources.iter().map(|s| &s.entry);
    let differs = |mine: &Option<String>, theirs: Vec<&String>| {
        mine.as_ref()
            .is_some_and(|m| !theirs.is_empty() && !theirs.contains(&m))
    };
    differs(
        &entry.doi,
        entries().filter_map(|e| e.doi.as_ref()).collect(),
    ) || differs(
        &entry.arxiv,
        entries().filter_map(|e| e.arxiv.as_ref()).collect(),
    )
}

/// An item's fields again, from its sources.
fn refresh(item: &mut Item) {
    let sources = std::mem::take(&mut item.sources);
    let linked = std::mem::take(&mut item.linked);
    *item = assemble(std::mem::take(&mut item.id), sources, linked);
}

/// An item of its sources: the fields of the published version's entry (the
/// preprint's if none, the first seen of a kind), the longest abstract and
/// text, every DOI.
fn assemble(id: String, sources: Vec<Source>, linked: Vec<String>) -> Item {
    let best = &sources
        .iter()
        .min_by_key(|s| (s.entry.kind, s.first_seen))
        .expect("an item has a source")
        .entry;
    let entries = || sources.iter().map(|s| &s.entry);
    let mut versions: Vec<Version> = Vec::new();
    for doi in entries().filter_map(|e| e.doi.as_ref()) {
        if !versions.iter().any(|v| &v.doi == doi) {
            let stage = match is_preprint_doi(doi) {
                true => Stage::Preprint,
                false => Stage::Published,
            };
            versions.push(Version {
                doi: doi.clone(),
                stage,
            });
        }
    }
    let doi = versions
        .iter()
        .find(|v| v.stage == Stage::Published)
        .or(versions.first())
        .map(|v| v.doi.clone());
    let longest = |text: fn(&Entry) -> &Option<String>| {
        entries()
            .filter_map(|e| text(e).as_ref())
            .max_by_key(|t| t.len())
            .cloned()
    };
    let authors = std::iter::once(best)
        .chain(entries())
        .map(|e| &e.authors)
        .find(|a| !a.is_empty());
    Item {
        id,
        kind: best.kind,
        title: best.title.clone(),
        authors: authors.cloned().unwrap_or_default(),
        journal: best.journal.clone(),
        date: best.date,
        doi,
        arxiv: entries().find_map(|e| e.arxiv.clone()),
        url: best
            .url
            .clone()
            .or_else(|| entries().find_map(|e| e.url.clone())),
        summary: longest(|e| &e.summary),
        content: longest(|e| &e.content),
        editorial: entries().any(|e| e.editorial),
        versions,
        linked,
        sources,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ItemKind;
    use chrono::TimeZone;

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, 12, 0, 0).unwrap()
    }

    fn entry(source: &str, guid: &str, title: &str, author: &str) -> Entry {
        Entry {
            source: source.to_string(),
            guid: guid.to_string(),
            title: title.to_string(),
            authors: vec![author.to_string()],
            journal: None,
            date: Some(at(20)),
            doi: None,
            arxiv: None,
            url: None,
            summary: None,
            content: None,
            editorial: false,
            kind: ItemKind::Paper,
        }
    }

    fn doi(mut e: Entry, doi: &str) -> Entry {
        e.doi = Some(doi.to_string());
        e
    }

    fn preprint(mut e: Entry, arxiv: &str) -> Entry {
        e.arxiv = Some(arxiv.to_string());
        e.kind = ItemKind::Preprint;
        e
    }

    /// What entries made.
    struct Merged {
        items: Vec<Item>,
        aliases: BTreeMap<String, String>,
        /// (absorbed, kept)
        merged: Vec<(String, String)>,
    }

    /// The items of entries added in turn, one day apart.
    fn merge(entries: Vec<Entry>) -> Merged {
        let mut items = Vec::new();
        let mut aliases = BTreeMap::new();
        let mut merger = Merger::new(&mut items, &mut aliases);
        for (day, e) in (1..).zip(entries) {
            merger.add(e, at(day));
        }
        let merged = merger.finish();
        Merged {
            items,
            aliases,
            merged,
        }
    }

    const TITLE: &str = "Grounding line retreat of the West Antarctic Ice Sheet";

    #[test]
    fn same_doi_is_one_item() {
        let a = doi(entry("tc", "a", TITLE, "A. Robinson"), "10.5194/tc-1");
        let mut b = doi(entry("crossref", "b", "Other title", "B"), "10.5194/tc-1");
        b.summary = Some("a longer abstract".to_string());
        let Merged { items, .. } = merge(vec![a, b]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].sources.len(), 2);
        assert_eq!(items[0].title, TITLE);
        assert_eq!(items[0].summary.as_deref(), Some("a longer abstract"));
        assert_eq!(items[0].first_seen(), at(1));
        assert_eq!(items[0].id, short_id("doi:10.5194/tc-1"));
    }

    #[test]
    fn same_arxiv_id_is_one_item() {
        let a = preprint(entry("arxiv", "oai:1v1", TITLE, "Robinson"), "2610.1");
        let b = preprint(entry("arxiv2", "oai:1v2", TITLE, "Robinson"), "2610.1");
        let Merged { items, .. } = merge(vec![a, b]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, short_id("arxiv:2610.1"));
    }

    #[test]
    fn published_version_wins_over_preprint() {
        let mut pre = preprint(entry("arxiv", "p", TITLE, "Alexander Robinson"), "2610.1");
        pre.summary = Some("The preprint's abstract, which is longer.".to_string());
        let paper = doi(
            entry("tc", "t", &format!("{TITLE}."), "Robinson, A."),
            "10.5194/tc-20-1-2026",
        );
        let Merged { items, .. } = merge(vec![pre, paper]);
        assert_eq!(items.len(), 1, "same title, first author, year");
        let item = &items[0];
        assert_eq!(item.kind, ItemKind::Paper);
        assert_eq!(item.doi.as_deref(), Some("10.5194/tc-20-1-2026"));
        assert_eq!(item.arxiv.as_deref(), Some("2610.1"));
        assert_eq!(item.authors, ["Robinson, A."]);
        assert_eq!(
            item.summary.as_deref(),
            Some("The preprint's abstract, which is longer.")
        );
        assert_eq!(item.id, short_id("arxiv:2610.1"), "the first key stays");
    }

    #[test]
    fn similar_titles_need_the_same_author_and_year() {
        let a = entry("j1", "a", TITLE, "A. Robinson");
        let other_author = entry("j2", "b", TITLE, "B. Smith");
        let mut other_year = entry("j3", "c", TITLE, "A. Robinson");
        other_year.date = Some(Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap());
        let other_title = entry(
            "j4",
            "d",
            "Grounding line advance of East Antarctica",
            "A. Robinson",
        );
        let Merged { items, .. } = merge(vec![a, other_author, other_year, other_title]);
        assert_eq!(items.len(), 4);
    }

    #[test]
    fn copernicus_final_and_egusphere_preprint_stay_two_items() {
        let title =
            "Propagating meteorological uncertainty in physically based mountain snow modeling";
        let tc = doi(
            entry("tc", "t", title, "David R. Casson"),
            "10.5194/tc-20-5895-2026",
        );
        let mut egu = doi(
            entry("egusphere", "e", title, "David R. Casson"),
            "10.5194/egusphere-2025-6066",
        );
        egu.kind = ItemKind::Preprint;
        let Merged { items, .. } = merge(vec![egu, tc]);
        assert_eq!(
            items.len(),
            2,
            "two DOIs: linked by Crossref's relations later"
        );
        assert_eq!(items[0].versions[0].stage, Stage::Preprint);
        assert_eq!(items[1].versions[0].stage, Stage::Published);
    }

    #[test]
    fn an_entry_that_joins_two_items_merges_them() {
        let paper = doi(entry("tc", "t", "A paper", "Robinson"), "10.1/x");
        let pre = preprint(
            entry("arxiv", "p", "A preprint title", "Robinson"),
            "2610.1",
        );
        // arXiv's later listing, with the journal's DOI
        let bridge = doi(
            preprint(
                entry("arxiv", "p", "A preprint title", "Robinson"),
                "2610.1",
            ),
            "10.1/x",
        );
        let Merged {
            items,
            aliases,
            merged,
        } = merge(vec![paper, pre, bridge]);
        assert_eq!(items.len(), 1);
        let kept = short_id("doi:10.1/x");
        let absorbed = short_id("arxiv:2610.1");
        assert_eq!(items[0].id, kept);
        assert_eq!(
            items[0].sources.len(),
            2,
            "the arXiv source updated, not added"
        );
        assert_eq!(aliases.get(&absorbed), Some(&kept));
        assert_eq!(merged, [(absorbed, kept)]);
    }

    #[test]
    fn an_entry_again_updates_its_source() {
        let a = entry("blog", "post-1", "Old title", "Group");
        let b = entry("blog", "post-1", "New title", "Group");
        let Merged { items, .. } = merge(vec![a, b]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "New title");
        assert_eq!(items[0].sources[0].first_seen, at(1));
    }
}
