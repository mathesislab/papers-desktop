//! `papers check`: the command line of `papers_rs::command::check`.

use std::io::IsTerminal;

use papers_rs::command::check::{
    CheckRequest, Encoding, Fetch, FixOptions, check, default_duplicate_mode,
};
use papers_rs::command::edit::Editor;
use papers_rs::command::read_selection;
use papers_rs::command::report::print_changes;
use papers_rs::config::Config;
use papers_rs::config::dirs::cache_dir;
use papers_rs::config::json::py_repr_str;
use papers_rs::error::Error;
use papers_rs::fetch::Fetcher;
use papers_rs::log;
use papers_rs::merge::{OnDuplicate, Resolve, Resolver};

use crate::cmd_write::{Done, Failure, check_install, color_enabled, open_session, run_session};
use crate::dialogue::Dialogue;
use crate::parse::Leaf;

/// The `--on-duplicate` word of a deprecated `-m/--mode` letter of `check` (`CHECK_MODES`).
fn mode_word(letter: &str) -> &'static str {
    match letter {
        "i" => "ask",
        "m" => "keep",
        _ => "skip",
    }
}

/// The spelling of the deprecated `-f/--force`, as it was given: argparse tells the option string
/// (a prefix of `--force` is `--force`).
fn force_spelling() -> &'static str {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let given = args
        .iter()
        .take_while(|a| *a != "--")
        .any(|a| a.len() >= 4 && "--force".starts_with(a.as_str()) && a.starts_with("--fo"));
    if given { "--force" } else { "-f" }
}

/// The resolver of the duplicates: `--on-duplicate`, else the deprecated `-m/--mode` letter, else
/// `skip` with `-y/--yes`, `ask` in a terminal and `error` without one (`on_duplicate_mode`).
fn duplicate_resolver(leaf: &Leaf, config: &Config, yes: bool, color: bool) -> Box<dyn Resolve> {
    let mut mode = leaf.string("on_duplicate");
    if let Some(letter) = leaf.string("mode") {
        let new = mode_word(&letter);
        log::warning(&format!(
            "-m/--mode {letter} is deprecated: use --on-duplicate {new}"
        ));
        if mode.is_none() {
            mode = Some(new.to_string());
        }
    }
    let mode = mode
        .as_deref()
        .and_then(OnDuplicate::parse)
        .unwrap_or_else(|| default_duplicate_mode(yes, std::io::stdin().is_terminal()));
    if mode == OnDuplicate::Ask {
        Box::new(Dialogue::new(color, Editor::of(config)))
    } else {
        Box::new(Resolver::new(mode))
    }
}

pub fn check_cmd(leaf: &Leaf, config: Config) -> Done {
    if leaf.flag("force") {
        log::warning(&format!("{} is deprecated: use -y/--yes", force_spelling()));
    }
    let yes = leaf.flag("yes") || leaf.flag("force");
    let config = check_install(leaf, config, true)?;
    let repr = |p: &Option<String>| p.as_deref().map_or("None".to_string(), py_repr_str);
    log::debug(&format!("bibtex: {}", repr(&config.bibtex)));
    log::debug(&format!("filesdir: {}", repr(&config.filesdir)));
    let session = open_session(leaf, config, false)?;
    let color = color_enabled(leaf);
    let porcelain = leaf.flag("porcelain");
    let encoding = leaf.string("encoding").and_then(|e| Encoding::parse(&e));
    let fix = FixOptions {
        fix_doi: leaf.flag("fix_doi"),
        fetch: leaf.flag("fetch"),
        fetch_all: leaf.flag("fetch_all"),
        fix_key: leaf.flag("fix_key"),
        auto_key: leaf.flag("auto_key"),
        key_ascii: leaf.flag("key_ascii"),
        encoding,
        format_name: leaf.flag("format_name"),
        interactive: !yes,
    };
    let mut selection = leaf.strings("entries");
    selection.extend(leaf.strings("keys"));
    let editor_config = session.config.clone();
    run_session(session, |session| {
        let selection = read_selection(&selection, &mut std::io::stdin()).map_err(Error::from)?;
        let request = CheckRequest {
            selection,
            fix: fix.clone(),
            yes,
            duplicates: leaf.flag("duplicates"),
            on_duplicate_given: leaf.string("on_duplicate").is_some()
                || leaf.string("mode").is_some(),
        };
        let fetcher = if fix.fetch || fix.fetch_all {
            Fetcher::new(&cache_dir()).ok()
        } else {
            None
        };
        let mut resolver = || duplicate_resolver(leaf, &editor_config, yes, color);
        let touched = check(
            session,
            &request,
            fetcher.as_ref().map(|f| f as &dyn Fetch),
            &mut resolver,
        )?;
        let committed = session.commit()?;
        let color = session.color();
        print_changes(
            &session.library,
            &touched,
            false,
            &committed.notes,
            porcelain,
            color,
            &session.say,
        );
        Ok::<(), Failure>(())
    })
}
