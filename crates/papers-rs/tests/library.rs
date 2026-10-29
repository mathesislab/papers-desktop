//! The library: sequences of operations (insert, remove, rename a key, tags, files, check
//! duplicates), the text saved, and the files placed by the name template, papers-rs against
//! what papers-cli did (`tests/gen_library_expected.py`, `expected/library/library.json` and
//! `files.json`); and what papers does with a save, as unit tests.

use std::collections::BTreeMap;
use std::path::Path;

use papers_rs::bib::Bib;
use papers_rs::config::paths::relpath;
use papers_rs::library::{Action, InsertOptions, Library, Policy, SaveStatus, What};
use papers_rs::merge::{OnDuplicate, Resolver};
use papers_rs::names::{Format, FormatKind};
use serde_json::{Value, json};
use sha2::Digest;

mod common;
use common::library_json;
use common::tally::Tally;

fn format_of(j: &Value, kind: FormatKind, default: Format) -> Format {
    if j.is_null() {
        return default;
    }
    let mut f = default;
    f.kind = kind;
    f.template = j["template"].as_str().unwrap().to_string();
    f.author_num = j["author_num"].as_i64().unwrap();
    f.author_sep = j["author_sep"].as_str().unwrap().to_string();
    f.title_word_num = j["title_word_num"].as_i64().unwrap();
    f.title_word_size = j["title_word_size"].as_i64().unwrap();
    f.title_length = j["title_length"].as_i64().unwrap();
    f.title_sep = j["title_sep"].as_str().unwrap().to_string();
    f.unknown_strict = j["unknown_strict"].as_bool().unwrap();
    f.unknown_template = j["unknown_template"].as_str().map(str::to_string);
    f
}

fn mode(s: &str) -> OnDuplicate {
    OnDuplicate::parse(s).unwrap()
}

/// What an operation gave: the result or the error, and what papers printed.
fn apply(lib: &mut Library, root: &str, op: &Value) -> Value {
    let key = || op["key"].as_str().unwrap().to_string();
    let result: Result<(Value, Vec<String>), papers_rs::error::Error> = (|| {
        let mut out: Vec<String> = Vec::new();
        let value = match op["op"].as_str().unwrap() {
            "add" => {
                let o = &op["options"];
                let options = InsertOptions {
                    update_key: o["update_key"].as_bool().unwrap_or(false),
                    check_duplicate: o["check_duplicate"].as_bool().unwrap_or(false),
                    rename: None,
                    metadata: o["metadata"]
                        .as_object()
                        .map(|m| {
                            m.iter()
                                .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
                                .collect()
                        })
                        .unwrap_or_default(),
                };
                let mut resolver =
                    Resolver::new(mode(o["on_duplicate"].as_str().unwrap_or("error")));
                let rel = op["relative_to"].as_str().map(|r| format!("{root}/{r}"));
                let inserted = lib.add_bibtex(
                    op["bibtex"].as_str().unwrap(),
                    rel.as_deref(),
                    &[],
                    false,
                    &options,
                    &mut resolver,
                )?;
                let mut keys = Vec::new();
                for i in inserted {
                    match &i.what {
                        What::Skipped => out.push(format!(
                            "skipped: {} (already in library: {})",
                            i.key,
                            i.existing.as_ref().unwrap()
                        )),
                        What::Merged {
                            already_in_library: true,
                        } => out.push(format!("already in library: {}", i.key)),
                        _ => {}
                    }
                    if i.what != What::Skipped {
                        keys.push(i.key);
                    }
                }
                json!(keys)
            }
            "remove" => {
                lib.remove(&key()).unwrap();
                Value::Null
            }
            "set_key" => {
                lib.set_key(
                    &key(),
                    op["new"].as_str().unwrap(),
                    op["keep_old"].as_bool().unwrap_or(false),
                )?;
                Value::Null
            }
            "tags_add" => {
                lib.add_tags(&common::strings(&op["keys"]), &common::strings(&op["tags"]));
                Value::Null
            }
            "tags_remove" => {
                lib.remove_tags(&common::strings(&op["keys"]), &common::strings(&op["tags"]));
                Value::Null
            }
            "tag_rename" => json!(
                lib.rename_tag(op["old"].as_str().unwrap(), op["new"].as_str().unwrap())
                    .len()
            ),
            "set_files" => {
                let files: Vec<String> = common::strings(&op["files"])
                    .iter()
                    .map(|f| format!("{root}/{f}"))
                    .collect();
                lib.set_files(&key(), &files)?;
                Value::Null
            }
            "files" => json!(
                lib.files(&key())
                    .iter()
                    .map(|f| relpath(f, root))
                    .collect::<Vec<_>>()
            ),
            "check_duplicates" => {
                let mut resolver = Resolver::new(mode(op["mode"].as_str().unwrap()));
                for (target, others) in lib.check_duplicates(&mut resolver)? {
                    out.push(format!(
                        "merged into {target}: {} (kept in ids)",
                        others.join(", ")
                    ));
                }
                Value::Null
            }
            "regenerate_key" => {
                // `check --fix-key`: an entry of the library; empty when the template gives none
                let entry = lib
                    .get(&key())
                    .cloned()
                    .ok_or_else(|| papers_rs::error::Error::exit("no entry"))?;
                json!(lib.regenerate_key(&entry)?)
            }
            "generate_key" => {
                let entries: Vec<_> = Bib::parse(op["bibtex"].as_str().unwrap())
                    .entries()
                    .cloned()
                    .collect();
                let mut keys = Vec::new();
                for e in &entries {
                    keys.push(lib.generate_key(e)?);
                }
                json!(keys)
            }
            "relative_to" => {
                let new = if op["to"] == "absolute" {
                    "/".to_string()
                } else {
                    format!("{root}/lib")
                };
                let updates = lib.set_relative_to(&new);
                json!(
                    updates
                        .iter()
                        .map(|(a, b)| json!([a.replace(root, "$"), b.replace(root, "$")]))
                        .collect::<Vec<_>>()
                )
            }
            other => panic!("{other}"),
        };
        Ok((value, out))
    })();
    match result {
        Ok((value, out)) => json!({"result": value, "stdout": out}),
        Err(e) => json!({"error": e.message(), "stdout": null}),
    }
}

