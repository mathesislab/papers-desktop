//! The rules of `design/history.md` (3.0.0b6) about the identifier of a state, the upgrade of numbered states, what
//! is recorded and the retention: the tests of papers' `tests/test_spec_history.py` and `test_history.py` for them,
//! ported to `papers_rs::history`. Each test names its section of the specification.

use std::fs;
use std::path::Path;

use papers_rs::command::{Options, Session};
use papers_rs::config::{Config, MachineConfig};
use papers_rs::history::ojson::J;
use papers_rs::history::project::{NewState, Text, is_state_id, short, state_id};
use papers_rs::history::store::{GRACE, Store, now, sha256};
use papers_rs::history::{FileOp, History, OpKind, PoolLink, State, Step};
use serde_json::{Map, Value, json};

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: String,
    machine: Map<String, Value>,
}

fn entry(key: &str) -> String {
    format!(
        "@article{{{key},\n title = {{Title of {key}}},\n author = {{Doe, Jane}},\n year = {{2000}},\n}}\n\n"
    )
}

impl Sandbox {
    fn new() -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let mut machine = Map::new();
        machine.insert("history-dir".into(), json!(format!("{root}/history")));
        Sandbox {
            _tmp: tmp,
            root,
            machine,
        }
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
        fs::read_to_string(self.path(rel)).unwrap()
    }

    fn config(&self, dir: &str) -> Config {
        Config::load(
            &self.path(&format!("{dir}/.papers/config.json")),
            MachineConfig::new(None, self.machine.clone()),
            None,
        )
        .unwrap()
    }

    /// A library as `papers init` makes it: no identifier yet.
    fn init(&self, dir: &str) -> Config {
        self.write(
            &format!("{dir}/.papers/config.json"),
            "{\n  \"bibtex\": \"papers.bib\",\n  \"filesdir\": \"files\",\n  \"pool\": \"none\",\n  \"version\": 3\n}",
        );
        self.write(&format!("{dir}/papers.bib"), "");
        self.config(dir)
    }

    /// A command that adds an entry: the start, the head, the write, the step.
    fn add(&self, config: &mut Config, key: &str) -> History {
        let before = History::before_command(config);
        let mut history = History::open(config, true, false).unwrap().unwrap();
        history.sync_head(before.as_ref()).unwrap();
        let bibtex = config.bibtex.clone().unwrap();
        let text = fs::read_to_string(&bibtex).unwrap() + &entry(key);
        fs::write(&bibtex, text).unwrap();
        history
            .record(Step::new(format!("papers add {key}.bib")))
            .unwrap();
        history
    }
}

fn open(config: &mut Config) -> History {
    History::open(config, false, false).unwrap().unwrap()
}

fn skip() -> papers_rs::merge::Resolver {
    papers_rs::merge::Resolver::new(papers_rs::merge::OnDuplicate::Skip)
}

fn undo(config: &mut Config, steps: usize) -> papers_rs::error::Result<papers_rs::history::Moved> {
    History::require(config, false)
        .unwrap()
        .undo(steps, false, None, "papers undo", &mut skip())
}

fn state(parent: Option<&str>, time: f64, text: &str, store: &Store) -> State {
    State::new(NewState {
        parent: parent.map(str::to_string),
        time,
        kind: "command".into(),
        message: "papers x".into(),
        ops: vec![],
        deleted: vec![],
        bibtex: Text {
            pieces: store.put_text(text).unwrap(),
            sha: sha256(text.as_bytes()),
        },
        files: vec![],
    })
}

// 2.5 States and their identifiers
// ================================

#[test]
fn a_state_is_written_once_under_its_identifier() {
    // history.md 2.5: created exclusively; the same state (same identifier) is not written again
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(tmp.path().to_string_lossy().into_owned());
    let mut p = store.project("p").unwrap();
    let mut first = state(None, 1.0, "", &store);
    first.set_message("first");
    let n = p.add(first).unwrap();
    let mut second = state(None, 1.0, "", &store);
    second.set_message("second");
    assert_eq!(p.add(second).unwrap(), n);
    let text = fs::read_to_string(format!("{}/{n}.json", p.states_dir())).unwrap();
    assert_eq!(
        J::parse(&text).unwrap().get("message").unwrap().as_str(),
        Some("first")
    );
    // a state is a JSON object in UTF-8 with \n line endings, and its identifier is that of what it is
    assert!(!text.contains('\r'));
    assert_eq!(n, state_id(p.state(&n).unwrap()));
    assert!(is_state_id(&n));
}

