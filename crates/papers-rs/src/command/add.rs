//! `papers add`: add the entries of files (bibtex, CSL-JSON, BibJSON), an entry
//! made of the fields given, or files to an entry that the library has
//! (`papers.cli.entry_cmds.addcmd`, `papers.bib.Biblio.insert_entry`).
//!
//! PDFs, folders (`-R`) and DOIs are in [`super::add_pdf`]; URLs as attachments are downloaded.
//! `add` looks in the pool of the library (or in the library of `--pool PATH`) first, unless `--no-pool`
//! (`pools::lookup`); in a shallow library the files go to the pool (`Session::use_pool_files`).
//! Not ported yet: Google Scholar (`--scholar`), an error that says so before anything is changed.

use std::collections::HashSet;

use crate::bib::Entry;
use crate::bib::encoding::standard_name;
use crate::config::json::py_repr_str;
use crate::config::paths::{abspath, dirname, isdir, realpath};
use crate::entry;
use crate::error::{Error, Result};
use crate::fetch::download::download_url;
use crate::fetch::openalex::OpenAlex;
use crate::formats::{Format, file_format, import::loads};
use crate::library::{InsertOptions, Inserted, Policy, What};
use crate::log;
use crate::merge::{Context, Decision, DuplicateError, Merge, OnDuplicate, Resolve};

use super::add_pdf::{self, Job};
use super::edit;
use super::files::{self, Mode};
use super::queries::Queries;
use super::session::{Session, not_implemented};

/// What the command line of `add` asks for.
#[derive(Debug, Clone)]
pub struct AddRequest {
    /// The files to add (`-` is not a file: `/dev/stdin` is).
    pub files: Vec<String>,
    /// `--format`: else from the extension.
    pub format: Option<String>,
    /// `--attach FILE...`: added to the files of the entry.
    pub attachments: Vec<String>,
    /// `--metadata FIELD=VALUE ...`, in order.
    pub metadata: Vec<(String, String)>,
    pub doi: Option<String>,
    pub key: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub journal: Option<String>,
    pub year: Option<String>,
    /// `--type`
    pub entry_type: Option<String>,
    /// `-u/--update-key`: a key from the key template for the entries new to the library.
    pub update_key: bool,
    pub check_duplicate: bool,
    /// What to do with the files of the entries ([`files::files_mode`]).
    pub mode: Mode,
    /// `-e/--edit`: the added entries in a text editor.
    pub edit: bool,
    pub ignore_errors: bool,
    pub recursive: bool,
    pub no_query_doi: bool,
    /// `--no-query-fulltext`: a PDF without a DOI is not looked up by its text.
    pub no_query_fulltext: bool,
    /// `--scholar`: Google Scholar (not ported).
    pub scholar: bool,
    /// `--openalex` (papers-rs, not in papers): the full-text search on OpenAlex, with how it is
    /// reached (the e-mail address, a key).
    pub openalex: Option<OpenAlex>,
    pub no_pool: bool,
    pub pool: Option<String>,
    /// Where the queries (DOIs, PDFs) go: the network.
    pub queries: Queries,
}

impl Default for AddRequest {
    fn default() -> AddRequest {
        AddRequest {
            files: Vec::new(),
            format: None,
            attachments: Vec::new(),
            metadata: Vec::new(),
            doi: None,
            key: None,
            title: None,
            author: None,
            journal: None,
            year: None,
            entry_type: None,
            update_key: false,
            check_duplicate: true,
            mode: Mode::Move,
            edit: false,
            ignore_errors: false,
            recursive: false,
            no_query_doi: false,
            no_query_fulltext: false,
            scholar: false,
            openalex: None,
            no_pool: false,
            pool: None,
            queries: Queries::default(),
        }
    }
}

/// A Python dict of the fields given: setting a name again keeps its place.
fn set_meta(metadata: &mut Vec<(String, String)>, name: &str, value: &str) {
    match metadata.iter_mut().find(|(k, _)| k == name) {
        Some((_, v)) => *v = value.to_string(),
        None => metadata.push((name.to_string(), value.to_string())),
    }
}

