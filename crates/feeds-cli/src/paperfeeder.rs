//! `feeds import-paper-feeder`: a paper-feeder setup (`sources.yaml`, `scoring.yaml`) as
//! subscriptions, the lookback and one interest. What has no equivalent here is skipped and
//! reported: `always_include` and `score_boost` of a source, the `lookback_days` of a query.

use std::path::Path;

use papers_feeds::{
    Error as FeedsError, Feed, Feeds, Interest, Lexicon, Matcher, SourceKind, Weighted,
};
use serde::Deserialize;

use crate::cli::{ImportArgs, IssnVia};
use crate::subs::guess_kind;
use crate::{Ctx, Error, Result};

/// A YAML scalar that may be a number where a string is meant (an ISSN written bare).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Scalar {
    Text(String),
    Int(i64),
}

impl Scalar {
    /// The scalar as text.
    fn text(&self) -> String {
        match self {
            Scalar::Text(text) => text.clone(),
            Scalar::Int(int) => int.to_string(),
        }
    }
}

/// An `rss:` entry of `sources.yaml`.
#[derive(Debug, Deserialize)]
struct RssEntry {
    name: Option<String>,
    url: String,
    journal: Option<String>,
    always_include: Option<bool>,
    score_boost: Option<f64>,
}

/// A query of `openalex:`.
#[derive(Debug, Deserialize)]
struct Query {
    name: Option<String>,
    filter: String,
    always_include: Option<bool>,
    score_boost: Option<f64>,
    lookback_days: Option<u32>,
}

/// The `openalex:` section.
#[derive(Debug, Default, Deserialize)]
struct OpenAlexSources {
    queries: Option<Vec<Query>>,
    issns: Option<Vec<Scalar>>,
}

/// The `crossref:` section.
#[derive(Debug, Default, Deserialize)]
struct CrossrefSources {
    issns: Option<Vec<Scalar>>,
}

/// `sources.yaml`.
#[derive(Debug, Default, Deserialize)]
struct Sources {
    rss: Option<Vec<RssEntry>>,
    openalex: Option<OpenAlexSources>,
    crossref: Option<CrossrefSources>,
    lookback_days: Option<u32>,
    mailto: Option<String>,
    window_days: Option<serde_norway::Value>,
    retention_days: Option<serde_norway::Value>,
    render: Option<serde_norway::Value>,
}

/// An `include` entry of `scoring.yaml`: a pattern, else a term.
#[derive(Debug, Deserialize)]
struct Include {
    term: Option<String>,
    pattern: Option<String>,
    weight: Option<f64>,
    label: Option<String>,
}

/// An `exclude` entry: a term, or `{term}` or `{pattern}`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Exclude {
    Term(String),
    Entry {
        term: Option<String>,
        pattern: Option<String>,
    },
}

/// `scoring.yaml`.
#[derive(Debug, Default, Deserialize)]
struct Scoring {
    title_boost: Option<f64>,
    publish_min_score: Option<f64>,
    include: Option<Vec<Include>>,
    exclude: Option<Vec<Exclude>>,
    serendipity_count: Option<serde_norway::Value>,
    llm_rerank: Option<serde_norway::Value>,
}

/// A YAML file read; an empty one is the default.
fn read<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(|source| Error::File {
        path: path.to_path_buf(),
        source,
    })?;
    let parsed: Option<T> = serde_norway::from_str(&text)
        .map_err(|e| Error::Message(format!("{}: {e}", path.display())))?;
    Ok(parsed.unwrap_or_default())
}

/// What the sources hold that has no equivalent, by what: the names of the sources.
#[derive(Debug, Default)]
struct Skipped {
    always_include: Vec<String>,
    score_boost: Vec<String>,
    lookback_days: Vec<String>,
}

/// How the subscriptions went.
#[derive(Debug, Default)]
struct Subscribed {
    added: usize,
    already: usize,
    failed: Vec<String>,
}

