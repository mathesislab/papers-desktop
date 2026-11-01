//! The feeds of a folder: subscribing, entries into the items (a fetch's,
//! [`crate::fetch`]), the triage of items, and the pruning of old ones.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};

use crate::interest::{self, Lexicon};
use crate::library::LibraryFacts;
use crate::merge::{Merger, refresh};
use crate::model::{
    Entry, Feed, Item, Mark, Origin, Reminder, Subscription, SubscriptionSettings, Triage,
};
use crate::normalize::is_attachment_doi;
use crate::parse::kind_of;
use crate::store::{
    self, CONFIG, Config, FORMAT, FeedSettings, HTTP, ITEMS, Items, LOOKUPS, Lookup, Lookups,
    STATE, State, Validated,
};
use crate::{Error, Result};

/// The time of the feeds: the clock, or a test's.
pub type Now = Box<dyn Fn() -> DateTime<Utc> + Send + Sync>;

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
    /// what is known of the library (`like:`, `is:inlibrary`)
    pub(crate) library: Option<Arc<LibraryFacts>>,
}

impl Feeds {
    /// The feeds of a folder (created if it is not there), on the clock.
    pub fn open(dir: &Path) -> Result<Feeds> {
        Feeds::open_with(dir, Box::new(Utc::now))
    }

    /// The feeds of a folder, on another clock; the items of an older
    /// format brought up to this one.
    pub fn open_with(dir: &Path, now: Now) -> Result<Feeds> {
        let dir = store::folder(dir)?;
        let config: Config = store::read(&dir, CONFIG)?;
        let mut feeds = Feeds {
            lexicons: interest::compile(&config.settings.interests, config.settings.title_boost)?,
            config,
            items: store::read(&dir, ITEMS)?,
            state: store::read(&dir, STATE)?,
            http: store::read(&dir, HTTP)?,
            lookups: store::read(&dir, LOOKUPS)?,
            dir,
            now,
            library: None,
        };
        feeds.migrate()?;
        feeds.prune()?;
        Ok(feeds)
    }

    /// A folder of an older format brought up to [`FORMAT`], written: the
    /// origin of each entry (1), the pages read by their URL (2), the items
    /// of one page one (3).
    fn migrate(&mut self) -> Result<()> {
        let format = self.items.format;
        if format >= FORMAT {
            return Ok(());
        }
        if format < 1 {
            let subscriptions = &self.config.subscriptions;
            for source in self.items.items.iter_mut().flat_map(|i| &mut i.sources) {
                let subscription = subscriptions.iter().find(|s| s.id == source.entry.source);
                source.entry.origin = Origin::of_old(&source.entry, subscription);
            }
        }
        if format < 2 {
            // the post's id after `page:`, which merging leaves behind
            let read: Vec<(String, DateTime<Utc>)> = (self.lookups.iter())
                .filter_map(|(key, &at)| Some((key.strip_prefix("page:")?.to_string(), at)))
                .collect();
            for (id, at) in read {
                self.lookups.remove(&format!("page:{id}"));
                if let Some(url) = self.item(&id).and_then(|i| i.url.clone()) {
                    self.lookups.insert(Lookup::Page(&url).key(), at);
                }
            }
        }
        if format < 3 {
            let mut merger = Merger::new(&mut self.items.items, &mut self.items.aliases);
            merger.collapse_pages();
            let merged = merger.finish();
            self.keep_marks(merged);
        }
        self.items.format = FORMAT;
        store::write(&self.dir, ITEMS, &self.items)?;
        store::write(&self.dir, STATE, &self.state)?;
        store::write(&self.dir, LOOKUPS, &self.lookups)
    }

    pub fn subscriptions(&self) -> &[Subscription] {
        &self.config.subscriptions
    }

    /// The subscription an entry was fetched under, if it is one (not a
    /// lookup's or a search's) and it still is.
    pub fn subscription_of(&self, entry: &Entry) -> Option<&Subscription> {
        self.subscriptions().iter().find(|s| s.id == entry.source)
    }

    /// The subscriptions of an item's entries, each once, in their order.
    pub fn subscriptions_of(&self, item: &Item) -> Vec<&Subscription> {
        let mut subscriptions: Vec<&Subscription> = Vec::new();
        for subscription in item
            .sources
            .iter()
            .filter_map(|s| self.subscription_of(&s.entry))
        {
            if !subscriptions.iter().any(|s| s.id == subscription.id) {
                subscriptions.push(subscription);
            }
        }
        subscriptions
    }

