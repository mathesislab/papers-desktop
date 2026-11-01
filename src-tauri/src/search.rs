//! The Search tab: the commands of `src/lib/search/api.ts` over OpenAlex's
//! works (crates/feeds' `search_works`) and the folder of the works found,
//! `app_data_dir/search` (a `Folder` of its own, `Store::Search`). A
//! search's first page takes the place of the works found before, but for
//! those starred or added; the next page adds to it.
//! The last query's journals and works looked up are kept for its next pages
//! and its subscription. The rows are the Feeds tab's (`FeedRow`); the details,
//! the triage, the abstracts and the PDFs are the Feeds tab's commands, with
//! the store. A search online is the address of a service without an API
//! (crates/feeds' `online_url`), which the page opens in the browser.

use std::sync::Mutex;

use papers_feeds::{
    Access, Feed, Http, Order, Resolved, Service, SourceKind, SubscriptionSettings, ask,
    online_url, resolve, search_works, subscription_filter,
};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::feeds::{FeedRow, Rows, SubscriptionInfo};
use crate::fetch::blocking;
use crate::settings::Settings;

/// The items' event of the Search tab: the works found changed.
pub const CHANGED: &str = "search-changed";

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
    /// What a query asks of OpenAlex: the last one's, else resolved (the
    /// lock not held meanwhile) and kept.
    fn of(&self, query: &str, http: &dyn Http, access: &Access) -> Result<Resolved, String> {
        if let Some((asked, resolved)) = self.0.lock().unwrap().as_ref()
            && asked == query
        {
            return Ok(resolved.clone());
        }
        let resolved = resolve(http, access, &ask(query)?)?;
        *self.0.lock().unwrap() = Some((query.to_string(), resolved.clone()));
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
        let requests = state.requests()?;
        let resolved = state
            .resolved
            .of(&query, &requests.http, &requests.access)?;
        let found = search_works(
            &requests.http,
            &requests.access,
            &resolved,
            order,
            cursor.as_deref(),
        )?;
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

/// A search subscribed to in the Feeds tab: a sweep of OpenAlex of its filter (its words
/// searched in the titles and abstracts), titled as the query, its works papers. Fetched by the
/// caller.
#[tauri::command]
pub async fn search_subscribe(app: AppHandle, query: String) -> Result<SubscriptionInfo, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let requests = state.requests()?;
        let filter = subscription_filter(&state.resolved.of(
            &query,
            &requests.http,
            &requests.access,
        )?)?;
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

/// The works starred, the latest found first.
#[tauri::command]
pub async fn search_starred(app: AppHandle) -> Result<Vec<FeedRow>, String> {
    blocking(move || {
        let ids = app.state::<AppState>().search.read(|feeds| {
            let starred = feeds.starred().into_iter();
            starred.map(|i| i.id.clone()).collect::<Vec<_>>()
        })?;
        rows_of(&app, &ids, "")
    })
    .await
}

/// The services of *Search online*, by name: those built in, then the
/// user's of the settings.
#[tauri::command]
pub fn search_services(app: AppHandle) -> Result<Vec<String>, String> {
    let settings = Settings::load(&app.state::<AppState>().settings_path)?;
    let own = settings.search_services.into_iter().map(|s| s.name);
    Ok(Service::BUILT_IN
        .map(String::from)
        .into_iter()
        .chain(own)
        .collect())
}

/// The address of a search of a service online, by its name, for the
/// page to open; what the service cannot be asked, said.
#[tauri::command]
pub fn search_online(app: AppHandle, service: String, query: String) -> Result<String, String> {
    let settings = Settings::load(&app.state::<AppState>().settings_path)?;
    let named = Service::named(&service, &settings.search_services);
    online_url(&named.ok_or(format!("{service}: no such service"))?, &query)
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
