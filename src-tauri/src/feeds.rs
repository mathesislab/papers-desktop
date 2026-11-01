//! The Feeds tab and the Feeds section of the settings: the commands of
//! `src/lib/feeds/api.ts` over the feeds of `app_data_dir/feeds`
//! (crates/feeds), opened at the first command and kept.
//!
//! A fetch takes a while: it runs on a copy of the feeds opened from the
//! folder, so the tab reads and triages the feeds as they were meanwhile.
//! Each change made while it runs is applied at once and kept in a journal,
//! then applied again to the fetched copy, which takes the place of the
//! feeds. It goes in two phases, each on a copy of its own: the feeds read,
//! whose summary the command answers, then what follows (the lookups of
//! Crossref and OpenAlex) on a thread of its own, the page told of each
//! (`feeds-fetching`). One fetch at a time: a fetch of every subscription
//! asked while another reads the feeds or looks up waits for its feeds read,
//! and answers its summary (the tab opening while the background polling
//! fetches does not fetch twice); any other waits for the whole fetch. Finding
//! the papers of a post works on a copy too, after a running fetch.
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
//! *Like in your library*, *in library*: the facts of the library (the index
//! of its papers, core's `Likeness`, and their identities) are gathered on a
//! thread of their own whenever the library is read (at the start, after a
//! change of its files or a write), then given to the feeds and to every
//! copy a fetch opens; the page is told (`feeds-changed`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use papers_core::{Library, Likeness, Matching, Span};
use papers_feeds::http::{Paced, Pacing};
use papers_feeds::{
    Access, Confidence, Feed, FeedReport, Feeds, Groups, Interest, Item, ItemKind, Journal,
    Journals, LibraryFacts, Network, Outcome, Pdf, Reminder, Rule, Score, Search, SiteFeed, Sort,
    SourceKind, Stage, Subscription, SubscriptionSettings, Triage, Via,
    ViewRequest as CrateRequest, safe_html,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::fetch::{blocking, page_agent};
use crate::settings::Settings;

/// The events of the page: the items changed; where a fetch is (`Phase`).
const CHANGED: &str = "feeds-changed";
const FETCHING: &str = "feeds-fetching";

/// The folder of feeds an item is of, as the page names it: the Feeds tab's,
/// or the works found in the Search tab (`crate::search`), whose items the
/// same commands show and triage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Store {
    Feeds,
    Search,
}

impl Store {
    pub fn of(self, state: &AppState) -> &Folder {
        match self {
            Store::Feeds => &state.feeds.folder,
            Store::Search => &state.search,
        }
    }
}

/// Where a fetch is: reading the feeds, looking up what they brought, done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Reading,
    LookingUp,
    Done,
}

/// The background polling: its first look a while after the start, then
/// each minute, fetching when the last fetch is older than the setting.
const FIRST: Duration = Duration::from_secs(120);
const TICK: Duration = Duration::from_secs(60);

/// The fewest minutes between two fetches of the polling.
const LEAST_MINUTES: u32 = 5;

/// Opens the feeds of a folder: on the clock, or a test's.
type Opener = Box<dyn Fn(&Path) -> papers_feeds::Result<Feeds> + Send + Sync>;

/// A change of the feeds, kept to make again on a copy that was fetched
/// (what no longer applies, an item gone, a feed subscribed twice, passed
/// over).
type Journalled = Arc<dyn Fn(&mut Feeds) + Send + Sync>;

/// A folder of feeds, opened at its first command and kept: the feeds the
/// commands see, and a copy worked on meanwhile (a fetch, the papers of a
/// post, an abstract looked up), the changes made while it runs made again
/// on it before it takes their place.
pub struct Folder {
    dir: PathBuf,
    live: Mutex<Live>,
    /// held by the work on a copy that runs
    running: Arc<Gate>,
    /// the free copy of each DOI asked for, this session
    pdfs: Mutex<HashMap<String, Pdf>>,
    /// counts the builds of the facts of the library: one that ends after a
    /// newer one is dropped
    builds: AtomicU64,
    open: Opener,
    /// the event that its items changed
    changed: &'static str,
}

/// A lock that a thread other than the one that took it can hold and let go
/// of: the work on a copy, whose fetch reads the feeds on one thread and
/// looks up what they brought on another.
#[derive(Default)]
struct Gate {
    held: Mutex<bool>,
    let_go: Condvar,
}

/// A gate held, until dropped.
pub struct Held(Arc<Gate>);

impl Gate {
    /// The gate held, once let go of by the one who holds it.
    fn hold(self: &Arc<Gate>) -> Held {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        while *held {
            held = (self.let_go.wait(held)).unwrap_or_else(PoisonError::into_inner);
        }
        *held = true;
        Held(self.clone())
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        *self.0.held.lock().unwrap_or_else(PoisonError::into_inner) = false;
        self.0.let_go.notify_one();
    }
}

/// The feeds the commands see, the changes made while a copy is worked on,
/// and the facts of the library that the feeds and their copies are given.
#[derive(Default)]
struct Live {
    feeds: Option<Feeds>,
    journal: Option<Vec<Journalled>>,
    library: Option<Arc<LibraryFacts>>,
    /// the build of `library`
    build: u64,
}

/// The feeds of the Feeds tab, their fetches, and the journals looked up.
pub struct FeedsState {
    pub folder: Folder,
    fetching: Fetching,
    /// the journals looked up in OpenAlex, opened at the first ask
    journals: Mutex<Option<Journals>>,
}

