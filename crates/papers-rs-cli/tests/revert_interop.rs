//! `papers history --revert`, and the revert of a pool step by `undo` and `redo`: papers and papers-rs on the
//! same history (the reference papers is `PAPERS_REFERENCE`, a command):
//!
//! ```text
//! PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test revert_interop -- --ignored
//! ```
//!
//! - what papers-rs records as a revert, papers undoes and redoes, and the reverse;
//! - a library with a pool, built by papers, whose pool moved on: `undo` and `redo` by papers-rs and by papers
//!   leave the same libraries, the same history store (the times aside) and print the same.
//!
//! (`spec/cases/history/revert-*` has the cases of the revert of a library, recorded from papers.)

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::json;

const RS: &str = env!("CARGO_BIN_EXE_papers-rs");

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

const REFS: &str = "@article{doe2000,\n title = {Ice sheets and sea level},\n author = {Doe, Jane and Roe, Richard},\n year = {2000},\n journal = {Nature},\n doi = {10.1000/doe.2000},\n}\n\n@article{roe2001,\n title = {Sea level rise in the next century},\n author = {Roe, Richard},\n year = {2001},\n journal = {Science},\n doi = {10.1000/roe.2001},\n}\n\n@book{smith2002,\n title = {Glaciers},\n author = {Smith, John},\n year = {2002},\n publisher = {Ice Press},\n}\n";

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
        s.write("refs.bib", REFS);
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

    fn output(&self, program: &str, cwd: &str, args: &[&str]) -> Output {
        let mut words = program.split_whitespace();
        Command::new(words.next().unwrap())
            .args(words)
            .args(args)
            .current_dir(self.path(cwd))
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", self.path("home"))
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .env("TZ", "UTC")
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    }

    /// The standard output of a command that succeeds.
    fn run(&self, program: &str, cwd: &str, args: &[&str]) -> String {
        let output = self.output(program, cwd, args);
        assert!(
            output.status.success(),
            "{program} {args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// What a command prints, the standard output then the standard error, with the sandbox and the times left out.
    fn says(&self, program: &str, cwd: &str, args: &[&str]) -> String {
        let output = self.output(program, cwd, args);
        let text = format!(
            "exit {}\n{}{}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        self.normalize(&text)
    }

    /// The identifiers of the libraries (the `id` of their configuration) with their names.
    fn ids(&self) -> Vec<(String, &'static str)> {
        let mut ids = Vec::new();
        for (dir, name) in [("lib", "$LIB"), ("pool", "$POOL")] {
            let text = self.read(&format!("{dir}/.papers/config.json"));
            if let Some(at) = text.find("\"id\": \"") {
                let id: String = text[at + 7..]
                    .chars()
                    .take_while(|c| c.is_ascii_hexdigit())
                    .collect();
                if id.len() == 32 {
                    ids.push((id, name));
                }
            }
        }
        ids
    }

    /// The sandbox, the times, the identifiers of the libraries and the times of the exchanges, as placeholders.
    fn normalize(&self, text: &str) -> String {
        let mut text = text.replace(&self.root, "$ROOT");
        for (id, name) in self.ids() {
            text = text.replace(&id, name);
        }
        let chars: Vec<char> = text.chars().collect();
        let starts = |i: usize, word: &str| {
            chars[i..]
                .iter()
                .take(word.chars().count())
                .collect::<String>()
                == word
        };
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            // `(2026-10-22 14:03)` -> `($TIME)`
            let rest: String = chars[i..chars.len().min(i + 17)].iter().collect();
            if rest.len() == 17
                && rest.starts_with('(')
                && rest[1..5].chars().all(|c| c.is_ascii_digit())
                && &rest[5..6] == "-"
                && &rest[11..12] == " "
                && &rest[14..15] == ":"
            {
                out.push_str("($TIME");
                i += 17;
                continue;
            }
            // the times of the states and heads, and of the records of the exchanges
            let mut replaced = false;
            for (word, placeholder) in [
                ("\"time\": ", "0"),
                ("papers-pulled ", "$N"),
                ("papers-time = {", "$T"),
            ] {
                if starts(i, word) {
                    out.push_str(word);
                    i += word.chars().count();
                    while i < chars.len() && !matches!(chars[i], ',' | '\n' | '}') {
                        i += 1;
                    }
                    out.push_str(placeholder);
                    replaced = true;
                    break;
                }
            }
            if replaced {
                continue;
            }
            let hex = chars.len() >= i + 32
                && chars[i..i + 32].iter().all(|c| c.is_ascii_hexdigit())
                && (i == 0 || !chars[i - 1].is_ascii_alphanumeric())
                && chars.get(i + 32).is_none_or(|c| !c.is_ascii_alphanumeric());
            if hex {
                out.push_str("$ID");
                i += 32;
                continue;
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    }

    /// Every file of the sandbox, as text, with the times, the places and the identifiers normalized. The objects
    /// of the history store are named by their normalized content (the same text is the same object, whenever it
    /// was made), and the states say which.
    fn snapshot(&self) -> BTreeMap<String, String> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::io::Read;

        fn files(dir: &Path, root: &Path, found: &mut Vec<String>) {
            let mut entries: Vec<_> = fs::read_dir(dir).unwrap().flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let path = e.path();
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                if path.is_dir() {
                    files(&path, root, found);
                } else if !(rel.ends_with("/lock")
                    || rel.contains("/cache")
                    || rel.contains("Caches"))
                {
                    found.push(rel);
                }
            }
        }
        let mut names = Vec::new();
        files(Path::new(&self.root), Path::new(&self.root), &mut names);
        let mut objects: BTreeMap<String, String> = BTreeMap::new(); // the sha of the object -> its normalized text
        let mut tree = BTreeMap::new();
        for rel in &names {
            let bytes = fs::read(self.path(rel)).unwrap_or_default();
            if let Some(sha) = rel
                .strip_prefix("history/objects/")
                .and_then(|r| r.rsplit('/').next())
            {
                let mut text = String::new();
                let _ = flate2::read::ZlibDecoder::new(&bytes[..]).read_to_string(&mut text);
                objects.insert(sha.to_string(), self.normalize(&text));
            } else {
                tree.insert(
                    self.normalize(rel),
                    self.normalize(&String::from_utf8_lossy(&bytes)),
                );
            }
        }
        // a text is cut in pieces where its content says, so the pieces of the same text differ when a time in it
        // does: the states are compared with the pieces put back together
        let summary = |text: &str| {
            let mut hasher = DefaultHasher::new();
            text.hash(&mut hasher);
            format!("text:{:016x}", hasher.finish())
        };
        fn put_back(
            value: &mut serde_json::Value,
            objects: &BTreeMap<String, String>,
            tree: &mut BTreeMap<String, String>,
            summary: &dyn Fn(&str) -> String,
        ) {
            use serde_json::Value;
            match value {
                Value::Array(items)
                    if !items.is_empty()
                        && items
                            .iter()
                            .all(|i| i.as_str().is_some_and(|i| objects.contains_key(i))) =>
                {
                    let text: String = items
                        .iter()
                        .map(|i| objects[i.as_str().unwrap()].clone())
                        .collect();
                    let name = summary(&text);
                    tree.insert(name.clone(), text);
                    *value = Value::String(name);
                }
                Value::Array(items) => items
                    .iter_mut()
                    .for_each(|i| put_back(i, objects, tree, summary)),
                Value::Object(map) => {
                    for (key, v) in map.iter_mut() {
                        if key == "sha" {
                            *v = Value::String("$SHA".into());
                        } else {
                            put_back(v, objects, tree, summary);
                        }
                    }
                }
                _ => {}
            }
        }
        let state_files: Vec<String> = tree
            .keys()
            .filter(|k| k.contains("history/projects/"))
            .cloned()
            .collect();
        for key in state_files {
            let text = tree[&key].clone();
            if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) {
                put_back(&mut value, &objects, &mut tree, &summary);
                tree.insert(key, serde_json::to_string_pretty(&value).unwrap());
            }
        }
        tree
    }
}

