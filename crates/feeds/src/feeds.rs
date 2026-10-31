//! The feeds of a folder: subscribing, fetching every subscription into the
//! items (the rules triaging the new ones, and what follows: the papers of
//! posts, the lookups by DOI), the triage of items, and the pruning of old
//! ones.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use papers_core::{Like, Likeness};

use crate::http::{Access, FEED_ACCEPT, Http, Request, failure};
use crate::interest::{self, Lexicon};
use crate::merge::Merger;
use crate::model::{Entry, Feed, Item, Mark, SourceKind, Subscription, Triage};
use crate::normalize::{is_attachment_doi, short_id};
use crate::parse::parse;
use crate::store::{
    self, CONFIG, Config, HTTP, ITEMS, Items, LOOKUPS, Lookups, STATE, State, Validated, Validators,
};
use crate::{Error, Result};

/// The time of the feeds: the clock, or a test's.
pub type Now = Box<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// What a fetch did, per subscription, and what followed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FetchReport {
    pub feeds: Vec<FeedReport>,
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

/// A feeds folder, loaded.
pub struct Feeds {
    pub(crate) dir: PathBuf,
    pub(crate) config: Config,
    /// the interests of `config`, compiled
    pub(crate) lexicons: Vec<Lexicon>,
    pub(crate) items: Items,
    pub(crate) state: State,
    pub(crate) http: Validated,
    pub(crate) lookups: Lookups,
    pub(crate) now: Now,
    /// the library's papers, to tell how like them an item is (`like:`)
    pub(crate) likeness: Option<Arc<Likeness>>,
}

impl Feeds {
    /// The feeds of a folder (created if it is not there), on the clock.
    pub fn open(dir: &Path) -> Result<Feeds> {
        Feeds::open_with(dir, Box::new(Utc::now))
    }

    /// The feeds of a folder, on another clock.
    pub fn open_with(dir: &Path, now: Now) -> Result<Feeds> {
        let dir = store::folder(dir)?;
        let config: Config = store::read(&dir, CONFIG)?;
        let mut feeds = Feeds {
            lexicons: interest::compile(&config.interests, config.title_boost)?,
            config,
            items: store::read(&dir, ITEMS)?,
            state: store::read(&dir, STATE)?,
            http: store::read(&dir, HTTP)?,
            lookups: store::read(&dir, LOOKUPS)?,
            dir,
            now,
            likeness: None,
        };
        feeds.prune()?;
        Ok(feeds)
    }

    pub fn subscriptions(&self) -> &[Subscription] {
        &self.config.subscriptions
    }

