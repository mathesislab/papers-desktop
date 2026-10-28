//! The write path (`papers_rs::command`) on libraries made in a temporary folder: the order of what a
//! command does around its change, what it leaves in the history, and what it refuses.

use std::fs;
use std::path::Path;

use papers_rs::command::add::{AddRequest, add};
use papers_rs::command::files::Mode;
use papers_rs::command::report::print_changes;
use papers_rs::command::rm::{Files, remove_entries};
use papers_rs::command::tag::{add_remove, rename, tag_tree};
use papers_rs::command::{Options, Session};
use papers_rs::config::{Config, MachineConfig};
use papers_rs::history::{History, OpKind};
use papers_rs::merge::{OnDuplicate, Resolver};
use serde_json::{Map, Value, json};

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: String,
}

fn sandbox() -> Sandbox {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    Sandbox { _tmp: tmp, root }
}

fn entry(key: &str, extra: &str) -> String {
    format!(
        "@article{{{key},\n author = {{Doe, Jane}},{extra}\n title = {{Title of {key}}},\n year = {{2000}},\n}}\n\n"
    )
}

impl Sandbox {
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

    /// A library as `papers init` makes it, with this bibtex.
    fn library(&self, bibtex: &str) -> Config {
        self.write("lib/.papers/config.json", "{\n  \"bibtex\": \"papers.bib\",\n  \"filesdir\": \"files\",\n  \"pool\": \"none\",\n  \"version\": 3\n}");
        self.write("lib/papers.bib", bibtex);
        fs::create_dir_all(self.path("lib/files")).unwrap();
        self.config()
    }

    fn config(&self) -> Config {
        let mut machine = Map::new();
        machine.insert("history-dir".into(), json!(self.path("history")));
        Config::load(
            &self.path("lib/.papers/config.json"),
            MachineConfig::new(None, machine),
            None,
        )
        .unwrap()
    }

    fn session(&self, message: &str) -> Session {
        Session::open(
            self.config(),
            Options {
                message: message.to_string(),
                ..Options::default()
            },
        )
        .unwrap()
    }

    fn history_states(&self) -> usize {
        let mut config = self.config();
        match History::open(&mut config, false, false).unwrap() {
            Some(h) => h.project.states().len(),
            None => 0,
        }
    }
}

fn request(files: &[&str]) -> AddRequest {
    AddRequest {
        files: files.iter().map(|f| f.to_string()).collect(),
        ..AddRequest::default()
    }
}

#[test]
fn a_command_saves_records_and_leaves_no_lock() {
    let s = sandbox();
    s.library(&entry("old2000", ""));
    s.write("lib/new.bib", &entry("new2001", ""));
    let mut session = s.session("papers add new.bib");
    let lock = s.path("lib/.papers/lock");
    assert!(
        Path::new(&lock).exists(),
        "the lock is held while the command runs"
    );
    let mut resolver = Resolver::new(OnDuplicate::Error);
    let mut r = request(&[&s.path("lib/new.bib")]);
    r.mode = Mode::Link;
    let touched = add(&mut session, &r, &mut resolver).unwrap();
    assert_eq!(touched, ["new2001"]);
    let committed = session.commit().unwrap();
    assert!(committed.saved);
    assert_eq!(
        committed.step,
        Some(2),
        "the library before, then the command"
    );
    print_changes(
        &session.library,
        &touched,
        true,
        &Default::default(),
        false,
        false,
        &session.say,
    );
    let outcome = session.finish();
    assert_eq!(outcome.out.len(), 1);
    assert!(
        outcome.out[0].starts_with("Added -> new2001: Title of new2001 "),
        "{:?}",
        outcome.out
    );
    assert!(s.read("lib/papers.bib").contains("@article{new2001,"));
    assert!(!Path::new(&lock).exists());
    assert_eq!(s.history_states(), 2);
}

#[test]
fn a_command_that_changes_nothing_leaves_no_trace() {
    let s = sandbox();
    s.library(&entry("old2000", ""));
    let before = s.read("lib/papers.bib");
    let mut session = s.session("papers tag old2000 --add x --rm x");
    let keys = session.library.select(&["old2000".to_string()]).unwrap();
    add_remove(
        &mut session.library,
        &keys,
        &["x".to_string()],
        &["x".to_string()],
    );
    let committed = session.commit().unwrap();
    assert!(!committed.saved && committed.step.is_none());
    session.finish();
    assert_eq!(s.read("lib/papers.bib"), before);
    assert!(
        !Path::new(&s.path("history")).exists(),
        "nothing is created by a command that changes nothing"
    );
    assert!(!s.read("lib/.papers/config.json").contains("\"id\""));
}

