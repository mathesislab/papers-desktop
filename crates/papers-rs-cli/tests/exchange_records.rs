//! The records of the exchanges through the command line (spec `exchange.md` sections 2.3, 3, 4.1 and 14): the
//! pool id (the identifier of the pool's library, or the path of the pool), the origin files named after the pool
//! folder, the records that move from a path id to the identifier, the rules of reading and writing them, the
//! keys renamed, and what `status` sees and does not write. (`tests/pools.rs` of papers-rs has the rules without
//! the command line; `spec/cases/exchange/*` the cases recorded from papers.)

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const IDENT: &str = "7e5754a0ccd246968e910bcd2a16dcf2";

const POOL_BIB: &str = "@article{smith2020,\n author = {Smith, John and Doe, Jane},\n doi = {10.1000/abc},\n journal = {Nat.},\n title = {Climate sensitivity},\n year = {2020}\n}\n\n@article{brown2021,\n author = {Brown, Bob},\n title = {Ocean heat},\n year = {2021}\n}\n";

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: String,
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

impl Sandbox {
    /// A pool with two entries, and a self-contained library `lib` (a pool of its own is given with `--pool`).
    fn new() -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
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
        s.ok(".", &["init", "pool"]);
        s.write("pool/papers.bib", POOL_BIB);
        s.ok(".", &["init", "lib"]);
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
        assert_eq!(
            output.status.code(),
            Some(0),
            "{args:?}: {}",
            text(&output.stderr)
        );
        text(&output.stdout)
    }

    /// `papers` in `lib`.
    fn lib(&self, args: &[&str]) -> String {
        self.ok("lib", args)
    }

    /// The names of the origin files of `lib`.
    fn origin_names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.path("lib/.papers/origin"))
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// The text of the only origin file of `lib`.
    fn origin_text(&self) -> String {
        let names = self.origin_names();
        assert_eq!(names.len(), 1, "{names:?}");
        self.read(&format!("lib/.papers/origin/{}", names[0]))
    }

    /// The origin files of `lib`, by name.
    fn origin_files(&self) -> Vec<(String, String)> {
        self.origin_names()
            .into_iter()
            .map(|n| {
                let t = self.read(&format!("lib/.papers/origin/{n}"));
                (n, t)
            })
            .collect()
    }

    /// Give the library in `dir` the identifier `id`.
    fn set_id(&self, dir: &str, id: &str) {
        let file = self.path(&format!("{dir}/.papers/config.json"));
        let mut config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        config["id"] = id.into();
        fs::write(file, config.to_string()).unwrap();
    }

    fn pool_ids_of_the_records(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .origin_files()
            .iter()
            .filter_map(|(_, t)| {
                t.lines()
                    .next()
                    .and_then(|l| l.strip_prefix("@comment{papers-pool "))
                    .map(|l| l.trim_end_matches('}').to_string())
            })
            .collect();
        ids.sort();
        ids
    }
}

fn sha1_6(id: &str) -> String {
    papers_rs::pools::origin::sha1_hex(id.as_bytes())[..6].to_string()
}

#[test]
fn only_32_lower_case_hexadecimal_digits_are_a_library_identifier() {
    let s = Sandbox::new();
    s.set_id("pool", &IDENT.to_uppercase());
    s.lib(&["pull", "--pool", "../pool", "smith2020"]);
    assert_eq!(s.pool_ids_of_the_records(), ["../pool"]);
    fs::remove_dir_all(s.path("lib/.papers/origin")).unwrap();
    s.set_id("pool", IDENT);
    s.lib(&["pull", "--pool", "../pool", "smith2020"]);
    assert_eq!(s.pool_ids_of_the_records(), [IDENT]);
}

#[test]
fn an_other_library_is_named_by_its_path_relative_to_the_library() {
    let s = Sandbox::new();
    s.lib(&["pull", "--pool", &s.path("pool"), "smith2020"]);
    assert_eq!(s.pool_ids_of_the_records(), ["../pool"]);
    s.ok("lib", &["init", "sub"]);
    s.write(
        "lib/sub/papers.bib",
        "@article{mine2022,\n author = {Mine, Ann},\n title = {Mine},\n year = {2022}\n}\n",
    );
    s.lib(&["pull", "--pool", "sub", "mine2022"]);
    assert_eq!(s.pool_ids_of_the_records(), ["../pool", "./sub"]);
}

