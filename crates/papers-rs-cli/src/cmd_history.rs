//! `papers undo`, `redo`, `restore` (and the deprecated `restore-backup`) and `papers history`
//! (and the deprecated `backup`): moving in the history of a library, and what it keeps. The
//! history itself is `papers_rs::history`; here are the commands' options, questions and output
//! (`papers/cli/backup_cmds.py`).

use std::io::{BufRead, IsTerminal, Write};

use papers_rs::config::json::py_repr_str;
use papers_rs::config::paths::{abspath, dirname, exists, realpath};
use papers_rs::config::{Config, Origin, find_library_config};
use papers_rs::error::{Error, Result};
use papers_rs::history::listing::{
    Removal, all_lines, drop_files_plan, find_project, history_lines, prune_after_removal,
};
use papers_rs::history::report::{linked_files, unlinked_notice};
use papers_rs::history::store::to_system_trash;
use papers_rs::history::{History, Moved, Setup};
use papers_rs::lock::{self, Lock};
use papers_rs::log;
use serde_json::Value;

use crate::cmd_config::no_library_message;
use crate::parse::Leaf;
use crate::util::file_uri_to_path;

const NOT_IMPLEMENTED: &str = "not implemented in papers-rs yet";

/// `papers COMMAND ARGS`, as the command line was written (the message of a lock).
fn command_line() -> String {
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    format!("papers {}", args.join(" "))
}

// the question
// ------------

/// `_confirm`: ask on the terminal; without one, papers does not guess an answer.
fn confirm(question: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        return Err(Error::exit(format!("{question} no terminal to confirm: -y/--yes removes without asking")));
    }
    log::print(&format!("{question} [y/N] "));
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    let read = std::io::stdin().lock().read_line(&mut answer).unwrap_or(0);
    if read == 0 {
        // the EOFError of input()
        return Err(Error::exit(
            "papers asked a question, but there is no input to answer it: run the command in a terminal, or \
             answer with its options (e.g. --on-duplicate for duplicates, -y/--yes)",
        ));
    }
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

// the library of the command line
// -------------------------------

/// `config_for_bibtex`: with `--bibtex` of another library, that library's configuration applies
/// (its files directory, history and formats); without one, the current configuration applies to
/// the bibtex, never written back, and the history is that of the bibtex (named by its path).
fn config_for_bibtex(bibtex: &str, mut config: Config) -> Result<Config> {
    if config.bibtex.as_deref().is_some_and(|b| realpath(bibtex) == realpath(b)) {
        return Ok(config);
    }
    if let Some(file) = find_library_config(&dirname(&abspath(bibtex)))? {
        let mut other = Config::load(&file, config.machine.clone(), None)?;
        if other.bibtex.as_deref().is_some_and(|b| realpath(b) == realpath(bibtex)) {
            log::debug(&format!("configuration of {bibtex}: {file}"));
            other.origin = Some(Origin::Library);
            return Ok(other);
        }
    }
    log::debug(&format!(
        "{bibtex} has no configuration of its own: use {} without saving it",
        config.file.as_deref().map(py_repr_str).unwrap_or_else(|| "None".to_string())
    ));
    config.file = None;
    config.id = None;
    Ok(config)
}

/// `check_install` (the commands that act on the library of the command line): `--bibtex` and
/// `--filesdir` apply, and the bibtex must be there.
fn check_install(leaf: &Leaf, mut config: Config, bibtex_must_exist: bool) -> Result<Config> {
    if let Some(bibtex) = leaf.string("bibtex").map(|b| file_uri_to_path(&b)) {
        config = config_for_bibtex(&bibtex, config)?;
        config.bibtex = Some(bibtex);
    }
    if let Some(filesdir) = leaf.string("filesdir") {
        config.filesdir = Some(file_uri_to_path(&filesdir));
    }
    // the last of the two deprecated options wins
    let relative = leaf.index("relative_paths");
    let absolute = leaf.index("absolute_paths");
    if relative.is_some() || absolute.is_some() {
        let absolute = absolute > relative;
        let (option, setting) =
            if absolute { ("--absolute-paths", "absolute") } else { ("--relative-paths", "relative") };
        log::warning(&format!(
            "{option} is deprecated: file paths are a setting of the library, `papers config paths {setting}`"
        ));
        config.absolute_paths = absolute;
    }
    let Some(bibtex) = config.bibtex.clone() else {
        return Err(Error::exit(format!("{}, or pass --bibtex", no_library_message(&config))));
    };
    if bibtex_must_exist && !exists(&bibtex) {
        return Err(Error::exit(format!(
            "no bibtex file found, do `touch {bibtex}` or `papers init --bibtex {bibtex}`"
        )));
    }
    log::debug(&format!("bibtex: {}", py_repr_str(&bibtex)));
    log::debug(&format!(
        "filesdir: {}",
        config.filesdir.as_deref().map(py_repr_str).unwrap_or_else(|| "None".to_string())
    ));
    Ok(config)
}

