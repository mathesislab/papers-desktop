//! `papers file` (rename, mv, attach, fetch, detach, rm, check, fix, clean, checksum) and the
//! deprecated `papers filecheck`: the command line of `papers_rs::command::file_cmd` (and of
//! `papers_rs::command::fetch_pdf` for `fetch`).

use papers_rs::command::fetch_pdf::{FileFetch, PdfOptions, file_fetch};
use papers_rs::command::file_cmd::{
    Attach, CheckOptions, MvOptions, attach_files, check_entries_files, check_entry_lines,
    checksums, clean, clean_filesdir, detach_broken_quietly, detach_entries, move_files,
    rename_entries, rm_files,
};
use papers_rs::command::files::{Mode, files_mode};
use papers_rs::command::pool::push_note_lines;
use papers_rs::command::queries::Queries;
use papers_rs::command::report::{Notes, print_changes};
use papers_rs::command::{Options, Session, read_selection};
use papers_rs::config::Config;
use papers_rs::config::paths::{abspath, short_home};
use papers_rs::error::Error;
use papers_rs::fetch::freepdf::Least;
use papers_rs::log;

use crate::cmd_write::{
    Done, Failure, Terminal, check_install, color_enabled, command_line, run_session,
};
use crate::parse::Leaf;
use crate::util::{file_uri_to_path, openalex_access};

/// A session for the library of `config`: the lines go to the standard output as they come,
/// questions to the terminal. `read_only`: nothing is written, nothing locked (a command that
/// only reports).
pub fn open(
    leaf: &Leaf,
    config: Config,
    assume_yes: bool,
    read_only: bool,
) -> Result<Session, Error> {
    let options = Options {
        message: command_line(),
        dry_run: read_only || leaf.dry_run(),
        assume_yes,
        color: color_enabled(leaf),
        prompt: Box::new(Terminal),
        sink: Some(Box::new(|line: &str| log::println(line))),
    };
    Session::open(config, options)
}

fn files_of(leaf: &Leaf, id: &str) -> Vec<String> {
    leaf.strings(id)
        .iter()
        .map(|f| file_uri_to_path(f))
        .collect()
}

/// The entries named on the command line, all without argument (`_entries_or_all`): their keys.
pub fn entries_or_all(session: &Session, entries: &[String]) -> Result<Vec<String>, Error> {
    if entries.is_empty() {
        return Ok(session.library.entries().map(|e| e.key.clone()).collect());
    }
    let args = read_selection(entries, &mut std::io::stdin()).map_err(Error::from)?;
    session.library.select(&args)
}

/// The mode of `--move`, `--copy`, `--link` of a command, if one is given.
fn mode_options(leaf: &Leaf, config: &Config) -> Result<Option<Mode>, Error> {
    if leaf.flag("move") || leaf.flag("copy") || leaf.flag("link") {
        return Ok(Some(files_mode(
            leaf.flag("link"),
            leaf.flag("move"),
            leaf.flag("copy"),
            config,
        )?));
    }
    Ok(None)
}

/// The extensions of the files that a `--key` value is likely to be.
const FILE_EXTENSIONS: [&str; 9] = [
    ".pdf", ".djvu", ".epub", ".ps", ".txt", ".zip", ".doc", ".docx", ".html",
];

/// The entries of the `-k/--key` option of `file detach` and `file rm` (`None` without it). As
/// the option takes several values, it swallows the files written after it: a value that is not
/// an entry but a path is an error that says so (`_key_option_entries`).
fn key_option_entries(session: &Session, keys: &[String]) -> Result<Option<Vec<String>>, Error> {
    if keys.is_empty() {
        return Ok(None);
    }
    let keys = read_selection(keys, &mut std::io::stdin()).map_err(Error::from)?;
    for key in &keys {
        let lower = key.to_lowercase();
        if (std::fs::symlink_metadata(key).is_ok()
            || FILE_EXTENSIONS.iter().any(|e| lower.ends_with(e)))
            && session.library.select(std::slice::from_ref(key)).is_err()
        {
            return Err(Error::exit(format!(
                "{key} is not an entry: put the files before --key (nothing was changed)"
            )));
        }
    }
    Ok(Some(session.library.select(&keys)?))
}