#[test]
fn only_the_files_named_by_an_identifier_holding_an_object_are_states() {
    // history.md 2.5
    let s = Sandbox::new();
    let mut config = s.init("lib");
    s.add(&mut config, "one2000");
    let history = open(&mut config);
    let folder = history.project.states_dir().to_string();
    let states: Vec<String> = history.project.states().keys().cloned().collect();
    fs::write(format!("{folder}/notes.json"), "{}").unwrap();
    fs::write(format!("{folder}/{}.json", "a".repeat(64)), "[1, 2]").unwrap();
    fs::write(format!("{folder}/{}.json.tmp", "b".repeat(64)), "{}").unwrap();
    fs::write(format!("{folder}/{}.json", "C".repeat(64)), "{}").unwrap();
    let reread = open(&mut config);
    assert_eq!(
        reread.project.states().keys().cloned().collect::<Vec<_>>(),
        states
    );
}

#[test]
fn a_state_without_kind_is_read() {
    // history.md 3.1: `kind` is optional for a reader (papers writes it in each state it makes)
    let s = Sandbox::new();
    let mut config = s.init("lib");
    s.add(&mut config, "one2000");
    s.add(&mut config, "two2000");
    let history = open(&mut config);
    for n in history.project.states().keys() {
        let file = format!("{}/{n}.json", history.project.states_dir());
        let text = fs::read_to_string(&file).unwrap();
        let start = text.find("\"kind\"").unwrap();
        let end = start + text[start..].find(",\n").unwrap() + 2;
        fs::write(
            &file,
            format!(
                "{}{}",
                text[..start].trim_end_matches(' '),
                text[end..].trim_start_matches(' ')
            ),
        )
        .unwrap();
    }
    let mut history = open(&mut config);
    assert!(
        history
            .project
            .states()
            .values()
            .all(|state| state.kind().is_empty())
    );
    history.sync_head(None).unwrap();
    assert_eq!(
        history.project.line(&history.head().unwrap().state).len(),
        3
    );
    undo(&mut config, 2).unwrap();
    assert_eq!(s.read("lib/papers.bib"), "");
}

#[test]
fn a_step_is_named_by_a_start_of_its_identifier() {
    // history.md 2.5: 4 characters or more, without case, spaces dropped; several is refused
    let s = Sandbox::new();
    let mut config = s.init("lib");
    s.add(&mut config, "one2000");
    s.add(&mut config, "two2000");
    let first = {
        let h = open(&mut config);
        let head = h.head().unwrap().state;
        h.project.state(&head).unwrap().parent().unwrap()
    };
    let restore = |config: &mut Config, value: &str| {
        History::require(config, false)
            .unwrap()
            .restore(Some(value), None)
    };
    restore(&mut config, &first[..4].to_uppercase()).unwrap();
    assert_eq!(open(&mut config).head().unwrap().state, first);
    let two = open(&mut config)
        .head()
        .unwrap()
        .redo
        .last()
        .cloned()
        .unwrap();
    restore(&mut config, &format!(" {} ", &two[..6])).unwrap();
    assert_eq!(open(&mut config).head().unwrap().state, two);
    for value in [&two[..3], "zzzzzz", &format!("{}x", &two[..4])] {
        assert_eq!(
            restore(&mut config, value).unwrap_err().message(),
            format!("no step {value} in the history of the library (see `papers history`)")
        );
    }
    // two states that start alike
    let folder = open(&mut config).project.states_dir().to_string();
    for name in ["abcd1", "abcd2"] {
        fs::write(
            format!("{folder}/{name}{}.json", "0".repeat(59)),
            json!({"parent": null, "time": 1, "bibtex": {"pieces": [], "sha": sha256(b"")}, "files": {}})
                .to_string(),
        )
        .unwrap();
    }
    assert_eq!(
        restore(&mut config, "ABCD").unwrap_err().message(),
        "abcd names several steps: abcd1000, abcd2000"
    );
}

#[test]
fn the_upgrade_of_numbered_states_keeps_numbers_json() {
    // history.md 2.5: the states are named by their identifier, the references rewritten, numbers.json written
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(tmp.path().to_string_lossy().into_owned());
    let folder = format!("{}/projects/p/states", tmp.path().display());
    fs::create_dir_all(&folder).unwrap();
    let bibtex = |sha: &str| json!({"pieces": [], "sha": sha});
    fs::write(
        format!("{folder}/000001.json"),
        json!({"parent": null, "time": 1, "bibtex": bibtex("a")}).to_string(),
    )
    .unwrap();
    fs::write(
        format!("{folder}/000002.json"),
        json!({"parent": 1, "time": 2, "bibtex": bibtex("b"), "reverts": [1, 7]}).to_string(),
    )
    .unwrap();
    fs::write(
        format!("{folder}/000003.json"),
        json!({"parent": 2, "time": 3, "bibtex": bibtex("c")}).to_string(),
    )
    .unwrap();
    fs::write(
        format!("{}/projects/p/heads.json", tmp.path().display()),
        json!({"/x.bib": {"state": 3, "redo": [2], "time": 5}}).to_string(),
    )
    .unwrap();
    let project = store.project("p").unwrap();
    let states = project.states();
    let one = states
        .iter()
        .find(|(_, s)| s.parent().is_none())
        .unwrap()
        .0
        .clone();
    let two = states
        .iter()
        .find(|(_, s)| s.parent() == Some(one.clone()))
        .unwrap()
        .0
        .clone();
    let three = states
        .iter()
        .find(|(_, s)| s.parent() == Some(two.clone()))
        .unwrap()
        .0
        .clone();
    assert_eq!(
        J::parse(&fs::read_to_string(format!("{}/numbers.json", tmp.path().display())).unwrap())
            .unwrap(),
        J::parse(&json!({"p": {"1": one, "2": two, "3": three}}).to_string()).unwrap()
    );
    let mut names: Vec<String> = fs::read_dir(&folder)
        .unwrap()
        .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut want = vec![
        format!("{one}.json"),
        format!("{two}.json"),
        format!("{three}.json"),
    ];
    want.sort();
    assert_eq!(names, want);
    // the references: a number that no longer exists is dropped from `reverts`, the heads are rewritten
    assert_eq!(states[&two].reverts(), vec![one.clone()]);
    let head = project.head("/x.bib").unwrap();
    assert_eq!((head.state, head.redo), (three, vec![two]));
}