/// The fetches of the feeds, one at a time, and what is kept of them.
struct Fetching {
    /// `feeds-fetched.json`: the last summary, each subscription's last fetch
    path: PathBuf,
    /// held by a fetch while it reads the feeds: a fetch asked meanwhile waits
    reading: Mutex<()>,
    /// the summary of the feeds read of a fetch of every subscription, while
    /// it looks up what they brought: a fetch of every subscription asked
    /// meanwhile answers it
    in_flight: Mutex<Option<FetchSummary>>,
    /// read from its file at the first ask
    fetched: Mutex<Option<Fetched>>,
}

/// A fetch begun: joining the one that looks up, or to read the feeds.
pub enum Begun<'a> {
    /// the summary of the feeds read of the fetch of every subscription that
    /// looks up what they brought
    Joined(FetchSummary),
    Reading(Reading<'a>),
}

/// A fetch to read the feeds, the work on a copy held: of every
/// subscription, or of one.
pub struct Reading<'a> {
    state: &'a FeedsState,
    only: Option<String>,
    /// a fetch of every subscription asked meanwhile waits
    _reading: MutexGuard<'a, ()>,
    running: Held,
}

/// What follows the feeds read of a fetch, to look up, the work on a copy
/// held till it is done: on a thread of its own.
pub struct Following {
    running: Held,
    /// the items before the fetch
    known: BTreeSet<String>,
    subscriptions: Vec<Subscription>,
    /// of the feeds read
    summary: FetchSummary,
    /// of every subscription
    whole: bool,
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

/// The number of items not `known` before a fetch.
fn new_items(feeds: &Feeds, known: &BTreeSet<String>) -> usize {
    feeds
        .items()
        .iter()
        .filter(|i| !known.contains(&i.id))
        .count()
}

/// A subscription's title: its own, else what it follows.
fn title_of(subscription: &Subscription) -> String {
    (subscription.settings.title.clone()).unwrap_or_else(|| subscription.feed.to_string())
}

/// The summary of the feeds read (nothing looked up yet), and each
/// subscription's last fetch.
fn summary(
    reports: &[FeedReport],
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
        lookups: 0,
        pages: 0,
    };
    let mut last = Vec::new();
    for feed in reports {
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

impl Folder {
    /// The feeds of a folder, not opened yet; `changed` the event that its
    /// items changed.
    pub fn new(dir: PathBuf, changed: &'static str) -> Folder {
        Folder::with_opener(dir, changed, Box::new(Feeds::open))
    }

    fn with_opener(dir: PathBuf, changed: &'static str, open: Opener) -> Folder {
        Folder {
            dir,
            live: Mutex::new(Live::default()),
            running: Arc::new(Gate::default()),
            pdfs: Mutex::new(HashMap::new()),
            builds: AtomicU64::new(0),
            open,
            changed,
        }
    }

    /// The feeds of the folder, opened, with the facts of the library.
    fn opened(&self, live: &Live) -> Result<Feeds, String> {
        let mut feeds = (self.open)(&self.dir).map_err(|e| e.to_string())?;
        if let Some(library) = &live.library {
            feeds.set_library(library.clone());
        }
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

    /// The windows told that its items changed.
    pub fn tell(&self, app: &AppHandle) {
        let _ = app.emit(self.changed, ());
    }

    /// The number of a new build of the facts of the library.
    pub fn next_build(&self) -> u64 {
        self.builds.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// The facts of the library of build `build`, for the feeds and the
    /// copies opened from now on, unless a newer build was given already:
    /// whether they were taken.
    pub fn set_library(&self, build: u64, library: Arc<LibraryFacts>) -> bool {
        let mut live = self.live.lock().unwrap();
        if build < live.build {
            return false;
        }
        live.build = build;
        live.library = Some(library.clone());
        if let Some(feeds) = &mut live.feeds {
            feeds.set_library(library);
        }
        true
    }

    /// Something read of the feeds.
    pub fn read<T>(&self, read: impl FnOnce(&Feeds) -> T) -> Result<T, String> {
        let live = self.live()?;
        Ok(read(live.feeds.as_ref().expect("opened")))
    }

    /// A change of the feeds, now, and again on the copy a fetch works on;
    /// the windows told.
    pub fn change<T: Send + 'static>(
        &self,
        app: &AppHandle,
        f: impl Fn(&mut Feeds) -> papers_feeds::Result<T> + Send + Sync + 'static,
    ) -> Result<T, String> {
        let done = self.apply(f)?;
        self.tell(app);
        Ok(done)
    }

    /// [`Folder::change`], the windows not told.
    fn apply<T: Send + 'static>(
        &self,
        f: impl Fn(&mut Feeds) -> papers_feeds::Result<T> + Send + Sync + 'static,
    ) -> Result<T, String> {
        let mut live = self.live()?;
        let done = f(live.feeds.as_mut().expect("opened")).map_err(|e| e.to_string())?;
        if let Some(journal) = &mut live.journal {
            journal.push(Arc::new(move |feeds: &mut Feeds| drop(f(feeds))));
        }
        Ok(done)
    }

    /// Work on a copy of the feeds, which then takes their place with the
    /// changes made meanwhile; the running gate is held.
    fn on_copy<T>(
        &self,
        _running: &Held,
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
            change(&mut copy);
        }
        // the facts gathered meanwhile
        if let Some(library) = &live.library {
            copy.set_library(library.clone());
        }
        live.feeds = Some(copy);
        Ok(done)
    }

    /// The papers behind a post, looked for again, after a running fetch.
    pub fn find_papers(
        &self,
        id: &str,
        http: &dyn papers_feeds::Http,
        access: &Access,
    ) -> Result<Vec<Linked>, String> {
        let running = self.running.hold();
        let found = self.on_copy(&running, |feeds| feeds.find_papers(id, http, access))?;
        self.read(|feeds| found.links.iter().map(|l| linked(feeds, l)).collect())
    }

    /// The abstract of an item looked up now, after a running fetch; whether
    /// its lookups asked.
    pub fn look_up(
        &self,
        id: &str,
        http: &dyn papers_feeds::Http,
        access: &Access,
    ) -> Result<bool, String> {
        let running = self.running.hold();
        self.on_copy(&running, |feeds| feeds.look_up_abstract(id, http, access))
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

impl FeedsState {
    /// The feeds of a folder, not opened yet; what is kept of their fetches in a file.
    pub fn new(dir: PathBuf, fetched_path: PathBuf) -> FeedsState {
        FeedsState::with_folder(Folder::new(dir, CHANGED), fetched_path)
    }

    fn with_folder(folder: Folder, fetched_path: PathBuf) -> FeedsState {
        FeedsState {
            folder,
            fetching: Fetching {
                path: fetched_path,
                reading: Mutex::new(()),
                in_flight: Mutex::new(None),
                fetched: Mutex::new(None),
            },
            journals: Mutex::new(None),
        }
    }

    /// Something done with the journals looked up, opened if they are not yet.
    pub fn journals<T>(&self, work: impl FnOnce(&mut Journals) -> T) -> Result<T, String> {
        let mut journals = self.journals.lock().unwrap();
        if journals.is_none() {
            *journals = Some(Journals::open(&self.folder.dir).map_err(|e| e.to_string())?);
        }
        Ok(work(journals.as_mut().expect("opened")))
    }

    /// A fetch of every subscription, or of the one of id `only`, begun:
    /// one of every subscription asked while another one looks up joins it;
    /// else the feeds are to be read, once the work on a copy that runs is
    /// done.
    pub fn begin(&self, only: Option<&str>) -> Begun<'_> {
        let fetching = &self.fetching;
        let reading = (fetching.reading.lock()).unwrap_or_else(PoisonError::into_inner);
        let in_flight = fetching.in_flight.lock().unwrap().clone();
        if let Some(summary) = in_flight.filter(|_| only.is_none()) {
            return Begun::Joined(summary);
        }
        Begun::Reading(Reading {
            state: self,
            only: only.map(String::from),
            _reading: reading,
            running: self.folder.running.hold(),
        })
    }

    /// The summary of the last fetch.
    pub fn last_fetch(&self) -> Result<Option<FetchSummary>, String> {
        Ok(self
            .fetching
            .fetched()?
            .as_ref()
            .and_then(|f| f.summary.clone()))
    }

    /// The last fetch of each subscription.
    fn lasts(&self) -> Result<BTreeMap<String, Last>, String> {
        let fetched = self.fetching.fetched()?;
        Ok(fetched.as_ref().map(|f| f.last.clone()).unwrap_or_default())
    }

    /// A subscription as the settings list it, with its last fetch.
    pub fn info(&self, subscription: Subscription) -> Result<SubscriptionInfo, String> {
        Ok(info(subscription, &self.lasts()?))
    }
}

impl Reading<'_> {
    /// The feeds read, on a copy: their summary (kept, unless of a single
    /// subscription), and what follows, to look up.
    pub fn read(
        self,
        http: &dyn papers_feeds::Http,
        access: &Access,
    ) -> Result<(FetchSummary, Following), String> {
        let (folder, fetching) = (&self.state.folder, &self.state.fetching);
        let only = self.only.as_deref();
        let (report, known) = folder.on_copy(&self.running, |feeds| {
            let known = feeds.ids();
            Ok((feeds.read_feeds(only, http, access)?, known))
        })?;
        let (subscriptions, added) =
            folder.read(|feeds| (feeds.subscriptions().to_vec(), new_items(feeds, &known)))?;
        let (summary, last) = summary(&report, &subscriptions, added, Utc::now());
        let whole = only.is_none();
        fetching.keep(whole.then_some(&summary), last, &subscriptions)?;
        if whole {
            *fetching.in_flight.lock().unwrap() = Some(summary.clone());
        }
        let following = Following {
            running: self.running,
            known,
            subscriptions,
            summary: summary.clone(),
            whole,
        };
        Ok((summary, following))
    }
}

impl Following {
    /// What follows the feeds read, looked up on a copy: the whole summary,
    /// kept unless of a single subscription.
    pub fn follow(
        self,
        state: &FeedsState,
        http: &dyn papers_feeds::Http,
        access: &Access,
    ) -> Result<FetchSummary, String> {
        let known = &self.known;
        let followed = state.folder.on_copy(&self.running, |feeds| {
            let followed = feeds.follow_up(known, http, access)?;
            Ok((followed, new_items(feeds, known)))
        });
        *state.fetching.in_flight.lock().unwrap() = None;
        let (followed, added) = followed?;
        let summary = FetchSummary {
            lookups: followed.lookups,
            pages: followed.pages,
            added,
            ..self.summary
        };
        if self.whole {
            (state.fetching).keep(Some(&summary), Vec::new(), &self.subscriptions)?;
        }
        Ok(summary)
    }
}

impl Fetching {
    /// Keeps a fetch's summary, unless of a single subscription, and each
    /// subscription's last fetch, those of subscriptions gone dropped.
    fn keep(
        &self,
        summary: Option<&FetchSummary>,
        last: Vec<(String, Last)>,
        subscriptions: &[Subscription],
    ) -> Result<(), String> {
        let mut fetched = self.fetched()?;
        let kept = fetched.as_mut().expect("read");
        if let Some(summary) = summary {
            kept.summary = Some(summary.clone());
        }
        kept.last.extend(last);
        kept.last
            .retain(|id, _| subscriptions.iter().any(|s| &s.id == id));
        let json = serde_json::to_string_pretty(kept).unwrap();
        let path = &self.path;
        let error = |e: std::io::Error| format!("{}: {e}", path.display());
        std::fs::create_dir_all(path.parent().unwrap()).map_err(error)?;
        std::fs::write(path, json + "\n").map_err(error)
    }