/// What a command prints of the entries it changed (`_print_biblio_diff`).
fn report(session: &Session, leaf: &Leaf, keys: &[String], notes: &Notes) {
    print_changes(
        &session.library,
        keys,
        false,
        notes,
        leaf.flag("porcelain"),
        session.color(),
        &session.say,
    );
}

/// The command to repair what `file check` reports (`_fix_command`): `-C DIR`, `--bibtex FILE`, the
/// selection, `--pool` and `--search`.
fn fix_command(leaf: &Leaf) -> String {
    let mut words = vec!["papers".to_string()];
    if let Some(directory) = directory_option() {
        words.push(format!("-C {directory}"));
    }
    words.push("file fix".to_string());
    if let Some(bibtex) = leaf.string("bibtex") {
        words.push(format!("--bibtex {bibtex}"));
    }
    words.extend(leaf.strings("entries"));
    words.extend(leaf.strings("pool").iter().map(|p| {
        format!(
            "--pool {}",
            short_home(&abspath(&papers_rs::config::paths::expanduser(p)))
        )
    }));
    words.extend(
        leaf.strings("search")
            .iter()
            .map(|d| format!("--search {}", short_home(&abspath(d)))),
    );
    words.join(" ")
}

/// The directory of the `-C DIR` options before the command name, as papers joins them.
fn directory_option() -> Option<String> {
    let mut args = std::env::args().skip(1).peekable();
    let mut directory: Option<String> = None;
    while let Some(arg) = args.peek().cloned() {
        if !arg.starts_with("-C") {
            break;
        }
        args.next();
        let d = if arg == "-C" {
            args.next()?
        } else {
            arg[2..].to_string()
        };
        directory = Some(match directory {
            Some(base) if !base.is_empty() => papers_rs::config::paths::join(&base, &d),
            _ => d,
        });
    }
    directory
}

/// A refused placement of files (a name outside the files directory, a template that cannot be
/// formatted): the errors are reported, the command still saved the library: exit 1.
pub fn refused(placed: bool) -> Done {
    if placed {
        Ok(())
    } else {
        Err(Error::exit("").into())
    }
}

fn usage(message: &str) -> Failure {
    Failure::Usage(message.to_string())
}

pub fn file_cmd(command: &str, leaf: &Leaf, config: Config) -> Done {
    let config = check_install(leaf, config, true)?;
    match command {
        "file rename" => rename(leaf, config),
        "file mv" => mv(leaf, config),
        "file attach" => attach(leaf, config),
        "file fetch" => fetch(leaf, config),
        "file detach" => detach(leaf, config),
        "file rm" => rm(leaf, config),
        "file check" => check(leaf, config, false),
        "file fix" => check(leaf, config, true),
        "file clean" => clean_cmd(leaf, config),
        "file checksum" => checksum(leaf, config),
        _ => Err(Error::exit(format!("{command}: not implemented in papers-rs yet")).into()),
    }
}

fn rename(leaf: &Leaf, config: Config) -> Done {
    let mode = mode_options(leaf, &config)?;
    let session = open(leaf, config, false, false)?;
    run_session(session, |session| {
        // in a shallow library, the files go to the pool
        session.use_pool_files()?;
        let keys = entries_or_all(session, &leaf.strings("entries"))?;
        let failed = rename_entries(session, &keys, leaf.flag("keep_source"), mode)?;
        let committed = session.commit()?;
        report(session, leaf, &keys, &committed.notes);
        refused(failed == 0)
    })
}

fn mv(leaf: &Leaf, config: Config) -> Done {
    let paths = files_of(leaf, "paths");
    if paths.len() < 2 {
        return Err(usage("papers file mv SRC... DEST"));
    }
    let options = MvOptions {
        force: leaf.flag("force"),
        filesdir_given: leaf.string("filesdir").is_some(),
    };
    let session = open(leaf, config, false, false)?;
    run_session(session, |session| {
        let (sources, dest) = paths.split_at(paths.len() - 1);
        move_files(session, sources, &dest[0], &options)?;
        Ok(())
    })
}

