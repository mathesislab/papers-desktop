//! Saved views: a name for a query of the feeds and the order of its
//! items, as the Feeds tab's sidebar and the CLI's `--saved` use them. The
//! query is the whole of what the tab chose (`is:new kind:paper,preprint
//! prof:AMOC`, and the text typed); removed items are left out unless it
//! names them, as every query of the view.

use serde::{Deserialize, Serialize};

use crate::feeds::Feeds;
use crate::store::{self, CONFIG};
use crate::view::Sort;
use crate::{Error, Result};

/// A query of the feeds kept under a name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedView {
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
    pub fn saved_views(&self) -> &[SavedView] {
        &self.config.views
    }

    /// A saved view, by its name in any case.
    pub fn saved_view(&self, name: &str) -> Option<&SavedView> {
        self.saved_views().iter().find(|v| same(&v.name, name))
    }

    /// Sets the saved views, in their order, the names trimmed; refused for
    /// a blank name or a name given twice (in any case).
    pub fn set_saved_views(&mut self, views: Vec<SavedView>) -> Result<()> {
        let mut kept: Vec<SavedView> = Vec::with_capacity(views.len());
        for mut view in views {
            view.name = view.name.trim().to_string();
            let refused = |message: &str| Error::View {
                name: view.name.clone(),
                message: message.to_string(),
            };
            if view.name.is_empty() {
                return Err(refused("a saved view needs a name"));
            }
            if kept.iter().any(|k| same(&k.name, &view.name)) {
                return Err(refused("the name of another saved view"));
            }
            kept.push(view);
        }
        self.config.views = kept;
        store::write(&self.dir, CONFIG, &self.config)
    }
}