#[test]
fn a_pool_without_identifier_has_the_first_path_id_of_its_records() {
    let s = Sandbox::new();
    let record = |pool: &str, key: &str| {
        format!(
            "@comment{{papers-pool {pool}}}\n\n@article{{{key},\n papers-pool-key = {{{key}}},\n title = {{T}}\n}}\n"
        )
    };
    s.write("lib/.papers/origin/a.bib", &record("../pool/", "brown2021"));
    s.write("lib/.papers/origin/b.bib", &record("../pool", "other"));
    let before = s.read("lib/.papers/origin/a.bib");
    s.lib(&["pull", "smith2020", "--pool", "../pool"]);
    assert_eq!(
        s.read("lib/.papers/origin/a.bib"),
        before,
        "the records of ../pool/ are not touched"
    );
    // the record of smith2020 is under ../pool (before ../pool/), written under the name of its pool id
    let own = s.read(&format!(
        "lib/.papers/origin/pool-{}.bib",
        sha1_6("../pool")
    ));
    assert!(
        own.starts_with("@comment{papers-pool ../pool}") && own.contains("@article{smith2020,"),
        "{own}"
    );
    assert!(!own.contains("brown2021"));
}

#[test]
fn a_pool_gets_an_identifier_at_its_first_step_and_the_next_exchange_moves_the_records() {
    let s = Sandbox::new();
    s.ok("lib", &["config", "pool", "../pool", "-y"]);
    s.lib(&["pull", "smith2020"]);
    assert_eq!(s.pool_ids_of_the_records(), ["../pool"]);
    assert!(!s.read("pool/.papers/config.json").contains("\"id\""));
    // a push records a step in the history of the pool: it has an identifier
    s.lib(&["edit", "smith2020", "--set", "note=x"]);
    s.lib(&["push"]);
    let config: serde_json::Value =
        serde_json::from_str(&s.read("pool/.papers/config.json")).unwrap();
    let id = config["id"].as_str().expect("the identifier").to_string();
    assert_eq!(s.pool_ids_of_the_records(), ["../pool"]);
    s.lib(&["pull"]);
    assert_eq!(s.pool_ids_of_the_records(), std::slice::from_ref(&id));
    assert_eq!(s.origin_names(), [format!("pool-{}.bib", sha1_6(&id))]);
}

#[test]
fn the_records_of_a_path_id_move_to_the_identifier() {
    let s = Sandbox::new();
    s.lib(&["pull", "--pool", "../pool", "smith2020"]);
    s.lib(&["pull", "--pool", "../pool"]);
    let pulled = s
        .origin_text()
        .lines()
        .find(|l| l.starts_with("@comment{papers-pulled "))
        .unwrap()
        .to_string();
    s.set_id("pool", IDENT);
    s.lib(&["pull", "--pool", "../pool"]);
    assert_eq!(s.origin_names(), [format!("pool-{}.bib", sha1_6(IDENT))]);
    let moved = s.origin_text();
    assert!(
        moved.starts_with(&format!(
            "@comment{{papers-pool {IDENT}}}\n\n{pulled}\n\n@comment{{papers-pool-path "
        )),
        "{moved}"
    );
    assert!(
        moved.contains(&format!("@comment{{papers-pool-path {}}}", s.path("pool"))),
        "{moved}"
    );
    assert!(moved.contains("@article{smith2020,"));
}

#[test]
fn the_file_of_a_pool_is_named_after_its_folder_and_follows_the_pool() {
    let s = Sandbox::new();
    s.set_id("pool", IDENT);
    s.lib(&["pull", "--pool", "../pool", "smith2020"]);
    assert_eq!(s.origin_names(), [format!("pool-{}.bib", sha1_6(IDENT))]);
    fs::rename(s.path("pool"), s.path("sealevel")).unwrap();
    s.lib(&["pull", "--pool", "../sealevel"]);
    let name = format!("sealevel-{}.bib", sha1_6(IDENT));
    assert_eq!(s.origin_names(), [name]);
    assert!(s.origin_text().contains(&format!(
        "@comment{{papers-pool-path {}}}",
        s.path("sealevel")
    )));
    // the records are found again: nothing to pull
    assert!(
        s.lib(&["pull", "--pool", "../sealevel"])
            .contains("nothing to pull")
    );
}

