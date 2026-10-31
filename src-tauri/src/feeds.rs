//! The Feeds tab and the Feeds section of the settings: the commands of
//! `src/lib/feeds/api.ts` over the feeds of `app_data_dir/feeds`
//! (crates/feeds), opened at the first command and kept.
//!
//! A fetch takes a while: it runs on a copy of the feeds opened from the
//! folder, so the tab reads and triages the feeds as they were meanwhile.
//! Each change made while it runs is applied at once and kept in a journal,
//! then applied again to the fetched copy, which takes the place of the
//! feeds. One fetch at a time: a call while one runs waits for it and
//! answers its summary (the tab opening while the background polling
//! fetches does not fetch twice). Finding the papers of a post works on a
//! copy too, after a running fetch.
//!
//! The requests go through the crate's network (a browser's user agent,
//! which every publisher answers), paced per host; Crossref and OpenAlex
//! are asked with the e-mail address and the OpenAlex key of the settings,
//! each sent only when set. The last fetch's summary, and how each
//! subscription's last fetch ended, are kept in `feeds-fetched.json`.
//!
//! *From your library*: the journals of the library's papers, each followed
//! by the suggested list's subscription, else by its ISSN, from the papers
//! or looked up in OpenAlex by its name (kept in the folder's
//! `journals.json`, opened at the first ask). A journal of the library or of
//! a search of OpenAlex is followed by the feed its home page links to (the
//! page read with the page user agent of the Add sheet, `fetch::page_agent`;
//! the feed as it will be fetched), else, if the user chooses, by its ISSN.
//!
//! *Like in your library*: the index of the library's papers (core's
//! `Likeness`) is built on a thread of its own whenever the library is read
//! (at the start, after a change of its files or a write), then given to the
//! feeds and to every copy a fetch opens; the page is told (`feeds-changed`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::Duration;

use chrono::{DateTime, Datelike, TimeDelta, Utc};
use papers_core::{Identity, Library, Likeness, Matching, Span};
use papers_feeds::http::{Paced, Pacing};
use papers_feeds::{
    Access, Confidence, Feed, Feeds, FetchReport, Interest, Item, ItemKind, Journal, Journals,
    LOOKUP, Network, Outcome, Pdf, Rule, Score, Search, SiteFeed, Sort, SourceKind, Stage,
    Subscription, Triage, Via, ViewRequest as CrateRequest, paragraphs,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::fetch::{blocking, page_agent};
use crate::settings::Settings;

/// The events of the page: the items changed; a fetch started (true) or ended.
const CHANGED: &str = "feeds-changed";
const FETCHING: &str = "feeds-fetching";

/// The background polling: its first look a while after the start, then
/// each minute, fetching when the last fetch is older than the setting.
const FIRST: Duration = Duration::from_secs(120);
const TICK: Duration = Duration::from_secs(60);

/// The fewest minutes between two fetches of the polling.
const LEAST_MINUTES: u32 = 5;

/// Opens the feeds of a folder: on the clock, or a test's.
type Opener = Box<dyn Fn(&Path) -> papers_feeds::Result<Feeds> + Send + Sync>;

/// The feeds of the app, and what is kept of their fetches.
pub struct FeedsState {
    dir: PathBuf,
    /// `feeds-fetched.json`: the last summary, each subscription's last fetch
    fetched_path: PathBuf,
    live: Mutex<Live>,
    /// held by the fetch (or the finding of papers) that runs
    running: Mutex<()>,
    /// read from its file at the first ask
    fetched: Mutex<Option<Fetched>>,
    /// the free copy of each DOI asked for, this session
    pdfs: Mutex<HashMap<String, Pdf>>,
    /// the journals looked up in OpenAlex, opened at the first ask
    journals: Mutex<Option<Journals>>,
    open: Opener,
    /// counts the builds of the index of the library: one that ends after a
    /// newer one is dropped
    builds: AtomicU64,
}

/// The feeds the commands see, the changes made while a copy is worked on,
/// and the index of the library that the feeds and their copies are given.
#[derive(Default)]
struct Live {
    feeds: Option<Feeds>,
    journal: Option<Vec<Change>>,
    likeness: Option<Arc<Likeness>>,
    /// the build of `likeness`
    build: u64,
}

/// A change of the feeds, kept to apply again to a copy that was fetched.
#[derive(Debug, Clone)]
pub enum Change {
    Triage(Vec<String>, Triage),
    /// a feed, the kind of its items, the title it is given (else the feed's own)
    Subscribe(Feed, SourceKind, Option<String>),
    Unsubscribe(String),
    Update(String, SubscriptionPatch),
    Settings(Box<Configured>),
    /// an OPML text
    Import(String),
    /// the saved searches, all of them
    Searches(Vec<Search>),
}

/// What a change gives back.
#[derive(Debug)]
pub enum Applied {
    Done,
    Subscription(Subscription),
    Count(usize),
}

/// The settings of the feeds kept in their folder.
#[derive(Debug, Clone)]
pub struct Configured {
    interests: Vec<Interest>,
    title_boost: f64,
    rules: Vec<Rule>,
    keep_days: u32,
    lookback_days: u32,
}

impl Change {
    fn apply(&self, feeds: &mut Feeds) -> papers_feeds::Result<Applied> {
        Ok(match self {
            Change::Triage(ids, triage) => {
                for id in ids {
                    feeds.set_triage(id, *triage)?;
                }
                Applied::Done
            }
            Change::Subscribe(feed, kind, title) => {
                let subscription = feeds.subscribe(feed.clone(), *kind)?;
                Applied::Subscription(match title {
                    Some(title) => feeds
                        .edit_subscription(&subscription.id, |s| s.title = Some(title.clone()))?,
                    None => subscription,
                })
            }
            Change::Unsubscribe(id) => {
                feeds.unsubscribe(id)?;
                Applied::Done
            }
            Change::Update(id, patch) => {
                let patch = patch.clone();
                Applied::Subscription(feeds.edit_subscription(id, |s| patch.apply(s))?)
            }
            Change::Settings(c) => {
                // the interests first: they may not compile, and then nothing changes
                feeds.set_interests(c.interests.clone(), c.title_boost)?;
                feeds.set_rules(c.rules.clone())?;
                feeds.set_keep_days(c.keep_days)?;
                feeds.set_lookback_days(c.lookback_days)?;
                Applied::Done
            }
            Change::Import(text) => Applied::Count(feeds.import_opml(text)?),
            Change::Searches(searches) => {
                feeds.set_searches(searches.clone())?;
                Applied::Done
            }
        })
    }
}

/// What a fetch did, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchSummary {
    pub at: DateTime<Utc>,
    pub fetched: usize,
    pub unchanged: usize,
    pub failed: Vec<Failed>,
    /// items new in this fetch
    pub added: usize,
    pub lookups: usize,
    pub pages: usize,
}

/// A subscription whose fetch failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failed {
    pub subscription: String,
    pub title: String,
    pub message: String,
}

/// The last fetch of a subscription: when, and its error if it failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Last {
    pub at: DateTime<Utc>,
    pub error: Option<String>,
}

/// `feeds-fetched.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Fetched {
    summary: Option<FetchSummary>,
    /// by subscription id
    #[serde(default)]
    last: BTreeMap<String, Last>,
}

/// A subscription's title: its own, else what it follows.
fn title_of(subscription: &Subscription) -> String {
    (subscription.title.clone()).unwrap_or_else(|| subscription.feed.to_string())
}

