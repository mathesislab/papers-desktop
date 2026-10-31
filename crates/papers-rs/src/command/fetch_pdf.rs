//! `papers file fetch` (also `papers add --fetch-pdf`): the free PDF of the entries, found by
//! [`crate::fetch::freepdf`] and attached as `papers file attach` attaches a downloaded file
//! (`papers.cli.file_cmds`, spec library.md "The free PDF of an entry").
//!
//! - An entry has a PDF when one of its links ends in `.pdf`, case-insensitively, and the file
//!   exists ([`has_pdf`]: a broken link is no PDF). Its main PDF is not fetched again, unless
//!   `again`; the supplements are still fetched.
//! - A downloaded file with the content of a file of the entry is not attached.
//! - Nothing else about the download is written in the library. The machine keeps, in its cache, the
//!   sha256 and size of the file each URL gave: before a URL of that cache is downloaded again, a
//!   file of the entry with that content means nothing to attach; a file of another entry, or of
//!   `.papers/files.json`, with it is attached without a download (the file of another entry is
//!   linked where it is); else the URL is downloaded.
//! - `file fetch`: the local repair first. An entry without PDF that has a broken link ending in
//!   `.pdf` first gets the repair of its broken links that `file fix` makes ([`repair_pdf_links`]);
//!   an entry that has a PDF after it is not downloaded (unless `again`). `add --fetch-pdf` makes no
//!   repair.
//! - The entries that got a file, relinked or downloaded, are saved in one step; the number of the
//!   entries that got no PDF is returned (an error is reported for each).
//! - An accepted manuscript or a submitted version attached after the published version was
//!   refused names the published links to open in a browser ([`Found::describe`]).
//! - A fetch of several entries ends with a summary ([`FetchSummary`], [`Fetched::summary`]).
//! - The version of a PDF attached (not a supplement) is written in `.papers/files.json`, by its
//!   checksum, at the save ([`Library::pdf_versions`]).
//! - `papers file fetch --upgrade` ([`file_upgrade`]): the published version in place of a PDF
//!   attached as an accepted manuscript or a submitted version, under the same name.
//!
//! `papers fetch --pdf` ([`fetch_free_pdf`]) writes the free PDF of a DOI or an arXiv id to a file,
//! without a library; with `--porcelain`, its lines for programs ([`pdf_porcelain_lines`]).
//! An existing `-o FILE` is not replaced, and nothing is looked up ([`NotWritten::Exists`]).

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::bib::pystr::strip;
use crate::config::paths::{
    abspath, basename, dirname, exists, getcwd, isdir, isfile, join, normpath, relpath, short_home,
};
use crate::entry::val;
use crate::error::{Error, Result};
use crate::fetch::Fetcher;
use crate::fetch::freepdf::{
    Found, How, Known, Least, Link, NoPdf, Search, Version, discard, fetch_pdf, fetch_supplements,
    no_pdf_message,
};
use crate::fetch::openalex::OpenAlex;
use crate::history::FilesJson;
use crate::history::restore::move_path;
use crate::identity::{entry_arxiv, normalize_arxiv, strip_doi};
use crate::library::Library;
use crate::log;
use crate::names::Format;

use super::add::same_content;
use super::file_cmd::{Attach, Relinked, attach_files, broken_links, relink_broken_links};
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

/// The checksums file of the library of the session (none for a bibtex without library).
fn files_json(session: &Session) -> Option<FilesJson> {
    let libdir = dirname(&abspath(
        session.config.bibtex.as_deref().unwrap_or_default(),
    ));
    FilesJson::of(&session.config, &libdir)
}

impl LibraryChecksums {
    fn new(session: &Session) -> LibraryChecksums {
        let hashes = files_json(session).map(|f| f.load()).unwrap_or_default();
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
        let found = |path: &str, how: How| Some(Found::new(path, link.clone(), how));
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
            found.describe()
        ),
        How::Known => format!(
            "{key}: {} ({}; a file of the library, not downloaded)",
            show(path),
            found.describe()
        ),
        _ => format!("{key}: {} ({})", show(path), found.describe()),
    }
}

/// Why an entry has links to open in a browser, in the summary of `papers file fetch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unattached {
    /// No PDF.
    NoPdf,
    /// The published version was refused; this version was found.
    Refused(Version),
    /// `--upgrade`: no published version (the PDF is kept as it is) (`NO_PUBLISHED`).
    NoPublished,
}

