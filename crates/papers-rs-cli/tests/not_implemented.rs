//! What papers-rs does not have yet says so, with status 1 (papers has no such error: the cases
//! of fixtures/cases never compare these).

use std::fs;
use std::process::{Command, Output};

fn papers(args: &[&str], dir: &std::path::Path) -> Output {
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    Command::new(env!("CARGO_BIN_EXE_papers-rs"))
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("HOME", home)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_commands_that_are_not_ported_say_so() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        &["status"][..],
        &["add", "x.pdf"],
        &["list"],
        &["rm", "key"],
        &["file", "clean"],
        &["history"],
        &["undo", "-n", "2"],
    ] {
        let output = papers(args, dir.path());
        assert_eq!(output.status.code(), Some(1), "papers {args:?}");
        assert_eq!(text(&output.stderr), "papers: error: not implemented in papers-rs yet\n", "papers {args:?}");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn the_version_is_that_of_papers() {
    let dir = tempfile::tempdir().unwrap();
    let output = papers(&["--version"], dir.path());
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(&output.stdout), format!("{}\n", papers_rs::error::PAPERS_VERSION));
}

/// Changing the paths setting rewrites the file links of the bibtex: not ported yet, and nothing is changed.
#[test]
fn config_paths_with_file_links_is_not_ported() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(papers(&["init"], dir.path()).status.code(), Some(0));
    fs::write(dir.path().join("papers.bib"), "@article{x,\n file = {:x.pdf:pdf}\n}\n").unwrap();
    let before = fs::read_to_string(dir.path().join(".papers/config.json")).unwrap();
    let output = papers(&["config", "paths", "absolute"], dir.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stderr).contains("not implemented in papers-rs yet"), "{}", text(&output.stderr));
    assert_eq!(fs::read_to_string(dir.path().join(".papers/config.json")).unwrap(), before);
}

/// A library whose entries have files cannot change its pool yet (the files follow): nothing is changed.
#[test]
fn config_pool_with_file_links_is_not_ported() {
    let dir = tempfile::tempdir().unwrap();
    for lib in ["A", "B"] {
        assert_eq!(papers(&["init", lib], dir.path()).status.code(), Some(0));
    }
    fs::write(dir.path().join("B/papers.bib"), "@article{x,\n file = {:x.pdf:pdf}\n}\n").unwrap();
    let output = papers(&["-C", "B", "config", "pool", "../A"], dir.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stderr).contains("not implemented in papers-rs yet"), "{}", text(&output.stderr));
    assert!(!fs::read_to_string(dir.path().join("B/.papers/config.json")).unwrap().contains("pool\": \"../A"));
}
