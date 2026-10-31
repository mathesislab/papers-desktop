//! The sweeps: the new works of a journal in Crossref (indexed since the last
//! sweep, created within the lookback before it, which leaves out the old
//! works that Crossref indexes again) and of a filter in OpenAlex (created
//! since the last sweep), page by page, as the entries of a subscription.

use chrono::{NaiveDate, TimeDelta};

use crate::feeds::{Feeds, Outcome};
use crate::http::{Access, Http, get_json};
use crate::model::{Entry, Feed, Subscription};
use crate::{crossref, openalex};

/// The most pages of one sweep.
pub const SWEEP_PAGES: usize = 10;

/// What a sweep got.
struct Swept {
    entries: Vec<Entry>,
    /// the journal's title (Crossref's container title)
    title: Option<String>,
    /// whether its entries have all a lookup by DOI would give (Crossref's)
    complete: bool,
}

impl Feeds {
    /// Fetches a subscription: a feed by a conditional GET; a sweep of
    /// Crossref or OpenAlex for the works since its last (since the lookback
    /// the first time), merged; a sweep that fails is asked for again whole.
    pub(crate) fn fetch_subscription(
        &mut self,
        i: usize,
        http: &dyn Http,
        access: &Access,
    ) -> Outcome {
        let subscription = &self.config.subscriptions[i];
        let id = subscription.id.clone();
        let mut validators = self.http.get(&id).cloned().unwrap_or_default();
        let now = (self.now)();
        let lookback = TimeDelta::days(self.config.lookback_days.into());
        let since = validators.swept.unwrap_or(now - lookback).date_naive();
        let swept = match &subscription.feed {
            Feed::Rss { url } => {
                let url = url.clone();
                return self.fetch_feed(i, &url, http);
            }
            Feed::Crossref { issn } => crossref(http, access, subscription, issn, since, lookback),
            Feed::OpenAlex { filter } => openalex(http, access, subscription, filter, since),
        };
        let swept = match swept {
            Ok(swept) => swept,
            Err(message) => return Outcome::Failed(message),
        };
        if swept.complete {
            for doi in swept.entries.iter().filter_map(|e| e.doi.as_ref()) {
                self.lookups.insert(format!("crossref:{doi}"), now);
            }
        }
        let entries = swept.entries.len();
        let new = self.merge(&id, swept.entries);
        let subscription = &mut self.config.subscriptions[i];
        if subscription.title.is_none() {
            subscription.title = swept.title;
        }
        validators.swept = Some(now);
        self.http.insert(id, validators);
        Outcome::Fetched { entries, new }
    }
}

/// Crossref's works of a journal indexed since a day and created within the
/// lookback before it.
fn crossref(
    http: &dyn Http,
    access: &Access,
    subscription: &Subscription,
    issn: &str,
    since: NaiveDate,
    lookback: TimeDelta,
) -> Result<Swept, String> {
    let kind = subscription.kind.item_kind();
    let mut swept = Swept {
        entries: Vec::new(),
        title: None,
        complete: true,
    };
    let mut cursor = "*".to_string();
    for _ in 0..SWEEP_PAGES {
        let url = crossref::list_url(issn, since, since - lookback, &cursor, access);
        let page: crossref::Message<crossref::List> =
            get_json(http, &url)?.ok_or_else(|| format!("{url}: not found"))?;
        let page = page.message;
        if swept.title.is_none() {
            swept.title = page
                .items
                .iter()
                .find_map(|w| w.container_title.first().cloned());
        }
        let full = page.items.len() == crossref::ROWS;
        let entries = page
            .items
            .iter()
            .filter_map(|w| w.entry(&subscription.id, kind));
        swept.entries.extend(entries);
        match page.next_cursor {
            Some(next) if full => cursor = next,
            _ => break,
        }
    }
    Ok(swept)
}

/// OpenAlex's articles of a filter created since a day; its key is needed.
fn openalex(
    http: &dyn Http,
    access: &Access,
    subscription: &Subscription,
    filter: &str,
    since: NaiveDate,
) -> Result<Swept, String> {
    if access.openalex_key.is_none() {
        return Err("OpenAlex: no key".to_string());
    }
    let kind = subscription.kind.item_kind();
    let mut swept = Swept {
        entries: Vec::new(),
        title: None,
        complete: false,
    };
    let mut cursor = "*".to_string();
    for _ in 0..SWEEP_PAGES {
        let url = openalex::list_url(filter, since, &cursor, access);
        let page: openalex::List =
            get_json(http, &url)?.ok_or_else(|| format!("{url}: not found"))?;
        let entries = page.results.iter().map(|w| w.entry(&subscription.id, kind));
        swept.entries.extend(entries);
        match page.meta.next_cursor {
            Some(next) if !page.results.is_empty() => cursor = next,
            _ => break,
        }
    }
    Ok(swept)
}