impl Unattached {
    fn text(self) -> String {
        match self {
            Unattached::NoPdf => "no PDF".to_string(),
            Unattached::Refused(version) => {
                format!("published version refused; {} found", version.name())
            }
            Unattached::NoPublished => "no published version".to_string(),
        }
    }
}

/// Links to open in a browser for an entry, in the summary of `papers file fetch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToOpen {
    pub key: String,
    pub links: Vec<String>,
    pub what: Unattached,
}

/// What `papers file fetch` did, entry by entry, for the summary printed at the end of a fetch of
/// several entries (`FetchSummary`).
#[derive(Debug, Clone, Default)]
pub struct FetchSummary {
    /// The main PDFs attached, by version.
    pub attached: BTreeMap<Version, usize>,
    pub supplements: usize,
    /// The entries repaired from a local file (not downloaded).
    pub repaired: usize,
    /// The entries that had the PDF already.
    pub had: usize,
    /// `--upgrade`: the PDFs replaced by the published version.
    pub upgraded: usize,
    /// The links to open in a browser, entry by entry.
    pub browser: Vec<ToOpen>,
}

impl FetchSummary {
    /// The summary of a fetch of `count` entries (`FetchSummary.print`).
    pub fn lines(&self, count: usize) -> Vec<String> {
        let plural = |n: usize| if n > 1 { "s" } else { "" };
        let mut lines = vec![format!("summary of {count} entries:")];
        let attached: usize = self.attached.values().sum();
        if attached > 0 {
            let versions: Vec<String> = self
                .attached
                .iter()
                .map(|(version, n)| format!("{n} {}", version.name()))
                .collect();
            lines.push(format!(
                "  {attached} PDF{} attached: {}",
                plural(attached),
                versions.join(", ")
            ));
        }
        if self.upgraded > 0 {
            lines.push(format!(
                "  {} replaced by the published version",
                self.upgraded
            ));
        }
        if self.repaired > 0 {
            lines.push(format!(
                "  {} found locally (repaired, not downloaded)",
                self.repaired
            ));
        }
        if self.had > 0 {
            lines.push(format!("  {} had the PDF already", self.had));
        }
        if self.supplements > 0 {
            lines.push(format!(
                "  {} supplement{} attached",
                self.supplements,
                plural(self.supplements)
            ));
        }
        let count = |what: Unattached| self.browser.iter().filter(|b| b.what == what).count();
        let failed = count(Unattached::NoPdf);
        if failed > 0 {
            lines.push(format!("  {failed} without PDF"));
        }
        let kept = count(Unattached::NoPublished);
        if kept > 0 {
            lines.push(format!(
                "  {kept} without published version (kept as they are)"
            ));
        }
        if lines.len() == 1 {
            lines.push("  nothing attached".to_string());
        }
        let links: Vec<String> = self
            .browser
            .iter()
            .filter(|b| !b.links.is_empty())
            .map(|b| format!("  {}: {} ({})", b.key, b.links.join(", "), b.what.text()))
            .collect();
        if !links.is_empty() {
            lines.push("open in a browser:".to_string());
            lines.extend(links);
        }
        lines
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
    /// A fetch of several entries: an entry without PDF gets one line on the standard error, not
    /// an error ([`no_pdf`]).
    pub several: bool,
}

/// The message of a [`NoPdf`] in one line (`_one_line`): the reasons; open LINKS in a browser.
pub fn one_line(message: &str) -> String {
    const BROWSER: &str = "open in a browser: ";
    let lines: Vec<&str> = message.lines().collect();
    let rest = lines.get(1..).unwrap_or_default();
    let mut reasons: Vec<String> = rest
        .iter()
        .filter(|line| !line.starts_with(BROWSER))
        .map(|line| strip(line).to_string())
        .collect();
    if reasons.is_empty() {
        reasons = lines.iter().take(1).map(|line| line.to_string()).collect();
    }
    reasons.extend(
        rest.iter()
            .filter_map(|line| line.strip_prefix(BROWSER))
            .map(|links| format!("open {links} in a browser")),
    );
    reasons.join("; ")
}

/// Report an entry without PDF (`_no_pdf`): a `papers: error:` for one entry; for several, a line
/// `KEY: no PDF (the reasons; open LINKS in a browser)` on the standard error.
fn no_pdf(key: &str, message: &str, several: bool) {
    if several {
        log::eprintln(&format!("{key}: no PDF ({})", one_line(message)));
    } else {
        log::error(&format!("{key}: {message}"));
    }
}

/// The DRYRUN lines of an entry (`_dry_run_lines`); false if no PDF would be found. `replacing`:
/// the end of a line of `--upgrade` (empty otherwise); `several`: see [`no_pdf`].
#[allow(clippy::too_many_arguments)]
fn dry_run_lines(
    session: &Session,
    key: &str,
    search: &mut Search,
    local: &mut dyn FnMut(&Link, &Known) -> Option<Found>,
    fetcher: &Fetcher,
    main: bool,
    supplements: bool,
    replacing: &str,
    several: bool,
) -> bool {
    let (mut lines, mut ok) = (Vec::new(), true);
    if main {
        let links: Vec<Link> = search.links().collect();
        for (i, link) in links.iter().enumerate() {
            let found = fetcher.local_copy(link, Some(&mut *local));
            let has = found.as_ref().is_some_and(|f| f.how == How::Has);
            let mut what = match &found {
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
            if !replacing.is_empty() && !has {
                what = what.replace("would attach", "would copy") + replacing;
            }
            lines.push(format!(
                "DRYRUN: {key}: {}{what}",
                if i > 0 { "else " } else { "" }
            ));
        }
        if links.is_empty() {
            no_pdf(key, &no_pdf_message(search, &[]), several);
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
/// repaired (no main PDF fetched unless `again`, without message). `summary`: what was done, filled
/// entry by entry. The number of the entries whose PDF was not found (the errors are reported).
pub fn fetch_entry_pdfs(
    session: &mut Session,
    fetcher: &Fetcher,
    keys: &[String],
    mode: Mode,
    options: &PdfOptions,
    repaired: &HashSet<String>,
    summary: &mut FetchSummary,
) -> Result<usize> {
    let dry_run = session.dry_run();
    let mut library = LibraryChecksums::new(session);
    let mut failed = 0;
    // (entry, found, the summary line of a refused published version)
    let mut plans: Vec<(String, Vec<Found>, Option<ToOpen>)> = Vec::new();
    for key in keys {
        let Some(e) = session.library.get(key) else {
            continue;
        };
        let main = options.again || (!has_pdf(&session.library, key) && !repaired.contains(key));
        if !main && !repaired.contains(key) {
            session.say.line(format!(
                "{key}: has a PDF already (--again to fetch it anyway)"
            ));
            summary.had += 1;
            if !options.supplements {
                continue;
            }
        }
        let doi = strip_doi(val(e, "doi"));
        let arxiv = entry_arxiv(e);
        if doi.is_none() && arxiv.is_empty() {
            if main {
                no_pdf(key, "no DOI or arXiv id to look for a PDF", options.several);
                summary.browser.push(ToOpen {
                    key: key.clone(),
                    links: Vec::new(),
                    what: Unattached::NoPdf,
                });
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
                "",
                options.several,
            );
            failed += usize::from(!ok);
            continue;
        }
        let mut found = Vec::new();
        let mut refused = None;
        if main {
            match fetch_pdf(&mut search, Some(&mut local)) {
                Ok(f) => {
                    if !f.refused.is_empty() {
                        // in the order of the entries; left out below if the PDF is not attached
                        let line = ToOpen {
                            key: key.clone(),
                            links: f.refused.clone(),
                            what: Unattached::Refused(f.link.version),
                        };
                        summary.browser.push(line.clone());
                        refused = Some(line);
                    }
                    found.push(f);
                }
                Err(error) => {
                    no_pdf(key, &error.to_string(), options.several);
                    summary.browser.push(ToOpen {
                        key: key.clone(),
                        links: error.links,
                        what: Unattached::NoPdf,
                    });
                    failed += 1;
                }
            }
        }
        if options.supplements {
            found.extend(fetch_supplements(&mut search, Some(&mut local)));
        }
        plans.push((key.clone(), found, refused));
    }

    // the downloads are done: the files are attached
    for (key, found, refused) in plans {
        if !attach_fetched(session, &key, found, refused.as_ref(), mode, summary)? {
            failed += 1;
        }
    }
    Ok(failed)
}

/// Attach the PDFs found for the entry `key` as [`fetch_entry_pdfs`] says, and count them in the
/// summary (`_attach_fetched`; `refused`: the summary line of a refused published version). False
/// when the files could not be placed ([`attach_files`]).
fn attach_fetched(
    session: &mut Session,
    key: &str,
    found: Vec<Found>,
    refused: Option<&ToOpen>,
    mode: Mode,
    summary: &mut FetchSummary,
) -> Result<bool> {
    let mut new: Vec<Found> = Vec::new();
    for f in found {
        let same = || {
            let mut files = session.library.files(key);
            files.extend(new.iter().map(|g| g.path.clone()));
            same_content(&f.path, &files)
        };
        if f.how == How::Has {
            session.say.line(format!(
                "{key}: already has {} (from {})",
                show(&f.path),
                f.link.host()
            ));
        } else if let Some(same) = (f.how == How::Downloaded).then(same).flatten() {
            session.say.line(format!(
                "{key}: {}: not attached, the entry has the same file: {}",
                f.link.url,
                show(&same)
            ));
            discard(&f.path);
        } else {
            new.push(f);
            continue;
        }
        // not attached: the entry had the PDF already (counted once, not as refused)
        if f.link.version != Version::Supplement {
            summary.had += 1;
            if let Some(i) = summary.browser.iter().position(|b| Some(b) == refused) {
                summary.browser.remove(i);
            }
        }
    }
    if new.is_empty() {
        return Ok(true);
    }
    // a file of another entry is linked where it is (shared, as add does); the others are placed
    let (linked, placed): (Vec<Found>, Vec<Found>) = new
        .into_iter()
        .partition(|f| matches!(f.how, How::Linked(_)));
    let mut files = session.library.files(key);
    files.extend(linked.iter().map(|f| f.path.clone()));
    session.library.set_files(key, &files)?;
    let ok = placed.is_empty() || {
        let paths: Vec<String> = placed.iter().map(|f| f.path.clone()).collect();
        attach_files(session, key, &paths, &Attach::new(mode))?
    };
    for f in &placed {
        // the temporary folder of a download, now empty
        if f.how == How::Downloaded && !exists(&f.path) {
            discard(&f.path);
        }
    }
    let all = session.library.files(key);
    let count = linked.len() + placed.len();
    let paths = &all[all.len().saturating_sub(count)..];
    for (path, f) in paths.iter().zip(linked.iter().chain(&placed)) {
        session.say.line(found_line(key, path, f));
        if f.link.version == Version::Supplement {
            summary.supplements += 1;
        } else {
            *summary.attached.entry(f.link.version).or_default() += 1;
            // in .papers/files.json, by checksum
            session
                .library
                .pdf_versions
                .insert(path.clone(), f.link.version.word().to_string());
        }
    }
    Ok(ok)
}

/// The versions lower than the published one, that `--upgrade` replaces (`LOWER_VERSIONS`).
const LOWER_VERSIONS: [Version; 2] = [Version::Accepted, Version::Submitted];

/// The first PDF of the entry whose version is accepted or submitted, with its version, when no
/// PDF of the entry is the published version; else none (`lower_version_pdf`). `versions`:
/// `{absolute path: version}` ([`FilesJson::file_versions`]).
pub fn lower_version_pdf(
    library: &Library,
    key: &str,
    versions: &BTreeMap<String, String>,
) -> Option<(String, Version)> {
    let pdfs: Vec<String> = library
        .files(key)
        .into_iter()
        .filter(|f| f.to_lowercase().ends_with(".pdf") && isfile(f))
        .collect();
    let known: Vec<Option<&str>> = pdfs
        .iter()
        .map(|f| versions.get(&normpath(f)).map(String::as_str))
        .collect();
    if known.contains(&Some(Version::Published.word())) {
        return None;
    }
    pdfs.into_iter().zip(known).find_map(|(f, v)| {
        LOWER_VERSIONS
            .into_iter()
            .find(|lower| Some(lower.word()) == v)
            .map(|lower| (f, lower))
    })
}

/// `papers file fetch --upgrade` (`upgrade_entry_pdfs`): the entries `keys` whose PDF is an
/// accepted manuscript or a submitted version ([`lower_version_pdf`]) look for the published
/// version only; one found replaces that file, under its name (the old file goes to the history,
/// [`Session::replace_file`]). The library is not saved. `named`: the entries were given by key
/// (an entry without such a PDF is said); `several`: an entry without published PDF gets one
/// line. The entries looked at.
pub fn upgrade_entry_pdfs(
    session: &mut Session,
    fetcher: &Fetcher,
    keys: &[String],
    named: bool,
    access: &OpenAlex,
    summary: &mut FetchSummary,
    several: bool,
) -> Result<Vec<String>> {
    let dry_run = session.dry_run();
    let versions = files_json(session)
        .map(|f| f.file_versions())
        .unwrap_or_default();
    let mut library = LibraryChecksums::new(session);
    let (mut looked, mut plans) = (Vec::new(), Vec::new());
    for key in keys {
        let Some(e) = session.library.get(key) else {
            continue;
        };
        let Some((file, version)) = lower_version_pdf(&session.library, key, &versions) else {
            if named {
                session.say.line(format!(
                    "{key}: no accepted manuscript or submitted version to replace"
                ));
            }
            continue;
        };
        looked.push(key.clone());
        let Some(doi) = strip_doi(val(e, "doi")) else {
            session.say.line(format!(
                "{key}: no DOI to look for the published version: {} ({}) kept",
                show(&file),
                version.name()
            ));
            summary.browser.push(ToOpen {
                key: key.clone(),
                links: Vec::new(),
                what: Unattached::NoPublished,
            });
            continue;
        };
        let mut search = Search::new(fetcher, access, Some(&doi), None, Least::Published);
        let current = &session.library;
        let mut local = |link: &Link, known: &Known| library.local(current, key, link, known);
        let replacing = format!(", in place of {} ({})", show(&file), version.name());
        if dry_run {
            dry_run_lines(
                session,
                key,
                &mut search,
                &mut local,
                fetcher,
                true,
                false,
                &replacing,
                several,
            );
            continue;
        }
        match fetch_pdf(&mut search, Some(&mut local)) {
            Ok(found) => plans.push((key.clone(), file, version, found)),
            Err(error) => {
                let kept = format!("{key}: {} ({}) kept", show(&file), version.name());
                session.say.line(if several {
                    format!(
                        "{kept} (no published version: {})",
                        one_line(&error.to_string())
                    )
                } else {
                    format!("{kept}: {error}")
                });
                summary.browser.push(ToOpen {
                    key: key.clone(),
                    links: error.links,
                    what: Unattached::NoPublished,
                });
            }
        }
    }

    // the downloads are done: the files are replaced
    for (key, file, version, found) in plans {
        replace_by_published(session, &key, &file, version, &found, summary)?;
    }
    Ok(looked)
}

/// Replace the PDF `file` of the entry `key` (of version `version`) by the published version found,
/// unless the entry has it already; count it in the summary (`_replace_by_published`).
fn replace_by_published(
    session: &mut Session,
    key: &str,
    file: &str,
    version: Version,
    found: &Found,
    summary: &mut FetchSummary,
) -> Result<()> {
    let same = if found.how == How::Has {
        Some(found.path.clone())
    } else {
        same_content(&found.path, &session.library.files(key))
    };
    if let Some(same) = same {
        // the entry has the published version already: it is now known
        session.say.line(format!(
            "{key}: already has {} ({})",
            show(&same),
            found.describe()
        ));
        if found.how == How::Downloaded {
            discard(&found.path);
        }
        session
            .library
            .pdf_versions
            .insert(same, Version::Published.word().to_string());
        summary.had += 1;
        return Ok(());
    }
    session.replace_file(file, &found.path, found.how != How::Downloaded)?;
    if found.how == How::Downloaded {
        // the temporary folder of the download, now empty
        discard(&found.path);
    }
    session
        .library
        .pdf_versions
        .insert(file.to_string(), Version::Published.word().to_string());
    session.say.line(format!(
        "{key}: {}: {} replaced by the {}{}",
        show(file),
        version.name(),
        found.describe(),
        if session.keeps_deleted() {
            ""
        } else {
            " (the old file went to the trash of the system)"
        }
    ));
    summary.upgraded += 1;
    Ok(())
}

/// `papers file fetch`, before it downloads (`repair_pdf_links`): the entries whose PDF link is
/// broken get the repair of `papers file fix` ([`relink_broken_links`]: the pool, the folders `search`
/// of `--search`, the history), the files placed with the files mode `mode`; with a dry run, what it
/// would relink. The keys of the entries that have a PDF after the repair: their PDF is not
/// downloaded (unless `again`).
pub fn repair_pdf_links(
    session: &mut Session,
    keys: &[String],
    search: &[String],
    nameformat: &Format,
    mode: Mode,
    again: bool,
) -> Result<HashSet<String>> {
    let dry_run = session.dry_run();
    let broken: Vec<String> = keys
        .iter()
        .filter(|k| {
            !has_pdf(&session.library, k)
                && session.library.get(k).is_some_and(|e| {
                    broken_links(&session.library, e)
                        .iter()
                        .any(|f| f.to_lowercase().ends_with(".pdf"))
                })
        })
        .cloned()
        .collect();
    if broken.is_empty() {
        return Ok(HashSet::new());
    }
    let Relinked {
        relinks,
        unclear,
        restores,
        ..
    } = relink_broken_links(
        session,
        &broken,
        search,
        nameformat,
        &[],
        Some(mode),
        !dry_run,
        false,
    )?;
    let is_pdf = |f: &String| f.to_lowercase().ends_with(".pdf");
    let mut repaired: HashSet<String> = relinks
        .iter()
        .filter(|p| p.found.iter().any(|(_, files)| files.iter().any(is_pdf)))
        .map(|p| p.key.clone())
        .collect();
    repaired.extend(
        restores
            .iter()
            .filter(|(_, link, _)| is_pdf(link))
            .map(|(key, _, _)| key.clone()),
    );
    let end = |key: &str| {
        if repaired.contains(key) && !again {
            ", not downloaded"
        } else {
            ""
        }
    };
    let shown = |files: &[String]| files.iter().map(|f| show(f)).collect::<Vec<_>>().join(", ");
    for plan in &relinks {
        let found: Vec<&(String, Vec<String>)> = plan
            .found
            .iter()
            .filter(|(_, files)| !files.is_empty())
            .collect();
        if dry_run {
            for (link, files) in found {
                session.say.line(format!(
                    "DRYRUN: {}: would relink {} -> {} ({})",
                    plan.key,
                    show(link),
                    shown(files),
                    plan.sources[link].describe()
                ));
            }
        } else if !found.is_empty() {
            let parts: Vec<String> = found
                .iter()
                .map(|(link, files)| {
                    format!("{} ({})", shown(files), plan.sources[link].describe())
                })
                .collect();
            session.say.line(format!(
                "{}: relinked {}{}",
                plan.key,
                parts.join("; "),
                end(&plan.key)
            ));
        }
    }
    for (key, link, reason) in &unclear {
        session.say.line(format!(
            "{key}: broken link: {}: {reason} (not relinked)",
            show(link)
        ));
    }
    for (key, link, when) in &restores {
        session.say.line(if dry_run {
            format!(
                "DRYRUN: {key}: would restore {} from the history ({when})",
                show(link)
            )
        } else {
            format!(
                "{key}: {}: restored from the history ({when}){}",
                show(link),
                end(key)
            )
        });
    }
    Ok(repaired)
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
    /// `--search DIR`: where to look for the file of a broken PDF link.
    pub search: Vec<String>,
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
    /// What was done, entry by entry.
    pub summary: FetchSummary,
}

impl Fetched {
    /// The summary printed at the end of a fetch of more than one entry (none for one entry or a
    /// dry run; papers prints none with `--porcelain` either).
    pub fn summary(&self) -> Option<Vec<String>> {
        (self.keys.len() > 1 && self.committed.is_some())
            .then(|| self.summary.lines(self.keys.len()))
    }
}

/// `papers file fetch` (`file_fetchcmd`): the free PDF of the entries, attached and saved in one
/// step (in a shallow library, the files go to its pool).
pub fn file_fetch(
    session: &mut Session,
    fetcher: &Fetcher,
    request: &FileFetch,
) -> Result<Fetched> {
    let dry_run = session.dry_run();
    // the library's own name template, as for `papers file fix`
    let nameformat = session.own_nameformat();
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
    // the repair of `papers file fix` first: the download, only if it gives no PDF (one step for both)
    let pdf = &request.pdf;
    let repaired = repair_pdf_links(
        session,
        &keys,
        &request.search,
        &nameformat,
        mode,
        pdf.again,
    )?;
    let fetched: Vec<String> = keys
        .iter()
        .filter(|k| !repaired.contains(*k) || pdf.again || pdf.supplements)
        .cloned()
        .collect();
    let mut summary = FetchSummary {
        repaired: if pdf.again { 0 } else { repaired.len() },
        ..FetchSummary::default()
    };
    // several entries: one line for each entry without PDF
    let pdf = PdfOptions {
        several: keys.len() > 1,
        ..pdf.clone()
    };
    let failed = fetch_entry_pdfs(
        session,
        fetcher,
        &fetched,
        mode,
        &pdf,
        &repaired,
        &mut summary,
    )?;
    let committed = if dry_run {
        None
    } else {
        Some(session.commit()?)
    };
    Ok(Fetched {
        keys,
        failed,
        committed,
        summary,
    })
}

/// What `papers file fetch --upgrade` is asked.
#[derive(Debug, Clone, Default)]
pub struct FileUpgrade {
    /// The entries named; none: those of the library whose PDF is of a lower version.
    pub keys: Option<Vec<String>>,
    /// How OpenAlex is asked (`OPENALEX_API_KEY`, `PAPERS_MAILTO`).
    pub access: OpenAlex,
    /// `-y/--yes`: no question before many entries.
    pub yes: bool,
}

/// What `papers file fetch --upgrade` did.
#[derive(Debug, Default)]
pub struct Upgraded {
    /// The entries of the command (without keys: those whose PDF is of a lower version).
    pub keys: Vec<String>,
    /// The entries looked at (a PDF of a lower version).
    pub looked: Vec<String>,
    /// The save (none with a dry run).
    pub committed: Option<Committed>,
    pub summary: FetchSummary,
}

impl Upgraded {
    /// The summary printed at the end of an upgrade of more than one entry looked at (none for
    /// one entry or a dry run; papers prints none with `--porcelain` either).
    pub fn summary(&self) -> Option<Vec<String>> {
        (self.looked.len() > 1 && self.committed.is_some())
            .then(|| self.summary.lines(self.looked.len()))
    }
}

/// `papers file fetch --upgrade [KEY...]` (`file_upgradecmd`): the published version of the PDFs
/// that `papers file fetch` attached as an accepted manuscript or a submitted version, in place of
/// them, saved in one step. Without keys, the entries of the library with such a PDF.
pub fn file_upgrade(
    session: &mut Session,
    fetcher: &Fetcher,
    request: &FileUpgrade,
) -> Result<Upgraded> {
    let dry_run = session.dry_run();
    if !dry_run {
        // a shallow library pushes the entries it saves
        session.use_pool_files()?;
    }
    let named = request.keys.is_some();
    let mut keys: Vec<String> = match &request.keys {
        Some(keys) => keys.clone(),
        None => session.library.entries().map(|e| e.key.clone()).collect(),
    };
    if !named {
        let versions = files_json(session)
            .map(|f| f.file_versions())
            .unwrap_or_default();
        keys.retain(|k| lower_version_pdf(&session.library, k, &versions).is_some());
    }
    confirm_openalex(session, &keys, request.yes, &request.access)?;
    let mut summary = FetchSummary::default();
    let looked = upgrade_entry_pdfs(
        session,
        fetcher,
        &keys,
        named,
        &request.access,
        &mut summary,
        keys.len() > 1,
    )?;
    let committed = if dry_run {
        None
    } else {
        Some(session.commit()?)
    };
    Ok(Upgraded {
        keys,
        looked,
        committed,
        summary,
    })
}

// papers fetch --pdf
// ------------------

/// A PDF that `papers fetch --pdf` wrote: the file, as the command names it, and what gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub target: String,
    pub found: Found,
}

impl Written {
    /// The line of the PDF written: `FILE (published version, from HOST)`.
    pub fn line(&self) -> String {
        format!("{} ({})", self.target, self.found.describe())
    }
}

/// The file name of `papers fetch --pdf` (`_pdf_name`): that of the URL when it looks like the
/// paper's (a digit in it), else made of the DOI or arXiv id.
fn pdf_name(found: &Found, id: &str) -> String {
    let name = basename(&found.path);
    let stem = match name.trim_start_matches('.').rfind('.') {
        Some(i) => &name[..name.len() - name.trim_start_matches('.').len() + i],
        None => name.as_str(),
    };
    if stem.chars().any(|c| c.is_numeric()) {
        return name;
    }
    format!("{}.pdf", id.replace('/', "_"))
}

/// Why `papers fetch --pdf` wrote no file for an argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotWritten {
    /// No PDF could be downloaded (the links to open in a browser), or there is no PDF to look
    /// for (not a DOI or arXiv id: no link).
    NoPdf(NoPdf),
    /// The file to write exists: it is not replaced.
    Exists(String),
}

impl std::fmt::Display for NotWritten {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            NotWritten::NoPdf(error) => error.fmt(f),
            NotWritten::Exists(target) => write!(
                f,
                "{target} exists: not replaced (-o FILE writes another file)"
            ),
        }
    }
}

impl From<NoPdf> for NotWritten {
    fn from(error: NoPdf) -> NotWritten {
        NotWritten::NoPdf(error)
    }
}

/// `papers fetch --pdf` of one argument (`fetch_pdfs`): the free PDF of a DOI or an arXiv id,
/// written to `output` (a directory: under the name of its URL in it), else under the name of its
/// URL in the current directory; an existing file is not replaced. An existing `output` file is
/// found before any request; a name made from the link, once its PDF is downloaded, before it is
/// written (the PDF is discarded; no other link is tried).
pub fn fetch_free_pdf(
    fetcher: &Fetcher,
    access: &OpenAlex,
    arg: &str,
    output: Option<&str>,
    least: Least,
) -> std::result::Result<Written, NotWritten> {
    let failed = |message: String| {
        NotWritten::NoPdf(NoPdf {
            message,
            links: Vec::new(),
        })
    };
    let taken = |path: &str| std::fs::symlink_metadata(path).is_ok();
    let doi = strip_doi(arg);
    let arxiv = if doi.is_some() {
        None
    } else {
        normalize_arxiv(arg)
    };
    let Some(id) = doi.as_deref().or(arxiv.as_deref()) else {
        return Err(failed("not a DOI or arXiv id".to_string()));
    };
    if let Some(output) = output.filter(|o| !isdir(o) && taken(o)) {
        return Err(NotWritten::Exists(output.to_string()));
    }
    let mut search = Search::new(fetcher, access, doi.as_deref(), arxiv.as_deref(), least);
    let found = fetch_pdf(&mut search, None)?;
    let mut target = output.map_or_else(|| pdf_name(&found, id), str::to_string);
    if isdir(&target) {
        target = join(&target, &pdf_name(&found, id));
    }
    if taken(&target) {
        discard(&found.path);
        return Err(NotWritten::Exists(target));
    }
    move_path(&found.path, &target).map_err(|e| failed(format!("{target}: {e}")))?;
    discard(&found.path);
    Ok(Written { target, found })
}

/// The porcelain lines of an argument of `papers fetch --pdf` (formats.md 7.2), `arg` as given:
/// `downloaded ID VERSION URL FILE` and a `refused-published ID URL` per published link refused
/// before it; `exists ID FILE` when the file to write exists; else a `refused ID URL` per link to
/// open in a browser, or `failed ID` when there is none.
pub fn pdf_porcelain_lines(
    arg: &str,
    result: &std::result::Result<Written, NotWritten>,
) -> Vec<String> {
    match result {
        Ok(Written { target, found }) => {
            let mut lines = vec![format!(
                "downloaded {arg} {} {} {target}",
                found.link.version.word(),
                found.link.url
            )];
            lines.extend(
                found
                    .refused
                    .iter()
                    .map(|url| format!("refused-published {arg} {url}")),
            );
            lines
        }
        Err(NotWritten::Exists(target)) => vec![format!("exists {arg} {target}")],
        Err(NotWritten::NoPdf(error)) if error.links.is_empty() => vec![format!("failed {arg}")],
        Err(NotWritten::NoPdf(error)) => error
            .links
            .iter()
            .map(|url| format!("refused {arg} {url}"))
            .collect(),
    }
}
