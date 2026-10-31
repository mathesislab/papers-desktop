//! What follows a fetch: the DOIs of papers not looked up yet, in Crossref
//! (its work's abstract, authors and versions, which merge a preprint and its
//! paper), and in OpenAlex for an abstract Crossref lacks, if a key is set.
//! Those without an abstract, or with a teaser, go first. Each DOI is looked
//! up once (the lookups are kept), within a budget per fetch.

use crate::crossref::{self, Message};
use crate::feeds::Feeds;
use crate::http::{Access, Http, get_json};
use crate::model::{Entry, ItemKind, LOOKUP};
use crate::openalex;

/// The most requests of Crossref and OpenAlex in one fetch.
pub const LOOKUPS_PER_FETCH: usize = 50;

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
    looked_up: usize,
}

impl Budget {
    pub fn new(lookups: usize) -> Budget {
        Budget {
            lookups,
            looked_up: 0,
        }
    }

    /// Whether a lookup may be made, counted if so.
    pub fn lookup(&mut self) -> bool {
        let left = self.looked_up < self.lookups;
        self.looked_up += usize::from(left);
        left
    }

    /// No more lookups: an API did not answer, or refused.
    pub fn stop(&mut self) {
        self.lookups = self.looked_up;
    }

    pub fn lookups_used(&self) -> usize {
        self.looked_up
    }
}

impl Feeds {
    /// What follows a fetch: the lookups of papers' DOIs; what it asked for.
    pub(crate) fn complete(&mut self, http: &dyn Http, access: &Access) -> Budget {
        let mut budget = Budget::new(LOOKUPS_PER_FETCH);
        self.look_up_papers(http, access, &mut budget);
        budget
    }

    /// The DOIs of papers and preprints not looked up yet, those that want an
    /// abstract first: their works merged as lookups.
    fn look_up_papers(&mut self, http: &dyn Http, access: &Access, budget: &mut Budget) {
        let mut wanted: Vec<(bool, String)> = self
            .items()
            .iter()
            .filter(|i| matches!(i.kind, ItemKind::Paper | ItemKind::Preprint))
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
