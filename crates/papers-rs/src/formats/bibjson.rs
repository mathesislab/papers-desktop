//! BibJSON (spec `formats.md` 5): the bibtex fields as they are, with the
//! structures BibJSON gives some of them.

use super::json::{dumps, put};
use super::names::{Sep, split_depth0};
use crate::bib::Entry;
use crate::history::ojson::J;

/// The keys that BibJSON gives a meaning (and `file-version`, that `papers list` writes): a bibtex
/// field of that name is written as `bibtex:NAME`, and a record's key of that name is not read as
/// a field.
pub(crate) const SKIPPED: [&str; 9] = [
    "type",
    "id",
    "cid",
    "collection",
    "file",
    "file-version",
    "identifier",
    "link",
    "license",
];

/// The namespace of the keys of bibtex fields that BibJSON gives a meaning, and its URL (in the
/// metadata of the collection).
pub(crate) const NAMESPACE: &str = "bibtex";
const NAMESPACE_URL: &str = "https://www.bibtex.org/Format/";

/// The BibJSON record of an entry: `type` and `id`, the fields in the order
/// of the entry (`author` and `editor` as lists of `{"name": ...}`, `journal` as
/// `{"name": ...}`, a field named as a key of BibJSON as `bibtex:NAME`), then the file links, then
/// `file-version`, `{path: version}` of the files whose version is known (`papers list`).
pub fn entry_to_bibjson(entry: &Entry, files: &[String], versions: &[(String, String)]) -> J {
    let mut record: Vec<(String, J)> = vec![
        ("type".into(), J::str(&entry.entry_type)),
        ("id".into(), J::str(&entry.key)),
    ];
    for field in &entry.fields {
        let value = match field.key.as_str() {
            "file" => continue,
            "author" | "editor" => J::Arr(
                split_depth0(&field.value, Sep::And)
                    .into_iter()
                    .map(|name| J::Obj(vec![("name".into(), J::Str(name))]))
                    .collect(),
            ),
            "journal" => J::Obj(vec![("name".into(), J::str(&field.value))]),
            name if SKIPPED.contains(&name) => {
                put(
                    &mut record,
                    &format!("{NAMESPACE}:{name}"),
                    J::str(&field.value),
                );
                continue;
            }
            _ => J::str(&field.value),
        };
        put(&mut record, &field.key, value);
    }
    if !files.is_empty() {
        put(&mut record, "file", J::strs(files));
        if !versions.is_empty() {
            put(
                &mut record,
                "file-version",
                super::file_versions_json(versions),
            );
        }
    }
    J::Obj(record)
}

/// The text of the BibJSON file of some entries; `versions`: the version of a file, if known.
pub fn dumps_bibjson(
    entries: &[&Entry],
    files: &dyn Fn(&Entry) -> Vec<String>,
    versions: &dyn Fn(&str) -> Option<String>,
) -> String {
    let records: Vec<J> = entries
        .iter()
        .map(|e| {
            let paths = files(e);
            entry_to_bibjson(e, &paths, &super::known_versions(&paths, versions))
        })
        .collect();
    let mut metadata = vec![("records".to_string(), J::Int(records.len() as i64))];
    let prefix = format!("{NAMESPACE}:");
    let namespaced = records.iter().any(|record| match record {
        J::Obj(items) => items.iter().any(|(name, _)| name.starts_with(&prefix)),
        _ => false,
    });
    if namespaced {
        metadata.push((
            "namespace".into(),
            J::Obj(vec![(NAMESPACE.into(), J::str(NAMESPACE_URL))]),
        ));
    }
    dumps(&J::Obj(vec![
        ("metadata".into(), J::Obj(metadata)),
        ("records".into(), J::Arr(records)),
    ]))
}
