//! Subscribing, unsubscribing, the table of subscriptions, and the fetch.

use papers_feeds::{Feed, Outcome, SourceKind};

use crate::cli::{AccessArgs, EditArgs, IdArg, JsonArg, SubscribeArgs};
use crate::text::{label, table};
use crate::{Ctx, Result, fail};
/// The kind of a feed's items when none is said: preprints for arXiv and the preprint servers,
/// else journal.
pub fn guess_kind(feed: &Feed) -> SourceKind {
    let host = match feed {
        Feed::Rss { url } => url.split('/').nth(2).unwrap_or_default().to_lowercase(),
        _ => String::new(),
    };
    let preprints = ["arxiv", "rxiv", "egusphere", "essopenarchive"];
    match preprints.iter().any(|p| host.contains(p)) {
        true => SourceKind::Preprints,
        false => SourceKind::Journal,
    }
}

/// `feeds subscribe`: follows a feed or a sweep.
pub fn subscribe(ctx: &mut Ctx, a: SubscribeArgs) -> Result<()> {
    let feed = match (a.url, a.issn, a.openalex) {
        (Some(url), None, None) => Feed::Rss { url },
        (None, Some(issn), None) => Feed::Crossref { issn },
        (None, None, Some(filter)) => Feed::OpenAlex { filter },
        _ => return fail("give one of a URL, --issn or --openalex"),
    };
    let mut feeds = ctx.open()?;
    let kind = a.kind.map_or_else(|| guess_kind(&feed), SourceKind::from);
    let mut subscription = feeds.subscribe(feed, kind)?;
    if a.title.is_some() || a.journal.is_some() || a.fetch_pages || a.boost.is_some() {
        subscription = feeds.edit_subscription(&subscription.id, |s| {
            s.title = a.title.or(s.title.take());
            s.journal = a.journal.or(s.journal.take());
            s.fetch_pages |= a.fetch_pages;
            s.boost = a.boost.unwrap_or(s.boost);
        })?;
    }
    let (id, feed) = (&subscription.id, &subscription.feed);
    writeln!(
        ctx.out,
        "subscribed {id} ({}) {feed}",
        kind_name(subscription.kind)
    )?;
    Ok(())
}

/// `feeds edit`: sets what the options say.
pub fn edit(ctx: &mut Ctx, a: EditArgs) -> Result<()> {
    let mut feeds = ctx.open()?;
    let s = feeds.edit_subscription(&a.id, |s| s.boost = a.boost)?;
    writeln!(ctx.out, "edited {} {}: boost {}", s.id, s.feed, s.boost)?;
    Ok(())
}

/// The name of a subscription's kind, as `--kind` takes it.
pub fn kind_name(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Journal => "journal",
        SourceKind::Preprints => "preprints",
        SourceKind::News => "news",
        SourceKind::Blog => "blog",
    }
}

/// `feeds unsubscribe`: ends a subscription.
pub fn unsubscribe(ctx: &mut Ctx, a: IdArg) -> Result<()> {
    let mut feeds = ctx.open()?;
    let subscription = feeds.unsubscribe(&a.id)?;
    writeln!(
        ctx.out,
        "unsubscribed {} {}",
        subscription.id, subscription.feed
    )?;
    Ok(())
}

/// `feeds subscriptions`: the table, or the JSON of the subscriptions.
pub fn subscriptions(ctx: &mut Ctx, a: JsonArg) -> Result<()> {
    let feeds = ctx.open()?;
    if a.json {
        writeln!(
            ctx.out,
            "{}",
            serde_json::to_string_pretty(feeds.subscriptions()).expect("json")
        )?;
        return Ok(());
    }
    // the column of boosts, if any subscription has one
    let boosted = feeds.subscriptions().iter().any(|s| s.boost != 0.0);
    let row = |s: &papers_feeds::Subscription| {
        let name = s
            .title
            .clone()
            .or_else(|| s.journal.clone())
            .unwrap_or_default();
        let mut row = vec![s.id.clone(), kind_name(s.kind).to_string(), name];
        if boosted {
            row.push(match s.boost {
                0.0 => String::new(),
                boost => format!("boost {boost:+}"),
            });
        }
        row.push(s.feed.to_string());
        row
    };
    let rows: Vec<Vec<String>> = feeds.subscriptions().iter().map(row).collect();
    write!(ctx.out, "{}", table(&rows))?;
    Ok(())
}

/// `feeds fetch`: fetches everything and reports each subscription.
pub fn fetch(ctx: &mut Ctx, a: AccessArgs) -> Result<()> {
    let access = ctx.access(&a);
    let mut feeds = ctx.open()?;
    let report = ctx.with_http(&access, |http| feeds.fetch(http, &access))??;
    let mut total = 0;
    let mut rows = Vec::new();
    for r in &report.feeds {
        let found = feeds
            .subscriptions()
            .iter()
            .find(|s| s.id == r.subscription);
        let name = found.map_or_else(|| r.subscription.clone(), label);
        rows.push(match &r.outcome {
            Outcome::Fetched { entries, new } => {
                total += new;
                vec![format!("{new} new"), format!("{name} ({entries} entries)")]
            }
            Outcome::Unchanged => vec!["unchanged".to_string(), name],
            Outcome::Failed(message) => vec!["failed".to_string(), format!("{name}: {message}")],
        });
    }
    write!(ctx.out, "{}", table(&rows))?;
    let (lookups, pages) = (report.lookups, report.pages);
    writeln!(ctx.out, "new: {total}; lookups: {lookups}; pages: {pages}")?;
    Ok(())
}
