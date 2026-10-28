//! The history of a library through the API that the library model calls
//! (`History::before_command`, `open`, `sync_head`, `record`, `add_operations`,
//! then `undo`, `redo`, `restore`): the cases of papers' own tests
//! (`tests/test_history.py`, `test_undo.py`), on libraries made by hand.

use std::fs;
use std::path::Path;

use papers_rs::config::paths::realpath;
use papers_rs::config::{Config, MachineConfig};
use papers_rs::history::listing::{Removal, history_lines};
use papers_rs::history::ojson::J;
use papers_rs::history::report::linked_files;
use papers_rs::history::{FileOp, History, OpKind, Recorded, Step};
use serde_json::{Map, Value, json};

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: String,
    machine: Map<String, Value>,
}

fn sandbox(machine: Value) -> Sandbox {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut machine = machine.as_object().cloned().unwrap_or_default();
    machine.insert("history-dir".into(), json!(format!("{root}/history")));
    Sandbox {
        _tmp: tmp,
        root,
        machine,
    }
}

fn entry(key: &str) -> String {
    format!(
        "@article{{{key},\n title = {{Title of {key}}},\n author = {{Doe, Jane}},\n year = {{2000}},\n}}\n\n"
    )
}

impl Sandbox {
    fn path(&self, rel: &str) -> String {
        format!("{}/{rel}", self.root)
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
        let file = self.path(&format!("{dir}/.papers/config.json"));
        fs::create_dir_all(Path::new(&file).parent().unwrap()).unwrap();
        fs::write(
            &file,
            "{\n  \"bibtex\": \"papers.bib\",\n  \"filesdir\": \"files\",\n  \"version\": 3\n}",
        )
        .unwrap();
        fs::write(self.path(&format!("{dir}/papers.bib")), "").unwrap();
        self.config(dir)
    }
}

fn bibtex(config: &Config) -> String {
    fs::read_to_string(config.bibtex.as_ref().unwrap()).unwrap()
}

/// What a command does around the save of a library: the start, the head, the
/// write of the bibtex (and of what the command puts in the library), the step.
fn command(
    config: &mut Config,
    message: &str,
    write: impl FnOnce(&Config),
    ops: Vec<FileOp>,
) -> Recorded {
    let before = History::before_command(config);
    let mut history = History::open(config, true, false).unwrap().unwrap();
    history.sync_head(before.as_ref()).unwrap();
    write(config);
    history
        .record(Step {
            message: message.into(),
            ops,
            ..Step::default()
        })
        .unwrap()
}

fn add(config: &mut Config, key: &str) -> Recorded {
    command(
        config,
        &format!("papers add ../{key}.bib"),
        |c| {
            let text = bibtex(c) + &entry(key);
            fs::write(c.bibtex.as_ref().unwrap(), text).unwrap();
        },
        vec![],
    )
}

fn keys(config: &Config) -> Vec<String> {
    ["one2000", "two2000", "three2000"]
        .iter()
        .filter(|k| bibtex(config).contains(&format!("{{{k},")))
        .map(|k| k.to_string())
        .collect()
}

fn open(config: &mut Config) -> History {
    History::open(config, false, false).unwrap().unwrap()
}

fn undo(config: &mut Config, steps: usize) -> papers_rs::error::Result<papers_rs::history::Moved> {
    History::require(config, false)
        .unwrap()
        .undo(steps, false, None, "papers undo")
}

fn id_of(config: &Config) -> String {
    let text = fs::read_to_string(config.file.as_ref().unwrap()).unwrap();
    J::parse(&text)
        .unwrap()
        .get("id")
        .and_then(J::as_str)
        .unwrap()
        .to_string()
}

#[test]
fn nothing_is_written_before_a_change() {
    let s = sandbox(json!({}));
    let mut config = s.init("lib");
    // a command that changes nothing: no identifier, no store
    assert!(History::open(&mut config, false, false).unwrap().is_none());
    assert!(
        J::parse(&fs::read_to_string(config.file.as_ref().unwrap()).unwrap())
            .unwrap()
            .get("id")
            .is_none()
    );
    assert!(!Path::new(&s.path("history")).exists());
    let setup = papers_rs::history::Setup::from_config(&config);
    assert_eq!(
        history_lines(&setup, None, true, 20)[0],
        "no history yet: each command that changes the library records a step"
    );
    // the first change gives the library its identifier
    add(&mut config, "one2000");
    let id = id_of(&config);
    assert_eq!(id.len(), 32);
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(Path::new(&s.path(&format!("history/projects/{id}/states/000002.json"))).exists());
    // a dry run gives no identifier and writes nothing
    let mut other = s.init("other");
    assert!(History::open(&mut other, true, true).unwrap().is_none());
    assert!(
        J::parse(&fs::read_to_string(other.file.as_ref().unwrap()).unwrap())
            .unwrap()
            .get("id")
            .is_none()
    );
}

