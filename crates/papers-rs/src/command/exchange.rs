//! `papers pull`, `papers push`, `papers sync` (`papers.cli.pool_cmds`, spec `exchange.md`
//! sections 6 to 11): the entries of a library and of its pool (or of another library, `--pool PATH`)
//! are exchanged by a three-way merge with the origin records. Operations on a [`Session`]: the lines go
//! to its `Say`, a question to its prompt; the program passes the [`Resolve`] that decides the conflicts.

use crate::command::pool::{push_note_lines, report_counts};
use crate::command::report::format_entry;
use crate::error::{Error, Result};
use crate::merge::Resolve;
use crate::pools::{Outcome, PoolError, PoolIx, What, removed_note};

use super::session::Session;

/// What the command line of `pull`, `push` and `sync` asks for.
#[derive(Debug, Clone, Default)]
pub struct ExchangeArgs {
    /// The identifiers given (`-` already read).
    pub entries: Vec<String>,
    /// The command line named entries (or `-`): a pull of named entries does not set the exchange time.
    pub given: bool,
    pub all: bool,
    /// `--pool PATH`: the other library.
    pub pool: Option<String>,
    /// `pull --check`
    pub check: bool,
    /// `papers list ... --pull`: the entries listed.
    pub selected: Option<Vec<String>>,
}

impl ExchangeArgs {
    /// Without KEY or `--all`, pull, push and sync only exchange the entries shared with the pool.
    fn nothing_shared(&self) -> bool {
        !self.given && !self.all && self.selected.is_none()
    }

    /// The `--pool` option of the command, to repeat in a message.
    fn options(&self) -> String {
        match &self.pool {
            Some(pool) => format!(" --pool {pool}"),
            None => String::new(),
        }
    }
}

fn stopped(error: PoolError) -> Error {
    Error::exit(format!("{error} (nothing was changed)"))
}

/// The pools of an exchange, made before anything is read: an error if there is no `--pool PATH` and
/// the library has no pool. A shallow library that exchanges with another library than its pool
/// places the files it takes in its pool, as `add` does.
pub fn prepare(session: &mut Session, args: &ExchangeArgs) -> Result<()> {
    if args.pool.is_none() && !session.config.has_pool() {
        return Err(Error::exit(crate::pools::NO_POOL_MESSAGE));
    }
    if args.pool.is_some() && session.config.shallow() {
        session
            .use_pool_files()
            .map_err(|e| Error::exit(format!("{e} (nothing was changed)")))?;
    }
    Ok(())
}

/// The pool of the command: `--pool PATH`, else the library's; an error if it is not a library.
fn source(session: &mut Session, args: &ExchangeArgs) -> Result<PoolIx> {
    let exchange = session.exchange();
    match &args.pool {
        Some(path) => exchange.pools.get(path),
        None => exchange.pools.target(),
    }
    .map_err(stopped)
}

/// The library entries to update, the pool entries to add and the library entries named, for
/// `pull KEY... [--all]`: each identifier is looked up in the library first, then in the pool; one
/// found in neither is an error (raised before anything changes). A pool entry that is in the
/// library is an update of that entry.
fn pull_selection(
    session: &mut Session,
    pool: PoolIx,
    args: &ExchangeArgs,
) -> Result<(Vec<String>, Vec<u64>, Vec<String>)> {
    let exchange = session.exchange.as_mut().expect("the exchange");
    let library = &mut session.library;
    let (mut entries, missing) = if args.entries.is_empty() {
        (
            library.entries().map(|e| e.key.clone()).collect(),
            Vec::new(),
        )
    } else {
        library.select_some(&args.entries)?
    };
    let mut named = if args.entries.is_empty() {
        Vec::new()
    } else {
        entries.clone()
    };
    let mut new: Vec<u64> = Vec::new();
    if !missing.is_empty() {
        let keys = exchange.pools.at_mut(pool).library()?.select(&missing)?;
        for key in keys {
            let uid = exchange
                .pools
                .at_mut(pool)
                .library()?
                .uid_of_key(&key)
                .expect("a selected entry");
            let pe = exchange
                .pools
                .at_mut(pool)
                .library()?
                .entry_by_uid(uid)
                .expect("an entry")
                .clone();
            match exchange.library_entry(library, &pe, pool)? {
                Some(e) => {
                    if !entries.contains(&e) {
                        entries.push(e.clone());
                        named.push(e);
                    }
                }
                None => {
                    if !new.contains(&uid) {
                        new.push(uid);
                    }
                }
            }
        }
    }
    if args.all {
        for uid in exchange.missing(library, pool)? {
            if !new.contains(&uid) {
                new.push(uid);
            }
        }
    }
    Ok((entries, new, named))
}

