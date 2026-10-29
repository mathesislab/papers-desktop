//! The store of the history and its retention: the cases of papers' own tests
//! (`tests/test_history.py`: pieces, states, places, prune) on `papers_rs::history`.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;

use papers_rs::history::project::{NewState, State, Text};
use papers_rs::history::store::{GRACE, Store, now, sha256};

const BIB: &str = "% a comment before the first entry\n@string{jgr = {Journal of Geophysical Research}}\n\n@article{a2000,\n title = {A, with an @ inside},\n journal = jgr\n}\n\n@article{b2001,\n title = {B \u{e9}t\u{e9}}\n}\n";

fn store() -> (tempfile::TempDir, Store) {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(tmp.path().to_string_lossy().into_owned());
    (tmp, store)
}

fn objects(store: &Store) -> Vec<String> {
    let mut found: Vec<String> = store.sizes().keys().map(|r| format!("{r:?}")).collect();
    found.sort();
    found
}

fn state(store: &Store, text: &str, parent: Option<&str>, time: f64, deleted: &[String]) -> State {
    State::new(NewState {
        parent: parent.map(str::to_string),
        time,
        kind: "command".into(),
        message: "papers x".into(),
        ops: vec![],
        deleted: deleted.to_vec(),
        bibtex: Text {
            pieces: store.put_text(text).unwrap(),
            sha: sha256(text.as_bytes()),
        },
        files: vec![],
    })
}

#[test]
fn a_text_comes_back_byte_for_byte() {
    let (_tmp, store) = store();
    for text in [
        BIB.to_string(),
        BIB.replace('\n', "\r\n"),
        String::new(),
        "@article{x,\n}".into(),
        "no entry at all\n".into(),
    ] {
        assert_eq!(store.text(&store.put_text(&text).unwrap()).unwrap(), text);
    }
}

#[test]
fn pieces_of_entries() {
    let (_tmp, store) = store();
    let big: String = (0..2000)
        .map(|i| format!("@article{{k{i},\n title = {{T{i}}}\n}}\n\n"))
        .collect();
    let pieces = store.put_text(&big).unwrap();
    assert!(pieces.len() > 20 && pieces.len() < 200, "{}", pieces.len()); // about 32 entries a piece
    // an entry added in the middle changes the piece it is in only
    let (head, tail) = big.split_once("@article{k1000,").unwrap();
    let changed = store
        .put_text(&format!(
            "{head}@article{{new,\n}}\n\n@article{{k1000,{tail}"
        ))
        .unwrap();
    let before: BTreeSet<&String> = pieces.iter().collect();
    assert!(
        changed
            .iter()
            .filter(|p| !before.contains(p))
            .collect::<BTreeSet<_>>()
            .len()
            <= 2
    );
}

#[test]
fn pieces_are_shared() {
    let (_tmp, store) = store();
    store.put_text(BIB).unwrap();
    let before = objects(&store).len();
    store
        .put_text(&BIB.replace("{B \u{e9}t\u{e9}}", "{B changed}"))
        .unwrap();
    assert_eq!(objects(&store).len(), before + 1); // one entry changed: one piece
}

#[test]
fn a_missing_piece_is_an_error() {
    let (_tmp, store) = store();
    let pieces = store.put_text(BIB).unwrap();
    fs::remove_dir_all(store.objects()).unwrap();
    assert!(store.text(&pieces).is_err());
}

#[test]
fn deleted_files_are_stored_once_by_checksum() {
    let (tmp, store) = store();
    let path = format!("{}/a.pdf", tmp.path().display());
    fs::write(&path, "pdf").unwrap();
    let digest = sha256(b"pdf");
    let kept = store.put_file(&path, &digest).unwrap();
    assert!(!std::path::Path::new(&path).exists());
    assert_eq!(store.file_object(&digest), Some(kept.clone()));
    assert!(kept.ends_with(&format!("{digest}.pdf")));
    fs::write(&path, "pdf").unwrap();
    assert_eq!(store.put_file(&path, &digest).unwrap(), kept); // stored once
    assert!(store.file_object(&"0".repeat(64)).is_none());
}

#[test]
fn the_identifier_of_a_project_is_checked() {
    let (_tmp, store) = store();
    for bad in ["", "../x", "a/b"] {
        assert!(store.project(bad).is_err(), "{bad}");
    }
    assert!(store.project("a-B_9").is_ok());
}

