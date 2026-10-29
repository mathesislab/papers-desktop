//! `papers history --revert N` (`papers_rs::command::revert`, `Session::revert`) on libraries made in a
//! temporary folder: the tests of papers (`test_revert.py`) ported to the library, step by step.
//! `spec/cases/history/revert-*` and `tests/revert_interop.rs` (cli) compare it with papers itself.

use std::fs;
use std::path::Path;

use papers_rs::command::revert::Revert;
use papers_rs::command::tag::add_remove;
use papers_rs::command::{Options, Session};
use papers_rs::config::{Config, MachineConfig};
use papers_rs::error::Result;
use papers_rs::history::{History, short};
use papers_rs::merge::{OnDuplicate, Resolve, Resolver};
use serde_json::{Map, json};

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: String,
}

fn entry(key: &str) -> String {
    format!(
        "@article{{{key},\n author = {{Doe, Jane}},\n title = {{Title of {key}}},\n year = {{2000}},\n}}\n\n"
    )
}

impl Sandbox {
    fn new(keys: &[&str]) -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let s = Sandbox { _tmp: tmp, root };
        s.write("lib/.papers/config.json", "{\n  \"bibtex\": \"papers.bib\",\n  \"filesdir\": \"files\",\n  \"pool\": \"none\",\n  \"version\": 3\n}");
        s.write(
            "lib/papers.bib",
            &keys.iter().map(|k| entry(k)).collect::<String>(),
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

    /// A command that changes the library: the closure changes it, the session is saved and recorded.
    fn run(&self, message: &str, change: impl FnOnce(&mut Session)) -> String {
        let mut session = self.session(message);
        change(&mut session);
        let committed = session.commit().unwrap();
        session.finish();
        committed.step.expect("a step")
    }

    fn tag(&self, key: &str, tags: &[&str]) -> String {
        let tags: Vec<String> = tags.iter().map(|t| t.to_string()).collect();
        self.run(&format!("papers tag {key} --add {}", tags.join(" ")), |s| {
            add_remove(&mut s.library, &[key.to_string()], &tags, &[]);
        })
    }

    fn untag(&self, key: &str, tag: &str) -> String {
        self.run(&format!("papers tag {key} --rm {tag}"), |s| {
            add_remove(&mut s.library, &[key.to_string()], &[], &[tag.to_string()]);
        })
    }

    fn set(&self, key: &str, field: &str, value: &str) -> String {
        self.run(&format!("papers edit {key} --set {field}={value}"), |s| {
            s.library.get_mut(key).unwrap().set(field, value);
        })
    }

    /// `papers history --revert N`: the lines it prints, and the step it records (none if nothing changed).
    fn revert(&self, n: &str, resolver: &mut dyn Resolve) -> Result<(Vec<String>, Option<String>)> {
        let mut session = self.session(&format!("papers history --revert {n}"));
        let result = session
            .revert(n, resolver, &Revert::default())
            .and_then(|(step, lines)| {
                session.mark_reverts(&[step]);
                let committed = session.commit()?;
                Ok((lines, committed.step))
            });
        session.finish();
        result
    }

    fn field(&self, key: &str, name: &str) -> Option<String> {
        let text = fs::read_to_string(self.path("lib/papers.bib")).unwrap();
        let bib = papers_rs::bib::Bib::parse(&text);
        let e = bib.entries().find(|e| e.key == key)?;
        e.get(name).map(str::to_string)
    }

    fn keys(&self) -> Vec<String> {
        let text = fs::read_to_string(self.path("lib/papers.bib")).unwrap();
        papers_rs::bib::Bib::parse(&text)
            .entries()
            .map(|e| e.key.clone())
            .collect()
    }

    fn history(&self) -> History {
        let mut config = self.config();
        History::open(&mut config, false, false).unwrap().unwrap()
    }

    fn head(&self) -> String {
        self.history().head().unwrap().state
    }

    /// The identifier of the n-th state (from 1) by time: what papers 3.0.0b5 numbered n.
    fn nth(&self, n: usize) -> String {
        let history = self.history();
        let mut states: Vec<_> = history.project.states().iter().collect();
        states.sort_by(|a, b| a.1.time().total_cmp(&b.1.time()));
        states[n - 1].0.clone()
    }

    fn states(&self) -> usize {
        self.history().project.states().len()
    }

    fn undo(&self) {
        let mut config = self.config();
        History::require(&mut config, false)
            .unwrap()
            .undo(
                1,
                false,
                None,
                "papers undo",
                &mut Resolver::new(OnDuplicate::Skip),
            )
            .unwrap();
    }
}

fn mode(mode: OnDuplicate) -> Resolver {
    Resolver::new(mode)
}

#[test]
fn tags_one_by_one() {
    let s = Sandbox::new(&["one2000", "two2001"]);
    let n = s.tag("one2000", &["climate"]);
    s.tag("one2000", &["later"]);
    s.tag("two2001", &["ice"]);
    let (lines, step) = s.revert(&n, &mut mode(OnDuplicate::Error)).unwrap();
    assert!(lines.is_empty());
    assert_eq!(s.field("one2000", "keywords").as_deref(), Some("later"));
    assert_eq!(s.field("two2001", "keywords").as_deref(), Some("ice"));
    // a new step, which says what it reverts; undo takes it back
    let step = step.expect("a step");
    let history = s.history();
    assert_eq!(history.project.state(&step).unwrap().reverts(), [n]);
    s.undo();
    assert_eq!(
        s.field("one2000", "keywords").as_deref(),
        Some("climate, later")
    );
}

#[test]
fn a_tag_that_was_removed_comes_back() {
    let s = Sandbox::new(&["one2000"]);
    s.tag("one2000", &["a", "b"]);
    let n = s.untag("one2000", "a");
    s.tag("one2000", &["c"]);
    s.revert(&n, &mut mode(OnDuplicate::Error)).unwrap();
    assert_eq!(s.field("one2000", "keywords").as_deref(), Some("b, c, a"));
}

#[test]
fn a_field_goes_back() {
    let s = Sandbox::new(&["one2000", "two2001"]);
    let n = s.set("one2000", "journal", "Nature");
    s.set("two2001", "journal", "Science");
    s.revert(&n, &mut mode(OnDuplicate::Error)).unwrap();
    assert_eq!(s.field("one2000", "journal"), None);
    assert_eq!(s.field("two2001", "journal").as_deref(), Some("Science"));
}

#[test]
fn a_conflict_is_decided_as_in_pull() {
    let s = Sandbox::new(&["one2000"]);
    let n = s.set("one2000", "journal", "Nature");
    s.set("one2000", "journal", "Science");
    let states = s.states();
    // an error without a terminal, and nothing is saved
    let error = s.revert(&n, &mut mode(OnDuplicate::Error)).unwrap_err();
    assert!(error.message().starts_with(&format!("one2000: changed since step {} too (1 conflict)\nPass --on-duplicate with one of: ask, keep, update, skip", short(&n))));
    assert!(error.message().ends_with("see `papers history --help`."));
    assert_eq!(s.states(), states);
    // keep: the values now; update: the values before the step
    s.revert(&n, &mut mode(OnDuplicate::Keep)).unwrap();
    assert_eq!(s.field("one2000", "journal").as_deref(), Some("Science"));
    s.revert(&n, &mut mode(OnDuplicate::Update)).unwrap();
    assert_eq!(s.field("one2000", "journal"), None);
}

#[test]
fn skip_leaves_the_entry_as_it_is() {
    let s = Sandbox::new(&["one2000"]);
    let n = s.set("one2000", "journal", "Nature");
    s.set("one2000", "journal", "Science");
    let states = s.states();
    let (lines, step) = s.revert(&n, &mut mode(OnDuplicate::Skip)).unwrap();
    assert_eq!(
        lines,
        [format!(
            "one2000: changed since step {} too: left as it is",
            short(&n)
        )]
    );
    assert_eq!(step, None);
    assert_eq!(s.states(), states);
    assert_eq!(s.field("one2000", "journal").as_deref(), Some("Science"));
}

#[test]
fn an_added_entry_is_removed_unless_it_changed_since() {
    let s = Sandbox::new(&["one2000"]);
    let n = s.run("papers add b.bib", |session| {
        session
            .library
            .add_unchecked(papers_rs::bib::Entry::new("article", "three2002"));
        session
            .library
            .get_mut("three2002")
            .unwrap()
            .set("title", "Three");
    });
    s.tag("one2000", &["later"]);
    let (lines, _) = s.revert(&n, &mut mode(OnDuplicate::Error)).unwrap();
    assert!(lines.is_empty());
    assert_eq!(s.keys(), ["one2000"]);
    assert_eq!(s.field("one2000", "keywords").as_deref(), Some("later"));
    // changed since: kept, with a line
    let n = s.run("papers add b.bib", |session| {
        session
            .library
            .add_unchecked(papers_rs::bib::Entry::new("article", "three2002"));
    });
    s.tag("three2002", &["later"]);
    let (lines, _) = s.revert(&n, &mut mode(OnDuplicate::Error)).unwrap();
    assert_eq!(
        lines,
        [format!(
            "three2002: added by step {} and changed since: kept (`papers rm three2002` removes it)",
            short(&n)
        )]
    );
    assert!(s.keys().contains(&"three2002".to_string()));
}

#[test]
fn a_removed_entry_comes_back() {
    let s = Sandbox::new(&["one2000", "two2001"]);
    let n = s.run("papers rm two2001", |session| {
        session.library.remove("two2001");
    });
    s.tag("one2000", &["later"]);
    s.revert(&n, &mut mode(OnDuplicate::Error)).unwrap();
    assert!(s.keys().contains(&"two2001".to_string()));
    assert_eq!(s.field("one2000", "keywords").as_deref(), Some("later"));
}

#[test]
fn the_key_goes_back_and_the_former_key_leaves_ids() {
    let s = Sandbox::new(&["one2000", "two2001"]);
    let n = s.run(
        "papers edit one2000 --key first2000 --keep-old-key",
        |session| {
            session
                .library
                .set_key("one2000", "first2000", true)
                .unwrap();
        },
    );
    s.tag("first2000", &["later"]);
    s.revert(&n, &mut mode(OnDuplicate::Error)).unwrap();
    assert!(
        s.keys().contains(&"one2000".to_string()) && !s.keys().contains(&"first2000".to_string())
    );
    assert_eq!(s.field("one2000", "ids"), None);
    assert_eq!(s.field("one2000", "keywords").as_deref(), Some("later"));
}

#[test]
fn the_pairs_of_distinct_json() {
    let s = Sandbox::new(&["one2000", "two2001"]);
    let distinct = s.path("lib/.papers/distinct.json");
    s.tag("two2001", &["first"]); // the history starts before the pair is written
    s.write(
        "lib/.papers/distinct.json",
        &json!({"distinct": [[{"authortitle": "one"}, {"authortitle": "two"}]]}).to_string(),
    );
    let n = s.tag("one2000", &["a"]); // the step has the pair
    s.tag("two2001", &["b"]);
    s.revert(&n, &mut mode(OnDuplicate::Error)).unwrap();
    let text = fs::read_to_string(&distinct).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).unwrap()["distinct"],
        json!([])
    );
}

