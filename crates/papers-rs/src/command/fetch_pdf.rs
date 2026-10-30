//! `papers file fetch` (also `papers add --fetch-pdf`): the free PDF of the entries, found by
//! [`crate::fetch::freepdf`] and attached as `papers file attach` attaches a downloaded file
//! (`papers.cli.file_cmds`, spec library.md "The free PDF of an entry").
//!
//! - An entry has a PDF when one of its links ends in `.pdf`, case-insensitively, and the file
//!   exists ([`has_pdf`]: a broken link is no PDF). Its main PDF is not fetched again, unless
//!   `again`; the supplements are still fetched.
//! - A downloaded file with the content of a file of the entry is not attached.
//! - Nothing about the download is written in the library. The machine keeps, in its cache, the
//!   sha256 and size of the file each URL gave: before a URL of that cache is downloaded again, a
//!   file of the entry with that content means nothing to attach; a file of another entry, or of
//!   `.papers/files.json`, with it is attached without a download (the file of another entry is
//!   linked where it is); else the URL is downloaded.
//! - The entries that got a file are saved in one step; the number of the entries that got no PDF
//!   is returned (an error is reported for each).

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::config::paths::{abspath, dirname, exists, getcwd, isfile, relpath, short_home};
use crate::entry::val;
use crate::error::{Error, Result};
use crate::fetch::Fetcher;
use crate::fetch::freepdf::{
    Found, How, Known, Least, Link, Search, discard, fetch_pdf, fetch_supplements, no_pdf_message,
};
use crate::fetch::openalex::OpenAlex;
use crate::history::FilesJson;
use crate::identity::{entry_arxiv, strip_doi};
use crate::library::Library;
use crate::log;

use super::add::same_content;
use super::file_cmd::{Attach, attach_files};
use super::files::{Mode, files_mode};
use super::session::{Committed, Session};

/// The entries of `--missing` with a DOI that may query OpenAlex, above which papers asks when no
/// API key is set (`OPENALEX_ASK`).
pub const OPENALEX_ASK: usize = 50;

/// The entry links to a PDF file that exists (`has_pdf`: a broken link is no PDF).
pub fn has_pdf(library: &Library, key: &str) -> bool {
    library
        .files(key)
        .iter()
        .any(|f| f.to_lowercase().ends_with(".pdf") && isfile(f))
}

/// A file in a message: relative to the current directory if under it, else with `~` (`_show`).
fn show(path: &str) -> String {
    let relative = relpath(&abspath(path), &getcwd());
    if relative.starts_with("..") {
        short_home(path)
    } else {
        relative
    }
}

/// The files of the library with the content of a PDF downloaded before (`LibraryChecksums`): of
/// the entry, of another entry, or of the checksums of the library (`.papers/files.json`). A file
/// is compared by size, then by checksum, each read once.
struct LibraryChecksums {
    hashes: BTreeMap<String, String>,
    sizes: HashMap<String, Option<u64>>,
    digests: HashMap<String, String>,
}

impl LibraryChecksums {
    fn new(session: &Session) -> LibraryChecksums {
        let libdir = dirname(&abspath(
            session.config.bibtex.as_deref().unwrap_or_default(),
        ));
        let hashes = FilesJson::of(&session.config, &libdir)
            .map(|f| f.load())
            .unwrap_or_default();
        LibraryChecksums {
            hashes,
            sizes: HashMap::new(),
            digests: HashMap::new(),
        }
    }

    fn matches(&mut self, f: &str, known: &Known) -> bool {
        let size = *self.sizes.entry(f.to_string()).or_insert_with(|| {
            std::fs::metadata(f)
                .ok()
                .filter(|m| m.is_file())
                .map(|m| m.len())
        });
        if size != Some(known.size) {
            return false;
        }
        let digest = self
            .digests
            .entry(f.to_string())
            .or_insert_with(|| crate::merge::checksum_hex(f).unwrap_or_default());
        *digest == known.sha256
    }

    /// The `local` of [`fetch_pdf`] for the entry `key`.
    fn local(&mut self, library: &Library, key: &str, link: &Link, known: &Known) -> Option<Found> {
        let found = |path: &str, how: How| {
            Some(Found {
                path: path.to_string(),
                link: link.clone(),
                how,
            })
        };
        for f in library.files(key) {
            if self.matches(&f, known) {
                return found(&f, How::Has);
            }
        }
        for other in library.entries() {
            if other.key == key {
                continue;
            }
            for f in library.files(&other.key) {
                if self.matches(&f, known) {
                    return found(&f, How::Linked(other.key.clone()));
                }
            }
        }
        let candidates: Vec<String> = self
            .hashes
            .iter()
            .filter(|(_, digest)| **digest == known.sha256)
            .map(|(f, _)| f.clone())
            .collect();
        for f in candidates {
            if self.matches(&f, known) {
                return found(&f, How::Known);
            }
        }
        None
    }
}