#[test]
fn a_link_that_keeps_a_number_is_looked_up_in_the_store_of_the_pool() {
    // history.md 2.5 and 7.2: the pool is in another store; its upgrade gives the number its identifier
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(tmp.path().to_string_lossy().into_owned());
    let folder = format!("{}/projects/pool/states", tmp.path().display());
    fs::create_dir_all(&folder).unwrap();
    for (n, parent) in [(1, json!(null)), (2, json!(1))] {
        fs::write(
            format!("{folder}/{n:06}.json"),
            json!({"parent": parent, "time": n, "bibtex": {"pieces": [], "sha": n.to_string()}})
                .to_string(),
        )
        .unwrap();
    }
    let pool = store.project("pool").unwrap();
    let second = pool
        .states()
        .iter()
        .find(|(_, s)| s.parent().is_some())
        .unwrap()
        .0
        .clone();
    let link = J::parse(r#"{"pools": {"pool": {"state": 2, "config": "/c"}}}"#).unwrap();
    let pools = State(link).pools();
    assert_eq!(
        pool.upgraded(&pools[0].1.state).as_deref(),
        Some(second.as_str())
    );
    // an identifier is its own, a number that no upgrade knows is none
    assert_eq!(
        pool.upgraded(&papers_rs::history::LinkedState::Number(9)),
        None
    );
}

// 5. Recording
// ============

#[test]
fn the_outside_state_has_the_kept_files_as_they_are_on_disk() {
    // history.md 5.2: the files of `.papers/` as they were at the start of the command
    let s = Sandbox::new();
    let mut config = s.init("lib");
    s.add(&mut config, "one2000");
    s.write("lib/.papers/distinct.json", "{\"distinct\": []}\n");
    let bibtex = config.bibtex.clone().unwrap();
    fs::write(
        &bibtex,
        fs::read_to_string(&bibtex).unwrap() + &entry("two2000"),
    )
    .unwrap();
    s.add(&mut config, "three2000");
    let history = open(&mut config);
    let head = history.head().unwrap().state;
    let outside = history
        .project
        .state(&history.project.state(&head).unwrap().parent().unwrap())
        .unwrap();
    assert_eq!(outside.kind(), "outside");
    assert!(outside.file(".papers/distinct.json").is_some());
    assert!(Path::new(&s.path("lib/.papers/distinct.json")).exists());
}

#[test]
fn the_first_state_is_the_library_before_the_command() {
    // history.md 5.3: also when the command wrote `.papers/` before its save
    let s = Sandbox::new();
    s.init("lib");
    s.write("lib/papers.bib", &entry("one2000"));
    let mut config = s.config("lib");
    let before = History::before_command(&mut config);
    s.write("lib/.papers/distinct.json", "{\"distinct\": []}\n"); // before the save
    let mut history = History::open(&mut config, true, false).unwrap().unwrap();
    history.sync_head(before.as_ref()).unwrap();
    fs::write(
        config.bibtex.clone().unwrap(),
        entry("one2000").replace("}\n\n", " note = {changed},\n}\n\n"),
    )
    .unwrap();
    history.record(Step::new("papers tag x")).unwrap();
    let history = open(&mut config);
    let head = history.head().unwrap().state;
    let state = history.project.state(&head).unwrap();
    let start = history.project.state(&state.parent().unwrap()).unwrap();
    assert_eq!(start.kind(), "start");
    assert_eq!(start.files().unwrap(), vec![]);
    assert!(state.file(".papers/distinct.json").is_some());
    assert_eq!(
        history.store.text(&start.bibtex_pieces()).unwrap(),
        entry("one2000")
    );
}

#[test]
fn a_command_after_an_undo_starts_a_branch() {
    // history.md 5.4 rule 7: the redo list is empty, the undone state stays
    let s = Sandbox::new();
    let mut config = s.init("lib");
    s.add(&mut config, "one2000");
    s.add(&mut config, "two2000");
    let undone = open(&mut config).head().unwrap().state;
    undo(&mut config, 1).unwrap();
    s.add(&mut config, "three2000");
    let history = open(&mut config);
    let head = history.head().unwrap();
    assert!(head.redo.is_empty());
    assert!(history.project.state(&undone).is_some());
    assert!(!history.project.line(&head.state).contains(&undone));
}

#[test]
fn operations_after_a_save_that_added_no_state_make_a_state_of_their_own() {
    // history.md 5.5: never joining the state of an earlier command
    let s = Sandbox::new();
    let mut config = s.init("lib");
    s.add(&mut config, "one2000");
    let (before, previous_ops) = {
        let h = open(&mut config);
        let head = h.head().unwrap().state;
        let ops = h.project.state(&head).unwrap().ops();
        (head, ops)
    };
    s.write("lib/files/stray.pdf", "pdf a");
    let mut session = Session::open(
        s.config("lib"),
        Options {
            message: "papers file checksum".into(),
            ..Options::default()
        },
    )
    .unwrap();
    session.commit_other_changes().unwrap(); // nothing changed: no state
    session.remove_file(&s.path("lib/files/stray.pdf")).unwrap();
    session.finish();
    let history = open(&mut config);
    let after = history.head().unwrap().state;
    assert_ne!(after, before);
    assert_eq!(history.project.state(&before).unwrap().ops(), previous_ops);
    let ops = history.project.state(&after).unwrap().ops();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].kind, OpKind::Trashed);
    assert_eq!(
        history.project.state(&after).unwrap().parent(),
        Some(before)
    );
    // undo brings the file back
    undo(&mut config, 1).unwrap();
    assert_eq!(s.read("lib/files/stray.pdf"), "pdf a");
}