fn reference() -> Option<String> {
    match std::env::var("PAPERS_REFERENCE") {
        Ok(r) => Some(r),
        Err(_) => {
            eprintln!("skipped: PAPERS_REFERENCE is not set");
            None
        }
    }
}

fn assert_same(a: &BTreeMap<String, String>, b: &BTreeMap<String, String>, what: &str) {
    let only = |x: &BTreeMap<String, String>, y: &BTreeMap<String, String>| -> Vec<String> {
        x.iter()
            .filter(|(k, _)| !y.contains_key(*k))
            .map(|(k, v)| format!("{k}:\n{v}"))
            .collect()
    };
    let (only_a, only_b) = (only(a, b), only(b, a));
    assert!(
        only_a.is_empty() && only_b.is_empty(),
        "{what}: the files differ\n=== only in the first:\n{}\n=== only in the second:\n{}",
        only_a.join("\n"),
        only_b.join("\n")
    );
    for (name, text) in a {
        assert_eq!(text, &b[name], "{what}: {name}");
    }
}

// a revert by one program, undone and redone by the other
// -------------------------------------------------------

fn library(s: &Sandbox, papers: &str) {
    s.run(papers, ".", &["init", "lib"]);
    s.write("lib/dl/a.txt", "a\n");
    s.run(papers, "lib", &["add", "../refs.bib"]);
    s.run(papers, "lib", &["tag", "doe2000", "--add", "x"]);
    s.run(papers, "lib", &["file", "attach", "roe2001", "dl/a.txt"]);
    s.run(papers, "lib", &["edit", "doe2000", "--set", "note=later"]);
    s.run(papers, "lib", &["tag", "doe2000", "--add", "y"]);
}

