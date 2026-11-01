//! Rules: a query in the feeds' language and what to do with the new items
//! it matches (`prof:amoc score:80-` → star, `kind:news -src:futurity`
//! → seen, `t:editorial` → remove). They act once, on the items a fetch
//! brings, never on an item with a triage.

use std::collections::BTreeSet;

use papers_core::Matching;
use serde::{Deserialize, Serialize};

use crate::feeds::Feeds;
use crate::model::{Mark, Triage, words};
use crate::query::parse;

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
    /// `shortlist` in a config.json of before
    #[serde(alias = "shortlist")]
    Star,
    /// `dismiss` in a config.json of before: an alias kept on purpose
    #[serde(alias = "dismiss")]
    Remove,
    Seen,
}

words!(Action {
    Star => "star",
    Remove => "remove",
    Seen => "seen",
});

impl Action {
    pub fn triage(self) -> Triage {
        match self {
            Action::Star => Triage::Starred,
            Action::Remove => Triage::Removed,
            Action::Seen => Triage::Seen,
        }
    }
}

impl Feeds {
    /// The rules, in the order they are tried.
    pub fn rules(&self) -> &[Rule] {
        &self.config.settings.rules
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
                let seen = self.seen(item);
                let matches = |q| seen.record.check(q, Matching::WordStart).is_some();
                let rule = rules.iter().find(|(query, _)| matches(query));
                rule.map(|&(_, action)| (item.id.clone(), action))
            })
            .collect();
        let at = (self.now)();
        for (id, action) in decided {
            let triage = action.triage();
            self.state.insert(id, Mark::new(triage, at));
        }
    }
}