impl Subscribed {
    /// Subscribes to a feed, naming it; one that fails or is there already is counted.
    fn add(
        &mut self,
        feeds: &mut Feeds,
        feed: Feed,
        kind: SourceKind,
        title: Option<String>,
        journal: Option<String>,
    ) -> Result<()> {
        let shown = feed.to_string();
        match feeds.subscribe(feed, kind) {
            Ok(subscription) => {
                self.added += 1;
                if title.is_some() || journal.is_some() {
                    feeds.edit_subscription(&subscription.id, |s| {
                        s.title = title;
                        s.journal = journal;
                    })?;
                }
            }
            Err(FeedsError::Subscribed { .. }) => self.already += 1,
            Err(
                e @ (FeedsError::Url { .. } | FeedsError::Issn { .. } | FeedsError::Filter { .. }),
            ) => {
                self.failed.push(format!("{shown}: {e}"));
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }
}

/// The ISSNs of a list as feeds, as `via` says.
fn issn_feeds(issns: &[Scalar], via: IssnVia) -> Vec<Feed> {
    let feed = |issn: String| match via {
        IssnVia::Crossref => Feed::Crossref { issn },
        IssnVia::Openalex => Feed::OpenAlex {
            filter: format!("primary_location.source.issn:{issn}"),
        },
    };
    issns.iter().map(|i| feed(i.text())).collect()
}

/// The subscriptions of the sources, in paper-feeder's order: rss, OpenAlex queries and ISSNs, Crossref ISSNs.
fn subscribe_all(
    feeds: &mut Feeds,
    sources: &Sources,
    via: IssnVia,
    skipped: &mut Skipped,
) -> Result<Subscribed> {
    let mut done = Subscribed::default();
    let mut skip = |kind: &str, entry: Option<bool>, boost: Option<f64>, lookback: Option<u32>| {
        let name = kind.to_string();
        if entry == Some(true) {
            skipped.always_include.push(name.clone());
        }
        if boost.is_some_and(|b| b != 0.0) {
            skipped
                .score_boost
                .push(format!("{name} ({})", boost.unwrap_or_default()));
        }
        if lookback.is_some() {
            skipped
                .lookback_days
                .push(format!("{name} ({} days)", lookback.unwrap_or_default()));
        }
    };
    for e in sources.rss.iter().flatten() {
        skip(
            &format!("rss:{}", e.name.as_deref().unwrap_or(&e.url)),
            e.always_include,
            e.score_boost,
            None,
        );
        let feed = Feed::Rss { url: e.url.clone() };
        let kind = guess_kind(&feed);
        done.add(feeds, feed, kind, e.journal.clone(), e.journal.clone())?;
    }
    let openalex = sources.openalex.as_ref();
    for q in openalex
        .and_then(|o| o.queries.as_ref())
        .into_iter()
        .flatten()
    {
        skip(
            &format!("openalex:{}", q.name.as_deref().unwrap_or(&q.filter)),
            q.always_include,
            q.score_boost,
            q.lookback_days,
        );
        let feed = Feed::OpenAlex {
            filter: q.filter.clone(),
        };
        done.add(feeds, feed, SourceKind::Journal, q.name.clone(), None)?;
    }
    let from_openalex = openalex
        .and_then(|o| o.issns.as_deref())
        .unwrap_or_default();
    let from_crossref = sources
        .crossref
        .as_ref()
        .and_then(|c| c.issns.as_deref())
        .unwrap_or_default();
    let issns = issn_feeds(from_openalex, via)
        .into_iter()
        .chain(issn_feeds(from_crossref, IssnVia::Crossref));
    for feed in issns {
        done.add(feeds, feed, SourceKind::Journal, None, None)?;
    }
    Ok(done)
}

/// The interest of the lexicon, and the entries that are no regex of this program (skipped).
fn lexicon(scoring: &Scoring, name: &str, bad: &mut Vec<String>) -> Interest {
    let min_score = scoring.publish_min_score.unwrap_or(3.0);
    let interest = |include: Vec<Weighted>, exclude: Vec<Matcher>| Interest {
        name: name.to_string(),
        include,
        exclude,
        min_score,
    };
    // an entry compiles, or the message that skips it
    let check = |include: Vec<Weighted>, exclude: Vec<Matcher>| {
        Lexicon::new(&interest(include, exclude), 1.0)
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    let mut includes = Vec::new();
    for e in scoring.include.iter().flatten() {
        let matcher = match (&e.pattern, &e.term) {
            (Some(pattern), _) => Matcher::Pattern(pattern.clone()),
            (None, Some(term)) => Matcher::Term(term.clone()),
            (None, None) => {
                bad.push("an include with neither term nor pattern".to_string());
                continue;
            }
        };
        let shown = format!("{matcher:?}");
        let w = Weighted {
            matcher,
            weight: e.weight.unwrap_or(1.0),
            label: e.label.clone(),
        };
        match check(vec![w.clone()], Vec::new()) {
            Ok(()) => includes.push(w),
            Err(e) => bad.push(format!("{shown}: {e}")),
        }
    }
    let mut excludes = Vec::new();
    for e in scoring.exclude.iter().flatten() {
        let matcher = match e {
            Exclude::Term(term)
            | Exclude::Entry {
                term: Some(term),
                pattern: None,
            } => Matcher::Term(term.clone()),
            Exclude::Entry {
                pattern: Some(pattern),
                ..
            } => Matcher::Pattern(pattern.clone()),
            Exclude::Entry { .. } => {
                bad.push("an exclude with neither term nor pattern".to_string());
                continue;
            }
        };
        match check(Vec::new(), vec![matcher.clone()]) {
            Ok(()) => excludes.push(matcher),
            Err(e) => bad.push(format!("{matcher:?}: {e}")),
        }
    }
    interest(includes, excludes)
}

/// A warning on the error stream.
fn warn(ctx: &mut Ctx, message: String) -> Result<()> {
    writeln!(ctx.err, "feeds: warning: {message}")?;
    Ok(())
}

/// Migrates a paper-feeder setup into the folder.
pub fn import(ctx: &mut Ctx, a: ImportArgs) -> Result<()> {
    let sources: Sources = read(&a.sources)?;
    let scoring: Scoring = read(&a.scoring)?;
    let mut feeds = ctx.open()?;
    let mut skipped = Skipped::default();
    let done = subscribe_all(&mut feeds, &sources, a.issn_via, &mut skipped)?;
    if let Some(days) = sources.lookback_days {
        feeds.set_lookback_days(days)?;
    }
    let mut bad = Vec::new();
    let interest = lexicon(&scoring, &a.interest, &mut bad);
    let mut interests = feeds.interests().to_vec();
    let replaced = interests.iter().any(|i| i.name == interest.name);
    interests.retain(|i| i.name != interest.name);
    let (terms, vetoes, min) = (
        interest.include.len(),
        interest.exclude.len(),
        interest.min_score,
    );
    interests.push(interest);
    let boost = scoring.title_boost.unwrap_or_else(|| feeds.title_boost());
    feeds.set_interests(interests, boost)?;

    let (added, already, failed) = (done.added, done.already, done.failed.len());
    writeln!(
        ctx.out,
        "subscriptions: {added} added, {already} there already, {failed} failed"
    )?;
    if let Some(days) = sources.lookback_days {
        writeln!(ctx.out, "lookback: {days} days")?;
    }
    let replaced = if replaced {
        ", replacing the one of this name"
    } else {
        ""
    };
    writeln!(
        ctx.out,
        "interest {}: {terms} terms, {vetoes} vetoes, from score {min}, title boost {boost}{replaced}",
        a.interest
    )?;
    if let Some(mailto) = &sources.mailto {
        writeln!(
            ctx.out,
            "mailto {mailto}: not stored; set FEEDS_MAILTO or pass --mailto"
        )?;
    }
    for message in done.failed.iter().chain(&bad) {
        warn(ctx, format!("skipped {message}"))?;
    }
    let unsupported = [
        ("always_include", &skipped.always_include),
        ("score_boost", &skipped.score_boost),
        ("lookback_days of a query", &skipped.lookback_days),
    ];
    for (what, names) in unsupported {
        if !names.is_empty() {
            warn(
                ctx,
                format!(
                    "{what} has no equivalent yet, skipped for {}",
                    names.join(", ")
                ),
            )?;
        }
    }
    let unused = [
        ("window_days", sources.window_days.is_some()),
        ("retention_days", sources.retention_days.is_some()),
        ("render", sources.render.is_some()),
        ("serendipity_count", scoring.serendipity_count.is_some()),
        ("llm_rerank", scoring.llm_rerank.is_some()),
    ];
    let unused: Vec<&str> = unused
        .into_iter()
        .filter(|(_, set)| *set)
        .map(|(name, _)| name)
        .collect();
    if !unused.is_empty() {
        warn(ctx, format!("not used: {}", unused.join(", ")))?;
    }
    Ok(())
}
