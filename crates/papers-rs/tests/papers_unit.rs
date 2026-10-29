//! The unit tests of papers-cli about names, files, duplicates and merges, ported
//! (`test_filename_unit.py`, `test_bib_unit.py`, `test_path_traversal.py`, `test_paths.py`,
//! `test_duplicates.py`, `test_merge_dup_unit.py`, `test_entries.py`): what they check by
//! calling papers' functions, the same by calling papers-rs'. The generated cases
//! (`names.rs`, `identity.rs`, `library.rs`) check more, against the reference itself.

use papers_rs::bib::{Bib, Entry};
use papers_rs::entry::{files, set_files, update_file_path};
use papers_rs::identity::{Distinct, Identifiers, Kind, duplicate_groups, parse_name, same_paper};
use papers_rs::library::{Action, InsertOptions, Library, Policy, What};
use papers_rs::merge::{Merge, OnDuplicate, Resolver, Status};
use papers_rs::names::{
    Format, FormatKind, append_abc, check_key, is_valid_key, list_tag, make_template_fields,
    path_safe,
};

fn entry(key: &str, ty: &str, fields: &[(&str, &str)]) -> Entry {
    let mut e = Entry::new(ty, key);
    for (k, v) in fields {
        e.set(k, *v);
    }
    e
}

fn kind(a: &Entry, b: &Entry) -> Kind {
    same_paper(a, b, None).kind
}

const SMITH: [(&str, &str); 3] = [
    ("author", "Smith, John and Doe, Jane"),
    ("title", "Climate sensitivity revisited"),
    ("year", "2020"),
];

fn with(more: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    SMITH.iter().chain(more).copied().collect()
}

// test_filename_unit.py
// ---------------------

#[test]
fn unknown_entries() {
    let mut strict = Format::name();
    strict.template = "{author}".into();
    strict.unknown_strict = true;
    let unknown = entry(
        "k",
        "article",
        &[("author", "unknown"), ("year", "0000"), ("title", "")],
    );
    assert!(strict.is_unknown(&unknown));
    assert!(!strict.is_unknown(&entry(
        "k",
        "article",
        &[("author", "Smith"), ("year", "0000"), ("title", "")]
    )));
    let mut any = strict.clone();
    any.unknown_strict = false;
    assert!(any.is_unknown(&entry(
        "k",
        "article",
        &[("author", "Smith"), ("year", "2020"), ("title", "")]
    )));
    assert!(!any.is_unknown(&entry(
        "k",
        "article",
        &[("author", "Smith"), ("year", "2020"), ("title", "A Paper")]
    )));
}

#[test]
fn the_unknown_key_comes_from_the_slugified_doi() {
    // issue #105: the raw doi (slashes, dots...) used to become the key
    let key = Format::key()
        .render(&entry("k", "article", &[("doi", "10.5194/bg-8(515)-2011")]))
        .unwrap();
    assert!(!key.is_empty());
    assert!(!key.contains(['/', '(', ')', '.']), "{key}");
}

#[test]
fn title_length_is_a_hard_limit() {
    let words = ["climate", "sensitivity", "revisited"];
    assert_eq!(list_tag(&words, 20, 1, 100, "-"), "climate-sensitivity");
    assert_eq!(list_tag(&words, 10, 1, 100, "-"), "climate"); // one word left
    assert_eq!(
        list_tag(&["sensitivity", "revisited"], 5, 1, 100, "-"),
        "sensi"
    ); // a long first word is cut
    assert_eq!(list_tag(&["a", "b", "c"], 100, 1, 2, "-"), "a-b");
    assert_eq!(list_tag(&["a", "bb", "ccc"], 100, 2, 100, "-"), "bb-ccc");
    // the length is that of the title in the name, with its separator
    let e = entry(
        "k",
        "article",
        &[("title", "Climate sensitivity revisited")],
    );
    let mut f = Format::name();
    f.title_sep = "--".into();
    f.title_length = 20;
    assert_eq!(
        make_template_fields(&e, &f).get("title").unwrap(),
        "climate--sensitivity"
    );
    f.title_length = 19;
    assert_eq!(
        make_template_fields(&e, &f).get("title").unwrap(),
        "climate"
    );
    let mut f = Format::name();
    f.unknown_template = None;
    f.template = "{title}".into();
    f.title_length = 12;
    assert_eq!(f.render(&e.clone()).unwrap(), "climate");
    f.template = "{Title}".into();
    f.title_length = 4;
    assert_eq!(
        f.render(&entry("k", "article", &[("title", "Climate")]))
            .unwrap(),
        "Clim"
    );
}

#[test]
fn placeholders_and_their_errors() {
    let e = entry(
        "smith2020",
        "article",
        &[
            ("author", "Smith, John"),
            ("year", "2020"),
            ("title", "Climate"),
        ],
    );
    let render = |template: &str, kind: FormatKind| {
        let mut f = if kind == FormatKind::Key {
            Format::key()
        } else {
            Format::name()
        };
        f.template = template.into();
        f.render(&e)
    };
    assert_eq!(render("{key}", FormatKind::Name).unwrap(), "smith2020");
    assert_eq!(render("{ID}", FormatKind::Name).unwrap(), "smith2020");
    let message = render("{year}_{xxx}", FormatKind::Name)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("unknown placeholder {xxx} in the name template '{year}_{xxx}'"),
        "{message}"
    );
    assert!(message.contains("available: {author}") && message.contains("{key}"));
    assert!(
        render("{foo}", FormatKind::Key)
            .unwrap_err()
            .to_string()
            .contains("unknown placeholder {foo} in the key template")
    );
    assert!(
        render("{}", FormatKind::Name)
            .unwrap_err()
            .to_string()
            .contains("unknown placeholder {} in the name template")
    );
    assert!(
        render("{year", FormatKind::Name)
            .unwrap_err()
            .to_string()
            .contains("invalid name template")
    );
}

// test_bib_unit.py
// ----------------

#[test]
fn key_suffixes_and_validity() {
    assert_eq!(append_abc("Author2000", &[]).unwrap(), "Author2000b");
    assert_eq!(append_abc("Author2000b", &[]).unwrap(), "Author2000c");
    assert_eq!(
        append_abc("Author2000", &["Author2000", "Author2000b"]).unwrap(),
        "Author2000c"
    );
    assert_eq!(
        append_abc("Author2000", &["author2000", "author2000b"]).unwrap(),
        "Author2000c"
    );
    // the last letter is a suffix only after a digit: 'perrette' used to become 'perrettf'
    assert_eq!(append_abc("perrette", &["perrette"]).unwrap(), "perretteb");
    assert_eq!(
        append_abc("10-1000-abc", &["10-1000-abc"]).unwrap(),
        "10-1000-abcb"
    );
    // test_bib_unit.py: a suffix in either case, and a second letter after z
    assert_eq!(append_abc("Author2000B", &[]).unwrap(), "Author2000C");
    assert_eq!(
        append_abc("Author2000z", &["Author2000z"]).unwrap(),
        "Author2000za"
    );
    assert_eq!(append_abc("Author2000Z", &[]).unwrap(), "Author2000Za");
    assert_eq!(
        append_abc("Author2000", &["author2000", "AUTHOR2000B"]).unwrap(),
        "Author2000c"
    );
    let all: Vec<String> = ('b'..='z').map(|c| format!("ab{c}")).collect();
    let mut taken: Vec<&str> = all.iter().map(String::as_str).collect();
    taken.push("ab");
    assert_eq!(append_abc("Ab", &taken).unwrap(), "Aba");
    assert!(is_valid_key("Author2020") && is_valid_key("Smith"));
    assert!(
        !is_valid_key("2020Author") && !is_valid_key("10.5194/bg-8-515-2011") && !is_valid_key("")
    );
    assert!(check_key("a b").is_some());
}