/// The summary of a fetch, and each subscription's last fetch.
fn summary(
    report: &FetchReport,
    subscriptions: &[Subscription],
    added: usize,
    at: DateTime<Utc>,
) -> (FetchSummary, Vec<(String, Last)>) {
    let mut summary = FetchSummary {
        at,
        fetched: 0,
        unchanged: 0,
        failed: Vec::new(),
        added,
        lookups: report.lookups,
        pages: report.pages,
    };
    let mut last = Vec::new();
    for feed in &report.feeds {
        let error = match &feed.outcome {
            Outcome::Fetched { .. } => {
                summary.fetched += 1;
                None
            }
            Outcome::Unchanged => {
                summary.unchanged += 1;
                None
            }
            Outcome::Failed(message) => {
                let subscription = subscriptions.iter().find(|s| s.id == feed.subscription);
                summary.failed.push(Failed {
                    subscription: feed.subscription.clone(),
                    title: subscription.map_or_else(|| feed.subscription.clone(), title_of),
                    message: message.clone(),
                });
                Some(message.clone())
            }
        };
        last.push((feed.subscription.clone(), Last { at, error }));
    }
    (summary, last)
}

impl FeedsState {
    /// The feeds of a folder, not opened yet; what is kept of their fetches in a file.
    pub fn new(dir: PathBuf, fetched_path: PathBuf) -> FeedsState {
        FeedsState::with_opener(dir, fetched_path, Box::new(Feeds::open))
    }

    fn with_opener(dir: PathBuf, fetched_path: PathBuf, open: Opener) -> FeedsState {
        FeedsState {
            dir,
            fetched_path,
            live: Mutex::new(Live::default()),
            running: Mutex::new(()),
            fetched: Mutex::new(None),
            pdfs: Mutex::new(HashMap::new()),
            journals: Mutex::new(None),
            open,
            builds: AtomicU64::new(0),
        }
    }

    /// The feeds of the folder, opened, with the index of the library.
    fn opened(&self, live: &Live) -> Result<Feeds, String> {
        let mut feeds = (self.open)(&self.dir).map_err(|e| e.to_string())?;
        feeds.set_likeness(live.likeness.clone());
        Ok(feeds)
    }

    /// The live feeds, opened if they are not yet.
    fn live(&self) -> Result<MutexGuard<'_, Live>, String> {
        let mut live = self.live.lock().unwrap();
        if live.feeds.is_none() {
            live.feeds = Some(self.opened(&live)?);
        }
        Ok(live)
    }

    /// The number of a new build of the index of the library.
    pub fn next_build(&self) -> u64 {
        self.builds.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// The index of the library of build `build`, for the feeds and the
    /// copies opened from now on, unless a newer build was given already:
    /// whether it was taken.
    pub fn set_likeness(&self, build: u64, likeness: Option<Arc<Likeness>>) -> bool {
        let mut live = self.live.lock().unwrap();
        if build < live.build {
            return false;
        }
        live.build = build;
        live.likeness = likeness.clone();
        if let Some(feeds) = &mut live.feeds {
            feeds.set_likeness(likeness);
        }
        true
    }

    /// Something done with the journals looked up, opened if they are not yet.
    pub fn journals<T>(&self, work: impl FnOnce(&mut Journals) -> T) -> Result<T, String> {
        let mut journals = self.journals.lock().unwrap();
        if journals.is_none() {
            *journals = Some(Journals::open(&self.dir).map_err(|e| e.to_string())?);
        }
        Ok(work(journals.as_mut().expect("opened")))
    }

    /// Something read of the feeds.
    pub fn read<T>(&self, read: impl FnOnce(&Feeds) -> T) -> Result<T, String> {
        let live = self.live()?;
        Ok(read(live.feeds.as_ref().expect("opened")))
    }

    /// A change of the feeds, now, and again on the copy a fetch works on.
    pub fn change(&self, change: Change) -> Result<Applied, String> {
        let mut live = self.live()?;
        let feeds = live.feeds.as_mut().expect("opened");
        let applied = change.apply(feeds).map_err(|e| e.to_string())?;
        if let Some(journal) = &mut live.journal {
            journal.push(change);
        }
        Ok(applied)
    }

    /// Work on a copy of the feeds, which then takes their place with the
    /// changes made meanwhile; the running guard is held.
    fn on_copy<T>(
        &self,
        _running: &MutexGuard<()>,
        work: impl FnOnce(&mut Feeds) -> papers_feeds::Result<T>,
    ) -> Result<T, String> {
        let mut copy = {
            let mut live = self.live()?;
            let copy = self.opened(&live)?;
            live.journal = Some(Vec::new());
            copy
        };
        let done = work(&mut copy);
        let mut live = self.live.lock().unwrap();
        let journal = live.journal.take().unwrap_or_default();
        let done = done.map_err(|e| e.to_string())?;
        for change in &journal {
            // what no longer applies (an item gone, a feed subscribed twice) is passed over
            let _ = change.apply(&mut copy);
        }
        // the index built meanwhile
        copy.set_likeness(live.likeness.clone());
        live.feeds = Some(copy);
        Ok(done)
    }

    /// Fetches every subscription, or waits for the fetch that runs and
    /// answers its summary; `fetching` is told when a fetch starts and ends.
    pub fn fetch(
        &self,
        http: &dyn papers_feeds::Http,
        access: &Access,
        fetching: impl Fn(bool),
    ) -> Result<FetchSummary, String> {
        let running = match self.running.try_lock() {
            Ok(running) => running,
            Err(TryLockError::WouldBlock) => {
                drop(self.running.lock().unwrap());
                return self.last_fetch()?.ok_or("the fetch failed".to_string());
            }
            Err(TryLockError::Poisoned(e)) => e.into_inner(),
        };
        fetching(true);
        let fetched = self.on_copy(&running, |feeds| {
            let known: HashSet<String> = feeds.items().iter().map(|i| i.id.clone()).collect();
            let report = feeds.fetch(http, access)?;
            let added = feeds
                .items()
                .iter()
                .filter(|i| !known.contains(&i.id))
                .count();
            Ok((report, added, feeds.subscriptions().to_vec()))
        });
        fetching(false);
        let (report, added, subscriptions) = fetched?;
        let (summary, last) = summary(&report, &subscriptions, added, Utc::now());
        self.keep(&summary, last, &subscriptions)?;
        Ok(summary)
    }

    /// Keeps a fetch's summary and each subscription's last fetch, those of
    /// subscriptions gone dropped.
    fn keep(
        &self,
        summary: &FetchSummary,
        last: Vec<(String, Last)>,
        subscriptions: &[Subscription],
    ) -> Result<(), String> {
        let mut fetched = self.fetched()?;
        let kept = fetched.as_mut().expect("read");
        kept.summary = Some(summary.clone());
        kept.last.extend(last);
        kept.last
            .retain(|id, _| subscriptions.iter().any(|s| &s.id == id));
        let json = serde_json::to_string_pretty(kept).unwrap();
        let path = &self.fetched_path;
        let error = |e: std::io::Error| format!("{}: {e}", path.display());
        std::fs::create_dir_all(path.parent().unwrap()).map_err(error)?;
        std::fs::write(path, json + "\n").map_err(error)
    }

    /// What is kept of the fetches, read from its file the first time.
    fn fetched(&self) -> Result<MutexGuard<'_, Option<Fetched>>, String> {
        let mut fetched = self.fetched.lock().unwrap();
        if fetched.is_none() {
            let kept = match std::fs::read_to_string(&self.fetched_path) {
                // a file that does not parse is started again
                Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Fetched::default(),
                Err(e) => return Err(format!("{}: {e}", self.fetched_path.display())),
            };
            *fetched = Some(kept);
        }
        Ok(fetched)
    }

    /// The summary of the last fetch.
    pub fn last_fetch(&self) -> Result<Option<FetchSummary>, String> {
        Ok(self.fetched()?.as_ref().and_then(|f| f.summary.clone()))
    }

    /// The last fetch of each subscription.
    fn lasts(&self) -> Result<BTreeMap<String, Last>, String> {
        Ok(self
            .fetched()?
            .as_ref()
            .map(|f| f.last.clone())
            .unwrap_or_default())
    }

    /// The papers behind a post, looked for again, after a running fetch.
    pub fn find_papers(
        &self,
        id: &str,
        http: &dyn papers_feeds::Http,
        access: &Access,
    ) -> Result<Vec<Linked>, String> {
        let running = self.running.lock().unwrap();
        let found = self.on_copy(&running, |feeds| feeds.find_papers(id, http, access))?;
        self.read(|feeds| found.links.iter().map(|l| linked(feeds, l)).collect())
    }

    /// The free copy of an item, asked once per DOI this session.
    pub fn pdf(
        &self,
        id: &str,
        http: &dyn papers_feeds::Http,
        access: &Access,
    ) -> Result<Pdf, String> {
        let item = self.read(|feeds| feeds.item(id).cloned())?;
        let item = item.ok_or_else(|| format!("{id}: no such item"))?;
        let key = item.doi.clone();
        if let Some(pdf) = key
            .as_ref()
            .and_then(|doi| self.pdfs.lock().unwrap().get(doi).cloned())
        {
            return Ok(pdf);
        }
        let pdf = papers_feeds::pdf(&item, http, access)?;
        if let Some(doi) = key {
            self.pdfs.lock().unwrap().insert(doi, pdf.clone());
        }
        Ok(pdf)
    }
}

