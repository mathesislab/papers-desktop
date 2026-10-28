use std::fs;
use std::path::{Path, PathBuf};

use papers_core::{Error, Library, entry_keys, with_files};
use tempfile::TempDir;

/// Copies of fixtures in a temporary directory.
fn copies(names: &[&str]) -> (TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let paths = names
        .iter()
        .map(|name| {
            let path = dir.path().join(name);
            fs::copy(fixtures.join(name), &path).unwrap();
            path
        })
        .collect();
    (dir, paths)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

fn tags(changes: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
    changes
        .iter()
        .map(|(id, tags)| (id.to_string(), tags.iter().map(|t| t.to_string()).collect()))
        .collect()
}

const STYLES_TAGGED: &str = r#"% A library written by hand, in several styles.

@article{braces,
  author = {Lind, Anna},
  keywords = {c},
  title = {Braces},
  year = {2020},
}

@Article{Quoted,
  Author = "Berg, Ola",
  Keywords = "x, y",
  Title = "Quoted"
}

@misc{nofield,
 title = {No keywords},
 year = {2019},
 keywords = {t}
}

@book{tabs,
	title	=	{Tabs},
	year	=	{2018},
	keywords	=	{t}
}

@article{aligned,
  author   = {Holm, Erik},
  title    = {Aligned},
  year     = {2017},
  keywords = {t},
}

@misc{oneline, title = {One line}, year = {2016}, keywords = {t}}

@Article{Capital,
  Author = {Vik, Lars},
  Title = {Capital},
  Keywords = {t}
}

@misc{last,
 title = {Last}
}
"#;

#[test]
fn keywords_in_the_style_of_the_entry() {
    let (_dir, bibs) = copies(&["write.bib"]);
    let original = read(&bibs[0]);
    let lib = Library::load(&bibs).unwrap();
    let changes = lib
        .set_tags(&tags(&[
            ("braces", &["c"]),
            ("Quoted", &["x", " y "]),
            ("nofield", &["t"]),
            ("tabs", &["t"]),
            ("aligned", &["t"]),
            ("oneline", &["t"]),
            ("Capital", &["t"]),
            ("last", &[]),
        ]))
        .unwrap();
    // one write for the file
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].bib(), bibs[0]);
    assert_eq!(read(&bibs[0]), STYLES_TAGGED);

    // removed, and inserted back where it was
    let lib = Library::load(&bibs).unwrap();
    assert_eq!(lib.get("Quoted").unwrap().keywords, ["x", "y"]);
    lib.set_tags(&tags(&[
        ("braces", &[]),
        ("Quoted", &[]),
        ("nofield", &[]),
        ("tabs", &[]),
        ("aligned", &[]),
        ("oneline", &[]),
        ("Capital", &[]),
        ("last", &["x"]),
    ]))
    .unwrap();
    let expected = original
        .replace("  keywords = {a, b},\n", "")
        .replace("  Keywords = \"a\",\n", "");
    assert_eq!(read(&bibs[0]), expected);
}

#[test]
fn tags_unchanged_write_nothing() {
    let (_dir, bibs) = copies(&["write.bib"]);
    let lib = Library::load(&bibs).unwrap();
    let changes = lib
        .set_tags(&tags(&[("braces", &["a", "b"]), ("nofield", &[])]))
        .unwrap();
    assert!(changes.is_empty());
}

#[test]
fn tags_with_unmatched_braces_are_refused() {
    let (_dir, bibs) = copies(&["write.bib"]);
    let before = read(&bibs[0]);
    let lib = Library::load(&bibs).unwrap();
    for tag in ["a}", "{a", "a\\"] {
        let error = lib.set_tags(&tags(&[("braces", &[tag])])).unwrap_err();
        assert!(matches!(error, Error::Tag { .. }), "{error}");
    }
    assert!(lib.set_tags(&tags(&[("braces", &["{CO}_2"])])).is_ok());
    assert_ne!(read(&bibs[0]), before);
}