    /// A new subscription of a feed (its address canonical) or a sweep,
    /// unless it is one already (http and https are one).
    pub fn subscribe(&mut self, feed: Feed, kind: SourceKind) -> Result<Subscription> {
        let feed = feed.canonical()?;
        let key = feed.key();
        if let Some(known) = self.subscriptions().iter().find(|s| s.feed.key() == key) {
            return Err(Error::Subscribed {
                feed: feed.to_string(),
                id: known.id.clone(),
            });
        }
        let subscription = Subscription {
            id: short_id(&key),
            feed,
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

    /// Changes what the user names or sets on a subscription (its title,
    /// journal, kind, `fetch_pages`); its id and feed stay as they are.
    pub fn edit_subscription(
        &mut self,
        id: &str,
        edit: impl FnOnce(&mut Subscription),
    ) -> Result<Subscription> {
        let subscription = self
            .config
            .subscriptions
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::NoSubscription { id: id.to_string() })?;
        let feed = subscription.feed.clone();
        edit(subscription);
        subscription.id = id.to_string();
        subscription.feed = feed;
        let edited = subscription.clone();
        store::write(&self.dir, CONFIG, &self.config)?;
        Ok(edited)
    }

    /// The days a sweep looks back the first time.
    pub fn lookback_days(&self) -> u32 {
        self.config.lookback_days
    }

    /// Sets the days a sweep looks back the first time.
    pub fn set_lookback_days(&mut self, days: u32) -> Result<()> {
        self.config.lookback_days = days;
        store::write(&self.dir, CONFIG, &self.config)
    }

    /// Fetches every subscription, one after the other, into the items, then
    /// what follows (the papers of posts, the lookups by DOI); a feed that
    /// fails is reported and the others go on. The APIs are asked with
    /// `access`. The rules then triage the new items (their abstracts looked
    /// up), and old items go.
    pub fn fetch(&mut self, http: &dyn Http, access: &Access) -> Result<FetchReport> {
        let known: BTreeSet<String> = self.items().iter().map(|i| i.id.clone()).collect();
        let mut report = FetchReport::default();
        for i in 0..self.config.subscriptions.len() {
            let outcome = self.fetch_subscription(i, http, access);
            let subscription = self.config.subscriptions[i].id.clone();
            report.feeds.push(FeedReport {
                subscription,
                outcome,
            });
        }
        let budget = self.complete(http, access);
        report.lookups = budget.lookups_used();
        report.pages = budget.pages_used();
        self.apply_rules(&known);
        self.drop_old();
        self.save()?;
        Ok(report)
    }

    /// Every file of the folder.
    pub(crate) fn save(&self) -> Result<()> {
        store::write(&self.dir, CONFIG, &self.config)?;
        store::write(&self.dir, ITEMS, &self.items)?;
        store::write(&self.dir, STATE, &self.state)?;
        store::write(&self.dir, HTTP, &self.http)?;
        store::write(&self.dir, LOOKUPS, &self.lookups)
    }

    /// Fetches a feed: a conditional GET, the feed parsed and merged.
    pub(crate) fn fetch_feed(&mut self, i: usize, url: &str, http: &dyn Http) -> Outcome {
        let subscription = &self.config.subscriptions[i];
        let id = subscription.id.clone();
        let known = self.http.get(&id).cloned().unwrap_or_default();
        let request = Request {
            url: url.to_string(),
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
            swept: None,
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
            _ => Outcome::Failed(failure(&reply)),
        }
    }

    /// The entries of a subscription's feed into the items; the number of new
    /// items. Only these entries are listed by it now.
    pub(crate) fn merge(&mut self, subscription: &str, entries: Vec<Entry>) -> usize {
        for item in &mut self.items.items {
            for source in &mut item.sources {
                if source.entry.source == subscription {
                    source.listed = false;
                }
            }
        }
        self.add(entries, true)
    }

    /// Entries into the items, listed by their feed or not (a lookup's),
    /// those of a paper's review comments and supplements left out; the
    /// number of new items. The triage of a merged item follows it.
    pub(crate) fn add(&mut self, entries: Vec<Entry>, listed: bool) -> usize {
        let now = (self.now)();
        let mut merger = Merger::new(&mut self.items.items, &mut self.items.aliases);
        let new = entries
            .into_iter()
            .filter(|e| !e.doi.as_deref().is_some_and(is_attachment_doi))
            .map(|e| merger.add(e, now, listed))
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

    /// Sets the index of the library's papers that `like:`, the sort by
    /// likeness and [`Feeds::like`] use; none: `like:` matches nothing.
    pub fn set_likeness(&mut self, likeness: Option<Arc<Likeness>>) {
        self.likeness = likeness;
    }

    pub fn likeness(&self) -> Option<&Arc<Likeness>> {
        self.likeness.as_ref()
    }

    /// How like the library's papers an item is, by its title and summary;
    /// none without an index.
    pub fn like(&self, item: &Item) -> Option<Like> {
        let summary = item.summary.as_deref().unwrap_or("");
        (self.likeness.as_ref()).map(|index| index.like(&item.title, summary))
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

    /// The days an item no feed lists is kept, unless shortlisted or added.
    pub fn keep_days(&self) -> u32 {
        self.config.keep_days
    }

    pub fn set_keep_days(&mut self, days: u32) -> Result<()> {
        self.config.keep_days = days;
        store::write(&self.dir, CONFIG, &self.config)
    }

    /// Drops the items first seen more than `keep_days` ago that no feed
    /// lists any more, unless shortlisted or added, and the lookups older
    /// (which may then be made again); the number of items dropped. On
    /// opening and after a fetch too.
    pub fn prune(&mut self) -> Result<usize> {
        let dropped = self.drop_old();
        if dropped > 0 {
            store::write(&self.dir, ITEMS, &self.items)?;
            store::write(&self.dir, STATE, &self.state)?;
        }
        store::write(&self.dir, LOOKUPS, &self.lookups)?;
        Ok(dropped)
    }

    /// [`Feeds::prune`], not written.
    fn drop_old(&mut self) -> usize {
        let before = (self.now)() - TimeDelta::days(self.config.keep_days.into());
        self.lookups.retain(|_, &mut at| at >= before);
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
        dropped.len()
    }
}