/// The library's papers by DOI and by title and year, to find a feed's item in it.
#[derive(Debug, Default)]
pub struct InLibrary {
    doi: HashMap<String, Vec<String>>,
    title: HashMap<String, Vec<String>>,
}

impl InLibrary {
    pub fn of(library: &Library) -> InLibrary {
        let mut index = InLibrary::default();
        for item in library.items() {
            let Identity { doi, title } = item.identity();
            for (map, key) in [(&mut index.doi, doi), (&mut index.title, title)] {
                if let Some(key) = key {
                    map.entry(key).or_default().push(item.id.clone());
                }
            }
        }
        index
    }

    /// The ids of the entries of the same paper: its DOI, or its title and year.
    pub fn same(&self, item: &Item) -> Vec<String> {
        let year = item.date.map(|d| d.year());
        let Identity { doi, title } = Identity::of(item.doi.as_deref(), &item.title, year);
        let by_doi = doi.and_then(|d| self.doi.get(&d));
        let by_title = title.and_then(|t| self.title.get(&t));
        let mut ids: Vec<String> = Vec::new();
        for id in by_doi.into_iter().chain(by_title).flatten() {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ids
    }

    /// The ids of the entries of another version of it (by DOI), not of itself.
    pub fn versions(&self, item: &Item, same: &[String]) -> Vec<String> {
        let own = item.doi.as_deref().map(str::to_lowercase);
        let mut ids: Vec<String> = Vec::new();
        for version in &item.versions {
            let doi = version.doi.to_lowercase();
            if Some(&doi) == own.as_ref() {
                continue;
            }
            for id in self.doi.get(&doi).into_iter().flatten() {
                if !same.contains(id) && !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
        }
        ids
    }
}

/// What a subscription follows, as the page writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeedOf {
    Rss(String),
    Crossref(String),
    OpenAlex(String),
}

impl From<&Feed> for FeedOf {
    fn from(feed: &Feed) -> FeedOf {
        match feed {
            Feed::Rss { url } => FeedOf::Rss(url.clone()),
            Feed::Crossref { issn } => FeedOf::Crossref(issn.clone()),
            Feed::OpenAlex { filter } => FeedOf::OpenAlex(filter.clone()),
        }
    }
}

impl From<FeedOf> for Feed {
    fn from(feed: FeedOf) -> Feed {
        match feed {
            FeedOf::Rss(url) => Feed::Rss { url },
            FeedOf::Crossref(issn) => Feed::Crossref { issn },
            FeedOf::OpenAlex(filter) => Feed::OpenAlex { filter },
        }
    }
}

/// What the page asks a view for.
#[derive(Debug, Deserialize)]
pub struct ViewRequest {
    query: String,
    sort: Sort,
    kinds: Vec<ItemKind>,
}

/// A row of the list.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedRow {
    id: String,
    kind: ItemKind,
    title: Vec<Span>,
    authors: Vec<Vec<Span>>,
    journal: Option<String>,
    /// the titles of the subscriptions that listed it
    sources: Vec<String>,
    date: Option<DateTime<Utc>>,
    score: f64,
    interests: Vec<Score>,
    /// how like the library it is, 0–100; none before the library is indexed
    like: Option<f64>,
    triage: Triage,
    in_library: Vec<String>,
    has_versions: bool,
    covered_by: usize,
    links: usize,
}

/// The counts of the sidebar, the subscriptions by title, the saved searches by name.
#[derive(Debug, Serialize)]
pub struct FeedCounts {
    all: usize,
    is: BTreeMap<String, usize>,
    kinds: BTreeMap<ItemKind, usize>,
    interests: BTreeMap<String, usize>,
    sources: BTreeMap<String, usize>,
    searches: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
pub struct FeedView {
    rows: Vec<FeedRow>,
    counts: FeedCounts,
}

/// A paper behind a post, and its item.
#[derive(Debug, Serialize)]
pub struct Linked {
    doi: String,
    confidence: Confidence,
    via: Via,
    item: Option<String>,
    title: Option<String>,
}

/// An item, for the details pane and the review.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedDetail {
    id: String,
    kind: ItemKind,
    title: Vec<Span>,
    authors: Vec<String>,
    journal: Option<String>,
    date: Option<DateTime<Utc>>,
    doi: Option<String>,
    arxiv: Option<String>,
    url: Option<String>,
    r#abstract: Vec<Span>,
    abstract_looked_up: bool,
    content: Vec<String>,
    editorial: bool,
    sources: Vec<SourceOf>,
    versions: Vec<VersionOf>,
    links: Vec<Linked>,
    covered_by: Vec<Post>,
    interests: Vec<Score>,
    triage: Triage,
    in_library: Vec<String>,
    version_in_library: Vec<String>,
    like: Option<f64>,
    /// the papers of the library most like it, the nearest first
    nearest: Vec<Nearest>,
}

/// A paper of the library like an item.
#[derive(Debug, Serialize)]
pub struct Nearest {
    id: String,
    key: String,
    title: String,
    /// "Feldmann & Levermann · 2017 · Cryosphere"
    subtitle: String,
    /// of the item's vector and the paper's
    cosine: f64,
}

#[derive(Debug, Serialize)]
pub struct SourceOf {
    id: String,
    title: String,
    kind: SourceKind,
}

#[derive(Debug, Serialize)]
pub struct VersionOf {
    doi: String,
    stage: Stage,
    item: Option<String>,
}

/// A post about a paper.
#[derive(Debug, Serialize)]
pub struct Post {
    id: String,
    title: String,
    source: String,
}

/// A subscription, as the settings list it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionInfo {
    id: String,
    title: String,
    kind: SourceKind,
    feed: FeedOf,
    journal: Option<String>,
    fetch_pages: bool,
    boost: f64,
    last: Option<Last>,
}

/// A change of a subscription, as the page sends it: a field left out stays,
/// a `journal` of null is cleared.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionPatch {
    title: Option<String>,
    kind: Option<SourceKind>,
    #[serde(default, deserialize_with = "present")]
    journal: Option<Option<String>>,
    fetch_pages: Option<bool>,
    boost: Option<f64>,
}

impl SubscriptionPatch {
    /// Sets what it has; a blank title or journal is none: the feed's own
    /// again (a title at the next fetch).
    fn apply(self, subscription: &mut Subscription) {
        let blank = |text: String| Some(text.trim().to_string()).filter(|t| !t.is_empty());
        if let Some(title) = self.title {
            subscription.title = blank(title);
        }
        if let Some(kind) = self.kind {
            subscription.kind = kind;
        }
        if let Some(journal) = self.journal {
            subscription.journal = journal.and_then(blank);
        }
        if let Some(fetch_pages) = self.fetch_pages {
            subscription.fetch_pages = fetch_pages;
        }
        if let Some(boost) = self.boost {
            subscription.boost = boost;
        }
    }
}

