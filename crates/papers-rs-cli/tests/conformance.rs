//! papers-rs does what papers-cli does: the cases recorded from papers-cli
//! (tests/record/record_cases.py, fixtures/cases/*.json) run through the
//! papers-rs binary, and what it prints and leaves must be the same.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

#[path = "common/openalex.rs"]
mod openalex;

/// The global configuration of papers, relative to the home directory, on this platform.
const MC: &str = if cfg!(target_os = "macos") { "Library/Application Support/papers/config.json" } else { ".config/papers/config.json" };
/// The global configuration of papers 2.
const V2: &str = if cfg!(target_os = "macos") { "Library/Application Support/papersconfig.json" } else { ".config/papersconfig.json" };

const IGNORED: [&str; 2] = ["home/Library/Caches/", "home/.cache/"];

const NOT_IMPLEMENTED: &str = "papers: error: not implemented in papers-rs yet\n";

/// The command of the arguments is one that papers-rs has.
fn is_ported(args: &[String]) -> bool {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "-C" {
            args.next();
        } else if !arg.starts_with('-') {
            return arg == "init" || arg == "config";
        }
    }
    false
}

fn real_of(key: &str) -> String {
    match key {
        "home/<MC>" => format!("home/{MC}"),
        "home/<V2>" => format!("home/{V2}"),
        other => other.to_string(),
    }
}

fn key_of(rel: &str) -> String {
    if rel == format!("home/{MC}") {
        "home/<MC>".to_string()
    } else if rel == format!("home/{V2}") {
        "home/<V2>".to_string()
    } else {
        rel.to_string()
    }
}

fn normalise(text: &str, tmp: &str) -> String {
    text.replace(&format!("{tmp}/home/{MC}"), "<MC>")
        .replace(&format!("~/{MC}"), "<MC~>")
        .replace(&format!("{tmp}/home/{V2}"), "<V2>")
        .replace(&format!("~/{V2}"), "<V2~>")
        .replace(tmp, "<TMP>")
}

fn write(path: &Path, content: &Value, tmp: &str) {
    match content {
        Value::Array(link) => {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(link[1].as_str().unwrap(), path).unwrap();
        }
        Value::Null => fs::create_dir_all(path).unwrap(),
        Value::String(text) => {
            if path.to_string_lossy().ends_with('/') {
                fs::create_dir_all(path).unwrap();
            } else {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, text.replace("<TMP>", tmp)).unwrap();
            }
        }
        other => panic!("bad file content {other}"),
    }
}

fn snapshot(tmp: &Path) -> BTreeMap<String, Option<String>> {
    fn walk(dir: &Path, tmp: &Path, tree: &mut BTreeMap<String, Option<String>>) {
        let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap()).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let rel = path.strip_prefix(tmp).unwrap().to_string_lossy().into_owned();
            let meta = fs::symlink_metadata(&path).unwrap();
            let tmp_str = tmp.to_string_lossy();
            if meta.file_type().is_symlink() {
                let target = fs::read_link(&path).unwrap().to_string_lossy().into_owned();
                if !IGNORED.iter().any(|p| rel.starts_with(p)) {
                    tree.insert(key_of(&rel), Some(format!("-> {target}")));
                }
            } else if meta.is_dir() {
                let empty = fs::read_dir(&path).unwrap().next().is_none();
                if empty && rel != "home" && rel != "cwd" && !IGNORED.iter().any(|p| rel.starts_with(p.trim_end_matches('/'))) {
                    tree.insert(format!("{}/", key_of(&rel)), None);
                }
                walk(&path, tmp, tree);
            } else if !IGNORED.iter().any(|p| rel.starts_with(p)) && !rel.ends_with(".papers/lock") {
                let content = match fs::read(&path).ok().and_then(|b| String::from_utf8(b).ok()) {
                    Some(text) => normalise(&text, &tmp_str),
                    None => "<binary>".to_string(),
                };
                tree.insert(key_of(&rel), Some(content));
            }
        }
    }
    let mut tree = BTreeMap::new();
    walk(tmp, tmp, &mut tree);
    tree
}

