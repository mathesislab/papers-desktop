//! BibJSON (spec `formats.md` 5): the bibtex fields as they are, with the
//! structures BibJSON gives some of them.

use super::json::{dumps, put};
use super::names::{Sep, split_depth0};
use crate::bib::Entry;
use crate::history::ojson::J;

/// The BibJSON record of an entry: `type` and `id`, the fields in the order
/// of the entry (`author` and `editor` as lists of `{"name": ...}`, `journal` as
/// `{"name": ...}`), then the file links.
pub fn entry_to_bibjson(entry: &Entry, files: &[String]) -> J {
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
            _ => J::str(&field.value),
        };
        put(&mut record, &field.key, value);
    }
    if !files.is_empty() {
        put(&mut record, "file", J::strs(files));
    }
    J::Obj(record)
}

/// The text of the BibJSON file of some entries.
pub fn dumps_bibjson(entries: &[&Entry], files: &dyn Fn(&Entry) -> Vec<String>) -> String {
    let records: Vec<J> = entries
        .iter()
        .map(|e| entry_to_bibjson(e, &files(e)))
        .collect();
    dumps(&J::Obj(vec![
        (
            "metadata".into(),
            J::Obj(vec![("records".into(), J::Int(records.len() as i64))]),
        ),
        ("records".into(), J::Arr(records)),
    ]))
}
