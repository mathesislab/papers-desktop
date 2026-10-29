//! The actions of `papers list` that change the entries it lists (`--add-tag`, `--rm-tag`, `--edit`,
//! `--delete`, `--fetch`): one step of the history each, that papers undoes (the reference papers is
//! `PAPERS_REFERENCE`, a command):
//!
//! ```text
//! PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test list_actions -- --ignored
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

const START: &str = "@article{one2000,\n author = {Doe, Jane},\n file = {:files/one.txt:txt},\n keywords = {a},\n title = {One},\n year = {2000}\n}\n\n@article{two2001,\n author = {Roe, R},\n title = {Two},\n year = {2001}\n}\n";

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
        s.write("lib/papers.bib", START);
        s.write("lib/files/one.txt", "content\n");
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

    fn exists(&self, rel: &str) -> bool {
        Path::new(&self.path(rel)).exists()
    }

    fn output(&self, program: &str, args: &[&str]) -> Output {
        let mut words = program.split_whitespace();
        Command::new(words.next().unwrap())
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
            .unwrap()
    }

    fn run(&self, program: &str, args: &[&str]) -> Output {
        let output = self.output(program, args);
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

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_actions_are_steps_of_the_history_with_the_command_line_as_message() {
    let s = Sandbox::new();
    s.run(RS, &["list", "--key", "one2000", "--add-tag", "b"]);
    assert!(s.read("lib/papers.bib").contains("keywords = {a, b}"));
    s.run(RS, &["list", "--rm-tag", "a", "--key", "one2000"]);
    assert!(s.read("lib/papers.bib").contains("keywords = {b}"));
    s.run(
        RS,
        &["list", "--key", "one2000", "--delete", "--delete-files"],
    );
    assert!(!s.read("lib/papers.bib").contains("one2000"));
    assert!(!s.exists("lib/files/one.txt"));
    assert!(!s.exists("lib/.papers/lock"), "the lock is released");
    let mut config = s.config();
    let history = History::open(&mut config, false, false).unwrap().unwrap();
    let states = history.project.states();
    assert_eq!(states.len(), 4, "the library before, and three commands");
    assert_eq!(
        states[&history.head().unwrap().state].message(),
        "papers list --key one2000 --delete --delete-files"
    );
    // papers-rs undoes them as well
    let output = s.run(RS, &["undo", "-n", "3"]);
    assert!(text(&output.stdout).starts_with("undo: papers.bib restored"));
    assert_eq!(s.read("lib/papers.bib"), START);
    assert_eq!(s.read("lib/files/one.txt"), "content\n");
}

#[test]
fn a_listing_that_changes_nothing_records_nothing() {
    let s = Sandbox::new();
    s.run(RS, &["list", "--key", "one2000", "--rm-tag", "notthere"]);
    s.run(RS, &["list", "--key", "nosuch", "--add-tag", "x"]);
    s.run(RS, &["list", "--key", "nosuch", "--delete"]);
    assert_eq!(s.read("lib/papers.bib"), START);
    let mut config = s.config();
    let history = History::open(&mut config, false, false).unwrap();
    assert!(history.is_none_or(|h| h.project.states().is_empty()));
}

#[test]
fn one_action_at_a_time_and_the_exchanges_need_a_pool() {
    let s = Sandbox::new();
    let output = s.output(RS, &["list", "--add-tag", "x", "--delete"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).contains("one action at a time: --delete and --add-tag"));
    for option in ["--push", "--pull", "--sync"] {
        let output = s.output(RS, &["list", option]);
        assert_eq!(output.status.code(), Some(1), "{option}");
        assert_eq!(
            text(&output.stderr),
            "papers: error: this library has no pool: give one with `--pool PATH`\n"
        );
    }
    assert_eq!(s.read("lib/papers.bib"), START);
}

#[test]
fn fetch_has_nothing_to_ask_when_nothing_changes() {
    let s = Sandbox::new();
    // no entry has a DOI or a title with authors to fetch, and the names are standard: no question
    s.write(
        "lib/papers.bib",
        "@misc{plain2021,\n journal = {Nature},\n year = {2021}\n}\n",
    );
    let output = s.run(RS, &["list", "--fetch"]);
    assert_eq!(text(&output.stdout), "");
    assert!(text(&output.stderr).contains("`papers list --fetch` is deprecated"));
}

#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test list_actions -- --ignored"]
fn papers_undoes_the_steps_of_the_actions_of_list() {
    let Ok(papers) = std::env::var("PAPERS_REFERENCE") else {
        eprintln!("skipped: PAPERS_REFERENCE is not set");
        return;
    };
    let s = Sandbox::new();
    s.run(RS, &["list", "--key", "one2000", "--add-tag", "b"]);
    let tagged = s.read("lib/papers.bib");
    s.run(
        RS,
        &["list", "--key", "one2000", "--delete", "--delete-files"],
    );
    assert!(!s.exists("lib/files/one.txt"));
    // the step of the deletion: the entry and its file are back
    s.run(&papers, &["undo"]);
    assert_eq!(s.read("lib/papers.bib"), tagged);
    assert_eq!(s.read("lib/files/one.txt"), "content\n");
    // the step of the tag
    s.run(&papers, &["undo"]);
    assert_eq!(s.read("lib/papers.bib"), START);
    s.run(&papers, &["redo", "-n", "2"]);
    assert!(!s.read("lib/papers.bib").contains("one2000"));
    assert!(!s.exists("lib/files/one.txt"));
}