#[test]
fn the_errors() {
    let s = Sandbox::new(&["one2000"]);
    let first = s.tag("one2000", &["a"]);
    let message = |n: &str| {
        s.revert(n, &mut mode(OnDuplicate::Error))
            .unwrap_err()
            .message()
            .to_string()
    };
    // the first step has nothing before it
    let start = s.nth(1);
    assert_eq!(
        message(&start),
        format!(
            "step {} is the first step of the history: nothing before it to go back to",
            short(&start)
        )
    );
    // a start of the identifier of 4 characters is enough, 3 are not
    assert_eq!(message(&start[..4].to_uppercase()), message(&start));
    assert_eq!(
        message(&start[..3]),
        format!(
            "no step {} in the history of the library (see `papers history`)",
            &start[..3]
        )
    );
    assert_eq!(
        message("99"),
        "no step 99 in the history of the library (see `papers history`)"
    );
    assert_eq!(
        message("-1"),
        "no step -1 in the history of the library (see `papers history`)"
    );
    // a step that is no longer in the current history (after an undo and another change)
    s.undo();
    s.tag("one2000", &["b"]);
    assert_eq!(
        message(&first),
        format!(
            "step {} is not in the current history of the library: `papers restore {}` goes there",
            short(&first),
            short(&first)
        )
    );
}

