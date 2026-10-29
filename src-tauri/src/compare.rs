//! The comparison of an entry to add with the paper of the library that is the same: the fields
//! that differ, as papers' two-way merge finds them (`add --on-duplicate ask`: the same values by
//! its normal form are not shown, a field the library lacks is added, the others are conflicts),
//! and the entry made of the library's with the values chosen. The sheet that adds a paper shows
//! the one and writes the other.

use papers_rs::bib::{Bib, Block};
use papers_rs::merge::{ENTRYTYPE, Merge, Status};
use serde::Serialize;
use tauri::State;

use crate::AppState;
use crate::input::entry_of;

/// The fields of the library entry that a new entry does not change: the tags have their own
/// commands (an edit of an entry keeps them), as the key and the files, which a merge does not
/// compare either.
const LIBRARY_ONLY: [&str; 1] = ["keywords"];

/// Whose value a field has by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Library,
    New,
}

/// A field whose values differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Difference {
    /// the name of the field; the type of the entry is `ENTRYTYPE`, as in papers' dialogue
    pub field: String,
    /// the library's value; none: the library lacks the field
    pub library: Option<String>,
    pub new: String,
    /// the library's value is kept, but a field it lacks is filled in (papers' `keep`)
    pub default: Side,
}

/// What the new entry has that the paper of the library does not.
#[derive(Debug, Clone, Serialize)]
pub struct Comparison {
    /// the key of the paper in the library
    pub key: String,
    pub differences: Vec<Difference>,
    /// whether the paper has a file
    pub files: bool,
}

/// The fields of `new` whose values differ from those of `library`.
pub fn differences(library: &papers_rs::bib::Entry, new: &papers_rs::bib::Entry) -> Vec<Difference> {
    let merge = Merge::new(&[library.clone(), new.clone()], None, false);
    merge
        .fields
        .iter()
        .filter(|f| f.status != Status::Same && !LIBRARY_ONLY.contains(&f.name.as_str()))
        .filter_map(|f| {
            Some(Difference {
                field: f.name.clone(),
                library: f.values[0].clone(),
                new: f.values[1].clone()?,
                default: if f.status == Status::Added { Side::New } else { Side::Library },
            })
        })
        .collect()
}

/// The entry of the library with the values of the new entry for the fields in `take`; its key,
/// tags and files, and the fields not in `take`, are the library's. Written as papers writes an entry.
pub fn merged(library: &papers_rs::bib::Entry, new: &papers_rs::bib::Entry, take: &[String]) -> String {
    let mut entry = library.clone();
    entry.source = None;
    for d in differences(library, new).iter().filter(|d| take.contains(&d.field)) {
        if d.field == ENTRYTYPE {
            entry.entry_type = d.new.clone();
        } else {
            entry.set(&d.field, d.new.clone());
        }
    }
    let bib = Bib {
        blocks: vec![Block::Entry(entry)],
    };
    bib.to_string().trim().to_string()
}

/// The two entries: the library's (as written in its .bib file) and the first of the text.
fn entries(state: &AppState, id: &str, text: &str) -> Result<(papers_rs::bib::Entry, papers_rs::bib::Entry, bool), String> {
    let library = state.library.read().unwrap();
    let raw = library.raw(id).ok_or_else(|| format!("{id}: not in the library"))?;
    let files = library.get(id).is_some_and(|it| !it.files.is_empty());
    Ok((entry_of(raw, "The paper")?, entry_of(text, "The BibTeX")?, files))
}

/// The fields of the first entry of a text that differ from the paper of the library it is the same as.
#[tauri::command]
pub fn compare_entry(state: State<AppState>, id: String, text: String) -> Result<Comparison, String> {
    let (library, new, files) = entries(&state, &id, &text)?;
    Ok(Comparison {
        key: library.key.clone(),
        differences: differences(&library, &new),
        files,
    })
}

/// The text of the paper of the library with the values of the first entry of a text for the
/// fields in `take`: what an edit of the paper writes.
#[tauri::command]
pub fn merge_entry(state: State<AppState>, id: String, text: String, take: Vec<String>) -> Result<String, String> {
    let (library, new, _) = entries(&state, &id, &text)?;
    Ok(merged(&library, &new, &take))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARY: &str = "@article{doe2019,\n author = {Doe, J. and Roe, R.},\n journal = {J},\n keywords = {ice},\n title = {A study},\n year = {2019},\n file = {:files/doe.pdf:pdf}\n}";
    const NEW: &str = "@article{Doe_2019,\n author = {Doe, Jane and Roe, Richard},\n doi = {10.1/X},\n journal = {Journal of Things},\n keywords = {sea, ocean},\n title = {A Study},\n volume = {3},\n year = {2019},\n file = {:other/new.pdf:pdf}\n}";

    fn entry(text: &str) -> papers_rs::bib::Entry {
        entry_of(text, "x").unwrap()
    }

    fn rows() -> Vec<(String, Option<String>, String, Side)> {
        differences(&entry(LIBRARY), &entry(NEW))
            .into_iter()
            .map(|d| (d.field, d.library, d.new, d.default))
            .collect()
    }

    #[test]
    fn only_the_fields_that_differ_by_papers_normal_form() {
        // the author (fuller names), the title (case) and the year are the same; the tags and the
        // links are not compared; the journal conflicts, the DOI and the volume are to fill in
        assert_eq!(
            rows(),
            [
                ("journal".to_string(), Some("J".to_string()), "Journal of Things".to_string(), Side::Library),
                ("doi".to_string(), None, "10.1/X".to_string(), Side::New),
                ("volume".to_string(), None, "3".to_string(), Side::New),
            ]
        );
        assert!(differences(&entry(LIBRARY), &entry(LIBRARY)).is_empty());
        // the type is a field too
        let misc = NEW.replace("@article", "@misc");
        assert_eq!(differences(&entry(LIBRARY), &entry(&misc))[0].field, ENTRYTYPE);
    }

    #[test]
    fn the_entry_has_the_values_taken_and_the_rest_of_the_library() {
        // (papers writes the fields in order of their names)
        let pairs = |e: &papers_rs::bib::Entry| {
            let mut fields: Vec<_> = e.fields.iter().map(|f| (f.key.clone(), f.value.clone())).collect();
            fields.sort();
            fields
        };
        let nothing = entry(&merged(&entry(LIBRARY), &entry(NEW), &[]));
        assert_eq!(pairs(&nothing), pairs(&entry(LIBRARY)));
        let take = |fields: &[&str]| merged(&entry(LIBRARY), &entry(NEW), &fields.iter().map(|f| f.to_string()).collect::<Vec<_>>());
        let one = entry(&take(&["doi"]));
        assert_eq!((one.key.as_str(), one.get("doi"), one.get("journal"), one.get("volume")), ("doe2019", Some("10.1/X"), Some("J"), None));
        let all = entry(&take(&["journal", "doi", "volume"]));
        assert_eq!((all.get("journal"), all.get("volume")), (Some("Journal of Things"), Some("3")));
        // the tags, the links and the names of the library stay
        assert_eq!(all.get("keywords"), Some("ice"));
        assert_eq!(all.get("file"), Some(":files/doe.pdf:pdf"));
        assert_eq!(all.get("author"), Some("Doe, J. and Roe, R."));
        assert_eq!(all.get("title"), Some("A study"));
        let misc = merged(&entry(LIBRARY), &entry(&NEW.replace("@article", "@misc")), &[ENTRYTYPE.to_string()]);
        assert!(misc.starts_with("@misc{doe2019,"), "{misc}");
    }
}
