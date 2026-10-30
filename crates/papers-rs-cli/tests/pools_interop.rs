//! Pools and the exchanges with them: papers and papers-rs on the same libraries (the reference papers is
//! `PAPERS_REFERENCE`, a command):
//!
//! ```text
//! PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test pools_interop -- --ignored
//! ```
//!
//! - what one records (the origin records, the times of the exchanges, `exchanges.json`) the other reads: a pull
//!   by papers-rs is "up to date" for papers, what papers took from the pool papers-rs pushes on, and the
//!   other way round;
//! - the same commands by both on copies of the same libraries print the same and leave the same records;
//! - an undo by one takes back the exchange that the other made, in the library and in its pool.
//!
//! (`design/cases/exchange/*` has the cases of the exchanges, recorded from papers.)

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::json;

const RS: &str = env!("CARGO_BIN_EXE_papers-rs");

const POOL: &str = "@article{smith2020,\n author = {Smith, John and Doe, Jane},\n doi = {10.1000/abc},\n journal = {Nature Geo},\n title = {A title of a paper},\n year = {2020}\n}\n\n@article{brown2021,\n author = {Brown, Bob},\n title = {Another paper},\n year = {2021}\n}\n\n@article{jones2019,\n author = {Jones, Mary},\n doi = {10.1000/xyz},\n keywords = {ice},\n title = {Ice sheets},\n year = {2019}\n}\n";

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

impl Sandbox {
    /// A pool with its entries and a shallow library, written as papers writes them, in a sandbox with the history
    /// of the machine in it.
    fn new() -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let s = Sandbox { _tmp: tmp, root };
        let config = config_dir(&s.path("home"));
        fs::create_dir_all(&config).unwrap();
        fs::write(
            format!("{config}/config.json"),
            json!({"history-dir": s.path("history"), "version": 3}).to_string(),
        )
        .unwrap();
        s.write("pool/.papers/config.json", "{\n  \"bibtex\": \"papers.bib\",\n  \"filesdir\": \"files\",\n  \"pool\": \"none\",\n  \"version\": 3\n}");
        s.write("pool/papers.bib", POOL);
        s.write(
            "pool/files/smith_doe_2020_a-title-of-a-paper.txt",
            "the file\n",
        );
        s.write(
            "pool/papers.bib",
            &POOL.replacen(
                "doi = {10.1000/abc},",
                "doi = {10.1000/abc},\n file = {:files/smith_doe_2020_a-title-of-a-paper.txt:txt},",
                1,
            ),
        );
        s.write("lib/.papers/config.json", "{\n  \"bibtex\": \"papers.bib\",\n  \"filesdir\": \"files\",\n  \"pool\": \"../pool\",\n  \"version\": 3\n}");
        s.write("lib/papers.bib", "");
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
        std::thread::sleep(std::time::Duration::from_millis(1100));
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

    /// What a command prints (exit status, standard output, standard error), with the sandbox and the times left out.
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

    fn run(&self, program: &str, cwd: &str, args: &[&str]) -> String {
        let says = self.says(program, cwd, args);
        assert!(says.starts_with("exit 0\n"), "{program} {args:?}: {says}");
        says
    }

    fn normalize(&self, text: &str) -> String {
        let mut out = text.replace(&self.root, "$ROOT");
        // the libraries have identifiers of their own (a pool id), and the origin files are named by a hash of it
        for sub in ["lib", "pool"] {
            if let Some(id) = self.library_id(sub) {
                out = out.replace(&id, "$ID");
            }
        }
        for (key, placeholder) in [("papers-pulled ", "$N"), ("papers-time = {", "$T")] {
            let mut done = String::new();
            let mut rest = out.as_str();
            while let Some(at) = rest.find(key) {
                done.push_str(&rest[..at + key.len()]);
                done.push_str(placeholder);
                let tail = &rest[at + key.len()..];
                let end = tail.find(['\n', '}']).unwrap_or(tail.len());
                rest = &tail[end..];
            }
            done.push_str(rest);
            out = done;
        }
        out
    }

    /// The identifier of a library (its `id`), once a command recorded a step in its history.
    fn library_id(&self, sub: &str) -> Option<String> {
        let config: serde_json::Value =
            serde_json::from_str(&self.read(&format!("{sub}/.papers/config.json"))).ok()?;
        config["id"].as_str().map(str::to_string)
    }