    /// What is kept of the fetches, read from its file the first time.
    fn fetched(&self) -> Result<MutexGuard<'_, Option<Fetched>>, String> {
        let mut fetched = self.fetched.lock().unwrap();
        if fetched.is_none() {
            let kept = match std::fs::read_to_string(&self.path) {
                // a file that does not parse is started again
                Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Fetched::default(),
                Err(e) => return Err(format!("{}: {e}", self.path.display())),
            };
            *fetched = Some(kept);
        }
        Ok(fetched)
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
    /// when the list was last brought up to date: what was fetched since is held back
    #[serde(default)]
    shown: Option<DateTime<Utc>>,
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
    /// snoozed, or due (first in the list)
    reminder: Option<Reminder>,
    /// when it is to be reminded of
    remind: Option<DateTime<Utc>>,
    in_library: Vec<String>,
    has_versions: bool,
    covered_by: usize,
    links: usize,
    /// the works that cite it, as OpenAlex counted them when it was found
    cited: Option<u64>,
}

/// The counts of the sidebar, the subscriptions by title, the saved searches by name.
#[derive(Debug, Serialize)]
pub struct FeedCounts {
    all: usize,
    is: BTreeMap<String, usize>,
    total: FeedGroups,
    /// those new
    unread: FeedGroups,
    searches: BTreeMap<String, usize>,
}

/// Items by kind, interest and subscription (by title).
#[derive(Debug, Serialize)]
pub struct FeedGroups {
    kinds: BTreeMap<ItemKind, usize>,
    interests: BTreeMap<String, usize>,
    sources: BTreeMap<String, usize>,
}

impl FeedGroups {
    /// The crate's groups, the subscriptions by title.
    fn of(groups: Groups, titles: &HashMap<&str, String>) -> FeedGroups {
        let mut sources: BTreeMap<String, usize> = BTreeMap::new();
        for (id, count) in groups.sources {
            if let Some(title) = titles.get(id.as_str()) {
                *sources.entry(title.clone()).or_default() += count;
            }
        }
        FeedGroups {
            kinds: groups.kinds,
            interests: groups.interests,
            sources,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FeedView {
    rows: Vec<FeedRow>,
    counts: FeedCounts,
    /// the items fetched since the list was shown, held back
    held: usize,
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
    /// the abstract is the text of a feed (a teaser, a summary), not Crossref's or OpenAlex's
    abstract_from_feed: bool,
    /// its lookups may still find its abstract: the reader asks (`feeds_look_up`)
    abstract_pending: bool,
    /// a post's HTML, safe to show (`safe_html`): its text, links and images
    content: Option<String>,
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
    /// the works that cite it, as OpenAlex counted them when it was found
    cited: Option<u64>,
    /// its OpenAlex id (`W…`), if an entry is OpenAlex's: the works around it are searched by it,
    /// else by its DOI
    openalex: Option<String>,
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
    /// Sets what it has (a blank title or journal is none once edited: the
    /// feed's own again).
    fn apply(&self, settings: &mut SubscriptionSettings) {
        if let Some(title) = &self.title {
            settings.title = Some(title.clone());
        }
        if let Some(kind) = self.kind {
            settings.kind = kind;
        }
        if let Some(journal) = &self.journal {
            settings.journal = journal.clone();
        }
        if let Some(fetch_pages) = self.fetch_pages {
            settings.fetch_pages = fetch_pages;
        }
        if let Some(boost) = self.boost {
            settings.boost = boost;
        }
    }
}

/// A subscription's settings changed by a patch.
fn update(
    feeds: &mut Feeds,
    id: &str,
    patch: &SubscriptionPatch,
) -> papers_feeds::Result<Subscription> {
    let mut settings = feeds.subscription(id)?.settings.clone();
    patch.apply(&mut settings);
    feeds.edit_subscription(id, settings)
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
pub fn view(feeds: &Feeds, matching: Matching, request: &ViewRequest) -> FeedView {
    let view = feeds.view(&CrateRequest {
        query: &request.query,
        matching,
        sort: request.sort,
        shown: request.shown,
        ..CrateRequest::default()
    });
    let rows = Rows::new(feeds);
    FeedView {
        held: view.held,
        counts: FeedCounts {
            all: view.counts.all,
            is: view.counts.is,
            total: FeedGroups::of(view.counts.total, &rows.titles),
            unread: FeedGroups::of(view.counts.unread, &rows.titles),
            searches: view.counts.searches,
        },
        rows: view.rows.into_iter().map(|row| rows.row(row)).collect(),
    }
}

/// What the rows of the list show besides the crate's rows: the titles of the
/// subscriptions, the posts that cover a paper.
pub struct Rows<'a> {
    feeds: &'a Feeds,
    titles: HashMap<&'a str, String>,
    /// the posts that link each DOI
    posts: HashMap<&'a str, HashSet<&'a str>>,
}

impl<'a> Rows<'a> {
    pub fn new(feeds: &'a Feeds) -> Rows<'a> {
        let titles = (feeds.subscriptions().iter())
            .map(|s| (s.id.as_str(), title_of(s)))
            .collect();
        let mut posts: HashMap<&str, HashSet<&str>> = HashMap::new();
        for post in feeds.items().iter().filter(|i| !i.links.is_empty()) {
            for link in &post.links {
                posts.entry(&link.doi).or_default().insert(&post.id);
            }
        }
        Rows {
            feeds,
            titles,
            posts,
        }
    }

    /// The row of the list of a crate's row.
    pub fn row(&self, row: papers_feeds::Row) -> FeedRow {
        let feeds = self.feeds;
        let item = feeds.item(&row.id).expect("a row's item");
        let dois = item.doi.iter().chain(item.versions.iter().map(|v| &v.doi));
        let covering: HashSet<&str> = dois
            .filter_map(|d| self.posts.get(d.as_str()))
            .flatten()
            .filter(|&&id| id != item.id)
            .copied()
            .collect();
        FeedRow {
            kind: item.kind,
            journal: item.journal.clone(),
            sources: (feeds.subscriptions_of(item).into_iter())
                .map(title_of)
                .collect(),
            date: item.date,
            triage: feeds.triage(&item.id),
            reminder: row.reminder,
            remind: feeds.mark(&item.id).and_then(|m| m.remind),
            in_library: row.in_library,
            has_versions: item.kind == ItemKind::Preprint
                || item.versions.iter().any(|v| v.stage == Stage::Preprint),
            covered_by: covering.len(),
            links: item.links.len(),
            cited: item.cited,
            id: row.id,
            title: row.title,
            authors: row.authors,
            score: row.score,
            interests: row.interests,
            like: row.like,
        }
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
    let first_source = |item: &Item| {
        let first = feeds.subscriptions_of(item).into_iter().next();
        first.map(title_of).unwrap_or_default()
    };
    let in_library = row.in_library;
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
        cited: item.cited,
        openalex: (item.sources.iter())
            .find_map(|s| s.entry.guid.strip_prefix("https://openalex.org/"))
            .filter(|id| id.starts_with('W'))
            .map(String::from),
        r#abstract: row.r#abstract,
        abstract_from_feed: item.summary.is_some() && !feeds.abstract_from_work(item),
        abstract_pending: feeds.pending(item).is_some(),
        content: (item.content.as_deref()).map(|html| safe_html(html, item.url.as_deref())),
        editorial: item.editorial,
        sources: (feeds.subscriptions_of(item).into_iter())
            .map(|s| SourceOf {
                id: s.id.clone(),
                title: title_of(s),
                kind: s.settings.kind,
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
        version_in_library: feeds.versions_in_library(item, &in_library),
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
        kind: subscription.settings.kind,
        journal: subscription.settings.journal,
        fetch_pages: subscription.settings.fetch_pages,
        boost: subscription.settings.boost,
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
pub fn network(state: &AppState) -> Result<(Paced<Network>, Access), String> {
    let access = access(&Settings::load(&state.settings_path)?);
    let network = Paced::new(Network::new()?, Pacing::polite(&access));
    Ok((network, access))
}

/// Fetches every subscription, or the one of id `only`: the summary once
/// the feeds are read, what follows going on, on a thread of its own; the
/// windows told where it is.
fn fetch_now(app: &AppHandle, only: Option<String>) -> Result<FetchSummary, String> {
    let state = app.state::<AppState>();
    let reading = match state.feeds.begin(only.as_deref()) {
        Begun::Joined(summary) => return Ok(summary),
        Begun::Reading(reading) => reading,
    };
    let (network, access) = network(&state)?;
    tell(app, Phase::Reading);
    let (summary, following) = match reading.read(&network, &access) {
        Ok(read) => read,
        Err(e) => {
            tell(app, Phase::Done);
            return Err(e);
        }
    };
    tell(app, Phase::LookingUp);
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let followed = following.follow(&state.feeds, &network, &access);
        tell(&app, Phase::Done);
        if let Err(e) = followed {
            eprintln!("papers-desktop: cannot look up what the feeds brought: {e}");
        }
    });
    Ok(summary)
}

/// The windows told where a fetch is, and that the items changed once it
/// reads no more.
fn tell(app: &AppHandle, phase: Phase) {
    let _ = app.emit(FETCHING, phase);
    if phase != Phase::Reading {
        app.state::<AppState>().feeds.folder.tell(app);
    }
}

/// Gathers the facts of the library on a thread of its own, the library as
/// it is now (the index of its papers, none for an empty library; their
/// identities), and gives them to the feeds and to the works found; the page
/// is told. After every read of the library.
pub fn index_library(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let builds = [Store::Feeds, Store::Search].map(|s| (s, s.of(&state).next_build()));
        let facts = {
            let library = state.library.read().unwrap();
            Arc::new(LibraryFacts {
                likeness: (!library.is_empty()).then(|| Likeness::of(&library)),
                identities: library.identities(),
            })
        };
        for (store, build) in builds {
            let folder = store.of(&state);
            if folder.set_library(build, facts.clone()) {
                folder.tell(&app);
            }
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
                if last.is_none_or(|l| Utc::now() - l.at >= every) {
                    // a failure is said by the thread of the fetch
                    let _ = fetch_now(&app, None);
                }
            }
            std::thread::sleep(TICK);
        }
    });
}

#[tauri::command]
pub async fn feeds_fetch(app: AppHandle, only: Option<String>) -> Result<FetchSummary, String> {
    blocking(move || fetch_now(&app, only)).await
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
        (state.feeds.folder).read(|feeds| view(feeds, matching, &request))
    })
    .await
}

#[tauri::command]
pub async fn feeds_item(
    app: AppHandle,
    store: Store,
    id: String,
    query: String,
) -> Result<FeedDetail, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let matching = *state.matching.read().unwrap();
        store.of(&state).read(|feeds| {
            let library = state.library.read().unwrap();
            detail(feeds, &library, matching, &id, &query)
        })?
    })
    .await
}

