use std::path::{Path, PathBuf};

use papers_core::{FileState, Library, Matching, TagCount};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn library() -> Library {
    Library::load(&[fixture("table.bib")]).unwrap()
}

#[test]
fn rows() {
    let lib = library();
    let rows = lib.rows();
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    // newest first, then by first author and title
    assert_eq!(
        ids,
        [
            "same2020",
            "preprint2020",
            "twin2019",
            "twin2019b",
            "twin2018"
        ]
    );
    let same = &rows[0];
    assert_eq!(same.authors, "Lind & Berg");
    assert_eq!(same.year, Some(2020));
    assert_eq!(same.title, "The same paper");
    assert_eq!(same.tags, ["AMOC", "tipping"]);
    assert_eq!(same.file, FileState::Found);
    assert_eq!(rows[1].file, FileState::None);
    assert_eq!(rows[2].authors, "Holm et al.");
    assert_eq!(rows[2].file, FileState::Missing);
}

#[test]
fn matching() {
    let lib = library();
    assert_eq!(lib.matching("", Matching::WordStart).len(), 5);
    assert_eq!(
        lib.matching("twin -year:2018", Matching::WordStart),
        ["twin2019", "twin2019b"]
    );
    assert_eq!(
        lib.matching("tag:amoc", Matching::WordStart),
        ["same2020", "preprint2020"]
    );
}

#[test]
fn tags() {
    let lib = library();
    let tag = |tag: &str, count| TagCount {
        tag: tag.to_string(),
        count,
    };
    // "AMOC" and "amoc" are one tag, named as the newest paper writes it
    assert_eq!(lib.tags(), [tag("AMOC", 2), tag("tipping", 1)]);
}

#[test]
fn duplicates() {
    let lib = library();
    // the same DOI (whatever its case); the same title and year
    assert_eq!(
        lib.duplicates(),
        [
            vec!["same2020", "preprint2020"],
            vec!["twin2019", "twin2019b"]
        ]
    );
}