#[test]
fn a_path_id_that_does_not_designate_the_pool_stays() {
    let s = Sandbox::new();
    s.ok("lib", &["init", "other"]);
    s.write(
        "lib/other/papers.bib",
        "@article{mine2022,\n author = {Mine, Ann},\n title = {Mine},\n year = {2022}\n}\n",
    );
    s.lib(&["pull", "--pool", "other", "mine2022"]);
    s.lib(&["pull", "--pool", "../pool", "smith2020"]);
    s.set_id("pool", IDENT);
    s.lib(&["pull", "--pool", "../pool"]);
    assert_eq!(s.pool_ids_of_the_records(), ["./other", IDENT]);
}

#[test]
fn status_does_not_write_the_move_of_the_records() {
    let s = Sandbox::new();
    s.ok("lib", &["config", "pool", "../pool", "-y"]);
    s.lib(&["pull", "smith2020"]);
    s.lib(&["pull"]);
    s.set_id("pool", IDENT);
    let before = s.origin_files();
    let status = s.lib(&["status", "--no-check-files"]);
    assert!(status.contains("../pool (in sync)"), "{status}");
    s.write(
        "lib/papers.bib",
        &s.read("lib/papers.bib")
            .replace(" journal = {Nat.},", " journal = {Nature},"),
    );
    let status = s.lib(&["status", "--no-check-files"]);
    assert!(status.contains("../pool (library changed)"), "{status}");
    assert_eq!(s.origin_files(), before);
    assert_eq!(s.pool_ids_of_the_records(), ["../pool"]);
}

#[test]
fn origin_files_are_read_as_utf8_without_the_byte_order_mark_and_merged_by_pool_id() {
    let s = Sandbox::new();
    fs::create_dir_all(s.path("lib/.papers/origin")).unwrap();
    let a = "\u{feff}@comment{papers-pool ../pool}\n\n@comment{papers-pulled 5}\n\n@article{Key,\n papers-pool-key = {A},\n title = {Été}\n}\n";
    let b = "@comment{papers-pool ../pool}\n\n@article{KEY,\n papers-pool-key = {B},\n title = {B}\n}\n\n@article{other,\n papers-pool-key = {O},\n title = {O}\n}\n";
    fs::write(s.path("lib/.papers/origin/a.bib"), a).unwrap();
    fs::write(s.path("lib/.papers/origin/b.bib"), b).unwrap();
    fs::write(s.path("lib/.papers/origin/notes.txt"), "not an origin file").unwrap();
    // the files of one pool id are merged (the last read wins), and when the pool is written, its own file stays
    s.lib(&["pull", "--pool", "../pool", "smith2020"]);
    let names = s.origin_names();
    assert_eq!(
        names,
        [
            "notes.txt".to_string(),
            format!("pool-{}.bib", sha1_6("../pool"))
        ]
    );
    let text = s.read(&format!("lib/.papers/origin/{}", names[1]));
    assert!(
        text.starts_with("@comment{papers-pool ../pool}\n\n@comment{papers-pulled 5}\n\n"),
        "{text}"
    );
    assert!(
        text.contains("@article{KEY,") && !text.contains("@article{Key,"),
        "{text}"
    );
    assert!(
        text.contains("papers-pool-key = {B}") && text.contains("@article{other,"),
        "{text}"
    );
}

#[test]
fn a_record_has_no_ids_and_the_pool_entry_is_found_by_the_pool_key_only_for_the_same_authors_and_year()
 {
    let s = Sandbox::new();
    s.lib(&["pull", "--pool", "../pool", "--all"]);
    // ids of a record (written by another build) are no part of the base
    let name = s.origin_names()[0].clone();
    let file = format!("lib/.papers/origin/{name}");
    s.write(
        &file,
        &s.read(&file)
            .replacen(" journal = ", " ids = {x},\n journal = ", 1),
    );
    assert!(
        s.lib(&["pull", "--pool", "../pool"])
            .contains("nothing to pull")
    );
    // the pool replaced the paper under the key smith2020 by another one without DOI: not the same paper
    let other = "@article{smith2020,\n author = {Jones, Mary},\n title = {Another paper},\n year = {2020}\n}\n";
    s.write(
        "lib/papers.bib",
        &s.read("lib/papers.bib")
            .replace(" doi = {10.1000/abc},\n", "")
            .replace("Climate sensitivity", "Library title"),
    );
    s.write(
        "pool/papers.bib",
        &format!("{other}\n{}", POOL_BIB.split("\n\n").nth(1).unwrap()),
    );
    let check = s.lib(&["pull", "--check", "--pool", "../pool"]);
    assert!(
        check.contains("smith2020: removed from pool ../pool"),
        "{check}"
    );
}