#[tauri::command]
pub async fn feeds_triage(
    app: AppHandle,
    store: Store,
    ids: Vec<String>,
    triage: Triage,
) -> Result<(), String> {
    blocking(move || {
        let state = app.state::<AppState>();
        store.of(&state).change(&app, move |feeds| {
            (ids.iter()).try_for_each(|id| feeds.set_triage(id, triage))
        })
    })
    .await
}

/// Items new, to be reminded of `until` (snoozed till then), else now (kept unread).
#[tauri::command]
pub async fn feeds_remind(
    app: AppHandle,
    ids: Vec<String>,
    until: Option<DateTime<Utc>>,
) -> Result<(), String> {
    let when = until.unwrap_or_else(Utc::now);
    blocking(move || {
        let state = app.state::<AppState>();
        (state.feeds.folder).change(&app, move |feeds| {
            ids.iter().try_for_each(|id| feeds.remind(id, when))
        })
    })
    .await
}

#[tauri::command]
pub async fn feeds_find_papers(app: AppHandle, id: String) -> Result<Vec<Linked>, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (network, access) = network(&state)?;
        let found = state.feeds.folder.find_papers(&id, &network, &access);
        state.feeds.folder.tell(&app);
        found
    })
    .await
}

#[tauri::command]
pub async fn feeds_look_up(app: AppHandle, store: Store, id: String) -> Result<bool, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (network, access) = network(&state)?;
        let folder = store.of(&state);
        let asked = folder.look_up(&id, &network, &access)?;
        if asked {
            folder.tell(&app);
        }
        Ok(asked)
    })
    .await
}

