//! The origin records of a library and how an exchange matches the entries of a library and of its pool
//! (`expected/pools.json`, made by `gen_pools.py` with papers-cli): the file of the records of a pool id,
//! the files that `Origin::save` writes for sequences of operations, the pool entry of each library entry,
//! the entries shared with the pool or removed from it, and the library entry of each pool entry.

use std::fs;

use papers_rs::bib::Entry;
use papers_rs::config::{Config, MachineConfig};
use papers_rs::library::Library;
use papers_rs::pools::{Exchange, Origin, origin_file_name};
use serde_json::Value;

fn expected() -> Value {
    let text = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/expected/pools.json"
    ))
    .unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn the_file_of_the_records_of_a_pool() {
    for (id, name) in expected()["names"].as_object().unwrap() {
        if id == "named" {
            // a pool id with where the pool is
            for case in name.as_array().unwrap() {
                let (id, path, name) = (
                    case[0].as_str().unwrap(),
                    case[1].as_str(),
                    case[2].as_str().unwrap(),
                );
                assert_eq!(origin_file_name(id, path), name, "{id:?} {path:?}");
            }
        } else {
            assert_eq!(origin_file_name(id, None), name.as_str().unwrap(), "{id:?}");
        }
    }
}

/// Replace the times of the records, which differ from one run to the next.
fn untimed(text: &str) -> String {
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        match line.find("papers-time = {") {
            Some(i) => out.push_str(&format!("{}papers-time = {{$TIME}},\n", &line[..i])),
            None => out.push_str(line),
        }
    }
    out
}

fn entry(fields: &Value) -> Entry {
    let mut e = Entry::new("article", fields["ID"].as_str().unwrap());
    for (k, v) in fields.as_object().unwrap() {
        if k != "ID" {
            e.set(k, v.as_str().unwrap());
        }
    }
    e
}

#[test]
fn the_records_written_for_a_sequence_of_operations() {
    for case in expected()["origin"].as_array().unwrap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("origin").to_string_lossy().into_owned();
        let mut origin = Origin::load(&path, false);
        for op in case["ops"].as_array().unwrap() {
            let op = op.as_array().unwrap();
            let s = |i: usize| op[i].as_str().unwrap();
            match s(0) {
                "set" => origin.set(s(1), s(2), &entry(&op[3]), op[4].as_str()),
                "set_files" => origin.set_files(s(1), s(2), op[3].as_str()),
                "remove" => origin.remove(s(1), s(2)),
                "rekey" => origin.rekey(s(1), s(2)),
                "pulled" => origin.set_pulled(s(1), Some(op[2].as_i64().unwrap() as i128)),
                "locate" => origin.locate(s(1), s(2)),
                "rename" => origin.rename_pool(s(1), s(2), Some(s(3))),
                "save" => origin.save().unwrap(),
                "reload" => origin = Origin::load(&path, false),
                "write" => {
                    fs::create_dir_all(&path).unwrap();
                    fs::write(format!("{path}/{}", s(1)), s(2)).unwrap();
                }
                other => panic!("{other}"),
            }
        }
        origin.save().unwrap();
        let mut written = std::collections::BTreeMap::new();
        for e in fs::read_dir(&path).unwrap().flatten() {
            let text = fs::read_to_string(e.path()).unwrap();
            written.insert(e.file_name().to_string_lossy().into_owned(), untimed(&text));
        }
        let want: std::collections::BTreeMap<String, String> = case["files"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
            .collect();
        assert_eq!(written, want, "{}", case["ops"]);
    }
}

fn bibtex(entries: &Value) -> String {
    let mut out = String::new();
    for e in entries.as_array().unwrap() {
        out.push_str(&format!(
            "@{}{{{},\n",
            e["type"].as_str().unwrap(),
            e["key"].as_str().unwrap()
        ));
        for f in e["fields"].as_array().unwrap() {
            out.push_str(&format!(
                " {} = {{{}}},\n",
                f[0].as_str().unwrap(),
                f[1].as_str().unwrap()
            ));
        }
        out.push_str("}\n\n");
    }
    out
}

fn some(s: Option<&str>) -> Value {
    s.map_or(Value::Null, |s| Value::String(s.to_string()))
}

