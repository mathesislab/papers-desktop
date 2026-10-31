//! The items: the list, one in full, the papers behind a post, the triage, the prune.

use papers_feeds::query::{kind_name, live, triage_name};
use papers_feeds::{Confidence, Feeds, Item, Row, Sort, Triage, Via, ViewRequest};
use serde_json::{Value, json};

use crate::cli::{FindArgs, ListArgs, MarkArgs, ShowArgs};
use crate::searches::{saved, with_saved};
use crate::text::{day, newest, source_label, table, wrap};
use crate::{Ctx, Error, Result, fail};

/// The mark of a state in the list.
fn marker(triage: Triage) -> &'static str {
    match triage {
        Triage::New => " ",
        Triage::Seen => ".",
        Triage::Shortlisted => "*",
        Triage::Dismissed => "x",
        Triage::Added => "+",
    }
}

/// An item as JSON: its fields, its state and its scores.
fn item_json(feeds: &Feeds, item: &Item) -> Value {
    let mut json = serde_json::to_value(item).expect("json");
    let object = json.as_object_mut().expect("an item is an object");
    object.insert("triage".into(), json!(triage_name(feeds.triage(&item.id))));
    object.insert("scores".into(), json!(feeds.scores(item)));
    object.insert(
        "best_score".into(),
        json!(papers_feeds::query::best(&feeds.scores(item))),
    );
    json
}

/// A row of the list as JSON: the item without its sources and full text, with its scores.
fn row_json(feeds: &Feeds, row: &Row, item: &Item) -> Value {
    json!({
        "id": item.id, "kind": kind_name(item.kind), "triage": triage_name(feeds.triage(&item.id)),
        "score": row.score, "interests": row.interests, "like": row.like, "date": item.date,
        "title": item.title,
        "authors": item.authors, "journal": item.journal, "doi": item.doi, "arxiv": item.arxiv,
        "url": item.url, "editorial": item.editorial, "summary": item.summary,
    })
}

/// `feeds list`: the items a query (and a saved search) passes, one line each, or as JSON.
pub fn list(ctx: &mut Ctx, a: ListArgs) -> Result<()> {
    let feeds = ctx.open()?;
    let search = a
        .saved
        .as_deref()
        .map(|name| saved(&feeds, name))
        .transpose()?;
    let mut query = with_saved(search, &a.query.join(" "));
    if !a.dismissed {
        query = live(&query);
    }
    let kinds: Vec<_> = a.kind.iter().map(|&k| k.into()).collect();
    let sort = a
        .sort
        .map_or(search.map_or(Sort::Newest, |s| s.sort), Sort::from);
    if sort == Sort::Like && ctx.library.is_none() {
        return fail("sorting by like needs the library: --library <file.bib>");
    }
    let request = ViewRequest {
        query: &query,
        sort,
        kinds: &kinds,
        ..ViewRequest::default()
    };
    let view = feeds.view(&request);
    let shown = a.limit.map_or(view.rows.len(), |n| n.min(view.rows.len()));
    let rows: Vec<(&Row, &Item)> = view.rows[..shown]
        .iter()
        .filter_map(|r| feeds.item(&r.id).map(|i| (r, i)))
        .collect();
    if a.json {
        let json: Vec<Value> = rows
            .iter()
            .map(|(row, item)| row_json(&feeds, row, item))
            .collect();
        writeln!(
            ctx.out,
            "{}",
            serde_json::to_string_pretty(&json).expect("json")
        )?;
    } else {
        let line = |(row, item): &(&Row, &Item)| {
            let journal = item
                .journal
                .as_deref()
                .map_or(String::new(), |j| format!(" — {j}"));
            // the likeness after the score, with the library
            let mut line = vec![
                item.id.clone(),
                marker(feeds.triage(&item.id)).to_string(),
                format!("{:>4.1}", row.score),
            ];
            line.extend(row.like.map(|like| format!("{like:>3.0}")));
            line.extend([
                kind_name(item.kind).to_string(),
                day(newest(item)),
                format!("{}{journal}", item.title),
            ]);
            line
        };
        let lines: Vec<Vec<String>> = rows.iter().map(line).collect();
        write!(ctx.out, "{}", table(&lines))?;
    }
    if shown < view.rows.len() {
        writeln!(
            ctx.err,
            "feeds: showing {shown} of {} items",
            view.rows.len()
        )?;
    }
    Ok(())
}

