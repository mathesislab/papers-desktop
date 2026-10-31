//! Plain text for the terminal: days, tables, wrapped paragraphs.

use chrono::{DateTime, Utc};
use papers_feeds::{Feeds, Item, Subscription};

/// A day as `2026-10-31`.
pub fn day(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%d").to_string()
}

/// What an item is of in time: its date, else when it was first fetched.
pub fn newest(item: &Item) -> DateTime<Utc> {
    item.date.unwrap_or_else(|| item.first_seen())
}

/// Rows in columns, each padded to its widest cell; the last column is not.
pub fn table(rows: &[Vec<String>]) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let width = |c: usize| {
        rows.iter()
            .filter_map(|r| r.get(c))
            .map(|s| s.chars().count())
            .max()
    };
    let widths: Vec<usize> = (0..columns).map(|c| width(c).unwrap_or(0)).collect();
    let mut out = String::new();
    for row in rows {
        let last = row.len().saturating_sub(1);
        let cells = row.iter().enumerate().map(|(c, cell)| match c == last {
            true => cell.clone(),
            false => format!("{cell:<w$}", w = widths[c]),
        });
        out.push_str(cells.collect::<Vec<_>>().join("  ").trim_end());
        out.push('\n');
    }
    out
}

/// A text in lines of at most `width` characters (a longer word has its own line); each
/// line of the text starts a paragraph.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let longer = line.chars().count() + 1 + word.chars().count() > width;
            if !line.is_empty() && longer {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}

/// What a subscription is called: its title, else its feed.
pub fn label(subscription: &Subscription) -> String {
    let named = subscription
        .title
        .clone()
        .or_else(|| subscription.journal.clone());
    named.unwrap_or_else(|| subscription.feed.to_string())
}

/// What a source of an entry is called: a subscription's label, the lookup, or its id.
pub fn source_label(feeds: &Feeds, id: &str) -> String {
    match feeds.subscriptions().iter().find(|s| s.id == id) {
        Some(subscription) => label(subscription),
        None => id.to_string(),
    }
}
