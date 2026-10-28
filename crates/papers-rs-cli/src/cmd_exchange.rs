//! `papers pull`, `papers push`, `papers sync`: the command line of `papers_rs::command::exchange`.

use papers_rs::command::exchange::{ExchangeArgs, pull, push, sync};
use papers_rs::command::read_selection;
use papers_rs::config::Config;
use papers_rs::error::Error;
use papers_rs::search::Search;

use crate::cmd_add::duplicate_resolver;
use crate::cmd_config;
use crate::cmd_write::{Done, Failure, check_install, color_enabled, open_session, run_session};
use crate::parse::Leaf;

/// The arguments of the command: `-` in the keys reads them from the standard input. Without
/// `given`, a pull of the shared entries; `sync` reads the keys once (the pull and the push use them).
pub fn exchange_args(leaf: &Leaf, read_once: bool) -> Result<ExchangeArgs, Error> {
    let given = leaf.strings("entries");
    let entries = if read_once {
        read_selection(&given, &mut std::io::stdin()).map_err(Error::from)?
    } else {
        given.clone()
    };
    Ok(ExchangeArgs {
        given: if read_once {
            !entries.is_empty()
        } else {
            !given.is_empty()
        },
        entries,
        all: leaf.flag("all"),
        pool: leaf.string("pool").filter(|p| !p.is_empty()),
        check: leaf.flag("check"),
        selected: None,
    })
}

pub fn exchange_cmd(command: &str, leaf: &Leaf, config: Config) -> Done {
    let config = check_install(leaf, config, true)?;
    let editor_config = config.clone();
    let color = color_enabled(leaf);
    let session = open_session(leaf, config, false)?;
    run_session(session, |session| {
        let mut args = exchange_args(leaf, command == "sync")?;
        if command != "sync" {
            // the keys are read where they are selected: `-` reads the standard input there
            args.entries = read_selection(&args.entries, &mut std::io::stdin()).map_err(Error::from)?;
        }
        let mut no_check = false;
        let mut resolver = duplicate_resolver(leaf, &editor_config, &mut no_check, color);
        match command {
            "pull" => pull(session, &args, resolver.as_mut())?,
            "push" => push(session, &args, resolver.as_mut())?,
            _ => {
                let mut push_resolver = duplicate_resolver(leaf, &editor_config, &mut no_check, color);
                sync(session, &args, resolver.as_mut(), push_resolver.as_mut())?;
            }
        }
        Ok(())
    })
}

/// `papers list ... --pull`, `--push`, `--sync`: the exchange of the entries that `list` selects (`pool`: the
/// `--pool PATH` option), with the options of `papers pull`, `push` and `sync`.
pub fn list_exchange(kind: &str, leaf: &Leaf, config: Config, search: &Search) -> Result<(), cmd_config::Failure> {
    let editor_config = config.clone();
    let color = color_enabled(leaf);
    let session = open_session(leaf, config, false)?;
    run_session(session, |session| {
        crate::cmd_list::warn_similarity(leaf);
        let all: Vec<&papers_rs::bib::Entry> = session.library.entries().collect();
        let keys: Vec<String> = search
            .select(&all, Some(&session.library.distinct), &session.library.relative_to)
            .map_err(Failure::Usage)?
            .iter()
            .map(|e| e.key.clone())
            .collect();
        let args = ExchangeArgs {
            pool: leaf.strings("pool").into_iter().next().filter(|p| !p.is_empty()),
            selected: Some(keys),
            ..ExchangeArgs::default()
        };
        let mut no_check = false;
        let mut resolver = duplicate_resolver(leaf, &editor_config, &mut no_check, color);
        match kind {
            "pull" => pull(session, &args, resolver.as_mut())?,
            "push" => push(session, &args, resolver.as_mut())?,
            _ => {
                let mut push_resolver = duplicate_resolver(leaf, &editor_config, &mut no_check, color);
                sync(session, &args, resolver.as_mut(), push_resolver.as_mut())?;
            }
        }
        Ok(())
    })
    .map_err(Into::into)
}
