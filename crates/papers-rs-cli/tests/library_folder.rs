//! The files of `.papers/` are in the library folder (`library.md`, Layout), also when the bibtex is in
//! another folder, and the lock writes the `.papers/.gitignore`: what the tests of papers do in
//! `tests/test_library_folder.py` and `tests/test_spec_library.py` (`TestLayout`), and the cases of
//! `fixtures/cases` (`layout-*`, recorded from papers) do without a writing command.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: String,
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

impl Sandbox {
    fn new() -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let s = Sandbox { _tmp: tmp, root };
        let config = if cfg!(target_os = "macos") {
            s.path("home/Library/Application Support/papers")
        } else {
            s.path("home/.config/papers")
        };
        fs::create_dir_all(&config).unwrap();
        let machine = serde_json::json!({"history-dir": s.path("history"), "version": 3});
        fs::write(format!("{config}/config.json"), machine.to_string()).unwrap();
        s
    }

    fn path(&self, rel: &str) -> String {
        format!("{}/{rel}", self.root)
    }

    fn write(&self, rel: &str, content: &str) {
        fs::create_dir_all(Path::new(&self.path(rel)).parent().unwrap()).unwrap();
        fs::write(self.path(rel), content).unwrap();
    }

    fn read(&self, rel: &str) -> Option<String> {
        fs::read_to_string(self.path(rel)).ok()
    }

    fn papers(&self, cwd: &str, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_papers-rs"))
            .args(args)
            .current_dir(self.path(cwd))
            .env_clear()
            .env("HOME", self.path("home"))
            .env("LANG", "C.UTF-8")
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    }

    fn ok(&self, cwd: &str, args: &[&str]) -> String {
        let output = self.papers(cwd, args);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{args:?}: {}",
            text(&output.stderr)
        );
        text(&output.stdout)
    }

    fn exists(&self, rel: &str) -> bool {
        Path::new(&self.path(rel)).exists()
    }
}

const NEW_BIB: &str = "@article{mine2022,\n author = {Mine, Me},\n doi = {10.1000/mine},\n title = {My own paper},\n year = {2022}\n}\n";

