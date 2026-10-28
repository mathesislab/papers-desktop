//! What `papers add` does with PDFs, folders and DOIs (`Biblio.add_pdf`, `Biblio.scan_dir`,
//! `Biblio.fetch_doi`, `papers.files.scan_dir_items`): the metadata of a PDF is its DOI (from
//! the PDF metadata, or from its text, checked against its crossref title), else a full-text
//! query; a PDF that the library has (same DOI) is attached to its entry; a folder is scanned
//! for PDFs, with the files named `<name>--<label>.<ext>` as attachments of `<name>.pdf`.

use std::collections::{HashMap, HashSet};

use crate::bib::encoding::parse_file;
use crate::bib::{Bib, Block, Entry};
use crate::config::paths::{abspath, basename, dirname, exists, isdir, isfile, join, realpath};
use crate::entry;
use crate::error::{Error, Result};
use crate::extract::doi::{parse_dois, title_in_text};
use crate::extract::{Options, PdfFile, extract_pdf_metadata};
use crate::fetch::download::download_url;
use crate::library::{InsertOptions, Library, Policy};
use crate::log;
use crate::merge::Resolve;

use crate::formats::Format;

use super::add::{Watch, add_file, attach_to_existing, insert_messages};
use super::queries::Queries;
use super::session::Session;

fn fetch_error(error: crate::fetch::FetchError) -> Error {
    Error::exit(error.message)
}

/// What is the same in all the files of one `add`.
#[derive(Clone, Copy)]
pub(super) struct Job<'a> {
    pub search_doi: bool,
    pub search_fulltext: bool,
    pub scholar: bool,
    /// `--doi`: the metadata of the DOI, not of the PDF.
    pub doi: Option<&'a str>,
    pub check_duplicate: bool,
    /// The insert options: the placement of the files (none: linked), `update_key`, the fields given.
    pub options: &'a InsertOptions,
    /// The placement of the files by the files mode: a file that was downloaded is always placed.
    pub placement: &'a Policy,
    pub queries: &'a Queries,
    /// Whether a question of the files policy went unanswered (see `Placement::check`).
    pub check: &'a dyn Fn() -> Result<()>,
}

/// `find_pdf`: the library entry of the DOI of a PDF, without network query: the DOI of the PDF
/// metadata, else a DOI of the text whose library entry has its title in the text.
pub fn find_pdf(library: &mut Library, pdf: &str) -> Option<String> {
    let file = PdfFile::new(pdf);
    match file.metadata_doi() {
        Ok(Some(doi)) => return library.find_doi(&doi),
        Ok(None) => {}
        Err(error) => {
            log::debug(&format!("{pdf}: {}", error.message));
            return None;
        }
    }
    let read = (|| -> std::result::Result<(String, Vec<String>), String> {
        let txt = file.head(10, 200, false).map_err(|e| e.message)?;
        let tail = file.tail().map_err(|e| e.message)?;
        let candidates = parse_dois(&txt, &tail).map_err(|e| e.0)?;
        Ok((txt, candidates))
    })();
    let (txt, candidates) = match read {
        Ok(found) => found,
        Err(error) => {
            log::debug(&format!("{pdf}: {error}"));
            return None;
        }
    };
    for candidate in candidates {
        let Some(key) = library.find_doi(&candidate) else {
            continue;
        };
        let title = library
            .get(&key)
            .map(|e| entry::val(e, "title").to_string());
        if title.is_some_and(|t| title_in_text(&t, &txt)) {
            return Some(key);
        }
    }
    None
}

/// The entry of a bibtex text, its LaTeX converted to unicode (`add_pdf`).
fn entry_of(bibtex: &str) -> Result<Entry> {
    let bib = Bib::parse(bibtex);
    let Some(first) = bib.entries().next().cloned() else {
        return Err(Error::exit("list index out of range"));
    };
    let one = Bib {
        blocks: vec![Block::Entry(first)],
    };
    let converted = crate::bib::unicode::latex_to_unicode_library(one);
    Ok(converted
        .entries()
        .next()
        .cloned()
        .expect("the entry that was put in"))
}