fn get_meta<'a>(metadata: &'a [(String, String)], name: &str) -> Option<&'a str> {
    metadata
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// The name of a field given on the command line, in the messages.
fn option_name(name: &str) -> &str {
    match name {
        "ID" => "key",
        "ENTRYTYPE" => "type",
        other => other,
    }
}

/// The error of a file that cannot be read, as Python words it.
fn os_error(error: &std::io::Error, path: &str) -> String {
    let message = crate::config::machine::io_message(error);
    match error.raw_os_error() {
        Some(code) => format!("[Errno {code}] {message}: {}", py_repr_str(path)),
        None => message,
    }
}

/// A resolver that remembers that it failed: an error of the duplicates stops
/// the command (`--ignore-errors` does not apply to it).
pub(super) struct Watch<'a> {
    inner: &'a mut dyn Resolve,
    failed: bool,
}

impl Watch<'_> {
    pub(super) fn failed(&self) -> bool {
        self.failed
    }
}

impl Resolve for Watch<'_> {
    fn mode(&self) -> OnDuplicate {
        self.inner.mode()
    }

    fn quit(&self) -> bool {
        self.inner.quit()
    }

    fn decide(
        &mut self,
        merge: Merge,
        context: Context,
        verdict: &crate::identity::Verdict,
    ) -> std::result::Result<Decision, DuplicateError> {
        let decision = self.inner.decide(merge, context, verdict);
        self.failed |= decision.is_err();
        decision
    }
}

/// What to say of the entries inserted, and their keys for the report: an
/// entry that is already there, one skipped.
pub(super) fn insert_messages(session: &Session, inserted: &[Inserted]) -> Vec<String> {
    let mut keys = Vec::new();
    for i in inserted {
        match &i.what {
            What::Merged {
                already_in_library: true,
            } => session.say.line(format!("already in library: {}", i.key)),
            What::Skipped => {
                session.say.line(format!(
                    "skipped: {} (already in library: {})",
                    i.key,
                    i.existing.as_deref().unwrap_or_default()
                ));
                continue;
            }
            _ => {}
        }
        keys.push(i.key.clone());
    }
    keys
}

/// The files of an entry for what is added to the library: `file` links given as
/// `metadata`, absolute.
fn metadata_files(metadata: &[(String, String)], relative_to: &str) -> Vec<String> {
    let mut holder = Entry::new("misc", "");
    if let Some(file) = get_meta(metadata, "file") {
        holder.set("file", file);
    }
    entry::files(&holder, Some(relative_to))
}

/// The first of `files` with the content of `file` (same size, then same checksum).
fn same_content(file: &str, files: &[String]) -> Option<String> {
    let size = std::fs::metadata(file).ok()?.len();
    let digest = crate::merge::checksum_hex(file).ok()?;
    files
        .iter()
        .find(|f| {
            std::fs::metadata(f.as_str()).is_ok_and(|m| m.is_file() && m.len() == size)
                && crate::merge::checksum_hex(f).ok().as_deref() == Some(digest.as_str())
        })
        .cloned()
}

