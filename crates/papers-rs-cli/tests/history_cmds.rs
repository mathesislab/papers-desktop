//! `papers-rs undo`, `redo`, `restore`, `history` and `status`: what they print and leave on histories
//! that the library model records (through the API of `papers_rs::history`, as a command does), the
//! questions on a terminal, and, with the reference papers (ignored by default), the same sequences run
//! by papers (spec/cases has the cases recorded from papers).

use std::fs;
use std::fs::File;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};
use std::path::Path;
use std::process::{Command, Stdio};

use papers_rs::config::{Config, MachineConfig};
use papers_rs::history::{History, Step};

const MC: &str = if cfg!(target_os = "macos") {
    "Library/Application Support/papers/config.json"
} else {
    ".config/papers/config.json"
};

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

/// A home directory, a working directory and a history store, in a temporary directory.
struct Sandbox {
    _tmp: tempfile::TempDir,
    root: String,
}

fn entry(key: &str) -> String {
    format!("@article{{{key},\n title = {{Title of {key}}},\n author = {{Doe, Jane}},\n year = {{2000}},\n}}\n\n")
}

impl Sandbox {
    fn new() -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap().to_string_lossy().into_owned();
        let machine = format!("{root}/home/{MC}");
        fs::create_dir_all(Path::new(&machine).parent().unwrap()).unwrap();
        fs::write(&machine, format!("{{\n  \"history-dir\": \"{root}/store\",\n  \"version\": 3\n}}")).unwrap();
        fs::create_dir_all(format!("{root}/work")).unwrap();
        Sandbox { _tmp: tmp, root }
    }

    fn path(&self, rel: &str) -> String {
        format!("{}/work/{rel}", self.root)
    }

    fn command(&self, binary: &str, args: &[&str], cwd: &str) -> Command {
        let mut command = Command::new(binary);
        command
            .args(args)
            .current_dir(self.path(cwd))
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", format!("{}/home", self.root))
            .env("LC_ALL", "C.UTF-8")
            .env("TZ", "UTC")
            .env("NO_COLOR", "1")
            .env("COLUMNS", "80");
        command
    }

    fn run_with(&self, binary: &str, args: &[&str], cwd: &str) -> Out {
        let output = self.command(binary, args, cwd).stdin(Stdio::null()).output().unwrap();
        Out {
            code: output.status.code().unwrap_or(-1),
            stdout: self.show(&String::from_utf8_lossy(&output.stdout)),
            stderr: self.show(&String::from_utf8_lossy(&output.stderr)),
        }
    }

    fn run(&self, args: &[&str]) -> Out {
        self.run_with(env!("CARGO_BIN_EXE_papers-rs"), args, ".")
    }

    fn run_in(&self, cwd: &str, args: &[&str]) -> Out {
        self.run_with(env!("CARGO_BIN_EXE_papers-rs"), args, cwd)
    }

    /// Text as the tests compare it: the temporary directory is `$ROOT`, the times `$TIME`, the
    /// identifiers `$ID`.
    fn show(&self, text: &str) -> String {
        normalise(&text.replace(&self.root, "$ROOT"))
    }

    fn machine(&self) -> MachineConfig {
        MachineConfig::load(Some(&format!("{}/home/{MC}", self.root))).unwrap()
    }

    /// A library as `papers init` makes it, with a bibtex of its own.
    fn init(&self, dir: &str) -> Config {
        let out = self.run(&["init", dir]);
        assert_eq!(out.code, 0, "{}", out.stderr);
        fs::write(self.path(&format!("{dir}/papers.bib")), "").unwrap();
        self.config(dir)
    }

    fn config(&self, dir: &str) -> Config {
        Config::load(&self.path(&format!("{dir}/.papers/config.json")), self.machine(), None).unwrap()
    }

    /// A command that changes the bibtex of a library, as the library model records it.
    fn step(&self, dir: &str, message: &str, text: impl FnOnce(&str) -> String) {
        let mut config = self.config(dir);
        let before = History::before_command(&mut config);
        let mut history = History::open(&mut config, true, false).unwrap().unwrap();
        history.sync_head(before.as_ref()).unwrap();
        let bibtex = config.bibtex.clone().unwrap();
        let old = fs::read_to_string(&bibtex).unwrap();
        fs::write(&bibtex, text(&old)).unwrap();
        history.record(Step::new(message)).unwrap();
    }

    fn add(&self, dir: &str, key: &str) {
        self.step(dir, &format!("papers add ../{key}.bib"), |old| format!("{old}{}", entry(key)));
    }

    fn bibtex(&self, dir: &str) -> String {
        fs::read_to_string(self.path(&format!("{dir}/papers.bib"))).unwrap()
    }

    fn keys(&self, dir: &str) -> Vec<String> {
        let text = self.bibtex(dir);
        ["one2000", "two2000", "three2000"]
            .iter()
            .filter(|k| text.contains(&format!("{{{k},")))
            .map(|k| k.to_string())
            .collect()
    }
}

