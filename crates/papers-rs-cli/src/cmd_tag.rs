//! `papers tag`: the command line of `papers_rs::command::tag`.

use papers_rs::command::read_selection;
use papers_rs::command::report::{Notes, print_changes};
use papers_rs::command::tag::{add_remove, rename, renamed_line, tag_tree};
use papers_rs::config::Config;
use papers_rs::error::Error;

use crate::cmd_write::{Done, Failure, check_install, open_session, run_session};
use crate::parse::Leaf;

pub fn tag_cmd(leaf: &Leaf, config: Config) -> Done {
    let entries = leaf.strings("entries");
    let add = leaf.strings("add");
    let remove = leaf.strings("rm");
    let renaming = leaf.strings("rename");
    let config = check_install(leaf, config, true)?;
    let session = open_session(leaf, config, false)?;
    let porcelain = leaf.flag("porcelain");
    run_session(session, |session| {
        if !renaming.is_empty() {
            if !entries.is_empty() || !add.is_empty() || !remove.is_empty() {
                return Err(Failure::Usage(
                    "--rename OLD NEW applies to the whole library: no KEY, --add or --rm"
                        .to_string(),
                ));
            }
            let (old, new) = (&renaming[0], &renaming[1]);
            let count = rename(&mut session.library, old, new);
            session.commit()?;
            session.say.line(renamed_line(old, new, count));
            return Ok(());
        }
        if !add.is_empty() || !remove.is_empty() {
            if entries.is_empty() {
                return Err(Failure::Usage(
                    "--add and --rm need the entries: KEY..., or - to read them from the standard input. --add and --rm take all the words after them: put the keys first (papers tag KEY... --add TAG...)"
                        .to_string(),
                ));
            }
            let args = read_selection(&entries, &mut std::io::stdin()).map_err(Error::from)?;
            let keys = session.library.select(&args)?;
            add_remove(&mut session.library, &keys, &add, &remove);
            session.commit()?;
            let color = session.color();
            print_changes(
                &session.library,
                &keys,
                false,
                &Notes::new(),
                porcelain,
                color,
                &session.say,
            );
            return Ok(());
        }
        let lines = if entries.is_empty() {
            tag_tree(session.library.entries())
        } else {
            let args = read_selection(&entries, &mut std::io::stdin()).map_err(Error::from)?;
            let keys = session.library.select(&args)?;
            tag_tree(keys.iter().filter_map(|k| session.library.get(k)))
        };
        for line in lines {
            session.say.line(line);
        }
        Ok(())
    })
}
