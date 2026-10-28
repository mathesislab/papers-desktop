//! `papers rm`: the command line of `papers_rs::command::rm`.

use papers_rs::command::read_selection;
use papers_rs::command::report::print_changes;
use papers_rs::command::rm::{Files, remove_entries};
use papers_rs::config::Config;
use papers_rs::error::Error;

use crate::cmd_write::{Done, Failure, check_install, open_session, run_session};
use crate::parse::Leaf;

pub fn rm_cmd(leaf: &Leaf, config: Config) -> Done {
    let delete = leaf.flag("delete_files");
    let keep = leaf.flag("keep_files");
    if delete && keep {
        return Err(Failure::Usage(
            "--delete-files and --keep-files are mutually exclusive".to_string(),
        ));
    }
    let files = if delete {
        Files::Delete
    } else if keep {
        Files::Keep
    } else {
        Files::Ask
    };
    let entries = leaf.strings("entries");
    if delete && config.shallow() {
        return Err(Error::exit(
            "--delete-files: the files of a shallow library are the pool's: remove them with `papers file rm FILE`",
        )
        .into());
    }
    let config = check_install(leaf, config, true)?;
    let session = open_session(leaf, config, leaf.flag("yes"))?;
    let porcelain = leaf.flag("porcelain");
    run_session(session, |session| {
        let args = read_selection(&entries, &mut std::io::stdin()).map_err(Error::from)?;
        let keys = session.library.select(&args)?;
        let removed: Vec<String> = keys.clone();
        let to_delete = remove_entries(session, &keys, files, !entries.iter().any(|e| e == "-"))?;
        let committed = session.commit()?;
        let color = session.color();
        print_changes(
            &session.library,
            &removed,
            false,
            &committed.notes,
            porcelain,
            color,
            &session.say,
        );
        session.delete_files(&to_delete, &[])?;
        Ok(())
    })
}