/// The entries shared with the pool among `entries` (for an exchange without KEY or `--all`). The entries
/// removed from the pool are left out: their note is printed, and their origin records are dropped (they
/// are no longer shared, and the note comes once).
fn shared(
    session: &mut Session,
    entries: &[String],
    pool: PoolIx,
    args: &ExchangeArgs,
) -> Result<Vec<String>> {
    let exchange = session.exchange.as_mut().expect("the exchange");
    let (shared, removed) = exchange.sharing(&session.library, entries, pool)?;
    for key in &removed {
        let label = exchange.label(pool);
        session.say.line(removed_note(key, &label, &args.options()));
    }
    exchange.forget(&session.library, &removed, pool);
    Ok(shared)
}

/// A shallow library that took entries from another library than its pool: they go to its pool too,
/// as with `add`.
fn forward(session: &mut Session, pool: PoolIx) -> Result<()> {
    if !session.config.shallow() || session.exchange().is_own(pool) {
        return Ok(());
    }
    session.auto_push()
}

/// The pull of `pull` and `sync`: the report, saved; the notes of the push to the pool of a shallow
/// library are in the committed result.
fn pull_round(
    session: &mut Session,
    args: &ExchangeArgs,
    resolver: &mut dyn Resolve,
    pool: PoolIx,
) -> Result<(Vec<Outcome>, super::session::Committed)> {
    let (mut entries, new, named) = match &args.selected {
        Some(selected) => (selected.clone(), Vec::new(), selected.clone()),
        None => pull_selection(session, pool, args)?,
    };
    if args.nothing_shared() {
        entries = shared(session, &entries, pool, args)?;
    }
    let report = {
        let exchange = session.exchange.as_mut().expect("the exchange");
        exchange.pull(
            &mut session.library,
            &entries,
            Some(pool),
            resolver,
            None,
            !args.given && args.selected.is_none(),
            &new,
            &named,
        )?
    };
    forward(session, pool)?;
    let committed = session.commit()?;
    Ok((report, committed))
}

/// The lines of the report of pull or push (`exchange_lines`): one per entry added, modified, skipped or
/// not found, and the line of the counts or none. The counts (the entries unchanged aside) are given when
/// more than one entry changed or was not exchanged, or when there is no entry line; without any, the line
/// says there was nothing to exchange.
fn exchange_lines(
    session: &mut Session,
    report: &[Outcome],
    pull: bool,
    pool: Option<PoolIx>,
) -> (Vec<String>, Option<String>) {
    let label = pool.map(|p| session.exchange().label(p));
    let color = session.color();
    let mut lines = Vec::new();
    for o in report {
        let (prefix, note) = match (pull, o.what) {
            (true, What::Added) => ("Added", "from {}"),
            (true, What::Updated) => ("Modified", "from {}"),
            (true, What::Skipped) => ("Conflict", "skipped, not taken from {}"),
            (true, What::NotFound) => ("Not found", "not in {}"),
            (false, What::Added) => ("Added", "to {}"),
            (false, What::Updated) => ("Modified", "in {}"),
            (false, What::Skipped) => ("Conflict", "skipped, not pushed to {}"),
            (false, What::Empty) => ("Not pushed", "no fields or no key, not pushed to {}"),
            _ => continue,
        };
        let where_ = match o.pool {
            Some(p) => session.exchange().label(p),
            None => label.clone().unwrap_or_else(|| "the pool".to_string()),
        };
        let Some(e) = session.library.get(&o.key) else {
            continue;
        };
        lines.push(format!(
            "{} [{}]",
            format_entry(&session.library.relative_to, e, false, prefix, color),
            note.replace("{}", &where_)
        ));
    }
    let counted = report.iter().filter(|o| o.what != What::Unchanged).count();
    let mut summary = None;
    if counted > 1 || lines.is_empty() {
        let kind = if pull { "pull" } else { "push" };
        let verb = match &label {
            None => kind.to_string(),
            Some(label) => format!("{kind} {} {label}", if pull { "from" } else { "to" }),
        };
        let counts = report_counts(report);
        summary = Some(if counts.is_empty() {
            format!("nothing to {verb}")
        } else {
            format!("{verb}: {counts}")
        });
    }
    (lines, summary)
}

