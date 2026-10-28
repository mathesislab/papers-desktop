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
    // with relative paths nothing is rewritten at load: the absolute links stay until the files
    // of the entry change, or until the links are converted (`papers config paths relative`)
    let mut lib = Library::open(&config_of(&root, "relative")).unwrap();
    assert_eq!(
        lib.get("x").unwrap().get("file"),
        Some(format!(":{root}/lib/files/x.pdf:pdf").as_str())
    );
    assert!(!lib.load_changes && lib.unchanged());
    let here = lib.relative_to.clone();
    assert_eq!(lib.set_relative_to(&here).len(), 1);
    assert_eq!(lib.get("x").unwrap().get("file"), Some(":files/x.pdf:pdf"));
    assert_eq!(lib.save(false).unwrap().status, SaveStatus::Written);
    let lib = Library::open(&config_of(&root, "relative")).unwrap();
    assert!(!lib.load_changes && lib.unchanged());
}