#[test]
fn ids_of_an_entry() {
    let id = |fields: &[(&str, &str)]| Identifiers::of(&entry("k", "article", fields));
    assert_eq!(id(&[("author", "Smith, John")]).author, "smith");
    assert_eq!(id(&[("title", "A Test Paper")]).title, "a test paper");
    assert_eq!(
        id(&[
            ("author", "Smith, J."),
            ("title", "Paper"),
            ("doi", "10.1234/test")
        ])
        .author_title,
        "smith|paper"
    );
    assert_eq!(id(&[("title", "Editorial")]).author_title, "");
    assert_eq!(id(&[("author", "Smith, J.")]).author_title, "");
    assert_eq!(
        id(&[("author", "M{\\\"u}ller, Hans and Müller, Jens")]).author,
        "muller muller"
    );
    assert_eq!(
        id(&[("title", "The {CO2}   Problem")]).title,
        "the co2 problem"
    );
}

// test_path_traversal.py
// ----------------------

const TRAVERSALS: [&str; 5] = [
    "../../escaped/x",
    "/../../../escaped/x",
    "..",
    "/etc/x",
    "..\\..\\escaped\\x",
];

#[test]
fn path_safe_fields() {
    assert_eq!(path_safe("a/b\\c\0d"), "a-b-c-d");
    assert_eq!(path_safe(".."), "--");
    assert_eq!(path_safe("."), "-");
    assert_eq!(path_safe("2013"), "2013");
    for value in TRAVERSALS {
        let e = entry(
            value,
            "article",
            &[
                ("author", "Doe, John"),
                ("title", "A title"),
                ("year", value),
                ("journal", value),
                ("doi", value),
            ],
        );
        let fields = make_template_fields(&e, &Format::name());
        for name in ["year", "journal", "ID", "doi"] {
            let f = fields.get(name).unwrap();
            assert!(!f.contains('/') && !f.contains('\\'), "{name} {f}");
            assert!(!f.trim_matches('.').is_empty(), "{name} {f}");
        }
    }
}

struct Disk {
    _dir: tempfile::TempDir,
    root: String,
}

impl Disk {
    fn new() -> Disk {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir_all(format!("{root}/lib")).unwrap();
        Disk { _dir: dir, root }
    }

    fn write(&self, path: &str, content: &str) -> String {
        let p = format!("{}/{path}", self.root);
        std::fs::create_dir_all(papers_rs::config::paths::dirname(&p)).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    fn library(&self, text: &str, nameformat: Format) -> Library {
        let bibtex = self.write("lib/papers.bib", text);
        let mut lib = Library::load(
            &bibtex,
            Some(&format!("{}/lib/files", self.root)),
            None,
            nameformat,
            Format::key(),
        )
        .unwrap();
        lib.mark_loaded();
        lib
    }

    /// The files outside the files directory, other than the sources.
    fn outside(&self) -> Vec<String> {
        let mut found = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(&self.root)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let s = p.to_string_lossy().into_owned();
                if !s.starts_with(&format!("{}/lib/files/", self.root))
                    && !s.ends_with("source.pdf")
                    && !s.ends_with("papers.bib")
                {
                    found.push(s);
                }
            }
        }
        found
    }
}

#[test]
fn fields_cannot_place_files_outside_the_files_directory() {
    for value in TRAVERSALS {
        for field in ["year", "journal", "doi"] {
            let disk = Disk::new();
            let source = disk.write("source.pdf", "payload");
            let mut lib = disk.library("", Format::name());
            let mut f = Format::name();
            f.template = format!("{{authorX}}_{{{field}}}_{{title}}");
            f.unknown_template = None;
            lib.nameformat = f;
            let text = format!(
                "@article{{x,\n author = {{Doe, John}},\n title = {{A title}},\n {field} = {{{value}}},\n file = {{:source.pdf:pdf}}\n}}\n"
            );
            let mut resolver = Resolver::new(OnDuplicate::Error);
            lib.add_bibtex(
                &text,
                Some(&disk.root),
                &[],
                false,
                &InsertOptions::default(),
                &mut resolver,
            )
            .unwrap();
            lib.rename_entry_files("x", &Policy::all(Action::Move), None, None, true)
                .unwrap();
            assert!(
                disk.outside().is_empty(),
                "{field} {value}: {:?}",
                disk.outside()
            );
            let placed = lib.files("x");
            assert!(
                placed[0].starts_with(&format!("{}/lib/files/", disk.root)),
                "{field} {value}: {placed:?}"
            );
            let _ = source;
        }
    }
}

#[test]
fn a_template_that_points_outside_is_refused() {
    for template in ["../{author}", "/tmp/{author}", "{author}/../../x", "."] {
        let disk = Disk::new();
        let source = disk.write("source.pdf", "payload");
        let mut f = Format::name();
        f.template = template.into();
        let mut lib = disk.library("", f);
        let text = "@article{x,\n author = {Doe, John},\n year = {2020},\n title = {A title},\n file = {:source.pdf:pdf}\n}\n";
        let mut resolver = Resolver::new(OnDuplicate::Error);
        lib.add_bibtex(
            text,
            Some(&disk.root),
            &[],
            false,
            &InsertOptions::default(),
            &mut resolver,
        )
        .unwrap();
        lib.rename_entry_files("x", &Policy::all(Action::Move), None, None, true)
            .unwrap();
        assert_eq!(lib.files("x"), [source], "{template}");
        assert!(disk.outside().is_empty());
    }
    // subfolders of the template itself still work
    let disk = Disk::new();
    disk.write("source.pdf", "payload");
    let mut f = Format::name();
    f.template = "{year}/{author}".into();
    let mut lib = disk.library("", f);
    let text = "@article{x,\n author = {Doe, John},\n year = {2020},\n title = {A title},\n file = {:source.pdf:pdf}\n}\n";
    let mut resolver = Resolver::new(OnDuplicate::Error);
    lib.add_bibtex(
        text,
        Some(&disk.root),
        &[],
        false,
        &InsertOptions::default(),
        &mut resolver,
    )
    .unwrap();
    lib.rename_entry_files("x", &Policy::all(Action::Move), None, None, true)
        .unwrap();
    assert_eq!(
        lib.files("x"),
        [format!("{}/lib/files/2020/doe.pdf", disk.root)]
    );
}

// test_paths.py
// -------------