fn print_exchange(session: &mut Session, report: &[Outcome], pull: bool, pool: Option<PoolIx>) {
    let (mut lines, summary) = exchange_lines(session, report, pull, pool);
    lines.extend(summary);
    session.say.line(lines.join("\n"));
}

fn print_notes(session: &Session, notes: &std::collections::HashMap<String, String>) {
    for line in push_note_lines(&session.library, notes) {
        session.say.line(line);
    }
}

/// `papers pull` (also `papers list --pull`, with the listed entries as `selected`).
pub fn pull(session: &mut Session, args: &ExchangeArgs, resolver: &mut dyn Resolve) -> Result<()> {
    session.no_auto_push();
    prepare(session, args)?;
    let pool = source(session, args)?;
    if args.check {
        let keys: Vec<String> = if args.entries.is_empty() {
            session.library.entries().map(|e| e.key.clone()).collect()
        } else {
            session.library.select(&args.entries)?
        };
        let keys = if args.nothing_shared() {
            let exchange = session.exchange.as_mut().expect("the exchange");
            let (shared, removed) = exchange.sharing(&session.library, &keys, pool)?;
            keys.into_iter()
                .filter(|k| shared.contains(k) || removed.contains(k))
                .collect()
        } else {
            keys
        };
        let exchange = session.exchange.as_mut().expect("the exchange");
        let lines = exchange.check(
            &session.library,
            &keys,
            pool,
            args.entries.is_empty(),
            &args.options(),
        )?;
        session.say.line(if lines.is_empty() {
            "nothing to pull".to_string()
        } else {
            lines.join("\n")
        });
        return Ok(());
    }
    let (report, committed) = pull_round(session, args, resolver, pool)?;
    print_exchange(session, &report, true, Some(pool));
    print_notes(session, &committed.notes);
    Ok(())
}

/// The entries to push: those named (or selected), all of them with `--all`, else the entries shared
/// with the pool.
fn push_entries(session: &mut Session, args: &ExchangeArgs, pool: PoolIx) -> Result<Vec<String>> {
    if let Some(selected) = &args.selected {
        return Ok(selected.clone());
    }
    if args.given {
        return if args.entries.is_empty() {
            Ok(session.library.entries().map(|e| e.key.clone()).collect())
        } else {
            session.library.select(&args.entries)
        };
    }
    let all: Vec<String> = session.library.entries().map(|e| e.key.clone()).collect();
    if args.all {
        return Ok(all);
    }
    shared(session, &all, pool, args)
}

