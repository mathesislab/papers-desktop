//! Metadata of a PDF (`papers.extract`): its text, the DOIs it contains (`doi`), and the
//! bibtex of the paper, checked against the text. The queries themselves are in `fetch`.
//!
//! The DOI parser is papers', character for character (`tests/extract.rs` checks it against
//! papers on generated texts). The text of a PDF is not: papers reads it with PyMuPDF
//! (AGPL), papers-rs with `pdf-extract` (`pdf`), see there.
//!
//! Not ported: the search by Google Scholar (`scholar`), and the PDF metadata through
//! `pdfinfo` that papers falls back on when PyMuPDF is not installed.

pub mod doi;
pub mod pdf;
mod text;

use std::cell::OnceCell;
use std::path::Path;
use std::process::Command;

use crate::bib::Bib;
use crate::config::json::py_repr_str;
use crate::fetch::{ErrorKind, FetchError, Fetcher};
use crate::log;

use self::doi::{DoiParsingError, parse_dois, query_text, title_in_text};
use self::pdf::Pdf;

type Result<T> = std::result::Result<T, FetchError>;

impl From<DoiParsingError> for FetchError {
    fn from(error: DoiParsingError) -> FetchError {
        FetchError::new(ErrorKind::Value, error.0)
    }
}

fn value_error(message: impl Into<String>) -> FetchError {
    FetchError::new(ErrorKind::Value, message)
}

/// What `extract_pdf_metadata` is asked.
#[derive(Debug, Clone)]
pub struct Options {
    pub search_doi: bool,
    pub search_fulltext: bool,
    pub maxpages: usize,
    pub minwords: i64,
    pub max_query_words: i64,
    pub scholar: bool,
    pub image: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            search_doi: true,
            search_fulltext: true,
            maxpages: 10,
            minwords: 200,
            max_query_words: 200,
            scholar: false,
            image: false,
        }
    }
}

/// A PDF, opened when its text or its metadata is first needed (PyMuPDF opens it for each
/// use; the first use is where an unreadable file is reported).
pub struct PdfFile<'a> {
    path: &'a str,
    cell: OnceCell<std::result::Result<Pdf, pdf::PdfError>>,
}

