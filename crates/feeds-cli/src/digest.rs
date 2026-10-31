//! The digest: the items of the last days that reach an interest, as paper-feeder's RSS feed
//! (guid the DOI else the link, `dc:creator` the authors, category the journal, a lead line
//! "journal — matched: terms" before the abstract) and its HTML page.

use std::fmt::Write as _;
use std::path::Path;

use chrono::{DateTime, TimeDelta, Utc};
use papers_feeds::{Item, ItemKind, Row, Sort, ViewRequest};

use crate::cli::DigestArgs;
use crate::text::{authors, day, link, newest};
use crate::{Ctx, Error, Result};

/// The stylesheet of paper-feeder's page.
const STYLE: &str = include_str!("digest.css");

/// A digest item: the item, and the terms it matched in its interests.
struct Entry<'a> {
    item: &'a Item,
    score: f64,
    matched: Vec<String>,
}

/// A text as XML or HTML escapes it, without the characters XML 1.0 does not allow.
fn escape(text: &str) -> String {
    let allowed = |c: &char| !c.is_control() || matches!(c, '\t' | '\n' | '\r');
    let mut out = String::with_capacity(text.len());
    for c in text.chars().filter(allowed) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// The lead line of an item: its journal, then what it matched.
fn lead(entry: &Entry) -> String {
    let matched = match entry.matched.is_empty() {
        true => String::new(),
        false => format!("matched: {}", entry.matched.join(", ")),
    };
    let parts = [entry.item.journal.clone().unwrap_or_default(), matched];
    parts
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" — ")
}

/// The RSS 2.0 feed of the entries.
fn rss(entries: &[Entry], now: DateTime<Utc>, title: &str, link_to: &str) -> String {
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str("<rss version=\"2.0\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><channel>");
    let _ = write!(
        x,
        "<title>{}</title><link>{}</link>",
        escape(title),
        escape(link_to)
    );
    let _ = write!(
        x,
        "<description>Keyword-scored digest of new papers.</description>"
    );
    let _ = write!(x, "<lastBuildDate>{}</lastBuildDate>", now.to_rfc2822());
    for entry in entries {
        let item = entry.item;
        let address = link(item);
        let title = match item.title.is_empty() {
            true => "(untitled)",
            false => &item.title,
        };
        let _ = write!(x, "<item><title>{}</title>", escape(title));
        if let Some(address) = &address {
            let _ = write!(x, "<link>{}</link>", escape(address));
        }
        let guid = item
            .doi
            .clone()
            .or(address)
            .unwrap_or_else(|| item.id.clone());
        let _ = write!(x, "<guid isPermaLink=\"false\">{}</guid>", escape(&guid));
        if let Some(date) = item.date {
            let _ = write!(x, "<pubDate>{}</pubDate>", date.to_rfc2822());
        }
        if !item.authors.is_empty() {
            let _ = write!(
                x,
                "<dc:creator>{}</dc:creator>",
                escape(&authors(&item.authors, 6))
            );
        }
        if let Some(journal) = &item.journal {
            let _ = write!(x, "<category>{}</category>", escape(journal));
        }
        let parts = [lead(entry), item.summary.clone().unwrap_or_default()];
        let parts: Vec<String> = parts.into_iter().filter(|p| !p.is_empty()).collect();
        let _ = write!(
            x,
            "<description>{}</description></item>",
            escape(&parts.join("\n\n"))
        );
    }
    x.push_str("</channel></rss>\n");
    x
}

/// One entry as an `<article>` of the page.
fn article(entry: &Entry, today: &str) -> String {
    let item = entry.item;
    let href = escape(&link(item).unwrap_or_else(|| "#".to_string()));
    let title = if item.title.is_empty() {
        "(untitled)"
    } else {
        &item.title
    };
    let new = item.first_seen().format("%Y-%m-%d").to_string() == today;
    let mut bits: Vec<String> = Vec::new();
    bits.extend(item.journal.iter().map(|j| escape(j)));
    bits.extend(item.date.map(day));
    if item.editorial {
        bits.push("<span class=\"editorial\">editorial</span>".to_string());
    }
    let mut meta = bits.join(" · ");
    if new {
        let badge = "<span class=\"new\">new</span>";
        meta = if meta.is_empty() {
            badge.to_string()
        } else {
            format!("{meta} {badge}")
        };
    }
    let mut a = String::from("<article>");
    let _ = write!(
        a,
        "<h2><a href=\"{href}\" target=\"_blank\" rel=\"noopener noreferrer\">{}</a></h2>",
        escape(title)
    );
    if !item.authors.is_empty() {
        let _ = write!(
            a,
            "<div class=\"meta\">{}</div>",
            escape(&authors(&item.authors, 6))
        );
    }
    let _ = write!(a, "<div class=\"meta\">{meta}</div>");
    let score =
        (entry.score > 0.0).then(|| format!("<span class=\"score\">{:.1}</span>", entry.score));
    let matched = (!entry.matched.is_empty())
        .then(|| format!("matched: {}", escape(&entry.matched.join(", "))));
    let why: Vec<String> = score.into_iter().chain(matched).collect();
    if !why.is_empty() {
        let _ = write!(a, "<div class=\"why\">{}</div>", why.join(" "));
    }
    match (&item.summary, item.kind) {
        (Some(summary), _) => {
            let _ = write!(
                a,
                "<details><summary>abstract</summary><p>{}</p></details>",
                escape(summary)
            );
        }
        (None, ItemKind::Paper | ItemKind::Preprint) => {
            a.push_str("<div class=\"section-note\">no abstract available</div>");
        }
        _ => {}
    }
    a.push_str("</article>");
    a
}

