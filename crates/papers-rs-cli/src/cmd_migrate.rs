//! `papers migrate`: the command line of `papers_rs::command::migrate`.

use papers_rs::command::migrate::{folders_line, migrate, migrate_folders};
use papers_rs::config::Config;
use papers_rs::log;

use crate::cmd_write::{Done, check_install, convert_file_links, open_session, run_session};
use crate::parse::Leaf;

pub fn migrate_cmd(leaf: &Leaf, mut config: Config) -> Done {
    if leaf.flag("folders") {
        let config = check_install(leaf, config, true)?;
        let session = open_session(leaf, config, false)?;
        return run_session(session, |session| {
            let count = migrate_folders(session)?;
            if count > 0 {
                session.commit()?;
            }
            log::println(&folders_line(count));
            Ok(())
        });
    }
    migrate(
        &mut config,
        leaf.flag("relative"),
        leaf.flag("dry_run"),
        &mut convert_file_links,
    )?;
    Ok(())
}
