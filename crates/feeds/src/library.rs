//! What the feeds know of the library, given whole whenever it is read: how
//! like its papers an item is (`like:`), and which of its papers an item is
//! (`is:inlibrary`). The queries, the rules and the rows see the same facts.

use std::sync::Arc;

use chrono::Datelike;
use papers_core::{Identities, Identity, Like, Likeness};

use crate::feeds::Feeds;
use crate::model::Item;

/// The facts of a library.
#[derive(Debug, Default)]
pub struct LibraryFacts {
    /// the index of its papers' texts; none for an empty library, or until
    /// it is built
    pub likeness: Option<Arc<Likeness>>,
    /// its papers by DOI and by title and year (core's `Library::identities`)
    pub identities: Arc<Identities>,
}

impl Feeds {
    /// Sets the facts of the library that `like:`, `is:inlibrary`, the sort
    /// by likeness and the rows use; without them, or without an index,
    /// `like:` matches nothing, and no item is in the library.
    pub fn set_library(&mut self, library: Arc<LibraryFacts>) {
        self.library = Some(library);
    }

    /// How like the library's papers an item is, by its title and summary;
    /// none without an index.
    pub fn like(&self, item: &Item) -> Option<Like> {
        let likeness = self.library.as_ref()?.likeness.as_ref()?;
        let summary = item.summary.as_deref().unwrap_or("");
        Some(likeness.like(&item.title, summary))
    }

    /// The ids of the library's entries of the same paper: of its DOI, or of
    /// its title and year.
    pub fn in_library(&self, item: &Item) -> Vec<String> {
        let Some(library) = &self.library else {
            return Vec::new();
        };
        let year = item.date.map(|d| d.year());
        let identity = Identity::of(item.doi.as_deref(), &item.title, year);
        library.identities.same(&identity)
    }

    /// The ids of the library's entries of another version of it (by DOI),
    /// not those of itself (`same`).
    pub fn versions_in_library(&self, item: &Item, same: &[String]) -> Vec<String> {
        let Some(library) = &self.library else {
            return Vec::new();
        };
        let own = item.doi.as_deref().map(str::to_lowercase);
        let mut ids: Vec<String> = Vec::new();
        for version in &item.versions {
            let doi = version.doi.to_lowercase();
            if Some(&doi) == own.as_ref() {
                continue;
            }
            for id in library.identities.of_doi(&doi) {
                if !same.contains(id) && !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
        }
        ids
    }
}