// undo, redo, restore
// -------------------

/// `_time_travel`: undo, redo or restore, then say which state is restored, the files that moved,
/// the pools that followed, and which files of the files directory no entry links to any more.
/// `pushes`: after undo and redo of a shallow library with auto-push papers pushes what differs
/// from the pool, which is not ported: the command stops before it moves anything.
fn time_travel(
    kind: &str,
    leaf: &Leaf,
    config: Config,
    bibtex_must_exist: bool,
    pushes: bool,
    run: impl FnOnce(&mut History, Option<&Lock>, &str) -> Result<Moved>,
) -> Result<()> {
    let mut config = check_install(leaf, config, bibtex_must_exist)?;
    let dry_run = leaf.flag("dry_run");
    if pushes
        && config.shallow()
        && config.get("auto-push") == Value::Bool(true)
        && !dry_run
        && !leaf.flag("library_only")
    {
        return Err(Error::exit(format!("auto-push after {kind} in a shallow library: {NOT_IMPLEMENTED}")));
    }
    let command = command_line();
    let lock = if dry_run { None } else { lock::acquire(&config, &command, None)? };
    let bibtex = config.bibtex.clone().unwrap_or_default();
    let before = linked_files(&bibtex);
    let mut history = History::require(&mut config, dry_run)?;
    let moved = run(&mut history, lock.as_ref(), &command)?;
    for line in moved.lines(kind, &history) {
        log::println(&line);
    }
    if moved.files.is_none() {
        return Ok(());
    }
    for line in unlinked_notice(&before, &linked_files(&bibtex), config.filesdir.as_deref()) {
        log::println(&line);
    }
    Ok(())
}

fn int_option(leaf: &Leaf, id: &str) -> Option<i64> {
    leaf.string(id).and_then(|s| papers_rs::config::settings::py_int(&s))
}

/// A number of steps that Python's indexing turns into something else (a negative one, or none
/// for redo): papers-rs does not follow it.
fn odd_steps(n: i64) -> Error {
    Error::exit(format!("{n} steps: {NOT_IMPLEMENTED}"))
}

pub fn undo(leaf: &Leaf, config: Config) -> Result<()> {
    let n = int_option(leaf, "steps").unwrap_or(1);
    let library_only = leaf.flag("library_only");
    time_travel("undo", leaf, config, true, true, |h, lock, command| {
        if n < 0 {
            return Err(odd_steps(n));
        }
        h.undo(n as usize, library_only, lock, command)
    })
}

pub fn redo(leaf: &Leaf, config: Config) -> Result<()> {
    let n = int_option(leaf, "steps").unwrap_or(1);
    let library_only = leaf.flag("library_only");
    time_travel("redo", leaf, config, true, true, |h, lock, command| {
        if n <= 0 {
            return Err(odd_steps(n));
        }
        h.redo(n as usize, library_only, lock, command)
    })
}

pub fn restore(leaf: &Leaf, config: Config, command: &str) -> Result<()> {
    if command == "restore-backup" {
        log::warning("`papers restore-backup` is deprecated: use `papers restore`");
    }
    let state = int_option(leaf, "state").or_else(|| int_option(leaf, "reference"));
    time_travel("restore", leaf, config, false, false, |h, lock, _| match state {
        Some(n) if n < 0 => Err(no_step(&n.to_string())),
        n => h.restore(n.map(|n| n as u64), lock),
    })
}

fn no_step(step: &str) -> Error {
    Error::exit(format!("no step {step} in the history of the library (see `papers history`)"))
}

// history
// -------

pub struct HistoryArgs {
    remove: Option<String>,
    all: bool,
    drop_files: Option<String>,
    revert: bool,
    n: i64,
    yes: bool,
    dry_run: bool,
}

