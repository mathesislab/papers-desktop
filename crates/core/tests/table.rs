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
    assert_eq!(same.key, "same2020");
    assert_eq!(same.kind, "article");
    assert_eq!(same.doi.as_deref(), Some("10.1000/abc"));
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
    let tag = |tag: &str, count, under| TagCount {
        tag: tag.to_string(),
        count,
        under,
    };
    // "AMOC" and "amoc" are one tag, named as the newest paper writes it
    assert_eq!(lib.tags(), [tag("AMOC", 2, 2), tag("tipping", 1, 1)]);
}

#[test]
fn nested_tags() {
    let lib = Library::load(&[fixture("nested.bib")]).unwrap();
    let tag = |tag: &str, count, under| TagCount {
        tag: tag.to_string(),
        count,
        under,
    };
    // A tag counts the papers that have it, and `under` those under it too,
    // each once ("climate/amoc" twice in one paper is one). "climate" is
    // named as its paper writes it, not as the tags under it do; "ocean" no
    // paper has, and comes as written in a tag under it. The tags under
    // "climate" follow it, before "climate-review".
    assert_eq!(
        lib.tags(),
        [
            tag("Climate", 1, 3),
            tag("CLIMATE/amoc", 2, 3),
            tag("climate/amoc/tipping", 1, 1),
            tag("climate-review", 2, 2),
            tag("ice", 1, 1),
            tag("ocean", 0, 1),
            tag("ocean/amoc", 1, 1),
        ]
    );
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

#[test]
fn export() {
    let lib = library();
    let bib = lib.export(&["twin2019".to_string(), "same2020".to_string()]);
    // in the order asked, a .bib file of their own
    let twin = bib.find("{twin2019,").unwrap();
    let same = bib.find("{same2020,").unwrap();
    assert!(twin < same);
    let path = std::env::temp_dir().join(format!("papers-core-export-{}.bib", std::process::id()));
    std::fs::write(&path, &bib).unwrap();
    let again = Library::load(std::slice::from_ref(&path));
    std::fs::remove_file(&path).unwrap();
    assert_eq!(again.unwrap().len(), 2);
}