#[test]
fn a_record_left_by_a_removed_entry_is_not_the_record_of_the_entry_renamed_onto_its_key() {
    let s = Sandbox::new();
    s.lib(&["pull", "--pool", "../pool", "--all"]);
    // an entry that has a record of the pool is removed; another one is renamed onto its key: no record of its own
    s.lib(&[
        "add",
        "--key",
        "mine2022",
        "--title",
        "Mine",
        "--author",
        "Mine, Ann",
        "--year",
        "2022",
    ]);
    s.lib(&["rm", "brown2021", "-y"]);
    s.lib(&["edit", "mine2022", "--key", "brown2021"]);
    let text = s.origin_text();
    assert!(!text.contains("Ocean heat"), "{text}");
    assert!(!text.contains("@article{brown2021,"), "{text}");
    let check = s.lib(&["pull", "--check", "--pool", "../pool"]);
    assert!(!check.contains("removed from pool"), "{check}");
}

#[test]
fn a_key_renamed_in_a_shallow_library_keeps_its_record() {
    let s = Sandbox::new();
    s.ok("lib", &["config", "pool", "../pool", "-y"]);
    s.lib(&["pull", "smith2020"]);
    s.lib(&["edit", "smith2020", "--key", "smith2020b"]);
    let text = s.origin_text();
    assert!(
        text.contains("@article{smith2020b,") && text.contains("papers-pool-key = {smith2020}"),
        "{text}"
    );
    assert!(s.lib(&["pull"]).contains("nothing to pull"));
}

#[test]
fn status_sees_a_change_of_entry_type_and_an_entry_removed_from_the_pool_and_says_when_a_pool_was_not_pulled_in_full()
 {
    let s = Sandbox::new();
    s.ok("lib", &["config", "pool", "../pool", "-y"]);
    s.lib(&["pull", "smith2020"]);
    let status = s.lib(&["status", "--no-check-files"]);
    assert!(
        status.contains("../pool (not pulled in full yet)"),
        "{status}"
    );
    assert!(
        status.ends_with("`papers pull` takes the changes of the pool\n"),
        "{status}"
    );
    assert!(
        s.lib(&["pull", "--check"])
            .contains("pool ../pool: not pulled in full yet")
    );
    s.lib(&["pull"]);
    assert!(
        s.lib(&["status", "--no-check-files"])
            .contains("../pool (in sync)")
    );
    // the entry type changed in the library: the fields are the same
    s.write(
        "lib/papers.bib",
        &s.read("lib/papers.bib")
            .replace("@article{smith2020", "@book{smith2020"),
    );
    let status = s.lib(&["status", "--no-check-files"]);
    assert!(status.contains("../pool (library changed)"), "{status}");
    s.lib(&["push"]);
    assert!(
        s.lib(&["status", "--no-check-files"])
            .contains("../pool (in sync)")
    );
    // the entry is removed from the pool, whose time stays: status sees it
    s.write("pool/papers.bib", POOL_BIB.split("\n\n").nth(1).unwrap());
    let status = s.lib(&["status", "--no-check-files"]);
    assert!(status.contains("../pool (pool changed)"), "{status}");
    // and wrote nothing in the library
    assert!(s.read("lib/papers.bib").contains("@book{smith2020"));
}

#[test]
fn the_origin_file_of_a_dry_run_and_of_the_legacy_file_is_not_written() {
    let s = Sandbox::new();
    s.lib(&["pull", "--pool", "../pool", "--all", "--dry-run"]);
    assert!(s.origin_names().is_empty());
    s.write(
        "lib/.papers/origin.bib",
        "@comment{papers-pull 12345 ../pool}\n\n@article{a2020,\n papers-pool = {../pool},\n papers-pool-key = {Legacy},\n title = {A}\n}\n",
    );
    s.lib(&["pull", "--pool", "../pool", "--dry-run"]);
    assert!(Path::new(&s.path("lib/.papers/origin.bib")).exists());
}