#[test]
fn the_pool_entry_of_a_library_entry_and_the_entries_shared() {
    for case in expected()["matching"].as_array().unwrap() {
        let scenario = &case["scenario"];
        let name = scenario["name"].as_str().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let write = |rel: &str, text: &str| {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        write(
            "lib/.papers/config.json",
            "{\"bibtex\": \"papers.bib\", \"filesdir\": \"files\", \"pool\": \"../pool\", \"version\": 3}",
        );
        write(
            "pool/.papers/config.json",
            "{\"bibtex\": \"papers.bib\", \"filesdir\": \"files\", \"pool\": \"none\", \"version\": 3}",
        );
        write("lib/papers.bib", &bibtex(&scenario["library"]));
        write("pool/papers.bib", &bibtex(&scenario["pool"]));
        if !scenario["records"].as_array().unwrap().is_empty() {
            write(
                &format!("lib/.papers/origin/{}", origin_file_name("../pool", None)),
                &format!(
                    "@comment{{papers-pool ../pool}}\n\n{}",
                    bibtex(&scenario["records"])
                ),
            );
        }
        if let Some(distinct) = scenario.get("distinct") {
            write(
                "lib/.papers/distinct.json",
                &serde_json::json!({ "distinct": distinct }).to_string(),
            );
        }
        let config = Config::load(
            &root.join("lib/.papers/config.json").to_string_lossy(),
            MachineConfig::empty(),
            None,
        )
        .unwrap();
        let mut library = Library::open(&config).unwrap();
        let mut exchange = Exchange::quiet(&config, true);
        let pool = exchange.pools.target().unwrap();
        let keys: Vec<String> = library.entries().map(|e| e.key.clone()).collect();
        let mut entries = Vec::new();
        for key in &keys {
            let e = library.get(key).unwrap().clone();
            let (pe, record) = exchange.pool_entry(&library, pool, &e).unwrap();
            let pool_key = pe.map(|uid| exchange.pool_entry_of(pool, uid).unwrap().key);
            entries.push(serde_json::json!({
                "key": key,
                "pool_key": some(pool_key.as_deref()),
                "record": some(record.as_ref().map(|r| r.pool_key.as_str())),
            }));
        }
        let (shared, removed) = exchange.sharing(&library, &keys, pool).unwrap();
        let mut library_entries = serde_json::Map::new();
        let uids = exchange.pools.at_mut(pool).library().unwrap().uids();
        for uid in uids {
            let pe = exchange.pool_entry_of(pool, uid).unwrap();
            let le = exchange.library_entry(&mut library, &pe, pool).unwrap();
            library_entries.insert(pe.key.clone(), some(le.as_deref()));
        }
        let missing: Vec<String> = exchange
            .missing(&mut library, pool)
            .unwrap()
            .into_iter()
            .map(|uid| exchange.pool_entry_of(pool, uid).unwrap().key)
            .collect();
        let got = serde_json::json!({
            "entries": entries,
            "shared": shared,
            "removed": removed,
            "library_entries": library_entries,
            "missing": missing,
        });
        assert_eq!(got, case["expected"], "{name}");
    }
}

/// The pool id of a pool, and what finding it does to the records (`expected/pools.json`, `ids`): the records of
/// the path ids that designate the pool move to its library identifier, the pool is located, and the files are
/// named after the folder of the pool; a pool without identifier keeps the path id of its records.
#[test]
fn the_pool_id_and_the_records_that_move_to_it() {
    for case in expected()["ids"].as_array().unwrap() {
        let scenario = &case["scenario"];
        let name = scenario["name"].as_str().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let write = |rel: &str, text: &str| {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        write(
            "lib/.papers/config.json",
            "{\"bibtex\": \"papers.bib\", \"filesdir\": \"files\", \"pool\": \"../pool\", \"version\": 3}",
        );
        let mut pool_config = serde_json::json!({"bibtex": "papers.bib", "filesdir": "files", "pool": "none", "version": 3});
        if let Some(id) = scenario["pool_id"].as_str() {
            pool_config["id"] = id.into();
        }
        write("pool/.papers/config.json", &pool_config.to_string());
        write("lib/papers.bib", "");
        write("pool/papers.bib", "");
        fs::create_dir_all(root.join("lib/.papers/origin")).unwrap();
        for (file, text) in scenario["files"].as_object().unwrap() {
            write(
                &format!("lib/.papers/origin/{file}"),
                text.as_str().unwrap(),
            );
        }
        let config = Config::load(
            &root.join("lib/.papers/config.json").to_string_lossy(),
            MachineConfig::empty(),
            None,
        )
        .unwrap();
        let mut exchange = Exchange::quiet(&config, false);
        let pool = exchange.target().unwrap();
        let id = exchange.pools.at(pool).id.clone();
        let root_text = root.to_string_lossy().into_owned();
        let fix = |text: &str| text.replace(&root_text, "$ROOT");
        let mut records = serde_json::Map::new();
        let mut pulls = serde_json::Map::new();
        let mut paths = serde_json::Map::new();
        let want_empty: Vec<&str> = case["expected"]["records"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, v)| v.as_array().is_some_and(|a| a.is_empty()))
            .map(|(k, _)| k.as_str())
            .collect();
        let mut ids = exchange.origin.pool_ids();
        ids.insert(id.clone());
        for pool_id in ids {
            let keys: Vec<String> = exchange
                .origin
                .records(Some(&pool_id))
                .into_iter()
                .map(|r| r.key)
                .collect();
            let mut keys = keys;
            keys.sort();
            // papers has a pool without records only if its file was read: compare the pools with records
            if !keys.is_empty() || want_empty.contains(&pool_id.as_str()) {
                records.insert(pool_id.clone(), serde_json::json!(keys));
            }
            if let Some(n) = exchange.origin.pulled(&pool_id) {
                pulls.insert(pool_id.clone(), serde_json::json!(n as i64));
            }
            if let Some(path) = exchange.origin.path_of(&pool_id) {
                paths.insert(pool_id.clone(), fix(path).into());
            }
        }
        let file =
            fix(&exchange.origin.file_of(&id).unwrap()).replace("$ROOT/lib/.papers/origin/", "");
        exchange.origin.save().unwrap();
        let mut saved = serde_json::Map::new();
        for e in fs::read_dir(root.join("lib/.papers/origin"))
            .unwrap()
            .flatten()
        {
            let text = fs::read_to_string(e.path()).unwrap();
            saved.insert(
                e.file_name().to_string_lossy().into_owned(),
                fix(&untimed(&text)).into(),
            );
        }
        let got = serde_json::json!({
            "id": id, "records": records, "pulls": pulls, "paths": paths, "file": file, "saved": saved,
        });
        let mut want = case["expected"].clone();
        // the times of the records, which differ from one run to the next
        if let Some(saved) = want["saved"].as_object_mut() {
            for text in saved.values_mut() {
                *text = fix(&untimed(text.as_str().unwrap())).into();
            }
        }
        assert_eq!(got, want, "{name}");
    }
}