/// `add` of files to a paper that the library has: the files are added to the
/// entry and placed as `file attach` does; its fields and other files are not touched
/// (`attach_to_existing`).
pub(super) fn attach_to_existing(
    session: &mut Session,
    key: &str,
    attachments: &[String],
    metadata: &[(String, String)],
    policy: Option<&Policy>,
) -> Result<Vec<String>> {
    let ignored: Vec<&str> = metadata
        .iter()
        .map(|(k, _)| k.as_str())
        .filter(|k| !matches!(*k, "file" | "doi"))
        .collect();
    if !ignored.is_empty() {
        let names: Vec<&str> = ignored.iter().map(|k| option_name(k)).collect();
        log::warning(&format!(
            "{key} is already in the library: its fields are not changed (ignored: {}; use `papers edit {key}`)",
            names.join(", ")
        ));
    }
    let old = session.library.files(key);
    let known: HashSet<String> = old.iter().map(|f| realpath(f)).collect();
    let new: Vec<String> = attachments
        .iter()
        .filter(|f| !known.contains(&realpath(f)))
        .map(|f| abspath(f))
        .collect();
    // a file with the content of a file of the entry is not attached again
    let same: Vec<(String, Option<String>)> = new
        .iter()
        .map(|f| (f.clone(), same_content(f, &old)))
        .collect();
    let new: Vec<String> = same
        .iter()
        .filter(|(_, other)| other.is_none())
        .map(|(f, _)| f.clone())
        .collect();
    if new.is_empty() {
        // otherwise the entry is reported Modified, with its new files
        session.say.line(format!("already in library: {key}"));
    }
    for (f, other) in &same {
        if let Some(other) = other {
            session.say.line(format!(
                "{f}: not attached, the entry has the same file: {other}"
            ));
        }
    }
    if new.is_empty() {
        return Ok(vec![key.to_string()]);
    }
    let all: Vec<String> = old.iter().cloned().chain(new).collect();
    session.library.set_files(key, &all)?;
    if let Some(policy) = policy {
        let policy = policy.clone();
        let wrapped = Policy::by_file(move |f: &str| {
            if known.contains(&realpath(f)) {
                crate::library::Action::Link
            } else {
                policy.action(f)
            }
        });
        session
            .library
            .rename_entry_files(key, &wrapped, None, None, true)?;
    }
    Ok(vec![key.to_string()])
}

/// The entries edited in a text editor, in place of the entries of these keys,
/// the fields emptied in the editor removed (`addcmd --edit`). An edit that is not valid
/// leaves the entries as they are.
fn edit_added(session: &mut Session, keys: &[String]) -> Result<Vec<String>> {
    let entries: Vec<Entry> = keys
        .iter()
        .filter_map(|k| session.library.get(k).cloned())
        .collect();
    let mut edited = match edit::edit_in_editor(&entries, &edit::Editor::of(&session.config)) {
        Ok(edited) => {
            log::info("successfully edited file, insert edited entries");
            edited
        }
        Err(error) => {
            log::error(error.message());
            entries
        }
    };
    for e in &mut edited {
        e.fields.retain(|f| !f.value.is_empty());
    }
    let new_keys: Vec<String> = edited.iter().map(|e| e.key.clone()).collect();
    session.library.replace_entries(keys, edited)?;
    Ok(new_keys)
}

