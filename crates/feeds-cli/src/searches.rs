//! The saved searches of the Feeds tab: listed, added, removed; and `--saved` of `list` and `digest`.

use papers_feeds::{Feeds, Search, Sort};

use crate::cli::{SearchAction, SearchesArgs, SortArg};
use crate::text::table;
use crate::{Ctx, Result, fail};

impl From<SortArg> for Sort {
    fn from(sort: SortArg) -> Sort {
        match sort {
            SortArg::Score => Sort::Score,
            SortArg::Newest => Sort::Newest,
            SortArg::Like => Sort::Like,
        }
    }
}

/// The word of an order.
fn sort_name(sort: Sort) -> String {
    let json = serde_json::to_value(sort).expect("json");
    json.as_str().unwrap_or_default().to_string()
}

/// A saved search by its name, in any case, or the error.
pub(crate) fn saved<'a>(feeds: &'a Feeds, name: &str) -> Result<&'a Search> {
    feeds
        .saved_search(name)
        .map_or_else(|| fail(format!("{name}: no such saved search")), Ok)
}

/// The query of a saved search (if one is named) and the query typed, both to hold.
pub(crate) fn with_saved(saved: Option<&Search>, typed: &str) -> String {
    let parts = [saved.map_or("", |s| s.query.as_str()), typed.trim()];
    parts
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `feeds searches`: the saved searches (name, order, query), or one added or removed.
pub fn searches(ctx: &mut Ctx, a: SearchesArgs) -> Result<()> {
    let mut feeds = ctx.open()?;
    match a.action {
        None if a.json => {
            let json = serde_json::to_string_pretty(feeds.searches()).expect("json");
            writeln!(ctx.out, "{json}")?;
        }
        None => {
            let row = |s: &Search| vec![s.name.clone(), sort_name(s.sort), s.query.clone()];
            let rows: Vec<Vec<String>> = feeds.searches().iter().map(row).collect();
            write!(ctx.out, "{}", table(&rows))?;
        }
        Some(SearchAction::Add { name, query, sort }) => {
            let search = Search {
                name: name.trim().to_string(),
                query: query.join(" "),
                sort: sort.into(),
            };
            let mut list = feeds.searches().to_vec();
            let said = match list
                .iter()
                .position(|s| feeds.saved_search(&name) == Some(s))
            {
                Some(at) => {
                    list[at] = search;
                    "replaced"
                }
                None => {
                    list.push(search);
                    "saved"
                }
            };
            feeds.set_searches(list)?;
            writeln!(ctx.out, "{said} {}", name.trim())?;
        }
        Some(SearchAction::Remove { name }) => {
            let gone = saved(&feeds, &name)?.name.clone();
            let list = feeds.searches().iter().filter(|s| s.name != gone);
            feeds.set_searches(list.cloned().collect())?;
            writeln!(ctx.out, "removed {gone}")?;
        }
    }
    Ok(())
}
