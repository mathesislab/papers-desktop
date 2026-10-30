//! The feeds of a folder: subscribing, fetching every subscription into the
//! items, the triage of items, and the pruning of old ones.

use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeDelta, Utc};

use crate::http::{FEED_ACCEPT, Http, Request};
use crate::merge::Merger;
use crate::model::{Item, Mark, SourceKind, Subscription, Triage};
use crate::normalize::{canonical_url, feed_key, short_id};
use crate::parse::parse;
use crate::store::{self, CONFIG, Config, HTTP, ITEMS, Items, STATE, State, Validated, Validators};
use crate::{Error, Result};

/// The time of the feeds: the clock, or a test's.
pub type Now = Box<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// What a fetch did, per subscription.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FetchReport {
    pub feeds: Vec<FeedReport>,
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

/// A feeds folder, loaded.
pub struct Feeds {
    dir: PathBuf,
    config: Config,
    items: Items,
    state: State,
    http: Validated,
    now: Now,
}

impl Feeds {
    /// The feeds of a folder (created if it is not there), on the clock.
    pub fn open(dir: &Path) -> Result<Feeds> {
        Feeds::open_with(dir, Box::new(Utc::now))
    }

    /// The feeds of a folder, on another clock.
    pub fn open_with(dir: &Path, now: Now) -> Result<Feeds> {
        let dir = store::folder(dir)?;
        Ok(Feeds {
            config: store::read(&dir, CONFIG)?,
            items: store::read(&dir, ITEMS)?,
            state: store::read(&dir, STATE)?,
            http: store::read(&dir, HTTP)?,
            dir,
            now,
        })
    }

    pub fn subscriptions(&self) -> &[Subscription] {
        &self.config.subscriptions
    }

    /// A new subscription of a feed's address, unless it is one already
    /// (http and https are one).
    pub fn subscribe(&mut self, url: &str, kind: SourceKind) -> Result<Subscription> {
        let url = canonical_url(url)?;
        let key = feed_key(&url);
        if let Some(known) = self
            .subscriptions()
            .iter()
            .find(|s| feed_key(&s.url) == key)
        {
            return Err(Error::Subscribed {
                url,
                id: known.id.clone(),
            });
        }
        let subscription = Subscription {
            id: short_id(&format!("feed:{key}")),
            url,
            title: None,
            kind,
            journal: None,
            fetch_pages: false,
        };
        self.config.subscriptions.push(subscription.clone());
        store::write(&self.dir, CONFIG, &self.config)?;
        Ok(subscription)
    }

