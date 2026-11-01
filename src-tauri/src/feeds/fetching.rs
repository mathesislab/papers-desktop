//! The fetches of the feeds, one at a time: the feeds read on a copy, whose
//! summary a command answers, then what follows on a thread of its own; the
//! last summary and each subscription's last fetch kept.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Utc};
use papers_feeds::{Access, FeedReport, Feeds, Outcome, Subscription};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use super::folder::Held;
use super::{FeedsState, title_of};
use crate::AppState;

/// The event of the page of where a fetch is (`Phase`).
const FETCHING: &str = "feeds-fetching";

/// Where a fetch is: reading the feeds, looking up what they brought, done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Reading,
    LookingUp,
    Done,
}

/// Who is told where a fetch is: the windows, or a test.
pub type Tell = Arc<dyn Fn(Phase) + Send + Sync>;

/// The end of a fetch's phase, whatever ends it (an error, a panic, what
/// follows never looked up): `Done` told, and the fetch of every
/// subscription in flight no more. Dropped before the work on a copy is let
/// go of, so a fetch that waits for it is told `Reading` after this `Done`.
struct Ending {
    tell: Tell,
    /// of a fetch of every subscription
    in_flight: Option<InFlight>,
}

impl Drop for Ending {
    fn drop(&mut self) {
        if let Some(in_flight) = &self.in_flight {
            *in_flight.lock().unwrap_or_else(PoisonError::into_inner) = None;
        }
        (self.tell)(Phase::Done);
    }
}

/// The summary of the feeds read of a fetch of every subscription, while it
/// looks up what they brought.
type InFlight = Arc<Mutex<Option<FetchSummary>>>;

