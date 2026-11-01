//! A folder of feeds, the Feeds tab's or the Search tab's: the feeds the
//! commands see, and a copy worked on meanwhile, the changes made while it
//! runs made again on it before it takes their place. What it is given (the
//! facts of the library; the Search tab's, the scoring of the feeds) goes to
//! the feeds and to each copy.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use papers_feeds::{Access, Feeds, LibraryFacts, Pdf, Scoring};
use tauri::{AppHandle, Emitter};

use super::rows::{Linked, linked};

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
    pub(super) dir: PathBuf,
    live: Mutex<Live>,
    /// held by the work on a copy that runs
    pub(super) running: Arc<Gate>,
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
pub(super) struct Gate {
    held: Mutex<bool>,
    let_go: Condvar,
}

/// A gate held, until dropped.
pub struct Held(Arc<Gate>);

impl Gate {
    /// The gate held, once let go of by the one who holds it.
    pub(super) fn hold(self: &Arc<Gate>) -> Held {
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

/// The changes of a folder kept while a copy of it is worked on; none once
/// dropped.
struct Journalling<'a>(&'a Folder);

impl Drop for Journalling<'_> {
    fn drop(&mut self) {
        let mut live = (self.0.live.lock()).unwrap_or_else(PoisonError::into_inner);
        live.journal = None;
    }
}

/// The feeds the commands see, the changes made while a copy is worked on,
/// and what the feeds and their copies are given: the facts of the library,
/// another folder's scoring.
#[derive(Default)]
struct Live {
    feeds: Option<Feeds>,
    journal: Option<Vec<Journalled>>,
    library: Option<Arc<LibraryFacts>>,
    /// the build of `library`
    build: u64,
    /// how another folder scores its items; none: the folder's own settings
    scoring: Option<Arc<Scoring>>,
}

impl Live {
    /// What the feeds are given, to feeds opened or worked on.
    fn give(&self, feeds: &mut Feeds) {
        if let Some(library) = &self.library {
            feeds.set_library(library.clone());
        }
        if let Some(scoring) = &self.scoring {
            feeds.set_scoring(scoring.clone());
        }
    }
}

impl Folder {
    /// The feeds of a folder, not opened yet; `changed` the event that its
    /// items changed.
    pub fn new(dir: PathBuf, changed: &'static str) -> Folder {
        Folder::with_opener(dir, changed, Box::new(Feeds::open))
    }

    pub(super) fn with_opener(dir: PathBuf, changed: &'static str, open: Opener) -> Folder {
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

    /// The feeds of the folder, opened, with what they are given.
    fn opened(&self, live: &Live) -> Result<Feeds, String> {
        let mut feeds = (self.open)(&self.dir).map_err(|e| e.to_string())?;
        live.give(&mut feeds);
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

    /// The facts of the library of build `build`, made of those it has, for
    /// the feeds and the copies opened from now on, unless a newer build was
    /// given already: whether they were taken.
    pub fn set_library(
        &self,
        build: u64,
        facts: impl FnOnce(Option<&LibraryFacts>) -> LibraryFacts,
    ) -> bool {
        let mut live = self.live.lock().unwrap();
        if build < live.build {
            return false;
        }
        live.build = build;
        let library = Arc::new(facts(live.library.as_deref()));
        live.library = Some(library.clone());
        if let Some(feeds) = &mut live.feeds {
            feeds.set_library(library);
        }
        true
    }

    /// Another folder's scoring, for the feeds and the copies opened from
    /// now on: their items scored as that folder's are.
    pub fn set_scoring(&self, scoring: Arc<Scoring>) {
        let mut live = self.live.lock().unwrap();
        if let Some(feeds) = &mut live.feeds {
            feeds.set_scoring(scoring.clone());
        }
        live.scoring = Some(scoring);
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
    pub(super) fn apply<T: Send + 'static>(
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
    pub(super) fn on_copy<T>(
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
        // whatever ends the work (a panic too), no change is kept for it
        let _journalling = Journalling(self);
        let done = work(&mut copy);
        let mut live = self.live.lock().unwrap();
        let journal = live.journal.take().unwrap_or_default();
        let done = done.map_err(|e| e.to_string())?;
        for change in &journal {
            change(&mut copy);
        }
        // the facts gathered meanwhile, a scoring given meanwhile
        live.give(&mut copy);
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

#[cfg(test)]
mod tests {
    use papers_feeds::{Entry, FOUND, FeedSettings, ItemKind, Origin};

    use super::*;

    /// The feeds' folder, with an interest in ice sheets.
    fn scored(dir: &Path) -> Folder {
        let feeds = Folder::new(dir.join("feeds"), "feeds-changed");
        let ice = r#"{"name": "Ice", "include": [{"term": "ice sheet", "weight": 3}]}"#;
        let ice: papers_feeds::Interest = serde_json::from_str(ice).unwrap();
        (feeds.apply(move |f| {
            f.set_settings(FeedSettings {
                interests: vec![ice.clone()],
                ..f.settings().clone()
            })
        }))
        .unwrap();
        feeds
    }

    /// The keywords part of the score of the works found, by their ids.
    fn keywords(folder: &Folder, ids: &[String]) -> Vec<Option<f64>> {
        let rating = |f: &Feeds, id: &String| f.rating(f.item(id).unwrap()).keywords;
        folder
            .read(|f| ids.iter().map(|id| rating(f, id)).collect())
            .unwrap()
    }

    #[test]
    fn the_works_found_are_scored_as_the_feeds_given_and_their_copies_too() {
        let dir = tempfile::tempdir().unwrap();
        let feeds = scored(dir.path());
        let search = Folder::new(dir.path().join("search"), "search-changed");
        let found = |title: &str| {
            let entry = Entry::new(
                FOUND,
                Origin::Work,
                title.into(),
                title.into(),
                ItemKind::Paper,
            );
            move |f: &mut Feeds| f.found(vec![entry.clone()], false)
        };
        let ids = search.apply(found("ice sheet")).unwrap();
        assert_eq!(
            keywords(&search, &ids),
            [None],
            "a folder without interests"
        );

        search.set_scoring(feeds.read(Feeds::scoring).unwrap());
        assert_eq!(keywords(&search, &ids), [Some(100.0)]);
        // a copy worked on, and the feeds opened again: given it too
        let running = search.running.hold();
        let more = search.on_copy(&running, found("the ice sheet")).unwrap();
        drop(running);
        let all: Vec<String> = ids.iter().chain(&more).cloned().collect();
        assert_eq!(keywords(&search, &all), [Some(100.0), Some(100.0)]);
        let again = Folder::new(dir.path().join("search"), "search-changed");
        again.set_scoring(feeds.read(Feeds::scoring).unwrap());
        assert_eq!(keywords(&again, &all), [Some(100.0), Some(100.0)]);
    }
}