#[tauri::command]
pub async fn feeds_pdf(app: AppHandle, store: Store, id: String) -> Result<Pdf, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (network, access) = network(&state)?;
        store.of(&state).pdf(&id, &network, &access)
    })
    .await
}

#[tauri::command]
pub async fn feeds_subscriptions(app: AppHandle) -> Result<Vec<SubscriptionInfo>, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let lasts = state.feeds.lasts()?;
        let subscriptions = (state.feeds.folder).read(|feeds| feeds.subscriptions().to_vec())?;
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
    let settings = SubscriptionSettings {
        title,
        ..SubscriptionSettings::new(kind)
    };
    let feed = Feed::from(feed);
    blocking(move || {
        let state = app.state::<AppState>();
        let subscription = (state.feeds.folder).change(&app, move |feeds| {
            feeds.subscribe(feed.clone(), settings.clone())
        })?;
        state.feeds.info(subscription)
    })
    .await
}

#[tauri::command]
pub async fn feeds_unsubscribe(app: AppHandle, id: String) -> Result<(), String> {
    blocking(move || {
        let state = app.state::<AppState>();
        (state.feeds.folder).change(&app, move |feeds| feeds.unsubscribe(&id).map(drop))
    })
    .await
}

#[tauri::command]
pub async fn feeds_update_subscription(
    app: AppHandle,
    id: String,
    patch: SubscriptionPatch,
) -> Result<SubscriptionInfo, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let subscription =
            (state.feeds.folder).change(&app, move |feeds| update(feeds, &id, &patch))?;
        state.feeds.info(subscription)
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
/// The kind of the feed at an address, guessed from its items and its title (the Subscribe sheet).
#[tauri::command]
pub async fn feeds_guess_kind(app: AppHandle, url: String) -> Result<papers_feeds::Guess, String> {
    blocking(move || {
        let (network, access) = network(&app.state::<AppState>())?;
        papers_feeds::guess_kind(&network, &access, url.trim())
    })
    .await
}

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
        let state = app.state::<AppState>();
        (state.feeds.folder).change(&app, move |feeds| feeds.import_opml(&text))
    })
    .await
}