    /// The records of the library: the files of `.papers/origin`, by name.
    fn records(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        if let Ok(read) = fs::read_dir(self.path("lib/.papers/origin")) {
            for e in read.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                // `pool-0fea92.bib`: the hash is of the pool id
                let shown = format!(
                    "{}-$H.bib",
                    name.trim_end_matches(".bib")
                        .rsplit_once('-')
                        .map_or("", |x| x.0)
                );
                out.insert(
                    shown,
                    self.normalize(&self.read(&format!("lib/.papers/origin/{name}"))),
                );
            }
        }
        out
    }
}

fn reference() -> Option<String> {
    let reference = std::env::var("PAPERS_REFERENCE").ok();
    if reference.is_none() {
        eprintln!("skipped: PAPERS_REFERENCE is not set");
    }
    reference
}

/// The same exchanges by papers and by papers-rs, on two sandboxes that start alike, say and leave the same.
#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test pools_interop -- --ignored"]
fn both_programs_exchange_alike() {
    let Some(papers) = reference() else { return };
    let (a, b) = (Sandbox::new(), Sandbox::new());
    let steps: &[&[&str]] = &[
        &["status"],
        &["pull", "--check"],
        &["pull", "--all"],
        &["status"],
        &["edit", "smith2020", "--set", "note=library note"],
        &["tag", "jones2019", "--add", "x"],
        &["status"],
        &["push"],
        &["pull", "--check"],
        &["add", "--key", "brown2021"],
        &["sync"],
        &["status"],
    ];
    for step in steps {
        let args: Vec<&str> = step.to_vec();
        let (x, y) = (a.says(&papers, "lib", &args), b.says(RS, "lib", &args));
        assert_eq!(x, y, "papers {args:?}");
        // the pool is edited by papers in both, in the middle of the sequence
        if args == ["status"] && a.read("lib/papers.bib").contains("library note") {
            a.run(
                &papers,
                "pool",
                &["edit", "brown2021", "--set", "year=2022"],
            );
            b.run(
                &papers,
                "pool",
                &["edit", "brown2021", "--set", "year=2022"],
            );
        }
    }
    assert_eq!(a.records(), b.records());
    assert_eq!(
        a.normalize(&a.read("lib/papers.bib")),
        b.normalize(&b.read("lib/papers.bib"))
    );
    assert_eq!(
        a.normalize(&a.read("pool/papers.bib")),
        b.normalize(&b.read("pool/papers.bib"))
    );
}

/// papers-rs records, papers reads: a pull of papers-rs is up to date for papers, and the other way round.
#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test pools_interop -- --ignored"]
fn what_one_records_the_other_reads() {
    let Some(papers) = reference() else { return };
    let s = Sandbox::new();
    s.run(RS, "lib", &["pull", "--all"]);
    for command in [&papers, RS] {
        let says = s.says(command, "lib", &["pull", "--check"]);
        assert!(
            says.contains("pool ../pool: up to date"),
            "{command}: {says}"
        );
        assert!(
            s.says(command, "lib", &["status"]).contains("(in sync)"),
            "{command}"
        );
        assert!(
            s.says(command, "lib", &["push"])
                .contains("nothing to push to pool ../pool"),
            "{command}"
        );
    }
    // the pool changes (by papers); papers-rs takes it, papers pushes the library's change back, papers-rs sees it
    s.run(
        &papers,
        "pool",
        &["edit", "smith2020", "--set", "journal=Pool Journal"],
    );
    assert!(
        s.says(RS, "lib", &["pull", "--check"])
            .contains("smith2020: changed in pool ../pool: journal")
    );
    s.run(RS, "lib", &["pull"]);
    s.run(
        &papers,
        "lib",
        &["edit", "jones2019", "--set", "note=from papers"],
    );
    assert!(s.says(RS, "lib", &["status"]).contains("(library changed)"));
    s.run(&papers, "lib", &["push"]);
    assert!(s.read("pool/papers.bib").contains("note = {from papers}"));
    assert!(s.says(RS, "lib", &["status"]).contains("(in sync)"));
    assert!(s.says(&papers, "lib", &["status"]).contains("(in sync)"));
    // exchanges.json: written by papers-rs, trusted by papers (and the other way round: the same text)
    let files: Vec<String> = fs::read_dir(s.path("history/projects"))
        .unwrap()
        .flatten()
        .map(|e| {
            e.path()
                .join("exchanges.json")
                .to_string_lossy()
                .into_owned()
        })
        .filter(|p| Path::new(p).exists())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    let by_rs = fs::read_to_string(&files[0]).unwrap();
    // the pool had no identifier at the first status (its id was its path), then it had (a push recorded a step)
    assert!(
        by_rs.starts_with("{\"../pool\": {\"library\": [\""),
        "{by_rs}"
    );
    let id = s.library_id("pool").expect("the identifier of the pool");
    let of_id = |text: &str| {
        let at = text
            .find(&format!("\"{id}\": "))
            .expect("the pieces of the pool");
        text[at..].to_string()
    };
    fs::remove_file(&files[0]).unwrap();
    s.run(&papers, "lib", &["status"]);
    let by_papers = fs::read_to_string(&files[0]).unwrap();
    assert_eq!(by_papers, format!("{{{}", of_id(&by_rs)));
}