#[test]
fn a_dry_run_changes_nothing() {
    let s = Sandbox::new(&["one2000"]);
    let n = s.tag("one2000", &["a"]);
    let mut session = Session::open(
        s.config(),
        Options {
            message: "papers history --revert".into(),
            dry_run: true,
            ..Options::default()
        },
    )
    .unwrap();
    session
        .revert(&n, &mut mode(OnDuplicate::Error), &Revert::default())
        .unwrap();
    session.commit().unwrap();
    session.finish();
    assert_eq!(s.field("one2000", "keywords").as_deref(), Some("a"));
    assert_eq!(s.head(), n);
}

#[test]
fn the_files_go_back_as_undo_moves_them() {
    let s = Sandbox::new(&["one2000"]);
    s.write("downloads/three.pdf", "pdf three");
    let n = s.run("papers add b.bib --move", |session| {
        let mut e = papers_rs::bib::Entry::new("article", "three2002");
        e.set("title", "Three");
        session.library.add_unchecked(e);
        // as `add --move` does: the file goes to the files directory, and the entry links to it
        let dst = session.library.filesdir.clone().unwrap() + "/three.pdf";
        fs::rename(s.path("downloads/three.pdf"), &dst).unwrap();
        session
            .library
            .set_files("three2002", std::slice::from_ref(&dst))
            .unwrap();
        session.record_operation(
            papers_rs::history::OpKind::Moved,
            &s.path("downloads/three.pdf"),
            &dst,
        );
    });
    s.tag("one2000", &["later"]);
    let (lines, _) = s.revert(&n, &mut mode(OnDuplicate::Error)).unwrap();
    assert!(
        lines.iter().any(|l| l.contains("moved back to")),
        "{lines:?}"
    );
    assert_eq!(
        fs::read_to_string(s.path("downloads/three.pdf")).unwrap(),
        "pdf three"
    );
    assert!(!Path::new(&s.path("lib/files/three.pdf")).exists());
    // undo of the revert: the file goes where the step had put it
    s.undo();
    assert!(!Path::new(&s.path("downloads/three.pdf")).exists());
    assert_eq!(
        fs::read_to_string(s.path("lib/files/three.pdf")).unwrap(),
        "pdf three"
    );
}

#[test]
fn nothing_to_take_back_records_no_step() {
    let s = Sandbox::new(&["one2000"]);
    let n = s.tag("one2000", &["x"]);
    s.untag("one2000", "x");
    let head = s.head();
    let mut session = s.session("papers history --revert 2");
    session
        .revert(&n, &mut mode(OnDuplicate::Error), &Revert::default())
        .unwrap();
    let committed = session.commit().unwrap();
    session.finish();
    assert_eq!((committed.saved, committed.step), (false, None));
    assert_eq!(s.head(), head);
}
