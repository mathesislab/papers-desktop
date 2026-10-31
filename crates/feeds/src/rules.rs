//! Rules: a query in the feeds' language and what to do with the new items
//! it matches (`prof:amoc score:6-` → shortlist, `kind:news -src:futurity`
//! → seen, `t:editorial` → dismiss). They act once, on the items a fetch
//! brings, never on an item with a triage.

use std::collections::BTreeSet;

use papers_core::Matching;
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::feeds::Feeds;
use crate::model::{Mark, Triage};
use crate::query::parse;
use crate::store::{self, CONFIG};

/// What to do with a new item that matches a query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// a query of the feeds; an empty one matches every item
    pub when: String,
    pub then: Action,
}

/// The triage a rule sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Shortlist,
    Dismiss,
    Seen,
}

impl Action {
    pub fn triage(self) -> Triage {
        match self {
            Action::Shortlist => Triage::Shortlisted,
            Action::Dismiss => Triage::Dismissed,
            Action::Seen => Triage::Seen,
        }
    }
}

impl Feeds {
    pub fn rules(&self) -> &[Rule] {
        &self.config.rules
    }

    /// Sets the rules, in the order they are tried.
    pub fn set_rules(&mut self, rules: Vec<Rule>) -> Result<()> {
        self.config.rules = rules;
        store::write(&self.dir, CONFIG, &self.config)
    }

    /// Triage of the items not `known` before a fetch: the first rule that
    /// matches an item decides; an item with a triage (merged into one the
    /// user marked) keeps it. Not written.
    pub(crate) fn apply_rules(&mut self, known: &BTreeSet<String>) {
        let rules: Vec<_> = self
            .rules()
            .iter()
            .map(|r| (parse(&r.when), r.then))
            .collect();
        if rules.is_empty() {
            return;
        }
        let new = self.items().iter().filter(|i| !known.contains(&i.id));
        let decided: Vec<(String, Action)> = new
            .filter(|item| self.mark(&item.id).is_none())
            .filter_map(|item| {
                let seen = self.seen(item, Vec::new());
                let matches = |q| seen.record.check(q, Matching::WordStart).is_some();
                let rule = rules.iter().find(|(query, _)| matches(query));
                rule.map(|&(_, action)| (item.id.clone(), action))
            })
            .collect();
        let at = (self.now)();
        for (id, action) in decided {
            let triage = action.triage();
            self.state.insert(id, Mark { triage, at });
        }
    }
}