/// `($TIME` for `(2026-10-22 14:03`, `$ID` for 32 hexadecimal characters.
fn normalise(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let is_time = |i: usize| {
        let pattern = "(dddd-dd-dd dd:dd";
        chars.len() >= i + pattern.len()
            && pattern.chars().zip(&chars[i..]).all(|(p, c)| if p == 'd' { c.is_ascii_digit() } else { p == *c })
    };
    while i < chars.len() {
        if is_time(i) {
            out.push_str("($TIME");
            i += "(dddd-dd-dd dd:dd".len();
        } else if chars[i].is_ascii_hexdigit()
            && (i == 0 || !chars[i - 1].is_ascii_alphanumeric())
            && chars.len() >= i + 32
            && chars[i..i + 32].iter().all(|c| c.is_ascii_hexdigit())
            && chars.get(i + 32).is_none_or(|c| !c.is_ascii_alphanumeric())
        {
            out.push_str("$ID");
            i += 32;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn ok(out: Out) -> String {
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stderr, "");
    out.stdout
}

fn error(out: Out) -> String {
    assert_eq!(out.code, 1, "{}", out.stdout);
    assert_eq!(out.stdout, "");
    out.stderr
}

const STORE_LINE: &str = "history: $ROOT/store, 0.0 MB (up to 500 MB: the history-size setting; deleted files kept)";

// history
// =======

#[test]
fn history_lists_the_line_the_redo_list_and_the_store() {
    let b = Sandbox::new();
    b.init("lib");
    assert_eq!(
        ok(b.run_in("lib", &["history"])),
        format!("no history yet: each command that changes the library records a step\n{STORE_LINE}\n")
    );
    // nothing is written by a command that changes nothing
    assert!(!Path::new(&format!("{}/store", b.root)).exists());
    b.add("lib", "one2000");
    b.add("lib", "two2000");
    b.add("lib", "three2000");
    assert_eq!(
        ok(b.run_in("lib", &["history"])),
        format!(
            "> 4 papers add ../three2000.bib ($TIME)\n  3 papers add ../two2000.bib ($TIME)\n  2 papers add ../one2000.bib ($TIME)\n  1 the library before its first recorded command ($TIME)\n{STORE_LINE}\n"
        )
    );
    assert_eq!(
        ok(b.run_in("lib", &["history", "-n", "2"])),
        format!(
            "> 4 papers add ../three2000.bib ($TIME)\n  3 papers add ../two2000.bib ($TIME)\n  ... 2 older step(s) (-n 0 shows them all)\n{STORE_LINE}\n"
        )
    );
    // a negative number leaves out the oldest, as Python's slice does
    assert_eq!(
        ok(b.run_in("lib", &["history", "-n", "-3"])),
        format!("> 4 papers add ../three2000.bib ($TIME)\n  ... 3 older step(s) (-n 0 shows them all)\n{STORE_LINE}\n")
    );
    ok(b.run_in("lib", &["undo", "-n", "2"]));
    assert_eq!(
        ok(b.run_in("lib", &["history", "-n", "0"])),
        format!(
            "  4 papers add ../three2000.bib ($TIME)  (undone: `papers redo`)\n  3 papers add ../two2000.bib ($TIME)  (undone: `papers redo`)\n> 2 papers add ../one2000.bib ($TIME)\n  1 the library before its first recorded command ($TIME)\n{STORE_LINE}\n"
        )
    );
}

#[test]
fn history_of_the_machine_and_the_stale_backups() {
    let b = Sandbox::new();
    assert_eq!(
        ok(b.run(&["history"])),
        format!("no library: `papers history --all` lists the histories of the machine\n{STORE_LINE}\n")
    );
    assert_eq!(ok(b.run(&["history", "--all"])), "");
    b.init("lib");
    b.add("lib", "one2000");
    let all = ok(b.run(&["history", "--all"]));
    assert_eq!(all, "$ROOT/store: 0.0 MB\n* $ID: 2 steps, 0.0 MB of its own, last used at $ROOT/work/lib/papers.bib\n");
    // the backups of earlier versions are listed, never deleted
    fs::create_dir_all(b.path("lib/.papers/backup/.git")).unwrap();
    fs::write(b.path("lib/.papers/backup/papers.bib"), "x".repeat(2000)).unwrap();
    let listing = ok(b.run_in("lib", &["history"]));
    assert!(
        listing.ends_with(
            "stale: $ROOT/work/lib/.papers/backup (backup of an earlier papers, 0.0 MB, no longer used) \u{2014} delete with: rm -rf $ROOT/work/lib/.papers/backup\n"
        ),
        "{listing}"
    );
    assert!(Path::new(&b.path("lib/.papers/backup")).is_dir());
}

#[test]
fn the_deprecated_backup_says_so() {
    let b = Sandbox::new();
    let out = b.run(&["backup"]);
    assert_eq!(out.code, 0);
    assert_eq!(out.stderr, "papers: warning: `papers backup` is deprecated: use `papers history`\n");
    assert!(out.stdout.starts_with("no library: "));
}

// undo, redo, restore
// ===================

#[test]
fn undo_redo_and_restore_move_the_bibtex() {
    let b = Sandbox::new();
    b.init("lib");
    for key in ["one2000", "two2000", "three2000"] {
        b.add("lib", key);
    }
    assert_eq!(ok(b.run_in("lib", &["undo"])), "undo: papers.bib restored to 3 papers add ../two2000.bib ($TIME)\n");
    assert_eq!(b.keys("lib"), ["one2000", "two2000"]);
    assert_eq!(
        ok(b.run_in("lib", &["undo", "--steps", "2"])),
        "undo: papers.bib restored to 1 the library before its first recorded command ($TIME)\n"
    );
    assert!(b.keys("lib").is_empty());
    assert_eq!(
        ok(b.run_in("lib", &["redo", "-n", "2"])),
        "redo: papers.bib restored to 3 papers add ../two2000.bib ($TIME)\n"
    );
    assert_eq!(b.keys("lib"), ["one2000", "two2000"]);
    assert_eq!(
        ok(b.run_in("lib", &["restore", "4"])),
        "restore: papers.bib restored to 4 papers add ../three2000.bib ($TIME)\n"
    );
    assert_eq!(b.keys("lib").len(), 3);
    assert_eq!(
        ok(b.run_in("lib", &["restore", "--ref", "2"])),
        "restore: papers.bib restored to 2 papers add ../one2000.bib ($TIME)\n"
    );
    assert_eq!(b.keys("lib"), ["one2000"]);
    // the lock of the library is released
    assert!(!Path::new(&b.path("lib/.papers/lock")).exists());
    // `restore` without a number is the current state: nothing changed since
    assert_eq!(error(b.run_in("lib", &["restore"])), "papers: error: nothing to restore: papers.bib is step 2\n");
    // the deprecated name
    let out = b.run_in("lib", &["restore-backup", "3"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stderr, "papers: warning: `papers restore-backup` is deprecated: use `papers restore`\n");
    assert_eq!(b.keys("lib"), ["one2000", "two2000"]);
}

#[test]
fn what_cannot_be_undone() {
    let b = Sandbox::new();
    b.init("lib");
    assert_eq!(error(b.run_in("lib", &["undo"])), "papers: error: nothing to undo\n");
    assert_eq!(error(b.run_in("lib", &["redo"])), "papers: error: nothing to redo\n");
    b.add("lib", "one2000");
    assert_eq!(error(b.run_in("lib", &["undo", "-n", "5"])), "papers: error: only 1 step(s) to undo\n");
    assert_eq!(error(b.run_in("lib", &["redo", "-n", "5"])), "papers: error: nothing to redo\n");
    ok(b.run_in("lib", &["undo"]));
    assert_eq!(error(b.run_in("lib", &["redo", "-n", "5"])), "papers: error: only 1 step(s) to redo\n");
    assert_eq!(
        error(b.run_in("lib", &["restore", "99"])),
        "papers: error: no step 99 in the history of the library (see `papers history`)\n"
    );
    assert_eq!(
        error(b.run_in("lib", &["restore", "-1"])),
        "papers: error: no step -1 in the history of the library (see `papers history`)\n"
    );
    // a number of steps that Python's indexing turns into something else: not followed
    assert!(error(b.run_in("lib", &["redo", "-n", "0"])).ends_with("not implemented in papers-rs yet\n"));
    assert!(error(b.run_in("lib", &["undo", "-n", "-1"])).ends_with("not implemented in papers-rs yet\n"));
    // outside of a library
    let out = b.run(&["undo"]);
    assert_eq!(out.stderr, "papers: error: no library found here: `papers init`, or pass --bibtex\n");
    assert_eq!(out.code, 1);
    // a library whose bibtex is not there
    let out = b.run_in("lib", &["undo", "--bibtex", "nosuch.bib"]);
    assert_eq!(
        out.stderr,
        "papers: error: no bibtex file found, do `touch nosuch.bib` or `papers init --bibtex nosuch.bib`\n"
    );
}

#[test]
fn a_dry_run_changes_nothing() {
    let b = Sandbox::new();
    b.init("lib");
    b.add("lib", "one2000");
    b.add("lib", "two2000");
    let tree = |b: &Sandbox| {
        let mut files = Vec::new();
        fn walk(dir: &Path, files: &mut Vec<(String, Vec<u8>)>) {
            let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
            entries.sort();
            for p in entries {
                if p.is_dir() {
                    walk(&p, files);
                } else {
                    files.push((p.to_string_lossy().into_owned(), fs::read(&p).unwrap()));
                }
            }
        }
        walk(Path::new(&b.root), &mut files);
        files
    };
    let before = tree(&b);
    assert_eq!(
        ok(b.run_in("lib", &["undo", "--dry-run"])),
        "DRYRUN: undo: $ROOT/work/lib/papers.bib not restored to 2 papers add ../one2000.bib ($TIME)\n"
    );
    assert_eq!(
        ok(b.run_in("lib", &["restore", "1", "--dry-run"])),
        "DRYRUN: restore: $ROOT/work/lib/papers.bib not restored to 1 the library before its first recorded command ($TIME)\n"
    );
    assert_eq!(error(b.run_in("lib", &["redo", "--dry-run"])), "papers: error: nothing to redo\n");
    assert_eq!(tree(&b), before);
}

#[test]
fn another_bibtex_with_its_own_library() {
    let b = Sandbox::new();
    b.init("a");
    b.init("b");
    b.add("a", "one2000");
    b.add("b", "two2000");
    b.add("b", "three2000");
    // from the folder of a: the history of the library of that bibtex
    ok(b.run_in("a", &["undo", "--bibtex", "../b/papers.bib"]));
    assert_eq!(b.keys("b"), ["two2000"]);
    assert_eq!(b.keys("a"), ["one2000"]);
    // the bibtex of the current library
    ok(b.run_in("a", &["undo", "--bibtex", "papers.bib"]));
    assert!(b.keys("a").is_empty());
}

// questions
// =========

#[test]
fn without_a_terminal_a_removal_asks_for_the_option() {
    let b = Sandbox::new();
    b.init("lib");
    b.add("lib", "one2000");
    assert_eq!(
        error(b.run_in("lib", &["history", "--remove"])),
        "papers: error: remove the history of the library (2 steps)? no terminal to confirm: -y/--yes removes without asking\n"
    );
    // nothing was removed
    assert!(ok(b.run_in("lib", &["history"])).starts_with("> 2 papers add"));
    assert_eq!(
        ok(b.run_in("lib", &["history", "--remove", "--yes"])),
        "removed the history of the library (2 steps)\n"
    );
    assert_eq!(ok(b.run_in("lib", &["history", "--remove", "-y"])), "the library has no history\n");
    assert!(ok(b.run_in("lib", &["history"])).starts_with("no history yet"));
}

#[test]
fn the_history_of_a_library_by_its_identifier() {
    let b = Sandbox::new();
    b.init("lib");
    b.add("lib", "one2000");
    let id = {
        let all = ok(b.run(&["history", "--all"]));
        let line = all.lines().nth(1).unwrap();
        // the identifier was hidden: ask the configuration
        assert!(line.starts_with("* $ID: 2 steps"));
        let text = fs::read_to_string(b.path("lib/.papers/config.json")).unwrap();
        text.split("\"id\": \"").nth(1).unwrap().split('"').next().unwrap().to_string()
    };
    assert_eq!(
        error(b.run(&["history", "--remove", "nosuchid", "-y"])),
        "papers: error: no history of a library nosuchid (see `papers history --all`)\n"
    );
    assert!(error(b.run(&["history", "--remove", &id])).contains("no terminal to confirm"));
    assert_eq!(ok(b.run(&["history", "--remove", &id, "-y"])), "removed the history of the library $ID (2 steps)\n");
    assert_eq!(ok(b.run(&["history", "--all"])), "");
}

#[test]
fn drop_files_needs_a_history_and_a_step() {
    let b = Sandbox::new();
    b.init("lib");
    assert_eq!(
        error(b.run_in("lib", &["history", "--drop-files", "1"])),
        "papers: error: the library has no history\n"
    );
    b.add("lib", "one2000");
    assert_eq!(
        error(b.run_in("lib", &["history", "--drop-files", "9"])),
        "papers: error: no step 9 in the history of the library (see `papers history`)\n"
    );
    assert_eq!(ok(b.run_in("lib", &["history", "--drop-files", "2"])), "the history keeps no file deleted by step 2\n");
    assert_eq!(
        ok(b.run_in("lib", &["history", "--drop-files", "files/x.pdf", "-y"])),
        "the history keeps no file deleted from files/x.pdf\n"
    );
    // an empty value is no option, as in papers: the history is listed
    assert!(ok(b.run_in("lib", &["history", "--drop-files", ""])).starts_with("> 2 papers add"));
}

#[test]
fn revert_takes_back_a_step_alone() {
    let b = Sandbox::new();
    b.init("lib");
    b.add("lib", "one2000");
    b.add("lib", "two2000");
    // step 3 added two2000: reverted, as a step of its own (undo takes it back); the first step has nothing before it
    assert_eq!(
        ok(b.run_in("lib", &["history", "--revert", "3"])).lines().next().unwrap(),
        "Removed -> two2000: Title of two2000 "
    );
    assert!(!fs::read_to_string(b.path("lib/papers.bib")).unwrap().contains("two2000"));
    assert!(ok(b.run_in("lib", &["history"])).starts_with("> 4 papers history --revert 3 ($TIME)\n"));
    assert!(ok(b.run_in("lib", &["undo"])).starts_with("undo: papers.bib restored to 3 papers add "));
    assert!(fs::read_to_string(b.path("lib/papers.bib")).unwrap().contains("two2000"));
    assert_eq!(
        error(b.run_in("lib", &["history", "--revert", "1"])),
        "papers: error: step 1 is the first step of the history: nothing before it to go back to\n"
    );
    assert_eq!(
        error(b.run_in("lib", &["history", "--revert", "9"])),
        "papers: error: no step 9 in the history of the library (see `papers history`)\n"
    );
    // nothing is written with --dry-run
    let before = fs::read_to_string(b.path("lib/papers.bib")).unwrap();
    let out = ok(b.run_in("lib", &["history", "--revert", "3", "--dry-run"]));
    assert!(out.starts_with("DRYRUN: "), "{out}");
    assert_eq!(fs::read_to_string(b.path("lib/papers.bib")).unwrap(), before);
    // without a library
    assert_eq!(
        error(b.run(&["history", "--revert", "2"])),
        "papers: error: no library: --revert takes back a step of the history of a library\n"
    );
    // its options are those of papers
    let out = b.run_in("lib", &["history", "--on-duplicate", "x"]);
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("invalid choice: 'x' (choose from 'ask', 'keep', 'update', 'skip', 'error')"),
        "{}",
        out.stderr
    );
}

/// What a command prints on a terminal, when it asks and is given `answer`.
fn on_terminal(b: &Sandbox, args: &[&str], cwd: &str, answer: &[u8]) -> (i32, String) {
    on_terminal_at(b, args, cwd, answer, b"[y/N] ")
}

/// What a command prints on a terminal, when it asks (the transcript ends with `prompt`) and is given `answer`.
fn on_terminal_at(b: &Sandbox, args: &[&str], cwd: &str, answer: &[u8], prompt: &[u8]) -> (i32, String) {
    use std::io::Write;
    let (mut master, mut slave) = (0, 0);
    // SAFETY: openpty fills the two descriptors it is given
    let status = unsafe {
        libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
    };
    assert_eq!(status, 0);
    // SAFETY: the descriptors are ours and open
    let (mut master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
    let mut command = b.command(env!("CARGO_BIN_EXE_papers-rs"), args, cwd);
    command
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave.try_clone().unwrap()));
    let mut child = command.spawn().unwrap();
    drop(command);
    drop(slave);
    let mut transcript = Vec::new();
    let mut sent = false;
    let mut buffer = [0u8; 4096];
    loop {
        let mut poll = libc::pollfd { fd: master.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd
        let ready = unsafe { libc::poll(&mut poll, 1, 20_000) };
        assert!(ready > 0, "no output within 20 s: {}", String::from_utf8_lossy(&transcript));
        match master.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => transcript.extend_from_slice(&buffer[..n]),
        }
        if !sent && transcript.ends_with(prompt) {
            master.write_all(answer).unwrap();
            sent = true;
        }
    }
    let code = child.wait().unwrap().code().unwrap_or(-1);
    (code, b.show(&String::from_utf8_lossy(&transcript)).replace("\r\n", "\n"))
}

/// On a terminal a conflict of a revert is asked (the dialogue); the answers are those of the duplicates.
#[test]
fn a_revert_asks_about_a_conflict_on_a_terminal() {
    let b = Sandbox::new();
    b.init("lib");
    b.add("lib", "one2000");
    for (message, year) in [("papers edit one2000 --set year=2001", "2001"), ("papers edit one2000 --set year=2002", "2002")] {
        b.step("lib", message, |old| old.replace("year = {2000}", &format!("year = {{{year}}}")).replace("year = {2001}", &format!("year = {{{year}}}")));
    }
    let (code, transcript) = on_terminal_at(&b, &["history", "--revert", "3"], "lib", b"2\n", b">>> ");
    assert_eq!(code, 0, "{transcript}");
    assert!(transcript.starts_with("Same paper: 2 entries (step 3)\n"), "{transcript}");
    assert!(transcript.contains("!  year   = {2002}"), "{transcript}");
    assert!(transcript.contains("  2 take values of 2"), "{transcript}");
    assert!(transcript.ends_with("Modified -> one2000: Title of one2000 \n"), "{transcript}");
    assert!(fs::read_to_string(b.path("lib/papers.bib")).unwrap().contains("year = {2000}"));
}

#[test]
fn a_removal_asks_on_a_terminal() {
    const QUESTION: &str = "remove the history of the library (2 steps)? [y/N] ";
    for (answer, removed) in
        [(&b"y\n"[..], true), (b"  YES \n", true), (b"yes\n", true), (b"n\n", false), (b"\n", false), (b"no\n", false)]
    {
        let b = Sandbox::new();
        b.init("lib");
        b.add("lib", "one2000");
        let (code, transcript) = on_terminal(&b, &["history", "--remove"], "lib", answer);
        assert_eq!(code, 0, "{transcript}");
        let typed = String::from_utf8_lossy(answer).trim_end_matches('\n').to_string();
        let result = if removed { "removed the history of the library (2 steps)\n" } else { "" };
        assert_eq!(transcript, format!("{QUESTION}{typed}\n{result}"), "answer {answer:?}");
        let listing = ok(b.run_in("lib", &["history"]));
        assert_eq!(listing.starts_with("no history yet"), removed);
    }
}

#[test]
fn a_question_without_an_answer_is_an_error() {
    let b = Sandbox::new();
    b.init("lib");
    b.add("lib", "one2000");
    // the end of the input (Ctrl-D)
    let (code, transcript) = on_terminal(&b, &["history", "--remove"], "lib", b"\x04");
    assert_eq!(code, 1);
    assert!(
        transcript.ends_with(
            "papers: error: papers asked a question, but there is no input to answer it: run the command in a terminal, or answer with its options (e.g. --on-duplicate for duplicates, -y/--yes)\n"
        ),
        "{transcript}"
    );
    assert!(ok(b.run_in("lib", &["history"])).starts_with("> 2 papers add"));
}

// status
// ======

#[test]
fn status_without_a_library() {
    let b = Sandbox::new();
    assert_eq!(
        ok(b.run(&["status"])),
        "papers library: (no library found: create one with `papers init`)\n* default library: none\n"
    );
    let verbose = ok(b.run(&["status", "-v"]));
    assert!(
        verbose.starts_with(
            "papers library: (no library found: create one with `papers init`)\n* configuration file: none\n* cache directory:    "
        ),
        "{verbose}"
    );
    assert!(
        verbose.ends_with(&format!(
            "* version:            {}\n* default library: none\n",
            papers_rs::error::PAPERS_VERSION
        ))
    );
    // colours when asked for
    assert!(ok(b.run(&["status", "--color", "always"])).starts_with(
        "\x1b[1mpapers library: \x1b[93m(no library found: create one with `papers init`)\x1b[0m\x1b[0m\n"
    ));
}

#[test]
fn status_of_a_library() {
    let b = Sandbox::new();
    b.init("lib");
    assert_eq!(
        ok(b.run_in("lib", &["status"])),
        "papers library: $ROOT/work/lib\n* bibtex: papers.bib (empty)\n* files:  files (the files directory of the library, empty)\n`papers config` lists and sets the settings\n"
    );
    fs::create_dir_all(b.path("lib/files/sub")).unwrap();
    fs::write(
        b.path("lib/papers.bib"),
        format!("{}{}", entry("one2000"), "@article{two2000,\n file = {:files/a.pdf:pdf;:files/sub/b.PDF:pdf},\n}\n"),
    )
    .unwrap();
    fs::write(b.path("lib/files/a.pdf"), vec![b'x'; 300_000]).unwrap();
    fs::write(b.path("lib/files/sub/b.PDF"), vec![b'y'; 10]).unwrap();
    fs::write(b.path("lib/files/notes.txt"), "not a pdf").unwrap();
    assert_eq!(
        ok(b.run_in("lib", &["status"])),
        "papers library: $ROOT/work/lib\n* bibtex: papers.bib (2 entries, 2 file links)\n* files:  files (the files directory of the library, 2 files, 0.3 MB)\n`papers config` lists and sets the settings\n"
    );
    assert_eq!(
        ok(b.run_in("lib", &["status", "--no-check-files"])),
        "papers library: $ROOT/work/lib\n* bibtex: papers.bib\n* files:  files (the files directory of the library)\n`papers config` lists and sets the settings\n"
    );
    assert_eq!(
        ok(b.run_in("lib", &["status", "--color", "always"])),
        "\x1b[1mpapers library: $ROOT/work/lib\x1b[0m\n* bibtex: papers.bib\x1b[94m (2 entries, 2 file links)\x1b[0m\n* files:  files (the files directory of the library, \x1b[94m2 files, 0.3 MB\x1b[0m)\n`papers config` lists and sets the settings\n"
    );
    // from a folder of the library, the paths are relative to it
    fs::create_dir_all(b.path("lib/sub")).unwrap();
    assert_eq!(
        ok(b.run_in("lib/sub", &["status", "--no-check-files"])),
        "papers library: $ROOT/work/lib\n* bibtex: ../papers.bib\n* files:  ../files (the files directory of the library)\n`papers config` lists and sets the settings\n"
    );
    // the history, with -v
    b.add("lib", "three2000");
    let verbose = ok(b.run_in("lib", &["status", "-v"]));
    assert!(verbose.contains("* history:            2 steps (`papers history`)\n"), "{verbose}");
    assert!(verbose.contains("* configuration file: .papers/config.json\n"), "{verbose}");
}

#[test]
fn the_status_of_a_library_with_a_pool_has_its_pool_line() {
    let b = Sandbox::new();
    b.init("lib");
    ok(b.run(&["init", "project", "--pool", "../lib"]));
    let status = ok(b.run_in("project", &["status", "--no-check-files"]));
    assert!(status.contains("* files:  in the pool, $ROOT/work/lib/files\n"), "{status}");
    assert!(status.contains("* pool:   ../lib (not pulled from yet)\n"), "{status}");
    assert!(status.ends_with("`papers pull` takes the entries shared with the pool\n"), "{status}");
}

// with the reference
// ==================

/// papers writes the libraries and papers-rs undoes, redoes and restores them, in a copy of the
/// sequence run by papers alone: the same output, the same files.
mod reference {
    use super::*;

    struct Run {
        steps: Vec<(String, Out)>,
        files: Vec<(String, String)>,
    }

    fn tree(b: &Sandbox) -> Vec<(String, String)> {
        let mut files = Vec::new();
        fn walk(b: &Sandbox, dir: &Path, files: &mut Vec<(String, String)>) {
            let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
            entries.sort();
            for p in entries {
                let name = p.to_string_lossy().into_owned();
                if p.is_dir() {
                    walk(b, &p, files);
                } else if !name.ends_with("/lock") && !name.contains("/store/") && !name.contains("/.papers/origin") {
                    let text = fs::read(&p).map(|d| String::from_utf8_lossy(&d).into_owned()).unwrap_or_default();
                    files.push((b.show(&name), b.show(&text)));
                }
            }
        }
        walk(b, Path::new(&format!("{}/work", b.root)), &mut files);
        files
    }

    /// `!` a command of papers, `>` a command that papers (the reference) or papers-rs runs.
    fn run(reference: &str, script: &[(&str, &str)], mine: bool) -> Run {
        let b = Sandbox::new();
        fs::write(b.path("one.bib"), entry("one2000")).unwrap();
        fs::write(b.path("two.bib"), entry("two2000")).unwrap();
        fs::write(b.path("three.bib"), entry("three2000")).unwrap();
        fs::create_dir_all(b.path("Downloads")).unwrap();
        fs::write(b.path("Downloads/a.pdf"), "%PDF a").unwrap();
        fs::write(b.path("Downloads/b.pdf"), "%PDF b").unwrap();
        let mut steps = Vec::new();
        for (kind, line) in script {
            let args: Vec<String> = line.split_whitespace().map(|a| a.replace("$WORK", &b.path(""))).collect();
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let binary = if *kind == ">" && mine { env!("CARGO_BIN_EXE_papers-rs") } else { reference };
            let out = b.run_with(binary, &args, ".");
            if *kind == ">" {
                steps.push((line.to_string(), out));
            }
        }
        Run { steps, files: tree(&b) }
    }

    pub fn same(script: &[(&str, &str)]) {
        let Ok(reference) = std::env::var("PAPERS_REFERENCE") else {
            panic!("PAPERS_REFERENCE is the path of the reference papers");
        };
        let papers = run(&reference, script, false);
        let papers_rs = run(&reference, script, true);
        assert_eq!(papers.steps.len(), papers_rs.steps.len());
        for ((line, a), (_, b)) in papers.steps.iter().zip(&papers_rs.steps) {
            assert_eq!((a.code, &a.stdout, &a.stderr), (b.code, &b.stdout, &b.stderr), "papers {line}");
        }
        assert_eq!(papers.files, papers_rs.files, "the files after");
    }
}

#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=/path/to/papers cargo test -p papers-rs-cli --test history_cmds -- --ignored"]
fn papers_writes_and_papers_rs_undoes() {
    reference::same(&[
        ("!", "init lib"),
        ("!", "-C lib add ../one.bib"),
        ("!", "-C lib add ../two.bib --attach ../Downloads/a.pdf --copy"),
        ("!", "-C lib add ../three.bib --attach ../Downloads/b.pdf --move"),
        ("!", "-C lib tag one2000 --add x"),
        ("!", "-C lib rm two2000 --delete-files -y"),
        (">", "-C lib history"),
        (">", "-C lib status"),
        (">", "-C lib status -v"),
        (">", "-C lib undo"),
        (">", "-C lib history -n 3"),
        (">", "-C lib undo -n 3"),
        (">", "-C lib redo"),
        (">", "-C lib redo -n 2"),
        (">", "-C lib restore 2"),
        (">", "-C lib restore 99"),
        (">", "-C lib undo --dry-run"),
        (">", "-C lib history -n 0"),
        (">", "-C lib history --all"),
        (">", "-C lib history --drop-files 6 -y"),
        (">", "-C lib undo"),
        (">", "-C lib history"),
        (">", "-C lib history --remove -y"),
        (">", "-C lib history"),
    ]);
}

#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=/path/to/papers cargo test -p papers-rs-cli --test history_cmds -- --ignored"]
fn papers_writes_a_shallow_library_and_papers_rs_undoes() {
    reference::same(&[
        ("!", "init lib"),
        ("!", "init project --pool ../lib"),
        ("!", "-C project add ../one.bib --attach ../Downloads/a.pdf --copy"),
        ("!", "-C project add ../two.bib"),
        (">", "-C project history"),
        (">", "-C lib history"),
        (">", "-C project undo"),
        (">", "-C lib history"),
        (">", "-C project redo"),
        (">", "-C project undo --library-only"),
        (">", "-C project redo --library-only"),
        (">", "-C project undo -n 2"),
        (">", "-C project redo -n 2"),
        (">", "-C lib undo"),
        (">", "-C project history"),
    ]);
}

/// papers-rs records (as the library model does), papers undoes and redoes, and both list: the
/// same store read by the two.
#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=/path/to/papers cargo test -p papers-rs-cli --test history_cmds -- --ignored"]
fn papers_rs_writes_and_papers_undoes() {
    let reference = std::env::var("PAPERS_REFERENCE").expect("PAPERS_REFERENCE is the path of the reference papers");
    let b = Sandbox::new();
    b.init("lib");
    for key in ["one2000", "two2000", "three2000"] {
        b.add("lib", key);
    }
    let both = |args: &[&str]| {
        let a = b.run_with(&reference, args, "lib");
        let r = b.run_with(env!("CARGO_BIN_EXE_papers-rs"), args, "lib");
        assert_eq!((a.code, &a.stdout, &a.stderr), (r.code, &r.stdout, &r.stderr), "papers {args:?}");
        a
    };
    both(&["history"]);
    both(&["history", "--all"]);
    assert_eq!(b.run_with(&reference, &["undo", "-n", "2"], "lib").code, 0);
    assert_eq!(b.keys("lib"), ["one2000"]);
    both(&["history", "-n", "0"]);
    // papers-rs brings it back, papers undoes it again, in the same history
    assert_eq!(b.run_in("lib", &["redo"]).code, 0);
    assert_eq!(b.keys("lib"), ["one2000", "two2000"]);
    assert_eq!(b.run_with(&reference, &["undo"], "lib").code, 0);
    assert_eq!(b.keys("lib"), ["one2000"]);
    b.add("lib", "three2000");
    assert_eq!(b.run_with(&reference, &["restore", "2"], "lib").code, 0);
    assert_eq!(b.keys("lib"), ["one2000"]);
    both(&["history", "-n", "0"]);
    assert_eq!(b.run_in("lib", &["restore", "5"]).code, 0);
    assert_eq!(b.keys("lib"), ["one2000", "three2000"]);
    both(&["history"]);
}

#[test]
fn undo_and_redo_of_a_library_with_auto_push_say_there_is_nothing_when_there_is_nothing() {
    let b = Sandbox::new();
    b.init("lib");
    ok(b.run(&["init", "project", "--pool", "../lib"]));
    ok(b.run_in("project", &["config", "auto-push", "true"]));
    for command in ["undo", "redo"] {
        assert_eq!(error(b.run_in("project", &[command])), format!("papers: error: nothing to {command}\n"));
    }
    assert_eq!(error(b.run_in("project", &["undo", "--library-only"])), "papers: error: nothing to undo\n");
    assert_eq!(error(b.run_in("project", &["redo", "--dry-run"])), "papers: error: nothing to redo\n");
}