#[test]
fn sequences_of_operations_as_papers() {
    let cases = library_json("library.json");
    let mut tally = Tally::default();
    let mut n_errors = 0;
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir_all(format!("{root}/lib")).unwrap();
        let bibtex = format!("{root}/lib/papers.bib");
        std::fs::write(&bibtex, case["library"].as_str().unwrap()).unwrap();
        let nameformat = format_of(&case["nameformat"], FormatKind::Name, Format::name());
        let keyformat = format_of(&case["keyformat"], FormatKind::Key, Format::key());
        let mut lib = Library::load(
            &bibtex,
            Some(&format!("{root}/lib/files")),
            None,
            nameformat,
            keyformat,
        )
        .unwrap();
        lib.mark_loaded();
        let text = case["library"].as_str().unwrap().to_string() + &case["ops"].to_string();
        for (op, expected) in case["ops"]
            .as_array()
            .unwrap()
            .iter()
            .zip(case["steps"].as_array().unwrap())
        {
            let got = apply(&mut lib, &root, op);
            if expected.get("error").is_some() {
                n_errors += 1;
                assert_eq!(got["error"], expected["error"], "{name}: {op}");
            } else {
                tally.check(
                    name,
                    &text,
                    got["result"].clone(),
                    expected["result"].clone(),
                );
                assert_eq!(got["stdout"], expected["stdout"], "{name}: {op}");
            }
        }
        let saved = lib.save(false).unwrap();
        let expected = &case["saved"];
        assert_eq!(
            saved.status == SaveStatus::Unchanged,
            expected["unchanged"].as_bool().unwrap(),
            "{name}"
        );
        let on_disk = std::fs::read_to_string(&bibtex).unwrap();
        tally.check(
            name,
            &text,
            on_disk,
            expected["text"].as_str().unwrap().to_string(),
        );
        let distinct = std::fs::read_to_string(format!("{root}/lib/.papers/distinct.json")).ok();
        assert_eq!(
            distinct.as_deref(),
            expected["distinct"].as_str(),
            "{name}: distinct.json"
        );
        let (new, modified) = lib.changed_entries();
        assert_eq!(
            new,
            common::strings(&expected["new"]),
            "{name}: new entries"
        );
        assert_eq!(
            modified,
            common::strings(&expected["modified"]),
            "{name}: modified entries"
        );
        if saved.status != SaveStatus::Unchanged {
            // what the history gets: the text before, and the text after
            assert_eq!(saved.before.as_deref(), case["library"].as_str());
            assert_eq!(saved.after, std::fs::read_to_string(&bibtex).unwrap());
        }
    }
    tally.report("library");
    assert!(
        tally.cases > 150 && n_errors >= 10,
        "{} cases, {n_errors} errors",
        tally.cases
    );
}

// the files of an entry
// ---------------------

fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                out.insert(
                    p.strip_prefix(root).unwrap().to_string_lossy().into_owned(),
                    std::fs::read(&p).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

#[test]
fn files_placed_as_papers() {
    let mut count = 0;
    for case in library_json("files.json").as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let rootstr = root.to_string_lossy().into_owned();
        for (path, content) in case["files"].as_object().unwrap() {
            let p = root.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content.as_str().unwrap()).unwrap();
        }
        let bibtex = format!("{rootstr}/lib/papers.bib");
        std::fs::create_dir_all(format!("{rootstr}/lib")).unwrap();
        std::fs::write(&bibtex, case["bibtex"].as_str().unwrap()).unwrap();
        let nameformat = format_of(&case["nameformat"], FormatKind::Name, Format::name());
        let filesdir = format!("{rootstr}/{}", case["filesdir"].as_str().unwrap());
        let mut lib =
            Library::load(&bibtex, Some(&filesdir), None, nameformat, Format::key()).unwrap();
        lib.mark_loaded();
        let policy = match case["policy"].as_str().unwrap() {
            "move" => Policy::all(Action::Move),
            "copy" => Policy::all(Action::Copy),
            "link" => Policy::all(Action::Link),
            _ => Policy::by_file(|f| {
                if f.ends_with(".pdf") {
                    Action::Move
                } else {
                    Action::Copy
                }
            }),
        };
        let keys: Vec<String> = lib.entries().map(|e| e.key.clone()).collect();
        let mut errors = Vec::new();
        for key in keys {
            if let Some(only) = case["keys"].as_array()
                && !only.iter().any(|k| k == key.as_str())
            {
                continue;
            }
            errors.push(
                lib.rename_entry_files(&key, &policy, None, None, true)
                    .err()
                    .map(|e| e.message().to_string()),
            );
        }
        for op in case["extra"].as_array().unwrap() {
            let key = op["key"].as_str().unwrap();
            match op["op"].as_str().unwrap() {
                "set_files" => {
                    let files: Vec<String> = common::strings(&op["files"])
                        .iter()
                        .map(|f| format!("{rootstr}/{f}"))
                        .collect();
                    lib.set_files(key, &files).unwrap();
                }
                _ => lib
                    .rename_entry_files(key, &policy, None, None, true)
                    .unwrap(),
            }
        }
        let ops = lib.file_ops().len();
        lib.save(true).unwrap();
        assert_eq!(json!(errors), case["errors"], "{name}");
        assert_eq!(
            std::fs::read_to_string(&bibtex).unwrap(),
            case["saved"].as_str().unwrap(),
            "{name}: the bibtex"
        );
        let expected: BTreeMap<String, Vec<u8>> = case["tree"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.as_str().unwrap().chars().map(|c| c as u8).collect(),
                )
            })
            .collect();
        assert_eq!(tree(&root), expected, "{name}: the files");
        let _ = ops;
        count += 1;
    }
    assert!(count > 30);
}

// what a save does
// ----------------

const TEXT: &str = "@article{b2020,\n title = {B},\n year = {2020}\n}\n\n@article{a2020,\n title = {A},\n year = {2020}\n}\n";

fn library_on_disk(text: &str) -> (tempfile::TempDir, String, Library) {
    let dir = tempfile::tempdir().unwrap();
    let bibtex = format!(
        "{}/papers.bib",
        std::fs::canonicalize(dir.path()).unwrap().display()
    );
    std::fs::write(&bibtex, text).unwrap();
    let mut lib = Library::load(&bibtex, None, None, Format::name(), Format::key()).unwrap();
    lib.mark_loaded();
    (dir, bibtex, lib)
}

fn mtime(path: &str) -> std::time::SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

#[test]
fn an_unchanged_library_is_not_written() {
    // the text is not the sorted text: a save would write it
    let (_dir, bibtex, mut lib) = library_on_disk(TEXT);
    let before = mtime(&bibtex);
    let saved = lib.save(false).unwrap();
    assert_eq!(saved.status, SaveStatus::Unchanged);
    assert_eq!(std::fs::read_to_string(&bibtex).unwrap(), TEXT);
    assert_eq!(mtime(&bibtex), before);
    // `other_changes`: the save goes on, and writes the sorted text
    let saved = lib.save(true).unwrap();
    assert_eq!(saved.status, SaveStatus::Written);
    assert!(
        std::fs::read_to_string(&bibtex)
            .unwrap()
            .starts_with("@article{a2020")
    );
    assert_eq!(saved.before.as_deref(), Some(TEXT));
}

#[test]
fn a_text_that_is_on_disk_is_not_written_again() {
    let (_dir, bibtex, mut lib) = library_on_disk(TEXT);
    lib.add_tags(&["a2020".to_string()], &["x".to_string()]);
    assert_eq!(lib.save(false).unwrap().status, SaveStatus::Written);
    let written = mtime(&bibtex);
    std::thread::sleep(std::time::Duration::from_millis(20));
    // a change that gives the text that is there
    lib.remove_tags(&["a2020".to_string()], &["x".to_string()]);
    lib.add_tags(&["a2020".to_string()], &["x".to_string()]);
    let saved = lib.save(true).unwrap();
    assert_eq!(saved.status, SaveStatus::Identical);
    assert_eq!(mtime(&bibtex), written);
}

