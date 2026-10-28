//! papers and papers-rs on one library and one history: what papers-rs records, papers undoes, and the
//! reverse (the reference papers is `PAPERS_REFERENCE`, a command):
//!
//! ```text
//! PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test interop -- --ignored
//! ```

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use papers_rs::config::{Config, MachineConfig};
use papers_rs::history::History;
use serde_json::{Map, Value, json};

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: String,
}

fn config_dir(home: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("{home}/Library/Application Support/papers")
    } else {
        format!("{home}/.config/papers")
    }
}

impl Sandbox {
    fn new() -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let s = Sandbox { _tmp: tmp, root };
        // the history of the machine is in the sandbox, for both programs
        let config = config_dir(&s.path("home"));
        fs::create_dir_all(&config).unwrap();
        fs::write(
            format!("{config}/config.json"),
            json!({"history-dir": s.path("history"), "version": 3}).to_string(),
        )
        .unwrap();
        s.write(
            "lib/.papers/config.json",
            "{\n  \"bibtex\": \"papers.bib\",\n  \"filesdir\": \"files\",\n  \"pool\": \"none\",\n  \"version\": 3\n}",
        );
        s.write(
            "lib/papers.bib",
            "@article{old2000,\n author = {Doe, Jane},\n title = {Old},\n year = {2000}\n}\n",
        );
        fs::create_dir_all(s.path("lib/files")).unwrap();
        s
    }

    fn path(&self, rel: &str) -> String {
        format!("{}/{rel}", self.root)
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.path(rel);
        fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap_or_default()
    }

    fn run(&self, program: &str, args: &[&str]) -> Output {
        let mut words = program.split_whitespace();
        let output = Command::new(words.next().unwrap())
            .args(words)
            .args(args)
            .current_dir(self.path("lib"))
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", self.path("home"))
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .env("TZ", "UTC")
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{program} {args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn config(&self) -> Config {
        let mut machine = Map::new();
        machine.insert("history-dir".into(), Value::String(self.path("history")));
        Config::load(
            &self.path("lib/.papers/config.json"),
            MachineConfig::new(None, machine),
            None,
        )
        .unwrap()
    }
}

const RS: &str = env!("CARGO_BIN_EXE_papers-rs");

fn reference() -> Option<String> {
    match std::env::var("PAPERS_REFERENCE") {
        Ok(r) => Some(r),
        Err(_) => {
            eprintln!("skipped: PAPERS_REFERENCE is not set");
            None
        }
    }
}

#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test interop -- --ignored"]
fn papers_undoes_and_redoes_what_papers_rs_recorded() {
    let Some(papers) = reference() else { return };
    let s = Sandbox::new();
    s.write("lib/in.bib", "@article{new2001,\n author = {Roe, R},\n file = {:src/n.txt:txt},\n title = {New},\n year = {2001}\n}\n");
    s.write("lib/src/n.txt", "content\n");
    let start = s.read("lib/papers.bib");

    s.run(RS, &["add", "in.bib", "--move"]);
    let added = s.read("lib/papers.bib");
    assert!(added.contains("new2001") && Path::new(&s.path("lib/files/roe_2001_new.txt")).exists());
    s.run(RS, &["tag", "new2001", "--add", "x"]);
    s.run(RS, &["edit", "old2000", "--set", "note=n"]);
    let before_rm = s.read("lib/papers.bib");
    s.run(RS, &["rm", "new2001", "--delete-files"]);
    assert!(!Path::new(&s.path("lib/files/roe_2001_new.txt")).exists());

    // papers undoes the step of papers-rs that deleted the file, then the others, then redoes them
    s.run(&papers, &["undo"]);
    assert_eq!(s.read("lib/papers.bib"), before_rm);
    assert_eq!(
        s.read("lib/files/roe_2001_new.txt"),
        "content\n",
        "the deleted file is back"
    );
    s.run(&papers, &["undo", "-n", "3"]);
    assert_eq!(s.read("lib/papers.bib"), start);
    assert_eq!(
        s.read("lib/src/n.txt"),
        "content\n",
        "the file moved by add is back where it was"
    );
    assert!(!Path::new(&s.path("lib/files/roe_2001_new.txt")).exists());
    s.run(&papers, &["redo", "-n", "4"]);
    assert!(!s.read("lib/papers.bib").contains("new2001"));
    assert!(s.read("lib/papers.bib").contains("note = {n}"));
    assert!(!Path::new(&s.path("lib/files/roe_2001_new.txt")).exists());
}

#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test interop -- --ignored"]
fn papers_rs_undoes_and_redoes_what_papers_recorded() {
    let Some(papers) = reference() else { return };
    let s = Sandbox::new();
    s.write("lib/in.bib", "@article{new2001,\n author = {Roe, R},\n file = {:src/n.txt:txt},\n title = {New},\n year = {2001}\n}\n");
    s.write("lib/src/n.txt", "content\n");
    let start = s.read("lib/papers.bib");

    s.run(&papers, &["add", "in.bib", "--move"]);
    assert!(Path::new(&s.path("lib/files/roe_2001_new.txt")).exists());
    s.run(&papers, &["tag", "new2001", "--add", "x"]);
    let before_rm = s.read("lib/papers.bib");
    s.run(&papers, &["rm", "new2001", "--delete-files"]);

    // papers-rs continues the history: a step of its own on top, then undoes all of them
    s.run(RS, &["edit", "old2000", "--set", "note=n"]);
    let edited = s.read("lib/papers.bib");
    let mut config = s.config();
    let moved = History::require(&mut config, false)
        .unwrap()
        .undo(2, false, None, "papers undo")
        .unwrap();
    let _ = moved;
    assert_eq!(
        s.read("lib/papers.bib"),
        before_rm,
        "the step of papers-rs and the rm of papers are undone"
    );
    assert_eq!(
        s.read("lib/files/roe_2001_new.txt"),
        "content\n",
        "the file deleted by papers is back"
    );
    let mut config = s.config();
    History::require(&mut config, false)
        .unwrap()
        .undo(2, false, None, "papers undo")
        .unwrap();
    assert_eq!(s.read("lib/papers.bib"), start);
    assert_eq!(
        s.read("lib/src/n.txt"),
        "content\n",
        "the file moved by papers is back where it was"
    );
    let mut config = s.config();
    History::require(&mut config, false)
        .unwrap()
        .redo(4, false, None, "papers redo")
        .unwrap();
    assert_eq!(s.read("lib/papers.bib"), edited);
    assert!(!Path::new(&s.path("lib/files/roe_2001_new.txt")).exists());
}

#[test]
fn papers_rs_adds_tags_edits_and_removes_without_the_reference() {
    let s = Sandbox::new();
    s.write("lib/in.bib", "@article{new2001,\n author = {Roe, R},\n file = {:src/n.txt:txt},\n title = {New},\n year = {2001}\n}\n");
    s.write("lib/src/n.txt", "content\n");
    let start = s.read("lib/papers.bib");
    s.run(RS, &["add", "in.bib", "--move"]);
    s.run(RS, &["tag", "new2001", "--add", "x"]);
    s.run(RS, &["rm", "new2001", "--delete-files"]);
    assert_eq!(s.read("lib/papers.bib"), start);
    assert!(!Path::new(&s.path("lib/.papers/lock")).exists());
    let mut config = s.config();
    let history = History::open(&mut config, false, false).unwrap().unwrap();
    assert_eq!(
        history.project.states().len(),
        4,
        "the library before, and three commands"
    );
}

/// The file commands (attach, mv, rm, clean) run by `first`, then undone and redone by `second`:
/// each finds the files that the other moved, copied and deleted.
fn file_commands_run_by_one_are_undone_by_the_other(first: &str, second: &str) {
    let s = Sandbox::new();
    s.write("lib/x.txt", "x\n");
    s.write("lib/y.txt", "y\n");
    s.write("lib/files/unlinked.txt", "u\n");
    let start = s.read("lib/papers.bib");

    s.run(first, &["file", "attach", "old2000", "x.txt", "y.txt"]);
    assert!(Path::new(&s.path("lib/files/doe_2000_old.txt")).exists());
    assert!(Path::new(&s.path("lib/files/doe_2000_old--y.txt")).exists());
    s.run(first, &["file", "mv", "files", "archive"]);
    let moved = s.read("lib/papers.bib");
    assert!(moved.contains("archive/doe_2000_old.txt"), "{moved}");
    s.run(first, &["file", "rm", "archive/doe_2000_old--y.txt"]);
    assert!(!Path::new(&s.path("lib/archive/doe_2000_old--y.txt")).exists());
    s.run(first, &["file", "clean", "-y"]);
    assert!(!Path::new(&s.path("lib/archive/unlinked.txt")).exists());
    let cleaned = s.read("lib/papers.bib");

    // undo all: the files are where they were
    s.run(second, &["undo", "-n", "4"]);
    assert_eq!(s.read("lib/papers.bib"), start);
    assert_eq!(s.read("lib/x.txt"), "x\n");
    assert_eq!(s.read("lib/y.txt"), "y\n");
    // the file that clean deleted is back where it was deleted: the folder had been moved
    assert_eq!(s.read("lib/archive/unlinked.txt"), "u\n");
    assert!(!Path::new(&s.path("lib/archive/doe_2000_old.txt")).exists());
    s.run(second, &["redo", "-n", "4"]);
    assert_eq!(s.read("lib/papers.bib"), cleaned);
    assert!(Path::new(&s.path("lib/archive/doe_2000_old.txt")).exists());
    assert!(!Path::new(&s.path("lib/archive/unlinked.txt")).exists());
}

#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test interop -- --ignored"]
fn papers_undoes_and_redoes_the_file_commands_of_papers_rs() {
    let Some(papers) = reference() else { return };
    file_commands_run_by_one_are_undone_by_the_other(RS, &papers);
}

#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test interop -- --ignored"]
fn papers_rs_undoes_and_redoes_the_file_commands_of_papers() {
    let Some(papers) = reference() else { return };
    file_commands_run_by_one_are_undone_by_the_other(&papers, RS);
}