#[test]
fn a_file_brought_back_from_the_history_is_kept_by_the_step_that_copied_it() {
    // history.md 3.4 and 6.4 (pass 3): `deleted` lists the file object, undo removes the copy, redo copies it again
    let s = Sandbox::new();
    let mut config = s.init("lib");
    s.write("lib/files/a.pdf", "pdf a");
    let digest = sha256(b"pdf a");
    let entry_text = entry("one2000").replace("}\n\n", " file = {:files/a.pdf:pdf},\n}\n\n");
    s.write("lib/papers.bib", &entry_text);
    let before = History::before_command(&mut config);
    let mut history = History::open(&mut config, true, false).unwrap().unwrap();
    history.sync_head(before.as_ref()).unwrap();
    // the file is deleted by a command (kept), the link stays
    let stored = history
        .store
        .put_file(&s.path("lib/files/a.pdf"), &digest)
        .unwrap();
    history
        .record(Step {
            message: "papers file rm".into(),
            ops: vec![FileOp::new(
                OpKind::Trashed,
                &s.path("lib/files/a.pdf"),
                None,
                Some(digest.clone()),
            )],
            ..Step::default()
        })
        .unwrap();
    // `papers file fix`: the file object is copied to the place of the link
    fs::copy(&stored, s.path("lib/files/a.pdf")).unwrap();
    let rec = history
        .record(Step {
            message: "papers file fix".into(),
            ops: vec![FileOp::new(
                OpKind::Copied,
                &stored,
                Some(&s.path("lib/files/a.pdf")),
                Some(digest.clone()),
            )],
            ..Step::default()
        })
        .unwrap();
    assert!(rec.added, "the operations are never skipped as no change");
    let history = open(&mut config);
    let head = history.head().unwrap().state;
    let state = history.project.state(&head).unwrap();
    assert_eq!(state.deleted(), vec![digest.clone()]);
    assert_eq!(state.ops()[0].src, stored);
    // undo removes the copy (the file object stays), redo copies it again
    let moved = undo(&mut config, 1).unwrap();
    assert!(
        moved
            .files
            .unwrap()
            .iter()
            .any(|l| l.contains("files/a.pdf: removed (a copy of ")),
        "undo removes the copy"
    );
    assert!(!Path::new(&s.path("lib/files/a.pdf")).exists());
    assert!(Path::new(&stored).exists());
    let moved = History::require(&mut config, false)
        .unwrap()
        .redo(1, false, None, "papers redo", &mut skip())
        .unwrap();
    assert!(
        moved
            .files
            .unwrap()
            .iter()
            .any(|l| l.contains("--> copied to"))
    );
    assert_eq!(s.read("lib/files/a.pdf"), "pdf a");
}

// 6. Undo, redo, restore
// ======================

/// The history for undo, redo and restore, as the commands open it: no identifier is made.
fn travel(config: &mut Config) -> History {
    History::for_travel(config, false).unwrap()
}

