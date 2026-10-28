//! The actions of `papers list` that change the entries it lists: `--add-tag`, `--rm-tag`, `--edit`,
//! `--fetch` and `--delete` (with `--delete-files`, `--keep-files`). Each is the command it stands
//! for, run on the selection of the search, through the write path (`papers.cli.entry_cmds.listcmd`):
//! the same lines and the same step of the history, whose message is the command line.

use papers_rs::command::check::{Fetch, FixOptions, fix_entries};
use papers_rs::command::edit::{Editor, edit_selected};
use papers_rs::command::report::print_changes;
use papers_rs::command::rm::{Files, remove_entries};
use papers_rs::command::tag::add_remove;
use papers_rs::config::Config;
use papers_rs::config::dirs::cache_dir;
use papers_rs::error::Error;
use papers_rs::fetch::Fetcher;
use papers_rs::search::Search;

use crate::cmd_config;
use crate::cmd_write::{Failure, open_session, run_session};
use crate::parse::Leaf;

/// What `papers list` does with the entries it lists, instead of printing them.
pub enum Action {
    AddTags(Vec<String>),
    RmTags(Vec<String>),
    Edit,
    Fetch,
    Delete,
}

impl Action {
    /// The action of the options, if it is one of these (the others are not this module's).
    pub fn of(leaf: &Leaf) -> Option<Action> {
        let add = leaf.strings("add_keywords");
        let remove = leaf.strings("rm_tag");
        if !add.is_empty() {
            Some(Action::AddTags(add))
        } else if !remove.is_empty() {
            Some(Action::RmTags(remove))
        } else if leaf.flag("edit") {
            Some(Action::Edit)
        } else if leaf.flag("fetch") {
            Some(Action::Fetch)
        } else if leaf.flag("delete") {
            Some(Action::Delete)
        } else {
            None
        }
    }
}

impl From<Failure> for cmd_config::Failure {
    fn from(failure: Failure) -> cmd_config::Failure {
        match failure {
            Failure::Usage(message) => cmd_config::Failure::Usage(message),
            Failure::Error(error) => cmd_config::Failure::Error(error),
        }
    }
}

/// Run an action on the entries that `search` selects.
pub fn run(
    leaf: &Leaf,
    config: Config,
    search: &Search,
    action: Action,
) -> Result<(), cmd_config::Failure> {
    let session = open_session(leaf, config, false)?;
    let porcelain = leaf.flag("porcelain");
    let delete_files = leaf.flag("delete_files");
    let keep_files = leaf.flag("keep_files");
    run_session(session, |session| {
        crate::cmd_list::warn_similarity(leaf);
        let all: Vec<&papers_rs::bib::Entry> = session.library.entries().collect();
        let keys: Vec<String> = search
            .select(
                &all,
                Some(&session.library.distinct),
                &session.library.relative_to,
            )
            .map_err(Failure::Usage)?
            .iter()
            .map(|e| e.key.clone())
            .collect();
        // what the command considered: the entries it changed are said, as `papers tag` says them
        let mut touched = keys.clone();
        let mut after_save: Vec<String> = Vec::new();
        match &action {
            Action::AddTags(tags) => add_remove(&mut session.library, &keys, tags, &[]),
            Action::RmTags(tags) => add_remove(&mut session.library, &keys, &[], tags),
            Action::Edit => {
                edit_selected(&mut session.library, &keys, &Editor::of(&session.config))?;
                // the edit may rename, add or drop entries: all are reported
                touched = session.library.entries().map(|e| e.key.clone()).collect();
            }
            Action::Fetch => {
                // `Biblio.fix_entry(fix_doi=True, fix_key=True, fetch_all=True, interactive=True)`
                let options = FixOptions {
                    fix_doi: true,
                    fix_key: true,
                    fetch_all: true,
                    interactive: true,
                    ..FixOptions::default()
                };
                let fetcher = Fetcher::new(&cache_dir()).ok();
                touched = fix_entries(session, &keys, &options, fetcher.as_ref().map(|f| f as &dyn Fetch))?;
            }
            Action::Delete => {
                if delete_files && keep_files {
                    return Err(Failure::Usage(
                        "--delete-files and --keep-files are mutually exclusive".to_string(),
                    ));
                }
                if delete_files && session.config.shallow() {
                    return Err(Error::exit(
                        "--delete-files: the files of a shallow library are the pool's: remove them with `papers file rm FILE`",
                    )
                    .into());
                }
                let files = if delete_files {
                    Files::Delete
                } else if keep_files {
                    Files::Keep
                } else {
                    Files::Ask
                };
                after_save = remove_entries(session, &keys, files, true)?;
            }
        }
        let committed = session.commit()?;
        if matches!(action, Action::Delete) {
            // the files go before the lines of what was removed (papers lists after it deletes)
            session.delete_files(&after_save, &[])?;
        }
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
        Ok(())
    })
    .map_err(Into::into)
}
