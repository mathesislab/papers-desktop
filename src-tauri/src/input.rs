//! What the user gives the sheet that adds a paper, and the text of the entry it works on: the
//! kind of what was typed or pasted in its field, and the merge of a fetched entry into the text
//! of an entry (Fill in, Update).

use std::path::PathBuf;

use papers_rs::bib::{Bib, Block};
use papers_rs::merge::Merge;
use serde::Serialize;

use crate::fetch::doi_of;
use crate::settings::expand;

/// What a text in the field of the sheet is taken for.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Input {
    Empty,
    Bibtex,
    /// a DOI, without its link or `doi:`
    Doi {
        doi: String,
    },
    /// a PDF on this computer
    Pdf {
        path: PathBuf,
    },
    /// a link to a PDF
    PdfUrl {
        url: String,
    },
    /// a link to a page, a journal article's, we hope
    Url {
        url: String,
    },
    Other {
        message: String,
    },
}

/// What a text is: BibTeX, a DOI (or its doi.org link), a link, or the path of a PDF (as typed,
/// quoted or with escaped spaces, a `file://` link, with `~`).
pub fn classify(text: &str) -> Input {
    let text = text.trim();
    if text.is_empty() {
        return Input::Empty;
    }
    if text.starts_with('@') {
        return Input::Bibtex;
    }
    if let Some(doi) = doi_of(text) {
        return Input::Doi { doi };
    }
    if text.starts_with("http://") || text.starts_with("https://") {
        let path = text.split(['?', '#']).next().unwrap_or("");
        return if path.to_lowercase().ends_with(".pdf") {
            Input::PdfUrl {
                url: text.to_string(),
            }
        } else {
            Input::Url {
                url: text.to_string(),
            }
        };
    }
    match path_of(text) {
        Some(path) if path.is_file() => {
            let pdf = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
            if pdf {
                Input::Pdf { path }
            } else {
                Input::Other {
                    message: format!("{}: not a PDF", path.display()),
                }
            }
        }
        Some(path) => Input::Other {
            message: format!("{}: no such file", path.display()),
        },
        None => Input::Other {
            message: "Not a DOI, a link, a PDF path or BibTeX".into(),
        },
    }
}

/// The path that a text is, if it looks like one: absolute or with `~`, a `file://` link,
/// quoted, or with its spaces escaped as a terminal writes them.
fn path_of(text: &str) -> Option<PathBuf> {
    let quoted = |q: char| text.len() > 1 && text.starts_with(q) && text.ends_with(q);
    let text = if quoted('"') || quoted('\'') {
        &text[1..text.len() - 1]
    } else {
        text
    };
    let path = match text.strip_prefix("file://") {
        Some(rest) => unquote(rest.strip_prefix("localhost").unwrap_or(rest)),
        None => text.replace("\\ ", " "),
    };
    let path = expand(&path);
    path.is_absolute().then_some(path)
}