/// An item by its id, an alias, or the error.
fn find_item<'a>(feeds: &'a Feeds, id: &str) -> Result<&'a Item> {
    feeds
        .item(id)
        .ok_or_else(|| Error::Feeds(papers_feeds::Error::NoItem { id: id.to_string() }))
}

/// `feeds show`: all that is known of an item.
pub fn show(ctx: &mut Ctx, a: ShowArgs) -> Result<()> {
    let feeds = ctx.open()?;
    let item = find_item(&feeds, &a.id)?;
    let covered: Vec<&Item> = feeds.covered_by(&item.id);
    let like = feeds.like(item);
    // the nearest papers of the library: cosine, key, title
    let nearest: Vec<(f64, &str, &str)> = match (&like, &ctx.library) {
        (Some(like), Some((library, _))) => (like.nearest.iter())
            .filter_map(|(id, cosine)| library.get(id).map(|e| (*cosine, &*e.key, &*e.title)))
            .collect(),
        _ => Vec::new(),
    };
    if a.json {
        let mut json = item_json(&feeds, item);
        if let Some(like) = &like {
            let near = |&(cosine, key, title): &(f64, &str, &str)| json!({"key": key, "title": title, "cosine": cosine});
            let nearest: Vec<Value> = nearest.iter().map(near).collect();
            json["like"] = json!({"score": like.score, "nearest": nearest});
        }
        let by = |i: &&Item| json!({"id": i.id, "title": i.title, "kind": kind_name(i.kind)});
        json["covered_by"] = covered.iter().map(by).collect();
        let at = feeds.mark(&item.id).map(|m| m.at);
        json["triage_at"] = json!(at);
        writeln!(
            ctx.out,
            "{}",
            serde_json::to_string_pretty(&json).expect("json")
        )?;
        return Ok(());
    }
    let editorial = if item.editorial { ", editorial" } else { "" };
    let since = feeds
        .mark(&item.id)
        .map_or(String::new(), |m| format!(" (since {})", day(m.at)));
    let fields = [
        ("authors", item.authors.join(", ")),
        ("journal", item.journal.clone().unwrap_or_default()),
        ("date", item.date.map(day).unwrap_or_default()),
        ("kind", format!("{}{editorial}", kind_name(item.kind))),
        (
            "state",
            format!("{}{since}", triage_name(feeds.triage(&item.id))),
        ),
        ("id", item.id.clone()),
        (
            "like",
            like.as_ref()
                .map_or(String::new(), |l| format!("{:.0}", l.score)),
        ),
        ("doi", item.doi.clone().unwrap_or_default()),
        ("arxiv", item.arxiv.clone().unwrap_or_default()),
        ("url", item.url.clone().unwrap_or_default()),
    ];
    let mut lines: Vec<String> = vec![item.title.clone()];
    let shown = fields.iter().filter(|(_, value)| !value.is_empty());
    lines.extend(shown.map(|(name, value)| format!("{name:<9}{value}")));
    let mut section = |name: &str, entries: Vec<String>| {
        if !entries.is_empty() {
            lines.push(format!("{name}:"));
            lines.extend(entries.into_iter().map(|e| format!("  {e}")));
        }
    };
    let scores = feeds.scores(item);
    let interests = scores
        .iter()
        .filter(|s| s.score > 0.0 || s.excluded)
        .map(|s| {
            let status = match (s.excluded, s.reached) {
                (true, _) => "excluded".to_string(),
                (_, true) => format!("{:.1} (reached) {}", s.score, s.terms.join(", ")),
                _ => format!("{:.1} {}", s.score, s.terms.join(", ")),
            };
            format!("{}: {status}", s.interest)
        });
    section("interests", interests.collect());
    let versions = item
        .versions
        .iter()
        .map(|v| format!("{} ({:?})", v.doi, v.stage).to_lowercase());
    section("versions", versions.collect());
    let links = item.links.iter().map(|l| {
        let known = feeds
            .items()
            .iter()
            .find(|i| i.versions.iter().any(|v| v.doi == l.doi));
        let title = known.map_or(String::new(), |i| format!("  {} [{}]", i.title, i.id));
        let (confidence, via) = (confidence_name(l.confidence), via_name(l.via));
        format!("{}  {confidence}, via {via}{title}", l.doi)
    });
    section("papers", links.collect());
    let by = covered.iter().map(|i| format!("{} [{}]", i.title, i.id));
    section("covered by", by.collect());
    let near = (nearest.iter()).map(|(cosine, key, title)| format!("{cosine:.2}  {title} [{key}]"));
    section("like in the library", near.collect());
    let sources = item.sources.iter().map(|s| {
        let listed = if s.listed { "listed" } else { "not listed" };
        format!(
            "{}  first seen {}, {listed}",
            source_label(&feeds, &s.entry.source),
            day(s.first_seen)
        )
    });
    section("sources", sources.collect());
    if let Some(summary) = &item.summary {
        lines.push("abstract:".to_string());
        lines.extend(wrap(summary, 76).into_iter().map(|l| format!("  {l}")));
    }
    for line in lines {
        writeln!(ctx.out, "{line}")?;
    }
    Ok(())
}

