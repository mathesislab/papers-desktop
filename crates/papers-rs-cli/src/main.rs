//! papers-rs: the command line of the Rust port of papers-cli.
//!
//! It behaves as `papers`: same options, same output, same exit status, same
//! files. The commands that are not ported yet say so (status 1).

mod cmd_add;
mod cmd_check;
mod cmd_config;
mod cmd_edit;
mod cmd_exchange;
mod cmd_export;
mod cmd_fetch;
mod cmd_file;
mod cmd_history;
mod cmd_init;
mod cmd_list;
mod cmd_list_actions;
mod cmd_list_files;
mod cmd_migrate;
mod cmd_rm;
mod cmd_status;
mod cmd_tag;
mod cmd_write;
mod commands;
mod dialogue;
mod help;
mod help_text;
mod parse;
mod spec;
mod textwrap;
mod util;

use std::io::Write;

use papers_rs::config::paths::{exists, isdir, isfile, join};
use papers_rs::config::{Config, MachineConfig, check_legacy_global_config, dirs, find_config};
use papers_rs::error::{Error, PAPERS_VERSION};
use papers_rs::log;

use crate::help::HelpValues;
use crate::parse::{Leaf, Parsed, Stop};

/// How the command line ends before the command has its result.
enum Exit {
    /// An error of papers: `papers: error: MESSAGE`, status 1 (nothing
    /// printed for an empty message).
    Error(Error),
    /// An error of the command line: usage and message, status 2.
    Usage { path: String, message: String },
    /// The text is printed on the standard output; status 0.
    Help(String),
    /// Status 1 after what the command printed (the help of `papers` run
    /// without command).
    Failed,
}

impl From<Error> for Exit {
    fn from(error: Error) -> Exit {
        Exit::Error(error)
    }
}

fn main() {
    let argv: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let code = match run(argv) {
        Ok(()) => 0,
        Err(Exit::Error(error)) => {
            if !error.message().is_empty() {
                log::error(error.message());
            }
            1
        }
        Err(Exit::Usage { path, message }) => {
            let prog = if path.is_empty() { "papers".to_string() } else { format!("papers {path}") };
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(stderr, "{}{prog}: error: {message}", help::usage(&path));
            2
        }
        Err(Exit::Help(text)) => {
            log::print(&text);
            0
        }
        Err(Exit::Failed) => 1,
    };
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}

/// The directory of the `-C DIR` options before the command name (several
/// `-C` combine as in git), and the remaining arguments.
fn pop_directory_options(mut argv: Vec<String>) -> Result<(Option<String>, Vec<String>), Error> {
    let mut directory: Option<String> = None;
    while argv.first().is_some_and(|a| a.starts_with("-C")) {
        let d = if argv[0] == "-C" {
            if argv.len() < 2 {
                return Err(Error::exit("-C requires a directory"));
            }
            let d = argv.remove(1);
            argv.remove(0);
            d
        } else {
            let first = argv.remove(0);
            first[2..].to_string()
        };
        directory = Some(match directory {
            Some(base) if !base.is_empty() => join(&base, &d),
            _ => d,
        });
    }
    Ok((directory, argv))
}

fn run(argv: Vec<String>) -> Result<(), Exit> {
    cmd_write::set_command_line(&argv);
    let (directory, argv) = pop_directory_options(argv)?;
    if let Some(directory) = &directory {
        if !isdir(directory) {
            return Err(Error::exit(format!("-C {directory}: not a directory")).into());
        }
        std::env::set_current_dir(directory).map_err(|e| Error::other(format!("{directory}: {e}")))?;
    }

    // before the library is loaded: a dry run moves nothing
    papers_rs::config::set_dry_run(argv.iter().any(|a| a == "--dry-run"));
    check_legacy_global_config();
    let config = find_config(".", MachineConfig::load(None)?)?;
    let values = HelpValues::from_config(&config);

    // the viewer passthrough of papers: files given instead of a command
    if !argv.is_empty()
        && argv.iter().all(|a| !a.starts_with('-'))
        && !commands::is_command(&argv[0])
        && argv.iter().all(|a| isfile(&util::file_uri_to_path(a)))
    {
        return Err(Error::exit("handing files to the system viewer: not implemented in papers-rs yet").into());
    }

    let parsed = parse::parse(&argv).map_err(|stop| match stop {
        Stop::Help(path) => Exit::Help(help::help(&path, &values)),
        Stop::Usage { path, message } => Exit::Usage { path, message },
    })?;
    if parsed.version {
        println!("{PAPERS_VERSION}");
        return Ok(());
    }
    if let Some(leaf) = &parsed.leaf {
        if leaf.flag("dry_run") {
            papers_rs::config::set_dry_run(true);
        }
        handle_logging(leaf);
        if leaf.flag("porcelain") {
            log::set_porcelain(true);
        }
    } else {
        log_level();
    }
    run_command(parsed, config, &values)
}

/// Verbosity: `-v` is INFO; of several levels, the most verbose applies.
fn handle_logging(leaf: &Leaf) {
    let mut levels: Vec<u8> = Vec::new();
    let last = [("debug", log::DEBUG), ("warn", log::WARNING), ("error", log::ERROR)]
        .into_iter()
        .filter_map(|(id, level)| leaf.index(id).map(|i| (i, level)))
        .max_by_key(|(i, _)| *i);
    if let Some((_, level)) = last {
        levels.push(level);
    }
    if leaf.flag("verbose") || leaf.flag("info") {
        levels.push(log::INFO);
    }
    if let Some(level) = levels.into_iter().min() {
        log::set_level(level);
    }
    log_level();
}

