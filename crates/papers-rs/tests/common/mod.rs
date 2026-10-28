//! What the test files share: the recorded files of `expected/` and the blocks as the generators dump them.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use papers_rs::bib::{Bib, Block, UnreadableKind};
use serde_json::{Value, json};

pub fn dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name)
}

pub fn json_file(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The blocks and the unreadable list as the generator dumps them.
pub fn dump(bib: &Bib) -> Value {
    let blocks: Vec<Value> = bib
        .blocks
        .iter()
        .map(|b| match b {
            Block::Entry(e) => json!({
                "kind": "entry", "type": e.entry_type, "key": e.key,
                "fields": e.fields.iter().map(|f| json!([f.key, f.value, f.verbatim])).collect::<Vec<_>>(),
            }),
            Block::String(s) => json!({"kind": "string", "key": s.key, "value": s.value, "verbatim": s.verbatim}),
            Block::Preamble(v) => json!({"kind": "preamble", "value": v}),
            Block::Comment(v) => json!({"kind": "comment", "value": v}),
            Block::Text(v) => json!({"kind": "text", "value": v}),
            Block::Unreadable(u) => {
                let mut d = json!({"kind": "unreadable", "raw": u.raw, "line": u.line});
                match &u.kind {
                    UnreadableKind::DuplicateFields(_) => d["class"] = json!("DuplicateFieldKeyBlock"),
                    UnreadableKind::DuplicateKey(k) => {
                        d["class"] = json!("DuplicateBlockKeyBlock");
                        d["key"] = json!(k);
                    }
                    UnreadableKind::Syntax => d["class"] = json!("ParsingFailedBlock"),
                    UnreadableKind::Decoding(_) => d["class"] = json!("MiddlewareErrorBlock"),
                }
                d
            }
        })
        .collect();
    let unreadable: Vec<Value> = bib
        .unreadable()
        .iter()
        .map(|(line, why)| json!([line, why]))
        .collect();
    json!({"blocks": blocks, "unreadable": unreadable})
}
