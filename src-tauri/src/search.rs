//! The Search tab: the commands of `src/lib/search/api.ts` over OpenAlex's
//! works (crates/feeds' `search_works`) and the folder of the works found,
//! `app_data_dir/search` (a `FeedsState` of its own, `Store::Search`). A
//! search's first page takes the place of the works found before, but for
//! those saved for later (shortlisted) or added; the next page adds to it.
//! The rows are the Feeds tab's (`FeedRow`); the details, the triage, the
//! abstracts and the PDFs are the Feeds tab's commands, with the store.

use papers_feeds::{Feed, Order, SourceKind, follow_filter, search_works};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::feeds::{
    Applied, Change, FeedRow, InLibrary, Rows, Store, SubscriptionInfo, change_in, network,
    subscription_of,
};
use crate::fetch::blocking;
use crate::settings::Settings;

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

/// The rows of items of the works found, in their order, the query's words
/// marked; those gone are left out.
fn rows_of(app: &AppHandle, ids: &[String], query: &str) -> Result<Vec<FeedRow>, String> {
    let state = app.state::<AppState>();
    let matching = *state.matching.read().unwrap();
    state.search.read(|feeds| {
        let library = state.library.read().unwrap();
        let rows = Rows::new(feeds, InLibrary::of(&library));
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
        let (network, access) = network(&app.state::<AppState>())?;
        let found = search_works(&network, &access, &query, order, cursor.as_deref())?;
        let fresh = cursor.is_none();
        let ids = match change_in(&app, Store::Search, Change::Found(found.entries, fresh))? {
            Applied::Ids(ids) => ids,
            _ => Vec::new(),
        };
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
        let (network, access) = network(&app.state::<AppState>())?;
        let filter = follow_filter(&network, &access, &query)?;
        let feed = Feed::OpenAlex { filter };
        let title = Some(query.trim().to_string());
        let applied = change_in(
            &app,
            Store::Feeds,
            Change::Subscribe(feed, SourceKind::Journal, title),
        )?;
        subscription_of(&app, applied)
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
