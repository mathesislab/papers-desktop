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
//! The requests go through one network of the crate's (a browser's user
//! agent, which every publisher answers), paced per host for every command;
//! Crossref and OpenAlex are asked with the e-mail address and the OpenAlex
//! key of the settings, each sent only when set (made again when they
//! change). The last fetch's summary, and how each
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

pub mod discover;
mod fetching;
mod folder;
mod rows;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use papers_core::Likeness;
use papers_feeds::http::{Paced, Pacing};
use papers_feeds::{
    Access, Feed, Feeds, Interest, Journals, LibraryFacts, Network, Pdf, Rule, SavedView,
    SourceKind, Subscription, SubscriptionSettings, Triage,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub use fetching::FetchSummary;
use fetching::{Fetching, Last, fetch_now};
pub use folder::Folder;
use rows::{FeedDetail, FeedView, Linked, ViewRequest, detail, view};
pub use rows::{FeedRow, Rows};

use crate::AppState;
use crate::fetch::blocking;
use crate::settings::Settings;

/// The event of the page that the items changed.
const CHANGED: &str = "feeds-changed";

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

/// The background polling: its first look a while after the start, then
/// each minute, fetching when the last fetch is older than the setting.
const FIRST: Duration = Duration::from_secs(120);
const TICK: Duration = Duration::from_secs(60);

/// The fewest minutes between two fetches of the polling.
const LEAST_MINUTES: u32 = 5;

/// The feeds of the Feeds tab, their fetches, and the journals looked up.
pub struct FeedsState {
    pub folder: Folder,
    fetching: Fetching,
    /// the journals looked up in OpenAlex, opened at the first ask
    journals: Mutex<Option<Journals>>,
}

impl FeedsState {
    /// The feeds of a folder, not opened yet; what is kept of their fetches in a file.
    pub fn new(dir: PathBuf, fetched_path: PathBuf) -> FeedsState {
        FeedsState::with_folder(Folder::new(dir, CHANGED), fetched_path)
    }

    fn with_folder(folder: Folder, fetched_path: PathBuf) -> FeedsState {
        FeedsState {
            folder,
            fetching: Fetching::new(fetched_path),
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

    /// A subscription as the settings list it, with its last fetch.
    pub fn info(&self, subscription: Subscription) -> Result<SubscriptionInfo, String> {
        Ok(info(subscription, &self.lasts()?))
    }
}

/// A subscription's title: its own, else what it follows.
fn title_of(subscription: &Subscription) -> String {
    (subscription.settings.title.clone()).unwrap_or_else(|| subscription.feed.to_string())
}

/// A subscription, as the settings list it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionInfo {
    id: String,
    title: String,
    kind: SourceKind,
    feed: Feed,
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

/// Something done to the item of each id, those of no item (gone since the
/// page listed them, or pruned on a fetched copy) passed over.
fn each(
    ids: &[String],
    mut f: impl FnMut(&str) -> papers_feeds::Result<()>,
) -> papers_feeds::Result<()> {
    for id in ids {
        match f(id) {
            Ok(()) | Err(papers_feeds::Error::NoItem { .. }) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// A field that is there, null or not.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
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

/// A subscription, with its last fetch.
fn info(subscription: Subscription, lasts: &BTreeMap<String, Last>) -> SubscriptionInfo {
    SubscriptionInfo {
        title: title_of(&subscription),
        last: lasts.get(&subscription.id).cloned(),
        feed: subscription.feed,
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

/// The requests of the feeds and of the Search tab: one network, paced per
/// host for them all, and what the APIs are asked with.
pub struct Requests {
    pub http: Paced<Network>,
    pub access: Access,
}

impl AppState {
    /// The requests, made at the first ask after the start, or after a
    /// change of the address or the key.
    pub fn requests(&self) -> Result<Arc<Requests>, String> {
        let mut requests = self.requests.lock().unwrap();
        if requests.is_none() {
            let access = access(&Settings::load(&self.settings_path)?);
            let http = Paced::new(Network::new()?, Pacing::polite(&access));
            *requests = Some(Arc::new(Requests { http, access }));
        }
        Ok(requests.clone().expect("made"))
    }

    /// The requests made again at the next ask, if the settings saved change
    /// what the APIs are asked with.
    pub fn saved_settings(&self, old: &Settings, new: &Settings) {
        if access(old) != access(new) {
            *self.requests.lock().unwrap() = None;
        }
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
        let give = |facts: &dyn Fn(Option<&LibraryFacts>) -> LibraryFacts| {
            for (store, build) in builds {
                let folder = store.of(&state);
                if folder.set_library(build, facts) {
                    folder.tell(&app);
                }
            }
        };
        // the identities at once, with the index of before till it is built
        let library = state.library.read().unwrap();
        let identities = library.identities();
        give(&|old| LibraryFacts {
            likeness: old.and_then(|f| f.likeness.clone()),
            identities: identities.clone(),
        });
        let likeness = (!library.is_empty()).then(|| Arc::new(Likeness::of(&library)));
        drop(library);
        give(&|_| LibraryFacts {
            likeness: likeness.clone(),
            identities: identities.clone(),
        });
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
                && settings.elsewhere.feeds_poll
            {
                let minutes = settings.elsewhere.feeds_poll_minutes.max(LEAST_MINUTES);
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
            each(&ids, |id| feeds.set_triage(id, triage))
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
        (state.feeds.folder).change(&app, move |feeds| each(&ids, |id| feeds.remind(id, when)))
    })
    .await
}

#[tauri::command]
pub async fn feeds_find_papers(app: AppHandle, id: String) -> Result<Vec<Linked>, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let requests = state.requests()?;
        let found = state
            .feeds
            .folder
            .find_papers(&id, &requests.http, &requests.access);
        state.feeds.folder.tell(&app);
        found
    })
    .await
}

#[tauri::command]
pub async fn feeds_look_up(app: AppHandle, store: Store, id: String) -> Result<bool, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let requests = state.requests()?;
        let folder = store.of(&state);
        let asked = folder.look_up(&id, &requests.http, &requests.access)?;
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
        let requests = state.requests()?;
        store.of(&state).pdf(&id, &requests.http, &requests.access)
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
    feed: Feed,
    kind: SourceKind,
    title: Option<String>,
) -> Result<SubscriptionInfo, String> {
    let settings = SubscriptionSettings {
        title,
        ..SubscriptionSettings::new(kind)
    };
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
                poll: settings.elsewhere.feeds_poll,
                poll_minutes: settings.elsewhere.feeds_poll_minutes,
            }
        })
    })
    .await
}

