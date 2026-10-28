//! papers-rs and papers on one history store (`tests/gen_history.py` records
//! what papers did: `tests/history/*.json`).
//!
//! For each step of each scenario, from the store and the library as papers
//! left them before the step:
//!
//! - undo, redo and restore: papers-rs does the same on a copy; the library
//!   (the files) and the store (the heads) are those papers left, and what it
//!   says (the state, the files that moved, the errors) is what papers said;
//! - the other commands: papers-rs records the step (given what the command
//!   did: its message, its file operations, the texts after) and the store is
//!   what papers wrote (the decompressed objects, the states and the heads
//!   without their times);
//! - `papers history`, `papers history --all`: the same lines.
//!
//! The retention is checked on a store of its own (`retention.json`).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, SystemTime};

use papers_rs::config::paths::realpath;
use papers_rs::config::{Config, MachineConfig};
use papers_rs::history::listing::{Removal, all_lines, drop_files_plan, history_lines};
use papers_rs::history::ojson::J;
use papers_rs::history::report::{linked_files, unlinked_notice};
use papers_rs::history::restore::Deleter;
use papers_rs::history::setup::{Setup, library_id, path_id};
use papers_rs::history::store::{Store, now};
use papers_rs::history::{FileOp, History, OpKind, PoolLink, Step};
use serde_json::{Map, Value};

mod common;
use common::{dir, json_file};

// files and trees
// ===============

fn b64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            fs::copy(&from, &to).unwrap();
            let modified = fs::metadata(&from).unwrap().modified().unwrap();
            fs::File::options()
                .write(true)
                .open(&to)
                .unwrap()
                .set_modified(modified)
                .unwrap();
        }
    }
}