/// `%20` and the like in a `file://` link, as UTF-8.
fn unquote(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        match (bytes[i], hex.and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What the text in the field is, for the sheet.
#[tauri::command]
pub fn classify_input(text: String) -> Input {
    classify(&text)
}

/// The first entry of a text of BibTeX.
pub(crate) fn entry_of(text: &str, what: &str) -> Result<papers_rs::bib::Entry, String> {
    Bib::parse(text)
        .entries()
        .next()
        .cloned()
        .ok_or_else(|| format!("{what} has no entry"))
}

/// The entry of `current` with what an entry fetched for the same paper has: the fields it
/// lacks are added; where the values differ, `current`'s are kept, or with `update` the fetched
/// ones taken. As papers merges an entry that is a paper of the library already (`add
/// --on-duplicate keep` and `update`): the key stays. Written as papers writes an entry.
pub fn merge_entries(current: &str, fetched: &str, update: bool) -> Result<String, String> {
    let entries = [
        entry_of(current, "The BibTeX")?,
        entry_of(fetched, "The fetched entry")?,
    ];
    let mut merge = Merge::new(&entries, None, false);
    if update {
        merge.choose_all(1);
    }
    let merged = Bib {
        blocks: vec![Block::Entry(merge.result())],
    };
    Ok(merged.to_string().trim().to_string())
}

/// The text of an entry with the fetched entry merged into it, for the sheet and the edit.
#[tauri::command]
pub fn merge_fetched(current: String, fetched: String, update: bool) -> Result<String, String> {
    merge_entries(&current, &fetched, update)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(text: &str) -> String {
        match classify(text) {
            Input::Empty => "empty".into(),
            Input::Bibtex => "bibtex".into(),
            Input::Doi { doi } => format!("doi {doi}"),
            Input::Pdf { path } => format!("pdf {}", path.display()),
            Input::PdfUrl { url } => format!("pdf-url {url}"),
            Input::Url { url } => format!("url {url}"),
            Input::Other { message } => format!("other {message}"),
        }
    }

    #[test]
    fn dois_links_and_bibtex() {
        assert_eq!(kind("  "), "empty");
        assert_eq!(kind("@article{a, title = {A}}"), "bibtex");
        assert_eq!(
            kind("https://doi.org/10.1038/171737a0"),
            "doi 10.1038/171737a0"
        );
        assert_eq!(kind("doi: 10.1038/171737a0"), "doi 10.1038/171737a0");
        assert_eq!(
            kind("https://tc.copernicus.org/articles/11/1647/2017/"),
            "url https://tc.copernicus.org/articles/11/1647/2017/"
        );
        assert_eq!(
            kind("https://x.org/a/paper.PDF?download=1"),
            "pdf-url https://x.org/a/paper.PDF?download=1"
        );
        assert_eq!(
            kind("some words"),
            "other Not a DOI, a link, a PDF path or BibTeX"
        );
    }

    #[test]
    fn the_path_of_a_pdf_as_it_may_be_pasted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("my paper.pdf");
        std::fs::write(&path, "%PDF").unwrap();
        let text = std::fs::write(dir.path().join("a.txt"), "x")
            .map(|_| dir.path().join("a.txt"))
            .unwrap();
        let want = format!("pdf {}", path.display());
        let p = path.display().to_string();
        assert_eq!(kind(&p), want);
        assert_eq!(kind(&format!("  {p}\n")), want);
        assert_eq!(kind(&format!("'{p}'")), want);
        assert_eq!(kind(&format!("\"{p}\"")), want);
        assert_eq!(kind(&p.replace(' ', "\\ ")), want);
        assert_eq!(kind(&format!("file://{}", p.replace(' ', "%20"))), want);
        assert_eq!(kind(&format!("file://localhost{p}")), want);
        // not a PDF, not there
        assert_eq!(
            kind(&text.display().to_string()),
            format!("other {}: not a PDF", text.display())
        );
        let gone = dir.path().join("gone.pdf");
        assert_eq!(
            kind(&gone.display().to_string()),
            format!("other {}: no such file", gone.display())
        );
    }

    const CURRENT: &str = "@article{doe2019,\n author = {Doe, Jane},\n journal = {J},\n keywords = {ice},\n title = {A study},\n year = {2019}\n}";
    const FETCHED: &str = "@article{doe_2019,\n author = {Doe, Jane},\n doi = {10.1/x},\n journal = {Journal of Things},\n title = {A study},\n volume = {3},\n year = {2019}\n}";

    #[test]
    fn fill_in_adds_the_fields_that_are_lacking() {
        let merged = merge_entries(CURRENT, FETCHED, false).unwrap();
        assert!(merged.starts_with("@article{doe2019,"), "{merged}");
        assert!(
            merged.contains("doi = {10.1/x}") && merged.contains("volume = {3}"),
            "{merged}"
        );
        assert!(merged.contains("journal = {J}"), "{merged}");
        assert!(merged.contains("keywords = {ice}"), "{merged}");
    }

    #[test]
    fn update_takes_the_fetched_values_too() {
        let merged = merge_entries(CURRENT, FETCHED, true).unwrap();
        assert!(merged.starts_with("@article{doe2019,"), "{merged}");
        assert!(merged.contains("journal = {Journal of Things}"), "{merged}");
        assert!(merged.contains("keywords = {ice}"), "{merged}");
    }

    #[test]
    fn no_entry_to_merge() {
        assert_eq!(
            merge_entries("", FETCHED, false).unwrap_err(),
            "The BibTeX has no entry"
        );
        assert_eq!(
            merge_entries(CURRENT, "x", false).unwrap_err(),
            "The fetched entry has no entry"
        );
    }
}