/// A field that is there, null or not.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

/// A subscription offered.
#[derive(Debug, Serialize)]
pub struct Suggested {
    title: String,
    kind: SourceKind,
    feed: FeedOf,
    group: String,
}

impl From<papers_feeds::Suggested> for Suggested {
    fn from(s: papers_feeds::Suggested) -> Suggested {
        Suggested {
            feed: FeedOf::from(&s.feed),
            title: s.title,
            kind: s.kind,
            group: s.group,
        }
    }
}

/// A journal of the library's papers, and what follows it.
#[derive(Debug, Serialize)]
pub struct LibraryJournal {
    title: String,
    papers: usize,
    follow: Follow,
}

/// What follows a journal of the library: a suggested subscription, a sweep
/// of its ISSN (none if none was found), nothing found, or not looked up yet.
#[derive(Debug, Serialize)]
#[serde(tag = "by", rename_all = "lowercase")]
pub enum Follow {
    Suggested { suggested: Suggested },
    Journal { journal: Journal },
    Unknown,
    Pending,
}

impl From<papers_feeds::Follow> for Follow {
    fn from(follow: papers_feeds::Follow) -> Follow {
        match follow {
            papers_feeds::Follow::Suggested(s) => Follow::Suggested {
                suggested: s.into(),
            },
            papers_feeds::Follow::Journal(journal) => Follow::Journal { journal },
            papers_feeds::Follow::Unknown => Follow::Unknown,
            papers_feeds::Follow::Pending => Follow::Pending,
        }
    }
}

/// What a journal's site gives: its home page, and what it links to (none
/// without a home page).
#[derive(Debug, Serialize)]
pub struct JournalSite {
    homepage: Option<String>,
    feed: Option<SiteFeed>,
}

/// The journals of the library's papers: the `journal` field of each (or
/// `journaltitle`), with its `issn`.
fn library_journals(library: &Library) -> Vec<papers_feeds::LibraryJournal> {
    let fields: Vec<(String, Option<String>)> = library
        .items()
        .iter()
        .filter_map(|item| {
            let journal = library.field(&item.id, &["journal", "journaltitle"])?;
            Some((journal, library.field(&item.id, &["issn"])))
        })
        .collect();
    papers_feeds::library_journals(fields.iter().map(|(j, issn)| (j.as_str(), issn.as_deref())))
}

/// The settings of the feeds: those of their folder and the polling of the app's.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedSettings {
    interests: Vec<Interest>,
    title_boost: f64,
    rules: Vec<Rule>,
    keep_days: u32,
    lookback_days: u32,
    poll: bool,
    poll_minutes: u32,
}

/// The ids of a subscription's titles, the lookups' left out.
fn source_ids(item: &Item) -> Vec<&str> {
    let mut ids: Vec<&str> = item
        .sources
        .iter()
        .map(|s| s.entry.source.as_str())
        .collect();
    ids.retain(|&id| id != LOOKUP);
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The item of a DOI, its own or one of its versions', but `not` this one.
fn item_of_doi<'a>(feeds: &'a Feeds, doi: &str, not: &str) -> Option<&'a Item> {
    let has = |i: &&Item| i.doi.as_deref() == Some(doi) || i.versions.iter().any(|v| v.doi == doi);
    feeds.items().iter().filter(|i| i.id != not).find(has)
}

/// A paper behind a post, with its item if there is one.
fn linked(feeds: &Feeds, link: &papers_feeds::Linked) -> Linked {
    let item = item_of_doi(feeds, &link.doi, "");
    Linked {
        doi: link.doi.clone(),
        confidence: link.confidence,
        via: link.via,
        item: item.map(|i| i.id.clone()),
        title: item.map(|i| i.title.clone()),
    }
}

/// The view of a request: the rows, each in the library or not, and the counts.
pub fn view(
    feeds: &Feeds,
    library: &Library,
    matching: Matching,
    request: &ViewRequest,
) -> FeedView {
    let index = InLibrary::of(library);
    let inlibrary = |item: &Item| match index.same(item).is_empty() {
        true => Vec::new(),
        false => vec!["inlibrary".to_string()],
    };
    let view = feeds.view(&CrateRequest {
        query: &request.query,
        matching,
        sort: request.sort,
        kinds: &request.kinds,
        is: Some(&inlibrary),
    });
    let titles: HashMap<&str, String> = (feeds.subscriptions().iter())
        .map(|s| (s.id.as_str(), title_of(s)))
        .collect();
    let items: HashMap<&str, &Item> = feeds.items().iter().map(|i| (i.id.as_str(), i)).collect();
    // the posts that link each DOI
    let mut posts: HashMap<&str, HashSet<&str>> = HashMap::new();
    for post in feeds.items().iter().filter(|i| !i.links.is_empty()) {
        for link in &post.links {
            posts.entry(&link.doi).or_default().insert(&post.id);
        }
    }
    let rows = view.rows.into_iter().map(|row| {
        let item = items[row.id.as_str()];
        let dois = item.doi.iter().chain(item.versions.iter().map(|v| &v.doi));
        let covering: HashSet<&str> = dois
            .filter_map(|d| posts.get(d.as_str()))
            .flatten()
            .filter(|&&id| id != item.id)
            .copied()
            .collect();
        FeedRow {
            kind: item.kind,
            journal: item.journal.clone(),
            sources: (source_ids(item).iter())
                .filter_map(|id| titles.get(id).cloned())
                .collect(),
            date: item.date,
            triage: feeds.triage(&item.id),
            in_library: index.same(item),
            has_versions: item.kind == ItemKind::Preprint
                || item.versions.iter().any(|v| v.stage == Stage::Preprint),
            covered_by: covering.len(),
            links: item.links.len(),
            id: row.id,
            title: row.title,
            authors: row.authors,
            score: row.score,
            interests: row.interests,
            like: row.like,
        }
    });
    let mut sources: BTreeMap<String, usize> = BTreeMap::new();
    for (id, count) in view.counts.sources {
        if let Some(title) = titles.get(id.as_str()) {
            *sources.entry(title.clone()).or_default() += count;
        }
    }
    FeedView {
        rows: rows.collect(),
        counts: FeedCounts {
            all: view.counts.all,
            is: view.counts.is,
            kinds: view.counts.kinds,
            interests: view.counts.interests,
            sources,
            searches: view.counts.searches,
        },
    }
}