fn copy_folder(from: &str, to: &str) {
    fs::create_dir_all(to).unwrap();
    for item in fs::read_dir(from).unwrap() {
        let item = item.unwrap();
        let target = format!("{to}/{}", item.file_name().to_string_lossy());
        if item.path().is_dir() {
            copy_folder(&item.path().to_string_lossy(), &target);
        } else {
            fs::copy(item.path(), target).unwrap();
        }
    }
}

#[test]
fn nothing_to_do_leaves_no_trace() {
    // history.md 6.2, 5.3 (test_spec_history.py::TestCommands::test_nothing_to_do_leaves_no_trace):
    // undo, redo and restore with nothing to do write no id, no state, no store folder
    let s = Sandbox::new();
    let mut config = s.init("lib");
    s.write("lib/papers.bib", &entry("one2000"));
    let mut errors = Vec::new();
    for steps in [1, 2] {
        errors.push(
            travel(&mut config)
                .undo(steps, false, None, "papers undo", &mut skip())
                .unwrap_err(),
        );
    }
    errors.push(
        travel(&mut config)
            .redo(1, false, None, "papers redo", &mut skip())
            .unwrap_err(),
    );
    errors.push(travel(&mut config).restore(None, None).unwrap_err());
    errors.push(travel(&mut config).restore(Some("abcd"), None).unwrap_err());
    let messages: Vec<&str> = errors.iter().map(|e| e.message()).collect();
    assert_eq!(
        messages,
        [
            "nothing to undo",
            "nothing to undo",
            "nothing to redo",
            "no history: nothing to restore",
            "no step abcd in the history of the library (see `papers history`)",
        ]
    );
    assert!(!s.read("lib/.papers/config.json").contains("\"id\""));
    assert!(config.id.is_none());
    assert!(!Path::new(&s.path("history")).exists());
    // a copy, never used in its folder, at the step it was copied at: no head written
    let mut lib = s.config("lib");
    s.add(&mut lib, "two2000");
    copy_folder(&s.path("lib"), &s.path("copy"));
    let mut copy = s.config("copy");
    let head = open(&mut lib).head().unwrap().state;
    let error = travel(&mut copy).restore(Some(&head), None).unwrap_err();
    assert!(error.message().starts_with("nothing to restore"), "{error}");
    let error = travel(&mut copy)
        .redo(1, false, None, "papers redo", &mut skip())
        .unwrap_err();
    assert_eq!(error.message(), "nothing to redo");
    let heads = open(&mut lib).project.heads();
    assert_eq!(heads.len(), 1);
    assert_eq!(heads[0].place, travel(&mut lib).place);
}

#[test]
fn a_change_made_outside_papers_counts_as_a_step_to_undo() {
    // history.md 6.2: the errors are decided as if the head were brought up to date, and nothing is written
    let s = Sandbox::new();
    let mut config = s.init("lib");
    s.add(&mut config, "one2000");
    s.add(&mut config, "two2000");
    let states = open(&mut config).project.states().len();
    s.write(
        "lib/papers.bib",
        &(s.read("lib/papers.bib") + &entry("three2000")),
    );
    // three steps and the change made outside: 3 steps to undo, not 4
    let error = travel(&mut config)
        .undo(4, false, None, "papers undo", &mut skip())
        .unwrap_err();
    assert_eq!(error.message(), "only 3 step(s) to undo");
    assert_eq!(open(&mut config).project.states().len(), states);
    // within the steps: the change is recorded, then undone
    travel(&mut config)
        .undo(3, false, None, "papers undo", &mut skip())
        .unwrap();
    assert_eq!(open(&mut config).project.states().len(), states + 1);
    assert_eq!(s.read("lib/papers.bib"), "");
}

// 7. Undo and redo with a pool
// ============================

fn pool_and_library(s: &Sandbox) -> (Config, Config) {
    let pool = s.init("pool");
    let lib = s.init("lib");
    (pool, lib)
}

/// A command of the library that writes its pool: the pool's step (`from`), then the step of the library
/// with the link. Returns the link.
fn command_with_pool(
    s: &Sandbox,
    lib: &mut Config,
    pool: &mut Config,
    saves: usize,
    links: Vec<(String, PoolLink)>,
) -> (Vec<(String, PoolLink)>, History) {
    let before = History::before_command(lib);
    let mut history = History::open(lib, true, false).unwrap().unwrap();
    history.sync_head(before.as_ref()).unwrap();
    let mut links = links;
    for _ in 0..saves {
        let before = History::before_command(pool);
        let mut pool_history = History::open(pool, true, false).unwrap().unwrap();
        pool_history.sync_head(before.as_ref()).unwrap();
        let bibtex = pool.bibtex.clone().unwrap();
        let text = fs::read_to_string(&bibtex).unwrap();
        // a key of its own (the command may be the second of the test)
        let key = format!("pool{}", text.matches("{pool").count());
        fs::write(&bibtex, text + &entry(&key)).unwrap();
        let rec = pool_history
            .record(Step {
                message: "papers sync".into(),
                from: Some(s.path("lib")),
                ..Step::default()
            })
            .unwrap();
        let (id, mut link) = rec.link.unwrap();
        // a pool saved several times: the last state, and the state before the first save
        if let Some((_, first)) = links.iter().find(|(i, _)| *i == id) {
            link.base = first.base.clone();
        }
        links.retain(|(i, _)| *i != id);
        links.push((id, link));
    }
    let bibtex = lib.bibtex.clone().unwrap();
    let text = fs::read_to_string(&bibtex).unwrap();
    let key = format!("lib{}", text.matches("{lib").count());
    fs::write(&bibtex, text + &entry(&key)).unwrap();
    history
        .record(Step {
            message: "papers sync".into(),
            pools: links.clone(),
            ..Step::default()
        })
        .unwrap();
    (links, history)
}