#[test]
fn a_bibtex_that_another_program_wrote_is_not_written() {
    let (_dir, bibtex, mut lib) = library_on_disk(TEXT);
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&bibtex, format!("{TEXT}\n@misc{{c,\n title = {{C}}\n}}\n")).unwrap();
    lib.add_tags(&["a2020".to_string()], &["x".to_string()]);
    let err = lib.save(false).unwrap_err();
    assert!(
        err.to_string().contains("changed since papers read it"),
        "{err}"
    );
    assert!(
        std::fs::read_to_string(&bibtex)
            .unwrap()
            .contains("@misc{c")
    );
    // the same text written again (a sync client touching the file) is not a change
    let (_dir, bibtex, mut lib) = library_on_disk(TEXT);
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&bibtex, TEXT).unwrap();
    lib.add_tags(&["a2020".to_string()], &["x".to_string()]);
    assert_eq!(lib.save(false).unwrap().status, SaveStatus::Written);
}

#[test]
fn a_save_keeps_the_permissions_and_follows_a_link() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, bibtex, mut lib) = library_on_disk(TEXT);
    std::fs::set_permissions(&bibtex, std::fs::Permissions::from_mode(0o640)).unwrap();
    lib.add_tags(&["a2020".to_string()], &["x".to_string()]);
    lib.save(false).unwrap();
    assert_eq!(
        std::fs::metadata(&bibtex).unwrap().permissions().mode() & 0o777,
        0o640
    );
    // through a link: the target is written
    let target = format!("{}/real.bib", dir.path().display());
    std::fs::rename(&bibtex, &target).unwrap();
    std::os::unix::fs::symlink(&target, &bibtex).unwrap();
    let mut lib = Library::load(&bibtex, None, None, Format::name(), Format::key()).unwrap();
    lib.mark_loaded();
    lib.add_tags(&["b2020".to_string()], &["y".to_string()]);
    lib.save(false).unwrap();
    assert!(
        std::fs::symlink_metadata(&bibtex)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        std::fs::read_to_string(&target)
            .unwrap()
            .contains("keywords = {y}")
    );
}

#[test]
fn renamed_keys_follow_in_crossref_and_are_returned() {
    let text = "@book{h,\n title = {H}\n}\n\n@incollection{c,\n crossref = {H},\n title = {C}\n}\n";
    let (_dir, bibtex, mut lib) = library_on_disk(text);
    lib.set_key("h", "h2", false).unwrap();
    assert_eq!(lib.renamed_keys(), [("h".to_string(), "h2".to_string())]);
    let saved = lib.save(false).unwrap();
    assert_eq!(saved.renamed_keys, [("h".to_string(), "h2".to_string())]);
    assert!(
        std::fs::read_to_string(&bibtex)
            .unwrap()
            .contains("crossref = {h2}")
    );
}

#[test]
fn a_new_bibtex_is_created_and_the_file_operations_are_returned() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let bibtex = format!("{root}/sub/papers.bib");
    let mut lib = Library::create(
        &bibtex,
        Some(&format!("{root}/sub/files")),
        None,
        Format::name(),
        Format::key(),
    )
    .unwrap();
    assert!(Path::new(&bibtex).exists());
    std::fs::write(format!("{root}/in.pdf"), "pdf").unwrap();
    let options = InsertOptions {
        rename: Some(Policy::all(Action::Move)),
        ..Default::default()
    };
    let mut resolver = Resolver::new(OnDuplicate::Error);
    let text = "@article{x,\n author = {Doe, Jane},\n year = {2020},\n title = {T},\n file = {:../in.pdf:pdf}\n}\n";
    lib.add_bibtex(
        text,
        Some(&format!("{root}/sub")),
        &[],
        false,
        &options,
        &mut resolver,
    )
    .unwrap();
    let saved = lib.save(true).unwrap();
    assert_eq!(saved.file_ops.len(), 1);
    let op = &saved.file_ops[0];
    assert_eq!(
        (op.name(), op.src.as_str(), op.dst.as_str()),
        (
            "moved",
            format!("{root}/in.pdf").as_str(),
            format!("{root}/sub/files/doe_2020_t.pdf").as_str()
        )
    );
    let digest: String = sha2::Sha256::digest(b"pdf")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(op.checksum.as_deref(), Some(digest.as_str()));
    assert!(Path::new(&format!("{root}/sub/files/doe_2020_t.pdf")).exists());
    assert!(lib.file_ops().is_empty(), "the operations went to the save");
}
