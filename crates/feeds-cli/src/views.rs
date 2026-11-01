//! The saved views of the Feeds tab: listed, added, removed; and `--saved` of `list` and `digest`.

use papers_feeds::{Feeds, SavedView};

use crate::cli::{ViewAction, ViewsArgs};
use crate::text::table;
use crate::{Ctx, Result, fail};

/// A saved view by its name, in any case, or the error.
pub(crate) fn saved<'a>(feeds: &'a Feeds, name: &str) -> Result<&'a SavedView> {
    feeds
        .saved_view(name)
        .map_or_else(|| fail(format!("{name}: no such saved view")), Ok)
}

/// The query of a saved view (if one is named) and the query typed, both to hold.
pub(crate) fn with_saved(saved: Option<&SavedView>, typed: &str) -> String {
    let parts = [saved.map_or("", |s| s.query.as_str()), typed.trim()];
    parts
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `feeds views`: the saved views (name, order, query), or one added or removed.
pub fn views(ctx: &mut Ctx, a: ViewsArgs) -> Result<()> {
    let mut feeds = ctx.open()?;
    match a.action {
        None if a.json => {
            let json = serde_json::to_string_pretty(feeds.saved_views()).expect("json");
            writeln!(ctx.out, "{json}")?;
        }
        None => {
            let row = |v: &SavedView| vec![v.name.clone(), v.sort.to_string(), v.query.clone()];
            let rows: Vec<Vec<String>> = feeds.saved_views().iter().map(row).collect();
            write!(ctx.out, "{}", table(&rows))?;
        }
        Some(ViewAction::Add { name, query, sort }) => {
            let view = SavedView {
                name: name.trim().to_string(),
                query: query.join(" "),
                sort,
            };
            let mut list = feeds.saved_views().to_vec();
            let said = match list.iter().position(|v| feeds.saved_view(&name) == Some(v)) {
                Some(at) => {
                    list[at] = view;
                    "replaced"
                }
                None => {
                    list.push(view);
                    "saved"
                }
            };
            feeds.set_saved_views(list)?;
            writeln!(ctx.out, "{said} {}", name.trim())?;
        }
        Some(ViewAction::Remove { name }) => {
            let gone = saved(&feeds, &name)?.name.clone();
            let list = feeds.saved_views().iter().filter(|v| v.name != gone);
            feeds.set_saved_views(list.cloned().collect())?;
            writeln!(ctx.out, "removed {gone}")?;
        }
    }
    Ok(())
}