    /// A new subscription of a feed (its address canonical) or a sweep, with
    /// its settings, unless it is one already (http and https are one).
    pub fn subscribe(
        &mut self,
        feed: Feed,
        settings: SubscriptionSettings,
    ) -> Result<Subscription> {
        let feed = feed.canonical()?;
        let key = feed.key();
        if let Some(known) = self.subscriptions().iter().find(|s| s.feed.key() == key) {
            return Err(Error::Subscribed {
                feed: feed.to_string(),
                id: known.id.clone(),
            });
        }
        let kind = settings.kind;
        let subscription = Subscription {
            settings: settings.trimmed(),
            ..Subscription::new(feed, kind)
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

    /// The subscription of an id.
    pub fn subscription(&self, id: &str) -> Result<&Subscription> {
        (self.subscriptions().iter())
            .find(|s| s.id == id)
            .ok_or_else(|| Error::NoSubscription { id: id.to_string() })
    }

    /// Sets what the user names or sets on a subscription (its title,
    /// journal, kind, `fetch_pages`, `boost`). Another kind is its items' at
    /// once, and the next fetch reads its feed whole again (a post keeps its
    /// HTML, a paper does not).
    pub fn edit_subscription(
        &mut self,
        id: &str,
        settings: SubscriptionSettings,
    ) -> Result<Subscription> {
        let kind = self.subscription(id)?.settings.kind;
        let subscription = (self.config.subscriptions.iter_mut())
            .find(|s| s.id == id)
            .expect("a subscription");
        subscription.settings = settings.trimmed();
        let edited = subscription.clone();
        store::write(&self.dir, CONFIG, &self.config)?;
        if edited.settings.kind != kind {
            self.rekind(&edited);
            self.http.remove(id);
            store::write(&self.dir, HTTP, &self.http)?;
            self.save()?;
        }
        Ok(edited)
    }

    /// The entries of a subscription of another kind: theirs, and their items'.
    fn rekind(&mut self, subscription: &Subscription) {
        for item in &mut self.items.items {
            let mut changed = false;
            for source in item
                .sources
                .iter_mut()
                .filter(|s| s.entry.source == subscription.id)
            {
                let e = &mut source.entry;
                e.kind = kind_of(
                    subscription.settings.kind,
                    e.doi.as_deref(),
                    e.arxiv.is_some(),
                );
                if !e.kind.is_post() {
                    e.content = None;
                }
                changed = true;
            }
            if changed {
                refresh(item);
            }
        }
    }

    /// The days a sweep looks back the first time.
    pub fn lookback_days(&self) -> u32 {
        self.config.settings.lookback_days
    }

    /// The settings of the feeds.
    pub fn settings(&self) -> &FeedSettings {
        &self.config.settings
    }

    /// Sets the settings of the feeds, if every interest compiles; else
    /// nothing changes.
    pub fn set_settings(&mut self, settings: FeedSettings) -> Result<()> {
        self.lexicons = interest::compile(&settings.interests, settings.title_boost)?;
        self.config.settings = settings;
        store::write(&self.dir, CONFIG, &self.config)
    }

    /// Every file of the folder.
    pub(crate) fn save(&self) -> Result<()> {
        store::write(&self.dir, CONFIG, &self.config)?;
        store::write(&self.dir, ITEMS, &self.items)?;
        store::write(&self.dir, STATE, &self.state)?;
        store::write(&self.dir, HTTP, &self.http)?;
        store::write(&self.dir, LOOKUPS, &self.lookups)
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
        let merged = merger.finish();
        self.keep_marks(merged);
        new
    }

    /// The marks of the items merged (absorbed, kept), on the item each went
    /// into: the stronger.
    fn keep_marks(&mut self, merged: Vec<(String, String)>) {
        for (absorbed, kept) in merged {
            if let Some(mark) = self.state.remove(&absorbed) {
                let merged = match self.state.get(&kept) {
                    Some(&other) => mark.stronger(other),
                    None => mark,
                };
                self.state.insert(kept, merged);
            }
        }
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
        let mark = Mark::new(triage, (self.now)());
        self.state.insert(item.id.clone(), mark);
        store::write(&self.dir, STATE, &self.state)
    }

    /// An item, by its id or an alias, new and to be reminded of at `when`:
    /// snoozed till then (now: kept unread, not seen by being shown).
    pub fn remind(&mut self, id: &str, when: DateTime<Utc>) -> Result<()> {
        let item = self
            .item(id)
            .ok_or_else(|| Error::NoItem { id: id.to_string() })?;
        let mark = Mark {
            remind: Some(when),
            ..Mark::new(Triage::New, (self.now)())
        };
        self.state.insert(item.id.clone(), mark);
        store::write(&self.dir, STATE, &self.state)
    }

    /// Where the reminder of an item is now, if it has one.
    pub fn reminder(&self, id: &str) -> Option<Reminder> {
        self.mark(id).and_then(|m| m.reminder((self.now)()))
    }

    /// The days an item no feed lists is kept, unless shortlisted, added or to
    /// be reminded of.
    pub fn keep_days(&self) -> u32 {
        self.config.settings.keep_days
    }

    /// Drops the items first seen more than `keep_days` ago that no feed
    /// lists any more, unless shortlisted or added, and the lookups older
    /// (which may then be made again); the number of items dropped. On
    /// opening and after a fetch too; written if anything went.
    pub fn prune(&mut self) -> Result<usize> {
        let lookups = self.lookups.len();
        let dropped = self.drop_old();
        if dropped > 0 {
            store::write(&self.dir, ITEMS, &self.items)?;
            store::write(&self.dir, STATE, &self.state)?;
        }
        if self.lookups.len() < lookups {
            store::write(&self.dir, LOOKUPS, &self.lookups)?;
        }
        Ok(dropped)
    }

    /// [`Feeds::prune`], not written.
    pub(crate) fn drop_old(&mut self) -> usize {
        let before = (self.now)() - TimeDelta::days(self.config.settings.keep_days.into());
        self.lookups.retain(|_, &mut at| at >= before);
        let state = &self.state;
        let old = |item: &Item| {
            let mark = state.get(&item.id);
            let kept = mark.is_some_and(|m| m.triage.kept() || m.remind.is_some());
            !kept && !item.listed() && item.first_seen() < before
        };
        let dropped: Vec<String> = (self.items.items.iter())
            .filter(|i| old(i))
            .map(|i| i.id.clone())
            .collect();
        self.forget(&dropped);
        dropped.len()
    }

    /// Items gone, their triage and aliases with them; not written.
    pub(crate) fn forget(&mut self, ids: &[String]) {
        let ids: BTreeSet<&String> = ids.iter().collect();
        self.items.items.retain(|i| !ids.contains(&i.id));
        self.state.retain(|id, _| !ids.contains(id));
        self.items.aliases.retain(|_, kept| !ids.contains(kept));
    }
}