#[test]
fn the_link_to_a_pool_step_has_its_base() {
    // history.md 7.1 and 5.4: {pool id: {state, base, config}}; the pool's state has no ops and has `from`
    let s = Sandbox::new();
    let (mut pool, mut lib) = pool_and_library(&s);
    let (links, history) = command_with_pool(&s, &mut lib, &mut pool, 1, vec![]);
    let head = history.head().unwrap().state;
    let state = history.project.state(&head).unwrap();
    assert!(state.from().is_none());
    let pools = state.pools();
    assert_eq!(pools.len(), 1);
    assert_eq!(pools[0], links[0]);
    let pool_history = open(&mut pool);
    let pool_head = pool_history.head().unwrap().state;
    assert_eq!(
        pools[0].1.state,
        papers_rs::history::LinkedState::Id(pool_head.clone())
    );
    let pool_state = pool_history.project.state(&pool_head).unwrap();
    assert_eq!(
        pools[0].1.base,
        papers_rs::history::Base::Id(pool_state.parent().unwrap())
    );
    assert!(pool_state.ops().is_empty());
    assert_eq!(pool_state.from(), Some(s.path("lib").as_str()));
    assert_eq!(pool_state.message(), state.message());
    assert!(pool_state.pools().is_empty());
    // the text of the link: state, base, config, in that order
    let text = fs::read_to_string(format!("{}/{head}.json", history.project.states_dir())).unwrap();
    let link = J::parse(&text).unwrap();
    let link = link.get("pools").unwrap();
    let J::Obj(items) = link else { panic!("pools") };
    let J::Obj(keys) = &items[0].1 else {
        panic!("a link")
    };
    assert_eq!(
        keys.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        ["state", "base", "config"]
    );
}

#[test]
fn a_command_that_saved_the_pool_several_times_is_undone_in_full() {
    // history.md 7.2: the pool goes back to its state before the first save (`base`)
    let s = Sandbox::new();
    let (mut pool, mut lib) = pool_and_library(&s);
    let start = {
        // the pool's first state, before the command
        let before = History::before_command(&mut pool);
        let mut h = History::open(&mut pool, true, false).unwrap().unwrap();
        h.sync_head(before.as_ref()).unwrap();
        h.head().unwrap().state
    };
    let (links, _) = command_with_pool(&s, &mut lib, &mut pool, 2, vec![]);
    assert_eq!(links[0].1.base, papers_rs::history::Base::Id(start.clone()));
    assert_eq!(open(&mut pool).project.states().len(), 3);
    let moved = undo(&mut lib, 1).unwrap();
    assert!(
        moved.pools.iter().any(|l| l.contains("also undone")),
        "{:?}",
        moved.pools
    );
    // not to the state between the two saves
    let history = open(&mut pool);
    assert_eq!(history.head().unwrap().state, start);
    assert!(!s.read("pool/papers.bib").contains("pool0"));
    // redo does the pool's steps again
    History::require(&mut lib, false)
        .unwrap()
        .redo(1, false, None, "papers redo", &mut skip())
        .unwrap();
    assert!(s.read("pool/papers.bib").contains("pool1"));
}

#[test]
fn redo_of_several_steps_reverted_together_reverts_the_revert_once() {
    // history.md 7.3 (test_undo_pool.py::UndoInLibrary::test_redo_of_several_steps_reverted_together): the pool moved
    // on, undo -n 2 reverts the two steps of the pool in one state, redo -n 2 reverts that state once
    let s = Sandbox::new();
    let (mut pool, mut lib) = pool_and_library(&s);
    let (links, _) = command_with_pool(&s, &mut lib, &mut pool, 1, vec![]);
    command_with_pool(&s, &mut lib, &mut pool, 1, links);
    s.add(&mut pool, "other2023");
    let mut resolver = skip();
    travel(&mut lib)
        .undo(2, false, None, "papers undo -n 2", &mut resolver)
        .unwrap();
    let undo = open(&mut pool).head().unwrap().state;
    let removed = s.read("pool/papers.bib");
    assert!(
        !removed.contains("pool0") && !removed.contains("pool1") && removed.contains("other2023"),
        "{removed}"
    );
    travel(&mut lib)
        .redo(2, false, None, "papers redo -n 2", &mut resolver)
        .unwrap();
    let history = open(&mut pool);
    let head = history.head().unwrap().state;
    assert_eq!(history.project.state(&head).unwrap().reverts(), vec![undo]);
    let text = s.read("pool/papers.bib");
    assert!(
        text.contains("pool0") && text.contains("pool1") && text.contains("other2023"),
        "{text}"
    );
}