#[test]
fn a_dry_run_writes_nothing_and_says_so() {
    let s = sandbox();
    s.library(&entry("old2000", ""));
    let before = s.read("lib/papers.bib");
    let options = Options {
        message: "papers tag".into(),
        dry_run: true,
        ..Options::default()
    };
    let mut session = Session::open(s.config(), options).unwrap();
    assert!(
        !Path::new(&s.path("lib/.papers/lock")).exists(),
        "a dry run takes no lock"
    );
    let keys = session.library.select(&["old2000".to_string()]).unwrap();
    add_remove(&mut session.library, &keys, &["x".to_string()], &[]);
    let committed = session.commit().unwrap();
    assert!(!committed.saved);
    let outcome = session.finish();
    assert_eq!(
        outcome.out,
        [format!("DRYRUN: {} not saved", s.path("lib/papers.bib"))]
    );
    assert_eq!(s.read("lib/papers.bib"), before);
    assert!(!Path::new(&s.path("history")).exists());
}

#[test]
fn a_library_with_a_pool_pushes_what_it_adds() {
    let s = sandbox();
    s.library("");
    s.write("pool/.papers/config.json", "{\n  \"bibtex\": \"papers.bib\",\n  \"filesdir\": \"files\",\n  \"pool\": \"none\",\n  \"version\": 3\n}");
    s.write("pool/papers.bib", "");
    s.write(
        "lib/.papers/config.json",
        "{\n  \"bibtex\": \"papers.bib\",\n  \"filesdir\": \"files\",\n  \"pool\": \"../pool\",\n  \"version\": 3\n}",
    );
    // a shallow library is written as the others: what it adds goes to the pool, which has its own step
    let mut session = s.session("papers add new.bib");
    // the lock of the pool is taken when the command reads it
    assert!(Path::new(&s.path("lib/.papers/lock")).exists());
    assert!(Path::new(&s.path("pool/.papers/lock")).exists());
    s.write("lib/new.bib", &entry("new2001", ""));
    let mut resolver = Resolver::new(OnDuplicate::Error);
    let request = AddRequest {
        files: vec![s.path("lib/new.bib")],
        ..AddRequest::default()
    };
    let keys = add(&mut session, &request, &mut resolver).unwrap();
    assert_eq!(keys, ["new2001"]);
    let committed = session.commit().unwrap();
    assert_eq!(
        committed.notes.get("new2001").map(String::as_str),
        Some("also added to pool ../pool")
    );
    session.finish();
    assert!(s.read("pool/papers.bib").contains("title = {Title of new2001}"));
    assert!(s.read("lib/papers.bib").contains("@article{new2001,"));
    assert!(Path::new(&s.path("lib/.papers/origin")).is_dir());
    assert!(!Path::new(&s.path("lib/.papers/lock")).exists());
    assert!(!Path::new(&s.path("pool/.papers/lock")).exists());
}

#[test]
fn a_bibtex_written_meanwhile_is_not_overwritten_and_the_files_are_put_back() {
    let s = sandbox();
    s.library(&entry("old2000", ""));
    s.write(
        "lib/new.bib",
        &entry("new2001", "\n file = {:src/n.txt:txt},"),
    );
    s.write("lib/src/n.txt", "the file\n");
    let mut session = s.session("papers add new.bib");
    let mut resolver = Resolver::new(OnDuplicate::Error);
    let mut r = request(&[&s.path("lib/new.bib")]);
    r.mode = Mode::Move;
    add(&mut session, &r, &mut resolver).unwrap();
    let placed = s.path("lib/files");
    let moved: Vec<_> = fs::read_dir(&placed).unwrap().flatten().collect();
    assert_eq!(
        moved.len(),
        1,
        "the file was moved into the files directory"
    );
    assert!(!Path::new(&s.path("lib/src/n.txt")).exists());
    // another program writes the bibtex
    let other = entry("other2002", "");
    std::thread::sleep(std::time::Duration::from_millis(20));
    s.write("lib/papers.bib", &other);
    let error = session.commit().expect_err("refused");
    assert!(
        error.message().contains("changed since papers read it"),
        "{error}"
    );
    let outcome = session.finish();
    assert!(
        outcome
            .out
            .join("\n")
            .contains("the files of the command are put back:"),
        "{:?}",
        outcome.out
    );
    assert_eq!(
        s.read("lib/papers.bib"),
        other,
        "the bibtex of the other program is left as it is"
    );
    assert_eq!(
        s.read("lib/src/n.txt"),
        "the file\n",
        "the file is back where it was"
    );
    assert_eq!(fs::read_dir(&placed).unwrap().count(), 0);
}