#[test]
fn a_file_moved_onto_itself_or_a_link_to_itself_stays() {
    // a link to the directory of the file
    let disk = Disk::new();
    disk.write("lib/files/doe_2020_t.pdf", "a");
    std::os::unix::fs::symlink(
        format!("{}/lib/files", disk.root),
        format!("{}/lib/link", disk.root),
    )
    .unwrap();
    let text = "@article{x,\n author = {Doe, Jane},\n year = {2020},\n title = {T},\n file = {:link/doe_2020_t.pdf:pdf}\n}\n";
    let mut lib = disk.library(text, Format::name());
    lib.rename_entry_files("x", &Policy::all(Action::Move), None, None, true)
        .unwrap();
    assert!(std::path::Path::new(&format!("{}/lib/files/doe_2020_t.pdf", disk.root)).exists());
    assert!(lib.file_ops().is_empty());
    // a hard link to the destination: the source goes away, the destination stays
    let disk = Disk::new();
    let a = disk.write("in/a.pdf", "a");
    std::fs::create_dir_all(format!("{}/lib/files", disk.root)).unwrap();
    std::fs::hard_link(&a, format!("{}/lib/files/doe_2020_t.pdf", disk.root)).unwrap();
    let text = "@article{x,\n author = {Doe, Jane},\n year = {2020},\n title = {T},\n file = {:../in/a.pdf:pdf}\n}\n";
    let mut lib = disk.library(text, Format::name());
    lib.rename_entry_files("x", &Policy::all(Action::Move), None, None, true)
        .unwrap();
    assert!(!std::path::Path::new(&a).exists());
    assert_eq!(
        std::fs::read_to_string(format!("{}/lib/files/doe_2020_t.pdf", disk.root)).unwrap(),
        "a"
    );
}

#[test]
fn a_file_with_other_content_at_the_destination_is_never_replaced() {
    // the file takes a -2 suffix, whatever the answer to `Replace? (y/n)` would be (test_file_cmd.py)
    for confirm in [false, true] {
        let disk = Disk::new();
        let a = disk.write("in/a.pdf", "new");
        let dest = disk.write("lib/files/doe_2020_t.pdf", "old");
        let text = "@article{x,\n author = {Doe, Jane},\n year = {2020},\n title = {T},\n file = {:../in/a.pdf:pdf}\n}\n";
        let mut lib = disk.library(text, Format::name());
        lib.confirm_replace = Box::new(move |_| confirm);
        lib.rename_entry_files("x", &Policy::all(Action::Move), None, None, true)
            .unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");
        let second = format!("{}/lib/files/doe_2020_t-2.pdf", disk.root);
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "new");
        assert_eq!(lib.files("x"), [second]);
        assert!(!std::path::Path::new(&a).exists());
    }
}

#[test]
fn two_papers_with_the_same_file_name() {
    // TestSameFileName: the second file gets a -2 suffix, no file is replaced; renaming again
    // changes nothing; the same content is the same file
    for same_content in [false, true] {
        let disk = Disk::new();
        disk.write("in/a.pdf", "pdf a");
        disk.write("in/b.pdf", if same_content { "pdf a" } else { "pdf b" });
        let text = "@article{one2000,\n author = {Doe, Jane},\n year = {2000},\n title = {T},\n file = {:../in/a.pdf:pdf}\n}\n\n@article{other2000,\n author = {Doe, Jane},\n year = {2000},\n title = {T},\n file = {:../in/b.pdf:pdf}\n}\n";
        let mut lib = disk.library(text, Format::name());
        lib.rename_all_files(&Policy::all(Action::Move), None, true);
        let (fa, fb) = (lib.files("one2000"), lib.files("other2000"));
        assert_eq!(fa, [format!("{}/lib/files/doe_2000_t.pdf", disk.root)]);
        if same_content {
            assert_eq!(fa, fb);
            continue;
        }
        assert_eq!(fb, [format!("{}/lib/files/doe_2000_t-2.pdf", disk.root)]);
        assert_eq!(std::fs::read_to_string(&fa[0]).unwrap(), "pdf a");
        assert_eq!(std::fs::read_to_string(&fb[0]).unwrap(), "pdf b");
        lib.rename_all_files(&Policy::all(Action::Move), None, true);
        assert_eq!((lib.files("one2000"), lib.files("other2000")), (fa, fb));
    }
}

// test_entries.py
// ---------------

#[test]
fn updating_file_paths() {
    let mut e = entry("test", "article", &[("file", ":/base/dir1/file.pdf:pdf")]);
    let (old, new) = update_file_path(&mut e, "/base/dir1", "/base/dir2").unwrap();
    assert_eq!(
        (old.as_str(), new.as_str()),
        (":/base/dir1/file.pdf:pdf", ":../dir1/file.pdf:pdf")
    );
    assert!(update_file_path(&mut e, "/base/dir2", "/base/dir2").is_none());
    assert_eq!(files(&e, Some("/base/dir2")), ["/base/dir1/file.pdf"]);
    let mut e = entry("test", "article", &[]);
    set_files(
        &mut e,
        &["/a/x.pdf".into(), "/a/x.pdf".into(), "/b/y.txt".into()],
        "/a",
    );
    assert_eq!(e.get("file"), Some(":x.pdf:pdf;:../b/y.txt:txt"));
    set_files(&mut e, &[], "/a");
    assert_eq!(e.get("file"), None);
    let mut e = entry("test", "article", &[("file", ":../lib/x.pdf:pdf")]);
    let abs = files(&e, Some("/a/b"));
    set_files(&mut e, &abs, "/");
    assert_eq!(e.get("file"), Some(":/a/lib/x.pdf:pdf"));
}

// test_duplicates.py
// ------------------

#[test]
fn identifiers_of_duplicates() {
    let a = entry(
        "a",
        "article",
        &[("doi", "10.1000/ABC123"), ("title", "One")],
    );
    let b = entry(
        "b",
        "article",
        &[
            ("doi", "https://doi.org/10.1000/abc123"),
            ("title", "Other title"),
        ],
    );
    assert_eq!(kind(&a, &b), Kind::Same);
    assert!(
        same_paper(&a, &b, None)
            .reason
            .contains("same DOI 10.1000/abc123")
    );
    // the 2019 preprint and the 2020 article
    let a = entry("a", "article", &with(&[("doi", "10.1000/abc123")]));
    let b = entry("b", "misc", &with(&[("doi", "10.5194/egusphere-2019-1")]));
    assert_eq!(kind(&a, &b), Kind::Different);
    let a = entry(
        "a",
        "article",
        &[
            ("eprint", "2101.01234"),
            ("title", "One"),
            ("archivePrefix", "HAL"),
            ("author", "A, B"),
        ],
    );
    let b = entry(
        "b",
        "article",
        &[
            ("eprint", "2101.01234"),
            ("title", "Two"),
            ("author", "C, D"),
        ],
    );
    assert_eq!(kind(&a, &b), Kind::Different);
    let a = entry("a", "article", &with(&[("eprint", "2101.01234")]));
    let b = entry("b", "article", &with(&[("eprint", "2101.09999")]));
    assert_eq!(kind(&a, &b), Kind::Different);
    let a = entry("a", "book", &with(&[("isbn", "0-306-40615-2")]));
    let b = entry("b", "book", &with(&[("isbn", "978-3-16-148410-0")])); // another edition
    assert_eq!(kind(&a, &b), Kind::Different);
    // a chapter carries the ISBN of its book
    let book = entry(
        "a",
        "book",
        &[
            ("isbn", "0-306-40615-2"),
            ("author", "Ed, A"),
            ("title", "Handbook"),
        ],
    );
    let chapter = entry(
        "b",
        "incollection",
        &[
            ("isbn", "0-306-40615-2"),
            ("author", "Smith, J"),
            ("title", "Chapter"),
        ],
    );
    assert_eq!(kind(&book, &chapter), Kind::Different);
    // the order of the identifiers
    let a = entry(
        "a",
        "article",
        &with(&[("doi", "10.1000/a"), ("eprint", "2101.01234")]),
    );
    let b = entry(
        "b",
        "article",
        &with(&[("doi", "10.1000/b"), ("eprint", "2101.01234")]),
    );
    assert_eq!(kind(&a, &b), Kind::Different);
    let a = entry(
        "a",
        "article",
        &[
            ("doi", "10.1000/a"),
            ("eprint", "2101.01234"),
            ("title", "One"),
            ("author", "A, B"),
        ],
    );
    let b = entry(
        "b",
        "article",
        &[
            ("eprint", "2101.01234"),
            ("title", "Two"),
            ("author", "C, D"),
        ],
    );
    assert_eq!(kind(&a, &b), Kind::Same); // b has no DOI: the arXiv id decides
    // one DOI only: the authors and title
    let a = entry("a", "article", &with(&[("doi", "10.1000/abc123")]));
    assert_eq!(kind(&a, &entry("b", "article", &SMITH)), Kind::Same);
}

