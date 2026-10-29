//! `papers edit`: the command line of `papers_rs::command::edit`.

use std::io::{BufRead, IsTerminal, Read};

use papers_rs::command::edit::{Editor, edit_selected, replace_from_text_in, set_fields, set_key};
use papers_rs::command::read_selection;
use papers_rs::command::report::print_changes;
use papers_rs::config::Config;
use papers_rs::config::paths::{abspath, dirname};
use papers_rs::error::Error;
use papers_rs::formats::{Format, file_format};

use crate::cmd_add::field_values;
use crate::cmd_write::{Done, Failure, check_install, open_session, run_session};
use crate::parse::Leaf;

/// The text of one entry from the standard input. From a pipe or a file: all
/// of it. From a terminal (a paste): until the braces of the first
/// `@type{...}` close, or the end of the input (Ctrl-D), with a hint on the
/// standard error first (`read_entry_text`).
fn read_entry_text() -> std::io::Result<String> {
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        let mut text = String::new();
        stdin.lock().read_to_string(&mut text)?;
        return Ok(text);
    }
    eprintln!(
        "Paste or type the entry: papers continues when its closing brace is read (Ctrl-D ends the input, Ctrl-C cancels)"
    );
    let mut lines = String::new();
    let (mut depth, mut started, mut opened) = (0i64, false, false);
    for line in stdin.lock().lines() {
        let line = line? + "\n";
        for c in line.chars() {
            match c {
                '@' => started = true,
                '{' if started => {
                    depth += 1;
                    opened = true;
                }
                '}' if started => depth -= 1,
                _ => {}
            }
        }
        lines.push_str(&line);
        if opened && depth <= 0 {
            break;
        }
    }
    Ok(lines)
}

pub fn edit_cmd(leaf: &Leaf, config: Config) -> Done {
    let from_file = leaf.string("from_file").filter(|f| !f.is_empty());
    let set = field_values(&leaf.strings("set"));
    let unset = leaf.strings("unset");
    let new_key = leaf.string("new_key").filter(|k| !k.is_empty());
    let keep_old_key = leaf.flag("keep_old_key");
    let entries = leaf.strings("entries");
    let format = leaf.string("format");
    let ways: Vec<&str> = [
        ("--from", from_file.is_some()),
        (
            "--set/--unset",
            !leaf.strings("set").is_empty() || !unset.is_empty(),
        ),
        ("--key", new_key.is_some()),
    ]
    .into_iter()
    .filter_map(|(name, given)| given.then_some(name))
    .collect();
    if ways.len() > 1 {
        return Err(Failure::Usage(format!(
            "{}: one at a time",
            ways.join(" and ")
        )));
    }
    if keep_old_key && new_key.is_none() {
        return Err(Failure::Usage(
            "--keep-old-key goes with --key NEW".to_string(),
        ));
    }
    if from_file.as_deref() == Some("-") && entries.iter().any(|e| e == "-") {
        return Err(Failure::Usage(
            "--from - and the keys from the standard input (-): only one of them can read it"
                .to_string(),
        ));
    }
    if format.is_some() && from_file.is_none() {
        return Err(Failure::Usage(
            "--format is the format of --from FILE".to_string(),
        ));
    }
    let config = check_install(leaf, config, true)?;
    let session = open_session(leaf, config, false)?;
    let porcelain = leaf.flag("porcelain");
    run_session(session, |session| {
        let args = read_selection(&entries, &mut std::io::stdin()).map_err(Error::from)?;
        let keys = session.library.select(&args)?;
        if keys.is_empty() {
            return Ok(());
        }
        if let Some(path) = &from_file {
            if keys.len() != 1 {
                return Err(Error::exit(format!(
                    "--from replaces one entry: {} selected",
                    keys.len()
                ))
                .into());
            }
            let text = if path == "-" {
                read_entry_text().map_err(Error::from)?
            } else {
                match std::fs::read(path) {
                    Ok(bytes) => {
                        let text =
                            String::from_utf8(bytes).map_err(|e| Error::exit(e.to_string()))?;
                        text.strip_prefix('\u{feff}').unwrap_or(&text).to_string()
                    }
                    Err(e) => {
                        let text = e.to_string();
                        let message = text.split(" (os error").next().unwrap_or(&text);
                        return Err(Error::exit(format!("--from {path}: {message}")).into());
                    }
                }
            };
            let forced = format.as_deref().and_then(Format::from_id);
            let fmt = if path == "-" {
                forced.unwrap_or(Format::Bibtex)
            } else {
                file_format(path, forced)
            };
            // the file links of FILE are relative to its folder, as with `add`
            let links_from = (path != "-").then(|| dirname(&abspath(path)));
            replace_from_text_in(
                &mut session.library,
                &keys[0],
                &text,
                fmt,
                &format!("--from {path}"),
                links_from.as_deref(),
            )?;
        } else if !set.is_empty() || !unset.is_empty() {
            set_fields(&mut session.library, &keys, &set, &unset)?;
        } else if let Some(new) = &new_key {
            if keys.len() != 1 {
                return Err(Error::exit(format!(
                    "--key changes the key of one entry: {} selected",
                    keys.len()
                ))
                .into());
            }
            set_key(&mut session.library, &keys[0], new, keep_old_key)?;
        } else {
            // the keys came from a pipe: the editor reads the terminal
            let mut editor = Editor::of(&session.config);
            editor.tty = entries.iter().any(|e| e == "-") && !std::io::stdin().is_terminal();
            edit_selected(&mut session.library, &keys, &editor)?;
        }
        let committed = session.commit()?;
        let all: Vec<String> = session.library.entries().map(|e| e.key.clone()).collect();
        let color = session.color();
        print_changes(
            &session.library,
            &all,
            false,
            &committed.notes,
            porcelain,
            color,
            &session.say,
        );
        Ok(())
    })
}
