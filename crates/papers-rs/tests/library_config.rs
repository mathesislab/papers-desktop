//! The library of a configuration (`Library::open`): its bibtex, its templates, and its file
//! links as the `paths` setting says (`papers.library.get_biblio`).

use std::path::Path;

use papers_rs::bib::Entry;
use papers_rs::config::{Config, MachineConfig};
use papers_rs::library::{Library, SaveStatus};
use serde_json::json;

fn config_of(root: &str, paths: &str) -> Config {
    let mut config = Config::new(MachineConfig::empty());
    config
        .set("bibtex", json!(format!("{root}/lib/papers.bib")))
        .unwrap();
    config
        .set("filesdir", json!(format!("{root}/lib/files")))
        .unwrap();
    config.set("paths", json!(paths)).unwrap();
    config
}

#[test]
fn a_library_of_a_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    // the bibtex is created, the templates are those of the configuration
    let mut config = config_of(&root, "relative");
    config
        .set("name.template", json!("{year}_{author}"))
        .unwrap();
    config.set("key.author-num", json!(1)).unwrap();
    let lib = Library::open(&config).unwrap();
    assert!(Path::new(&format!("{root}/lib/papers.bib")).exists());
    assert_eq!(lib.relative_to, format!("{root}/lib"));
    assert_eq!(lib.nameformat.template, "{year}_{author}");
    assert_eq!(lib.keyformat.author_num, 1);
    let mut e = Entry::new("article", "x");
    e.set("author", "Doe, Jane and Roe, R.");
    e.set("year", "2020");
    e.set("title", "Ice");
    assert_eq!(lib.generate_key(&e).unwrap(), "doe2020");
    assert_eq!(lib.nameformat.render(&e).unwrap(), "2020_doe_roe");
    // a configuration without bibtex
    let none = Config::new(MachineConfig::empty());
    assert!(
        Library::open(&none)
            .unwrap_err()
            .message()
            .contains("bibtex is not initialized")
    );

    // the links are written as the `paths` setting says, and saved though nothing else changed
    std::fs::write(
        format!("{root}/lib/papers.bib"),
        "@article{x,\n file = {:files/x.pdf:pdf},\n title = {T}\n}\n",
    )
    .unwrap();
    let mut lib = Library::open(&config_of(&root, "absolute")).unwrap();
    assert_eq!(lib.relative_to, "/");
    assert_eq!(
        lib.get("x").unwrap().get("file"),
        Some(format!(":{root}/lib/files/x.pdf:pdf").as_str())
    );
    assert!(lib.load_changes && !lib.unchanged());
    assert_eq!(lib.save(false).unwrap().status, SaveStatus::Written);
    let text = std::fs::read_to_string(format!("{root}/lib/papers.bib")).unwrap();
    assert!(text.contains(&format!("{root}/lib/files/x.pdf")));
    // with relative paths, an absolute link is made relative at load, in memory (the text on disk
    // is as it was until the next command that saves), as relative links are made absolute in an
    // absolute library; the links that are relative are left alone
    let mut lib = Library::open(&config_of(&root, "relative")).unwrap();
    assert_eq!(lib.get("x").unwrap().get("file"), Some(":files/x.pdf:pdf"));
    assert!(lib.load_changes && !lib.unchanged());
    let on_disk = std::fs::read_to_string(format!("{root}/lib/papers.bib")).unwrap();
    assert!(on_disk.contains(&format!("{root}/lib/files/x.pdf")));
    assert_eq!(lib.save(false).unwrap().status, SaveStatus::Written);
    let text = std::fs::read_to_string(format!("{root}/lib/papers.bib")).unwrap();
    assert!(text.contains(" file = {:files/x.pdf:pdf}") && !text.contains(&root));
    let lib = Library::open(&config_of(&root, "relative")).unwrap();
    assert_eq!(lib.get("x").unwrap().get("file"), Some(":files/x.pdf:pdf"));
    assert!(!lib.load_changes && lib.unchanged());
    // links as JabRef writes them (`files/a.pdf:PDF`) are relative: nothing to rewrite
    std::fs::write(
        format!("{root}/lib/papers.bib"),
        "@article{a,\n file = {files/a.pdf:PDF},\n title = {A}\n}\n",
    )
    .unwrap();
    let lib = Library::open(&config_of(&root, "relative")).unwrap();
    assert_eq!(lib.get("a").unwrap().get("file"), Some("files/a.pdf:PDF"));
    assert!(!lib.load_changes && lib.unchanged());
}

/// A library in `lib/` with its bibtex in `lib/bib/`.
fn library_with_bibtex_in_a_subfolder(root: &str) -> Config {
    std::fs::create_dir_all(format!("{root}/lib/.papers")).unwrap();
    std::fs::write(
        format!("{root}/lib/.papers/config.json"),
        r#"{"bibtex": "bib/papers.bib", "filesdir": "files", "version": 3}"#,
    )
    .unwrap();
    Config::load(
        &format!("{root}/lib/.papers/config.json"),
        MachineConfig::empty(),
        None,
    )
    .unwrap()
}

#[test]
fn the_distinct_pairs_are_in_the_folder_of_the_library_not_in_that_of_the_bibtex() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let config = library_with_bibtex_in_a_subfolder(&root);
    assert_eq!(config.libdir(), format!("{root}/lib"));
    let mut lib = Library::open(&config).unwrap();
    let mut a = Entry::new("article", "a2000");
    a.set("title", "One");
    let mut b = Entry::new("article", "b2000");
    b.set("title", "Another");
    lib.distinct.add(&a, &b);
    lib.save(true).unwrap();
    assert!(Path::new(&format!("{root}/lib/.papers/distinct.json")).exists());
    assert!(!Path::new(&format!("{root}/lib/bib/.papers")).exists());
    // read again from there
    let lib = Library::open(&config).unwrap();
    assert_eq!(lib.distinct.pairs.len(), 1);
}

#[test]
fn the_distinct_pairs_of_an_earlier_build_are_moved_when_the_configuration_is_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    library_with_bibtex_in_a_subfolder(&root);
    std::fs::create_dir_all(format!("{root}/lib/bib/.papers")).unwrap();
    std::fs::write(
        format!("{root}/lib/bib/.papers/distinct.json"),
        r#"{"distinct": [[{"title": "a"}, {"title": "b"}]]}"#,
    )
    .unwrap();
    let config = Config::load(
        &format!("{root}/lib/.papers/config.json"),
        MachineConfig::empty(),
        None,
    )
    .unwrap();
    assert!(!Path::new(&format!("{root}/lib/bib/.papers/distinct.json")).exists());
    assert_eq!(Library::open(&config).unwrap().distinct.pairs.len(), 1);
}