#[test]
fn the_title_guard_asks() {
    let book = entry(
        "a",
        "book",
        &[("doi", "10.1000/book"), ("title", "Handbook of climate")],
    );
    let chapter = entry(
        "b",
        "incollection",
        &[("doi", "10.1000/book"), ("title", "Sea level rise")],
    );
    let v = same_paper(&book, &chapter, None);
    assert_eq!(v.kind, Kind::Ask);
    assert!(v.reason.contains("entry types"));
    let a = entry(
        "a",
        "article",
        &[
            ("doi", "10.1000/x"),
            ("title", "Climate sensitivity revisited"),
        ],
    );
    let b = entry(
        "b",
        "misc",
        &[
            ("doi", "10.1000/x"),
            ("title", "{Climate} Sensitivity  Revisited"),
        ],
    );
    assert_eq!(kind(&a, &b), Kind::Same);
    let b = entry(
        "b",
        "misc",
        &[
            ("doi", "10.1000/x"),
            ("title", "Data for: Climate sensitivity revisited"),
        ],
    );
    assert_eq!(kind(&a, &b), Kind::Ask);
}

#[test]
fn distinct_pairs_in_a_file() {
    let disk = Disk::new();
    let bibtex = format!("{}/lib/papers.bib", disk.root);
    let (a, b) = (entry("a", "article", &SMITH), entry("b", "article", &SMITH));
    let mut d = Distinct::for_bibtex(&bibtex);
    assert_eq!(same_paper(&a, &b, Some(&d)).kind, Kind::Same);
    d.add(&a, &b);
    d.save().unwrap();
    let path = format!("{}/lib/.papers/distinct.json", disk.root);
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        json["distinct"],
        serde_json::json!([[{"authortitle": "smith doe|climate sensitivity revisited"}, {"authortitle": "smith doe|climate sensitivity revisited"}]])
    );
    let d2 = Distinct::for_bibtex(&bibtex);
    assert_eq!(same_paper(&a, &b, Some(&d2)).kind, Kind::Different);
    assert_eq!(same_paper(&b, &a, Some(&d2)).kind, Kind::Different);
    // identities, not keys
    assert_eq!(
        same_paper(
            &entry("x", "article", &SMITH),
            &entry("y", "article", &SMITH),
            Some(&d2)
        )
        .kind,
        Kind::Different
    );
    // another chapter of the same book is still asked about
    let mut d = Distinct::new();
    let book = entry(
        "a",
        "book",
        &[("doi", "10.1000/book"), ("title", "Handbook of climate")],
    );
    let chapter = entry(
        "b",
        "incollection",
        &[("doi", "10.1000/book"), ("title", "Sea level rise")],
    );
    d.add(&book, &chapter);
    assert_eq!(same_paper(&book, &chapter, Some(&d)).kind, Kind::Different);
    let other = entry(
        "c",
        "incollection",
        &[("doi", "10.1000/book"), ("title", "Ocean tides")],
    );
    assert_eq!(same_paper(&book, &other, Some(&d)).kind, Kind::Ask);
    // a pair does not leak to other entries
    let mut d = Distinct::new();
    let with_doi = entry("a", "article", &with(&[("doi", "10.1000/abc123")]));
    let without = entry("b", "article", &SMITH);
    let preprint = entry(
        "c",
        "article",
        &with(&[("doi", "10.5194/egusphere-2019-1")]),
    );
    d.add(&with_doi, &without);
    assert_eq!(
        same_paper(&with_doi, &without, Some(&d)).kind,
        Kind::Different
    );
    assert_eq!(same_paper(&without, &preprint, Some(&d)).kind, Kind::Same);
    // the union of two sets of pairs
    let (mut d1, mut d2) = (Distinct::new(), Distinct::new());
    d2.add(
        &entry("a", "article", &[("doi", "10.1/a")]),
        &entry("b", "article", &[("doi", "10.1/b")]),
    );
    d1.update(&d2);
    d1.update(&d2);
    assert_eq!(d1.pairs.len(), 1);
    // a file that cannot be read is ignored; an entry without identity is not recorded
    let bad = disk.write("bad/distinct.json", "[1, 2]");
    assert!(Distinct::load(&bad).pairs.is_empty());
    let mut d = Distinct::new();
    d.add(&entry("a", "misc", &[]), &entry("b", "article", &SMITH));
    assert!(d.pairs.is_empty() && !d.changed);
}

#[test]
fn groups_of_duplicates() {
    let article = entry(
        "article",
        "article",
        &with(&[("doi", "10.1000/abc123"), ("journal", "Nature")]),
    );
    let copy = entry("copy", "article", &SMITH);
    let preprint = entry(
        "preprint",
        "misc",
        &with(&[("doi", "10.5194/egusphere-2019-1")]),
    );
    let groups = duplicate_groups(&[&copy, &preprint, &article], None);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].entries, [2, 0]); // most fields first
    let book = entry(
        "a",
        "book",
        &[("doi", "10.1000/book"), ("title", "Handbook of climate")],
    );
    let chapter = entry(
        "b",
        "incollection",
        &[("doi", "10.1000/book"), ("title", "Sea level rise")],
    );
    assert_eq!(
        duplicate_groups(&[&book, &chapter], None)[0].kind,
        Kind::Ask
    );
    assert_eq!(
        parse_name("John {van der Berg}"),
        ("{van der Berg}".to_string(), "John".to_string())
    );
}

// test_duplicates.py: merge, and --on-duplicate of add and check
// --------------------------------------------------------------