/// An item, for the details: its texts marked by the query and its interests.
pub fn detail(
    feeds: &Feeds,
    library: &Library,
    matching: Matching,
    id: &str,
    query: &str,
) -> Result<FeedDetail, String> {
    let item = feeds
        .item(id)
        .ok_or_else(|| format!("{id}: no such item"))?;
    let row = feeds.row_of(id, query, matching).expect("an item");
    let subscription = |id: &str| feeds.subscriptions().iter().find(|s| s.id == id);
    let first_source = |item: &Item| {
        let id = source_ids(item).first().map(|id| id.to_string());
        id.map(|id| subscription(&id).map_or(id, title_of))
            .unwrap_or_default()
    };
    let index = InLibrary::of(library);
    let in_library = index.same(item);
    let like = feeds.like(item);
    let nearest = (like.iter().flat_map(|l| &l.nearest))
        .filter_map(|(id, cosine)| {
            library.get(id).map(|entry| Nearest {
                id: entry.id.clone(),
                key: entry.key.clone(),
                title: entry.title.clone(),
                subtitle: entry.subtitle.clone(),
                cosine: *cosine,
            })
        })
        .collect();
    let looked_up = item.sources.iter().any(|s| {
        s.entry.source == LOOKUP && s.entry.summary.is_some() && s.entry.summary == item.summary
    });
    Ok(FeedDetail {
        id: item.id.clone(),
        kind: item.kind,
        title: row.title,
        authors: item.authors.clone(),
        journal: item.journal.clone(),
        date: item.date,
        doi: item.doi.clone(),
        arxiv: item.arxiv.clone(),
        url: item.url.clone(),
        r#abstract: row.r#abstract,
        abstract_looked_up: looked_up,
        content: item.content.as_deref().map(paragraphs).unwrap_or_default(),
        editorial: item.editorial,
        sources: (source_ids(item).into_iter())
            .filter_map(subscription)
            .map(|s| SourceOf {
                id: s.id.clone(),
                title: title_of(s),
                kind: s.kind,
            })
            .collect(),
        versions: (item.versions.iter())
            .map(|v| VersionOf {
                doi: v.doi.clone(),
                stage: v.stage,
                item: item_of_doi(feeds, &v.doi, &item.id).map(|i| i.id.clone()),
            })
            .collect(),
        links: item.links.iter().map(|l| linked(feeds, l)).collect(),
        covered_by: (feeds.covered_by(&item.id).into_iter())
            .map(|post| Post {
                id: post.id.clone(),
                title: post.title.clone(),
                source: first_source(post),
            })
            .collect(),
        interests: row.interests,
        triage: feeds.triage(&item.id),
        version_in_library: index.versions(item, &in_library),
        in_library,
        like: like.map(|l| l.score),
        nearest,
    })
}

/// A subscription, with its last fetch.
fn info(subscription: Subscription, lasts: &BTreeMap<String, Last>) -> SubscriptionInfo {
    SubscriptionInfo {
        title: title_of(&subscription),
        last: lasts.get(&subscription.id).cloned(),
        feed: FeedOf::from(&subscription.feed),
        id: subscription.id,
        kind: subscription.kind,
        journal: subscription.journal,
        fetch_pages: subscription.fetch_pages,
        boost: subscription.boost,
    }
}

/// What the APIs are asked with: the settings' address and OpenAlex key, each if set.
fn access(settings: &Settings) -> Access {
    Access {
        mailto: settings.mailto().map(str::to_string),
        openalex_key: settings.openalex().api_key,
    }
}

/// The network, paced per host, and the access, from the settings.
fn network(state: &AppState) -> Result<(Paced<Network>, Access), String> {
    let access = access(&Settings::load(&state.settings_path)?);
    let network = Paced::new(Network::new()?, Pacing::polite(&access));
    Ok((network, access))
}

/// Fetches, telling the windows.
fn fetch_now(app: &AppHandle) -> Result<FetchSummary, String> {
    let state = app.state::<AppState>();
    let (network, access) = network(&state)?;
    let fetching = |on: bool| {
        let _ = app.emit(FETCHING, on);
    };
    let summary = state.feeds.fetch(&network, &access, fetching);
    let _ = app.emit(CHANGED, ());
    summary
}

/// A change, the windows told.
fn change(app: &AppHandle, change: Change) -> Result<Applied, String> {
    let applied = app.state::<AppState>().feeds.change(change)?;
    let _ = app.emit(CHANGED, ());
    Ok(applied)
}

/// The subscription a change gives.
fn subscription_of(app: &AppHandle, applied: Applied) -> Result<SubscriptionInfo, String> {
    match applied {
        Applied::Subscription(s) => Ok(info(s, &app.state::<AppState>().feeds.lasts()?)),
        _ => Err("no subscription".to_string()),
    }
}

/// Indexes the library's papers on a thread of its own, the library as it is
/// now, and gives the index to the feeds (none for an empty library); the
/// page is told. After every read of the library.
pub fn index_library(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let build = state.feeds.next_build();
        let likeness = {
            let library = state.library.read().unwrap();
            (!library.is_empty()).then(|| Arc::new(Likeness::of(&library)))
        };
        if state.feeds.set_likeness(build, likeness) {
            let _ = app.emit(CHANGED, ());
        }
    });
}

/// Fetch in the background, every so many minutes, while the settings say so.
pub fn watch(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST);
        loop {
            let state = app.state::<AppState>();
            if let Ok(settings) = Settings::load(&state.settings_path)
                && settings.feeds_poll
            {
                let minutes = settings.feeds_poll_minutes.max(LEAST_MINUTES);
                let every = TimeDelta::minutes(minutes.into());
                let last = state.feeds.last_fetch().ok().flatten();
                if last.is_none_or(|l| Utc::now() - l.at >= every)
                    && let Err(e) = fetch_now(&app)
                {
                    eprintln!("papers-desktop: cannot fetch the feeds: {e}");
                }
            }
            std::thread::sleep(TICK);
        }
    });
}

#[tauri::command]
pub async fn feeds_fetch(app: AppHandle) -> Result<FetchSummary, String> {
    blocking(move || fetch_now(&app)).await
}

#[tauri::command]
pub async fn feeds_last_fetch(app: AppHandle) -> Result<Option<FetchSummary>, String> {
    blocking(move || app.state::<AppState>().feeds.last_fetch()).await
}

#[tauri::command]
pub async fn feeds_view(app: AppHandle, request: ViewRequest) -> Result<FeedView, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let matching = *state.matching.read().unwrap();
        state.feeds.read(|feeds| {
            let library = state.library.read().unwrap();
            view(feeds, &library, matching, &request)
        })
    })
    .await
}

#[tauri::command]
pub async fn feeds_item(app: AppHandle, id: String, query: String) -> Result<FeedDetail, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let matching = *state.matching.read().unwrap();
        state.feeds.read(|feeds| {
            let library = state.library.read().unwrap();
            detail(feeds, &library, matching, &id, &query)
        })?
    })
    .await
}

#[tauri::command]
pub async fn feeds_triage(app: AppHandle, ids: Vec<String>, triage: Triage) -> Result<(), String> {
    blocking(move || change(&app, Change::Triage(ids, triage)).map(drop)).await
}

#[tauri::command]
pub async fn feeds_find_papers(app: AppHandle, id: String) -> Result<Vec<Linked>, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (network, access) = network(&state)?;
        let found = state.feeds.find_papers(&id, &network, &access);
        let _ = app.emit(CHANGED, ());
        found
    })
    .await
}

#[tauri::command]
pub async fn feeds_pdf(app: AppHandle, id: String) -> Result<Pdf, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (network, access) = network(&state)?;
        state.feeds.pdf(&id, &network, &access)
    })
    .await
}

#[tauri::command]
pub async fn feeds_subscriptions(app: AppHandle) -> Result<Vec<SubscriptionInfo>, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let lasts = state.feeds.lasts()?;
        let subscriptions = state.feeds.read(|feeds| feeds.subscriptions().to_vec())?;
        Ok(subscriptions.into_iter().map(|s| info(s, &lasts)).collect())
    })
    .await
}

/// Subscribes to a feed, with the title given (a blank one is none: the feed's own, at the next fetch).
#[tauri::command]
pub async fn feeds_subscribe(
    app: AppHandle,
    feed: FeedOf,
    kind: SourceKind,
    title: Option<String>,
) -> Result<SubscriptionInfo, String> {
    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    blocking(move || {
        let applied = change(&app, Change::Subscribe(feed.into(), kind, title))?;
        subscription_of(&app, applied)
    })
    .await
}

#[tauri::command]
pub async fn feeds_unsubscribe(app: AppHandle, id: String) -> Result<(), String> {
    blocking(move || change(&app, Change::Unsubscribe(id)).map(drop)).await
}

#[tauri::command]
pub async fn feeds_update_subscription(
    app: AppHandle,
    id: String,
    patch: SubscriptionPatch,
) -> Result<SubscriptionInfo, String> {
    blocking(move || {
        let applied = change(&app, Change::Update(id, patch))?;
        subscription_of(&app, applied)
    })
    .await
}