#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test revert_interop -- --ignored"]
fn what_one_program_reverts_the_other_undoes_and_redoes() {
    let Some(papers) = reference() else { return };
    for (reverting, other) in [
        (RS.to_string(), papers.clone()),
        (papers.clone(), RS.to_string()),
    ] {
        let s = Sandbox::new();
        library(&s, &papers);
        // the steps: 1 start, 2 add, 3 tag x, 4 attach, 5 edit, 6 tag y
        let before = s.snapshot();
        s.run(&reverting, "lib", &["history", "--revert", "3"]);
        s.run(&reverting, "lib", &["history", "--revert", "4"]);
        let reverted = s.read("lib/papers.bib");
        assert!(!reverted.contains("keywords = {x"), "{reverted}");
        assert!(
            Path::new(&s.path("lib/dl/a.txt")).exists(),
            "the file went back"
        );
        assert!(reverted.contains("note = {later}"));
        // the other program undoes the two reverts, and redoes them
        s.run(&other, "lib", &["undo", "-n", "2"]);
        let undone = s.snapshot();
        assert_eq!(
            undone
                .iter()
                .filter(|(k, _)| k.starts_with("lib/"))
                .collect::<Vec<_>>(),
            before
                .iter()
                .filter(|(k, _)| k.starts_with("lib/"))
                .collect::<Vec<_>>(),
            "the library is as before the reverts"
        );
        s.run(&other, "lib", &["redo", "-n", "2"]);
        assert_eq!(s.read("lib/papers.bib"), reverted);
        assert!(Path::new(&s.path("lib/files")).exists());
    }
}

#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test revert_interop -- --ignored"]
fn the_same_revert_by_both_programs() {
    let Some(papers) = reference() else { return };
    let runs: Vec<_> = [papers.clone(), RS.to_string()]
        .iter()
        .map(|program| {
            let s = Sandbox::new();
            library(&s, &papers);
            let said: Vec<String> = [
                vec!["history", "--revert", "3"],
                vec!["history", "--revert", "4"],
                vec!["history", "--revert", "5", "--on-duplicate", "keep"],
                vec!["history", "--revert", "6", "--dry-run"],
                vec!["history"],
                vec!["undo", "-n", "3"],
                vec!["redo", "-n", "3"],
                vec!["history", "--revert", "2"],
                vec!["history"],
            ]
            .iter()
            .map(|args| s.says(program, "lib", args))
            .collect();
            (said, s.snapshot())
        })
        .collect();
    assert_eq!(runs[0].0, runs[1].0, "what they print");
    assert_same(&runs[0].1, &runs[1].1, "the files after");
}

// a pool that moved on
// --------------------

/// A library with a pool, made by papers: the add and the pushes write the pool, so their steps link to its steps.
fn shallow(s: &Sandbox, papers: &str) {
    s.run(papers, ".", &["init", "pool"]);
    s.run(papers, ".", &["init", "lib", "--pool", "pool"]);
    s.run(papers, "lib", &["add", "../refs.bib"]);
    // a step of the library that changes the pool
    s.run(
        papers,
        "lib",
        &["edit", "doe2000", "--set", "note=from the library"],
    );
    s.run(papers, "lib", &["push", "doe2000"]);
    assert!(
        s.read("pool/papers.bib").contains("from the library"),
        "{}",
        s.read("pool/papers.bib")
    );
}

/// What the pool does after that, so that it moved on.
fn pool_moves_on(s: &Sandbox, papers: &str, how: &str) {
    match how {
        // an entry the add put there changes, and so does the one the push changed
        "conflict" => {
            s.run(
                papers,
                "pool",
                &["edit", "doe_roe2000", "--set", "note=from the pool"],
            );
            s.run(papers, "pool", &["tag", "roe2001", "--add", "pooltag"]);
        }
        "no conflict" => {
            s.run(papers, "pool", &["tag", "roe2001", "--add", "pooltag"]);
            s.run(
                papers,
                "pool",
                &["edit", "doe_roe2000", "--set", "year=2001"],
            );
        }
        // removed in the pool
        "removed" => {
            s.run(papers, "pool", &["rm", "roe2001"]);
        }
        _ => {}
    }
}