/// The saved views of the sidebar, in their order.
#[tauri::command]
pub async fn feeds_saved_views(app: AppHandle) -> Result<Vec<SavedView>, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        state.feeds.folder.read(|feeds| feeds.saved_views().to_vec())
    })
    .await
}

/// Sets the saved views; refused, nothing changed, for a blank name or one
/// given twice.
#[tauri::command]
pub async fn feeds_set_saved_views(app: AppHandle, views: Vec<SavedView>) -> Result<(), String> {
    blocking(move || {
        let state = app.state::<AppState>();
        (state.feeds.folder).change(&app, move |feeds| feeds.set_saved_views(views.clone()))
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
        kept.elsewhere.feeds_poll = settings.poll;
        kept.elsewhere.feeds_poll_minutes = settings.poll_minutes.max(LEAST_MINUTES);
        kept.save(&state.settings_path)
    })
    .await
}

#[cfg(test)]
impl FeedsState {
    /// The feeds of a folder on a fixed clock, what is kept of their fetches
    /// beside it.
    fn on_clock(dir: &std::path::Path) -> FeedsState {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 10, 31, 8, 0, 0).unwrap();
        let open =
            Box::new(move |dir: &std::path::Path| Feeds::open_with(dir, Box::new(move || now)));
        let folder = Folder::with_opener(dir.join("feeds"), CHANGED, open);
        FeedsState::with_folder(folder, dir.join("feeds-fetched.json"))
    }

    /// A subscription of a journal's feed, by its address: its id.
    fn subscribed(&self, url: &str) -> String {
        let feed = Feed::Rss { url: url.into() };
        let settings = SubscriptionSettings::new(SourceKind::Journal);
        let subscription = (self.folder)
            .apply(move |feeds| feeds.subscribe(feed.clone(), settings.clone()))
            .unwrap();
        subscription.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_gone_are_passed_over() {
        let mut done = Vec::new();
        let ids = ["a", "gone", "b"].map(String::from);
        each(&ids, |id| {
            if id == "gone" {
                return Err(papers_feeds::Error::NoItem { id: id.into() });
            }
            done.push(id.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(done, ["a", "b"]);
    }

    #[test]
    fn a_subscription_patched_as_the_page_writes_it() {
        // a journal left out stays, null clears it
        let patch: SubscriptionPatch = serde_json::from_str(r#"{"title": "Eos"}"#).unwrap();
        assert_eq!(patch.journal, None);
        let patch: SubscriptionPatch = serde_json::from_str(r#"{"journal": null}"#).unwrap();
        assert_eq!(patch.journal, Some(None));
        let patch: SubscriptionPatch = serde_json::from_str(r#"{"fetchPages": true}"#).unwrap();
        assert_eq!(patch.fetch_pages, Some(true));
        // applied: what it has, a blank title or journal as none
        let dir = tempfile::tempdir().unwrap();
        let state = FeedsState::on_clock(dir.path());
        let id = state.subscribed("https://eos.org/feed");
        let patched = |patch: &str| {
            let patch: SubscriptionPatch = serde_json::from_str(patch).unwrap();
            let id = id.clone();
            let edited = state.folder.apply(move |f| update(f, &id, &patch));
            edited.unwrap().settings
        };
        patched(r#"{"title": "Eos", "journal": "Eos"}"#);
        let eos = patched(r#"{"title": " ", "kind": "news", "journal": null}"#);
        assert_eq!(
            (eos.title.clone(), eos.kind, eos.journal.clone()),
            (None, SourceKind::News, None)
        );
        assert!(!eos.fetch_pages);
        assert_eq!(eos.boost, 0.0, "left out stays");
        assert_eq!(patched(r#"{"boost": 2.5}"#).boost, 2.5);
    }
}
