//! The bibliography formats other than bibtex (`papers.formats`, spec
//! `formats.md`): RIS, CSL-JSON and BibJSON, and the JSON text they are
//! written in.
//!
//! - [`csl`]: a bibtex entry as a CSL-JSON item;
//! - [`ris`]: as a RIS record (written as the `rispy` package does);
//! - [`bibjson`]: as a BibJSON record;
//! - [`names`]: the names of an `author`, `editor` or `translator`, as the
//!   parts of CSL names, and back;
//! - [`import`]: the other direction, the text of a file as bibtex entries (`add`, `edit --from`);
//! - [`line`]: the lines of `papers list`;
//! - [`json`]: the JSON text (two-space indent, no escaped non-ASCII);
//! - [`common`]: dates, pages, DOIs, keys.
//!
//! The values are converted from LaTeX to unicode on export (RIS, CSL-JSON:
//! [`crate::bib::unicode::value_to_unicode`]); BibJSON keeps them as they are.
//! The file links are given by the caller (`files`: entry to paths, written
//! as given), and the version of the PDFs (`versions`, `papers list`).
//!
//! ```
//! use papers_rs::bib::Bib;
//! use papers_rs::formats::{Format, dumps, no_versions};
//!
//! let bib = Bib::parse("@article{k, author = {Perrette, M. and van der Berg, Jan}, title = {G{\\\"o}ttingen}, year = 2013}");
//! let entries: Vec<_> = bib.entries().collect();
//! let ris = dumps(&entries, Format::Ris, &|_| vec![], &no_versions);
//! assert_eq!(ris, "TY  - JOUR\nID  - k\nAU  - Perrette, M.\nAU  - van der Berg, Jan\nTI  - Göttingen\nPY  - 2013\nER  - \n");
//! ```

pub mod bibjson;
pub mod common;
pub mod csl;
pub mod import;
pub mod json;
pub mod line;
pub mod names;
pub mod ris;

use crate::bib::Entry;
use crate::history::ojson::J;

/// The formats, in the order that matters (`export`, the options).
pub const FORMATS: [Format; 4] = [
    Format::Bibtex,
    Format::Ris,
    Format::CslJson,
    Format::BibJson,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Bibtex,
    Ris,
    CslJson,
    BibJson,
}

impl Format {
    /// The id of the format, as the options write it.
    pub fn id(self) -> &'static str {
        match self {
            Format::Bibtex => "bibtex",
            Format::Ris => "ris",
            Format::CslJson => "csl-json",
            Format::BibJson => "bibjson",
        }
    }

    pub fn from_id(id: &str) -> Option<Format> {
        FORMATS.into_iter().find(|f| f.id() == id)
    }

    /// How the format is named in messages.
    pub fn label(self) -> &'static str {
        match self {
            Format::Bibtex => "bibtex",
            Format::Ris => "RIS",
            Format::CslJson => "CSL-JSON",
            Format::BibJson => "BibJSON",
        }
    }

    /// The name of the file that `papers export` writes.
    pub fn filename(self) -> &'static str {
        match self {
            Format::Bibtex => "papers.bib",
            Format::Ris => "papers.ris",
            Format::CslJson => "papers.json",
            Format::BibJson => "papers.bib.json",
        }
    }
}

/// The format of a file to add, from its name: `forced` if given, else from the
/// extension, the longest first (`.bib.json` before `.json`); bibtex by default
/// (`file_format`).
pub fn file_format(path: &str, forced: Option<Format>) -> Format {
    if let Some(f) = forced {
        return f;
    }
    let name = crate::config::paths::basename(path).to_lowercase();
    for (extension, format) in [
        (".bib.json", Format::BibJson),
        (".json", Format::CslJson),
        (".ris", Format::Ris),
        (".bib", Format::Bibtex),
    ] {
        if name.ends_with(extension) {
            return format;
        }
    }
    Format::Bibtex
}

/// The entries as a text in a format other than bibtex (`dumps`); `files`: the
/// paths of the file links of an entry, as written; `versions`: the version of the PDF of a path
/// (`papers list`), written in CSL-JSON and BibJSON.
///
/// # Panics
/// For bibtex, which the library writes ([`crate::bib::Bib::format`]).
pub fn dumps(
    entries: &[&Entry],
    format: Format,
    files: &dyn Fn(&Entry) -> Vec<String>,
    versions: &dyn Fn(&str) -> Option<String>,
) -> String {
    match format {
        Format::Ris => ris::dumps_ris(entries, files),
        Format::CslJson => csl::dumps_csl(entries, files, versions),
        Format::BibJson => bibjson::dumps_bibjson(entries, files, versions),
        Format::Bibtex => panic!("bibtex is written by the library"),
    }
}

/// No version known (`papers export`, the tests).
pub fn no_versions(_: &str) -> Option<String> {
    None
}

/// `{path: version}` of the files whose version is known (`_versions`), in their order.
fn known_versions(
    files: &[String],
    versions: &dyn Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    files
        .iter()
        .filter_map(|f| versions(f).map(|v| (f.clone(), v)))
        .collect()
}

/// The `file-version` object of CSL-JSON and BibJSON.
fn file_versions_json(versions: &[(String, String)]) -> J {
    J::Obj(
        versions
            .iter()
            .map(|(path, version)| (path.clone(), J::str(version)))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_of_files() {
        assert_eq!(file_format("a/Refs.BIB.JSON", None), Format::BibJson);
        assert_eq!(file_format("refs.json", None), Format::CslJson);
        assert_eq!(file_format("x.ris", None), Format::Ris);
        assert_eq!(file_format("x.txt", None), Format::Bibtex);
        assert_eq!(file_format("x.ris", Some(Format::CslJson)), Format::CslJson);
    }
}