/// The push of `push` and `sync`: the report. Without KEY or `--all`, only the entries shared with the pool
/// are pushed: only the origin records of the entries removed from the pool are written when there are none.
fn push_round(
    session: &mut Session,
    args: &ExchangeArgs,
    resolver: &mut dyn Resolve,
    pool: PoolIx,
) -> Result<Vec<Outcome>> {
    let nothing_shared = args.nothing_shared();
    let entries = push_entries(session, args, pool)?;
    if entries.is_empty() && nothing_shared {
        if session.exchange().origin.changed() {
            session.commit()?;
        }
        return Ok(Vec::new());
    }
    let report = {
        let exchange = session.exchange.as_mut().expect("the exchange");
        exchange.push(&mut session.library, &entries, pool, resolver, true)?
    };
    session.commit()?;
    Ok(report)
}

/// `papers push` (also `papers list --push`).
pub fn push(session: &mut Session, args: &ExchangeArgs, resolver: &mut dyn Resolve) -> Result<()> {
    session.no_auto_push();
    prepare(session, args)?;
    let pool = source(session, args)?;
    let report = push_round(session, args, resolver, pool)?;
    if report.is_empty() && args.nothing_shared() {
        let (label, options) = (session.exchange().label(pool), args.options());
        session.say.line(format!(
            "push to {label}: no entries shared with it (`papers push{options} KEY` or `papers push{options} --all` adds entries to it)"
        ));
        return Ok(());
    }
    print_exchange(session, &report, false, Some(pool));
    Ok(())
}

/// `papers sync` (also `papers list --sync`): pull, then push the same entries (without KEY or `--all`, the
/// entries shared with the pool); stop before pushing if the pull left conflicts undecided. One line when
/// nothing changed. `pull_resolver` and `push_resolver` decide the conflicts of each step (the same mode).
pub fn sync(
    session: &mut Session,
    args: &ExchangeArgs,
    pull_resolver: &mut dyn Resolve,
    push_resolver: &mut dyn Resolve,
) -> Result<()> {
    session.no_auto_push();
    prepare(session, args)?;
    let pool = source(session, args)?;
    let (pull_report, committed) = pull_round(session, args, pull_resolver, pool)?;
    print_notes(session, &committed.notes);
    let pull_report: Vec<Outcome> = pull_report
        .into_iter()
        .filter(|o| o.what != What::NotFound)
        .collect();
    let (pull_lines, pull_summary) = exchange_lines(session, &pull_report, true, Some(pool));
    if !pull_lines.is_empty() {
        let mut text = pull_lines.clone();
        text.extend(pull_summary.clone());
        session.say.line(text.join("\n"));
    }
    let skipped: Vec<String> = pull_report
        .iter()
        .filter(|o| o.what == What::Skipped)
        .map(|o| o.key.clone())
        .collect();
    if !skipped.is_empty() || pull_resolver.quit() {
        return Err(Error::exit(format!(
            "not pushed: the pull left conflicts undecided ({}). Decide them with `papers pull`, then run `papers sync` again",
            if skipped.is_empty() {
                "quit".to_string()
            } else {
                skipped.join(", ")
            }
        )));
    }
    session.exchange().next_round();
    let push_report = push_round(session, args, push_resolver, pool)?;
    let (push_lines, push_summary) = exchange_lines(session, &push_report, false, Some(pool));
    let pull_counted = report_counts(&pull_report);
    let push_counted = report_counts(&push_report);
    if !pull_lines.is_empty()
        || !push_lines.is_empty()
        || !pull_counted.is_empty()
        || !push_counted.is_empty()
    {
        if pull_lines.is_empty() && !pull_counted.is_empty() {
            session.say.line(pull_summary.clone().unwrap_or_default());
        }
        if !push_lines.is_empty() || !push_counted.is_empty() {
            let mut text = push_lines;
            text.extend(push_summary);
            session.say.line(text.join("\n"));
        }
        return Ok(());
    }
    let involved = !pull_report.is_empty() || !push_report.is_empty();
    let label = session.exchange().label(pool);
    if !involved && args.nothing_shared() {
        let options = args.options();
        session.say.line(format!(
            "nothing to sync with {label}: no entries shared with it (`papers sync{options} --all` adds the entries missing on each side)"
        ));
    } else {
        session.say.line(format!("nothing to sync with {label}"));
    }
    Ok(())
}