#[tauri::command]
pub async fn feeds_suggested() -> Vec<Suggested> {
    papers_feeds::suggested()
        .into_iter()
        .map(Suggested::from)
        .collect()
}

/// The journals of the library's papers, each with what follows it as far
/// as known, at once: those to look up are pending.
#[tauri::command]
pub async fn feeds_library_suggested(app: AppHandle) -> Result<Vec<LibraryJournal>, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let journals = library_journals(&state.library.read().unwrap());
        state.feeds.journals(|known| {
            let follow = |j: &papers_feeds::LibraryJournal| Follow::from(known.follow(j));
            journals
                .iter()
                .map(|j| LibraryJournal {
                    follow: follow(j),
                    title: j.title.clone(),
                    papers: j.papers,
                })
                .collect()
        })
    })
    .await
}

/// The journals OpenAlex finds for a text, the best first, 10 at most.
#[tauri::command]
pub async fn feeds_search_journals(app: AppHandle, text: String) -> Result<Vec<Journal>, String> {
    blocking(move || {
        let (network, access) = network(&app.state::<AppState>())?;
        papers_feeds::search_journals(&network, &access, text.trim())
    })
    .await
}

/// The feed a journal's site links to, read to tell it is the journal's:
/// its home page (OpenAlex's, else looked up by its ISSN) read with the page
/// user agent, its feeds as the subscription will be fetched.
#[tauri::command]
pub async fn feeds_journal_feed(app: AppHandle, journal: Journal) -> Result<JournalSite, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (network, access) = network(&state)?;
        let journal = state
            .feeds
            .journals(|journals| journals.with_homepage(journal, &network, &access))??;
        let page = Network::with_user_agent(&page_agent(access.mailto.as_deref()))?;
        let feed = journal
            .homepage
            .as_deref()
            .map(|homepage| papers_feeds::site_feed(&page, &network, homepage, &journal.title));
        Ok(JournalSite {
            homepage: journal.homepage,
            feed,
        })
    })
    .await
}

/// What follows a journal of the library, looked up in OpenAlex by its name
/// (once: what it answers is kept).
#[tauri::command]
pub async fn feeds_lookup_journal(app: AppHandle, title: String) -> Result<Follow, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (network, access) = network(&state)?;
        let follow = state
            .feeds
            .journals(|journals| journals.look_up(&title, &network, &access))??;
        Ok(follow.into())
    })
    .await
}

#[tauri::command]
pub async fn feeds_import_opml(app: AppHandle, path: PathBuf) -> Result<usize, String> {
    blocking(move || {
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        match change(&app, Change::Import(text))? {
            Applied::Count(added) => Ok(added),
            _ => Err("no count".to_string()),
        }
    })
    .await
}

#[tauri::command]
pub async fn feeds_export_opml(app: AppHandle, path: PathBuf) -> Result<(), String> {
    blocking(move || {
        let opml = app.state::<AppState>().feeds.read(Feeds::opml)?;
        std::fs::write(&path, opml).map_err(|e| format!("{}: {e}", path.display()))
    })
    .await
}

#[tauri::command]
pub async fn feeds_settings(app: AppHandle) -> Result<FeedSettings, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let settings = Settings::load(&state.settings_path)?;
        state.feeds.read(|feeds| FeedSettings {
            interests: feeds.interests().to_vec(),
            title_boost: feeds.title_boost(),
            rules: feeds.rules().to_vec(),
            keep_days: feeds.keep_days(),
            lookback_days: feeds.lookback_days(),
            poll: settings.feeds_poll,
            poll_minutes: settings.feeds_poll_minutes,
        })
    })
    .await
}

/// The saved searches of the sidebar, in their order.
#[tauri::command]
pub async fn feeds_searches(app: AppHandle) -> Result<Vec<Search>, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        state.feeds.read(|feeds| feeds.searches().to_vec())
    })
    .await
}

/// Sets the saved searches; refused, nothing changed, for a blank name or one
/// given twice.
#[tauri::command]
pub async fn feeds_set_searches(app: AppHandle, searches: Vec<Search>) -> Result<(), String> {
    blocking(move || change(&app, Change::Searches(searches)).map(drop)).await
}

/// Sets the settings of the feeds; refused, nothing changed, with the
/// message of the first interest that does not compile.
#[tauri::command]
pub async fn feeds_set_settings(app: AppHandle, settings: FeedSettings) -> Result<(), String> {
    blocking(move || {
        let configured = Configured {
            interests: settings.interests,
            title_boost: settings.title_boost,
            rules: settings.rules,
            keep_days: settings.keep_days,
            lookback_days: settings.lookback_days,
        };
        change(&app, Change::Settings(Box::new(configured)))?;
        let state = app.state::<AppState>();
        let mut kept = Settings::load(&state.settings_path)?;
        kept.feeds_poll = settings.poll;
        kept.feeds_poll_minutes = settings.poll_minutes.max(LEAST_MINUTES);
        kept.save(&state.settings_path)
    })
    .await
}

/// Whether the main window shows the Feeds tab (the setting `feeds_tab`).
#[tauri::command]
pub fn feeds_tab(app: AppHandle) -> Result<bool, String> {
    Ok(Settings::load(&app.state::<AppState>().settings_path)?.feeds_tab)
}

/// Shows or hides the Feeds tab of the main window: from the Feeds section of the settings and
/// Window → Show Feeds Tab. The windows are told at once ([`TAB_EVENT`]), and the menu follows.
#[tauri::command]
pub fn feeds_set_tab(app: AppHandle, show: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut settings = Settings::load(&state.settings_path)?;
    settings.feeds_tab = show;
    settings.save(&state.settings_path)?;
    #[cfg(target_os = "macos")]
    crate::menu::feeds_tab(&app, show);
    let _ = app.emit(TAB_EVENT, show);
    Ok(())
}