/// How sure a linked paper is, as a word.
fn confidence_name(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::High => "high",
        Confidence::Low => "low",
    }
}

/// Where a linked paper was found, as a word.
fn via_name(via: Via) -> &'static str {
    match via {
        Via::Feed => "feed",
        Via::Page => "page",
        Via::Search => "search",
    }
}

/// `feeds find`: looks again for the papers behind a post.
pub fn find(ctx: &mut Ctx, a: FindArgs) -> Result<()> {
    let access = ctx.access(&a.access);
    let mut feeds = ctx.open()?;
    let id = find_item(&feeds, &a.id)?.id.clone();
    let found = ctx.with_http(&access, |http| feeds.find_papers(&id, http, &access))??;
    if let Some(page) = &found.page {
        writeln!(
            ctx.err,
            "feeds: warning: the page could not be read: {page}"
        )?;
    }
    if found.links.is_empty() {
        writeln!(ctx.out, "no paper found")?;
    }
    let rows: Vec<Vec<String>> = found
        .links
        .iter()
        .map(|l| {
            let known = feeds
                .items()
                .iter()
                .find(|i| i.versions.iter().any(|v| v.doi == l.doi));
            let title = known.map_or(String::new(), |i| format!("{} [{}]", i.title, i.id));
            let how = format!("{}, via {}", confidence_name(l.confidence), via_name(l.via));
            vec![l.doi.clone(), how, title]
        })
        .collect();
    write!(ctx.out, "{}", table(&rows))?;
    Ok(())
}

/// `feeds mark`: sets the state of items; none changes if one is unknown.
pub fn mark(ctx: &mut Ctx, a: MarkArgs) -> Result<()> {
    let mut feeds = ctx.open()?;
    let triage = Triage::from(a.state);
    let ids: Vec<String> = a
        .ids
        .iter()
        .map(|id| find_item(&feeds, id).map(|i| i.id.clone()))
        .collect::<Result<_>>()?;
    for id in &ids {
        feeds.set_triage(id, triage)?;
        let title = find_item(&feeds, id)?.title.clone();
        writeln!(ctx.out, "{} {id} {title}", triage_name(triage))?;
    }
    Ok(())
}

/// `feeds prune`: the folder is pruned on opening; what is kept is reported.
pub fn prune(ctx: &mut Ctx) -> Result<()> {
    let mut feeds = ctx.open()?;
    feeds.prune()?;
    let (items, days) = (feeds.items().len(), feeds.keep_days());
    writeln!(
        ctx.out,
        "{items} items kept (those no feed lists, first seen over {days} days ago, are dropped; shortlisted and added stay)"
    )?;
    Ok(())
}
