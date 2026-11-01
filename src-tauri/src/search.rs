//! The Search tab: the commands of `src/lib/search/api.ts` over OpenAlex's
//! works (crates/feeds' `search_works`) and the folder of the works found,
//! `app_data_dir/search` (a `Folder` of its own, `Store::Search`). A
//! search's first page takes the place of the works found before, but for
//! those saved for later (shortlisted) or added; the next page adds to it.
//! The last query's journals and works looked up are kept for its next pages
//! and its following. The rows are the Feeds tab's (`FeedRow`); the details,
//! the triage, the abstracts and the PDFs are the Feeds tab's commands, with
//! the store.

use std::sync::Mutex;

use papers_feeds::{
    Access, Feed, Http, Order, Resolved, SourceKind, SubscriptionSettings, ask, follow_filter,
    resolve, search_works,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::feeds::{FeedRow, Rows, SubscriptionInfo, network};
use crate::fetch::blocking;
use crate::settings::Settings;

/// The items' event of the Search tab: the works found changed.
pub const CHANGED: &str = "search-changed";

/// The Search tab shown or hidden, to every window: whether it is shown.
pub const TAB_EVENT: &str = "search-tab";

/// A page of a search, as the list shows it.
#[derive(Debug, Serialize)]
pub struct SearchPage {
    rows: Vec<FeedRow>,
    /// the works found in all
    total: u64,
    /// the cursor of the next page; none after the last
    next: Option<String>,
}

/// The last query resolved: what it asks of OpenAlex, its journals and works
/// looked up.
#[derive(Default)]
pub struct LastResolved(Mutex<Option<(String, Resolved)>>);

impl LastResolved {
    /// What a query asks of OpenAlex: the last one's, else resolved and kept.
    fn of(&self, query: &str, http: &dyn Http, access: &Access) -> Result<Resolved, String> {
        let mut last = self.0.lock().unwrap();
        if let Some((asked, resolved)) = last.as_ref()
            && asked == query
        {
            return Ok(resolved.clone());
        }
        let resolved = resolve(http, access, &ask(query)?)?;
        *last = Some((query.to_string(), resolved.clone()));
        Ok(resolved)
    }
}

/// The rows of items of the works found, in their order, the query's words
/// marked; those gone are left out.
fn rows_of(app: &AppHandle, ids: &[String], query: &str) -> Result<Vec<FeedRow>, String> {
    let state = app.state::<AppState>();
    let matching = *state.matching.read().unwrap();
    state.search.read(|feeds| {
        let rows = Rows::new(feeds);
        (ids.iter())
            .filter_map(|id| feeds.row_of(id, query, matching))
            .map(|row| rows.row(row))
            .collect()
    })
}

/// A page of the works of a query: the first (`cursor` none), or the next.
#[tauri::command]
pub async fn search_run(
    app: AppHandle,
    query: String,
    order: Order,
    cursor: Option<String>,
) -> Result<SearchPage, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (network, access) = network(&state)?;
        let resolved = state.resolved.of(&query, &network, &access)?;
        let found = search_works(&network, &access, &resolved, order, cursor.as_deref())?;
        let fresh = cursor.is_none();
        let entries = found.entries;
        let ids = (state.search).change(&app, move |feeds| feeds.found(entries.clone(), fresh))?;
        Ok(SearchPage {
            rows: rows_of(&app, &ids, &query)?,
            total: found.total,
            next: found.next,
        })
    })
    .await
}

/// The rows of works found again, as they are now (starred, added).
#[tauri::command]
pub async fn search_rows(
    app: AppHandle,
    ids: Vec<String>,
    query: String,
) -> Result<Vec<FeedRow>, String> {
    blocking(move || rows_of(&app, &ids, &query)).await
}

/// A search followed in the Feeds tab: a sweep of OpenAlex of its filter (its words searched in
/// the titles and abstracts), titled as the query, its works papers. Fetched by the caller.
#[tauri::command]
pub async fn search_follow(app: AppHandle, query: String) -> Result<SubscriptionInfo, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (network, access) = network(&state)?;
        let filter = follow_filter(&state.resolved.of(&query, &network, &access)?)?;
        let feed = Feed::OpenAlex { filter };
        let settings = SubscriptionSettings {
            title: Some(query),
            ..SubscriptionSettings::new(SourceKind::Journal)
        };
        let subscription = (state.feeds.folder).change(&app, move |feeds| {
            feeds.subscribe(feed.clone(), settings.clone())
        })?;
        state.feeds.info(subscription)
    })
    .await
}

/// Whether the main window shows the Search tab (the setting `search_tab`).
#[tauri::command]
pub fn search_tab(app: AppHandle) -> Result<bool, String> {
    Ok(Settings::load(&app.state::<AppState>().settings_path)?.search_tab)
}

/// Shows or hides the Search tab of the main window: from the settings and Window → Show Search
/// Tab. The windows are told at once ([`TAB_EVENT`]), and the menu follows.
#[tauri::command]
pub fn search_set_tab(app: AppHandle, show: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut settings = Settings::load(&state.settings_path)?;
    settings.search_tab = show;
    settings.save(&state.settings_path)?;
    #[cfg(target_os = "macos")]
    crate::menu::search_tab(&app, show);
    let _ = app.emit(TAB_EVENT, show);
    Ok(())
}

/// The works saved for later, the latest found first.
#[tauri::command]
pub async fn search_saved(app: AppHandle) -> Result<Vec<FeedRow>, String> {
    blocking(move || {
        let ids = app.state::<AppState>().search.read(|feeds| {
            let mut saved: Vec<_> = (feeds.items().iter())
                .filter(|i| feeds.triage(&i.id) == papers_feeds::Triage::Shortlisted)
                .map(|i| (i.first_seen(), i.id.clone()))
                .collect();
            saved.sort_by_key(|a| std::cmp::Reverse(a.0));
            saved.into_iter().map(|(_, id)| id).collect::<Vec<_>>()
        })?;
        rows_of(&app, &ids, "")
    })
    .await
}

#[cfg(test)]
mod tests {
    use papers_feeds::Reply;
    use papers_feeds::http::Recorded;

    use super::*;

    #[test]
    fn a_query_is_resolved_once() {
        let doi = "https://api.openalex.org/works/doi:10.5194/tc-1?select=id,doi,title,\
            publication_date,authorships,primary_location,abstract_inverted_index,type,cited_by_count";
        let w1 = r#"{"id": "https://openalex.org/W1"}"#;
        let http = Recorded::new().reply(doi, Reply::ok(doi, w1));
        let (last, access) = (LastResolved::default(), Access::default());
        let cites = last.of("cites:10.5194/tc-1", &http, &access).unwrap();
        assert_eq!(cites.filters, ["cites:W1"]);
        let again = last.of("cites:10.5194/tc-1", &http, &access).unwrap();
        assert_eq!((again, http.requests().len()), (cites, 1), "kept");
        let other = last.of("cites:10.5194/tc-2", &http, &access);
        assert!(other.is_err(), "another query looked up again");
    }
}
