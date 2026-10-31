//! CSL-JSON (spec `formats.md` 3): a bibtex entry as a CSL-JSON item.

use super::common::{clean_doi, date_parts, entry_date_parts, pages_to_bibtex};
use super::json::{dumps, put};
use super::names::{Name, parse_bibtex_names};
use crate::bib::Entry;
use crate::bib::pystr::strip;
use crate::bib::unicode::entry_unicode_fields;
use crate::entry::val;
use crate::history::ojson::J;

/// The CSL type of a bibtex entry type (lower case).
pub fn csl_type(entry_type: &str) -> &'static str {
    match entry_type {
        "article" => "article-journal",
        "book" | "proceedings" => "book",
        "booklet" => "pamphlet",
        "inbook" | "incollection" => "chapter",
        "inproceedings" | "conference" => "paper-conference",
        "phdthesis" | "mastersthesis" | "thesis" => "thesis",
        "techreport" | "report" | "manual" => "report",
        "misc" => "document",
        "online" | "electronic" | "www" => "webpage",
        "dataset" => "dataset",
        "software" => "software",
        "unpublished" => "manuscript",
        "patent" => "patent",
        _ => "document",
    }
}

/// The fields with the same meaning in bibtex and CSL, no conversion (bibtex name, CSL name).
const SIMPLE: [(&str, &str); 15] = [
    ("title", "title"),
    ("volume", "volume"),
    ("doi", "DOI"),
    ("url", "URL"),
    ("isbn", "ISBN"),
    ("issn", "ISSN"),
    ("publisher", "publisher"),
    ("address", "publisher-place"),
    ("abstract", "abstract"),
    ("note", "note"),
    ("edition", "edition"),
    ("language", "language"),
    ("series", "collection-title"),
    ("pmid", "PMID"),
    ("pmcid", "PMCID"),
];

/// The fields of an entry as unicode text (`_unicode_fields`), looked up by name.
pub(crate) struct Fields(Vec<(String, String)>);

impl Fields {
    pub(crate) fn of(entry: &Entry) -> Fields {
        Fields(entry_unicode_fields(entry))
    }

    /// The value of a field, empty if it has none (`f.get(name)`).
    pub(crate) fn get(&self, name: &str) -> &str {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map_or("", |(_, v)| v.as_str())
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.0.iter().any(|(k, _)| k == name)
    }
}

pub(crate) fn name_json(name: &Name) -> J {
    let mut object: Vec<(String, J)> = Vec::new();
    if let Some(literal) = &name.literal {
        object.push(("literal".into(), J::str(literal)));
        return J::Obj(object);
    }
    for (key, part) in [
        ("family", &name.family),
        ("given", &name.given),
        ("non-dropping-particle", &name.non_dropping_particle),
        ("suffix", &name.suffix),
    ] {
        if let Some(part) = part {
            object.push((key.into(), J::str(part)));
        }
    }
    J::Obj(object)
}

fn date_json(parts: &[i128]) -> J {
    J::Obj(vec![(
        "date-parts".into(),
        J::Arr(vec![J::Arr(
            parts.iter().map(|&n| J::Int(n as i64)).collect(),
        )]),
    )])
}

/// A CSL-JSON item of a bibtex entry; `files`: the paths of its file links, as written;
/// `versions`: `{path: version}` of those whose version is known (`papers list`).
pub fn entry_to_csl(entry: &Entry, files: &[String], versions: &[(String, String)]) -> J {
    let f = Fields::of(entry);
    let entry_type = entry.entry_type.to_lowercase();
    let mut csl = csl_type(&entry_type);
    if entry_type == "misc" && !f.get("eprint").is_empty() {
        csl = "article";
    }
    let mut item: Vec<(String, J)> = vec![
        ("id".into(), J::str(&entry.key)),
        ("citation-key".into(), J::str(&entry.key)),
        ("type".into(), J::str(csl)),
    ];
    for name in ["author", "editor", "translator"] {
        let names = parse_bibtex_names(val(entry, name));
        if !names.is_empty() {
            put(
                &mut item,
                name,
                J::Arr(names.iter().map(name_json).collect()),
            );
        }
    }
    // the publisher of a thesis, a report... is at the position of `publisher`
    let publisher = ["publisher", "school", "institution", "organization"]
        .into_iter()
        .map(|name| f.get(name))
        .find(|v| !v.is_empty())
        .unwrap_or("");
    for (bib, key) in SIMPLE {
        let value = if bib == "publisher" {
            publisher
        } else {
            f.get(bib)
        };
        if !value.is_empty() {
            put(&mut item, key, J::str(value));
        }
    }
    if f.has("doi") {
        put(&mut item, "DOI", J::str(clean_doi(f.get("doi"))));
    }
    let container = [f.get("journal"), f.get("booktitle")]
        .into_iter()
        .find(|v| !v.is_empty());
    if let Some(container) = container {
        put(&mut item, "container-title", J::str(container));
    }
    let number = [f.get("issue"), f.get("number")]
        .into_iter()
        .find(|v| !v.is_empty());
    if let Some(number) = number {
        let key = if csl == "article-journal" || !f.get("issue").is_empty() {
            "issue"
        } else {
            "number"
        };
        put(&mut item, key, J::str(number));
    }
    if !f.get("pages").is_empty() {
        put(
            &mut item,
            "page",
            J::str(pages_to_bibtex(f.get("pages")).replace("--", "-")),
        );
    }
    if entry_type == "phdthesis" {
        put(&mut item, "genre", J::str("PhD thesis"));
    } else if entry_type == "mastersthesis" {
        put(&mut item, "genre", J::str("Master's thesis"));
    } else if !f.get("type").is_empty() && (csl == "thesis" || csl == "report") {
        put(&mut item, "genre", J::str(f.get("type")));
    }
    if !f.get("keywords").is_empty() {
        let keywords: Vec<&str> = f
            .get("keywords")
            .split(',')
            .map(strip)
            .filter(|w| !w.is_empty())
            .collect();
        put(&mut item, "keyword", J::str(keywords.join(", ")));
    }
    let parts = entry_date_parts(entry);
    if !parts.is_empty() {
        put(&mut item, "issued", date_json(&parts));
    } else if !f.get("year").is_empty() {
        put(
            &mut item,
            "issued",
            J::Obj(vec![("literal".into(), J::str(f.get("year")))]),
        );
    }
    let accessed = date_parts(f.get("urldate"));
    if !f.get("urldate").is_empty() && !accessed.is_empty() {
        put(&mut item, "accessed", date_json(&accessed));
    }
    if !files.is_empty() {
        let mut custom = vec![("file".into(), J::strs(files))];
        if !versions.is_empty() {
            custom.push(("file-version".into(), super::file_versions_json(versions)));
        }
        put(&mut item, "custom", J::Obj(custom));
    }
    J::Obj(item)
}

/// The text of the CSL-JSON file of some entries; `versions`: the version of a file, if known.
pub fn dumps_csl(
    entries: &[&Entry],
    files: &dyn Fn(&Entry) -> Vec<String>,
    versions: &dyn Fn(&str) -> Option<String>,
) -> String {
    dumps(&J::Arr(
        entries
            .iter()
            .map(|e| {
                let paths = files(e);
                entry_to_csl(e, &paths, &super::known_versions(&paths, versions))
            })
            .collect(),
    ))
}
