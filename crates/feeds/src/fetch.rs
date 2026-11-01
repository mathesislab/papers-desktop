//! A fetch: every subscription read into the items, then what follows (the
//! papers of posts, the lookups by DOI, the rules on the new items). A
//! subscription's source is a feed, read by a conditional GET, or a sweep of
//! Crossref or OpenAlex ([`crate::sweep`]); what each got ends alike: merged
//! as the subscription's entries, the subscription titled if it is not, what
//! its next fetch starts from kept.

use std::collections::BTreeSet;

use chrono::TimeDelta;

use crate::Result;
use crate::feeds::Feeds;
use crate::http::{Access, FEED_ACCEPT, Http, Request, failure};
use crate::model::{Entry, Feed, Subscription};
use crate::parse::parse;
use crate::store::{Since, Validators};
use crate::sweep;

/// What a fetch did, per subscription, and what followed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FetchReport {
    pub feeds: Vec<FeedReport>,
    pub follow_up: FollowUp,
}

/// What followed reading the feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FollowUp {
    /// the requests of Crossref and OpenAlex for works by DOI, title or PII
    pub lookups: usize,
    /// the posts looked for on the web (their page, then their title)
    pub pages: usize,
}

/// What the fetch of a subscription did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedReport {
    /// the subscription's id
    pub subscription: String,
    pub outcome: Outcome,
}

/// How the fetch of a subscription ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// the entries of the feed, and the items they made
    Fetched { entries: usize, new: usize },
    /// not modified since the last fetch (304)
    Unchanged,
    /// no answer, an error status, or a feed that does not parse
    Failed(String),
}

/// What reading a subscription's source got.
pub(crate) enum Got {
    Entries {
        entries: Vec<Entry>,
        /// the source's own title: the feed's, a journal's in Crossref
        title: Option<String>,
        /// those of a feed's answer, for the next request
        validators: Validators,
        /// its entries have all that a lookup of their DOIs in Crossref
        /// would give (a sweep of Crossref's): kept as looked up
        looked_up: bool,
    },
    /// not modified since the last fetch (304), with the validators of the answer
    Unchanged(Validators),
}

impl Feeds {
    /// Fetches every subscription ([`Feeds::read_feeds`]), then what follows
    /// ([`Feeds::follow_up`]).
    pub fn fetch(&mut self, http: &dyn Http, access: &Access) -> Result<FetchReport> {
        let known = self.ids();
        let feeds = self.read_feeds(None, http, access)?;
        let follow_up = self.follow_up(&known, http, access)?;
        Ok(FetchReport { feeds, follow_up })
    }

    /// The ids of the items, to tell those a fetch brings.
    pub fn ids(&self) -> BTreeSet<String> {
        self.items().iter().map(|i| i.id.clone()).collect()
    }

    /// Reads every subscription, or the one of id `only`, one after the
    /// other, into the items: the feeds' own text, written. A feed that fails
    /// is reported and the others go on; the APIs of the sweeps are asked
    /// with `access`.
    pub fn read_feeds(
        &mut self,
        only: Option<&str>,
        http: &dyn Http,
        access: &Access,
    ) -> Result<Vec<FeedReport>> {
        if let Some(id) = only
            && !self.subscriptions().iter().any(|s| s.id == id)
        {
            return Err(crate::Error::NoSubscription { id: id.to_string() });
        }
        let mut reports = Vec::new();
        for i in 0..self.config.subscriptions.len() {
            let subscription = self.config.subscriptions[i].id.clone();
            if only.is_some_and(|id| id != subscription) {
                continue;
            }
            let outcome = self.fetch_subscription(i, http, access);
            reports.push(FeedReport {
                subscription,
                outcome,
            });
        }
        self.save()?;
        Ok(reports)
    }

    /// What follows reading the feeds: the papers of posts, the lookups by
    /// DOI. The rules then triage the items not `known` before (their
    /// abstracts looked up), and old items go; written.
    pub fn follow_up(
        &mut self,
        known: &BTreeSet<String>,
        http: &dyn Http,
        access: &Access,
    ) -> Result<FollowUp> {
        let budget = self.complete(http, access);
        self.apply_rules(known);
        self.drop_old();
        self.save()?;
        Ok(FollowUp {
            lookups: budget.lookups_used(),
            pages: budget.pages_used(),
        })
    }

    /// Fetches a subscription: its feed, or a sweep for the works since its
    /// last (since the lookback the first time); what it got merged. A
    /// sweep that fails is asked for again from the same day.
    fn fetch_subscription(&mut self, i: usize, http: &dyn Http, access: &Access) -> Outcome {
        let subscription = &self.config.subscriptions[i];
        let id = subscription.id.clone();
        let last = self.http.get(&id).cloned().unwrap_or_default();
        let now = (self.now)();
        let lookback = TimeDelta::days(self.config.lookback_days.into());
        let since = last.swept.unwrap_or(now - lookback).date_naive();
        let got = match &subscription.feed {
            Feed::Rss { url } => read_feed(http, url, subscription, &last.validators),
            Feed::Crossref { issn } => {
                sweep::crossref(http, access, subscription, issn, since, lookback)
            }
            Feed::OpenAlex { filter } => {
                sweep::openalex(http, access, subscription, filter, since - lookback)
            }
        };
        let swept = (!matches!(subscription.feed, Feed::Rss { .. })).then_some(now);
        match got {
            Err(message) => Outcome::Failed(message),
            Ok(Got::Unchanged(validators)) => {
                self.http.insert(id, Since { validators, swept });
                Outcome::Unchanged
            }
            Ok(Got::Entries {
                entries,
                title,
                validators,
                looked_up,
            }) => {
                if looked_up {
                    for doi in entries.iter().filter_map(|e| e.doi.as_ref()) {
                        self.lookups.insert(format!("crossref:{doi}"), now);
                    }
                }
                let count = entries.len();
                let new = self.merge(&id, entries);
                let subscription = &mut self.config.subscriptions[i];
                if subscription.title.is_none() {
                    subscription.title = title;
                }
                self.http.insert(id, Since { validators, swept });
                Outcome::Fetched {
                    entries: count,
                    new,
                }
            }
        }
    }

    /// The entries of a subscription's source into the items; the number of
    /// new items. Only these entries are listed by it now.
    fn merge(&mut self, subscription: &str, entries: Vec<Entry>) -> usize {
        for item in &mut self.items.items {
            for source in &mut item.sources {
                if source.entry.source == subscription {
                    source.listed = false;
                }
            }
        }
        self.add(entries, true)
    }
}

/// A subscription's feed by a GET conditional on the validators of its last
/// answer, parsed.
fn read_feed(
    http: &dyn Http,
    url: &str,
    subscription: &Subscription,
    known: &Validators,
) -> std::result::Result<Got, String> {
    let request = Request {
        url: url.to_string(),
        accept: Some(FEED_ACCEPT.to_string()),
        if_none_match: known.etag.clone(),
        if_modified_since: known.last_modified.clone(),
    };
    let reply = http.get(&request)?;
    let validators = Validators {
        etag: reply.etag.clone().or(known.etag.clone()),
        last_modified: reply.last_modified.clone().or(known.last_modified.clone()),
    };
    match reply.status {
        304 => Ok(Got::Unchanged(validators)),
        200..=299 => {
            let parsed =
                parse(&reply.body, Some(&reply.url), subscription).map_err(|e| e.to_string())?;
            Ok(Got::Entries {
                entries: parsed.entries,
                title: parsed.title,
                validators,
                looked_up: false,
            })
        }
        _ => Err(failure(&reply)),
    }
}
