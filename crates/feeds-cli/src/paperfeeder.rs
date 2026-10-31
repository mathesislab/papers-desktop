//! `feeds import-paper-feeder`: a paper-feeder setup (`sources.yaml`, `scoring.yaml`) as
//! subscriptions, the lookback and one interest. The `score_boost` of a source is the boost of
//! its subscription, its `always_include` a rule that shortlists its items; what has no
//! equivalent here is skipped and reported: the `lookback_days` of a query.

use std::path::Path;

use papers_feeds::{
    Action, Error as FeedsError, Feed, Feeds, Interest, Lexicon, Matcher, Rule, SourceKind,
    Subscription, Weighted,
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

/// What a source of `sources.yaml` says of its subscription, beside its feed.
#[derive(Debug, Default)]
struct Source {
    /// the name shown in the report
    name: String,
    title: Option<String>,
    journal: Option<String>,
    always_include: bool,
    score_boost: f64,
}

/// How the subscriptions went.
#[derive(Debug, Default)]
struct Subscribed {
    added: usize,
    already: usize,
    failed: Vec<String>,
    /// the sources whose subscription got a boost, with it
    boosted: Vec<String>,
    /// the sources whose items a rule shortlists
    always: Vec<String>,
    /// the rules of `always`
    rules: Vec<Rule>,
}

/// The query of the items of a subscription: `src:"title"`, by its id if it has no title (the
/// language has no way to quote a quote).
fn items_of(subscription: &Subscription) -> String {
    let name = subscription.title.as_ref().unwrap_or(&subscription.id);
    format!("src:\"{}\"", name.replace('"', ""))
}

impl Subscribed {
    /// Subscribes to a feed, naming it and setting its boost; one that fails or is there already
    /// is counted (and left as it is).
    fn add(
        &mut self,
        feeds: &mut Feeds,
        feed: Feed,
        kind: SourceKind,
        source: Source,
    ) -> Result<()> {
        let shown = feed.to_string();
        match feeds.subscribe(feed, kind) {
            Ok(subscription) => {
                self.added += 1;
                let subscription = feeds.edit_subscription(&subscription.id, |s| {
                    s.title = source.title;
                    s.journal = source.journal;
                    s.boost = source.score_boost;
                })?;
                if source.score_boost != 0.0 {
                    let boost = source.score_boost;
                    self.boosted.push(format!("{} ({boost})", source.name));
                }
                if source.always_include {
                    self.always.push(source.name);
                    self.rules.push(Rule {
                        when: items_of(&subscription),
                        then: Action::Shortlist,
                    });
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
    lookbacks: &mut Vec<String>,
) -> Result<Subscribed> {
    let mut done = Subscribed::default();
    for e in sources.rss.iter().flatten() {
        let feed = Feed::Rss { url: e.url.clone() };
        let kind = guess_kind(&feed);
        let source = Source {
            name: format!("rss:{}", e.name.as_deref().unwrap_or(&e.url)),
            title: e.journal.clone(),
            journal: e.journal.clone(),
            always_include: e.always_include == Some(true),
            score_boost: e.score_boost.unwrap_or_default(),
        };
        done.add(feeds, feed, kind, source)?;
    }
    let openalex = sources.openalex.as_ref();
    for q in openalex
        .and_then(|o| o.queries.as_ref())
        .into_iter()
        .flatten()
    {
        let name = format!("openalex:{}", q.name.as_deref().unwrap_or(&q.filter));
        if let Some(days) = q.lookback_days {
            lookbacks.push(format!("{name} ({days} days)"));
        }
        let feed = Feed::OpenAlex {
            filter: q.filter.clone(),
        };
        let source = Source {
            name,
            title: q.name.clone(),
            always_include: q.always_include == Some(true),
            score_boost: q.score_boost.unwrap_or_default(),
            ..Source::default()
        };
        done.add(feeds, feed, SourceKind::Journal, source)?;
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
        done.add(feeds, feed, SourceKind::Journal, Source::default())?;
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
    let mut lookbacks = Vec::new();
    let done = subscribe_all(&mut feeds, &sources, a.issn_via, &mut lookbacks)?;
    if let Some(days) = sources.lookback_days {
        feeds.set_lookback_days(days)?;
    }
    let mut rules = feeds.rules().to_vec();
    let new_rules: Vec<Rule> = done
        .rules
        .iter()
        .filter(|r| !rules.contains(r))
        .cloned()
        .collect();
    rules.extend(new_rules.iter().cloned());
    feeds.set_rules(rules)?;
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
    if !done.boosted.is_empty() {
        writeln!(ctx.out, "boost: {}", done.boosted.join(", "))?;
    }
    if !done.always.is_empty() {
        let (n, names) = (new_rules.len(), done.always.join(", "));
        writeln!(
            ctx.out,
            "rules: {n} added, shortlisting the items of {names} (always_include)"
        )?;
    }
    if let Some(mailto) = &sources.mailto {
        writeln!(
            ctx.out,
            "mailto {mailto}: not stored; set FEEDS_MAILTO or pass --mailto"
        )?;
    }
    for message in done.failed.iter().chain(&bad) {
        warn(ctx, format!("skipped {message}"))?;
    }
    if !lookbacks.is_empty() {
        let names = lookbacks.join(", ");
        let message =
            format!("lookback_days of a query has no equivalent yet, skipped for {names}");
        warn(ctx, message)?;
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