const LIB: &str = "@article{smith2020climate,\n author = {Smith, John and Doe, Jane},\n doi = {10.1000/abc123},\n file = {:files/smith.pdf:pdf},\n journal = {Nature},\n keywords = {climate, mine},\n title = {Climate sensitivity revisited},\n year = {2020}\n}\n";
const NEW: &str = "@article{Smith_2020,\n author = {Smith, J. and Doe, J.},\n doi = {10.1000/ABC123},\n file = {:other/smith-preprint.pdf:pdf},\n journal = {Nature Climate Change},\n title = {Climate Sensitivity Revisited},\n volume = {10},\n year = {2020}\n}\n";

fn add(
    lib: &mut Library,
    text: &str,
    mode: OnDuplicate,
    update_key: bool,
) -> papers_rs::error::Result<Vec<papers_rs::library::Inserted>> {
    let options = InsertOptions {
        update_key,
        check_duplicate: true,
        ..Default::default()
    };
    lib.add_bibtex(
        text,
        Some(&papers_rs::config::paths::dirname(&lib.bibtex)),
        &[],
        false,
        &options,
        &mut Resolver::new(mode),
    )
}

fn field(lib: &Library, key: &str, name: &str) -> Option<String> {
    lib.get(key).and_then(|e| e.get(name)).map(str::to_string)
}

#[test]
fn add_with_on_duplicate() {
    let disk = Disk::new();
    let err = add(
        &mut disk.library(LIB, Format::name()),
        NEW,
        OnDuplicate::Error,
        false,
    )
    .unwrap_err();
    assert!(
        err.message().contains("--on-duplicate")
            && err
                .message()
                .contains("already in library as smith2020climate")
    );

    let mut lib = disk.library(LIB, Format::name());
    let result = add(&mut lib, NEW, OnDuplicate::Keep, false).unwrap();
    assert_eq!(
        result[0].what,
        What::Merged {
            already_in_library: false
        }
    );
    assert_eq!(lib.len(), 1);
    assert_eq!(
        field(&lib, "smith2020climate", "journal").unwrap(),
        "Nature"
    );
    assert_eq!(field(&lib, "smith2020climate", "volume").unwrap(), "10");
    assert_eq!(
        field(&lib, "smith2020climate", "author").unwrap(),
        "Smith, John and Doe, Jane"
    );
    assert_eq!(
        field(&lib, "smith2020climate", "title").unwrap(),
        "Climate sensitivity revisited"
    );
    assert_eq!(
        field(&lib, "smith2020climate", "file").unwrap(),
        ":files/smith.pdf:pdf;:other/smith-preprint.pdf:pdf"
    );
    assert_eq!(field(&lib, "smith2020climate", "ids"), None);

    let mut lib = disk.library(LIB, Format::name());
    add(&mut lib, NEW, OnDuplicate::Update, true).unwrap(); // the library key stays
    assert_eq!(
        lib.entries().map(|e| e.key.as_str()).collect::<Vec<_>>(),
        ["smith2020climate"]
    );
    assert_eq!(
        field(&lib, "smith2020climate", "journal").unwrap(),
        "Nature Climate Change"
    );
    assert_eq!(
        field(&lib, "smith2020climate", "keywords").unwrap(),
        "climate, mine"
    );
    assert_eq!(
        field(&lib, "smith2020climate", "author").unwrap(),
        "Smith, John and Doe, Jane"
    ); // the fuller form

    let mut lib = disk.library(LIB, Format::name());
    let result = add(&mut lib, NEW, OnDuplicate::Skip, false).unwrap();
    assert_eq!(result[0].what, What::Skipped);
    assert_eq!(
        lib.save(false).unwrap().status,
        papers_rs::library::SaveStatus::Unchanged
    );

    // an exact duplicate never asks, and brings its files
    let exact = "@article{otherkey,\n author = {Smith, J. and Doe, J.},\n doi = {10.1000/abc123},\n file = {:other/smith-preprint.pdf:pdf},\n title = {Climate sensitivity revisited}\n}\n";
    let mut lib = disk.library(LIB, Format::name());
    let result = add(&mut lib, exact, OnDuplicate::Error, false).unwrap();
    assert_eq!(
        result[0].what,
        What::Merged {
            already_in_library: false
        }
    ); // new files: the report says Modified
    assert_eq!(
        field(&lib, "smith2020climate", "file").unwrap(),
        ":files/smith.pdf:pdf;:other/smith-preprint.pdf:pdf"
    );
    let again = add(&mut lib, exact, OnDuplicate::Error, false).unwrap();
    assert_eq!(
        again[0].what,
        What::Merged {
            already_in_library: true
        }
    );

    // the title guard is not answered by keep
    let chapter = "@incollection{chap,\n doi = {10.1000/abc123},\n title = {Sea level rise in the next century}\n}\n";
    let mut lib = disk.library(LIB, Format::name());
    assert!(add(&mut lib, chapter, OnDuplicate::Error, false).is_err());
    assert_eq!(
        add(&mut lib, chapter, OnDuplicate::Keep, false).unwrap()[0].what,
        What::Skipped
    );
    assert_eq!(lib.len(), 1);
}

#[test]
fn check_with_on_duplicate() {
    let group = format!(
        "{LIB}\n{NEW}\n@misc{{smith2019preprint,\n author = {{Smith, John and Doe, Jane}},\n doi = {{10.5194/egusphere-2019-1}},\n title = {{Climate sensitivity revisited}},\n year = {{2019}}\n}}\n\n@article{{brown2021,\n author = {{Brown, Bob}},\n title = {{Other}},\n year = {{2021}}\n}}\n"
    );
    let disk = Disk::new();
    let mut lib = disk.library(&group, Format::name());
    let err = lib
        .check_duplicates(&mut Resolver::new(OnDuplicate::Error))
        .unwrap_err();
    assert!(
        err.message()
            .contains("same paper: smith2020climate, Smith_2020")
            && err.message().contains("--on-duplicate")
    );
    assert!(
        lib.check_duplicates(&mut Resolver::new(OnDuplicate::Update))
            .is_err()
    );
    assert!(
        lib.check_duplicates(&mut Resolver::new(OnDuplicate::Skip))
            .unwrap()
            .is_empty()
    );
    assert_eq!(lib.len(), 4);
    let merged = lib
        .check_duplicates(&mut Resolver::new(OnDuplicate::Keep))
        .unwrap();
    assert_eq!(
        merged,
        [(
            "smith2020climate".to_string(),
            vec!["Smith_2020".to_string()]
        )]
    );
    let mut keys: Vec<&str> = lib.entries().map(|e| e.key.as_str()).collect();
    keys.sort();
    assert_eq!(keys, ["brown2021", "smith2019preprint", "smith2020climate"]);
    assert_eq!(
        field(&lib, "smith2020climate", "journal").unwrap(),
        "Nature"
    );
    assert_eq!(
        field(&lib, "smith2020climate", "ids").unwrap(),
        "Smith_2020"
    );
    assert_eq!(
        field(&lib, "smith2020climate", "file").unwrap(),
        ":files/smith.pdf:pdf;:other/smith-preprint.pdf:pdf"
    );
    // the former keys are appended without repetition
    let with_ids = group.replace(
        " journal = {Nature},",
        " ids = {old, Smith_2020},\n journal = {Nature},",
    );
    let mut lib = disk.library(&with_ids, Format::name());
    lib.check_duplicates(&mut Resolver::new(OnDuplicate::Keep))
        .unwrap();
    assert_eq!(
        field(&lib, "smith2020climate", "ids").unwrap(),
        "old, Smith_2020"
    );
}

