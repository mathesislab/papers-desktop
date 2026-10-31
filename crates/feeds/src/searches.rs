//! Saved searches: a name for a query of the feeds and the order of its
//! items, as the Feeds tab's sidebar and the CLI's `--saved` use them. The
//! query is the whole of what the tab chose (`is:new kind:paper,preprint
//! prof:AMOC`, and the text typed); dismissed items are left out unless it
//! asks for them ([`live`](crate::query::live)).

use serde::{Deserialize, Serialize};

use crate::feeds::Feeds;
use crate::store::{self, CONFIG};
use crate::view::Sort;
use crate::{Error, Result};

/// A query of the feeds kept under a name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Search {
    pub name: String,
    pub query: String,
    #[serde(default)]
    pub sort: Sort,
}

/// Whether two names are the same in any case.
fn same(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

impl Feeds {
    pub fn searches(&self) -> &[Search] {
        &self.config.searches
    }

    /// A saved search, by its name in any case.
    pub fn saved_search(&self, name: &str) -> Option<&Search> {
        self.searches().iter().find(|s| same(&s.name, name))
    }

    /// Sets the saved searches, in their order, the names trimmed; refused
    /// for a blank name or a name given twice (in any case).
    pub fn set_searches(&mut self, searches: Vec<Search>) -> Result<()> {
        let mut kept: Vec<Search> = Vec::with_capacity(searches.len());
        for mut search in searches {
            search.name = search.name.trim().to_string();
            let refused = |message: &str| Error::Search {
                name: search.name.clone(),
                message: message.to_string(),
            };
            if search.name.is_empty() {
                return Err(refused("a saved search needs a name"));
            }
            if kept.iter().any(|k| same(&k.name, &search.name)) {
                return Err(refused("the name of another saved search"));
            }
            kept.push(search);
        }
        self.config.searches = kept;
        store::write(&self.dir, CONFIG, &self.config)
    }
}