#[test]
fn a_command_that_saves_the_same_library_adds_no_state() {
    let s = sandbox(json!({}));
    let mut config = s.init("lib");
    add(&mut config, "one2000");
    let rec = command(&mut config, "papers tag nothing", |_| {}, vec![]);
    assert!(!rec.added);
    assert_eq!(rec.number, Some(2));
    assert_eq!(open(&mut config).project.states().len(), 2);
}

#[test]
fn a_moved_library_continues_its_history() {
    let s = sandbox(json!({}));
    let mut config = s.init("lib");
    add(&mut config, "one2000");
    add(&mut config, "two2000");
    fs::rename(s.path("lib"), s.path("moved")).unwrap();
    let mut moved = s.config("moved");
    undo(&mut moved, 1).unwrap();
    assert_eq!(keys(&moved), vec!["one2000"]);
    let history = open(&mut moved);
    let places: Vec<String> = history
        .project
        .heads()
        .into_iter()
        .map(|h| h.place)
        .collect();
    assert_eq!(places, vec![realpath(&s.path("moved/papers.bib"))]); // the old place is forgotten
}

#[test]
fn a_copied_library_branches() {
    let s = sandbox(json!({}));
    let mut config = s.init("lib");
    add(&mut config, "one2000");
    fs::create_dir_all(s.path("copy/.papers")).unwrap();
    for f in ["papers.bib", ".papers/config.json"] {
        fs::copy(s.path(&format!("lib/{f}")), s.path(&format!("copy/{f}"))).unwrap();
    }
    let mut copy = s.config("copy");
    add(&mut config, "two2000"); // in the library
    add(&mut copy, "three2000"); // in the copy
    undo(&mut copy, 1).unwrap();
    assert_eq!(keys(&copy), vec!["one2000"]);
    assert_eq!(keys(&config), vec!["one2000", "two2000"]); // the library is left alone
    undo(&mut copy, 1).unwrap(); // the history before the copy is shared
    assert!(keys(&copy).is_empty());
    undo(&mut config, 1).unwrap();
    assert_eq!(keys(&config), vec!["one2000"]);
    assert_eq!(open(&mut config).project.heads().len(), 2);
    // removing the history of a place keeps the project for the other
    let mut history = open(&mut copy);
    match history.remove_place().unwrap() {
        Removal::Place { others, .. } => assert_eq!(others.len(), 1),
        other => panic!("{other:?}"),
    }
}

#[test]
fn removing_the_history_of_the_last_place() {
    let s = sandbox(json!({}));
    let mut config = s.init("lib");
    add(&mut config, "one2000");
    let mut history = open(&mut config);
    assert_eq!(
        history.remove_place().unwrap(),
        Removal::Project { steps: 2 }
    );
    history.remove_project();
    assert!(!history.project.exists());
    assert!(
        History::open(&mut config, false, false)
            .unwrap()
            .unwrap()
            .head()
            .is_none()
    );
}