// test_merge_dup_unit.py, test_duplicates.py: the merge
// -----------------------------------------------------

fn entries(text: &str) -> Vec<Entry> {
    Bib::parse(text).entries().cloned().collect()
}

#[test]
fn merge_branches() {
    const A: &str = "@article{a, author = {Smith, John}, doi = {10.1000/a}, journal = {J1}, title = {T}, year = {2000}}";
    const B: &str = "@article{b, author = {Smith, John}, doi = {10.1000/a}, journal = {J2}, title = {T}, year = {2000}}";
    const C: &str = "@article{c, author = {Smith, John}, doi = {10.1000/a}, journal = {J2}, title = {T}, year = {2000}}";
    // an empty value is no value
    let m = Merge::new(
        &[
            entry("a", "article", &[("note", "")]),
            entry("b", "article", &[]),
        ],
        None,
        false,
    );
    let note = m.fields.iter().find(|f| f.name == "note").unwrap();
    assert_eq!((note.status, note.value()), (Status::Same, Some("")));
    // entries 2 and 3 have the same journal: one alternative for both
    let m = Merge::new(&entries(&format!("{A}{B}{C}")), None, true);
    let journal = m
        .conflicts()
        .into_iter()
        .find(|f| f.name == "journal")
        .unwrap()
        .alternatives();
    assert_eq!(journal, [(vec![1, 2], Some("J2".to_string()))]);
    // an entry that has everything the other has is exact
    let (a, b) = (&entries(A)[0], entries(&A.replace("{a,", "{b,"))[0].clone());
    assert!(Merge::new(&[a.clone(), b], None, false).is_exact());
    // the table of a book
    let book = &entries("@book{b, isbn = {0-306-40615-2}, title = {T}}")[0];
    let id = Identifiers::of(book).identity();
    assert_eq!(
        (id.isbn.as_deref(), id.title.as_deref()),
        (Some("9780306406157"), Some("t"))
    );
}

#[test]
fn the_merge_model() {
    let lib = entry(
        "smith2020climate",
        "article",
        &with(&[
            ("doi", "10.1000/abc123"),
            ("journal", "Nature"),
            ("keywords", "climate, mine"),
            ("file", ":/tmp/files/smith.pdf:pdf"),
        ]),
    );
    let new = entry(
        "Smith_2020",
        "article",
        &[
            ("doi", "10.1000/ABC123"),
            ("journal", "Nature Climate Change"),
            ("volume", "10"),
            ("file", ":/tmp/other/smith-preprint.pdf:pdf"),
            ("author", "Smith, J. and Doe, J."),
            ("title", "Climate Sensitivity Revisited"),
            ("year", "2020"),
        ],
    );
    let mut m = Merge::new(&[lib, new], Some("/"), false);
    assert!(!m.is_exact());
    assert_eq!(m.new_files, ["/tmp/other/smith-preprint.pdf"]);
    m.choose_all(1);
    let r = m.result();
    assert_eq!(
        (r.key.as_str(), r.get("journal")),
        ("smith2020climate", Some("Nature Climate Change"))
    );
    // two links to one file by its real path
    let disk = Disk::new();
    let pdf = disk.write("a.pdf", "x");
    std::os::unix::fs::symlink(&pdf, format!("{}/link.pdf", disk.root)).unwrap();
    let m = Merge::new(
        &[
            entry("a", "article", &[("file", &format!(":{pdf}:pdf"))]),
            entry(
                "b",
                "article",
                &[("file", &format!(":{}/./link.pdf:pdf", disk.root))],
            ),
        ],
        None,
        false,
    );
    assert_eq!(m.files, [pdf]);
    assert!(m.new_files.is_empty() && m.is_exact());
    // the same content under two names too
    let other = disk.write("copy.pdf", "x");
    let m = Merge::new(
        &[
            entry(
                "a",
                "article",
                &[("file", &format!(":{}/a.pdf:pdf", disk.root))],
            ),
            entry("b", "article", &[("file", &format!(":{other}:pdf"))]),
        ],
        None,
        false,
    );
    assert_eq!(m.files.len(), 1);
}

// the tests of papers 3.0.0b6 for bibtex.md
// -----------------------------------------

fn name_of(template: &str, kind: FormatKind, fields: &[(&str, &str)]) -> String {
    let mut f = if kind == FormatKind::Key {
        Format::key()
    } else {
        Format::name()
    };
    f.template = template.into();
    f.unknown_template = None;
    f.render(&entry("k", "article", fields)).unwrap()
}

#[test]
fn file_names_are_valid_on_every_system() {
    // test_filename_unit.py TestSafeFileNames
    let n = FormatKind::Name;
    let fields = [
        ("author", "Doe, J."),
        ("year", "2000"),
        ("journal", "Nature: \"A|B\" <x>?*\x01. "),
    ];
    assert_eq!(
        name_of("{journal}/{author}_{year}", n, &fields),
        "Nature- -A-B- -x----/doe_2000"
    );
    assert_eq!(
        name_of(
            "{journal}/{author}",
            n,
            &[("author", "Nul"), ("journal", "con.x")]
        ),
        "con-.x/nul-"
    );
    // no journal: no 'None' folder, an empty folder of the template is left out
    assert_eq!(
        name_of(
            "{journal}/{author}_{year}",
            n,
            &[("author", "Doe, J."), ("year", "2000")]
        ),
        "doe_2000"
    );
    assert_eq!(
        name_of(
            "{author}{journal}",
            FormatKind::Key,
            &[("author", "Doe, J.")]
        ),
        "doe"
    );
    // a key is not a file name
    assert_eq!(
        name_of("{journal}", FormatKind::Key, &[("journal", "a:b")]),
        "a:b"
    );
}

#[test]
fn file_names_are_at_most_250_bytes() {
    use papers_rs::names::{MAX_NAME_BYTES, attachment_names, fit_name};
    let mut f = Format::name();
    f.template = "{journal}/{author}".into();
    f.unknown_template = None;
    f.author_num = 200;
    let author = vec!["Müller"; 200].join(" and ");
    let e = entry(
        "k",
        "article",
        &[("author", author.as_str()), ("journal", &"é".repeat(300))],
    );
    let name = f.render(&e).unwrap();
    let (folder, base) = name.split_once('/').unwrap();
    assert!(folder.len() <= MAX_NAME_BYTES && base.len() <= MAX_NAME_BYTES);
    let fitted = fit_name(&format!("{name}.pdf"));
    assert!(fitted.ends_with(".pdf"));
    assert_eq!(fitted.split_once('/').unwrap().1.len(), MAX_NAME_BYTES);
    // the other files of an entry, with a suffix for two that coincide
    let target = format!("files/{}", "a".repeat(240));
    let files = ["x.pdf", "a/supplement.pdf", "b/supplement.pdf"].map(String::from);
    let names = attachment_names(&files, &target);
    assert_eq!(names[0], format!("{target}.pdf"));
    for n in &names {
        assert!(n.split_once('/').unwrap().1.len() <= MAX_NAME_BYTES + 2);
    }
    assert_eq!(
        names.iter().collect::<std::collections::HashSet<_>>().len(),
        3
    );
}