/// The line of a PDF attached (`_found_line`).
fn found_line(key: &str, path: &str, found: &Found) -> String {
    match &found.how {
        How::Linked(other) => format!(
            "{key}: {} ({}; same file as {other}, not downloaded)",
            show(path),
            found.link.describe()
        ),
        How::Known => format!(
            "{key}: {} ({}; a file of the library, not downloaded)",
            show(path),
            found.link.describe()
        ),
        _ => format!("{key}: {} ({})", show(path), found.link.describe()),
    }
}

/// What to fetch of each entry.
#[derive(Debug, Clone, Default)]
pub struct PdfOptions {
    /// `--version`: the least final version.
    pub least: Least,
    /// `--supplements`
    pub supplements: bool,
    /// `--again`: the main PDF of an entry that has one.
    pub again: bool,
    /// How OpenAlex is asked (`OPENALEX_API_KEY`, `PAPERS_MAILTO`).
    pub access: OpenAlex,
}

/// The DRYRUN lines of an entry (`_dry_run_lines`); false if no PDF would be found.
fn dry_run_lines(
    session: &Session,
    key: &str,
    search: &mut Search,
    local: &mut dyn FnMut(&Link, &Known) -> Option<Found>,
    fetcher: &Fetcher,
    main: bool,
    supplements: bool,
) -> bool {
    let (mut lines, mut ok) = (Vec::new(), true);
    if main {
        let links: Vec<Link> = search.links().collect();
        for (i, link) in links.iter().enumerate() {
            let found = fetcher
                .known_download(&link.url)
                .and_then(|known| local(link, &known));
            let what = match &found {
                Some(found) if found.how == How::Has => {
                    format!("already has {} (from {})", show(&found.path), link.host())
                }
                Some(found) => format!(
                    "would attach {} ({}; {})",
                    show(&found.path),
                    link.describe(),
                    match &found.how {
                        How::Linked(other) => format!("same file as {other}"),
                        _ => "a file of the library".to_string(),
                    }
                ),
                None => format!("would download {} ({})", link.url, link.describe()),
            };
            lines.push(format!(
                "DRYRUN: {key}: {}{what}",
                if i > 0 { "else " } else { "" }
            ));
        }
        if links.is_empty() {
            log::error(&format!("{key}: {}", no_pdf_message(search, &[])));
            ok = false;
        }
    }
    if supplements {
        for link in search.supplements() {
            lines.push(format!(
                "DRYRUN: {key}: would download {} ({})",
                link.url,
                link.describe()
            ));
        }
    }
    for line in lines {
        session.say.line(line);
    }
    ok
}

/// `papers file fetch` (also `papers add --fetch-pdf`) on the entries `keys` (`fetch_entry_pdfs`):
/// find the free PDF of each entry and attach it as `papers file attach` does, with the files mode
/// `mode`; the library is not saved. The main PDF of an entry that has a PDF is not fetched, unless
/// `again`; with `supplements`, the supplements are fetched too. A PDF with the content of a file
/// of the entry is not attached; one that this machine downloaded before (same URL) and that the
/// library has is attached without a download. `repaired`: the entries whose broken PDF link was
/// repaired (no main PDF fetched unless `again`, without message). The number of the entries whose
/// PDF was not found (the errors are reported).
pub fn fetch_entry_pdfs(
    session: &mut Session,
    fetcher: &Fetcher,
    keys: &[String],
    mode: Mode,
    options: &PdfOptions,
    repaired: &HashSet<String>,
) -> Result<usize> {
    let dry_run = session.dry_run();
    let mut library = LibraryChecksums::new(session);
    let mut failed = 0;
    let mut plans: Vec<(String, Vec<Found>)> = Vec::new();
    for key in keys {
        let Some(e) = session.library.get(key) else {
            continue;
        };
        let main = options.again || (!has_pdf(&session.library, key) && !repaired.contains(key));
        if !main && !repaired.contains(key) {
            session.say.line(format!(
                "{key}: has a PDF already (--again to fetch it anyway)"
            ));
            if !options.supplements {
                continue;
            }
        }
        let doi = strip_doi(val(e, "doi"));
        let arxiv = entry_arxiv(e);
        if doi.is_none() && arxiv.is_empty() {
            if main {
                log::error(&format!("{key}: no DOI or arXiv id to look for a PDF"));
                failed += 1;
            }
            continue;
        }
        let mut search = Search::new(
            fetcher,
            &options.access,
            doi.as_deref(),
            Some(arxiv.as_str()),
            options.least,
        );
        let current = &session.library;
        let mut local = |link: &Link, known: &Known| library.local(current, key, link, known);
        if dry_run {
            let ok = dry_run_lines(
                session,
                key,
                &mut search,
                &mut local,
                fetcher,
                main,
                options.supplements,
            );
            failed += usize::from(!ok);
            continue;
        }
        let mut found = Vec::new();
        if main {
            match fetch_pdf(&mut search, Some(&mut local)) {
                Ok(f) => found.push(f),
                Err(message) => {
                    log::error(&format!("{key}: {message}"));
                    failed += 1;
                }
            }
        }
        if options.supplements {
            found.extend(fetch_supplements(&mut search, Some(&mut local)));
        }
        plans.push((key.clone(), found));
    }

    // the downloads are done: the files are attached
    for (key, found) in plans {
        let mut new: Vec<Found> = Vec::new();
        for f in found {
            match f.how {
                How::Has => {
                    session.say.line(format!(
                        "{key}: already has {} (from {})",
                        show(&f.path),
                        f.link.host()
                    ));
                    continue;
                }
                How::Downloaded => {
                    let mut files = session.library.files(&key);
                    files.extend(new.iter().map(|g| g.path.clone()));
                    if let Some(same) = same_content(&f.path, &files) {
                        session.say.line(format!(
                            "{key}: {}: not attached, the entry has the same file: {}",
                            f.link.url,
                            show(&same)
                        ));
                        discard(&f.path);
                        continue;
                    }
                }
                _ => {}
            }
            new.push(f);
        }
        if new.is_empty() {
            continue;
        }
        // a file of another entry is linked where it is (shared, as add does); the others are placed
        let (linked, placed): (Vec<Found>, Vec<Found>) = new
            .into_iter()
            .partition(|f| matches!(f.how, How::Linked(_)));
        let mut files = session.library.files(&key);
        files.extend(linked.iter().map(|f| f.path.clone()));
        session.library.set_files(&key, &files)?;
        if !placed.is_empty() {
            let paths: Vec<String> = placed.iter().map(|f| f.path.clone()).collect();
            if !attach_files(session, &key, &paths, &Attach::new(mode))? {
                failed += 1;
            }
        }
        for f in &placed {
            // the temporary folder of a download, now empty
            if f.how == How::Downloaded && !exists(&f.path) {
                discard(&f.path);
            }
        }
        let all = session.library.files(&key);
        let count = linked.len() + placed.len();
        let paths = &all[all.len().saturating_sub(count)..];
        for (path, f) in paths.iter().zip(linked.iter().chain(&placed)) {
            session.say.line(found_line(&key, path, f));
        }
    }
    Ok(failed)
}