fn run_case(case: &Value) -> Vec<String> {
    let name = case["name"].as_str().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let tmp_path = fs::canonicalize(dir.path()).unwrap();
    let tmp = tmp_path.to_string_lossy().into_owned();
    fs::create_dir_all(tmp_path.join("home")).unwrap();
    fs::create_dir_all(tmp_path.join("cwd")).unwrap();
    for (rel, content) in case["home"].as_object().unwrap() {
        let real = real_of(&format!("home/{rel}"));
        write(&tmp_path.join(real), content, &tmp);
    }
    for (rel, content) in case["cwd"].as_object().unwrap() {
        write(&tmp_path.join("cwd").join(rel), content, &tmp);
    }
    let mut failures = Vec::new();
    for (i, step) in case["steps"].as_array().unwrap().iter().enumerate() {
        for (rel, content) in step["write"].as_object().unwrap() {
            write(&tmp_path.join(real_of(rel)), content, &tmp);
        }
        for rel in step["rm"].as_array().unwrap() {
            fs::remove_file(tmp_path.join(real_of(rel.as_str().unwrap()))).unwrap();
        }
        let args: Vec<String> = step["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().replace("<TMP>", &tmp)).collect();
        let mut command = Command::new(env!("CARGO_BIN_EXE_papers-rs"));
        command
            .args(&args)
            .current_dir(tmp_path.join("cwd").join(step["cwd"].as_str().unwrap()))
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", tmp_path.join("home"))
            .env("LC_ALL", "C.UTF-8")
            .stdin(Stdio::null());
        for (key, value) in step["env"].as_object().unwrap() {
            command.env(key, value.as_str().unwrap());
        }
        let output = command.output().unwrap();
        let rc = output.status.code().unwrap_or(-1);
        let stdout = normalise(&String::from_utf8_lossy(&output.stdout), &tmp);
        let stderr = normalise(&String::from_utf8_lossy(&output.stderr), &tmp);
        // a command that papers-rs does not have yet: its arguments were accepted as papers accepts them
        if stderr == NOT_IMPLEMENTED && rc == 1 && stdout.is_empty() && step["rc"].as_i64() != Some(2) && !is_ported(&args) {
            continue;
        }
        let mut problems = Vec::new();
        if rc as i64 != step["rc"].as_i64().unwrap() {
            problems.push(format!("exit status {rc}, expected {}", step["rc"]));
        }
        if !openalex::is_as_papers(&stdout, step["stdout"].as_str().unwrap()) {
            problems.push(format!("stdout:\n{stdout}\n--- expected:\n{}", step["stdout"].as_str().unwrap()));
        }
        if !openalex::is_as_papers(&stderr, step["stderr"].as_str().unwrap()) {
            problems.push(format!("stderr:\n{stderr}\n--- expected:\n{}", step["stderr"].as_str().unwrap()));
        }
        let tree = snapshot(&tmp_path);
        let expected: BTreeMap<String, Option<String>> = step["tree"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().map(str::to_string)))
            .collect();
        for (key, content) in &tree {
            match expected.get(key) {
                None => problems.push(format!("file written that papers does not write: {key}")),
                Some(want) if want != content => {
                    problems.push(format!("{key}:\n{}\n--- expected:\n{}", content.clone().unwrap_or_default(), want.clone().unwrap_or_default()))
                }
                _ => {}
            }
        }
        for key in expected.keys() {
            if !tree.contains_key(key) {
                problems.push(format!("file missing: {key}"));
            }
        }
        if !problems.is_empty() {
            failures.push(format!("{name}, step {i}: papers {args:?}\n  {}", problems.join("\n  ").replace(&tmp, "<TMP>")));
        }
    }
    failures
}

#[test]
fn conformance_cases() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cases");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    files.sort();
    assert!(!files.is_empty(), "no fixtures in {}", dir.display());
    let filter = std::env::var("PAPERS_CASE").ok();
    let mut failures = Vec::new();
    let mut steps = 0;
    for file in files {
        let case: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        if filter.as_deref().is_some_and(|f| case["name"].as_str() != Some(f)) {
            continue;
        }
        if case["platform"].as_str() == Some("macos") && !cfg!(target_os = "macos") {
            continue;
        }
        steps += case["steps"].as_array().unwrap().len();
        failures.extend(run_case(&case));
    }
    if !failures.is_empty() {
        panic!("{} of {steps} steps differ from papers:\n\n{}", failures.len(), failures.join("\n\n"));
    }
}