/// Add a PDF (`add_pdf`) with the files attached to it: the keys of the entries it ended in.
/// A URL is downloaded first (and always placed in the files directory).
pub(super) fn add_pdf(
    session: &mut Session,
    pdf: &str,
    attachments: &[String],
    job: &Job,
    resolver: &mut dyn Resolve,
) -> Result<Vec<String>> {
    let mut options = job.options.clone();
    let pdf = if pdf.starts_with("http") {
        options.rename = Some(job.placement.clone());
        download_url(pdf, true, None).map_err(Error::exit)?
    } else {
        pdf.to_string()
    };
    // the library first (same DOI: the files are attached to its entry), then online
    if job.check_duplicate {
        let existing = match job.doi {
            Some(doi) => session.library.find_doi(doi),
            None if job.search_doi => find_pdf(&mut session.library, &pdf),
            None => None,
        };
        if let Some(key) = existing {
            let files: Vec<String> = std::iter::once(pdf.clone())
                .chain(attachments.iter().cloned())
                .collect();
            return attach_to_existing(
                session,
                &key,
                &files,
                &options.metadata,
                options.rename.as_ref(),
            );
        }
    }
    // then the pool of the library
    if session.lookup.is_some() {
        let files: Vec<String> = std::iter::once(&pdf)
            .chain(attachments)
            .map(|f| abspath(f))
            .collect();
        let taken = match (session.lookup.as_mut(), session.exchange.as_mut()) {
            (Some(lookup), Some(exchange)) => {
                lookup.pdf(exchange, &session.library, &files[0], &files, job.doi)?
            }
            _ => None,
        };
        if let Some(taken) = taken {
            options.update_key = true;
            return insert_taken(session, taken, &options, resolver);
        }
    }
    let fetcher = job.queries.fetcher().map_err(fetch_error)?;
    let bibtex = match job.doi {
        Some(doi) => fetcher.fetch_bibtex_by_doi(doi),
        None => extract_pdf_metadata(
            &fetcher,
            &pdf,
            &Options {
                search_doi: job.search_doi,
                search_fulltext: job.search_fulltext,
                scholar: job.scholar,
                ..Options::default()
            },
        ),
    }
    .map_err(fetch_error)?;
    let mut entry = entry_of(&bibtex)?;
    let files: Vec<String> = std::iter::once(&pdf)
        .chain(attachments)
        .map(|f| abspath(f))
        .collect();
    entry::set_files(&mut entry, &files, &session.library.relative_to);
    entry.key = session.library.keyformat.render(&entry)?;
    log::debug(&format!("generated PDF key: {}", entry.key));
    options.update_key = true;
    let inserted = session.library.insert(entry, &options, resolver)?;
    Ok(insert_messages(session, std::slice::from_ref(&inserted)))
}

/// Insert an entry taken from the pool and record its origin (`_insert_taken`): the keys of the entries it ended in.
pub(super) fn insert_taken(
    session: &mut Session,
    taken: Entry,
    options: &InsertOptions,
    resolver: &mut dyn Resolve,
) -> Result<Vec<String>> {
    let inserted = session.library.insert(taken, options, resolver)?;
    if let (Some(lookup), Some(exchange)) = (session.lookup.as_mut(), session.exchange.as_mut()) {
        lookup.inserted(exchange, &session.library, &inserted, options.rename.as_ref())?;
    }
    session.remove_stored_duplicates();
    Ok(insert_messages(session, std::slice::from_ref(&inserted)))
}

/// `Biblio.fetch_doi` (`add --doi DOI` without file): the entry of the DOI is added to the
/// library, or, if the library has it, the attachments are added to its entry.
pub(super) fn add_doi(
    session: &mut Session,
    doi: &str,
    attachments: &[String],
    job: &Job,
    resolver: &mut dyn Resolve,
) -> Result<Vec<String>> {
    let attachments: Vec<String> = attachments.iter().map(|f| abspath(f)).collect();
    if job.check_duplicate
        && let Some(key) = session.library.find_doi(doi)
    {
        return attach_to_existing(
            session,
            &key,
            &attachments,
            &job.options.metadata,
            job.options.rename.as_ref(),
        );
    }
    // the pool of the library, then online
    let taken = match (session.lookup.as_mut(), session.exchange.as_mut()) {
        (Some(lookup), Some(exchange)) => lookup.doi(exchange, &session.library, doi, &attachments)?,
        _ => None,
    };
    if let Some(taken) = taken {
        let options = InsertOptions {
            update_key: true,
            ..job.options.clone()
        };
        return insert_taken(session, taken, &options, resolver);
    }
    let fetcher = job.queries.fetcher().map_err(fetch_error)?;
    let bibtex = fetcher.fetch_bibtex_by_doi(doi).map_err(fetch_error)?;
    // the key of a fetched entry is always updated
    let options = InsertOptions {
        update_key: true,
        ..job.options.clone()
    };
    let inserted =
        session
            .library
            .add_bibtex(&bibtex, None, &attachments, false, &options, resolver)?;
    Ok(insert_messages(session, &inserted))
}