#[test]
fn a_pool_that_is_not_there_is_said_with_the_short_identifier() {
    // history.md 7.2
    let s = Sandbox::new();
    let (mut pool, mut lib) = pool_and_library(&s);
    let (links, _) = command_with_pool(&s, &mut lib, &mut pool, 1, vec![]);
    fs::rename(s.path("pool"), s.path("gone")).unwrap();
    let moved = undo(&mut lib, 1).unwrap();
    let papers_rs::history::LinkedState::Id(id) = &links[0].1.state else {
        panic!("an identifier")
    };
    assert!(
        moved.pools[0].starts_with(&format!("the pool of step {} is not found (at ", short(id))),
        "{:?}",
        moved.pools
    );
    assert!(moved.pools[0].ends_with("): the library only"));
    assert!(!s.read("lib/papers.bib").contains("lib2"));
}

// 9. Retention
// ============

#[test]
fn with_no_limit_no_state_is_dropped_and_the_removable_objects_go() {
    // history.md 9.2 and 9.1
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(tmp.path().to_string_lossy().into_owned());
    let mut p = store.project("p").unwrap();
    let old = p.add(state(None, 1.0, "@a{old,\n}\n", &store)).unwrap();
    let new = p
        .add(state(Some(&old), 2.0, "@a{new,\n}\n", &store))
        .unwrap();
    p.set_head("/x.bib", &new, &[]).unwrap();
    store.put_text("@a{orphan,\n}\n").unwrap();
    assert!(
        store
            .prune_at(0, 0, false, &|_| {}, now() + GRACE + 1.0)
            .is_empty()
    );
    assert!(store.project("p").unwrap().state(&old).is_some());
    assert_eq!(store.sizes().len(), 2);
}

#[test]
fn a_damaged_state_file_is_named_and_skipped() {
    // history.md 2.9 (test_history.py::TestPrune::test_damaged_state_skipped): a state file that is not
    // JSON is not a state, but it is named; the retention leaves it
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(tmp.path().to_string_lossy().into_owned());
    let mut p = store.project("p").unwrap();
    let old = p
        .add(state(None, 1.0, "@article{old,\n}\n", &store))
        .unwrap();
    let new = p
        .add(state(Some(&old), 2.0, "@article{new,\n}\n", &store))
        .unwrap();
    p.set_head("/lib.bib", &new, &[]).unwrap();
    let file = format!("{}/{old}.json", p.states_dir());
    fs::write(&file, "{\"parent\"").unwrap();
    let q = store.project("p").unwrap();
    assert_eq!(q.states().keys().collect::<Vec<_>>(), [&new]);
    assert_eq!(q.damaged().iter().collect::<Vec<_>>(), [&old]);
    assert_eq!(q.resolve(&old[..6]).unwrap(), Some(old.clone()));
    assert!(
        store
            .prune_at(1, 0, false, &|_| {}, now() + GRACE + 1.0)
            .is_empty()
    );
    assert!(Path::new(&file).exists());
}

#[test]
fn the_states_are_dropped_by_time_then_identifier_all_projects_together() {
    // history.md 9.2
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(tmp.path().to_string_lossy().into_owned());
    let later = now() + GRACE + 1.0;
    let mut expected: Vec<(f64, String, String)> = Vec::new();
    for id in ["p", "q"] {
        let mut p = store.project(id).unwrap();
        let big = "x".repeat(2000);
        let a = p
            .add(state(None, 5.0, &format!("@a{{{id}a,\n}}\n{big}"), &store))
            .unwrap();
        let tb = if id == "q" { 5.0 } else { 9.0 };
        let b = p
            .add(state(
                Some(&a),
                tb,
                &format!("@a{{{id}b,\n}}\n{big}"),
                &store,
            ))
            .unwrap();
        let c = p
            .add(state(Some(&b), 10.0, &format!("@a{{{id}c,\n}}\n"), &store))
            .unwrap();
        p.set_head("/x.bib", &c, &[]).unwrap();
        expected.push((5.0, id.to_string(), a));
        expected.push((tb, id.to_string(), b));
    }
    expected.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));
    let order: Vec<(String, String)> = expected.into_iter().map(|(_, p, n)| (p, n)).collect();
    let dropped = |limit: u64| -> Vec<(String, String)> {
        store
            .prune_at(limit, 0, true, &|_| {}, later)
            .into_iter()
            .map(|d| (d.project, d.id))
            .collect()
    };
    assert_eq!(dropped(1), order);
    assert_eq!(dropped(store.size() - 1), order[..1].to_vec());
}