#[test]
fn an_extension_that_leaves_no_room_for_the_name_is_cut_too() {
    // test_filename_unit.py TestSafeFileNames::test_long_extension
    use papers_rs::names::{MAX_NAME_BYTES, fit_name};
    let long = |c: &str, n: usize| c.repeat(n);
    assert_eq!(
        fit_name(&format!("/lib/{}.{}", long("a", 300), long("b", 300))),
        format!("/lib/a.{}", long("b", 248))
    );
    assert_eq!(
        fit_name(&format!("/lib/{}.{}", long("a", 10), long("é", 200))),
        format!("/lib/a.{}", long("é", 124))
    );
    let fitted = fit_name(&format!("x.{}", long("é", 200)));
    assert!(fitted.len() <= MAX_NAME_BYTES);
}

#[test]
fn the_key_of_a_new_entry_without_a_key_from_the_template_is_made_of_its_title() {
    // test_spec_bibtex.py TestKeySuffix::test_empty_template_key
    use papers_rs::names::fallback_key;
    assert_eq!(fallback_key("Only a title", "2000"), "only2000");
    assert_eq!(fallback_key("Only a title", ""), "only");
    assert_eq!(fallback_key("A b", ""), "a");
    assert_eq!(fallback_key("100 years", "2000"), "years2000");
    assert_eq!(fallback_key("", "2000"), "entry");
    assert_eq!(fallback_key("100", ""), "entry");
}

#[test]
fn only_the_ascii_digits_are_digits_of_keys_and_identifiers() {
    // bibtex.md 7.2, 9.2
    use papers_rs::identity::{normalize_arxiv, normalize_isbn, parse_identifier};
    use papers_rs::names::{append_abc, check_key};
    assert!(check_key("٣abc").is_none() && check_key("３abc").is_none());
    assert!(check_key("0abc").is_some() && check_key("9abc").is_some());
    assert_eq!(append_abc("x٣b", &[]).unwrap(), "x٣bb");
    assert_eq!(append_abc("x3b", &[]).unwrap(), "x3c");
    assert_eq!(normalize_arxiv("٢١٠١.٠١٢٣٤"), None);
    assert_eq!(normalize_isbn("９７８０３０６４０６１５７"), None);
    assert_eq!(parse_identifier("٣abc").unwrap().1, "٣abc");
    assert!(parse_identifier("3abc").is_err());
}

#[test]
fn doi_or_id_takes_the_key_without_a_doi() {
    // an empty doi, or one of spaces, is no doi
    use papers_rs::names::Format;
    let mut f = Format::name();
    f.template = "{doi_or_id}".into();
    f.unknown_template = None;
    let none = entry("Smith.2020", "article", &[("doi", "  ")]);
    assert_eq!(f.render(&none).unwrap(), "smith-2020");
    let doi = entry("k", "article", &[("doi", "10.1000/ABC")]);
    assert_eq!(f.render(&doi).unwrap(), "10-1000-abc");
}

#[test]
fn slugs_are_made_of_the_own_table() {
    // test_filename_unit.py TestSlugTable: papers' own table (AnyAscii), apostrophes included
    use papers_rs::names::slugify;
    assert_eq!(slugify("x˯x"), "xvx");
    assert_eq!(
        slugify("l'objet Объект Хрущёв µm"),
        "l-objet-obekt-khrushchev-mm"
    );
}

#[test]
fn braces_are_one_name() {
    use papers_rs::bib::encoding::{family_names, standard_name};
    use papers_rs::identity::split_names;
    // test_encoding.py, test_entries.py
    assert_eq!(
        family_names("{Barnes and Noble} and {Smith, Jones}, J. and John Doe"),
        ["{Barnes and Noble}", "{Smith, Jones}", "Doe"]
    );
    assert_eq!(standard_name("John {van der Berg}"), "{van der Berg}, John");
    assert_eq!(
        standard_name("{World Health Organization}"),
        "{World Health Organization}"
    );
    assert_eq!(
        standard_name("John Smith and\n  Jane Doe"),
        "Smith, John and Doe, Jane"
    );
    assert_eq!(
        standard_name("Smith, John and {Barnes and Noble}"),
        "Smith, John and {Barnes and Noble}"
    );
    assert_eq!(
        standard_name("Jean-Paul de la Fontaine"),
        "Fontaine, Jean-Paul de la"
    );
    // test_merge_dup_unit.py
    assert_eq!(
        parse_name("{Smith, Jones and Co}"),
        ("{Smith, Jones and Co}".into(), String::new())
    );
    assert_eq!(
        parse_name("{Smith, Jones}, Ltd"),
        ("{Smith, Jones}".into(), "Ltd".into())
    );
    assert_eq!(
        split_names("{Barnes and Noble} and Smith, J."),
        ["{Barnes and Noble}", "Smith, J."]
    );
    let e = entry(
        "k",
        "article",
        &[
            ("author", "{Barnes and Noble} and Smith, J."),
            ("title", "T"),
        ],
    );
    assert_eq!(Identifiers::of(&e).author, "barnes and noble smith");
}

#[test]
fn file_links_are_trimmed() {
    use papers_rs::bib::encoding::parse_file;
    // test_entries.py test_parse_files_trimmed
    assert_eq!(parse_file("a.pdf; b.pdf"), ["a.pdf", "b.pdf"]);
    assert_eq!(parse_file(":a.pdf :pdf; :b.pdf:pdf"), ["a.pdf", "b.pdf"]);
    assert_eq!(parse_file(r":C:\x.pdf:pdf"), [r"C:\x.pdf"]);
    assert_eq!(parse_file(r"C:\x.pdf"), ["C"]);
    // a file given twice, however written, is one link (test_spec_bibtex.py TestFilePaths)
    let mut e = Entry::new("article", "x");
    set_files(
        &mut e,
        &[
            "/lib/files/./a.pdf".into(),
            "/lib/files/a.pdf".into(),
            "/lib/b.pdf".into(),
        ],
        "/lib",
    );
    assert_eq!(e.get("file"), Some(":files/a.pdf:pdf;:b.pdf:pdf"));
    set_files(&mut e, &[], "/lib");
    assert_eq!(e.get("file"), None);
}

#[test]
fn tags_are_compared_without_case() {
    use papers_rs::entry::{add_tags, remove_tags, rename_tag, tags};
    // test_entries.py test_tags_without_case
    let mut e = Entry::new("article", "a");
    e.set("keywords", "climate, Ice");
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    add_tags(&mut e, &s(&["Climate", "ocean", "OCEAN"]));
    assert_eq!(tags(&e), ["climate", "Ice", "ocean"]);
    remove_tags(&mut e, &s(&["ice"]));
    assert_eq!(tags(&e), ["climate", "ocean"]);
    assert!(rename_tag(&mut e, "CLIMATE", "Climate")); // a rename can change the case
    assert_eq!(tags(&e), ["Climate", "ocean"]);
    add_tags(&mut e, &s(&["Climate/amoc"]));
    assert!(rename_tag(&mut e, "climate", "sea"));
    assert_eq!(tags(&e), ["sea", "ocean", "sea/amoc"]);
}

