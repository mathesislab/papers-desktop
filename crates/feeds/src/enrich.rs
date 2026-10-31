//! What follows a fetch: the papers behind posts (in their feed's text, and
//! on their page for the subscriptions that ask), a paper item from Crossref
//! for each linked DOI that is not one, then the DOIs of papers not looked up
//! yet: Crossref's work (its abstract, its authors, its versions, which merge
//! a preprint and its paper), OpenAlex's abstract if Crossref has none (or a
//! teaser). Those whose feed gave no abstract, or a teaser, go first. Each
//! DOI is asked of each once (the lookups are kept), within a budget per
//! fetch; the reader looks up the item it opens at once.

use std::collections::HashSet;

use crate::crossref::{self, Message};
use crate::feeds::Feeds;
use crate::http::{Access, Http, get_json};
use crate::links::{Reach, sure};
use crate::model::{Entry, Feed, Item, ItemKind, LOOKUP};
use crate::openalex;
use crate::{Error, Result};

/// The most requests of Crossref and OpenAlex (by DOI, title or PII) in one fetch.
pub const LOOKUPS_PER_FETCH: usize = 50;

/// The most posts looked for on the web (their page, then their title in
/// Crossref) in one fetch.
pub const PAGES_PER_FETCH: usize = 20;

/// An abstract shorter than this, in characters, is a teaser (phys.org's,
/// a publisher's first lines), looked up.
pub const TEASER: usize = 300;

/// Whether an abstract is missing or a teaser.
fn wants_abstract(summary: Option<&str>) -> bool {
    summary.is_none_or(|s| s.chars().count() < TEASER)
}

/// What one fetch may still ask for, and what it did.
#[derive(Debug)]
pub(crate) struct Budget {
    lookups: usize,
    pages: usize,
    looked_up: usize,
    read: usize,
    /// OpenAlex did not answer, or refused: asked no more
    openalex_stopped: bool,
}

impl Budget {
    pub fn new(lookups: usize, pages: usize) -> Budget {
        Budget {
            lookups,
            pages,
            looked_up: 0,
            read: 0,
            openalex_stopped: false,
        }
    }

    /// Whether a lookup may be made, counted if so.
    pub fn lookup(&mut self) -> bool {
        let left = self.looked_up < self.lookups;
        self.looked_up += usize::from(left);
        left
    }

    /// Whether OpenAlex may be asked, counted if so.
    fn openalex(&mut self) -> bool {
        !self.openalex_stopped && self.lookup()
    }

    /// Whether a post may be looked for on the web, counted if so.
    pub fn page(&mut self) -> bool {
        let left = self.read < self.pages;
        self.read += usize::from(left);
        left
    }

    /// No more lookups: an API did not answer, or refused.
    pub fn stop(&mut self) {
        self.lookups = self.looked_up;
    }

    /// No more of OpenAlex; Crossref is still asked.
    fn stop_openalex(&mut self) {
        self.openalex_stopped = true;
    }

    pub fn lookups_used(&self) -> usize {
        self.looked_up
    }

    pub fn pages_used(&self) -> usize {
        self.read
    }
}

impl Feeds {
    /// What follows a fetch: the papers of posts, their paper items, the
    /// lookups of papers' DOIs; what it asked for.
    pub(crate) fn complete(&mut self, http: &dyn Http, access: &Access) -> Budget {
        let mut budget = Budget::new(LOOKUPS_PER_FETCH, PAGES_PER_FETCH);
        let pages: HashSet<&str> = self
            .subscriptions()
            .iter()
            .filter(|s| s.fetch_pages)
            .map(|s| s.id.as_str())
            .collect();
        let posts: Vec<(String, Reach)> = self
            .items()
            .iter()
            .filter(|i| i.kind.is_post() && !sure(&i.links))
            .filter_map(|i| {
                let asks = i
                    .sources
                    .iter()
                    .any(|s| pages.contains(s.entry.source.as_str()));
                let read = self.lookups.contains_key(&format!("page:{}", i.id));
                match (asks && !read, i.links.is_empty()) {
                    (true, _) => Some((i.id.clone(), Reach::Web)),
                    (false, true) => Some((i.id.clone(), Reach::Feed)),
                    (false, false) => None,
                }
            })
            .collect();
        for (id, reach) in &posts {
            self.find(id, *reach, http, access, &mut budget);
        }
        let ids: Vec<String> = self
            .items()
            .iter()
            .filter(|i| i.kind.is_post())
            .map(|i| i.id.clone())
            .collect();
        self.papers_of(&ids, http, access, &mut budget);
        self.look_up_papers(http, access, &mut budget);
        budget
    }

    /// A paper item for each DOI linked by these posts that is not an item
    /// yet, from Crossref; its source the post's.
    pub(crate) fn papers_of(
        &mut self,
        posts: &[String],
        http: &dyn Http,
        access: &Access,
        budget: &mut Budget,
    ) {
        let known: HashSet<&str> = self
            .items()
            .iter()
            .flat_map(|i| i.versions.iter().map(|v| v.doi.as_str()))
            .collect();
        let mut wanted: Vec<(String, String)> = Vec::new();
        for post in posts.iter().filter_map(|id| self.item(id)) {
            let source = &post.sources[0].entry.source;
            for link in &post.links {
                let new = !known.contains(link.doi.as_str())
                    && !self.lookups.contains_key(&format!("crossref:{}", link.doi))
                    && !wanted.iter().any(|(doi, _)| *doi == link.doi);
                if new {
                    wanted.push((link.doi.clone(), source.clone()));
                }
            }
        }
        let mut papers = Vec::new();
        for (doi, source) in wanted {
            papers.extend(self.look_up(&doi, &source, http, access, budget));
        }
        self.add(papers, false);
    }