// folders
// -------

/// A thing found in a folder (`scan_dir_walk`).
#[derive(Debug, PartialEq, Eq)]
pub enum Item {
    /// A folder of the layout before papers 2.19: `<name>/` with a hidden `.<name>.bib`.
    EntryDir(String),
    /// A file, with the files named after it (`<name>--<label>.<ext>`).
    File(String, Vec<String>),
}

impl Item {
    fn path(&self) -> &str {
        match self {
            Item::EntryDir(p) | Item::File(p, _) => p,
        }
    }
}

const ATTACHMENT_SEP: &str = "--";

fn splitext_stem(name: &str) -> &str {
    let lead = name.len() - name.trim_start_matches('.').len();
    match name[lead..].rfind('.') {
        Some(i) => &name[..lead + i],
        None => name,
    }
}

/// Split the file names of a directory into main files and attachments: `<name>--<label>.<ext>`
/// is an attachment of `<name>.pdf` when that file is among the names (the layout of
/// `rename_entry_files`). Every name that is not an attachment is a main file; in order.
pub fn group_attachments(names: &[String]) -> Vec<(String, Vec<String>)> {
    let mut stems: Vec<(String, String)> = Vec::new();
    for name in names.iter().filter(|n| n.to_lowercase().ends_with(".pdf")) {
        let stem = splitext_stem(name).to_string();
        match stems.iter_mut().find(|(s, _)| *s == stem) {
            Some((_, n)) => *n = name.clone(),
            None => stems.push((stem, name.clone())),
        }
    }
    let mut attached: HashMap<&str, &str> = HashMap::new();
    for name in names {
        let stem = splitext_stem(name);
        // the longest main stem that this name continues with '--'
        let best = stems
            .iter()
            .filter(|(s, n)| n != name && stem.starts_with(&format!("{s}{ATTACHMENT_SEP}")))
            .max_by_key(|(s, _)| s.chars().count());
        if let Some((_, main)) = best {
            attached.insert(name, main);
        }
    }
    let mut groups: Vec<(String, Vec<String>)> = names
        .iter()
        .filter(|n| !attached.contains_key(n.as_str()))
        .map(|n| (n.clone(), Vec::new()))
        .collect();
    for name in names {
        if let Some(&first) = attached.get(name.as_str()) {
            let mut main = first;
            while let Some(&next) = attached.get(main) {
                main = next;
            }
            if let Some((_, list)) = groups.iter_mut().find(|(g, _)| g == main) {
                list.push(name.clone());
            }
        }
    }
    groups
}

fn hidden_bibtex(direc: &str) -> String {
    join(direc, &format!(".{}.bib", basename(direc)))
}

/// The folders and files that `add -R` finds (`scan_dir_walk`): the files of every folder that is
/// not hidden (a name that starts with `.` or `_`), but those that start with `.`.
pub fn scan_dir_walk(direc: &str) -> Vec<Item> {
    let top = abspath(direc);
    let mut items = Vec::new();
    walk(direc, &top, &mut items);
    items
}

fn walk(root: &str, top: &str, items: &mut Vec<Item>) {
    let Ok(read) = std::fs::read_dir(root) else {
        return;
    };
    let (mut dirs, mut files): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if isdir(&join(root, &name)) {
            dirs.push(name);
        } else {
            files.push(name);
        }
    }
    dirs.sort();
    files.sort();
    let name = basename(root);
    // never skip the scan root itself: basename('.') == '.' looks hidden
    let hidden = abspath(root) != top && (name.starts_with('.') || name.starts_with('_'));
    if !hidden {
        if exists(&hidden_bibtex(root)) {
            items.push(Item::EntryDir(root.to_string()));
        } else {
            let names: Vec<String> = files.into_iter().filter(|f| !f.starts_with('.')).collect();
            for (main, attachments) in group_attachments(&names) {
                items.push(Item::File(
                    join(root, &main),
                    attachments.iter().map(|a| join(root, a)).collect(),
                ));
            }
        }
    }
    for d in dirs {
        let path = join(root, &d);
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            walk(&path, top, items);
        }
    }
}

