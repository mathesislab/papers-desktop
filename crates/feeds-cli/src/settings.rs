//! The interests, the rules, and the folder's settings.

use papers_feeds::Matcher;

use crate::cli::{AccessArgs, ConfigArgs, ConfigSet, JsonArg};
use crate::{Ctx, Result};

/// A term or pattern as written: a pattern between slashes.
fn matcher(matcher: &Matcher) -> String {
    match matcher {
        Matcher::Term(term) => term.clone(),
        Matcher::Pattern(pattern) => format!("/{pattern}/"),
    }
}

/// `feeds interests`: each interest with its terms and weights, or as JSON.
pub fn interests(ctx: &mut Ctx, a: JsonArg) -> Result<()> {
    let feeds = ctx.open()?;
    let boost = feeds.title_boost();
    if a.json {
        let json = serde_json::json!({"interests": feeds.interests(), "title_boost": boost});
        writeln!(
            ctx.out,
            "{}",
            serde_json::to_string_pretty(&json).expect("json")
        )?;
        return Ok(());
    }
    for interest in feeds.interests() {
        let (name, min) = (&interest.name, interest.min_score);
        writeln!(
            ctx.out,
            "{name} (from score {min}; a term in the title counts {boost} times)"
        )?;
        for w in &interest.include {
            let label = w
                .label
                .as_ref()
                .map_or(String::new(), |l| format!(" as {l}"));
            writeln!(ctx.out, "  {:>5}  {}{label}", w.weight, matcher(&w.matcher))?;
        }
        for m in &interest.exclude {
            writeln!(ctx.out, "  {:>5}  {}", "veto", matcher(m))?;
        }
    }
    Ok(())
}

/// `feeds rules`: each rule, `when` and what it does, or as JSON.
pub fn rules(ctx: &mut Ctx, a: JsonArg) -> Result<()> {
    let feeds = ctx.open()?;
    if a.json {
        writeln!(
            ctx.out,
            "{}",
            serde_json::to_string_pretty(feeds.rules()).expect("json")
        )?;
        return Ok(());
    }
    for rule in feeds.rules() {
        let then = serde_json::to_value(rule.then).expect("json");
        writeln!(
            ctx.out,
            "{:?} -> {}",
            rule.when,
            then.as_str().unwrap_or_default()
        )?;
    }
    Ok(())
}

/// `feeds config`: sets what it is told, then shows the folder, its settings, and what the APIs
/// are asked with.
pub fn config(ctx: &mut Ctx, a: ConfigArgs) -> Result<()> {
    let mut feeds = ctx.open()?;
    match a.set {
        Some(ConfigSet::KeepDays { days }) => feeds.set_keep_days(days)?,
        Some(ConfigSet::LookbackDays { days }) => feeds.set_lookback_days(days)?,
        None => {}
    }
    let access = ctx.access(&AccessArgs {
        mailto: None,
        openalex_key: None,
    });
    let state = |set: bool, name: &str| match set {
        true => format!("set ({name})"),
        false => format!("not set ({name}, or the option of fetch)"),
    };
    let rows = [
        ("folder", ctx.dir.display().to_string()),
        ("subscriptions", feeds.subscriptions().len().to_string()),
        ("items", feeds.items().len().to_string()),
        ("interests", feeds.interests().len().to_string()),
        ("rules", feeds.rules().len().to_string()),
        ("title boost", feeds.title_boost().to_string()),
        ("keep days", feeds.keep_days().to_string()),
        ("lookback days", feeds.lookback_days().to_string()),
        (
            "mailto",
            access
                .mailto
                .clone()
                .unwrap_or_else(|| state(false, "FEEDS_MAILTO")),
        ),
        (
            "openalex key",
            state(access.openalex_key.is_some(), "OPENALEX_KEY"),
        ),
    ];
    for (name, value) in rows {
        writeln!(ctx.out, "{name:<14}{value}")?;
    }
    Ok(())
}