/// `--missing`: ask before more than [`OPENALEX_ASK`] entries with a DOI may query OpenAlex without
/// an API key (`yes` does not ask); without a terminal, an error (`_confirm_openalex`).
pub fn confirm_openalex(
    session: &Session,
    keys: &[String],
    yes: bool,
    access: &OpenAlex,
) -> Result<()> {
    let count = keys
        .iter()
        .filter(|k| {
            session
                .library
                .get(k)
                .is_some_and(|e| strip_doi(val(e, "doi")).is_some())
        })
        .count();
    if count <= OPENALEX_ASK || access.api_key.as_deref().is_some_and(|k| !k.is_empty()) || yes {
        return Ok(());
    }
    let question = format!(
        "{count} entries with a DOI: each may make a request to OpenAlex, which allows few requests a day without an API key (OPENALEX_API_KEY, free at https://openalex.org/settings/api)"
    );
    if !session.interactive() {
        return Err(Error::exit(format!(
            "{question}: no terminal to confirm; set OPENALEX_API_KEY, pass -y/--yes, or name the entries (nothing was changed)"
        )));
    }
    let answer = session.input(&format!("{question}. Continue? [y/N] "))?;
    if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
        return Err(Error::exit("nothing was changed"));
    }
    Ok(())
}

/// What `papers file fetch` is asked.
#[derive(Debug, Clone, Default)]
pub struct FileFetch {
    /// The entries named (all of the library for `--missing` without them).
    pub keys: Vec<String>,
    /// `--missing`: the entries without PDF.
    pub missing: bool,
    pub pdf: PdfOptions,
    /// `-y/--yes`: no question before many entries.
    pub yes: bool,
}

/// What `papers file fetch` did.
#[derive(Debug, Default)]
pub struct Fetched {
    /// The entries of the command (without those that have a PDF, with `--missing`).
    pub keys: Vec<String>,
    /// The entries that got no PDF.
    pub failed: usize,
    /// The save (none with a dry run).
    pub committed: Option<Committed>,
}

/// `papers file fetch` (`file_fetchcmd`): the free PDF of the entries, attached and saved in one
/// step (in a shallow library, the files go to its pool).
pub fn file_fetch(
    session: &mut Session,
    fetcher: &Fetcher,
    request: &FileFetch,
) -> Result<Fetched> {
    let dry_run = session.dry_run();
    if !dry_run {
        session.use_pool_files()?;
    }
    let mut keys = request.keys.clone();
    if request.missing {
        keys.retain(|k| !has_pdf(&session.library, k));
    }
    if session.config.filesdir.is_none() {
        return Err(Error::exit(
            "the library has no files directory, where the downloaded PDFs go (pass --filesdir, or set it with `papers config filesdir DIR`)",
        ));
    }
    let mode = files_mode(false, false, false, &session.config)?;
    if request.missing {
        confirm_openalex(session, &keys, request.yes, &request.pdf.access)?;
    }
    let failed = fetch_entry_pdfs(session, fetcher, &keys, mode, &request.pdf, &HashSet::new())?;
    let committed = if dry_run {
        None
    } else {
        Some(session.commit()?)
    };
    Ok(Fetched {
        keys,
        failed,
        committed,
    })
}