/// The Feeds tab shown or hidden, to every window: whether it is shown.
pub const TAB_EVENT: &str = "feeds-tab";

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use chrono::TimeZone;
    use papers_feeds::http::Recorded;
    use papers_feeds::{Reply, Request, Version};

    use super::*;

    const TC: &str = "https://example.org/tc.atom";
    const NEWS: &str = "https://example.org/news.atom";

    /// An Atom feed of entries: id, title, DOI.
    fn atom(title: &str, entries: &[(&str, &str, Option<&str>)]) -> Reply {
        let entries: String = entries
            .iter()
            .map(|(id, title, doi)| {
                let doi = doi.map(|d| format!("<arxiv:doi>{d}</arxiv:doi>")).unwrap_or_default();
                format!(
                    "<entry><id>{id}</id><title>{title}</title><link href=\"https://example.org/{id}\"/>\
                     <published>2026-10-30T00:00:00Z</published><author><name>Ann Lee</name></author>{doi}</entry>"
                )
            })
            .collect();
        let body = format!(
            "<?xml version='1.0'?><feed xmlns=\"http://www.w3.org/2005/Atom\" \
             xmlns:arxiv=\"http://arxiv.org/schemas/atom\"><title>{title}</title><id>feed</id>{entries}</feed>"
        );
        Reply::ok(TC, body)
    }

    /// The feeds of a folder on a fixed clock, kept in another.
    fn opened(dir: &Path) -> FeedsState {
        let now = Utc.with_ymd_and_hms(2026, 10, 31, 8, 0, 0).unwrap();
        let open: Opener = Box::new(move |dir| Feeds::open_with(dir, Box::new(move || now)));
        FeedsState::with_opener(dir.join("feeds"), dir.join("feeds-fetched.json"), open)
    }

    fn subscribe(state: &FeedsState, url: &str) -> String {
        let feed = Feed::Rss { url: url.into() };
        match state
            .change(Change::Subscribe(feed, SourceKind::Journal, None))
            .unwrap()
        {
            Applied::Subscription(s) => s.id,
            _ => unreachable!(),
        }
    }

    #[test]
    fn a_fetch_is_summed_up_and_each_subscription_s_last_kept() {
        let dir = tempfile::tempdir().unwrap();
        let state = opened(dir.path());
        let tc = subscribe(&state, TC);
        let news = subscribe(&state, NEWS);
        let feed = atom("The Cryosphere", &[("a", "Ice", None), ("b", "Firn", None)]);
        let http = Recorded::new()
            .reply(TC, feed.with_validators(Some("\"v1\""), None))
            .reply(NEWS, Reply::status(NEWS, 503))
            .reply(TC, Reply::status(TC, 304))
            .fail(NEWS, "offline");
        let told = Mutex::new(Vec::new());
        let summary = state
            .fetch(&http, &Access::default(), |on| {
                told.lock().unwrap().push(on)
            })
            .unwrap();
        assert_eq!(*told.lock().unwrap(), [true, false]);
        assert_eq!(
            (summary.fetched, summary.unchanged, summary.added),
            (1, 0, 2)
        );
        let failed = &summary.failed[0];
        assert_eq!(
            (failed.subscription.as_str(), failed.message.as_str()),
            (news.as_str(), "HTTP 503")
        );
        assert_eq!(failed.title, NEWS, "untitled: its address");
        let lasts = state.lasts().unwrap();
        assert_eq!(lasts[&tc].error, None);
        assert_eq!(lasts[&news].error.as_deref(), Some("HTTP 503"));

        let again = state.fetch(&http, &Access::default(), |_| {}).unwrap();
        assert_eq!((again.fetched, again.unchanged, again.added), (0, 1, 0));
        assert_eq!(again.failed[0].message, "offline");
        // kept across a start; a subscription gone is dropped at the next fetch
        let reopened = opened(dir.path());
        assert_eq!(reopened.last_fetch().unwrap(), Some(again));
        reopened.change(Change::Unsubscribe(news.clone())).unwrap();
        let http = Recorded::new().reply(TC, Reply::status(TC, 304));
        reopened.fetch(&http, &Access::default(), |_| {}).unwrap();
        assert!(!reopened.lasts().unwrap().contains_key(&news));
        assert_eq!(opened(dir.path()).lasts().unwrap().len(), 1);
    }

    /// The network of a test that changes the feeds while it is asked.
    struct Meanwhile<'a> {
        inner: Recorded,
        state: &'a FeedsState,
        change: Mutex<Option<Change>>,
    }

    impl papers_feeds::Http for Meanwhile<'_> {
        fn get(&self, request: &Request) -> Result<Reply, String> {
            if let Some(change) = self.change.lock().unwrap().take() {
                self.state.change(change).unwrap();
            }
            self.inner.get(request)
        }
    }

    #[test]
    fn a_change_made_during_a_fetch_stays() {
        let dir = tempfile::tempdir().unwrap();
        let state = opened(dir.path());
        subscribe(&state, TC);
        let http = Recorded::new().reply(TC, atom("TC", &[("a", "Ice", None)]));
        state.fetch(&http, &Access::default(), |_| {}).unwrap();
        let id = state.read(|f| f.items()[0].id.clone()).unwrap();
        let feed = atom("TC", &[("a", "Ice", None), ("b", "Firn", None)]);
        let http = Meanwhile {
            inner: Recorded::new().reply(TC, feed),
            state: &state,
            change: Mutex::new(Some(Change::Triage(vec![id.clone()], Triage::Shortlisted))),
        };
        let summary = state.fetch(&http, &Access::default(), |_| {}).unwrap();
        assert_eq!(summary.added, 1);
        assert_eq!(state.read(|f| f.triage(&id)).unwrap(), Triage::Shortlisted);
        assert_eq!(state.read(|f| f.items().len()).unwrap(), 2);
        let reopened = Feeds::open(&dir.path().join("feeds")).unwrap();
        assert_eq!(reopened.triage(&id), Triage::Shortlisted, "written");
    }

    #[test]
    fn saved_searches_set_during_a_fetch_stay_and_a_bad_one_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = opened(dir.path());
        subscribe(&state, TC);
        let search = |name: &str| Search {
            name: name.to_string(),
            query: "is:new".to_string(),
            sort: Sort::Score,
        };
        let http = Meanwhile {
            inner: Recorded::new().reply(TC, atom("TC", &[("a", "Ice", None)])),
            state: &state,
            change: Mutex::new(Some(Change::Searches(vec![search("New")]))),
        };
        state.fetch(&http, &Access::default(), |_| {}).unwrap();
        let names = |state: &FeedsState| {
            state.read(|f| {
                f.searches()
                    .iter()
                    .map(|s| s.name.clone())
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(names(&state).unwrap(), ["New"]);
        let twice = Change::Searches(vec![search("A"), search("a")]);
        assert!(state.change(twice).is_err());
        assert_eq!(
            names(&opened(dir.path())).unwrap(),
            ["New"],
            "written, unchanged"
        );
    }

    #[test]
    fn a_second_fetch_joins_the_running_one() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(opened(dir.path()));
        subscribe(&state, TC);
        let running = state.running.lock().unwrap();
        let joining = {
            let state = state.clone();
            std::thread::spawn(move || state.fetch(&Recorded::new(), &Access::default(), |_| {}))
        };
        std::thread::sleep(Duration::from_millis(50));
        drop(running);
        // no fetch ran, none was kept
        assert_eq!(joining.join().unwrap(), Err("the fetch failed".to_string()));
    }

    #[test]
    fn the_index_of_the_library_goes_to_the_feeds_and_their_copies() {
        let dir = tempfile::tempdir().unwrap();
        let state = opened(dir.path());
        subscribe(&state, TC);
        let configured = Configured {
            interests: Vec::new(),
            title_boost: 2.0,
            rules: vec![Rule {
                when: "like:30-".into(),
                then: papers_feeds::Action::Shortlist,
            }],
            keep_days: 90,
            lookback_days: 14,
        };
        state
            .change(Change::Settings(Box::new(configured)))
            .unwrap();
        let index = |title: &str| {
            let doc = papers_core::Doc {
                id: "lib".into(),
                title: title.into(),
                ..Default::default()
            };
            Some(Arc::new(Likeness::build([doc])))
        };
        let (old, new) = (state.next_build(), state.next_build());
        assert!(state.set_likeness(new, index("Firn densification")));
        assert!(
            !state.set_likeness(old, index("Ice")),
            "an older build is dropped"
        );
        let http = Recorded::new().reply(
            TC,
            atom(
                "TC",
                &[("a", "Ice", None), ("b", "Firn densification", None)],
            ),
        );
        state.fetch(&http, &Access::default(), |_| {}).unwrap();
        let triage = |title: &str| {
            state
                .read(|f| f.triage(&f.items().iter().find(|i| i.title == title).unwrap().id))
                .unwrap()
        };
        assert_eq!(
            triage("Firn densification"),
            Triage::Shortlisted,
            "the copy fetched had the index"
        );
        assert_eq!(triage("Ice"), Triage::New);
        let like = |title: &str| {
            state
                .read(|f| {
                    f.like(f.items().iter().find(|i| i.title == title).unwrap())
                        .map(|l| l.score)
                })
                .unwrap()
        };
        assert!(like("Firn densification").is_some_and(|l| l > 99.9));
        assert!(state.set_likeness(state.next_build(), None));
        assert_eq!(like("Firn densification"), None);
    }

    #[test]
    fn the_journals_of_the_library_and_what_follows_them() {
        let dir = tempfile::tempdir().unwrap();
        let bib = dir.path().join("refs.bib");
        let entries = [
            "@article{a, title = {A}, journal = {The {Cryosphere}}}",
            "@article{b, title = {B}, journal = {The Cryosphere}}",
            "@article{c, title = {C}, journaltitle = {the cryosphere}}",
            "@article{d, title = {D}, journal = {Polar Science}, issn = {1873-9652}}",
            "@article{e, title = {E}, journal = {Polar Science}}",
            "@article{f, title = {F}, journal = {Polar Science}}",
            "@inproceedings{g, title = {G}, booktitle = {Polar Science}}",
            "@article{h, title = {H}, journal = {Ice}}",
            "@article{i, title = {I}, journal = {Ice}}",
            "@article{j, title = {J}, journal = {Ice}}",
        ];
        std::fs::write(&bib, entries.join("\n")).unwrap();
        let library = Library::load(&[bib]).unwrap();
        let journals = library_journals(&library);
        let counted: Vec<(&str, usize, &[String])> = journals
            .iter()
            .map(|j| (j.title.as_str(), j.papers, j.issns.as_slice()))
            .collect();
        let polar = ["1873-9652".to_string()];
        assert_eq!(
            counted,
            [
                ("Ice", 3, &[][..]),
                ("Polar Science", 3, &polar[..]),
                ("The Cryosphere", 3, &[][..])
            ],
            "a book title is no journal"
        );

        let state = opened(dir.path());
        let follow = |j| {
            state
                .journals(|known| Follow::from(known.follow(j)))
                .unwrap()
        };
        let json = |j| serde_json::to_value(follow(j)).unwrap();
        assert_eq!(json(&journals[0]), serde_json::json!({"by": "pending"}));
        assert_eq!(json(&journals[1])["journal"]["issn"], "1873-9652");
        let tc = json(&journals[2]);
        assert_eq!(tc["by"], "suggested");
        assert_eq!(
            tc["suggested"]["feed"],
            serde_json::json!({"rss": "https://tc.copernicus.org/xml/rss2_0.xml"})
        );
    }

    #[test]
    fn a_subscription_given_a_title_and_a_site_as_the_page_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let state = opened(dir.path());
        let feed = Feed::Crossref {
            issn: "0022-1430".into(),
        };
        let title = Some("Journal of Glaciology".to_string());
        let Applied::Subscription(s) = state
            .change(Change::Subscribe(feed, SourceKind::Journal, title.clone()))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(s.title, title);
        assert_eq!(
            state.read(|f| f.subscriptions()[0].title.clone()).unwrap(),
            title
        );

        let site = |feed| {
            let site = JournalSite {
                homepage: Some("https://j.example.org/".into()),
                feed,
            };
            serde_json::to_value(site).unwrap()
        };
        let found = SiteFeed::Found {
            url: "https://j.example.org/rss".into(),
            title: None,
        };
        assert_eq!(
            site(Some(found)),
            serde_json::json!({"homepage": "https://j.example.org/",
                "feed": {"site": "found", "url": "https://j.example.org/rss", "title": null}})
        );
        let refused = SiteFeed::FeedRefused {
            url: "https://j.example.org/rss".into(),
            reason: "HTTP 403".into(),
        };
        assert_eq!(site(Some(refused))["feed"]["site"], "feedrefused");
        assert_eq!(
            site(Some(SiteFeed::NoFeed))["feed"],
            serde_json::json!({"site": "nofeed"})
        );
        assert_eq!(site(None)["feed"], serde_json::Value::Null);
    }

    #[test]
    fn items_in_the_library_by_doi_or_title_and_year() {
        let dir = tempfile::tempdir().unwrap();
        let bib = dir.path().join("refs.bib");
        std::fs::write(
            &bib,
            "@article{doi2026, title = {Anything}, year = {2026}, doi = {10.5194/TC-20-1-2026}}\n\
             @article{title2026, title = {Hysteresis of the {Greenland} ice sheet}, year = {2026}}\n\
             @article{title2019, title = {Hysteresis of the Greenland ice sheet}, year = {2019}}\n\
             @article{preprint, title = {Old title}, year = {2025}, doi = {10.5194/egusphere-2025-1}}\n",
        )
        .unwrap();
        let library = Library::load(&[bib]).unwrap();
        let index = InLibrary::of(&library);
        let id = |key: &str| {
            library
                .items()
                .iter()
                .find(|i| i.key == key)
                .unwrap()
                .id
                .clone()
        };
        let date = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).single();
        let item = |doi: Option<&str>, title: &str| Item {
            id: "x".into(),
            kind: ItemKind::Paper,
            title: title.into(),
            authors: Vec::new(),
            journal: None,
            date,
            doi: doi.map(String::from),
            arxiv: None,
            url: None,
            summary: None,
            content: None,
            editorial: false,
            sources: Vec::new(),
            versions: Vec::new(),
            links: Vec::new(),
        };
        let by_doi = item(Some("10.5194/tc-20-1-2026"), "Another title");
        assert_eq!(index.same(&by_doi), [id("doi2026")]);
        let by_title = item(None, "Hysteresis of the Greenland Ice-Sheet");
        assert_eq!(
            index.same(&by_title),
            [id("title2026")],
            "the same year only"
        );
        assert!(index.same(&item(None, "Unrelated")).is_empty());

        let mut paper = item(Some("10.5194/tc-20-1-2026"), "Anything");
        paper.versions = vec![
            Version {
                doi: "10.5194/tc-20-1-2026".into(),
                stage: Stage::Published,
            },
            Version {
                doi: "10.5194/egusphere-2025-1".into(),
                stage: Stage::Preprint,
            },
        ];
        let same = index.same(&paper);
        assert_eq!(same, [id("doi2026")], "its DOI and its title: one entry");
        assert_eq!(index.versions(&paper, &same), [id("preprint")]);
    }

    #[test]
    fn a_feed_as_the_page_writes_it() {
        let rss: FeedOf = serde_json::from_str(r#"{"rss": "https://eos.org/feed"}"#).unwrap();
        assert_eq!(
            Feed::from(rss),
            Feed::Rss {
                url: "https://eos.org/feed".into()
            }
        );
        let filter = FeedOf::from(&Feed::OpenAlex {
            filter: "cites:W1".into(),
        });
        assert_eq!(
            serde_json::to_string(&filter).unwrap(),
            r#"{"openalex":"cites:W1"}"#
        );
        let issn = serde_json::to_string(&FeedOf::Crossref("0022-1430".into())).unwrap();
        assert_eq!(issn, r#"{"crossref":"0022-1430"}"#);
        // a journal left out stays, null clears it
        let patch: SubscriptionPatch = serde_json::from_str(r#"{"title": "Eos"}"#).unwrap();
        assert_eq!(patch.journal, None);
        let patch: SubscriptionPatch = serde_json::from_str(r#"{"journal": null}"#).unwrap();
        assert_eq!(patch.journal, Some(None));
        let patch: SubscriptionPatch = serde_json::from_str(r#"{"fetchPages": true}"#).unwrap();
        assert_eq!(patch.fetch_pages, Some(true));
        // applied: what it has, a blank title or journal as none
        let mut eos = Subscription {
            id: "e".into(),
            feed: Feed::Rss {
                url: "https://eos.org/feed".into(),
            },
            title: Some("Eos".into()),
            kind: SourceKind::Journal,
            journal: Some("Eos".into()),
            fetch_pages: false,
            boost: 0.0,
        };
        let patch: SubscriptionPatch =
            serde_json::from_str(r#"{"title": " ", "kind": "news", "journal": null}"#).unwrap();
        patch.apply(&mut eos);
        assert_eq!(
            (eos.title.clone(), eos.kind, eos.journal.clone()),
            (None, SourceKind::News, None)
        );
        assert!(!eos.fetch_pages);
        assert_eq!(eos.boost, 0.0, "left out stays");
        let patch: SubscriptionPatch = serde_json::from_str(r#"{"boost": 2.5}"#).unwrap();
        patch.apply(&mut eos);
        assert_eq!(eos.boost, 2.5);
    }
}
