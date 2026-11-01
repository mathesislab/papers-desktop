//! Subscribing, unsubscribing, the table of subscriptions, and the fetch.

use papers_feeds::{Feed, Outcome, SourceKind, SubscriptionSettings, preprint_host};

use crate::cli::{AccessArgs, EditArgs, IdArg, JsonArg, OpmlArgs, OpmlCommand, SubscribeArgs};
use crate::text::{label, table};
use crate::{Ctx, Error, Result, fail};
/// The kind of a feed's items when none is said: preprints for arXiv and the preprint servers,
/// else journal.
fn guess_kind(feed: &Feed) -> SourceKind {
    match feed {
        Feed::Rss { url } if preprint_host(url) => SourceKind::Preprints,
        _ => SourceKind::Journal,
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
    let settings = SubscriptionSettings {
        title: a.title,
        kind: a.kind.unwrap_or_else(|| guess_kind(&feed)),
        journal: a.journal,
        fetch_pages: a.fetch_pages,
        boost: a.boost.unwrap_or(0.0),
    };
    let subscription = feeds.subscribe(feed, settings)?;
    let (id, feed) = (&subscription.id, &subscription.feed);
    writeln!(
        ctx.out,
        "subscribed {id} ({}) {feed}",
        subscription.settings.kind
    )?;
    Ok(())
}

/// `feeds edit`: sets what the options say.
pub fn edit(ctx: &mut Ctx, a: EditArgs) -> Result<()> {
    let mut feeds = ctx.open()?;
    let settings = SubscriptionSettings {
        boost: a.boost,
        ..feeds.subscription(&a.id)?.settings.clone()
    };
    let s = feeds.edit_subscription(&a.id, settings)?;
    let boost = s.settings.boost;
    writeln!(ctx.out, "edited {} {}: boost {boost}", s.id, s.feed)?;
    Ok(())
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
    let boosted = (feeds.subscriptions().iter()).any(|s| s.settings.boost != 0.0);
    let row = |s: &papers_feeds::Subscription| {
        let settings = &s.settings;
        let name = (settings.title.clone())
            .or_else(|| settings.journal.clone())
            .unwrap_or_default();
        let mut row = vec![s.id.clone(), settings.kind.to_string(), name];
        if boosted {
            row.push(match settings.boost {
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

/// `feeds opml`: follows what a file lists, or writes the subscriptions.
pub fn opml(ctx: &mut Ctx, a: OpmlArgs) -> Result<()> {
    let file_error = |path: &std::path::Path| {
        let path = path.to_path_buf();
        move |source| Error::File { path, source }
    };
    match a.command {
        OpmlCommand::Import { file } => {
            let text = std::fs::read_to_string(&file).map_err(file_error(&file))?;
            let added = ctx.open()?.import_opml(&text)?;
            writeln!(ctx.out, "subscribed to {added} new")?;
        }
        OpmlCommand::Export { file } => {
            let feeds = ctx.open()?;
            match file {
                Some(file) => {
                    std::fs::write(&file, feeds.opml()).map_err(file_error(&file))?;
                    let n = feeds.subscriptions().len();
                    writeln!(ctx.out, "exported {n} to {}", file.display())?;
                }
                None => write!(ctx.out, "{}", feeds.opml())?,
            }
        }
    }
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
    let (lookups, pages) = (report.follow_up.lookups, report.follow_up.pages);
    writeln!(ctx.out, "new: {total}; lookups: {lookups}; pages: {pages}")?;
    Ok(())
}