/// The HTML page of the entries; it links the RSS feed `feed`, if it is written too.
fn html(entries: &[Entry], now: DateTime<Utc>, title: &str, feed: Option<&str>) -> String {
    let today = day(now);
    let new = entries
        .iter()
        .filter(|e| day(e.item.first_seen()) == today)
        .count();
    let mut h = String::from("<!doctype html><html lang=en><head><meta charset=utf-8>");
    h.push_str("<meta name=viewport content=\"width=device-width, initial-scale=1\">");
    if let Some(feed) = feed {
        let _ = write!(
            h,
            "<link rel=\"alternate\" type=\"application/rss+xml\" title=\"{}\" href=\"{}\">",
            escape(title),
            escape(feed)
        );
    }
    let _ = write!(
        h,
        "<title>{}</title><style>{STYLE}</style></head><body><h1>{}</h1>",
        escape(title),
        escape(title)
    );
    let rss = feed.map_or(String::new(), |f| {
        format!(" · <a class=\"rss\" href=\"{}\">RSS feed</a>", escape(f))
    });
    let _ = write!(
        h,
        "<p class=\"meta\">{} papers ({new} new) · generated {today}{rss}</p>",
        entries.len()
    );
    if entries.is_empty() {
        h.push_str("<p class=\"section-note\">No matching papers in the window.</p>");
    }
    for entry in entries {
        h.push_str(&article(entry, &today));
    }
    h.push_str("</body></html>\n");
    h
}

/// A file written, its error naming it.
fn write_file(path: &Path, text: &str) -> Result<()> {
    std::fs::write(path, text).map_err(|source| Error::File {
        path: path.to_path_buf(),
        source,
    })
}

/// `feeds digest`: writes the RSS feed and the HTML page of the items that pass the query in
/// the last days, the best score first.
pub fn digest(ctx: &mut Ctx, a: DigestArgs) -> Result<()> {
    let feeds = ctx.open()?;
    let now = (ctx.host.now)();
    let query = format!("{} -is:dismissed", a.query);
    let view = feeds.view(&ViewRequest {
        query: &query,
        sort: Sort::Score,
        ..ViewRequest::default()
    });
    let since = now - TimeDelta::days(a.days.into());
    let rows: Vec<(&Row, &Item)> = view
        .rows
        .iter()
        .filter_map(|r| feeds.item(&r.id).map(|i| (r, i)))
        .collect();
    let entries: Vec<Entry> = rows
        .into_iter()
        .filter(|(_, item)| newest(item) >= since)
        .map(|(row, item)| {
            let mut matched: Vec<String> = Vec::new();
            for term in row.interests.iter().flat_map(|s| &s.terms) {
                if !matched.contains(term) {
                    matched.push(term.clone());
                }
            }
            Entry {
                item,
                score: row.score,
                matched,
            }
        })
        .collect();
    if let Some(path) = &a.rss {
        write_file(path, &rss(&entries, now, &a.title, &a.link))?;
    }
    if let Some(path) = &a.html {
        let feed = a
            .rss
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|n| n.to_str());
        write_file(path, &html(&entries, now, &a.title, feed))?;
    }
    let files: Vec<String> = [&a.rss, &a.html]
        .into_iter()
        .flatten()
        .map(|p| p.display().to_string())
        .collect();
    writeln!(ctx.out, "{} items in {}", entries.len(), files.join(", "))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_marks_and_drops_what_xml_does_not_allow() {
        assert_eq!(
            escape("a < b & c > \"d\" 'e'"),
            "a &lt; b &amp; c &gt; &quot;d&quot; &#39;e&#39;"
        );
        assert_eq!(
            escape("tab\tnl\ncr\rbell\u{7}nul\u{0}esc\u{1b}"),
            "tab\tnl\ncr\rbellnulesc"
        );
        assert_eq!(escape("Müller – α"), "Müller – α");
    }
}