impl<'a> PdfFile<'a> {
    pub fn new(path: &'a str) -> PdfFile<'a> {
        PdfFile {
            path,
            cell: OnceCell::new(),
        }
    }

    fn get(&self) -> Result<&Pdf> {
        self.cell
            .get_or_init(|| Pdf::open(self.path))
            .as_ref()
            .map_err(|e| FetchError::new(ErrorKind::Other, e.0.clone()))
    }

    /// `parse_doi_from_pdf_metadata`
    pub fn metadata_doi(&self) -> Result<Option<String>> {
        Ok(self.get()?.metadata_doi())
    }

    /// `readpdf` of one page (counted from 1)
    fn read_page(&self, page: usize) -> Result<String> {
        if !Path::new(self.path).is_file() {
            return Err(value_error(format!(
                "{}: not a file",
                py_repr_str(self.path)
            )));
        }
        Ok(self.get()?.page_text(page - 1))
    }

    /// `pdftail`: the text of the last page, if the PDF has more than one.
    pub fn tail(&self) -> Result<String> {
        Ok(self.get()?.tail())
    }

    /// `pdfhead`: the text of the first pages, up to `maxpages`, and until it has `minwords` words.
    pub fn head(&self, maxpages: usize, minwords: i64, image: bool) -> Result<String> {
        let mut txt = String::new();
        let mut i = 0;
        while (doi::py_split(crate::bib::pystr::strip(&txt)).len() as i64) < minwords
            && i < maxpages
        {
            i += 1;
            log::debug(&format!("read pdf page: {i}"));
            txt.push_str(&if image {
                read_page_image(self.path, i)?
            } else {
                self.read_page(i)?
            });
        }
        Ok(txt)
    }
}

/// `readpdf_image`: the text of a page by optical character recognition, with `pdftoppm` and
/// `tesseract`.
fn read_page_image(pdf: &str, page: usize) -> Result<String> {
    if !Path::new(pdf).is_file() {
        return Err(value_error(format!("{}: not a file", py_repr_str(pdf))));
    }
    let other = |m: String| FetchError::new(ErrorKind::Other, m);
    let dir = tempfile::tempdir().map_err(|e| other(e.to_string()))?;
    let base = dir.path().join("pdf");
    let png = format!("{}.png", base.display());
    let txt = format!("{}.txt", base.display());
    let run = |argv: Vec<String>| -> Result<()> {
        log::debug(&argv.join(" "));
        let status = Command::new(&argv[0])
            .args(&argv[1..])
            .status()
            .map_err(|e| {
                other(format!(
                    "[Errno {}] {}: {}",
                    e.raw_os_error().unwrap_or(0),
                    os_message(&e),
                    py_repr_str(&argv[0])
                ))
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(other(format!(
                "Command {} returned non-zero exit status {}.",
                crate::config::json::py_repr(&serde_json::json!(argv)),
                status.code().unwrap_or(-1)
            )))
        }
    };
    // the page as a picture, then its text
    let mut cmd: Vec<String> = ["pdftoppm", "-singlefile", "-png", "-q"]
        .map(String::from)
        .to_vec();
    cmd.extend([
        "-f".to_string(),
        page.to_string(),
        "-l".to_string(),
        page.to_string(),
    ]);
    cmd.extend([pdf.to_string(), base.display().to_string()]);
    run(cmd)?;
    run([
        "tesseract",
        &png,
        &base.display().to_string(),
        "-l",
        "eng",
        "quiet",
    ]
    .map(String::from)
    .to_vec())?;
    std::fs::read_to_string(&txt).map_err(|e| other(e.to_string()))
}

fn os_message(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "No such file or directory".to_string(),
        _ => e.to_string(),
    }
}

/// `_bibtex_title`: the title of the first entry.
fn bibtex_title(bibtex: &str) -> String {
    Bib::parse(bibtex)
        .entries()
        .next()
        .and_then(|e| e.get("title"))
        .unwrap_or("")
        .to_string()
}

/// `confirm_doi`: the first candidate DOI whose title (from crossref) appears in the text, as
/// (DOI, bibtex). A candidate is never accepted on faith: a DOI cited in the text, or the DOI of
/// the previous article printed on the same page, is a valid DOI of another paper. DOIs without
/// metadata or without title cannot be checked and are skipped.
pub fn confirm_doi(
    fetcher: &Fetcher,
    dois: &[String],
    txt: &str,
) -> Result<Option<(String, String)>> {
    for doi in dois {
        let bibtex = match fetcher.fetch_bibtex_by_doi(doi) {
            Ok(b) => b,
            Err(error) if error.is_request_error() => {
                log::info(&format!("{doi}: no metadata found ({error})"));
                continue;
            }
            Err(error) => return Err(error),
        };
        let title = bibtex_title(&bibtex);
        if title.is_empty() {
            log::info(&format!("{doi}: metadata without title, skip"));
            continue;
        }
        if title_in_text(&title, txt) {
            return Ok(Some((doi.clone(), bibtex)));
        }
        log::info(&format!("{doi}: title not found in the text: {title}"));
    }
    Ok(None)
}

/// `extract_pdf_doi`: the article's DOI: from the PDF metadata if present (trusted as is),
/// otherwise the first DOI of the text whose title appears in the text (one crossref request per
/// candidate).
pub fn extract_pdf_doi(fetcher: &Fetcher, pdf: &str, image: bool) -> Result<String> {
    let file = PdfFile::new(pdf);
    if let Some(metadata_doi) = file.metadata_doi()? {
        return Ok(metadata_doi);
    }
    let txt = file.head(10, 200, image)?;
    let tail = if image { String::new() } else { file.tail()? };
    // the title is on the first page(s)
    match confirm_doi(fetcher, &parse_dois(&txt, &tail)?, &txt)? {
        Some((doi, _)) => Ok(doi),
        None => Err(value_error("no DOI found whose title appears in the text")),
    }
}

const NO_METADATA: &str = "failed to extract metadata: pass the DOI with --doi, or, without DOI, add the entry with --title, --author, --year... and the PDF with --attach";

/// `extract_txt_metadata`: the bibtex of a text, by parsing it for DOIs and querying them, or by
/// a full-text query. The result is accepted only if its title appears in `txt` (not in the tail:
/// for most papers the last page is the reference list, where cited papers have both their title
/// and DOI).
pub fn extract_txt_metadata(
    fetcher: &Fetcher,
    txt: &str,
    options: &Options,
    tail: &str,
) -> Result<String> {
    if options.search_doi {
        log::debug("parse doi");
        let dois = match parse_dois(txt, tail) {
            Ok(dois) => {
                log::info(&format!("found doi(s): {}", dois.join(", ")));
                dois
            }
            Err(error) => {
                log::debug(&format!("doi parsing error: {error}"));
                Vec::new()
            }
        };
        if !dois.is_empty() {
            log::debug("query bibtex by doi");
            if let Some((_, bibtex)) = confirm_doi(fetcher, &dois, txt)? {
                log::debug("doi query successful");
                return Ok(bibtex);
            }
            log::warning(&format!(
                "no DOI found whose title appears in the text: {}",
                dois.join(", ")
            ));
        }
    }
    if options.search_fulltext {
        log::debug("query bibtex by fulltext");
        let queried = query_text(txt, options.max_query_words)
            .map_err(value_error)
            .and_then(|query| {
                if options.scholar {
                    Err(FetchError::new(
                        ErrorKind::Other,
                        "scholarly (Google Scholar) is not implemented in papers-rs yet",
                    ))
                } else {
                    fetcher.fetch_bibtex_by_fulltext_crossref(&query, None, None, 5)
                }
            });
        match queried {
            Err(error) => log::warning(&format!("fulltext query failed: {error}")),
            Ok(bibtex) => {
                let title = bibtex_title(&bibtex);
                if !title.is_empty() && title_in_text(&title, txt) {
                    log::debug("fulltext query successful");
                    return Ok(bibtex);
                }
                let short: String = title.chars().take(100).collect();
                log::warning(&format!(
                    "fulltext query: title not found in the text: {short}"
                ));
            }
        }
    }
    Err(value_error(NO_METADATA))
}

/// `_metadata_from_pdf_contents`: the network part of `extract_pdf_metadata`; `read_text`
/// gives the text of the first pages and of the last page.
pub fn metadata_from_pdf_contents(
    fetcher: &Fetcher,
    metadata_doi: Option<&str>,
    read_text: impl FnOnce() -> Result<(String, String)>,
    options: &Options,
) -> Result<String> {
    // a DOI in the PDF metadata is the article's own: no need to parse the text
    if options.search_doi
        && let Some(metadata_doi) = metadata_doi
    {
        log::info(&format!("found doi in pdf metadata: {metadata_doi}"));
        match fetcher.fetch_bibtex_by_doi(metadata_doi) {
            Ok(b) => return Ok(b),
            Err(error) if error.is_request_error() => {
                log::warning(&format!(
                    "{metadata_doi}: no metadata found ({error}), search the text"
                ));
            }
            Err(error) => return Err(error),
        }
    }
    let (txt, tail) = read_text()?;
    extract_txt_metadata(fetcher, &txt, options, &tail)
}

/// `extract_pdf_metadata`: the bibtex of a PDF.
pub fn extract_pdf_metadata(fetcher: &Fetcher, pdf: &str, options: &Options) -> Result<String> {
    let file = PdfFile::new(pdf);
    let metadata_doi = if options.search_doi {
        file.metadata_doi()?
    } else {
        None
    };
    let read_text = || {
        let head = file.head(options.maxpages, options.minwords, options.image)?;
        let tail = if options.image {
            String::new()
        } else {
            file.tail()?
        };
        Ok((head, tail))
    };
    metadata_from_pdf_contents(fetcher, metadata_doi.as_deref(), read_text, options)
}