#[tauri::command]
pub async fn feeds_export_opml(app: AppHandle, path: PathBuf) -> Result<(), String> {
    blocking(move || {
        let opml = app.state::<AppState>().feeds.folder.read(Feeds::opml)?;
        std::fs::write(&path, opml).map_err(|e| format!("{}: {e}", path.display()))
    })
    .await
}

#[tauri::command]
pub async fn feeds_settings(app: AppHandle) -> Result<FeedSettings, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let settings = Settings::load(&state.settings_path)?;
        state.feeds.folder.read(|feeds| {
            let kept = feeds.settings().clone();
            FeedSettings {
                interests: kept.interests,
                title_boost: kept.title_boost,
                rules: kept.rules,
                keep_days: kept.keep_days,
                lookback_days: kept.lookback_days,
                poll: settings.feeds_poll,
                poll_minutes: settings.feeds_poll_minutes,
            }
        })
    })
    .await
}

/// The saved searches of the sidebar, in their order.
#[tauri::command]
pub async fn feeds_searches(app: AppHandle) -> Result<Vec<Search>, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        state.feeds.folder.read(|feeds| feeds.searches().to_vec())
    })
    .await
}

/// Sets the saved searches; refused, nothing changed, for a blank name or one
/// given twice.
#[tauri::command]
pub async fn feeds_set_searches(app: AppHandle, searches: Vec<Search>) -> Result<(), String> {
    blocking(move || {
        let state = app.state::<AppState>();
        (state.feeds.folder).change(&app, move |feeds| feeds.set_searches(searches.clone()))
    })
    .await
}