#[test]
fn the_origin_records_are_in_the_library_folder_not_in_that_of_the_bibtex() {
    let s = Sandbox::new();
    s.write("new.bib", NEW_BIB);
    s.ok(".", &["init", "pool"]);
    s.ok(
        ".",
        &[
            "init",
            "lib",
            "--pool",
            "pool",
            "--bibtex",
            "lib/bib/papers.bib",
        ],
    );
    s.ok("lib", &["add", "../new.bib"]);
    let records: Vec<_> = fs::read_dir(s.path("lib/.papers/origin"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(records.len(), 1);
    assert!(!s.exists("lib/bib/.papers"));
    // the lock and the checksums are in the library folder too
    assert!(!s.exists("lib/.papers/lock"));
    assert!(s.exists("lib/.papers/config.json"));
}

#[test]
fn the_records_of_an_earlier_build_next_to_the_bibtex_move_when_the_library_is_loaded() {
    let s = Sandbox::new();
    s.write("new.bib", NEW_BIB);
    s.ok(".", &["init", "pool"]);
    s.ok(
        ".",
        &[
            "init",
            "lib",
            "--pool",
            "pool",
            "--bibtex",
            "lib/bib/papers.bib",
        ],
    );
    s.ok("lib", &["add", "../new.bib"]);
    // as an earlier build wrote them
    let name = fs::read_dir(s.path("lib/.papers/origin"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .file_name();
    let name = name.to_string_lossy().into_owned();
    s.write(
        "lib/bib/.papers/origin/placed.bib",
        &s.read(&format!("lib/.papers/origin/{name}")).unwrap(),
    );
    fs::remove_dir_all(s.path("lib/.papers/origin")).unwrap();
    s.write("lib/bib/.papers/distinct.json", "{\"distinct\": []}\n");
    let output = s.papers("lib", &["list", "-v"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(
        s.exists("lib/.papers/origin/placed.bib"),
        "{}",
        text(&output.stderr)
    );
    assert_eq!(
        s.read("lib/.papers/distinct.json").as_deref(),
        Some("{\"distinct\": []}\n")
    );
    assert!(!s.exists("lib/bib/.papers/origin") && !s.exists("lib/bib/.papers/distinct.json"));
    // a dry run moves nothing
    s.write(
        "lib/bib/.papers/distinct.json",
        "{\"distinct\": [[1, 2]]}\n",
    );
    s.ok("lib", &["list", "--dry-run"]);
    assert!(s.exists("lib/bib/.papers/distinct.json"));
    assert_eq!(
        s.read("lib/.papers/distinct.json").as_deref(),
        Some("{\"distinct\": []}\n")
    );
}

#[test]
fn nothing_moves_with_another_spelling_of_the_dry_run_or_a_command_line_with_an_error() {
    // library.md, Layout (test_spec_library.py::TestLayout::test_no_move_with_other_spellings_of_dry_run): the move is made
    // once the command line is read, and it says that the command is not a dry run (an abbreviation, `file mv -n`),
    // and is correct (a usage error moves nothing)
    let s = Sandbox::new();
    s.ok(".", &["init", "lib", "--bibtex", "lib/refs/papers.bib"]);
    let moved = |s: &Sandbox| s.exists("lib/.papers/distinct.json");
    let reset = |s: &Sandbox| {
        let _ = fs::remove_file(s.path("lib/.papers/distinct.json"));
        s.write("lib/refs/.papers/distinct.json", "{\"distinct\": []}\n");
    };
    for args in [
        &["list", "--dry"][..],
        &["list", "--dry-run"],
        &["file", "mv", "-n", "x.pdf", "y.pdf"],
        &["list", "--no-such-option"],
        &["undo", "-n", "0"],
        &["--version"],
        &["--help"],
    ] {
        reset(&s);
        s.papers("lib", args);
        assert!(!moved(&s), "{args:?}");
        assert!(s.exists("lib/refs/.papers/distinct.json"), "{args:?}");
    }
    // a command without a dry run moves them
    reset(&s);
    s.ok("lib", &["list"]);
    assert!(moved(&s));
    assert!(!s.exists("lib/refs/.papers/distinct.json"));
}

#[test]
fn the_lock_writes_the_gitignore_of_the_papers_folder() {
    let s = Sandbox::new();
    s.write("x.bib", NEW_BIB);
    let config = "{\"bibtex\": \"papers.bib\", \"filesdir\": \"files\", \"version\": 3}";
    // a configuration that papers never saved: written by hand, or of papers 2
    s.write("hand/.papers/config.json", config);
    s.write(
        "v2/.papersconfig.json",
        "{\"bibtex\": \"papers.bib\", \"local\": true}",
    );
    s.write("mine/.papers/config.json", config);
    s.write("mine/.papers/.gitignore", "mine\n");
    s.write("old/.papers/config.json", config);
    s.write("old/.papers/.gitignore", "backup/\n");
    s.write("dry/.papers/config.json", config);
    for lib in ["hand", "v2", "mine", "old", "dry"] {
        s.write(&format!("{lib}/papers.bib"), "");
    }
    for lib in ["hand", "v2", "mine", "old"] {
        s.ok(lib, &["add", "../x.bib"]);
    }
    assert_eq!(s.read("hand/.papers/.gitignore").as_deref(), Some("*\n"));
    assert_eq!(s.read("v2/.papers/.gitignore").as_deref(), Some("*\n"));
    assert_eq!(s.read("mine/.papers/.gitignore").as_deref(), Some("mine\n"));
    assert_eq!(s.read("old/.papers/.gitignore").as_deref(), Some("*\n"));
    // a dry run takes no lock, and writes nothing; no identifier either
    s.ok("dry", &["add", "../x.bib", "--dry-run"]);
    assert!(!s.exists("dry/.papers/.gitignore"));
    assert_eq!(s.read("dry/.papers/config.json").as_deref(), Some(config));
}