#[test]
fn replace_entry_keeps_the_rest() {
    let (_dir, bibs) = copies(&["library.bib"]);
    let before = read(&bibs[0]);
    let lib = Library::load(&bibs).unwrap();
    let raw = lib.raw("robinson2012").unwrap().to_string();
    let new = raw.replace("Nature Climate Change", "Nat. Clim. Change");
    let changes = lib
        .replace_entry("robinson2012", &format!("\n  {new}\n\n"))
        .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(read(&bibs[0]), before.replacen(&raw, &new, 1));

    let lib = Library::load(&bibs).unwrap();
    let it = lib.get("robinson2012").unwrap();
    assert_eq!(it.container.as_deref(), Some("Nat. Clim. Change"));
    // the key, the tags and the files stay
    let after = read(&bibs[0]);
    let kept = |text: String| match lib.replace_entry("robinson2012", &text) {
        Err(Error::Kept { what }) => what,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        kept(new.replace("{robinson2012,", "{robinson2012a,")),
        "key"
    );
    assert_eq!(
        kept(new.replace(" doi =", " keywords = {ice},\n doi =")),
        "tags"
    );
    assert_eq!(
        kept(new.replace(";Supplement:papers/robinson_et_al_2012--supp.pdf:pdf", "")),
        "file"
    );
    assert!(
        lib.check_edit("robinson2012", &new.replace("{2012}", "{2013}"))
            .is_ok()
    );
    assert_eq!(read(&bibs[0]), after);
}

#[test]
fn invalid_replacement_is_refused() {
    let (_dir, bibs) = copies(&["library.bib"]);
    let before = read(&bibs[0]);
    let lib = Library::load(&bibs).unwrap();
    let replace = |text: &str| lib.replace_entry("calov2015", text).unwrap_err();

    let two = "@misc{a, title = {A}}\n\n@misc{b, title = {B}}";
    assert!(matches!(replace(two), Error::NotOneEntry { found: 2 }));
    assert!(matches!(replace("  "), Error::NotOneEntry { found: 0 }));
    let error = replace("@article{calov2015,\n title = {A}\n");
    assert!(matches!(error, Error::Bibtex { .. }), "{error}");
    // closed too early: the rest is not in the entry
    let error = replace("@article{calov2015, title = {A}}, year = {2015}}");
    assert!(matches!(error, Error::Bibtex { line: 1, .. }), "{error}");
    // the key of another entry
    let error = replace("@article{Robinson2012, title = {A}}");
    assert!(matches!(error, Error::Kept { what: "key" }), "{error}");
    // what the library would not load, at a line of the text given
    let error = replace("@article{calov2015,\n title = {A},\n journal = jgr\n}");
    assert!(matches!(error, Error::Bibtex { line: 3, .. }), "{error}");
    assert!(matches!(
        lib.replace_entry("nonexistent", "@misc{x, title = {X}}"),
        Err(Error::NotFound { .. })
    ));
    assert_eq!(read(&bibs[0]), before);
}

#[test]
fn changed_file_is_refused() {
    let (_dir, bibs) = copies(&["library.bib"]);
    let lib = Library::load(&bibs).unwrap();
    let changed = read(&bibs[0]) + "\n@misc{new, title = {New}}\n";
    fs::write(&bibs[0], &changed).unwrap();
    let error = lib.set_tags(&tags(&[("calov2015", &["ice"])])).unwrap_err();
    assert!(matches!(error, Error::Changed { .. }), "{error}");
    assert!(
        error
            .to_string()
            .ends_with("changed since it was read; reloaded, try again"),
        "{error}"
    );
    assert_eq!(read(&bibs[0]), changed);

    // the library read again writes, once: the next write needs it read again
    let lib = Library::load(&bibs).unwrap();
    lib.set_tags(&tags(&[("calov2015", &["ice"])])).unwrap();
    let error = lib.remove_entries(&["new".to_string()]).unwrap_err();
    assert!(matches!(error, Error::Changed { .. }), "{error}");
    let lib = Library::load(&bibs).unwrap();
    lib.remove_entries(&["new".to_string()]).unwrap();
}

#[test]
fn add_entries_at_the_end() {
    let (_dir, bibs) = copies(&["library.bib", "other.bib"]);
    let other = &bibs[1];
    let before = read(other);
    let lib = Library::load(&bibs).unwrap();
    // a key of another file of the library is no duplicate
    let text = "@article{feldmann2017,\n  title = {Again}\n}\n\n% pasted\n@misc{new2024,\n  title = {New}\n}";
    assert_eq!(entry_keys(text).unwrap(), ["feldmann2017", "new2024"]);
    let (changes, added) = lib.add_entries(other, &format!("{text}\n\n")).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(added.added, ["feldmann2017", "new2024"]);
    // the entries only, a blank line between
    let entries = text.replace("% pasted\n", "");
    assert_eq!(read(other), format!("{before}\n{entries}\n"));
    let lib = Library::load(&bibs).unwrap();
    assert_eq!(lib.find(other, "new2024").unwrap().title, "New");

    // a file without a line break at its end
    fs::write(other, before.trim_end()).unwrap();
    let lib = Library::load(&bibs).unwrap();
    lib.add_entries(other, "@misc{x, title = {X}}").unwrap();
    assert_eq!(
        read(other),
        format!("{}\n\n@misc{{x, title = {{X}}}}\n", before.trim_end())
    );
}