#[test]
fn changes_made_outside_papers_are_a_step_of_their_own() {
    let s = sandbox(json!({}));
    let mut config = s.init("lib");
    add(&mut config, "one2000");
    fs::write(
        config.bibtex.as_ref().unwrap(),
        bibtex(&config) + &entry("two2000"),
    )
    .unwrap();
    // undo goes back before the outside change, redo brings it back
    let moved = undo(&mut config, 1).unwrap();
    assert_eq!(moved.target, 2);
    assert_eq!(keys(&config), vec!["one2000"]);
    let history = open(&mut config);
    assert_eq!(history.project.state(3).unwrap().kind(), "outside");
    assert_eq!(
        history.project.state(3).unwrap().message(),
        "changed outside papers"
    );
    History::require(&mut config, false)
        .unwrap()
        .redo(1, false, None, "papers redo")
        .unwrap();
    assert_eq!(keys(&config), vec!["one2000", "two2000"]);
    // a change after the undo clears the redo list: redo says why
    undo(&mut config, 1).unwrap();
    fs::write(
        config.bibtex.as_ref().unwrap(),
        bibtex(&config) + &entry("three2000"),
    )
    .unwrap();
    let error = History::require(&mut config, false)
        .unwrap()
        .redo(1, false, None, "papers redo")
        .unwrap_err();
    assert!(
        error
            .message()
            .starts_with("nothing to redo: the library changed outside papers since the undo"),
        "{error}"
    );
    // restore without a number: the outside edit is undone (and recorded), redo brings it back
    let head = open(&mut config).head().unwrap().state;
    let text = bibtex(&config);
    fs::write(config.bibtex.as_ref().unwrap(), "").unwrap();
    let mut history = History::require(&mut config, false).unwrap();
    assert_eq!(history.restore(None, None).unwrap().target, head);
    assert_eq!(bibtex(&config), text);
    History::require(&mut config, false)
        .unwrap()
        .redo(1, false, None, "papers redo")
        .unwrap();
    assert_eq!(bibtex(&config), "");
    // and when nothing changed outside, there is nothing to restore
    let error = History::require(&mut config, false)
        .unwrap()
        .restore(None, None)
        .unwrap_err();
    assert!(
        error.message().starts_with("nothing to restore:"),
        "{error}"
    );
}

#[test]
fn undo_and_redo_stop_where_the_history_does() {
    let s = sandbox(json!({}));
    let mut config = s.init("lib");
    add(&mut config, "one2000");
    add(&mut config, "two2000");
    assert_eq!(
        undo(&mut config, 9).unwrap_err().message(),
        "only 2 step(s) to undo"
    );
    undo(&mut config, 2).unwrap();
    assert_eq!(
        undo(&mut config, 1).unwrap_err().message(),
        "nothing to undo"
    );
    let mut history = History::require(&mut config, false).unwrap();
    assert_eq!(
        history.redo(9, false, None, "").unwrap_err().message(),
        "only 2 step(s) to redo"
    );
    assert_eq!(
        history.restore(Some(99), None).unwrap_err().message(),
        "no step 99 in the history of the library (see `papers history`)"
    );
    // a dry run changes nothing
    let mut dry = History::require(&mut config, true).unwrap();
    let moved = dry.redo(2, false, None, "").unwrap();
    assert!(moved.files.is_none() && keys(&config).is_empty());
    assert!(moved.lines("redo", &dry)[0].starts_with("DRYRUN: redo: "));
}

#[test]
fn a_file_taken_out_of_downloads_goes_back_there() {
    let s = sandbox(json!({}));
    let mut config = s.init("lib");
    let (src, dst) = (
        s.path("Downloads/paper.pdf"),
        s.path("lib/files/doe_2000_a.pdf"),
    );
    fs::create_dir_all(s.path("Downloads")).unwrap();
    fs::create_dir_all(s.path("lib/files")).unwrap();
    fs::write(&src, "%PDF the paper").unwrap();
    // the command moves the file, writes the entry, records the step with the operation
    let before = History::before_command(&mut config);
    let mut history = History::open(&mut config, true, false).unwrap().unwrap();
    history.sync_head(before.as_ref()).unwrap();
    let mut entry_text = entry("one2000");
    entry_text = entry_text.replace(" year", " file = {:files/doe_2000_a.pdf:pdf},\n year");
    fs::rename(&src, &dst).unwrap();
    fs::write(config.bibtex.as_ref().unwrap(), entry_text).unwrap();
    let digest = papers_rs::history::restore::file_digest(&dst);
    let op = FileOp::new(OpKind::Moved, &src, Some(&dst), digest);
    history
        .record(Step {
            message: "papers add one.bib --attach paper.pdf --move".into(),
            ops: vec![op],
            ..Step::default()
        })
        .unwrap();
    // outside the library: absolute in the state; inside: relative to the library
    let history = open(&mut config);
    let ops = history.project.state(2).unwrap().ops();
    assert_eq!(
        (ops[0].src.as_str(), ops[0].dst.as_deref()),
        (src.as_str(), Some("files/doe_2000_a.pdf"))
    );
    assert_eq!(linked_files(config.bibtex.as_ref().unwrap()).len(), 1);
    let moved = undo(&mut config, 1).unwrap();
    let lines = moved.files.unwrap();
    assert!(
        lines[0].ends_with(&format!("--> moved back to {src}")),
        "{lines:?}"
    );
    assert_eq!(fs::read_to_string(&src).unwrap(), "%PDF the paper");
    assert!(!Path::new(&dst).exists());
    History::require(&mut config, false)
        .unwrap()
        .redo(1, false, None, "papers redo")
        .unwrap();
    assert!(!Path::new(&src).exists());
    assert_eq!(fs::read_to_string(&dst).unwrap(), "%PDF the paper");
}