fn attach(leaf: &Leaf, config: Config) -> Done {
    let mode = files_mode(
        leaf.flag("link"),
        leaf.flag("move"),
        leaf.flag("copy"),
        &config,
    )?;
    let files = files_of(leaf, "files");
    let session = open(leaf, config, false, false)?;
    run_session(session, |session| {
        session.use_pool_files()?;
        let key = leaf.string("key").unwrap_or_default();
        let keys = session.library.select(std::slice::from_ref(&key))?;
        if keys.len() != 1 {
            return Err(
                Error::exit(format!("{key}: give one entry ({} given)", keys.len())).into(),
            );
        }
        let placed = attach_files(session, &keys[0], &files, &Attach::new(mode))?;
        let committed = session.commit()?;
        report(session, leaf, &keys, &committed.notes);
        refused(placed)
    })
}

fn fetch(leaf: &Leaf, config: Config) -> Done {
    let entries = leaf.strings("entries");
    let missing = leaf.flag("missing");
    if entries.is_empty() && !missing {
        return Err(usage("papers file fetch KEY... or --missing [KEY...]"));
    }
    let pdf = PdfOptions {
        least: leaf
            .string("least_version")
            .and_then(|v| Least::parse(&v))
            .unwrap_or_default(),
        supplements: leaf.flag("supplements"),
        again: leaf.flag("again"),
        access: openalex_access(),
    };
    let session = open(leaf, config, false, false)?;
    run_session(session, |session| {
        let keys = entries_or_all(session, &entries)?;
        let request = FileFetch {
            keys,
            missing,
            pdf,
            yes: leaf.flag("yes"),
        };
        let fetcher = Queries::default()
            .fetcher()
            .map_err(|e| Error::other(e.message))?;
        let fetched = file_fetch(session, &fetcher, &request)?;
        if let Some(committed) = &fetched.committed {
            if leaf.flag("porcelain") {
                report(session, leaf, &fetched.keys, &committed.notes);
            } else {
                for line in push_note_lines(&session.library, &committed.notes) {
                    session.say.line(line);
                }
            }
        }
        if missing && fetched.keys.is_empty() {
            session.say.line("every entry has a PDF");
        }
        refused(fetched.failed == 0)
    })
}

fn detach(leaf: &Leaf, config: Config) -> Done {
    let files = files_of(leaf, "files");
    let session = open(leaf, config, leaf.flag("yes"), false)?;
    run_session(session, |session| {
        let keys = key_option_entries(session, &leaf.strings("key"))?;
        if leaf.flag("broken") && !files.is_empty() {
            return Err(usage("--broken takes no FILE (--key limits it to entries)"));
        }
        if !leaf.flag("broken") && files.is_empty() && keys.is_none() {
            return Err(usage(
                "papers file detach FILE... [--key KEY...], --key KEY... or --broken [--key KEY...]",
            ));
        }
        detach_entries(
            session,
            keys.as_deref(),
            &files,
            leaf.flag("broken"),
            leaf.flag("recursive"),
        )?;
        Ok(())
    })
}

fn rm(leaf: &Leaf, config: Config) -> Done {
    let files = files_of(leaf, "files");
    let session = open(leaf, config, false, false)?;
    run_session(session, |session| {
        let keys = key_option_entries(session, &leaf.strings("key"))?;
        if files.is_empty() {
            return Err(usage("papers file rm FILE... [-r] [--key KEY...]"));
        }
        rm_files(session, keys.as_deref(), &files, leaf.flag("recursive"))?;
        Ok(())
    })
}