/// The items of [`scan_dir_walk`] in the order `add -R` adds them: entry folders and bibtex files
/// first, then the other files, without those linked from the bibtex files (their entry comes
/// from the bibtex, so they are neither parsed nor looked up online) (`scan_dir_items`).
pub fn scan_dir_items(direc: &str, quiet: bool) -> Vec<Item> {
    let (mut first, mut others) = (Vec::new(), Vec::new());
    for item in scan_dir_walk(direc) {
        let is_first =
            matches!(item, Item::EntryDir(_)) || item.path().to_lowercase().ends_with(".bib");
        if is_first {
            first.push(item);
        } else {
            others.push(item);
        }
    }
    let mut linked: HashSet<String> = HashSet::new();
    for item in &first {
        let Item::File(path, _) = item else { continue };
        let text = match std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|b| String::from_utf8(b).map_err(|e| e.to_string()))
        {
            Ok(text) => text,
            Err(error) => {
                log::warning(&format!("{path}::{error}"));
                continue;
            }
        };
        let relative_to = dirname(&abspath(path));
        for e in Bib::parse(&crate::bib::universal_newlines(&text)).entries() {
            for file in parse_file(entry::val(e, "file")) {
                linked.insert(realpath(&abspath(&join(&relative_to, &file))));
            }
        }
    }
    let mut rest = Vec::new();
    for item in others {
        if linked.contains(&realpath(item.path())) {
            if !quiet {
                log::info(&format!(
                    "{}: linked from a scanned bibtex file, not scanned",
                    item.path()
                ));
            }
            continue;
        }
        rest.push(item);
    }
    first.extend(rest);
    first
}

/// The entry of a folder of the layout before papers 2.19 (`read_entry_dir`): the hidden
/// bibtex, with the files of the folder (not hidden, not in subfolders) as its links.
fn read_entry_dir(direc: &str, relative_to: &str) -> Result<Entry> {
    let hidden = hidden_bibtex(direc);
    if !exists(&hidden) {
        return Err(Error::exit("hidden bib missing: not an entry dir"));
    }
    let text =
        std::fs::read_to_string(&hidden).map_err(|e| Error::exit(format!("{hidden}: {e}")))?;
    let bib = Bib::parse(&crate::bib::universal_newlines(&text));
    let entries: Vec<&Entry> = bib.entries().collect();
    if entries.len() != 1 {
        return Err(Error::exit(format!(
            "hidden bib must have one entry, got: {}",
            entries.len()
        )));
    }
    let mut entry = entries[0].clone();
    let mut files: Vec<String> = std::fs::read_dir(direc)
        .map(|r| {
            r.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|f| !f.starts_with('.') && isfile(&join(direc, f)))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let files: Vec<String> = files.iter().map(|f| join(direc, f)).collect();
    entry.set("file", entry::format_files(&files, Some(relative_to)));
    Ok(entry)
}

/// `add -R DIR` (`scan_dir`): the PDFs and the bibtex files of a folder and of the folders
/// below it are added. A file that cannot be added is a warning, the scan goes on; a question
/// without answer or an error of the duplicates stops it.
pub(super) fn scan_dir(
    session: &mut Session,
    direc: &str,
    job: &Job,
    watch: &mut Watch<'_>,
) -> Result<Vec<String>> {
    let mut touched = Vec::new();
    for item in scan_dir_items(direc, false) {
        let path = item.path().to_string();
        let result = match &item {
            Item::EntryDir(path) => {
                log::debug("read from hidden bibtex");
                let relative_to = session.library.relative_to.clone();
                read_entry_dir(path, &relative_to).and_then(|entry| {
                    let inserted = session.library.insert(entry, job.options, watch)?;
                    Ok(insert_messages(session, std::slice::from_ref(&inserted)))
                })
            }
            Item::File(path, attachments) if path.to_lowercase().ends_with(".pdf") => {
                add_pdf(session, path, attachments, job, watch)
            }
            Item::File(path, _) if path.to_lowercase().ends_with(".bib") => {
                add_file(session, path, Format::Bibtex, &[], job.options, watch)
            }
            Item::File(..) => Ok(Vec::new()),
        };
        match result {
            Ok(keys) => {
                (job.check)()?;
                touched.extend(keys);
            }
            Err(error) => {
                if watch.failed() {
                    return Err(error);
                }
                log::warning(&format!("{path}::{}", error.message()));
            }
        }
    }
    Ok(touched)
}