#[test]
fn deleted_files_are_kept_and_the_operations_after_the_save_join_the_state() {
    let s = sandbox(json!({}));
    let mut config = s.init("lib");
    let file = s.path("lib/files/a.pdf");
    fs::create_dir_all(s.path("lib/files")).unwrap();
    fs::write(&file, "pdf a").unwrap();
    let text = entry("one2000").replace(" year", " file = {:files/a.pdf:pdf},\n year");
    let t2 = text.clone();
    let rec = command(
        &mut config,
        "papers add one.bib",
        move |c| fs::write(c.bibtex.as_ref().unwrap(), t2).unwrap(),
        vec![],
    );
    assert_eq!(rec.number, Some(2));
    // rm --delete-files: the bibtex is saved first, the file deleted after
    let rec = command(
        &mut config,
        "papers rm one2000 --delete-files",
        |c| fs::write(c.bibtex.as_ref().unwrap(), "").unwrap(),
        vec![],
    );
    let n = rec.number.unwrap();
    let mut history = open(&mut config);
    let mut recorded = Some(Vec::new());
    let deleter = papers_rs::history::restore::Deleter {
        keep: true,
        store: Some(&history.store),
        trash: &|p| drop(fs::remove_file(p)),
    };
    deleter.remove(&file, &mut recorded).unwrap();
    let ops = recorded.unwrap();
    history.add_operations(n, &ops).unwrap();
    let state = history.project.state(n).unwrap().clone();
    let digest = papers_rs::history::store::sha256(b"pdf a");
    assert_eq!(state.deleted(), vec![digest.clone()]);
    assert_eq!(state.ops()[0].src, "files/a.pdf");
    assert!(history.store.file_object(&digest).is_some());
    assert_eq!(history.kept_size(n), 5);
    // undo brings it back, from the history
    let moved = undo(&mut config, 1).unwrap();
    assert!(moved.files.unwrap()[0].ends_with("files/a.pdf: restored from the history"));
    assert_eq!(fs::read_to_string(&file).unwrap(), "pdf a");
    // the operations are not recorded when the library changed since (a warning, no change)
    let mut history = open(&mut config);
    let before = history.project.state(n).unwrap().clone();
    history.add_operations(n, &ops).unwrap();
    assert_eq!(history.project.state(n).unwrap(), &before);
}

#[test]
fn the_files_json_of_a_state_finds_a_file_by_content() {
    use papers_rs::history::FilesJson;
    let s = sandbox(json!({}));
    let mut config = s.init("lib");
    fs::create_dir_all(s.path("lib/files")).unwrap();
    fs::write(s.path("lib/files/a.pdf"), "pdf a").unwrap();
    let files = FilesJson::of(&config, &s.path("lib")).unwrap();
    let links = [s.path("lib/files/a.pdf")].into_iter().collect();
    let text = entry("one2000").replace(" year", " file = {:files/a.pdf:pdf},\n year");
    let before = History::before_command(&mut config);
    let mut history = History::open(&mut config, true, false).unwrap().unwrap();
    history.sync_head(before.as_ref()).unwrap();
    assert!(files.update(&links, &links, true).unwrap());
    fs::write(config.bibtex.as_ref().unwrap(), text).unwrap();
    history.record(Step::new("papers add one.bib")).unwrap();
    let state = open(&mut config).project.state(2).unwrap().clone();
    assert_eq!(
        state
            .files()
            .unwrap()
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        vec![".papers/files.json"]
    );
    // the file is renamed outside papers, and its link too: undo finds it by its checksum
    fs::rename(s.path("lib/files/a.pdf"), s.path("lib/files/renamed.pdf")).unwrap();
    let relinked = bibtex(&config).replace("files/a.pdf", "files/renamed.pdf");
    fs::write(config.bibtex.as_ref().unwrap(), relinked).unwrap();
    let moved = undo(&mut config, 1).unwrap();
    assert_eq!(moved.target, 2);
    let lines = moved.files.unwrap();
    assert!(
        lines[0].ends_with("renamed.pdf --> renamed back to a.pdf"),
        "{lines:?}"
    );
    assert!(
        Path::new(&s.path("lib/files/a.pdf")).exists()
            && !Path::new(&s.path("lib/files/renamed.pdf")).exists()
    );
    // a file whose content is not the one of the checksum is never overwritten
    fs::remove_file(s.path("lib/files/a.pdf")).unwrap();
    fs::write(s.path("lib/files/renamed.pdf"), "something else").unwrap();
    History::require(&mut config, false)
        .unwrap()
        .redo(1, false, None, "")
        .unwrap();
    assert_eq!(
        fs::read_to_string(s.path("lib/files/renamed.pdf")).unwrap(),
        "something else"
    );
}

