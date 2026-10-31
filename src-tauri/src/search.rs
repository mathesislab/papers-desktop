//! The Search tab: the commands of `src/lib/search/api.ts` over OpenAlex's
//! works (crates/feeds' `search_works`) and the folder of the works found,
//! `app_data_dir/search` (a `FeedsState` of its own, `Store::Search`). A
//! search's first page takes the place of the works found before, but for
//! those saved for later (shortlisted) or added; the next page adds to it.
//! The rows are the Feeds tab's (`FeedRow`); the details, the triage, the
//! abstracts and the PDFs are the Feeds tab's commands, with the store.

use papers_feeds::{Order, search_works};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::feeds::{Applied, Change, FeedRow, InLibrary, Rows, Store, change_in, network};
use crate::fetch::blocking;

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