#[test]
fn add_entries_existing() {
    let (_dir, bibs) = copies(&["library.bib", "other.bib"]);
    let other = &bibs[1];
    let before = read(other);
    let lib = Library::load(&bibs).unwrap();
    // the keys of the file, in any case, are existing; the others are added
    let (changes, added) = lib
        .add_entries(other, "@misc{Beckmann2019, title = {Again}}")
        .unwrap();
    assert!(changes.is_empty());
    assert_eq!(added.existing, ["Beckmann2019"]);
    assert_eq!(read(other), before);
    let text = "@misc{Beckmann2019, title = {Again}}\n@misc{new, title = {New}}";
    let (_, added) = lib.add_entries(other, text).unwrap();
    assert_eq!(
        (added.added, added.existing),
        (vec!["new".to_string()], vec!["Beckmann2019".to_string()])
    );
    assert_eq!(
        read(other),
        format!("{before}\n@misc{{new, title = {{New}}}}\n")
    );
}

#[test]
fn add_entries_refused() {
    let (_dir, bibs) = copies(&["library.bib", "other.bib"]);
    let other = &bibs[1];
    let before = read(other);
    let lib = Library::load(&bibs).unwrap();
    let add = |text: &str| lib.add_entries(other, text).unwrap_err();

    let error = add("@misc{a, title = {A}}\n@misc{a, title = {B}}");
    assert!(matches!(error, Error::DuplicateKey { .. }), "{error}");
    assert!(matches!(add("% nothing\n"), Error::NoEntry));
    assert!(matches!(add("just text"), Error::Bibtex { line: 1, .. }));
    assert!(matches!(entry_keys(""), Err(Error::NoEntry)));
    let outside = lib.add_entries(Path::new("/elsewhere.bib"), "@misc{a, title = {A}}");
    assert!(matches!(outside, Err(Error::NotInLibrary { .. })));
    assert_eq!(read(other), before);
}

#[test]
fn remove_entries_and_their_blank_lines() {
    let (_dir, bibs) = copies(&["library.bib"]);
    let before = read(&bibs[0]);
    let lib = Library::load(&bibs).unwrap();
    let raw = |id: &str| lib.raw(id).unwrap().to_string();
    // the first, one in the middle, the last
    let expected = before
        .replace(&format!("{}\n\n", raw("feldmann2017")), "")
        .replace(&format!("{}\n\n", raw("calov2015")), "")
        .replace(&format!("\n{}\n", raw("10.1002/xyz.123")), "");
    let ids = ["feldmann2017", "calov2015", "10.1002/xyz.123"].map(String::from);
    lib.remove_entries(&ids).unwrap();
    assert_eq!(read(&bibs[0]), expected);
    assert!(
        expected.starts_with(
            "@comment{jabref-meta: databaseType:bibtex}\n\n@article{vandenbroeke2009,"
        )
    );
    assert!(expected.ends_with("year = {None}\n}\n"));

    // the two last
    let lib = Library::load(&bibs).unwrap();
    lib.remove_entries(&["ipcc2021", "smith_handbook"].map(String::from))
        .unwrap();
    assert!(
        read(&bibs[0]).ends_with("year = {2020}\n}\n"),
        "{}",
        read(&bibs[0])
    );
    assert_eq!(Library::load(&bibs).unwrap().len(), 3);
}

#[test]
fn undo_and_redo() {
    let (_dir, bibs) = copies(&["write.bib"]);
    let before = read(&bibs[0]);
    let lib = Library::load(&bibs).unwrap();
    let changes = lib.set_tags(&tags(&[("nofield", &["t"])])).unwrap();
    let after = read(&bibs[0]);
    let change = &changes[0];
    change.undo().unwrap();
    assert_eq!(read(&bibs[0]), before);
    change.redo().unwrap();
    assert_eq!(read(&bibs[0]), after);

    // not over a change made since
    fs::write(&bibs[0], after.replace("One line", "One")).unwrap();
    assert!(matches!(change.undo(), Err(Error::Changed { .. })));
    assert!(read(&bibs[0]).contains("{One}"));
}

