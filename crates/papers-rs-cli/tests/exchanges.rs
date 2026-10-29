//! The exchanges with a pool through the command line: `status` records `exchanges.json` (the pieces of the
//! library, of the pool and of the origin file when it found them in sync, spec `exchange.md` 14.4), trusts
//! it, and finds again what changed; the exchange commands that need a pool say so.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use papers_rs::history::store::piece_digests;

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
        let root = fs::canonicalize(tmp.path()).unwrap().to_string_lossy().into_owned();
        let s = Sandbox { _tmp: tmp, root };
        let config = if cfg!(target_os = "macos") {
            s.path("home/Library/Application Support/papers")
        } else {
            s.path("home/.config/papers")
        };
        fs::create_dir_all(&config).unwrap();
        fs::write(
            format!("{config}/config.json"),
            serde_json::json!({"history-dir": s.path("history"), "version": 3}).to_string(),
        )
        .unwrap();
        s
    }

    fn path(&self, rel: &str) -> String {
        format!("{}/{rel}", self.root)
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
        assert_eq!(output.status.code(), Some(0), "{args:?}: {}", text(&output.stderr));
        text(&output.stdout)
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap_or_default()
    }

    /// `exchanges.json` of the history folder of the library, if there is one.
    fn exchanges_json(&self) -> Option<String> {
        fs::read_dir(self.path("history/projects"))
            .ok()?
            .flatten()
            .map(|e| e.path().join("exchanges.json"))
            .find(|p| p.exists())
            .and_then(|p| fs::read_to_string(p).ok())
    }
}

fn sorted_digests(text: &str) -> Vec<String> {
    let mut digests: Vec<String> = piece_digests(text).into_iter().map(|(d, _)| d).collect();
    digests.sort();
    digests.dedup();
    digests
}

fn list(digests: &[String]) -> String {
    format!("[{}]", digests.iter().map(|d| format!("\"{d}\"")).collect::<Vec<_>>().join(", "))
}

#[test]
fn status_records_the_pieces_of_the_texts_it_found_in_sync() {
    let s = Sandbox::new();
    s.ok(".", &["init", "pool"]);
    let bib = "@article{smith2020,\n author = {Smith, John},\n title = {A paper},\n year = {2020}\n}\n";
    fs::write(s.path("pool/refs.bib"), bib).unwrap();
    s.ok("pool", &["add", "refs.bib"]);
    s.ok(".", &["init", "lib", "--pool", "pool"]);
    s.ok("lib", &["pull", "--all"]);
    assert!(s.exchanges_json().is_none(), "nothing before status");
    assert!(s.ok("lib", &["status", "--no-check-files"]).contains("../pool (in sync)\n"));
    let json = s.exchanges_json().expect("written");
    let origin_file = fs::read_dir(s.path("lib/.papers/origin")).unwrap().flatten().next().unwrap().path();
    // the pool is named by the identifier of its library
    let config: serde_json::Value = serde_json::from_str(&s.read("pool/.papers/config.json")).unwrap();
    let id = config["id"].as_str().expect("the pool has an identifier").to_string();
    let want = format!(
        "{{\"{id}\": {{\"library\": {}, \"pool\": {}, \"origin\": {}}}}}",
        list(&sorted_digests(&s.read("lib/papers.bib"))),
        list(&sorted_digests(&s.read("pool/papers.bib"))),
        list(&sorted_digests(&fs::read_to_string(origin_file).unwrap())),
    );
    assert_eq!(json, want);
    // the pieces are trusted while nothing changed, and what changed is found again
    assert!(s.ok("lib", &["status", "--no-check-files"]).contains("../pool (in sync)\n"));
    s.ok("pool", &["edit", "smith2020", "--set", "note=changed in the pool"]);
    let status = s.ok("lib", &["status", "--no-check-files"]);
    assert!(status.contains("../pool (pool changed)\n"), "{status}");
    assert!(status.ends_with("`papers pull --dry-run` shows the changes, `papers pull` takes them\n"), "{status}");
    s.ok("lib", &["edit", "smith2020", "--set", "journal=Nature"]);
    assert!(s.ok("lib", &["status", "--no-check-files"]).contains("../pool (both changed)\n"));
    // the file is not part of the states of the history
    assert!(Path::new(&s.path("lib/.papers/origin")).is_dir());
}

#[test]
fn the_exchanges_need_a_pool() {
    let s = Sandbox::new();
    s.ok(".", &["init", "lib"]);
    for command in ["pull", "push", "sync"] {
        let output = s.papers("lib", &[command]);
        assert_eq!(output.status.code(), Some(1), "{command}");
        assert_eq!(text(&output.stderr), "papers: error: this library has no pool: give one with `--pool PATH`\n");
    }
    // a pool is never created, and the message says where it is missing
    let output = s.papers("lib", &["pull", "--pool", "../nowhere"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(stderr.contains("nowhere does not exist: create it first with `papers init "), "{stderr}");
    assert!(stderr.ends_with("(nothing was changed)\n"), "{stderr}");
    assert!(!Path::new(&s.path("nowhere")).exists());
    // the library is not its own pool
    let output = s.papers("lib", &["push", "--pool", "."]);
    assert!(text(&output.stderr).contains("this is the library itself"), "{}", text(&output.stderr));
}

/// The configured pool is checked again for a pool of its own when the library exchanges with it
/// (`library.md`, States): `pull`, `push`, `sync` and the commands that add to the pool stop with the
/// error of `--pool PATH`; the automatic push after a command warns, and pushes nothing.
#[test]
fn a_pool_that_took_a_pool_of_its_own_is_not_exchanged_with() {
    let s = Sandbox::new();
    let bib = |key: &str| format!("@article{{{key},\n author = {{Smith, John}},\n title = {{A paper}},\n year = {{2020}}\n}}\n");
    fs::write(s.path("x.bib"), bib("x2020")).unwrap();
    fs::write(s.path("y.bib"), bib("y2021")).unwrap();
    for lib in ["C", "A"] {
        s.ok(".", &["init", lib]);
    }
    s.ok(".", &["init", "B", "--pool", "A"]);
    s.ok("B", &["add", "../x.bib"]);
    s.ok("B", &["config", "auto-push", "true"]);
    // the pool takes a pool of its own, after the pool setting of B was made
    s.ok("A", &["config", "pool", "../C"]);
    let error = "papers: error: ../A has a pool itself (../C): give a library without pool";
    for command in [&["pull"][..], &["push"], &["sync"], &["pull", "--all"], &["add", "../y.bib"]] {
        let output = s.papers("B", command);
        assert_eq!(output.status.code(), Some(1), "{command:?}");
        assert!(text(&output.stderr).starts_with(error), "{command:?}: {}", text(&output.stderr));
    }
    // commands that only read the pool go on
    assert!(s.ok("B", &["status", "--no-check-files"]).contains("* pool:   ../A (in sync)\n"));
    // the automatic push warns, and the command goes on
    let output = s.papers("B", &["edit", "x2020", "--set", "journal=J"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stderr),
        "papers: warning: not pushed to the pool: ../A has a pool itself (../C): give a library without pool\n"
    );
    assert!(s.read("B/papers.bib").contains("journal = {J}"));
    assert!(!s.read("A/papers.bib").contains("journal"));
}