/// Add what the request says to the library of the session (not saved: see
/// [`Session::commit`]); the keys of the entries that the report should consider.
pub fn add(
    session: &mut Session,
    request: &AddRequest,
    resolver: &mut dyn Resolve,
) -> Result<Vec<String>> {
    if request.pool.is_some() && request.no_pool {
        return Err(Error::exit("--pool PATH and --no-pool exclude each other"));
    }
    // in a shallow library, the files go to the pool, under the template of the pool
    session.use_pool_files()?;
    let mut source_dirs: Vec<String> = Vec::new();
    if !request.no_pool
        && let Some(pool) = session.start_pool_lookup(request.pool.as_deref())?
        && request.pool.is_some()
    {
        source_dirs.extend(session.exchange().pools.at(pool).filesdir());
    }
    let mut pool_dirs = session.pool_files_dirs();
    pool_dirs.extend(source_dirs);
    let in_pool = session.config.shallow()
        && session
            .library
            .filesdir
            .as_deref()
            .is_some_and(|d| pool_dirs.iter().any(|p| realpath(p) == realpath(d)));
    let mut config = session.config.clone();
    config.filesdir = session.library.filesdir.clone();
    let placement = files::files_policy_with(
        &config,
        request.mode,
        session.say.clone(),
        session.prompt(),
        &files::PolicyOptions {
            pool_dirs,
            in_pool,
            ..files::PolicyOptions::default()
        },
    );

    let mut metadata: Vec<(String, String)> = Vec::new();
    for (k, v) in &request.metadata {
        set_meta(&mut metadata, k, v);
    }
    for (name, value) in [
        ("doi", &request.doi),
        ("ID", &request.key),
        ("title", &request.title),
        ("author", &request.author),
        ("journal", &request.journal),
        ("year", &request.year),
        ("ENTRYTYPE", &request.entry_type),
    ] {
        if let Some(v) = value.as_deref().filter(|v| !v.is_empty()) {
            set_meta(&mut metadata, name, v);
        }
    }
    if let Some(author) = get_meta(&metadata, "author").map(standard_name) {
        set_meta(&mut metadata, "author", &author);
    }
    let mut attachments: Vec<String> = Vec::new();
    let mut downloaded = false;
    for a in &request.attachments {
        if a.starts_with("http://") || a.starts_with("https://") {
            attachments.push(download_url(a, false, None).map_err(Error::exit)?);
            downloaded = true;
        } else {
            attachments.push(a.clone());
        }
    }
    // downloaded attachments live in a temporary directory: they are moved into the files
    // directory whatever the mode
    let rename = request.mode != Mode::Link || downloaded;
    let policy = rename.then(|| placement.policy.clone());
    if !attachments.is_empty() {
        let relative_to = session.library.relative_to.clone();
        let files: Vec<String> = metadata_files(&metadata, &relative_to)
            .into_iter()
            .chain(attachments.clone())
            .collect();
        set_meta(
            &mut metadata,
            "file",
            &entry::format_files(&files, Some(&relative_to)),
        );
    }
    if !metadata.is_empty() && (request.files.len() > 1 || request.files.iter().any(|f| isdir(f))) {
        return Err(Error::exit(
            "--doi, --metadata, --key, --attach and the other metadata options are only valid for one PDF / BIBTEX entry",
        ));
    }

    let mut watch = Watch {
        inner: resolver,
        failed: false,
    };
    let options = InsertOptions {
        update_key: request.update_key,
        check_duplicate: request.check_duplicate,
        rename: policy.clone(),
        metadata: metadata.clone(),
    };
    let mut touched: Vec<String> = Vec::new();

    let check = || placement.check();
    let job = Job {
        search_doi: !request.no_query_doi,
        search_fulltext: !request.no_query_fulltext,
        scholar: request.scholar,
        openalex: request.openalex.as_ref(),
        doi: request.doi.as_deref().filter(|d| !d.is_empty()),
        check_duplicate: request.check_duplicate,
        options: &options,
        placement: &placement.policy,
        queries: &request.queries,
        check: &check,
    };

    if request.files.is_empty() && job.doi.is_some() && !request.no_query_doi {
        let doi = job.doi.unwrap_or_default();
        touched.extend(add_pdf::add_doi(
            session,
            doi,
            &attachments,
            &job,
            &mut watch,
        )?);
        placement.check()?;
    } else if request.files.is_empty() {
        let only_key = request.key.is_some()
            && !(request.edit
                || !request.metadata.is_empty()
                || request.doi.is_some()
                || request.title.is_some()
                || request.author.is_some()
                || request.journal.is_some()
                || request.year.is_some());
        if only_key {
            // --key alone: the entry of that key in the library
            let key = request.key.clone().unwrap_or_default();
            let others: Vec<(String, String)> = metadata
                .iter()
                .filter(|(k, _)| k != "ID")
                .cloned()
                .collect();
            let existing = session
                .library
                .entries()
                .find(|e| e.key.to_lowercase() == key.to_lowercase())
                .map(|e| e.key.clone());
            match existing {
                Some(existing) => {
                    touched.extend(attach_to_existing(
                        session,
                        &existing,
                        &attachments,
                        &others,
                        policy.as_ref(),
                    )?);
                    placement.check()?;
                }
                None => {
                    // the pool entry of that key, as it is written there
                    let taken = match (session.lookup.as_mut(), session.exchange.as_mut()) {
                        (Some(lookup), Some(exchange)) => {
                            let files: Vec<String> = attachments.iter().map(|f| abspath(f)).collect();
                            lookup.key(exchange, &session.library, &key, &files)?
                        }
                        _ => None,
                    };
                    let Some(taken) = taken else {
                        let where_ = if request.no_pool {
                            "the library (--no-pool)".to_string()
                        } else if let Some(pool) = session.lookup.as_ref().map(|l| l.pool) {
                            session.exchange().label(pool)
                        } else {
                            "the library (it has no pool)".to_string()
                        };
                        return Err(Error::exit(format!(
                            "nothing to add: no file, identifier or bibtex given, and no entry {key} in {where_}"
                        )));
                    };
                    let mut keyed = options.clone();
                    keyed.metadata = others;
                    touched.extend(add_pdf::insert_taken(session, taken, &keyed, &mut watch)?);
                    placement.check()?;
                }
            }
        } else {
            if !request.edit
                && request.metadata.is_empty()
                && request.key.is_none()
                && request.doi.is_none()
                && request.attachments.is_empty()
                && request.title.is_none()
                && request.author.is_none()
                && request.journal.is_none()
                && request.year.is_none()
            {
                return Err(Error::exit(
                    "No entry added: use --doi, --metadata, --key, --attachment, --title, --author, --journal, --year to add a new entry",
                ));
            }
            if get_meta(&metadata, "ID").is_none() {
                let mut sample = Entry::new("article", "");
                for (k, v) in metadata.iter().filter(|(k, _)| k != "ENTRYTYPE") {
                    sample.set(k, v.clone());
                }
                let key = session.library.keyformat.render(&sample)?;
                set_meta(&mut metadata, "ID", &key);
            }
            if get_meta(&metadata, "ENTRYTYPE").is_none() {
                set_meta(&mut metadata, "ENTRYTYPE", "article");
            }
            let mut entry = Entry::new(
                get_meta(&metadata, "ENTRYTYPE").unwrap_or("misc"),
                get_meta(&metadata, "ID").unwrap_or("unknown"),
            );
            for (k, v) in metadata
                .iter()
                .filter(|(k, _)| k != "ENTRYTYPE" && k != "ID")
            {
                entry.set(k, v.clone());
            }
            if request.edit {
                let edited = match edit::edit_in_editor(
                    &[entry.clone()],
                    &edit::Editor::of(&session.config),
                ) {
                    Ok(edited) => {
                        log::info("successfully edited file, insert edited entries");
                        edited
                    }
                    Err(error) => {
                        log::error(error.message());
                        vec![entry]
                    }
                };
                if edited.len() != 1 {
                    return Err(Error::exit(format!(
                        "the edited text has {} entries: add one entry at a time",
                        edited.len()
                    )));
                }
                entry = edited.into_iter().next().expect("one entry");
            }
            if entry.fields.iter().all(|f| f.key == "file") || entry.key.is_empty() {
                return Err(Error::exit(format!(
                    "nothing to add: the entry has no fields or no key{}",
                    if request.key.is_some() {
                        ""
                    } else {
                        " (use --title, --author... or --edit)"
                    }
                )));
            }
            // the entry is made of the metadata (and edited): not applied again
            let options = InsertOptions {
                metadata: Vec::new(),
                ..options.clone()
            };
            let inserted = session.library.insert(entry, &options, &mut watch)?;
            placement.check()?;
            touched.extend(insert_messages(session, std::slice::from_ref(&inserted)));
        }
    }

    // what is not ported yet: before anything is added
    if request.scholar
        && request.doi.is_none()
        && request.files.iter().any(|f| {
            (isdir(f) && request.recursive) || (!isdir(f) && f.to_lowercase().ends_with(".pdf"))
        })
    {
        return Err(not_implemented("add --scholar"));
    }

    for file in &request.files {
        let many = request.files.len() > 1 || (isdir(file) && request.recursive);
        let added = if isdir(file) && request.recursive {
            // the PDFs of a folder: their own DOI, never the one given
            let scan = Job {
                doi: None,
                scholar: false,
                openalex: None,
                ..job
            };
            add_pdf::scan_dir(session, file, &scan, &mut watch)
        } else if !isdir(file) && file.to_lowercase().ends_with(".pdf") {
            add_pdf::add_pdf(session, file, &attachments, &job, &mut watch)
        } else {
            let format = file_format(file, request.format.as_deref().and_then(Format::from_id));
            add_file(session, file, format, &attachments, &options, &mut watch)
        };
        match added {
            Ok(keys) => {
                placement.check()?;
                touched.extend(keys);
            }
            Err(error) => {
                if watch.failed() {
                    return Err(error);
                }
                if request.ignore_errors {
                    log::error(error.message());
                } else if many {
                    return Err(Error::exit(format!(
                        "{error}\nuse --ignore-errors to add other files anyway"
                    )));
                } else {
                    return Err(Error::exit(error.message()));
                }
            }
        }
    }

    if request.edit && !touched.is_empty() && !request.files.is_empty() {
        let mut seen = HashSet::new();
        let unique: Vec<String> = touched
            .iter()
            .filter(|k| seen.insert(k.to_lowercase()))
            .cloned()
            .collect();
        touched = edit_added(session, &unique)?;
    }
    session.finish_pool_lookup();
    Ok(touched)
}