#[cfg(unix)]
#[test]
fn permissions_and_links_stay() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let (dir, bibs) = copies(&["write.bib"]);
    fs::set_permissions(&bibs[0], fs::Permissions::from_mode(0o640)).unwrap();
    let link = dir.path().join("link.bib");
    symlink(&bibs[0], &link).unwrap();
    let lib = Library::load(std::slice::from_ref(&link)).unwrap();
    lib.set_tags(&tags(&[("nofield", &["t"])])).unwrap();
    assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
    assert!(read(&bibs[0]).contains("keywords = {t}"));
    let mode = fs::metadata(&bibs[0]).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
    // no temporary file left
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);

    // a read-only file is not written
    fs::set_permissions(&bibs[0], fs::Permissions::from_mode(0o444)).unwrap();
    let lib = Library::load(&bibs).unwrap();
    let error = lib.set_tags(&tags(&[("nofield", &["u"])])).unwrap_err();
    assert!(matches!(error, Error::Write { .. }), "{error}");
}

#[test]
fn files_linked_to_an_entry_as_they_are() {
    let (_dir, bibs) = copies(&["write.bib"]);
    let lib = Library::load(&bibs).unwrap();
    let pdf = PathBuf::from("/home/me/Downloads/a b.pdf");
    // an entry without files gets a `file` field in the style of its fields
    let changes = lib
        .attach_files("nofield", None, std::slice::from_ref(&pdf))
        .unwrap();
    assert_eq!(changes.len(), 1);
    let after = read(&bibs[0]);
    assert!(
        after.contains(
            " title = {No keywords},\n year = {2019},\n file = {:/home/me/Downloads/a b.pdf:PDF}"
        ),
        "{after}"
    );
    let lib = Library::load(&bibs).unwrap();
    assert_eq!(
        lib.get("nofield").unwrap().files,
        std::slice::from_ref(&pdf)
    );
    // more links come after those there
    let other = PathBuf::from("/x/c;d.pdf");
    lib.attach_files("nofield", None, std::slice::from_ref(&other))
        .unwrap();
    let lib = Library::load(&bibs).unwrap();
    assert_eq!(lib.get("nofield").unwrap().files, [pdf, other]);
}

#[test]
fn files_linked_with_the_entry_replaced() {
    let (_dir, bibs) = copies(&["library.bib"]);
    let lib = Library::load(&bibs).unwrap();
    let raw = lib.raw("robinson2012").unwrap().to_string();
    let had = lib.get("robinson2012").unwrap().files.len();
    let pdf = PathBuf::from("/d/new.pdf");
    let new = raw.replace("{2012}", "{2013}");
    lib.attach_files("robinson2012", Some(&new), std::slice::from_ref(&pdf))
        .unwrap();
    let lib = Library::load(&bibs).unwrap();
    let item = lib.get("robinson2012").unwrap();
    assert!(lib.raw("robinson2012").unwrap().contains("{2013}"));
    assert_eq!(item.files.last(), Some(&pdf));
    assert_eq!(item.files.len(), had + 1);
    // an edit that changes what is kept is refused, with nothing written
    let text = read(&bibs[0]);
    let refused = lib.attach_files(
        "robinson2012",
        Some(&new.replace("{robinson2012,", "{x,")),
        &[pdf],
    );
    assert!(matches!(refused, Err(Error::Kept { what: "key" })));
    assert_eq!(read(&bibs[0]), text);
}

#[test]
fn the_text_of_an_entry_with_files() {
    let pdf = [PathBuf::from("/d/a.pdf")];
    assert_eq!(
        with_files("@article{a,\n title = {A}\n}", &pdf).unwrap(),
        "@article{a,\n title = {A},\n file = {:/d/a.pdf:PDF}\n}"
    );
    assert_eq!(
        with_files(
            "@article{a,\n file = {:/d/b.pdf:PDF},\n title = {A}\n}",
            &pdf
        )
        .unwrap(),
        "@article{a,\n file = {:/d/b.pdf:PDF;:/d/a.pdf:PDF},\n title = {A}\n}"
    );
    assert!(matches!(
        with_files("@article{a, title = {A}}\n@article{b, title = {B}}", &pdf),
        Err(Error::NotOneEntry { found: 2 })
    ));
}