/// The undo of one program takes back the exchange of the other, in the library and in the pool.
#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test pools_interop -- --ignored"]
fn an_undo_takes_back_the_exchange_of_the_other_program() {
    let Some(papers) = reference() else { return };
    let new =
        "@article{fresh,\n author = {Fresh, Fred},\n title = {A fresh paper},\n year = {2024}\n}\n";
    for (adds, undoes) in [(&papers, RS), (&RS.to_string(), &papers)] {
        let s = Sandbox::new();
        s.write("new.bib", new);
        s.run(adds, "lib", &["add", "../new.bib"]);
        assert!(s.read("lib/papers.bib").contains("@article{fresh,"));
        assert!(s.read("pool/papers.bib").contains("A fresh paper"));
        s.run(undoes, "lib", &["undo"]);
        assert!(!s.read("lib/papers.bib").contains("@article{fresh,"));
        assert!(
            !s.read("pool/papers.bib").contains("A fresh paper"),
            "{undoes} left the pool"
        );
        s.run(adds, "lib", &["redo"]);
        assert!(s.read("pool/papers.bib").contains("A fresh paper"));
    }
}

/// A file link that the pool removed: `status` says `pool changed`, a push does not add it back to the pool (the
/// record keeps it), and the next pull removes it from the library, which then has no `file` field; the same by both.
#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=papers cargo test -p papers-rs-cli --test pools_interop -- --ignored"]
fn a_link_that_the_pool_removed_is_not_pushed_back() {
    let Some(papers) = reference() else { return };
    let (a, b) = (Sandbox::new(), Sandbox::new());
    for (s, program) in [(&a, papers.as_str()), (&b, RS)] {
        s.run(program, "lib", &["pull", "--all"]);
        assert!(
            s.read("lib/papers.bib")
                .contains("smith_doe_2020_a-title-of-a-paper.txt")
        );
        // the pool removes the link
        s.write(
            "pool/papers.bib",
            &POOL.replace(
                "doi = {10.1000/abc},",
                "doi = {10.1000/abc},\n note = {pool note},",
            ),
        );
    }
    let steps: &[&[&str]] = &[
        &["status"],
        &["edit", "smith2020", "--set", "journal=Library Journal"],
        &["status"],
        &["push"],
        &["status"],
        &["pull", "--check"],
        &["pull"],
        &["status"],
    ];
    for step in steps {
        let (x, y) = (a.says(&papers, "lib", step), b.says(RS, "lib", step));
        assert_eq!(x, y, "papers {step:?}");
    }
    assert!(!a.read("pool/papers.bib").contains("smith_doe_2020"));
    assert!(
        !a.read("lib/papers.bib").contains("file = "),
        "{}",
        a.read("lib/papers.bib")
    );
    assert_eq!(a.records(), b.records());
    assert_eq!(
        a.normalize(&a.read("lib/papers.bib")),
        b.normalize(&b.read("lib/papers.bib"))
    );
    assert_eq!(
        a.normalize(&a.read("pool/papers.bib")),
        b.normalize(&b.read("pool/papers.bib"))
    );
}