impl HistoryArgs {
    pub fn from_leaf(leaf: &Leaf) -> HistoryArgs {
        HistoryArgs {
            remove: leaf.matches.contains_id("remove").then(|| leaf.string("remove").unwrap_or_default()),
            all: leaf.flag("all"),
            drop_files: leaf.string("drop_files"),
            revert: leaf.string("revert").is_some(),
            n: int_option(leaf, "n").unwrap_or(20),
            yes: leaf.flag("yes"),
            dry_run: leaf.flag("dry_run"),
        }
    }
}

pub fn history(args: HistoryArgs, mut config: Config, command: &str) -> Result<()> {
    if command == "backup" {
        log::warning("`papers backup` is deprecated: use `papers history`");
    }
    if let Some(id) = &args.remove {
        return remove_history(&mut config, &args, id);
    }
    if args.all {
        for line in all_lines(&Setup::from_config(&config)) {
            log::println(&line);
        }
        return Ok(());
    }
    if let Some(target) = args.drop_files.as_deref().filter(|t| !t.is_empty()) {
        return drop_files(&mut config, &args, target);
    }
    if args.revert {
        // papers takes the step back with a three-way merge of the entries
        if config.bibtex.as_deref().is_none_or(|b| !exists(b)) {
            return Err(Error::exit("no library: --revert takes back a step of the history of a library"));
        }
        return Err(Error::exit(format!("history --revert: {NOT_IMPLEMENTED}")));
    }
    let has_bibtex = config.bibtex.is_some();
    let history = if has_bibtex { History::open(&mut config, false, args.dry_run)? } else { None };
    let setup = history.as_ref().map(|h| h.setup.clone()).unwrap_or_else(|| Setup::from_config(&config));
    for line in history_lines(&setup, history.as_ref(), has_bibtex, args.n) {
        log::println(&line);
    }
    Ok(())
}

/// `remove_history`: the history of this place (and of the library, when no other place uses it),
/// or of the library `id`.
fn remove_history(config: &mut Config, args: &HistoryArgs, id: &str) -> Result<()> {
    if !id.is_empty() {
        let setup = Setup::from_config(config);
        let (store, mut project) = find_project(&setup, id)?;
        let what = format!("the history of the library {id} ({} steps)", project.states().len());
        if !args.yes && !confirm(&format!("remove {what}?"))? {
            return Ok(());
        }
        project.remove();
        log::println(&format!("removed {what}"));
        prune_after_removal(&setup, &store, &to_system_trash);
        return Ok(());
    }
    let history = if config.bibtex.is_some() { History::open(config, false, args.dry_run)? } else { None };
    let Some(mut history) = history else {
        log::println(&Removal::NoHistory.message());
        return Ok(());
    };
    let removal = history.remove_place()?;
    match &removal {
        Removal::NoHistory | Removal::Place { .. } => log::println(&removal.message()),
        Removal::Project { .. } => {
            let what = removal.message();
            if !args.yes && !confirm(&format!("remove {what}?"))? {
                return Ok(());
            }
            history.remove_project();
            log::println(&format!("removed {what}"));
        }
    }
    Ok(())
}

/// `drop_files`: the files that step N deleted, or the file deleted from PATH, leave the history
/// (they go to the trash of the system).
fn drop_files(config: &mut Config, args: &HistoryArgs, target: &str) -> Result<()> {
    let history = if config.bibtex.is_some() { History::open(config, false, args.dry_run)? } else { None };
    let Some(mut history) = history.filter(|h| h.project.exists()) else {
        return Err(Error::exit("the library has no history"));
    };
    let is_step = target.chars().all(|c| c.is_ascii_digit());
    let found = if is_step {
        match target.parse::<u64>().ok().filter(|n| history.project.state(*n).is_some()) {
            Some(n) => history.files_to_drop(Some(n), None)?,
            None => return Err(no_step(target)),
        }
    } else {
        history.files_to_drop(None, Some(target))?
    };
    if found.is_empty() {
        let from = if is_step { format!("by step {target}") } else { format!("from {target}") };
        log::println(&format!("the history keeps no file deleted {from}"));
        return Ok(());
    }
    let plan = drop_files_plan(&found);
    for line in &plan.list {
        log::println(line);
    }
    if !args.yes && !confirm(&plan.question)? {
        return Ok(());
    }
    history.drop_files(&found.keys().cloned().collect())?;
    log::println(&plan.done);
    Ok(())
}