#[test]
fn places_and_their_start_states() {
    let (tmp, store) = store();
    let mut p = store.project("p").unwrap();
    let one = p.add(state(&store, "1", None, 1.0, &[])).unwrap();
    let two = p.add(state(&store, "2", Some(&one), 2.0, &[])).unwrap();
    let three = p.add(state(&store, "1", Some(&two), 3.0, &[])).unwrap(); // the same bibtex, later
    let place = format!("{}/lib.bib", tmp.path().display());
    fs::write(&place, "").unwrap();
    p.set_head(&place, &two, std::slice::from_ref(&three))
        .unwrap();
    let head = p.head(&place).unwrap();
    assert_eq!((head.state, head.redo), (two.clone(), vec![three.clone()]));
    // a new place starts from the latest state with its bibtex, or else the latest
    assert_eq!(p.start_state(&sha256(b"1")), Some(three.clone()));
    assert_eq!(p.start_state(&sha256(b"unknown")), Some(three.clone()));
    p.set_head("/nowhere/lib.bib", &one, &[]).unwrap();
    assert_eq!(
        p.forget_missing_places().unwrap(),
        vec!["/nowhere/lib.bib".to_string()]
    );
    assert_eq!(
        p.heads().into_iter().map(|h| h.place).collect::<Vec<_>>(),
        vec![place.clone()]
    );
    assert_eq!(p.remove_head(&place).unwrap(), 0);
}

#[test]
fn the_oldest_states_go_first_and_the_current_one_stays() {
    let (_tmp, store) = store();
    let mut p = store.project("p").unwrap();
    let t = now();
    let old = p
        .add(state(&store, "@article{old,\n}\n", None, t - 100.0, &[]))
        .unwrap();
    let new = p
        .add(state(&store, "@article{new,\n}\n", Some(&old), t, &[]))
        .unwrap();
    p.set_head("/lib.bib", &new, &[]).unwrap();
    let later = t + GRACE + 1.0;
    let nothing = |_: &str| {};
    assert!(store.prune_at(0, 0, false, &nothing, later).is_empty()); // no limit
    let dropped = store.prune_at(1, 0, true, &nothing, later);
    assert_eq!(
        dropped.iter().map(|d| d.id.clone()).collect::<Vec<_>>(),
        vec![old.clone()]
    );
    assert!(store.project("p").unwrap().state(&old).is_some()); // dry: nothing changed
    store.prune_at(1, 0, false, &nothing, later);
    let left: Vec<String> = store
        .project("p")
        .unwrap()
        .states()
        .keys()
        .cloned()
        .collect();
    assert_eq!(left, vec![new]); // the current state is kept
    assert_eq!(objects(&store).len(), 1);
}

#[test]
fn recent_objects_are_kept() {
    // a command writes its pieces before its state
    let (_tmp, store) = store();
    store.put_text("@article{orphan,\n}\n").unwrap();
    store.prune(1, 0, false, &|_| {});
    assert_eq!(objects(&store).len(), 1);
    store.prune_at(1, 0, false, &|_| {}, now() + GRACE + 1.0);
    assert!(objects(&store).is_empty());
}

#[test]
fn files_go_to_the_trash_function() {
    let (tmp, store) = store();
    let mut p = store.project("p").unwrap();
    let path = format!("{}/a.pdf", tmp.path().display());
    fs::write(&path, vec![b'x'; 1000]).unwrap();
    let digest = sha256(&[b'x'; 1000]);
    store.put_file(&path, &digest).unwrap();
    let old = p.add(state(&store, "", None, 1.0, &[digest])).unwrap();
    let head = p
        .add(state(&store, "@a{b,\n}", Some(&old), 2.0, &[]))
        .unwrap();
    p.set_head("/lib.bib", &head, &[]).unwrap();
    let discarded = RefCell::new(Vec::new());
    store.prune_at(
        10,
        0,
        false,
        &|p| discarded.borrow_mut().push(p.to_string()),
        now() + GRACE + 1.0,
    );
    assert_eq!(discarded.borrow().len(), 1);
    assert!(discarded.borrow()[0].ends_with(".pdf"));
}

#[test]
fn what_is_about_to_be_stored_counts() {
    let (_tmp, store) = store();
    let mut p = store.project("p").unwrap();
    let old = p.add(state(&store, "@a{a,\n}", None, 1.0, &[])).unwrap();
    let head = p
        .add(state(&store, "@a{b,\n}", Some(&old), 2.0, &[]))
        .unwrap();
    p.set_head("/lib.bib", &head, &[]).unwrap();
    let size = store.size();
    assert!(store.prune(size, 0, true, &|_| {}).is_empty());
    assert_eq!(store.prune(size, 1000, true, &|_| {}).len(), 1);
}
