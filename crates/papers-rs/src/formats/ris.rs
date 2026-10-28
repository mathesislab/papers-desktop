//! RIS (spec `formats.md` 4): a bibtex entry as a RIS record, written as
//! the `rispy` package writes it (the tags in the order of the record, no
//! record number before each).

use super::common::{clean_doi, entry_date_parts, split_pages};
use super::csl::Fields;
use super::names::{parse_bibtex_names, ris_name};
use crate::bib::Entry;
use crate::bib::pystr::strip;
use crate::entry::val;

/// The RIS type of a bibtex entry type (lower case).
pub fn ris_type(entry_type: &str) -> &'static str {
    match entry_type {
        "article" => "JOUR",
        "book" => "BOOK",
        "booklet" => "PAMP",
        "inbook" | "incollection" => "CHAP",
        "inproceedings" | "conference" => "CPAPER",
        "proceedings" => "CONF",
        "phdthesis" | "mastersthesis" | "thesis" => "THES",
        "techreport" | "report" | "manual" => "RPRT",
        "misc" => "GEN",
        "online" | "electronic" | "www" => "ELEC",
        "dataset" => "DATA",
        "software" => "COMP",
        "unpublished" => "UNPB",
        "patent" => "PAT",
        _ => "GEN",
    }
}

/// The lines (tag, value) of the RIS record of an entry, `TY` first and without `ER`;
/// `files`: the paths of its file links, as written.
pub fn entry_to_ris(entry: &Entry, files: &[String]) -> Vec<(&'static str, String)> {
    let f = Fields::of(entry);
    let entry_type = entry.entry_type.to_lowercase();
    let mut lines: Vec<(&'static str, String)> = vec![
        ("TY", ris_type(&entry_type).to_string()),
        ("ID", entry.key.clone()),
    ];
    for (tag, field) in [("AU", "author"), ("ED", "editor")] {
        for name in parse_bibtex_names(val(entry, field)) {
            lines.push((tag, ris_name(&name)));
        }
    }
    let first = |names: &[&str]| names.iter().map(|n| f.get(n)).find(|v| !v.is_empty());
    if let Some(title) = first(&["title"]) {
        lines.push(("TI", title.to_string()));
    }
    if let Some(container) = first(&["journal", "booktitle"]) {
        lines.push(("T2", container.to_string()));
    }
    if let Some(series) = first(&["series"]) {
        lines.push(("T3", series.to_string()));
    }
    let parts = entry_date_parts(entry);
    if !parts.is_empty() {
        lines.push(("PY", parts[0].to_string()));
        if parts.len() > 1 {
            let date: Vec<String> = parts
                .iter()
                .enumerate()
                .map(|(i, x)| {
                    if i > 0 {
                        format!("{x:02}")
                    } else {
                        x.to_string()
                    }
                })
                .collect();
            lines.push(("DA", format!("{}/", date.join("/"))));
        }
    } else if !f.get("year").is_empty() {
        lines.push(("PY", f.get("year").to_string()));
    }
    if let Some(volume) = first(&["volume"]) {
        lines.push(("VL", volume.to_string()));
    }
    if let Some(number) = first(&["issue", "number"]) {
        lines.push(("IS", number.to_string()));
    }
    if !f.get("pages").is_empty() {
        let (start, end) = split_pages(f.get("pages"));
        if !start.is_empty() {
            lines.push(("SP", start));
        }
        if !end.is_empty() {
            lines.push(("EP", end));
        }
    }
    if f.has("doi") {
        lines.push(("DO", clean_doi(f.get("doi"))));
    }
    if let Some(url) = first(&["url"]) {
        lines.push(("UR", url.to_string()));
    }
    for name in ["isbn", "issn"] {
        if let Some(number) = first(&[name]) {
            lines.push(("SN", number.to_string()));
        }
    }
    if let Some(publisher) = first(&["publisher", "school", "institution", "organization"]) {
        lines.push(("PB", publisher.to_string()));
    }
    if let Some(address) = first(&["address"]) {
        lines.push(("CY", address.to_string()));
    }
    if let Some(r#abstract) = first(&["abstract"]) {
        lines.push(("AB", r#abstract.to_string()));
    }
    if !f.get("keywords").is_empty() {
        for word in f
            .get("keywords")
            .split(',')
            .map(strip)
            .filter(|w| !w.is_empty())
        {
            lines.push(("KW", word.to_string()));
        }
    }
    if let Some(note) = first(&["note"]) {
        lines.push(("N1", note.to_string()));
    }
    if let Some(edition) = first(&["edition"]) {
        lines.push(("ET", edition.to_string()));
    }
    if let Some(language) = first(&["language"]) {
        lines.push(("LA", language.to_string()));
    }
    if !f.get("type").is_empty()
        && matches!(
            entry_type.as_str(),
            "techreport" | "report" | "phdthesis" | "mastersthesis" | "thesis"
        )
    {
        lines.push(("M3", f.get("type").to_string()));
    }
    for file in files {
        lines.push(("L1", file.clone()));
    }
    lines
}

/// The text of the RIS file of some entries: records separated by an empty
/// line, each ending with `ER  - `; empty for no entry.
pub fn dumps_ris(entries: &[&Entry], files: &dyn Fn(&Entry) -> Vec<String>) -> String {
    let mut lines: Vec<String> = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        for (tag, value) in entry_to_ris(entry, &files(entry)) {
            lines.push(format!("{tag}  - {value}"));
        }
        lines.push("ER  - ".to_string());
        if i + 1 < entries.len() {
            lines.push(String::new());
        }
    }
    lines.push(String::new());
    lines.join("\n")
}