#[test]
fn the_key_of_an_entry_in_the_library_is_not_taken() {
    // test_bib_fix.py test_auto_key_keeps_a_right_key: an entry whose key is the template key keeps it
    let disk = Disk::new();
    let text = "@article{smith2000,\n author = {Smith, J.},\n title = {T},\n year = {2000}\n}\n\n@article{smith2000b,\n author = {Smith, K.},\n title = {U},\n year = {2000}\n}\n";
    let lib = disk.library(text, Format::name());
    let own = lib.get("smith2000").unwrap().clone();
    assert_eq!(lib.regenerate_key(&own).unwrap(), "smith2000");
    // an entry that is not in the library gets a letter
    assert_eq!(lib.generate_key(&own).unwrap(), "smith2000c");
}

#[test]
fn a_merge_keeps_bare_values_and_adds_no_empty_field() {
    use papers_rs::bib::Block;
    use papers_rs::merge::apply_entry;
    // test_merge_dup_unit.py test_bare_value_stays_bare, test_empty_field_not_added
    let es =
        entries("@article{a, month = jan, title = {T}}\n@article{b, title = {T}, year = {2000}}");
    let merged = Merge::new(&[es[0].clone(), es[1].clone()], None, false).result();
    let mut bib = Bib::default();
    bib.blocks.push(Block::Entry(merged));
    let written = bib.to_string();
    assert!(written.contains(" month = jan,"), "{written}");
    assert!(written.contains(" year = {2000}"), "{written}");
    // written into the library entry
    let mut target = es[1].clone();
    apply_entry(
        &mut target,
        &Merge::new(&[es[1].clone(), es[0].clone()], None, false).result(),
    );
    let mut bib = Bib::default();
    bib.blocks.push(Block::Entry(target));
    assert!(bib.to_string().contains(" month = jan,"));
    // an empty field of the added entry is not added; an empty field of the target stays
    let es = entries("@article{a, note = {}, title = {T}}\n@article{b, pages = {}, title = {T}}");
    let result = Merge::new(&es, None, false).result();
    assert_eq!(result.get("pages"), None);
    assert_eq!(result.get("note"), Some(""));
    // given names beyond the shorter name are not compared (J. and John Quincy)
    assert!(papers_rs::merge::names_compatible(
        "Smith, J.",
        "Smith, John Quincy"
    ));
}

#[test]
fn a_crossref_follows_the_merge() {
    // test_duplicates.py test_crossref_follows_the_merge
    let disk = Disk::new();
    let text = "@book{proc2000a, title = {Proceedings of X}, year = {2000}, doi = {10.1000/p}}\n\n@book{proc2000b, title = {Proceedings of X}, year = {2000}, doi = {10.1000/p}, publisher = {P}}\n\n@inproceedings{talk2000, title = {A talk}, crossref = {proc2000a}}\n";
    let mut lib = disk.library(text, Format::name());
    lib.check_duplicates(&mut Resolver::new(OnDuplicate::Keep))
        .unwrap();
    let keys: Vec<String> = lib.entries().map(|e| e.key.clone()).collect();
    let target = if keys.contains(&"proc2000b".to_string()) {
        "proc2000b"
    } else {
        "proc2000a"
    };
    assert_eq!(field(&lib, "talk2000", "crossref").as_deref(), Some(target));
    assert_eq!(keys.len(), 2);
}

#[test]
fn ids_are_the_former_keys_once() {
    // test_entry_cmds.py test_keep_old_key_once, test_spec_bibtex.py TestIds
    let disk = Disk::new();
    let mut lib = disk.library(
        "@article{a,\n ids = {Old, new},\n title = {T}\n}\n",
        Format::name(),
    );
    lib.set_key("a", "b", true).unwrap();
    assert_eq!(field(&lib, "b", "ids").as_deref(), Some("Old, new, a"));
    lib.set_key("b", "a", true).unwrap(); // back: never the key itself
    assert_eq!(field(&lib, "a", "ids").as_deref(), Some("Old, new, b"));
    lib.set_key("a", "b", true).unwrap(); // a former key once
    assert_eq!(field(&lib, "b", "ids").as_deref(), Some("Old, new, a"));
    // without keep-old-key a part equal to the new key goes, and the field with the last of them
    let mut lib = disk.library(
        "@article{a,\n ids = {B},\n title = {T}\n}\n",
        Format::name(),
    );
    lib.set_key("a", "b", false).unwrap();
    assert_eq!(field(&lib, "b", "ids"), None);
}

#[test]
fn the_same_paper_with_a_taken_key_is_folded_in_when_nothing_differs() {
    // test_bib_fix.py test_same_key_title_by_case: a title that differs by case is no difference
    let disk = Disk::new();
    let mut lib = disk.library(LIB, Format::name());
    let same_key = "@article{smith2020climate,\n author = {Smith, John and Doe, Jane},\n doi = {10.1000/abc123},\n file = {:files/smith.pdf:pdf},\n journal = {Nature},\n keywords = {climate, mine},\n title = {Climate Sensitivity Revisited},\n year = {2020}\n}\n";
    let result = lib
        .add_bibtex(
            same_key,
            Some(&disk.root),
            &[],
            false,
            &InsertOptions::default(),
            &mut Resolver::new(OnDuplicate::Error),
        )
        .unwrap();
    assert_eq!(result[0].what, What::ExactDuplicate);
    assert_eq!(lib.len(), 1);
    // another version of the paper (a field to add) is not: the key is taken
    let more = same_key.replace(" year = {2020}", " volume = {1},\n year = {2020}");
    let err = lib
        .add_bibtex(
            &more,
            Some(&disk.root),
            &[],
            false,
            &InsertOptions::default(),
            &mut Resolver::new(OnDuplicate::Error),
        )
        .unwrap_err();
    assert!(
        err.message()
            .contains("for another paper or another version of it")
    );
}

#[test]
fn a_bibtex_to_add_with_entries_that_cannot_be_read() {
    // test_unreadable.py test_added_bibtex_warns: they are not added
    let disk = Disk::new();
    let mut lib = disk.library("", Format::name());
    lib.add_bibtex(
        "@article{a2000, title={A}, year={2000}}\n\n@article{b2000, Title={B}, title={C}}\n",
        Some(&disk.root),
        &[],
        false,
        &InsertOptions::default(),
        &mut Resolver::new(OnDuplicate::Error),
    )
    .unwrap();
    let keys: Vec<&str> = lib.entries().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, ["a2000"]);
}

#[test]
fn only_commands_and_braces_are_decoded_in_a_path() {
    // test_files_unit.py test_fix_mendeley_path_keeps_text_signs
    use papers_rs::bib::latex::latex_to_unicode;
    assert_eq!(
        latex_to_unicode("/a/x--y~z$w{\\_}{\\\"u}.pdf", false),
        "/a/x--y~z$w_ü.pdf"
    );
}
