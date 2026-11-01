//! Entries into items: one item per paper or post, from every source that
//! listed it. An entry joins the item of the same feed item, else of its DOI
//! or of a version Crossref relates it to (a preprint and its paper), else of
//! its arXiv id, else of its page (the link of an entry of neither, as two
//! news feeds of one site list an article), else of a title of the same words
//! (Jaccard ≥ 0.9) by the same first author within a year; an entry that joins
//! two items merges them, the newer becoming an alias of the older.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Datelike, Utc};

use crate::model::{Entry, Item, LOOKUP, Linked, Source, Stage, Version};
use crate::normalize::{family, is_preprint_doi, jaccard, page_key, short_id, title_words};

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
    /// every item of a page, that the items of one page (merged before pages
    /// were compared) collapse at once
    by_page: HashMap<String, Vec<usize>>,
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
            by_page: HashMap::new(),
            merged: Vec::new(),
        };
        (0..count).for_each(|i| merger.index(i));
        merger
    }

    /// The keys of an item's entries, to the item.
    fn index(&mut self, i: usize) {
        for source in &self.items[i].sources {
            let e = &source.entry;
            for doi in e.doi.iter().chain(e.versions.iter().map(|v| &v.doi)) {
                self.by_doi.insert(doi.clone(), i);
            }
            if let Some(arxiv) = &e.arxiv {
                self.by_arxiv.insert(arxiv.clone(), i);
            }
            self.by_source.insert((e.source.clone(), e.guid.clone()), i);
            let family = first_family(e).map(|f| self.by_family.entry(f).or_default());
            let page = page_of(e).map(|p| self.by_page.entry(p).or_default());
            for items in family.into_iter().chain(page) {
                if !items.contains(&i) {
                    items.push(i);
                }
            }
        }
    }

    /// An entry fetched at `now`, into its item or a new one, listed by its
    /// feed or not (a lookup's); true if new.
    pub fn add(&mut self, entry: Entry, now: DateTime<Utc>, listed: bool) -> bool {
        let own = self
            .by_source
            .get(&(entry.source.clone(), entry.guid.clone()))
            .copied();
        let dois = entry
            .doi
            .iter()
            .chain(entry.versions.iter().map(|v| &v.doi));
        let doi = dois.filter_map(|d| self.by_doi.get(d));
        let arxiv = entry.arxiv.as_ref().and_then(|a| self.by_arxiv.get(a));
        let mut found: BTreeSet<usize> = doi.chain(arxiv).copied().collect();
        found.extend(self.same_page(&entry));
        if own.is_none() && found.is_empty() {
            found.extend(self.same_title(&entry));
        }
        found.extend(own);
        // the oldest item stays
        let Some(&target) = found.first() else {
            self.create(entry, now, listed);
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
                source.listed = listed;
            }
            None => item.sources.push(Source {
                entry,
                first_seen: now,
                listed,
            }),
        }
        refresh(item);
        self.index(target);
        false
    }

    /// A new item of an entry, its id from the entry's first key.
    fn create(&mut self, entry: Entry, now: DateTime<Utc>, listed: bool) {
        let key = match (&entry.doi, &entry.arxiv) {
            (Some(doi), _) => format!("doi:{doi}"),
            (None, Some(arxiv)) => format!("arxiv:{arxiv}"),
            (None, None) => format!("entry:{}\n{}", entry.source, entry.guid),
        };
        let source = Source {
            entry,
            first_seen: now,
            listed,
        };
        self.items
            .push(assemble(short_id(&key), vec![source], Vec::new()));
        self.gone.push(false);
        self.index(self.items.len() - 1);
    }

    /// Moves an item's sources into another; its id becomes an alias.
    fn absorb(&mut self, target: usize, other: usize) {
        let sources = std::mem::take(&mut self.items[other].sources);
        let links = std::mem::take(&mut self.items[other].links);
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
        item.links = join_links(std::mem::take(&mut item.links), links);
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

    /// The items of an entry's page, none if a subscription lists the page
    /// under two guids: a feed that links its entries to one page (a section,
    /// a podcast's) says it is not an article's.
    fn same_page(&self, entry: &Entry) -> Vec<usize> {
        let Some(page) = page_of(entry) else {
            return Vec::new();
        };
        let items = self.by_page.get(&page).into_iter().flatten();
        let items: Vec<usize> = items.copied().filter(|&i| !self.gone[i]).collect();
        let entries = items.iter().flat_map(|&i| &self.items[i].sources);
        let mut guids = HashMap::from([(&entry.source, &entry.guid)]);
        for e in entries.map(|s| &s.entry) {
            if page_of(e).as_ref() != Some(&page) {
                continue;
            }
            if guids
                .insert(&e.source, &e.guid)
                .is_some_and(|g| *g != e.guid)
            {
                return Vec::new();
            }
        }
        items
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

/// The page of an entry of no DOI and no arXiv id, a post's mostly: its
/// link, as [`page_key`] compares them. A paper's DOI or arXiv id is its key,
/// not its link: the link of a journal's feed can be its issue's page.
fn page_of(entry: &Entry) -> Option<String> {
    if entry.doi.is_some() || entry.arxiv.is_some() {
        return None;
    }
    page_key(entry.url.as_deref()?)
}

/// Whether an entry and an item are two works: another DOI, another arXiv
/// id, a post and a paper.
fn conflicts(entry: &Entry, item: &Item) -> bool {
    if entry.kind.is_post() != item.kind.is_post() {
        return true;
    }
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
pub(crate) fn refresh(item: &mut Item) {
    let sources = std::mem::take(&mut item.sources);
    let links = std::mem::take(&mut item.links);
    *item = assemble(std::mem::take(&mut item.id), sources, links);
}

/// The links of two merged posts: each paper once, at its best confidence,
/// the high ones first.
pub(crate) fn join_links(mut links: Vec<Linked>, more: Vec<Linked>) -> Vec<Linked> {
    for link in more {
        match links.iter_mut().find(|l| l.doi == link.doi) {
            Some(known) if link.confidence < known.confidence => *known = link,
            Some(_) => {}
            None => links.push(link),
        }
    }
    links.sort_by_key(|l| l.confidence);
    links
}

/// An item of its sources: the fields of the published version's entry (the
/// preprint's if none, the first seen of a kind), the abstract looked up by
/// DOI over a feed's (a teaser, an issue line, a plain-language summary)
/// else the longest, the longest text, every DOI (of a stage as Crossref
/// relates them, else by its prefix).
fn assemble(id: String, sources: Vec<Source>, links: Vec<Linked>) -> Item {
    let best = &sources
        .iter()
        .min_by_key(|s| (s.entry.kind, s.first_seen))
        .expect("an item has a source")
        .entry;
    let entries = || sources.iter().map(|s| &s.entry);
    let related = entries().flat_map(|e| e.versions.iter().cloned());
    let own = entries().filter_map(|e| e.doi.as_ref()).map(|doi| Version {
        doi: doi.clone(),
        stage: match is_preprint_doi(doi) {
            true => Stage::Preprint,
            false => Stage::Published,
        },
    });
    let mut versions: Vec<Version> = Vec::new();
    for version in related.chain(own) {
        if !versions.iter().any(|v| v.doi == version.doi) {
            versions.push(version);
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
        summary: (entries().filter(|e| e.source == LOOKUP))
            .find_map(|e| e.summary.clone())
            .or_else(|| longest(|e| &e.summary)),
        content: longest(|e| &e.content),
        editorial: entries().any(|e| e.editorial),
        versions,
        links,
        cited: entries().filter_map(|e| e.cited).max(),
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
            origin: crate::model::Origin::Feed,
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
            versions: Vec::new(),
            cited: None,
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
            merger.add(e, at(day), true);
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
    fn crossrefs_versions_join_a_preprint_and_its_paper() {
        let title = "Snow modeling";
        let mut egu = doi(
            entry("egusphere", "e", title, "Casson"),
            "10.5194/egusphere-2025-6066",
        );
        egu.kind = ItemKind::Preprint;
        let tc = doi(
            entry("tc", "t", "Snow modelling, final", "Casson"),
            "10.5194/tc-20-5895-2026",
        );
        let mut looked_up = tc.clone();
        looked_up.source = crate::model::LOOKUP.to_string();
        looked_up.origin = crate::model::Origin::Work;
        looked_up.versions = vec![
            Version {
                doi: "10.5194/tc-20-5895-2026".to_string(),
                stage: Stage::Published,
            },
            Version {
                doi: "10.5194/egusphere-2025-6066".to_string(),
                stage: Stage::Preprint,
            },
        ];
        let Merged { items, merged, .. } = merge(vec![egu, tc, looked_up]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].doi.as_deref(), Some("10.5194/tc-20-5895-2026"));
        assert_eq!(items[0].title, "Snow modelling, final");
        assert_eq!(items[0].kind, ItemKind::Paper);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn a_post_and_a_paper_of_one_title_stay_two() {
        let paper = entry("journal", "p", TITLE, "A. Robinson");
        let mut post = entry("blog", "b", TITLE, "A. Robinson");
        post.kind = ItemKind::Blog;
        let Merged { items, .. } = merge(vec![paper, post]);
        assert_eq!(items.len(), 2);
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

    /// A news article of a feed, its guid its link (titles that differ, that
    /// they merge by their page only).
    fn article(source: &str, url: &str, title: &str) -> Entry {
        let mut e = entry(source, url, title, "ScienceDaily");
        e.url = Some(url.to_string());
        e.kind = ItemKind::News;
        e
    }

    const PAGE: &str = "https://www.sciencedaily.com/releases/2026/09/260919031037.htm";

    #[test]
    fn an_article_of_two_feeds_is_one_item() {
        let earth = article("earth-climate", PAGE, "Glaciers retreat");
        let climate = article(
            "climate",
            "http://WWW.ScienceDaily.com/releases/2026/09/260919031037.htm/?utm_source=rss#top",
            "Glaciers retreat faster",
        );
        let Merged { items, .. } = merge(vec![earth, climate]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].sources.len(), 2);
        assert_eq!(
            items[0].id,
            short_id(&format!("entry:earth-climate\n{PAGE}"))
        );
    }

    #[test]
    fn a_sites_home_page_is_no_article() {
        let a = article("a", "https://www.sciencedaily.com/", "Glaciers retreat");
        let b = article("b", "https://www.sciencedaily.com", "Sea ice thins");
        let Merged { items, .. } = merge(vec![a, b]);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn a_page_a_feed_links_its_entries_to_is_no_article() {
        let section = "https://example.org/podcast";
        let mut first = article("podcast", section, "Glaciers retreat");
        first.guid = "episode-1".to_string();
        let mut second = article("podcast", section, "Sea ice thins");
        second.guid = "episode-2".to_string();
        let other = article("digest", section, "Permafrost thaws");
        let Merged { items, .. } = merge(vec![first, second, other]);
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn papers_merge_by_their_doi_not_their_link() {
        let issue = "https://example.org/journal/current-issue";
        let mut a = doi(entry("j", "a", TITLE, "Robinson"), "10.1/a");
        a.url = Some(issue.to_string());
        let mut b = doi(entry("j", "b", "Another paper", "Smith"), "10.1/b");
        b.url = Some(issue.to_string());
        let post = article("news", issue, "This issue");
        let Merged { items, .. } = merge(vec![a, b, post]);
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn duplicates_of_an_article_collapse_when_it_comes_again() {
        // two items of one article, as merged before pages were compared
        let earth = article("earth-climate", PAGE, "Glaciers retreat");
        let mut items = merge(vec![earth]).items;
        let climate = article("climate", PAGE, "Glaciers retreat faster");
        items.extend(merge(vec![climate.clone()]).items);
        let (kept, absorbed) = (items[0].id.clone(), items[1].id.clone());
        let mut aliases = BTreeMap::new();
        let mut merger = Merger::new(&mut items, &mut aliases);
        assert!(!merger.add(climate, at(2), true));
        let merged = merger.finish();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, kept);
        let sources: Vec<&str> = items[0]
            .sources
            .iter()
            .map(|s| s.entry.source.as_str())
            .collect();
        assert_eq!(sources, ["earth-climate", "climate"]);
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