fn check(leaf: &Leaf, config: Config, fix: bool) -> Done {
    let options = CheckOptions {
        pools: leaf.strings("pool"),
        search: leaf.strings("search"),
        metadata: leaf.flag("metadata"),
        move_: leaf.flag("move"),
        fix_command: if fix {
            String::new()
        } else {
            fix_command(leaf)
        },
    };
    let session = open(leaf, config, false, !fix)?;
    run_session(session, |session| {
        if fix && !session.dry_run() {
            // a shallow library places the files it finds in its pool
            session.use_pool_files()?;
        }
        let keys = entries_or_all(session, &leaf.strings("entries"))?;
        check_entries_files(session, &options, fix, &keys, &Queries::default())?;
        Ok(())
    })
}

fn clean_cmd(leaf: &Leaf, config: Config) -> Done {
    if config.shallow() {
        // its filesdir setting is not used: the files are those of the pool, which other libraries link to too
        return Err(Error::exit(format!(
            "the files of a shallow library are in its pool: clean them there, with all the entries of the pool (`papers -C {} file clean`)",
            config.pool_label().unwrap_or_default()
        ))
        .into());
    }
    let session = open(leaf, config, false, false)?;
    run_session(session, |session| {
        clean(session, leaf.flag("yes"))?;
        Ok(())
    })
}

fn checksum(leaf: &Leaf, config: Config) -> Done {
    let session = open(leaf, config, false, false)?;
    run_session(session, |session| {
        let keys = entries_or_all(session, &leaf.strings("entries"))?;
        checksums(session, &keys, leaf.flag("check"))?;
        Ok(())
    })
}

/// The deprecated `papers filecheck`: its options run the `papers file` subcommands.
pub fn filecheck(leaf: &Leaf, config: Config) -> Done {
    let (fix_mendeley, hash_check, metadata_check) = (
        leaf.flag("fix_mendeley"),
        leaf.flag("hash_check"),
        leaf.flag("metadata_check"),
    );
    let (delete_broken, rename, clean_filesdir_option) = (
        leaf.flag("delete_broken"),
        leaf.flag("rename"),
        leaf.flag("clean_filesdir"),
    );
    let check = fix_mendeley
        || hash_check
        || metadata_check
        || !(delete_broken || rename || clean_filesdir_option);
    let mut new: Vec<String> = Vec::new();
    if check {
        if fix_mendeley || hash_check {
            new.push("papers file fix".to_string());
        }
        if metadata_check || !(fix_mendeley || hash_check) {
            new.push(format!(
                "papers file check{}",
                if metadata_check { " --metadata" } else { "" }
            ));
        }
    }
    if delete_broken {
        new.push("papers file detach --broken".to_string());
    }
    if rename {
        new.push(format!(
            "papers file rename{}",
            if leaf.flag("copy") {
                " --keep-source"
            } else {
                ""
            }
        ));
    }
    if clean_filesdir_option {
        new.push(format!(
            "papers file clean{}",
            if leaf.flag("force") { " --yes" } else { "" }
        ));
    }
    log::warning(&format!(
        "`papers filecheck` is deprecated: use {}",
        new.iter()
            .map(|c| format!("`{c}`"))
            .collect::<Vec<_>>()
            .join(" and ")
    ));
    let config = check_install(leaf, config, true)?;
    let session = open(leaf, config, false, false)?;
    run_session(session, |session| {
        let keys: Vec<String> = session.library.entries().map(|e| e.key.clone()).collect();
        if check {
            let queries = Queries::default();
            for key in &keys {
                for line in check_entry_lines(
                    session,
                    key,
                    metadata_check,
                    fix_mendeley || hash_check,
                    &queries,
                )? {
                    session.say.line(line);
                }
            }
        }
        if delete_broken {
            detach_broken_quietly(session)?;
        }
        if rename {
            // deprecated: moves the files, as before the files setting applied to the renames
            rename_entries(session, &keys, leaf.flag("copy"), Some(Mode::Move))?;
        }
        if clean_filesdir_option {
            session.say.line("Check files directory for unlinked files");
            let ignore = [session.config.bibtex.clone().unwrap_or_default()];
            clean_filesdir(session, !leaf.flag("force"), &ignore)?;
        }
        session.commit()?;
        Ok(())
    })
}