fn files_of(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, at: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = fs::read_dir(at) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                out.insert(
                    p.strip_prefix(base).unwrap().to_string_lossy().into_owned(),
                    fs::read(&p).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// A sandbox: the working folder and the history store, as the generator had them.
struct Universe {
    _tmp: tempfile::TempDir,
    root: String,
    /// The bibtex files without library: their path in the working folder and
    /// the identifier of their project in the fixture (it comes from the path of
    /// the sandbox that made it: here it is made from the path of this one).
    bibs: Vec<(String, String)>,
}

impl Universe {
    fn new(bibs: &[(String, String)]) -> Universe {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        fs::create_dir_all(format!("{root}/work")).unwrap();
        fs::create_dir_all(format!("{root}/history")).unwrap();
        Universe {
            _tmp: tmp,
            root,
            bibs: bibs.to_vec(),
        }
    }

    /// The identifier of the project of a bibtex without library, here.
    fn own_id(&self, bibtex: &str) -> String {
        path_id(&realpath(&format!("{}/work/{bibtex}", self.root)))
    }

    /// A path of the store of the fixture, with the identifiers made here.
    fn map_rel(&self, rel: &str) -> String {
        let mut out = rel.to_string();
        for (bib, id) in &self.bibs {
            out = out.replace(id.as_str(), &self.own_id(bib));
        }
        out
    }

    fn work(&self) -> String {
        format!("{}/work", self.root)
    }

    fn store(&self) -> String {
        format!("{}/history", self.root)
    }

    fn copy(&self) -> Universe {
        let other = Universe::new(&self.bibs);
        copy_tree(Path::new(&self.work()), Path::new(&other.work()));
        copy_tree(Path::new(&self.store()), Path::new(&other.store()));
        for (bib, _) in &self.bibs {
            // the project of a bibtex without library is named by its path (and may have a store of its own)
            let (old, new) = (self.own_id(bib), other.own_id(bib));
            let (store, separate) = (other.store(), format!("{}/separate", other.store()));
            for (from, to) in [
                (
                    format!("{separate}/{old}/projects/{old}"),
                    format!("{separate}/{old}/projects/{new}"),
                ),
                (format!("{separate}/{old}"), format!("{separate}/{new}")),
                (
                    format!("{store}/projects/{old}"),
                    format!("{store}/projects/{new}"),
                ),
            ] {
                if Path::new(&from).exists() {
                    fs::rename(from, to).unwrap();
                }
            }
        }
        // the states and the heads name paths of the sandbox
        for rel in files_of(Path::new(&other.store()))
            .keys()
            .filter(|r| r.ends_with(".json"))
        {
            let path = format!("{}/{rel}", other.store());
            let text = fs::read_to_string(&path).unwrap();
            if text.contains(&self.root) {
                let modified = fs::metadata(&path).unwrap().modified().unwrap();
                fs::write(&path, text.replace(&self.root, &other.root)).unwrap();
                fs::File::options()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_modified(modified)
                    .unwrap();
            }
        }
        other
    }

    fn write_file(&self, base: &str, rel: &str, entry: &Value) {
        let path = format!("{base}/{rel}");
        fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
        let bytes = match (entry.get("text"), entry.get("b64")) {
            (Some(t), _) => t.as_str().unwrap().replace("$TMP", &self.root).into_bytes(),
            (_, Some(b)) => b64_decode(b.as_str().unwrap()),
            _ => panic!("a file without content: {rel}"),
        };
        fs::write(path, bytes).unwrap();
    }

    /// Apply a delta of the generator to the working folder or the store;
    /// `only` restricts the files that are set or deleted.
    fn apply(&self, base: &str, delta: &Value, only: &dyn Fn(&str) -> bool) {
        let store = base == self.store();
        for rel in delta["del"].as_array().unwrap() {
            let rel = rel.as_str().unwrap();
            if only(rel) {
                let rel = if store {
                    self.map_rel(rel)
                } else {
                    rel.to_string()
                };
                let path = format!("{base}/{rel}");
                let _ = fs::remove_file(&path);
                // the folders that are empty now are gone too (the fixture lists files only)
                let mut at = Path::new(&path).parent();
                while let Some(folder) = at.filter(|f| f.starts_with(base) && *f != Path::new(base))
                {
                    if fs::remove_dir(folder).is_err() {
                        break;
                    }
                    at = folder.parent();
                }
            }
        }
        for (rel, entry) in delta["set"].as_object().unwrap() {
            if only(rel) {
                let rel = if store {
                    self.map_rel(rel)
                } else {
                    rel.clone()
                };
                self.write_file(base, &rel, entry);
            }
        }
    }

    fn apply_all(&self, step: &Value) {
        self.apply(&self.work(), &step["work"], &|_| true);
        self.apply(&self.store(), &step["store"], &|_| true);
    }

    /// The objects of the store are older (the retention keeps the young ones).
    fn age(&self, seconds: u64) {
        let old = SystemTime::now() - Duration::from_secs(seconds);
        for sub in ["objects", "file-objects"] {
            for rel in files_of(Path::new(&format!("{}/{sub}", self.store()))).keys() {
                let f = fs::File::options()
                    .write(true)
                    .open(format!("{}/{sub}/{rel}", self.store()))
                    .unwrap();
                f.set_modified(old).unwrap();
            }
        }
    }
}

// the store, normalized
// =====================

/// The text of a state or of the heads with every `"time": N` as `"time": 0`.
fn without_times(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("\"time\": ") {
        let (head, tail) = rest.split_at(at + 8);
        out.push_str(head);
        let end = tail
            .find(|c: char| !matches!(c, '0'..='9' | '.' | 'e' | 'E' | '+' | '-'))
            .unwrap_or(tail.len());
        out.push('0');
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// The store as comparable text: the objects decompressed, the states and the
/// heads without their times and with `$TMP` for the folder of the sandbox.
fn store_snapshot(u: &Universe) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    for (rel, bytes) in files_of(Path::new(&u.store())) {
        let value = if rel.contains("/objects/") || rel.starts_with("objects/") {
            let mut data = Vec::new();
            flate2::read::ZlibDecoder::new(bytes.as_slice())
                .read_to_end(&mut data)
                .expect("an object is zlib");
            data
        } else if rel.ends_with(".json") {
            without_times(&String::from_utf8(bytes).unwrap().replace(&u.root, "$TMP")).into_bytes()
        } else {
            bytes
        };
        let key = u
            .bibs
            .iter()
            .fold(rel, |k, (bib, _)| k.replace(&u.own_id(bib), "$ID"));
        out.insert(key, value);
    }
    out
}

fn assert_same_store(got: &Universe, want: &Universe, ctx: &str) {
    let (a, b) = (store_snapshot(got), store_snapshot(want));
    let only_got: Vec<&String> = a.keys().filter(|k| !b.contains_key(*k)).collect();
    let only_want: Vec<&String> = b.keys().filter(|k| !a.contains_key(*k)).collect();
    assert!(
        only_got.is_empty() && only_want.is_empty(),
        "{ctx}: files of the store only in papers-rs {only_got:?}, only in papers {only_want:?}"
    );
    for (k, v) in &a {
        assert!(
            *v == b[k],
            "{ctx}: {k} differs:\npapers-rs: {}\npapers:    {}",
            String::from_utf8_lossy(v),
            String::from_utf8_lossy(&b[k])
        );
    }
}

fn assert_same_work(got: &Universe, want: &Universe, ctx: &str) {
    let (a, b) = (
        files_of(Path::new(&got.work())),
        files_of(Path::new(&want.work())),
    );
    let only_got: Vec<&String> = a.keys().filter(|k| !b.contains_key(*k)).collect();
    let only_want: Vec<&String> = b.keys().filter(|k| !a.contains_key(*k)).collect();
    assert!(
        only_got.is_empty() && only_want.is_empty(),
        "{ctx}: files only in papers-rs {only_got:?}, only in papers {only_want:?}"
    );
    for (k, v) in &a {
        assert!(
            *v == b[k],
            "{ctx}: {k} differs:\npapers-rs: {}\npapers:    {}",
            String::from_utf8_lossy(v),
            String::from_utf8_lossy(&b[k])
        );
    }
}

// the output
// ==========

/// `(2026-10-28 13:33` -> `($TIME`, the paths of the sandbox as `$TMP`, the store line
/// without the folder of the store.
fn norm_lines(text: &str, u: &Universe) -> Vec<String> {
    let text = u
        .bibs
        .iter()
        .fold(text.replace(&u.root, "$TMP"), |t, (bib, fixture)| {
            t.replace(fixture.as_str(), "$ID")
                .replace(&u.own_id(bib), "$ID")
        });
    text.lines()
        .filter(|line| !line.starts_with("stale: "))
        .map(|line| {
            let mut out = String::new();
            let b = line.as_bytes();
            let mut i = 0;
            while i < b.len() {
                let is_time = b[i] == b'('
                    && i + 17 <= b.len()
                    && line.is_char_boundary(i + 17)
                    && line[i + 1..i + 17]
                        .bytes()
                        .enumerate()
                        .all(|(k, c)| match k {
                            4 | 7 => c == b'-',
                            10 => c == b' ',
                            13 => c == b':',
                            _ => c.is_ascii_digit(),
                        });
                if is_time {
                    out.push_str("($TIME");
                    i += 17;
                } else {
                    let ch = line[i..].chars().next().unwrap();
                    out.push(ch);
                    i += ch.len_utf8();
                }
            }
            if let Some(rest) = out.strip_prefix("history: ")
                && let Some(at) = rest.find(", ")
            {
                out = format!("history: $STORE{}", &rest[at..]);
            } else if !out.starts_with(['*', ' ', '>'])
                && out.contains(": ")
                && out.ends_with(" MB")
            {
                let at = out.rfind(": ").unwrap();
                out = format!("$STORE{}", &out[at..]);
            }
            out
        })
        .collect()
}

// configurations
// ==============

fn machine_for(u: &Universe, fx: &Value) -> MachineConfig {
    let mut data: Map<String, Value> = fx["machine"].as_object().cloned().unwrap_or_default();
    data.insert("history-dir".into(), Value::String(u.store()));
    MachineConfig::new(None, data)
}

/// The configuration of the library a step ran on: its folder, or the bibtex without library.
fn config_for(u: &Universe, fx: &Value, step: &Value) -> Config {
    let machine = machine_for(u, fx);
    if let Some(bibtex) = step.get("bibtex").and_then(Value::as_str) {
        let mut config = Config::new(machine);
        config.bibtex = Some(format!("{}/{bibtex}", u.work()));
        return config;
    }
    let lib = step["lib"].as_str().expect("a library");
    Config::load(
        &format!("{}/{lib}/.papers/config.json", u.work()),
        machine,
        None,
    )
    .unwrap()
}

fn argv(step: &Value) -> Vec<String> {
    step["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().replace("$TMP", ""))
        .collect()
}

/// The arguments with the identifier of a library where the generator wrote `$ID:folder`.
fn argv_in(u: &Universe, step: &Value) -> Vec<String> {
    argv(step)
        .into_iter()
        .map(|a| match a.strip_prefix("$ID:") {
            Some(dir) => {
                let text =
                    fs::read_to_string(format!("{}/{dir}/.papers/config.json", u.work())).unwrap();
                J::parse(&text)
                    .unwrap()
                    .get("id")
                    .and_then(J::as_str)
                    .unwrap()
                    .to_string()
            }
            None => a,
        })
        .collect()
}

/// The first step that records something gives a library its identifier, written into its
/// configuration: papers-rs makes one (checked: the file is the one papers wrote, with that
/// identifier), then the file is the one of papers, with the identifier of the fixture.
fn align_library_id(e: &Universe, want: &Universe, fx: &Value, dir: &str, ctx: &str) {
    let before = format!("{}/{dir}/.papers/config.json", e.work());
    let after = format!("{}/{dir}/.papers/config.json", want.work());
    let (Ok(old), Ok(new)) = (fs::read_to_string(&before), fs::read_to_string(&after)) else {
        return;
    };
    let id_of =
        |t: &str| J::parse(t).and_then(|j| j.get("id").and_then(J::as_str).map(str::to_string));
    let (None, Some(id)) = (id_of(&old), id_of(&new)) else {
        return;
    };
    let mut config = Config::load(&before, machine_for(e, fx), None).unwrap();
    let made = library_id(&mut config, true, false).unwrap().unwrap();
    assert_eq!(
        fs::read_to_string(&before).unwrap(),
        new.replace(&id, &made),
        "{ctx}: the configuration file with the new identifier"
    );
    fs::write(&before, new).unwrap();
}

/// The folder where the command ran.
fn cwd_of(u: &Universe, step: &Value) -> String {
    format!("{}/{}", u.work(), step["cwd"].as_str().unwrap())
        .trim_end_matches("/.")
        .to_string()
}

/// `-n N` of the arguments, or the default.
fn option_n(args: &[String], default: usize) -> usize {
    args.iter()
        .position(|a| a == "-n")
        .map(|i| args[i + 1].parse().unwrap())
        .unwrap_or(default)
}

#[derive(Default, Debug)]
struct Counts {
    undone: usize,
    recorded: usize,
    listed: usize,
    edited: usize,
}

// the steps
// =========

fn time_travel(
    e: &Universe,
    want: &Universe,
    fx: &Value,
    step: &Value,
    ctx: &str,
    live: Option<(&str, &Universe)>,
) {
    let args = argv(step);
    if let Some(lib) = step["lib"].as_str() {
        align_library_id(e, want, fx, lib, ctx);
    }
    let mut config = config_for(e, fx, step);
    let before = config
        .bibtex
        .as_deref()
        .map(linked_files)
        .unwrap_or_default();
    let mut history = History::require(&mut config, false).unwrap();
    history.setup.cwd = cwd_of(e, step);
    history.trash = |p| drop(fs::remove_file(p));
    let kind = args[0].as_str();
    let steps = option_n(&args, 1);
    let library_only = args.iter().any(|a| a == "--library-only");
    let n: Option<u64> = args
        .iter()
        .skip(1)
        .find(|a| a.parse::<u64>().is_ok())
        .and_then(|a| a.parse().ok());
    let result = match kind {
        "undo" => history.undo(steps, library_only, None, "papers undo"),
        "redo" => history.redo(steps, library_only, None, "papers redo"),
        _ => history.restore(n, None),
    };
    let stdout = step["stdout"].as_str().unwrap();
    let stderr = step["stderr"].as_str().unwrap();
    match result {
        Ok(moved) => {
            assert_eq!(
                step["exit"], 0,
                "{ctx}: papers failed, papers-rs did not:\n{stderr}"
            );
            let mut lines = moved.lines(kind, &history);
            let after = config
                .bibtex
                .as_deref()
                .map(linked_files)
                .unwrap_or_default();
            lines.extend(unlinked_notice(&before, &after, config.filesdir.as_deref()));
            let got = norm_lines(&lines.join("\n"), e);
            assert_eq!(got, norm_lines(stdout, e), "{ctx}: what it says");
        }
        Err(error) => {
            assert_eq!(
                step["exit"], 1,
                "{ctx}: papers-rs failed ({error}), papers did not"
            );
            let want_text = stderr.trim_end().replace("$TMP", &e.root);
            assert_eq!(
                format!("papers: error: {}", error.message()),
                want_text,
                "{ctx}: the error"
            );
        }
    }
    assert_same_work(e, want, ctx);
    assert_same_store(e, want, ctx);
    if let Some((reference, before)) = live {
        // the reference papers does it on the store that papers-rs wrote
        let r = before.copy();
        let (code, out, err) = reference_run(reference, &r, fx, step);
        assert_eq!(
            code,
            step["exit"].as_i64().unwrap() as i32,
            "{ctx}: the reference papers: {err}"
        );
        if code == 0 {
            assert_eq!(
                norm_lines(&out, &r),
                norm_lines(stdout, e),
                "{ctx}: the reference papers on the store of papers-rs"
            );
        }
        assert_same_work(
            e,
            &r,
            &format!("{ctx} (the reference papers on the store of papers-rs)"),
        );
        assert_same_store(
            e,
            &r,
            &format!("{ctx} (the reference papers on the store of papers-rs)"),
        );
    }
}

/// Run the reference papers on a sandbox (a temporary home, the machine configuration of the fixture).
fn reference_run(reference: &str, u: &Universe, fx: &Value, step: &Value) -> (i32, String, String) {
    let home = format!("{}/home", u.root);
    let config = if cfg!(target_os = "macos") {
        format!("{home}/Library/Application Support/papers")
    } else {
        format!("{home}/.config/papers")
    };
    fs::create_dir_all(&config).unwrap();
    let mut machine: Map<String, Value> = fx["machine"].as_object().cloned().unwrap_or_default();
    machine.insert("history-dir".into(), Value::String(u.store()));
    machine.insert("version".into(), Value::from(3));
    fs::write(
        format!("{config}/config.json"),
        Value::Object(machine).to_string(),
    )
    .unwrap();
    let mut words = reference.split_whitespace();
    let mut command = std::process::Command::new(words.next().unwrap());
    let args: Vec<String> = step["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().replace("$TMP", &u.root))
        .map(|a| match a.strip_prefix("$ID:") {
            Some(dir) => {
                let text =
                    fs::read_to_string(format!("{}/{dir}/.papers/config.json", u.work())).unwrap();
                J::parse(&text)
                    .unwrap()
                    .get("id")
                    .and_then(J::as_str)
                    .unwrap()
                    .to_string()
            }
            None => a,
        })
        .collect();
    command.args(words).args(args);
    let cwd = format!("{}/{}", u.work(), step["cwd"].as_str().unwrap());
    command
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("TZ", "UTC")
        .env("COLUMNS", "80")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb");
    let out = command.output().expect("the reference papers runs");
    let _ = fs::remove_dir_all(&home);
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// `papers history --remove [ID]` and `--drop-files STEP|PATH`, without the questions (`-y`).
fn history_edit(
    e: &Universe,
    want: &Universe,
    fx: &Value,
    step: &Value,
    ctx: &str,
    live: Option<(&str, &Universe)>,
) {
    let args = argv_in(want, step);
    let delete: fn(&str) = |p| drop(fs::remove_file(p));
    let mut config = config_for(e, fx, step);
    let setup = Setup::from_config(&config);
    let mut out: Vec<String> = Vec::new();
    let result: Result<(), String> = (|| {
        if let Some(at) = args.iter().position(|a| a == "--remove") {
            let id = args.get(at + 1).filter(|a| !a.starts_with('-'));
            if let Some(id) = id {
                let (store, mut project) = papers_rs::history::listing::find_project(&setup, id)
                    .map_err(|e| e.message().to_string())?;
                let what = format!(
                    "the history of the library {id} ({} steps)",
                    project.states().len()
                );
                project.remove();
                out.push(format!("removed {what}"));
                papers_rs::history::listing::prune_after_removal(&setup, &store, &delete);
            } else {
                match History::open(&mut config, false, false).unwrap() {
                    None => out.push("the library has no history".into()),
                    Some(mut history) => {
                        let removal = history.remove_place().unwrap();
                        match removal {
                            Removal::Project { .. } => {
                                history.remove_project();
                                out.push(format!("removed {}", removal.message()));
                            }
                            other => out.push(other.message()),
                        }
                    }
                }
            }
        } else {
            let at = args.iter().position(|a| a == "--drop-files").unwrap();
            let target = &args[at + 1];
            let Some(mut history) = History::open(&mut config, false, false)
                .unwrap()
                .filter(|h| h.project.exists())
            else {
                return Err("the library has no history".into());
            };
            history.trash = delete;
            history.setup.cwd = cwd_of(e, step);
            let (number, path) = match target.parse::<u64>() {
                Ok(n) => (Some(n), None),
                Err(_) => (None, Some(target.as_str())),
            };
            let found = history
                .files_to_drop(number, path)
                .map_err(|e| e.message().to_string())?;
            if found.is_empty() {
                out.push(match number {
                    Some(n) => format!("the history keeps no file deleted by step {n}"),
                    None => format!("the history keeps no file deleted from {target}"),
                });
            } else {
                let plan = drop_files_plan(&found);
                out.extend(plan.list.clone());
                history
                    .drop_files(&found.keys().cloned().collect())
                    .unwrap();
                out.push(plan.done);
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            assert_eq!(step["exit"], 0, "{ctx}: papers failed, papers-rs did not");
            assert_eq!(
                norm_lines(&out.join("\n"), e),
                norm_lines(step["stdout"].as_str().unwrap(), e),
                "{ctx}: what it says"
            );
        }
        Err(message) => {
            assert_eq!(
                step["exit"], 1,
                "{ctx}: papers-rs failed ({message}), papers did not"
            );
            assert_eq!(
                format!("papers: error: {message}"),
                step["stderr"].as_str().unwrap().trim_end(),
                "{ctx}: the error"
            );
        }
    }
    assert_same_work(e, want, ctx);
    assert_same_store(e, want, ctx);
    if let Some((reference, before)) = live {
        let r = before.copy();
        let (code, stdout, stderr) = reference_run(reference, &r, fx, step);
        assert_eq!(
            code,
            step["exit"].as_i64().unwrap() as i32,
            "{ctx}: the reference papers: {stderr}"
        );
        assert_eq!(
            norm_lines(&stdout, &r),
            norm_lines(&out.join("\n"), e),
            "{ctx}: the reference papers on the store of papers-rs"
        );
        assert_same_store(
            e,
            &r,
            &format!("{ctx} (the reference papers on the store of papers-rs)"),
        );
    }
}

fn listing(e: &Universe, fx: &Value, step: &Value, ctx: &str) {
    let args = argv(step);
    let mut config = if step.get("bibtex").is_some() || step["lib"].is_string() {
        config_for(e, fx, step)
    } else {
        Config::new(machine_for(e, fx))
    };
    let mut setup = Setup::from_config(&config);
    setup.cwd = e.work();
    let lines = if args.iter().any(|a| a == "--all") {
        all_lines(&setup)
    } else {
        let history = History::open(&mut config, false, false).unwrap();
        history_lines(
            &setup,
            history.as_ref(),
            config.bibtex.is_some(),
            option_n(&args, 20),
        )
    };
    let got = norm_lines(&lines.join("\n"), e);
    assert_eq!(
        got,
        norm_lines(step["stdout"].as_str().unwrap(), e),
        "{ctx}: the lines of papers history"
    );
}

/// The new states of the store delta of a step: (project id, number, state).
fn new_states(e: &Universe, step: &Value) -> Vec<(String, u64, J)> {
    let mut found = Vec::new();
    for (rel, entry) in step["store"]["set"].as_object().unwrap() {
        let parts: Vec<&str> = rel.split('/').collect();
        if let [.., "projects", id, "states", name] = parts.as_slice()
            && let Some(number) = name.strip_suffix(".json")
        {
            let text = entry["text"].as_str().unwrap().replace("$TMP", &e.root);
            found.push((
                e.map_rel(id),
                number.parse::<u64>().unwrap(),
                J::parse(&text).unwrap(),
            ));
        }
    }
    found.sort_by_key(|f| f.1);
    found
}

fn state_ops(state: &J, libdir: &str) -> Vec<FileOp> {
    let ops: Vec<FileOp> = state
        .get("ops")
        .and_then(J::as_array)
        .unwrap()
        .iter()
        .filter_map(FileOp::from_j)
        .collect();
    papers_rs::history::ops::absolute_ops(&ops, libdir)
}

/// papers-rs records what a command recorded.
fn record_command(e: &Universe, want: &Universe, fx: &Value, step: &Value, ctx: &str) -> bool {
    let states = new_states(e, step);
    if states.is_empty() {
        // a command that records nothing (init, a failure): the files it wrote
        e.apply(&e.work(), &step["work"], &|_| true);
        return false;
    }
    // the libraries: (id, files of the working folder that are its own: a folder, or the bibtex without library, configuration)
    let mut libraries: Vec<(String, String, Config)> = Vec::new();
    if let Some(bibtex) = step.get("bibtex").and_then(Value::as_str) {
        let place = realpath(&format!("{}/{bibtex}", e.work()));
        let mut config = config_for(e, fx, step);
        config.bibtex = Some(place.clone());
        libraries.push((path_id(&place), bibtex.to_string(), config));
    } else {
        let mut names: Vec<_> = fs::read_dir(want.work()).unwrap().flatten().collect();
        names.sort_by_key(|n| n.file_name());
        for name in names {
            let after = format!("{}/.papers/config.json", name.path().display());
            let dir = name.file_name().to_string_lossy().into_owned();
            let before = format!("{}/{dir}/.papers/config.json", e.work());
            let Ok(text) = fs::read_to_string(&after) else {
                continue;
            };
            let Some(id) =
                J::parse(&text).and_then(|j| j.get("id").and_then(J::as_str).map(str::to_string))
            else {
                continue;
            };
            if !Path::new(&before).exists() {
                continue;
            }
            align_library_id(e, want, fx, &dir, ctx);
            let config = Config::load(&before, machine_for(e, fx), None).unwrap();
            assert_eq!(config.id.as_deref(), Some(id.as_str()));
            libraries.push((id, dir, config));
        }
    }
    // the library of a project: a pool is another folder than the library of the command (copies share an identifier)
    let command_lib = step["lib"].as_str().map(str::to_string);
    let meta: Vec<(String, String)> = libraries
        .iter()
        .map(|l| (l.0.clone(), l.1.clone()))
        .collect();
    let pick = |pid: &str, pool: bool| -> usize {
        let candidates: Vec<usize> = (0..meta.len()).filter(|i| meta[*i].0 == pid).collect();
        candidates
            .iter()
            .copied()
            .find(|i| (Some(&meta[*i].1) != command_lib.as_ref()) == pool)
            .or(candidates.first().copied())
            .unwrap_or_else(|| panic!("{ctx}: no library for the project {pid}"))
    };
    let is_owned = |rel: &str, own: &str| rel == own || rel.starts_with(&format!("{own}/"));
    // the projects written, the pool first (its states name the library of the command)
    let mut projects: Vec<(String, bool)> = Vec::new();
    for (pid, _, state) in &states {
        if state.get("kind").and_then(J::as_str) == Some("command")
            && !projects.iter().any(|(p, _)| p == pid)
        {
            projects.push((pid.clone(), state.get("from").is_some()));
        }
    }
    projects.sort_by_key(|(_, from)| std::cmp::Reverse(*from));
    // the library of the command, as it is before anything is written
    let mut before_texts = BTreeMap::new();
    for (pid, from) in &projects {
        if !from {
            let i = pick(pid, false);
            before_texts.insert(pid.clone(), History::before_command(&mut libraries[i].2));
        }
    }
    // what is not in a library (files moved into it, downloads) comes first
    e.apply(&e.work(), &step["work"], &|rel| {
        !meta.iter().any(|l| is_owned(rel, &l.1))
    });
    let mut links: Vec<(String, PoolLink)> = Vec::new();
    let mut done: Vec<String> = Vec::new();
    for (pid, from) in &projects {
        let i = pick(pid, *from);
        let mut history = History::open(&mut libraries[i].2, true, false)
            .unwrap()
            .unwrap();
        history.trash = |p| drop(fs::remove_file(p)); // never the trash of the machine
        history
            .sync_head(before_texts.get(pid).and_then(Option::as_ref))
            .unwrap();
        let (_, number, state) = states
            .iter()
            .rev()
            .find(|(p, _, s)| p == pid && s.get("kind").and_then(J::as_str) == Some("command"))
            .unwrap();
        let ops = state_ops(state, &history.setup.libdir);
        // what the command deleted goes into the store before its state is recorded
        let deleter = Deleter {
            keep: true,
            store: Some(&history.store),
            trash: &|p| drop(fs::remove_file(p)),
        };
        for op in ops.iter().filter(|o| o.kind == OpKind::Trashed) {
            let mut recorded = Some(Vec::new());
            deleter
                .remove(&op.src, &mut recorded)
                .unwrap_or_else(|error| panic!("{ctx}: {error}"));
            assert_eq!(
                recorded.unwrap()[0].checksum,
                op.checksum,
                "{ctx}: the checksum of a deleted file"
            );
        }
        let own = libraries[i].1.clone();
        e.apply(&e.work(), &step["work"], &|rel| is_owned(rel, &own));
        done.push(own);
        let recorded = history
            .record(Step {
                message: state
                    .get("message")
                    .and_then(J::as_str)
                    .unwrap()
                    .to_string(),
                ops,
                snapshot: None,
                pools: if *from { vec![] } else { links.clone() },
                from: state.get("from").and_then(J::as_str).map(str::to_string),
                reverts: state.get("reverts").map(J::numbers).unwrap_or_default(),
            })
            .unwrap();
        assert_eq!(
            recorded.number,
            Some(*number),
            "{ctx}: the number of the state"
        );
        links.extend(recorded.link);
    }
    e.apply(&e.work(), &step["work"], &|rel| {
        meta.iter().any(|l| is_owned(rel, &l.1)) && !done.iter().any(|d| is_owned(rel, d))
    });
    assert_same_store(e, want, ctx);
    true
}

fn run_fixture(name: &str) -> Counts {
    run(name, None)
}

/// Replay a scenario. With a `reference` papers (a command), the store is the one that papers-rs
/// writes, step after step, and the reference papers undoes, redoes and restores it too.
fn run(name: &str, reference: Option<&str>) -> Counts {
    let fx = json_file(&dir("history").join(format!("{name}.json")));
    let bibs: Vec<(String, String)> = fx["place_ids"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect();
    let want = Universe::new(&bibs);
    let mut chain = Universe::new(&bibs);
    let mut counts = Counts::default();
    for (i, step) in fx["steps"].as_array().unwrap().iter().enumerate() {
        let kind = step["kind"].as_str().unwrap();
        if kind == "age" {
            want.age(step["seconds"].as_u64().unwrap());
            chain.age(step["seconds"].as_u64().unwrap());
            continue;
        }
        if kind != "papers" {
            want.apply_all(step);
            if reference.is_some() {
                chain.apply_all(step);
            }
            continue;
        }
        let args = argv(step);
        let ctx = format!("{name}, step {i} (papers {})", args.join(" "));
        // papers-rs works on the store as papers left it, or (live) as papers-rs left it
        let e = if reference.is_some() {
            chain.copy()
        } else {
            want.copy()
        };
        want.apply_all(step);
        match args[0].as_str() {
            "undo" | "redo" | "restore" => {
                time_travel(&e, &want, &fx, step, &ctx, reference.map(|r| (r, &chain)));
                counts.undone += 1;
            }
            "history" if args.iter().any(|a| a == "--remove" || a == "--drop-files") => {
                history_edit(&e, &want, &fx, step, &ctx, reference.map(|r| (r, &chain)));
                counts.edited += 1;
            }
            "history" => {
                listing(&e, &fx, step, &ctx);
                counts.listed += 1;
            }
            _ => {
                if record_command(&e, &want, &fx, step, &ctx) {
                    counts.recorded += 1;
                }
            }
        }
        if reference.is_some() {
            chain = e;
        }
    }
    counts
}

/// The reference papers (`PAPERS_REFERENCE`, a command; `PAPERS_REFERENCE_B5` for the scenarios of papers
/// 3.0.0b5): the scenarios again, on the store that papers-rs writes.
fn live(name: &str, variable: &str) {
    let Ok(reference) = std::env::var(variable) else {
        eprintln!("{name}: skipped, {variable} is not set");
        return;
    };
    let c = run(name, Some(&reference));
    assert!(c.undone + c.edited > 0, "{c:?}");
}

#[test]
#[ignore = "needs the reference papers: PAPERS_REFERENCE=/path/to/papers cargo test -p papers-rs --test history -- --ignored"]
fn the_reference_papers_undoes_what_papers_rs_recorded() {
    for name in [
        "basic",
        "many",
        "files",
        "outside",
        "copies",
        "separate",
        "bibtex",
        "bibtex-separate",
        "remove",
    ] {
        live(name, "PAPERS_REFERENCE");
    }
    live("pools", "PAPERS_REFERENCE_B5");
}

#[test]
fn basic() {
    let c = run_fixture("basic");
    assert!(c.undone >= 12 && c.recorded >= 4 && c.listed >= 5, "{c:?}");
}

#[test]
fn a_large_bibtex_in_pieces() {
    let c = run_fixture("many");
    assert!(c.undone >= 5 && c.recorded >= 4, "{c:?}");
}

#[test]
fn files_moved_deleted_and_restored() {
    let c = run_fixture("files");
    assert!(c.undone >= 15 && c.recorded >= 8 && c.edited >= 5, "{c:?}");
}

#[test]
fn the_history_of_a_place_and_of_a_library_is_removed() {
    let c = run_fixture("remove");
    assert!(c.edited >= 5 && c.recorded >= 3, "{c:?}");
}

#[test]
fn changes_made_outside_papers() {
    let c = run_fixture("outside");
    assert!(c.undone >= 6 && c.recorded >= 2, "{c:?}");
}

#[test]
fn copies_and_moved_libraries() {
    let c = run_fixture("copies");
    assert!(c.undone >= 2 && c.recorded >= 4, "{c:?}");
}

#[test]
fn a_store_of_its_own() {
    let c = run_fixture("separate");
    assert!(c.undone >= 2 && c.recorded >= 2, "{c:?}");
}

#[test]
fn a_bibtex_without_library() {
    let c = run_fixture("bibtex");
    assert!(c.undone >= 3 && c.recorded >= 2, "{c:?}");
}

#[test]
fn undo_on_a_library_without_history() {
    let c = run_fixture("fresh");
    assert!(c.undone >= 5 && c.recorded >= 1, "{c:?}");
}

#[test]
fn a_bibtex_without_library_with_history_separate() {
    let c = run_fixture("bibtex-separate");
    assert!(c.undone >= 1 && c.recorded >= 1, "{c:?}");
}

#[test]
fn a_shallow_library_and_its_pool() {
    let c = run_fixture("pools");
    assert!(c.undone >= 5 && c.recorded >= 3, "{c:?}");
}

// the retention
// =============

#[test]
fn the_retention_drops_what_papers_drops() {
    let fx = json_file(&dir("history").join("retention.json"));
    for case in fx["cases"].as_array().unwrap() {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        for (rel, entry) in fx["store"].as_object().unwrap() {
            let path = format!("{root}/{rel}");
            fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
            fs::write(&path, b64_decode(entry["b64"].as_str().unwrap())).unwrap();
            let when =
                SystemTime::UNIX_EPOCH + Duration::from_secs_f64(entry["mtime"].as_f64().unwrap());
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(when)
                .unwrap();
        }
        let store = Store::new(root.clone());
        let gone = RefCell::new(Vec::new());
        let discard = |p: &str| {
            gone.borrow_mut()
                .push(p.strip_prefix(&format!("{root}/")).unwrap().to_string());
            fs::remove_file(p).unwrap();
        };
        let ctx = format!(
            "limit {} extra {} dry {}",
            case["limit"], case["extra"], case["dry"]
        );
        let dropped = store.prune_at(
            case["limit"].as_u64().unwrap(),
            case["extra"].as_u64().unwrap(),
            case["dry"].as_bool().unwrap(),
            &discard,
            case["now"].as_f64().unwrap(),
        );
        let got: Vec<(String, u64)> = dropped
            .iter()
            .map(|d| (d.project.clone(), d.number))
            .collect();
        let want: Vec<(String, u64)> = case["dropped"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| (d[0].as_str().unwrap().to_string(), d[1].as_u64().unwrap()))
            .collect();
        assert_eq!(got, want, "{ctx}: the states dropped");
        let mut gone = gone.into_inner();
        gone.sort();
        let want_gone: Vec<&str> = case["discarded"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        assert_eq!(gone, want_gone, "{ctx}: the files sent to the trash");
        let after: Vec<String> = files_of(Path::new(&root)).into_keys().collect();
        let want_after: Vec<&str> = case["after"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        assert_eq!(after, want_after, "{ctx}: what is left");
    }
    let _ = now();
}