fn log_level() {
    let name = match log::level() {
        log::DEBUG => "DEBUG",
        log::INFO => "INFO",
        log::ERROR => "ERROR",
        _ => "WARNING",
    };
    log::debug(&format!("LOGGER LEVEL: {name}"));
}

/// The warnings of the deprecated and ignored options of the command.
fn option_warnings(command: &str, leaf: &Leaf) {
    const NO_GIT: &str = "papers keeps the history of the libraries without git (see `papers history`)";
    const NO_GIT_LFS: &str = "papers keeps the files that its commands delete in its history (see `papers history`)";
    let mut warnings: Vec<(usize, String)> = Vec::new();
    let mut add = |id: &str, text: String| {
        if let Some(i) = leaf.index(id) {
            warnings.push((i, text));
        }
    };
    add("info", "--info is deprecated: use -v".to_string());
    add("no_git", format!("--no-git is ignored: {NO_GIT}"));
    if command == "init" {
        add("git", format!("--git is ignored: {NO_GIT}"));
        add("git_lfs", format!("--git-lfs is ignored: {NO_GIT_LFS}"));
    }
    warnings.sort();
    for (_, text) in warnings {
        log::warning(&text);
    }
}

fn run_command(parsed: Parsed, config: Config, values: &HelpValues) -> Result<(), Exit> {
    let command = parsed.command.clone().unwrap_or_default();
    if let Some(leaf) = &parsed.leaf {
        option_warnings(&command, leaf);
    }
    // the global configuration of papers v2 still applies outside of libraries
    let v2_config = dirs::config_file();
    if exists(&v2_config) {
        let global_refused = parsed.leaf.as_ref().is_some_and(|l| l.flag("global_install"));
        if command != "migrate" && !(command == "install" && global_refused) {
            log::warning(&format!(
                "found the global configuration of papers v2 ({v2_config}): it still applies outside of libraries; `papers migrate` turns it into a library, the default library"
            ));
        }
    }
    let Some(leaf) = &parsed.leaf else {
        // papers without command, or `papers file` without subcommand: its help
        log::print(&help::help(&command, values));
        return Err(Exit::Failed);
    };
    match command.as_str() {
        "init" => cmd_init::init(cmd_init::InitArgs::from_leaf(leaf), config)?,
        "config" => match cmd_config::config_cmd(&cmd_config::ConfigArgs::from_leaf(leaf), config) {
            Ok(()) => {}
            Err(cmd_config::Failure::Usage(message)) => return Err(Exit::Usage { path: command, message }),
            Err(cmd_config::Failure::Error(error)) => return Err(error.into()),
        },
        "add" => written(&command, cmd_add::add_cmd(leaf, config))?,
        "edit" => written(&command, cmd_edit::edit_cmd(leaf, config))?,
        "rm" => written(&command, cmd_rm::rm_cmd(leaf, config))?,
        "tag" => written(&command, cmd_tag::tag_cmd(leaf, config))?,
        "check" => written(&command, cmd_check::check_cmd(leaf, config))?,
        "pull" | "push" | "sync" => written(&command, cmd_exchange::exchange_cmd(&command, leaf, config))?,
        "migrate" => written(&command, cmd_migrate::migrate_cmd(leaf, config))?,
        "filecheck" => written(&command, cmd_file::filecheck(leaf, config))?,
        file if file.starts_with("file ") => written(&command, cmd_file::file_cmd(file, leaf, config))?,
        "status" => cmd_status::status(cmd_status::StatusArgs::from_leaf(leaf), config)?,
        "undo" => cmd_history::undo(leaf, config)?,
        "redo" => cmd_history::redo(leaf, config)?,
        "restore" | "restore-backup" => cmd_history::restore(leaf, config, &command)?,
        "history" | "backup" => cmd_history::history(leaf, config, &command)?,
        "list" | "export" => {
            let result = if command == "list" { cmd_list::list_cmd(leaf, config) } else { cmd_export::export_cmd(leaf, config) };
            match result {
                Ok(()) => {}
                Err(cmd_config::Failure::Usage(message)) => return Err(Exit::Usage { path: command, message }),
                Err(cmd_config::Failure::Error(error)) => return Err(error.into()),
            }
        }
        "fetch" | "extract" | "doi" => {
            let outcome = match command.as_str() {
                "fetch" => cmd_fetch::fetch(leaf),
                "extract" => cmd_fetch::extract(leaf),
                _ => cmd_fetch::doi(leaf),
            };
            match outcome {
                Ok(()) => {}
                Err(cmd_fetch::Failure::Usage(message)) => return Err(Exit::Usage { path: command, message }),
                Err(cmd_fetch::Failure::Error(error)) => return Err(error.into()),
            }
        }
        _ => return Err(Error::exit("not implemented in papers-rs yet").into()),
    }
    Ok(())
}

/// The end of a command that writes a library: a usage error of its command line, or an error.
fn written(command: &str, outcome: cmd_write::Done) -> Result<(), Exit> {
    match outcome {
        Ok(()) => Ok(()),
        Err(cmd_write::Failure::Usage(message)) => Err(Exit::Usage { path: command.to_string(), message }),
        Err(cmd_write::Failure::Error(error)) => Err(error.into()),
    }
}