/// The fetches of the feeds, one at a time, and what is kept of them.
pub(super) struct Fetching {
    /// `feeds-fetched.json`: the last summary, each subscription's last fetch
    path: PathBuf,
    /// held by a fetch while it reads the feeds: a fetch asked meanwhile waits
    reading: Mutex<()>,
    /// a fetch of every subscription asked meanwhile answers it
    in_flight: InFlight,
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
    /// before `running`: dropped first
    _ending: Ending,
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

impl FeedsState {
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
    pub(super) fn lasts(&self) -> Result<BTreeMap<String, Last>, String> {
        let fetched = self.fetching.fetched()?;
        Ok(fetched.as_ref().map(|f| f.last.clone()).unwrap_or_default())
    }
}

impl Reading<'_> {
    /// The feeds read, on a copy: their summary (kept, unless of a single
    /// subscription), and what follows, to look up; `tell` told where it is.
    pub fn read(
        self,
        http: &dyn papers_feeds::Http,
        access: &Access,
        tell: Tell,
    ) -> Result<(FetchSummary, Following), String> {
        let Reading {
            state,
            only,
            _reading,
            running,
        } = self;
        let (folder, fetching) = (&state.folder, &state.fetching);
        tell(Phase::Reading);
        let mut ending = Ending {
            tell: tell.clone(),
            in_flight: None,
        };
        let only = only.as_deref();
        let (report, known) = folder.on_copy(&running, |feeds| {
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
            ending.in_flight = Some(fetching.in_flight.clone());
        }
        tell(Phase::LookingUp);
        let following = Following {
            _ending: ending,
            running,
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
        let (followed, added) = state.folder.on_copy(&self.running, |feeds| {
            let followed = feeds.follow_up(known, http, access)?;
            Ok((followed, new_items(feeds, known)))
        })?;
        let summary = FetchSummary {
            lookups: followed.lookups,
            pages: followed.pages,
            added,
            ..self.summary.clone()
        };
        if self.whole {
            (state.fetching).keep(Some(&summary), Vec::new(), &self.subscriptions)?;
        }
        // `self` goes on return: `Done` told, then the work on a copy let go of
        Ok(summary)
    }
}

impl Fetching {
    /// The fetches of feeds, what is kept of them in a file.
    pub(super) fn new(path: PathBuf) -> Fetching {
        Fetching {
            path,
            reading: Mutex::new(()),
            in_flight: Arc::new(Mutex::new(None)),
            fetched: Mutex::new(None),
        }
    }

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

/// Fetches every subscription, or the one of id `only`: the summary once
/// the feeds are read, what follows going on, on a thread of its own; the
/// windows told where it is.
pub(super) fn fetch_now(app: &AppHandle, only: Option<String>) -> Result<FetchSummary, String> {
    let state = app.state::<AppState>();
    let reading = match state.feeds.begin(only.as_deref()) {
        Begun::Joined(summary) => return Ok(summary),
        Begun::Reading(reading) => reading,
    };
    let requests = state.requests()?;
    let told: Tell = {
        let app = app.clone();
        Arc::new(move |phase| tell(&app, phase))
    };
    let (summary, following) = reading.read(&requests.http, &requests.access, told)?;
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let followed = following.follow(&state.feeds, &requests.http, &requests.access);
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use papers_feeds::http::Recorded;
    use papers_feeds::{Reply, Request, Rule, Search, Sort, Triage};

    use super::*;
    use std::time::Duration;

    use papers_core::Likeness;
    use papers_feeds::LibraryFacts;

    use crate::feeds::Folder;

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

    /// Nobody told where a fetch is.
    fn quiet() -> Tell {
        Arc::new(|_| {})
    }

    /// The phases a fetch tells, kept.
    fn recorded() -> (Arc<Mutex<Vec<Phase>>>, Tell) {
        let phases = Arc::new(Mutex::new(Vec::new()));
        let kept = phases.clone();
        (
            phases,
            Arc::new(move |phase| kept.lock().unwrap().push(phase)),
        )
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
        let (_, following) = reading.read(http, &Access::default(), quiet())?;
        following.follow(state, http, &Access::default())
    }

    #[test]
    fn a_fetch_is_summed_up_and_each_subscription_s_last_kept() {
        let dir = tempfile::tempdir().unwrap();
        let state = FeedsState::on_clock(dir.path());
        let tc = state.subscribed(TC);
        let news = state.subscribed(NEWS);
        let feed = atom("The Cryosphere", &[("a", "Ice", None), ("b", "Firn", None)]);
        let http = Recorded::new()
            .reply(TC, feed.with_validators(Some("\"v1\""), None))
            .reply(NEWS, Reply::status(NEWS, 503))
            .reply(TC, Reply::status(TC, 304))
            .fail(NEWS, "offline");
        let Begun::Reading(reading) = state.begin(None) else {
            unreachable!()
        };
        let (phases, told) = recorded();
        let (read, following) = reading.read(&http, &Access::default(), told).unwrap();
        let summary = following.follow(&state, &http, &Access::default()).unwrap();
        assert_eq!(read, summary, "nothing looked up");
        let phases = phases.lock().unwrap().clone();
        assert_eq!(phases, [Phase::Reading, Phase::LookingUp, Phase::Done]);
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
        let reopened = FeedsState::on_clock(dir.path());
        assert_eq!(reopened.last_fetch().unwrap(), Some(again));
        let gone = news.clone();
        (reopened.folder)
            .apply(move |feeds| feeds.unsubscribe(&gone).map(drop))
            .unwrap();
        let http = Recorded::new().reply(TC, Reply::status(TC, 304));
        fetch(&reopened, None, &http).unwrap();
        assert!(!reopened.lasts().unwrap().contains_key(&news));
        assert_eq!(FeedsState::on_clock(dir.path()).lasts().unwrap().len(), 1);
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
        let state = FeedsState::on_clock(dir.path());
        state.subscribed(TC);
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
        let state = FeedsState::on_clock(dir.path());
        state.subscribed(TC);
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
            names(&FeedsState::on_clock(dir.path())).unwrap(),
            ["New"],
            "written, unchanged"
        );
    }

    #[test]
    fn a_second_fetch_joins_the_running_one() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(FeedsState::on_clock(dir.path()));
        state.subscribed(TC);
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
        let (read, following) = reading.read(&http, &Access::default(), quiet()).unwrap();
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
        let state = Arc::new(FeedsState::on_clock(dir.path()));
        let tc = state.subscribed(TC);
        state.subscribed(NEWS);
        let http = Recorded::new().reply(TC, atom("TC", &[("a", "Ice", None)]));
        let Begun::Reading(reading) = state.begin(Some(&tc)) else {
            unreachable!()
        };
        let (_, following) = reading.read(&http, &Access::default(), quiet()).unwrap();
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
        let state = FeedsState::on_clock(dir.path());
        let tc = state.subscribed(TC);
        state.subscribed(NEWS);
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
        let state = FeedsState::on_clock(dir.path());
        state.subscribed(TC);
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
            let likeness = Some(Arc::new(Likeness::build([doc])));
            move |_: Option<&LibraryFacts>| LibraryFacts {
                likeness,
                ..LibraryFacts::default()
            }
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
        // the identities of a library read again, with the index of before
        let identities = |old: Option<&LibraryFacts>| LibraryFacts {
            likeness: old.and_then(|f| f.likeness.clone()),
            ..LibraryFacts::default()
        };
        assert!(folder.set_library(folder.next_build(), identities));
        assert!(like("Firn densification").is_some_and(|l| l > 99.9));
        let empty = |_: Option<&LibraryFacts>| LibraryFacts::default();
        assert!(folder.set_library(folder.next_build(), empty));
        assert_eq!(like("Firn densification"), None);
    }

    /// A network that panics at the lookups (Crossref).
    struct Panics(Recorded);

    impl papers_feeds::Http for Panics {
        fn get(&self, request: &Request) -> Result<Reply, String> {
            assert!(!request.url.contains("crossref"), "a lookup");
            self.0.get(request)
        }
    }

    /// A fetch whose lookups panic, or whose feeds cannot be read, says it is
    /// done before it lets go of the work on a copy, and is in flight no more.
    #[test]
    fn a_fetch_that_ends_badly_is_done() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(FeedsState::on_clock(dir.path()));
        state.subscribed(TC);
        let feed = atom("TC", &[("a", "Ice", Some("10.5194/tc-1"))]);
        let http = Panics(Recorded::new().reply(TC, feed));
        let Begun::Reading(reading) = state.begin(None) else {
            unreachable!()
        };
        let (phases, told) = recorded();
        let (_, following) = reading.read(&http, &Access::default(), told).unwrap();
        let panicked = std::thread::scope(|scope| {
            let follow = || following.follow(&state, &http, &Access::default());
            scope.spawn(follow).join()
        });
        assert!(panicked.is_err());
        let phases = phases.lock().unwrap().clone();
        assert_eq!(phases, [Phase::Reading, Phase::LookingUp, Phase::Done]);
        // the gate let go of, nothing in flight: the next fetch reads the feeds
        let Begun::Reading(reading) = state.begin(None) else {
            panic!("joined a fetch that panicked")
        };
        let (phases, told) = recorded();
        let gone = Recorded::new().fail(TC, "offline");
        drop(reading.read(&gone, &Access::default(), told));
        assert_eq!(
            phases.lock().unwrap()[..],
            [Phase::Reading, Phase::LookingUp, Phase::Done]
        );
        let Begun::Reading(reading) = state.begin(Some("gone")) else {
            unreachable!()
        };
        let (phases, told) = recorded();
        assert!(reading.read(&gone, &Access::default(), told).is_err());
        assert_eq!(phases.lock().unwrap()[..], [Phase::Reading, Phase::Done]);
    }
}