    /// The DOIs of papers and preprints whose lookups may still find their
    /// abstract, those whose feed gave none (or a teaser) first: their works
    /// merged as lookups.
    fn look_up_papers(&mut self, http: &dyn Http, access: &Access, budget: &mut Budget) {
        let mut wanted: Vec<(bool, String)> = self
            .items()
            .iter()
            .filter_map(|i| {
                let doi = self.pending(i)?;
                Some((wants_abstract(i.summary.as_deref()), doi.to_string()))
            })
            .collect();
        wanted.sort_by_key(|(abstract_, _)| !abstract_);
        let mut works = Vec::new();
        for (_, doi) in wanted {
            works.extend(self.look_up(&doi, LOOKUP, http, access, budget));
        }
        self.add(works, false);
    }

    /// The DOI of a paper or preprint whose lookups may still find its
    /// abstract: Crossref not asked, or its work (looked up, or of a sweep)
    /// without one (or a teaser) and OpenAlex not asked.
    pub fn pending<'a>(&self, item: &'a Item) -> Option<&'a str> {
        let doi = item.doi.as_deref().filter(|_| !item.kind.is_post())?;
        let asked = |api: &str| self.lookups.contains_key(&format!("{api}:{doi}"));
        let work = (item.sources.iter().map(|s| &s.entry))
            .find(|e| e.doi.as_deref() == Some(doi) && self.is_work(e));
        let wants = wants_abstract(work.and_then(|e| e.summary.as_deref()));
        (!asked("crossref") || (wants && !asked("openalex"))).then_some(doi)
    }

    /// Whether an item's abstract is a work's (Crossref's, OpenAlex's), not
    /// the text of a feed.
    pub fn abstract_from_work(&self, item: &Item) -> bool {
        item.summary.is_some()
            && (item.sources.iter().map(|s| &s.entry))
                .any(|e| self.is_work(e) && e.summary == item.summary)
    }

    /// Whether an entry is a work of Crossref or OpenAlex: looked up, or of a sweep.
    fn is_work(&self, entry: &Entry) -> bool {
        entry.source == LOOKUP
            || (self.subscriptions().iter())
                .any(|s| s.id == entry.source && !matches!(s.feed, Feed::Rss { .. }))
    }

    /// Looks up the abstract of an item now (the reader opening it), if its
    /// lookups may still find it; whether they asked.
    pub fn look_up_abstract(&mut self, id: &str, http: &dyn Http, access: &Access) -> Result<bool> {
        let item = self
            .item(id)
            .ok_or_else(|| Error::NoItem { id: id.to_string() })?;
        let Some(doi) = self.pending(item).map(str::to_string) else {
            return Ok(false);
        };
        let mut budget = Budget::new(2, 0);
        let work = self.look_up(&doi, LOOKUP, http, access, &mut budget);
        self.add(work.into_iter().collect(), false);
        self.save()?;
        Ok(budget.lookups_used() > 0)
    }

    /// The work of a DOI as an entry of a source: Crossref's (asked once, then
    /// the entry kept), with OpenAlex's abstract if Crossref has none (or a
    /// teaser) and OpenAlex was not asked; None if unknown, or if the budget
    /// is spent. Each asking is kept; an API that does not answer is not
    /// asked again in this fetch.
    fn look_up(
        &mut self,
        doi: &str,
        source: &str,
        http: &dyn Http,
        access: &Access,
        budget: &mut Budget,
    ) -> Option<Entry> {
        let crossref = format!("crossref:{doi}");
        let mut entry = match self.lookups.contains_key(&crossref) {
            true => self.kept(doi, source),
            false if !budget.lookup() => return None,
            false => {
                match get_json::<Message<crossref::Work>>(http, &crossref::work_url(doi, access)) {
                    Ok(work) => {
                        self.lookups.insert(crossref, (self.now)());
                        work.and_then(|w| w.message.entry(source, ItemKind::Paper))
                    }
                    Err(_) => {
                        budget.stop();
                        return None;
                    }
                }
            }
        };
        let openalex = format!("openalex:{doi}");
        let more = wants_abstract(entry.as_ref().and_then(|e| e.summary.as_deref()))
            && !self.lookups.contains_key(&openalex);
        if more && budget.openalex() {
            match get_json::<openalex::Work>(http, &openalex::work_url(doi, access)) {
                Ok(work) => {
                    self.lookups.insert(openalex, (self.now)());
                    let other = work.map(|w| w.entry(source, ItemKind::Paper));
                    entry = match (entry, other) {
                        (Some(mut e), Some(o)) => {
                            let longer = |s: &Option<String>| s.as_ref().map_or(0, String::len);
                            if longer(&o.summary) > longer(&e.summary) {
                                e.summary = o.summary;
                            }
                            Some(e)
                        }
                        (e, o) => e.or(o),
                    };
                }
                Err(_) => budget.stop_openalex(),
            }
        }
        entry
    }

    /// The entry of a source a lookup of a DOI made, as its item keeps it.
    fn kept(&self, doi: &str, source: &str) -> Option<Entry> {
        (self.items().iter().flat_map(|i| &i.sources))
            .map(|s| &s.entry)
            .find(|e| e.source == source && e.doi.as_deref() == Some(doi))
            .cloned()
    }
}