/// The taker of the pool lookup of the session, if it has one.
fn taker_of<'a>(
    lookup: &'a mut Option<crate::pools::lookup::PoolLookup>,
    exchange: &'a mut Option<crate::pools::Exchange>,
) -> Option<crate::pools::lookup::PoolTaker<'a>> {
    match (lookup.as_mut(), exchange.as_mut()) {
        (Some(lookup), Some(exchange)) => Some(crate::pools::lookup::PoolTaker { lookup, exchange }),
        _ => None,
    }
}

/// The entries of one file added: bibtex, CSL-JSON or BibJSON.
pub(super) fn add_file(
    session: &mut Session,
    file: &str,
    format: Format,
    attachments: &[String],
    options: &InsertOptions,
    resolver: &mut dyn Resolve,
) -> Result<Vec<String>> {
    if isdir(file) {
        return Err(Error::exit(format!(
            "{file} is a directory, requires -R/--recursive to explore"
        )));
    }
    let bytes = std::fs::read(file).map_err(|e| Error::exit(os_error(&e, file)))?;
    let text = String::from_utf8(bytes).map_err(|e| Error::exit(format!("{e}")))?;
    let text = crate::bib::universal_newlines(&text);
    // file links in the added file are relative to its own directory
    let relative_to = dirname(&abspath(file));
    let inserted = if format == Format::Bibtex {
        let entries: Vec<Entry> = crate::bib::Bib::parse(&text).entries().cloned().collect();
        let mut taker = taker_of(&mut session.lookup, &mut session.exchange);
        session.library.add_entries_taking(
            entries,
            Some(&relative_to),
            attachments,
            "bibtex",
            options,
            resolver,
            taker.as_mut().map(|t| t as &mut dyn crate::library::Taker),
        )?
    } else {
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let entries = loads(text, format).map_err(Error::exit)?;
        let what = format!("{} file", format.label());
        let mut taker = taker_of(&mut session.lookup, &mut session.exchange);
        session.library.add_entries_taking(
            entries,
            Some(&relative_to),
            attachments,
            &what,
            options,
            resolver,
            taker.as_mut().map(|t| t as &mut dyn crate::library::Taker),
        )?
    };
    session.remove_stored_duplicates();
    Ok(insert_messages(session, &inserted))
}