#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test revert_interop -- --ignored"]
fn undo_and_redo_revert_a_pool_that_moved_on_as_papers_does() {
    let Some(papers) = reference() else { return };
    for how in ["conflict", "no conflict", "removed", "unchanged"] {
        let runs: Vec<_> = [papers.clone(), RS.to_string()]
            .iter()
            .map(|program| {
                let s = Sandbox::new();
                shallow(&s, &papers);
                pool_moves_on(&s, &papers, how);
                let mut said = Vec::new();
                for (cwd, args) in [
                    ("lib", vec!["history"]),
                    ("pool", vec!["history"]),
                    ("lib", vec!["undo"]),
                    ("pool", vec!["history"]),
                    ("lib", vec!["undo"]),
                    ("pool", vec!["history"]),
                    ("lib", vec!["redo"]),
                    ("pool", vec!["history"]),
                    ("lib", vec!["redo", "-n", "2"]),
                    ("pool", vec!["history"]),
                    ("lib", vec!["history"]),
                ] {
                    said.push(s.says(program, cwd, &args));
                }
                (said, s.snapshot())
            })
            .collect();
        for (i, (a, b)) in runs[0].0.iter().zip(&runs[1].0).enumerate() {
            assert_eq!(a, b, "{how}: what they print, command {i}");
        }
        assert_same(&runs[0].1, &runs[1].1, how);
        if std::env::var_os("PAPERS_RS_SHOW").is_some() {
            eprintln!("=== {how}\n{}", runs[1].0.join("---\n"));
        }
        // the pool did get steps of its own, when it moved on
        if how != "unchanged" {
            assert!(
                runs[1]
                    .0
                    .iter()
                    .any(|t| t.contains("changed since") || t.contains("also undone")),
                "{how}: {:?}",
                runs[1].0
            );
        }
    }
}

/// Several steps of the library undone at once, the steps of the pool reverted in one new step (and the pool and the
/// library themselves changed outside: papers leaves the pool alone).
#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test revert_interop -- --ignored"]
fn several_steps_undone_at_once_revert_the_pool_in_one_step() {
    let Some(papers) = reference() else { return };
    for how in ["no conflict", "removed", "conflict"] {
        let runs: Vec<_> = [papers.clone(), RS.to_string()]
            .iter()
            .map(|program| {
                let s = Sandbox::new();
                shallow(&s, &papers);
                s.run(&papers, "lib", &["tag", "roe2001", "--add", "libtag"]);
                s.run(&papers, "lib", &["push", "roe2001"]);
                pool_moves_on(&s, &papers, how);
                let mut said = Vec::new();
                for (cwd, args) in [
                    ("lib", vec!["undo", "-n", "3"]),
                    ("pool", vec!["history"]),
                    ("pool", vec!["list", "-K"]),
                    ("lib", vec!["redo", "-n", "2"]),
                    ("pool", vec!["history"]),
                    ("lib", vec!["undo", "--library-only"]),
                    ("pool", vec!["history"]),
                    ("lib", vec!["redo", "-n", "5"]),
                    ("pool", vec!["history"]),
                    ("lib", vec!["history"]),
                ] {
                    said.push(s.says(program, cwd, &args));
                }
                (said, s.snapshot())
            })
            .collect();
        for (i, (a, b)) in runs[0].0.iter().zip(&runs[1].0).enumerate() {
            assert_eq!(a, b, "{how}: what they print, command {i}");
        }
        assert_same(&runs[0].1, &runs[1].1, how);
        if std::env::var_os("PAPERS_RS_SHOW").is_some() {
            eprintln!("=== {how}\n{}", runs[1].0.join("---\n"));
        }
    }
}

#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test revert_interop -- --ignored"]
fn a_pool_step_reverted_by_one_program_is_undone_by_the_other() {
    let Some(papers) = reference() else { return };
    let s = Sandbox::new();
    shallow(&s, &papers);
    pool_moves_on(&s, &papers, "no conflict");
    let pool_before = s.read("pool/papers.bib");
    // papers-rs reverts the pool's step by its undo, papers undoes the pool (-C pool undo) and redoes it
    s.run(RS, "lib", &["undo"]);
    let reverted = s.read("pool/papers.bib");
    assert_ne!(reverted, pool_before);
    s.run(&papers, "pool", &["undo"]);
    assert_eq!(
        s.read("pool/papers.bib"),
        pool_before,
        "papers undoes the revert that papers-rs made"
    );
    s.run(&papers, "pool", &["redo"]);
    assert_eq!(s.read("pool/papers.bib"), reverted);
    // and papers-rs redoes the library step: the pool takes the revert back
    s.run(RS, "lib", &["redo"]);
}