#[test]
fn a_failed_command_records_the_files_it_moved() {
    let s = sandbox();
    s.library(&entry("old2000", ""));
    s.write("lib/a.bib", &entry("a2001", "\n file = {:src/a.txt:txt},"));
    s.write("lib/src/a.txt", "a\n");
    let mut session = s.session("papers add a.bib b.bib");
    let mut resolver = Resolver::new(OnDuplicate::Error);
    let mut r = request(&[&s.path("lib/a.bib"), &s.path("lib/missing.bib")]);
    r.mode = Mode::Move;
    let error = add(&mut session, &r, &mut resolver).expect_err("the second file does not exist");
    assert!(error.message().contains("use --ignore-errors"), "{error}");
    // the library is not saved, but the file was moved: the history has a step with the operation, for undo
    session.finish();
    let mut config = s.config();
    let history = History::open(&mut config, false, false).unwrap().unwrap();
    let states = history.project.states();
    let last = states.iter().last().expect("a state");
    assert_eq!(last.1.ops().len(), 1);
    assert_eq!(last.1.ops()[0].kind, OpKind::Moved);
}

#[test]
fn files_deleted_by_rm_are_kept_in_the_history_and_undo_brings_them_back() {
    let s = sandbox();
    s.library(&(entry("doe2000", "\n file = {:files/d.txt:txt},") + &entry("keep2001", "")));
    s.write("lib/files/d.txt", "deleted\n");
    let mut session = s.session("papers rm doe2000 --delete-files");
    let keys = session.library.select(&["doe2000".to_string()]).unwrap();
    let to_delete = remove_entries(&mut session, &keys, Files::Delete, true).unwrap();
    assert_eq!(to_delete, [s.path("lib/files/d.txt")]);
    session.commit().unwrap();
    print_changes(
        &session.library,
        &keys,
        false,
        &Default::default(),
        false,
        false,
        &session.say,
    );
    session.delete_files(&to_delete, &[]).unwrap();
    let outcome = session.finish();
    assert_eq!(outcome.out.len(), 2, "{:?}", outcome.out);
    assert!(outcome.out[0].starts_with("Removed -> doe2000: "));
    assert!(
        outcome.out[1].ends_with("d.txt (kept in the history: undo restores it)"),
        "{:?}",
        outcome.out
    );
    assert!(!Path::new(&s.path("lib/files/d.txt")).exists());
    assert!(!s.read("lib/papers.bib").contains("doe2000"));

    let mut config = s.config();
    let history = History::open(&mut config, false, false).unwrap().unwrap();
    let states = history.project.states();
    let ops = states.iter().last().unwrap().1.ops();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].kind, OpKind::Trashed);
    assert_eq!(history.project.head(&history.place).unwrap().state, 2);
    drop(history);

    let mut config = s.config();
    History::require(&mut config, false)
        .unwrap()
        .undo(1, false, None, "papers undo", &mut skip())
        .unwrap();
    assert_eq!(s.read("lib/files/d.txt"), "deleted\n");
    assert!(s.read("lib/papers.bib").contains("doe2000"));
}

#[test]
fn tags_are_added_removed_renamed_and_listed() {
    let s = sandbox();
    s.library(&(entry("a", "\n keywords = {x/y, z},") + &entry("b", "\n keywords = {x},")));
    let mut session = s.session("papers tag");
    assert_eq!(
        tag_tree(session.library.entries()),
        ["x (2)", "  y (1)", "z (1)"]
    );
    assert_eq!(rename(&mut session.library, "x", "w"), 2);
    let keys = session.library.select(&["b".to_string()]).unwrap();
    add_remove(
        &mut session.library,
        &keys,
        &["new".to_string()],
        &["w".to_string()],
    );
    session.commit().unwrap();
    print_changes(
        &session.library,
        &keys,
        false,
        &Default::default(),
        true,
        false,
        &session.say,
    );
    let outcome = session.finish();
    assert_eq!(outcome.porcelain, ["modified b"]);
    let text = s.read("lib/papers.bib");
    assert!(text.contains("keywords = {w/y, z}"), "{text}");
    assert!(text.contains("keywords = {new}"), "{text}");
}

#[test]
fn the_checksums_of_the_files_placed_go_to_files_json() {
    let s = sandbox();
    s.library("");
    s.write("lib/n.bib", &entry("n2001", "\n file = {:src/n.txt:txt},"));
    s.write("lib/src/n.txt", "content\n");
    let mut session = s.session("papers add n.bib");
    let mut resolver = Resolver::new(OnDuplicate::Error);
    let mut r = request(&[&s.path("lib/n.bib")]);
    r.mode = Mode::Copy;
    add(&mut session, &r, &mut resolver).unwrap();
    session.commit().unwrap();
    session.finish();
    let files_json = s.read("lib/.papers/files.json");
    assert!(
        files_json.contains("files/doe_2000_title-of-n2001.txt"),
        "{files_json}"
    );
    assert!(files_json.contains("\"files\""), "{files_json}");
    assert_eq!(
        s.read("lib/src/n.txt"),
        "content\n",
        "a copy keeps the original"
    );
    let _: Value = serde_json::from_str(&files_json).unwrap();
}

/// What `papers undo` uses without a terminal for the conflicts of the revert of a pool step.
fn skip() -> papers_rs::merge::Resolver {
    papers_rs::merge::Resolver::new(papers_rs::merge::OnDuplicate::Skip)
}