    /// Ends a subscription; its items stay, until pruned.
    pub fn unsubscribe(&mut self, id: &str) -> Result<Subscription> {
        let index = self
            .subscriptions()
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| Error::NoSubscription { id: id.to_string() })?;
        let subscription = self.config.subscriptions.remove(index);
        self.http.remove(id);
        store::write(&self.dir, CONFIG, &self.config)?;
        store::write(&self.dir, HTTP, &self.http)?;
        Ok(subscription)
    }

    /// Fetches every subscription, one after the other, into the items; a
    /// feed that fails is reported and the others go on.
    pub fn fetch(&mut self, http: &dyn Http) -> Result<FetchReport> {
        let mut report = FetchReport::default();
        for i in 0..self.config.subscriptions.len() {
            let outcome = self.fetch_one(i, http);
            let subscription = self.config.subscriptions[i].id.clone();
            report.feeds.push(FeedReport {
                subscription,
                outcome,
            });
        }
        store::write(&self.dir, CONFIG, &self.config)?;
        store::write(&self.dir, ITEMS, &self.items)?;
        store::write(&self.dir, STATE, &self.state)?;
        store::write(&self.dir, HTTP, &self.http)?;
        Ok(report)
    }

    /// Fetches a subscription: a conditional GET, the feed parsed and merged.
    fn fetch_one(&mut self, i: usize, http: &dyn Http) -> Outcome {
        let subscription = &self.config.subscriptions[i];
        let id = subscription.id.clone();
        let known = self.http.get(&id).cloned().unwrap_or_default();
        let request = Request {
            url: subscription.url.clone(),
            accept: Some(FEED_ACCEPT.to_string()),
            if_none_match: known.etag.clone(),
            if_modified_since: known.last_modified.clone(),
        };
        let reply = match http.get(&request) {
            Ok(reply) => reply,
            Err(message) => return Outcome::Failed(message),
        };
        let validators = Validators {
            etag: reply.etag.clone().or(known.etag),
            last_modified: reply.last_modified.clone().or(known.last_modified),
        };
        match reply.status {
            304 => {
                self.http.insert(id, validators);
                Outcome::Unchanged
            }
            200..=299 => match parse(&reply.body, Some(&reply.url), subscription) {
                Ok(parsed) => {
                    let entries = parsed.entries.len();
                    let new = self.merge(&id, parsed.entries);
                    let subscription = &mut self.config.subscriptions[i];
                    if subscription.title.is_none() {
                        subscription.title = parsed.title;
                    }
                    self.http.insert(id, validators);
                    Outcome::Fetched { entries, new }
                }
                Err(e) => Outcome::Failed(e.to_string()),
            },
            status => {
                let retry = reply
                    .retry_after
                    .map(|r| format!(", retry after {r}"))
                    .unwrap_or_default();
                Outcome::Failed(format!("HTTP {status}{retry}"))
            }
        }
    }

    /// The entries of a subscription's feed into the items; the number of new
    /// items. Only these entries are listed by it now, and the triage of a
    /// merged item follows it.
    fn merge(&mut self, subscription: &str, entries: Vec<crate::Entry>) -> usize {
        let now = (self.now)();
        for item in &mut self.items.items {
            for source in &mut item.sources {
                if source.entry.source == subscription {
                    source.listed = false;
                }
            }
        }
        let mut merger = Merger::new(&mut self.items.items, &mut self.items.aliases);
        let new = entries
            .into_iter()
            .map(|e| merger.add(e, now))
            .filter(|&new| new)
            .count();
        for (absorbed, kept) in merger.finish() {
            if let Some(mark) = self.state.remove(&absorbed) {
                let merged = match self.state.get(&kept) {
                    Some(&other) => mark.stronger(other),
                    None => mark,
                };
                self.state.insert(kept, merged);
            }
        }
        new
    }

    pub fn items(&self) -> &[Item] {
        &self.items.items
    }

    /// The id an item has now: its own, or that of the item it was merged into.
    pub fn resolve<'a>(&'a self, id: &'a str) -> &'a str {
        self.items.aliases.get(id).map_or(id, String::as_str)
    }

    /// An item, by its id or an alias.
    pub fn item(&self, id: &str) -> Option<&Item> {
        let id = self.resolve(id);
        self.items().iter().find(|i| i.id == id)
    }

    /// The triage of an item, by its id or an alias: new if never set.
    pub fn triage(&self, id: &str) -> Triage {
        self.mark(id).map_or(Triage::New, |m| m.triage)
    }

    /// The triage of an item and when it was set.
    pub fn mark(&self, id: &str) -> Option<&Mark> {
        self.state.get(self.resolve(id))
    }

    /// Sets the triage of an item, by its id or an alias.
    pub fn set_triage(&mut self, id: &str, triage: Triage) -> Result<()> {
        let item = self
            .item(id)
            .ok_or_else(|| Error::NoItem { id: id.to_string() })?;
        let mark = Mark {
            triage,
            at: (self.now)(),
        };
        self.state.insert(item.id.clone(), mark);
        store::write(&self.dir, STATE, &self.state)
    }

    /// Drops the items first seen more than `days` ago that no feed lists any
    /// more, unless shortlisted or added; the number dropped.
    pub fn prune(&mut self, days: u32) -> Result<usize> {
        let before = (self.now)() - TimeDelta::days(days.into());
        let state = &self.state;
        let old = |item: &Item| {
            let triage = state.get(&item.id).map_or(Triage::New, |m| m.triage);
            !triage.kept() && !item.listed() && item.first_seen() < before
        };
        let (dropped, kept): (Vec<Item>, Vec<Item>) = std::mem::take(&mut self.items.items)
            .into_iter()
            .partition(old);
        self.items.items = kept;
        for item in &dropped {
            self.state.remove(&item.id);
        }
        let dropped_ids: Vec<&String> = dropped.iter().map(|i| &i.id).collect();
        self.items
            .aliases
            .retain(|_, kept| !dropped_ids.contains(&&*kept));
        store::write(&self.dir, ITEMS, &self.items)?;
        store::write(&self.dir, STATE, &self.state)?;
        Ok(dropped.len())
    }
}