#[test]
fn the_size_of_a_store_of_its_own() {
    let s = sandbox(json!({"history-size": 3}));
    let mut config = s.init("lib");
    config.set("history-separate", json!(true)).unwrap();
    config.set("history-size", json!(7)).unwrap();
    add(&mut config, "one2000");
    let id = id_of(&config);
    assert!(Path::new(&s.path(&format!("history/separate/{id}/projects/{id}"))).is_dir());
    assert!(!Path::new(&s.path(&format!("history/projects/{id}"))).exists());
    let history = open(&mut config);
    assert_eq!(history.setup.history_size(&history.store), 7);
    let lines = history_lines(&history.setup, Some(&history), true, 20);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("history: ") && l.contains("up to 7 MB")),
        "{lines:?}"
    );
    // without history-separate the size of the library does not apply
    let mut shared = s.init("shared");
    add(&mut shared, "one2000");
    let history = open(&mut shared);
    assert_eq!(history.setup.history_size(&history.store), 3);
}

#[test]
fn the_guard_before_deleting_files() {
    let s = sandbox(json!({"history-size": 1}));
    let mut config = s.init("lib");
    add(&mut config, "one2000");
    let mut history = open(&mut config);
    let mut asked = Vec::new();
    // more than the history keeps: the question, and what yes and no mean
    let big = 2 * 1024 * 1024;
    assert_eq!(
        history.guard(big, 2, &mut |q| {
            asked.push(q.to_string());
            true
        }),
        Some(false)
    );
    assert!(
        asked[0].starts_with(
            "2 files to delete, 2 MB: more than the history keeps (history-size: 1 MB)."
        ),
        "{asked:?}"
    );
    assert_eq!(history.guard(big, 2, &mut |_| false), None);
    // a small file: kept, without a question
    assert_eq!(
        history.guard(100, 1, &mut |_| panic!("no question")),
        Some(true)
    );
    // keep-deleted-files false: the system trash
    history.setup.keep_deleted_files = false;
    assert_eq!(
        history.guard(100, 1, &mut |_| panic!("no question")),
        Some(false)
    );
}

#[test]
fn a_damaged_store_is_reported_as_papers_does() {
    let s = sandbox(json!({}));
    let mut config = s.init("lib");
    add(&mut config, "one2000");
    add(&mut config, "two2000");
    let history = open(&mut config);
    let piece = history.project.state(2).unwrap().bibtex_pieces()[0].clone();
    drop(history);
    fs::remove_file(s.path(&format!("history/objects/{}/{piece}", &piece[..2]))).unwrap();
    let error = undo(&mut config, 1).unwrap_err();
    assert_eq!(
        error.message(),
        format!(
            "the history of the library is damaged: 'piece {piece} is missing from the history'"
        )
    );
}

#[test]
fn the_store_is_pruned_at_every_fiftieth_state() {
    let s = sandbox(json!({}));
    let mut config = s.init("lib");
    for i in 1..49 {
        command(
            &mut config,
            &format!("papers x {i}"),
            |c| fs::write(c.bibtex.as_ref().unwrap(), entry(&format!("k{i}"))).unwrap(),
            vec![],
        );
    }
    // an object that no state refers to, a day old
    let store = open(&mut config).store.clone();
    let orphan = store.put_text("@article{orphan,\n}\n").unwrap();
    let path = s.path(&format!(
        "history/objects/{}/{}",
        &orphan[0][..2],
        orphan[0]
    ));
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(86400);
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(old)
        .unwrap();
    // 48 commands and the start state make 49 states: the 50th prunes
    let rec = command(
        &mut config,
        "papers x 49",
        |c| fs::write(c.bibtex.as_ref().unwrap(), entry("k49")).unwrap(),
        vec![],
    );
    assert_eq!(rec.number, Some(50));
    assert!(!Path::new(&path).exists());
}