/// Sets the settings of the feeds; refused, nothing changed, with the
/// message of the first interest that does not compile.
#[tauri::command]
pub async fn feeds_set_settings(app: AppHandle, settings: FeedSettings) -> Result<(), String> {
    blocking(move || {
        let kept = papers_feeds::FeedSettings {
            interests: settings.interests,
            title_boost: settings.title_boost,
            rules: settings.rules,
            keep_days: settings.keep_days,
            lookback_days: settings.lookback_days,
        };
        let state = app.state::<AppState>();
        (state.feeds.folder).change(&app, move |feeds| feeds.set_settings(kept.clone()))?;
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

/// Whether the Feeds tab shows an item's details beside the list (the setting `feeds_beside`),
/// rather than in place of it, to read.
#[tauri::command]
pub fn feeds_beside(app: AppHandle) -> Result<bool, String> {
    Ok(Settings::load(&app.state::<AppState>().settings_path)?.feeds_beside)
}

/// The details beside the list, or in place of it: from the Feeds section of the settings and
/// Window → Feeds Details Beside the List. The windows are told at once ([`BESIDE_EVENT`]), and
/// the menu follows.
#[tauri::command]
pub fn feeds_set_beside(app: AppHandle, beside: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut settings = Settings::load(&state.settings_path)?;
    settings.feeds_beside = beside;
    settings.save(&state.settings_path)?;
    #[cfg(target_os = "macos")]
    crate::menu::feeds_beside(&app, beside);
    let _ = app.emit(BESIDE_EVENT, beside);
    Ok(())
}

/// The details beside the list or in place of it, to every window: whether beside.
pub const BESIDE_EVENT: &str = "feeds-beside";

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use chrono::TimeZone;
    use papers_feeds::http::Recorded;
    use papers_feeds::{Reply, Request};

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
        let folder = Folder::with_opener(dir.join("feeds"), CHANGED, open);
        FeedsState::with_folder(folder, dir.join("feeds-fetched.json"))
    }

    /// A fetch made whole on this thread: the feeds read, then what follows.
    fn fetch(
        state: &FeedsState,
        only: Option<&str>,
        http: &dyn papers_feeds::Http,
    ) -> Result<FetchSummary, String> {
        let reading = match state.begin(only) {
            Begun::Joined(summary) => return Ok(summary),
            Begun::Reading(reading) => reading,
        };
        let (_, following) = reading.read(http, &Access::default())?;
        following.follow(state, http, &Access::default())
    }

    fn subscribe(state: &FeedsState, url: &str) -> String {
        let feed = Feed::Rss { url: url.into() };
        let settings = SubscriptionSettings::new(SourceKind::Journal);
        let subscription = (state.folder)
            .apply(move |feeds| feeds.subscribe(feed.clone(), settings.clone()))
            .unwrap();
        subscription.id
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
        let Begun::Reading(reading) = state.begin(None) else {
            unreachable!()
        };
        let (read, following) = reading.read(&http, &Access::default()).unwrap();
        let summary = following.follow(&state, &http, &Access::default()).unwrap();
        assert_eq!(read, summary, "nothing looked up");
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

        let again = fetch(&state, None, &http).unwrap();
        assert_eq!((again.fetched, again.unchanged, again.added), (0, 1, 0));
        assert_eq!(again.failed[0].message, "offline");
        // kept across a start; a subscription gone is dropped at the next fetch
        let reopened = opened(dir.path());
        assert_eq!(reopened.last_fetch().unwrap(), Some(again));
        let gone = news.clone();
        (reopened.folder)
            .apply(move |feeds| feeds.unsubscribe(&gone).map(drop))
            .unwrap();
        let http = Recorded::new().reply(TC, Reply::status(TC, 304));
        fetch(&reopened, None, &http).unwrap();
        assert!(!reopened.lasts().unwrap().contains_key(&news));
        assert_eq!(opened(dir.path()).lasts().unwrap().len(), 1);
    }

    /// The network of a test that changes the feeds while it is asked.
    /// A change of a folder, made while it is fetched.
    type Edit<'a> = Box<dyn FnOnce(&Folder) + Send + 'a>;

    struct Meanwhile<'a> {
        inner: Recorded,
        state: &'a FeedsState,
        change: Mutex<Option<Edit<'a>>>,
    }

    impl papers_feeds::Http for Meanwhile<'_> {
        fn get(&self, request: &Request) -> Result<Reply, String> {
            if let Some(change) = self.change.lock().unwrap().take() {
                change(&self.state.folder);
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
        fetch(&state, None, &http).unwrap();
        let id = state.folder.read(|f| f.items()[0].id.clone()).unwrap();
        let feed = atom("TC", &[("a", "Ice", None), ("b", "Firn", None)]);
        let http = Meanwhile {
            inner: Recorded::new().reply(TC, feed),
            state: &state,
            change: Mutex::new(Some(Box::new({
                let id = id.clone();
                move |folder: &Folder| {
                    let shortlist = move |f: &mut Feeds| f.set_triage(&id, Triage::Shortlisted);
                    folder.apply(shortlist).unwrap()
                }
            }))),
        };
        let summary = fetch(&state, None, &http).unwrap();
        assert_eq!(summary.added, 1);
        let read = |f: &Feeds| (f.triage(&id), f.items().len());
        assert_eq!(state.folder.read(read).unwrap(), (Triage::Shortlisted, 2));
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
            change: Mutex::new(Some(Box::new(move |folder: &Folder| {
                let new = move |f: &mut Feeds| f.set_searches(vec![search("New")]);
                folder.apply(new).unwrap()
            }))),
        };
        fetch(&state, None, &http).unwrap();
        let names = |state: &FeedsState| {
            state.folder.read(|f| {
                f.searches()
                    .iter()
                    .map(|s| s.name.clone())
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(names(&state).unwrap(), ["New"]);
        let twice = move |f: &mut Feeds| f.set_searches(vec![search("A"), search("a")]);
        assert!(state.folder.apply(twice).is_err());
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
        let http = Recorded::new().reply(TC, atom("TC", &[("a", "Ice", None)]));
        // a fetch reading the feeds, which goes on to look up
        let Begun::Reading(reading) = state.begin(None) else {
            unreachable!()
        };
        let joining = {
            let state = state.clone();
            let fetch = move || fetch(&state, None, &Recorded::new());
            std::thread::spawn(fetch)
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(!joining.is_finished(), "waits while the feeds are read");
        let (read, following) = reading.read(&http, &Access::default()).unwrap();
        assert_eq!(
            joining.join().unwrap(),
            Ok(read),
            "the summary of the feeds read"
        );
        following.follow(&state, &http, &Access::default()).unwrap();
    }

    /// A fetch of every subscription asked while a single one looks up
    /// waits for it, then fetches every subscription: it joins no fetch.
    #[test]
    fn a_whole_fetch_waits_for_a_single_subscription_s() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(opened(dir.path()));
        let tc = subscribe(&state, TC);
        subscribe(&state, NEWS);
        let http = Recorded::new().reply(TC, atom("TC", &[("a", "Ice", None)]));
        let Begun::Reading(reading) = state.begin(Some(&tc)) else {
            unreachable!()
        };
        let (_, following) = reading.read(&http, &Access::default()).unwrap();
        let whole = {
            let state = state.clone();
            std::thread::spawn(move || {
                let http = Recorded::new()
                    .reply(TC, Reply::status(TC, 304))
                    .reply(NEWS, Reply::status(NEWS, 304));
                fetch(&state, None, &http)
            })
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !whole.is_finished(),
            "waits while the subscription's lookups run"
        );
        following.follow(&state, &http, &Access::default()).unwrap();
        let whole = whole.join().unwrap().unwrap();
        assert_eq!((whole.fetched, whole.unchanged), (0, 2), "its own");
        assert_eq!(state.last_fetch().unwrap(), Some(whole));
    }

    #[test]
    fn a_single_subscription_fetched_keeps_the_summary_of_the_last_whole_fetch() {
        let dir = tempfile::tempdir().unwrap();
        let state = opened(dir.path());
        let tc = subscribe(&state, TC);
        subscribe(&state, NEWS);
        let http = Recorded::new()
            .reply(TC, Reply::status(TC, 304))
            .reply(NEWS, Reply::status(NEWS, 304));
        let whole = fetch(&state, None, &http).unwrap();
        let http = Recorded::new().reply(TC, atom("TC", &[("a", "Ice", None)]));
        let one = fetch(&state, Some(&tc), &http).unwrap();
        assert_eq!((one.fetched, one.unchanged, one.added), (1, 0, 1));
        assert_eq!(http.requests().len(), 1, "its feed alone");
        assert_eq!(state.last_fetch().unwrap(), Some(whole));
        assert!(state.lasts().unwrap()[&tc].at >= one.at);
        let gone = fetch(&state, Some("gone"), &http);
        assert!(gone.unwrap_err().contains("gone"));
    }

    #[test]
    fn the_facts_of_the_library_go_to_the_feeds_and_their_copies() {
        let dir = tempfile::tempdir().unwrap();
        let state = opened(dir.path());
        subscribe(&state, TC);
        let kept = papers_feeds::FeedSettings {
            rules: vec![Rule {
                when: "like:30-".into(),
                then: papers_feeds::Action::Shortlist,
            }],
            ..papers_feeds::FeedSettings::default()
        };
        (state.folder)
            .apply(move |f| f.set_settings(kept.clone()))
            .unwrap();
        let index = |title: &str| {
            let doc = papers_core::Doc {
                id: "lib".into(),
                title: title.into(),
                ..Default::default()
            };
            Arc::new(LibraryFacts {
                likeness: Some(Likeness::build([doc])),
                ..LibraryFacts::default()
            })
        };
        let folder = &state.folder;
        let (old, new) = (folder.next_build(), folder.next_build());
        assert!(folder.set_library(new, index("Firn densification")));
        assert!(
            !folder.set_library(old, index("Ice")),
            "an older build is dropped"
        );
        let http = Recorded::new().reply(
            TC,
            atom(
                "TC",
                &[("a", "Ice", None), ("b", "Firn densification", None)],
            ),
        );
        fetch(&state, None, &http).unwrap();
        let triage = |title: &str| {
            (state.folder)
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
            (state.folder)
                .read(|f| {
                    f.like(f.items().iter().find(|i| i.title == title).unwrap())
                        .map(|l| l.score)
                })
                .unwrap()
        };
        assert!(like("Firn densification").is_some_and(|l| l > 99.9));
        let empty = Arc::new(LibraryFacts::default());
        assert!(folder.set_library(folder.next_build(), empty));
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
        let settings = SubscriptionSettings {
            title: title.clone(),
            ..SubscriptionSettings::new(SourceKind::Journal)
        };
        let s = (state.folder)
            .apply(move |f| f.subscribe(feed.clone(), settings.clone()))
            .unwrap();
        assert_eq!(s.settings.title, title);
        assert_eq!(
            (state.folder)
                .read(|f| f.subscriptions()[0].settings.title.clone())
                .unwrap(),
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
        let dir = tempfile::tempdir().unwrap();
        let state = opened(dir.path());
        let id = subscribe(&state, "https://eos.org/feed");
        let update = |patch: &str| {
            let patch: SubscriptionPatch = serde_json::from_str(patch).unwrap();
            let id = id.clone();
            let edited = state.folder.apply(move |f| update(f, &id, &patch));
            edited.unwrap().settings
        };
        update(r#"{"title": "Eos", "journal": "Eos"}"#);
        let eos = update(r#"{"title": " ", "kind": "news", "journal": null}"#);
        assert_eq!(
            (eos.title.clone(), eos.kind, eos.journal.clone()),
            (None, SourceKind::News, None)
        );
        assert!(!eos.fetch_pages);
        assert_eq!(eos.boost, 0.0, "left out stays");
        assert_eq!(update(r#"{"boost": 2.5}"#).boost, 2.5);
    }
}
