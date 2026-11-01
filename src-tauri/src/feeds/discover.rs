//! The Add sheet's discovery: the suggested subscriptions, the journals of
//! the library and what follows each, a journal looked up or searched in
//! OpenAlex, the feed its site links to, the kind of a feed guessed.

use papers_core::Library;
use papers_feeds::{Feed, Journal, Network, SiteFeed, SourceKind};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::fetch::{blocking, page_agent};

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
        let requests = app.state::<AppState>().requests()?;
        papers_feeds::guess_kind(&requests.http, &requests.access, url.trim())
    })
    .await
}

#[tauri::command]
pub async fn feeds_search_journals(app: AppHandle, text: String) -> Result<Vec<Journal>, String> {
    blocking(move || {
        let requests = app.state::<AppState>().requests()?;
        papers_feeds::search_journals(&requests.http, &requests.access, text.trim())
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
        let requests = state.requests()?;
        let journal = state.feeds.journals(|journals| {
            journals.with_homepage(journal, &requests.http, &requests.access)
        })??;
        let page = Network::with_user_agent(&page_agent(requests.access.mailto.as_deref()))?;
        let feed = (journal.homepage.as_deref()).map(|homepage| {
            papers_feeds::site_feed(&page, &requests.http, homepage, &journal.title)
        });
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
        let requests = state.requests()?;
        let follow = state
            .feeds
            .journals(|journals| journals.look_up(&title, &requests.http, &requests.access))??;
        Ok(follow.into())
    })
    .await
}

#[cfg(test)]
mod tests {
    use papers_feeds::SubscriptionSettings;

    use super::*;
    use crate::feeds::FeedsState;

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

        let state = FeedsState::on_clock(dir.path());
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
        let state = FeedsState::on_clock(dir.path());
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
    }
}
