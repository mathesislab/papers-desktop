//! `papers fetch` and `papers extract` (and the deprecated `papers doi`): the bibtex of a
//! DOI, of a full-text search, or of PDF files, printed (they need no library and write nothing).

use std::fs;
use std::path::Path;

use papers_rs::config::dirs::cache_dir;
use papers_rs::config::settings::py_int;
use papers_rs::error::Error;
use papers_rs::extract::{Options, extract_pdf_doi, extract_pdf_metadata};
use papers_rs::fetch::{ErrorKind, FetchError, Fetcher, openalex};
use papers_rs::identity::strip_doi;
use papers_rs::log;

use crate::parse::Leaf;
use crate::util::{file_uri_to_path, openalex_access};

/// What ends the command other than a result.
pub enum Failure {
    /// An error of the command line (`papers fetch: error: ...`, status 2).
    Usage(String),
    Error(Error),
}

impl From<Error> for Failure {
    fn from(error: Error) -> Failure {
        Failure::Error(error)
    }
}

type Outcome = Result<(), Failure>;

const SCHOLAR: &str = "--scholar: Google Scholar is not implemented in papers-rs yet";

/// An exception that papers does not catch: it ends the command (a traceback there).
fn uncaught(error: FetchError) -> Failure {
    Failure::Error(Error::other(error.message))
}

fn fetcher() -> Result<Fetcher, Failure> {
    Fetcher::new(&cache_dir()).map_err(uncaught)
}

/// `papers fetch`: the bibtex of one or several DOIs, or of a full-text search.
pub fn fetch(leaf: &Leaf) -> Outcome {
    let fields = leaf.strings("doi_or_text");
    let scholar = leaf.flag("scholar");
    let openalex = leaf.flag("openalex");
    // either one or several DOIs
    let dois: Vec<Option<String>> = fields.iter().map(|f| strip_doi(f)).collect();
    if dois.iter().all(|d| d.is_some()) {
        if scholar {
            return Err(Failure::Usage(
                "Fetching from DOI does not support Google Scholar option".into(),
            ));
        }
        if openalex {
            return Err(Failure::Usage(
                "Fetching from DOI does not support the OpenAlex option".into(),
            ));
        }
        let fetcher = fetcher()?;
        for doi in dois.into_iter().flatten() {
            log::println(&fetcher.fetch_bibtex_by_doi(&doi).map_err(uncaught)?);
        }
        return Ok(());
    }
    // or one full text search
    if scholar {
        return Err(Error::exit(SCHOLAR).into());
    }
    let text = fields.join(" ");
    let bibtex = if openalex {
        // papers-rs, not in papers: the search of OpenAlex instead of crossref's
        let access = openalex_access();
        fetcher()?.fetch_bibtex_by_fulltext_openalex(&text, &access, openalex::ROWS, None)
    } else {
        fetcher()?.fetch_bibtex_by_fulltext_crossref(&text, None, None, 5)
    }
    .map_err(uncaught)?;
    log::println(&bibtex);
    Ok(())
}

/// `pathlib.Path(path)` as a string: no empty or `.` components, no trailing slash.
fn pathlib_str(path: &str) -> String {
    let rooted = path.starts_with('/');
    let leading = if path.starts_with("//") && !path.starts_with("///") {
        "//"
    } else if rooted {
        "/"
    } else {
        ""
    };
    let parts: Vec<&str> = path
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    let joined = parts.join("/");
    if joined.is_empty() && leading.is_empty() {
        ".".to_string()
    } else {
        format!("{leading}{joined}")
    }
}

/// `Path.suffix.lower() == '.pdf'`
fn has_pdf_suffix(name: &str) -> bool {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => name[i..].to_lowercase() == ".pdf",
        _ => false,
    }
}

/// `Path(base).rglob('*')`: the files of the directory and of those below it (the links to
/// directories are not entered), as the path joined to the base.
fn pdf_files_below(base: &str) -> Vec<String> {
    fn walk(dir: &Path, shown: &str, found: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let shown_path = if shown == "." {
                name.clone()
            } else if shown.ends_with('/') {
                format!("{shown}{name}")
            } else {
                format!("{shown}/{name}")
            };
            let path = entry.path();
            let is_link = entry.file_type().map(|t| t.is_symlink()).unwrap_or(false);
            if path.is_file() && has_pdf_suffix(&name) {
                found.push(shown_path.clone());
            }
            if !is_link && entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                walk(&path, &shown_path, found);
            }
        }
    }
    let shown = pathlib_str(base);
    let mut found = Vec::new();
    walk(Path::new(base), &shown, &mut found);
    found.sort();
    found
}