#[test]
fn the_current_states_of_all_the_projects_are_never_dropped() {
    // history.md 9.2
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(tmp.path().to_string_lossy().into_owned());
    for id in ["p", "q"] {
        let mut p = store.project(id).unwrap();
        let n = p
            .add(state(
                None,
                1.0,
                &format!("@a{{{id},\n}}\n{}", "x".repeat(5000)),
                &store,
            ))
            .unwrap();
        p.set_head("/x.bib", &n, &[]).unwrap();
    }
    assert!(
        store
            .prune_at(1, 0, false, &|_| {}, now() + GRACE + 1.0)
            .is_empty()
    );
}

#[test]
fn an_object_of_a_dropped_state_is_freed_at_once_unless_it_was_used_again() {
    // history.md 9.2 (4) and (5): recent, but not later than the newest state file that referred to it
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(tmp.path().to_string_lossy().into_owned());
    let mut p = store.project("p").unwrap();
    let old = p
        .add(state(
            None,
            1.0,
            &format!("@a{{old,\n}}\n{}", "x".repeat(3000)),
            &store,
        ))
        .unwrap();
    let new = p
        .add(state(Some(&old), 2.0, "@a{new,\n}\n", &store))
        .unwrap();
    p.set_head("/x.bib", &new, &[]).unwrap();
    // the objects were written before their states: the state file is newer, so the objects are freed with it
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    for sub in ["objects"] {
        for item in store
            .sizes()
            .values()
            .map(|i| i.path.clone())
            .filter(|p| p.contains(sub))
        {
            fs::File::options()
                .write(true)
                .open(item)
                .unwrap()
                .set_modified(past)
                .unwrap();
        }
    }
    let dropped = store.prune(1, 0, false, &|_| {});
    assert_eq!(dropped.len(), 1);
    assert_eq!(
        store.sizes().len(),
        1,
        "the piece of the dropped state is gone, though it is recent"
    );
    // an object stored again after the states that referred to it is kept
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(tmp.path().to_string_lossy().into_owned());
    let mut p = store.project("p").unwrap();
    let text = format!("@a{{a,\n}}\n{}", "y".repeat(3000));
    let a = p.add(state(None, 5.0, &text, &store)).unwrap();
    let b = p.add(state(Some(&a), 6.0, "@a{b,\n}\n", &store)).unwrap();
    p.set_head("/x.bib", &b, &[]).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    store.put_text(&text).unwrap(); // used again by a command whose state is to come: touched now
    let dropped = store.prune(1, 0, false, &|_| {});
    assert_eq!(dropped.len(), 1);
    assert_eq!(
        store.sizes().len(),
        2,
        "the piece that a command stored again is kept"
    );
}

#[test]
fn a_store_is_separate_when_its_place_is_not_the_shared_one() {
    // history.md 9.1: not told by the name of its folder
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut machine = Map::new();
    // a shared store in a folder named `separate`
    machine.insert(
        "history-dir".into(),
        json!(format!("{root}/data/separate/h")),
    );
    machine.insert("history-size".into(), json!(10));
    let config = Config::new(MachineConfig::new(None, machine));
    let setup = papers_rs::history::Setup::from_config(&config);
    let shared = setup.store(None);
    assert_eq!(setup.history_size(&shared), 10);
}

// 10. .papers/files.json
// ======================

#[test]
fn the_keys_of_files_json_are_written_and_read_with_a_slash() {
    // history.md 10: `\` is read as `/` (papers on Windows wrote it before 3.0.0b6)
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let text = "{\"files\": {\"files\\\\a.pdf\": \"abc\", \"files/./b.pdf\": \"d\", \"files/b.pdf\": \"e\"}}";
    let read = papers_rs::history::files_json::parse(text, &root);
    assert_eq!(
        read.get(&format!("{root}/files/a.pdf")).map(String::as_str),
        Some("abc")
    );
    // two spellings of one file are one link
    assert_eq!(
        read.get(&format!("{root}/files/b.pdf")).map(String::as_str),
        Some("e")
    );
    assert_eq!(read.len(), 2);
    let files = papers_rs::history::FilesJson {
        path: format!("{root}/.papers/files.json"),
        libdir: root.clone(),
        absolute_paths: false,
    };
    files.save(&read, None).unwrap();
    assert_eq!(
        fs::read_to_string(&files.path).unwrap(),
        "{\n \"files\": {\n  \"files/a.pdf\": \"abc\",\n  \"files/b.pdf\": \"e\"\n }\n}\n"
    );
    // a byte order mark is dropped when the file is read
    fs::write(&files.path, "\u{feff}{\"files\": {\"x.pdf\": \"f\"}}").unwrap();
    assert_eq!(files.load().len(), 1);
}
