//! The sweeps: the new works of a journal in Crossref (indexed since the last
//! sweep, created within the lookback before it, which leaves out the old
//! works that Crossref indexes again) and of a filter in OpenAlex (published
//! within the lookback before the last sweep: those it lists again are the
//! items they were), page by page, as the entries of a subscription.

use chrono::{NaiveDate, TimeDelta};

use crate::fetch::Got;
use crate::http::{Access, Http, get_json, redact};
use crate::model::Subscription;
use crate::store::Validators;
use crate::{crossref, openalex};

/// The most pages of one sweep.
pub const SWEEP_PAGES: usize = 10;

/// Crossref's works of a journal indexed since a day and created within the
/// lookback before it, with the journal's title; as looked up.
pub(crate) fn crossref(
    http: &dyn Http,
    access: &Access,
    subscription: &Subscription,
    issn: &str,
    since: NaiveDate,
    lookback: TimeDelta,
) -> Result<Got, String> {
    let kind = subscription.settings.kind.item_kind();
    let (mut entries, mut title) = (Vec::new(), None);
    let mut cursor = "*".to_string();
    for _ in 0..SWEEP_PAGES {
        let url = crossref::list_url(issn, since, since - lookback, &cursor, access);
        let page: crossref::Message<crossref::List> =
            get_json(http, &url)?.ok_or_else(|| redact(&format!("{url}: not found"), &url))?;
        let page = page.message;
        if title.is_none() {
            title = (page.items.iter()).find_map(|w| w.container_title.first().cloned());
        }
        let full = page.items.len() == crossref::ROWS;
        entries.extend((page.items.iter()).filter_map(|w| w.entry(&subscription.id, kind)));
        match page.next_cursor {
            Some(next) if full => cursor = next,
            _ => break,
        }
    }
    Ok(Got::Entries {
        entries,
        title,
        validators: Validators::default(),
        looked_up: true,
    })
}

/// OpenAlex's articles of a filter published since a day; with its key, if
/// set (without, OpenAlex answers a small daily budget).
pub(crate) fn openalex(
    http: &dyn Http,
    access: &Access,
    subscription: &Subscription,
    filter: &str,
    published: NaiveDate,
) -> Result<Got, String> {
    let kind = subscription.settings.kind.item_kind();
    let mut entries = Vec::new();
    let mut cursor = "*".to_string();
    for _ in 0..SWEEP_PAGES {
        let url = openalex::list_url(filter, published, &cursor, access);
        let page: openalex::List =
            get_json(http, &url)?.ok_or_else(|| redact(&format!("{url}: not found"), &url))?;
        entries.extend(page.results.iter().map(|w| w.entry(&subscription.id, kind)));
        match page.meta.next_cursor {
            Some(next) if !page.results.is_empty() => cursor = next,
            _ => break,
        }
    }
    Ok(Got::Entries {
        entries,
        title: None,
        validators: Validators::default(),
        looked_up: false,
    })
}