/// `_collect_pdf_files`: the files, plain files as given, the directories (with `-R`) scanned
/// for PDF files. The error is the message of papers' `ValueError`.
fn collect_pdf_files(paths: &[String], recursive: bool) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    for path in paths {
        if Path::new(path).is_dir() {
            if !recursive {
                return Err(format!(
                    "{path} is a directory, requires -R/--recursive to scan"
                ));
            }
            files.extend(pdf_files_below(path));
        } else {
            files.push(path.clone());
        }
    }
    Ok(files)
}

/// The failure of one file: papers' `ValueError`s are the errors of the file (a single file:
/// the error of the command), the other exceptions end the command or, among several files, are
/// reported as the others.
fn file_failure(file: &str, error: &FetchError, single: bool) -> Result<(), Failure> {
    if single {
        return Err(if error.is_value_error() {
            Error::exit(format!("{file}: {error}")).into()
        } else {
            uncaught(error.clone())
        });
    }
    log::error(&format!("{file}: {error}"));
    Ok(())
}

/// The panics of the libraries that read PDFs are caught by `papers-rs` (the page has no text):
/// they are not worth a message. Those of this program are.
fn quiet_pdf_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let in_a_library = info
            .location()
            .is_some_and(|l| l.file().contains(".cargo/registry"));
        if !in_a_library {
            default(info);
        }
    }));
}

/// `papers extract`: the bibtex (or with `--doi-only` the DOI) of PDF files.
pub fn extract(leaf: &Leaf) -> Outcome {
    quiet_pdf_panics();
    let paths: Vec<String> = leaf
        .strings("pdf")
        .iter()
        .map(|p| file_uri_to_path(p))
        .collect();
    let files = collect_pdf_files(&paths, leaf.flag("recursive")).map_err(Failure::Usage)?;
    if files.is_empty() {
        log::warning("no PDF file found");
        return Ok(());
    }
    let word_count = match leaf.string("word_count") {
        Some(n) => py_int(&n).expect("checked by the parser"),
        None => 200,
    };
    let fetcher = fetcher()?;
    let options = Options {
        search_doi: !leaf.flag("fulltext"),
        search_fulltext: true,
        minwords: word_count,
        max_query_words: word_count,
        scholar: leaf.flag("scholar"),
        openalex: leaf.flag("openalex").then(openalex_access),
        image: leaf.flag("image"),
        ..Options::default()
    };
    let single = files.len() == 1;
    let mut failed = 0;
    for file in &files {
        if !single {
            log::println(&format!("% {file}"));
        }
        let result = if !Path::new(file).is_file() {
            let why = if !Path::new(file).exists() {
                "no such file"
            } else {
                "not a file"
            };
            Err(FetchError::new(ErrorKind::Value, why))
        } else if leaf.flag("doi_only") {
            extract_pdf_doi(&fetcher, file, options.image)
        } else {
            extract_pdf_metadata(&fetcher, file, &options)
        };
        match result {
            Ok(text) => log::println(&text),
            Err(error) => {
                file_failure(file, &error, single)?;
                failed += 1;
            }
        }
    }
    if failed > 0 {
        return Err(Error::exit(format!("{failed} of {} files failed", files.len())).into());
    }
    Ok(())
}

/// `papers doi` (deprecated): the DOI of a PDF.
pub fn doi(leaf: &Leaf) -> Outcome {
    log::warning("`papers doi` is deprecated: use `papers extract --doi-only`");
    quiet_pdf_panics();
    let pdf = leaf
        .string("pdf")
        .map(|p| file_uri_to_path(&p))
        .unwrap_or_default();
    let fetcher = fetcher()?;
    match extract_pdf_doi(&fetcher, &pdf, leaf.flag("image")) {
        Ok(doi) => {
            log::println(&doi);
            Ok(())
        }
        Err(error) if error.is_value_error() => Err(Error::exit(format!("{pdf}: {error}")).into()),
        Err(error) => Err(uncaught(error)),
    }
}
