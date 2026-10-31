//! What follows a fetch: the papers behind posts (in their feed's text, and
//! on their page for the subscriptions that ask), a paper item from Crossref
//! for each linked DOI that is not one, then the DOIs of papers not looked up
//! yet: Crossref's work (its abstract, its authors, its versions, which merge
//! a preprint and its paper), OpenAlex's abstract if Crossref has none and a
//! key is set. Those without an abstract, or with a teaser, go first. Each
//! DOI is looked up once (the lookups are kept), within a budget per fetch.

use std::collections::HashSet;

use crate::crossref::{self, Message};
use crate::feeds::Feeds;
use crate::http::{Access, Http, get_json};
use crate::links::{Reach, sure};
use crate::model::{Entry, ItemKind, LOOKUP};
use crate::openalex;

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
}

impl Budget {
    pub fn new(lookups: usize, pages: usize) -> Budget {
        Budget {
            lookups,
            pages,
            looked_up: 0,
            read: 0,
        }
    }

    /// Whether a lookup may be made, counted if so.
    pub fn lookup(&mut self) -> bool {
        let left = self.looked_up < self.lookups;
        self.looked_up += usize::from(left);
        left
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
            papers.extend(self.look_up(&doi, &source, true, http, access, budget));
        }
        self.add(papers, false);
    }

    /// The DOIs of papers and preprints not looked up yet, those that want an
    /// abstract first: their works merged as lookups.
    fn look_up_papers(&mut self, http: &dyn Http, access: &Access, budget: &mut Budget) {
        let mut wanted: Vec<(bool, String)> = self
            .items()
            .iter()
            .filter(|i| !i.kind.is_post())
            .filter_map(|i| {
                let doi = i.doi.as_ref()?;
                let known = self.lookups.contains_key(&format!("crossref:{doi}"));
                (!known).then(|| (wants_abstract(i.summary.as_deref()), doi.clone()))
            })
            .collect();
        wanted.sort_by_key(|(abstract_, _)| !abstract_);
        let mut works = Vec::new();
        for (abstract_, doi) in wanted {
            works.extend(self.look_up(&doi, LOOKUP, abstract_, http, access, budget));
        }
        self.add(works, false);
    }

    /// The work of a DOI as an entry of a source: Crossref's, with OpenAlex's
    /// abstract if `abstract_` is wanted, Crossref has none (or a teaser) and
    /// a key is set; None if unknown, or if the budget is spent. Each asking
    /// is kept; an API that does not answer stops the lookups.
    fn look_up(
        &mut self,
        doi: &str,
        source: &str,
        abstract_: bool,
        http: &dyn Http,
        access: &Access,
        budget: &mut Budget,
    ) -> Option<Entry> {
        if !budget.lookup() {
            return None;
        }
        let url = crossref::work_url(doi, access);
        let mut entry = match get_json::<Message<crossref::Work>>(http, &url) {
            Ok(work) => {
                self.lookups.insert(format!("crossref:{doi}"), (self.now)());
                work.and_then(|w| w.message.entry(source, ItemKind::Paper))
            }
            Err(_) => {
                budget.stop();
                return None;
            }
        };
        let openalex = format!("openalex:{doi}");
        let more = abstract_
            && wants_abstract(entry.as_ref().and_then(|e| e.summary.as_deref()))
            && access.openalex_key.is_some()
            && !self.lookups.contains_key(&openalex);
        if more && budget.lookup() {
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
                Err(_) => budget.stop(),
            }
        }
        entry
    }
}
