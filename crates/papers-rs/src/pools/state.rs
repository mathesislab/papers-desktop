//! How a library stands with its pool, for `papers status` (`papers.pools.status_line`,
//! `exchange_state`; spec `exchange.md` section 14): the line of the pool and the hint for the last line.
//!
//! `status` writes nothing in the library (no origin file, no bibtex, no history step): the records stay
//! under their path id, and it repairs the broken links to pool files in memory, as a pull finds them.
//!
//! To avoid comparing every entry, the pieces of the library's bibtex, of the pool's bibtex and of
//! the origin file, as `status` last found them in sync, are kept in `exchanges.json` in the history
//! folder of the library (section 14.4); only the entries in the pieces that changed since are compared.

use std::collections::{BTreeMap, BTreeSet};

use crate::bib::Entry;
use crate::config::json::{loads, quote};
use crate::config::paths::{exists, isfile, join};
use crate::config::{Config, write_atomic};
use crate::error::Result;
use crate::history::History;
use crate::history::store::piece_digests;
use crate::merge::Merge;

use super::exchange::{Exchange, exchange_copy};
use super::pool::PoolIx;

/// The colours of the note of a pool line (empty strings without colours).
#[derive(Debug, Clone, Copy, Default)]
pub struct Palette {
    pub warning: &'static str,
    pub end: &'static str,
}

/// The hint of `papers status` for each state of a library and its pool.
fn hint(state: &str) -> Option<&'static str> {
    Some(match state {
        "pool changed" => "`papers pull --dry-run` shows the changes, `papers pull` takes them",
        "library changed" => "`papers push --dry-run` shows the changes, `papers push` sends them",
        "both changed" => "`papers sync --dry-run` shows the changes, `papers sync` exchanges them",
        "not pulled from yet" => "`papers pull` takes the entries shared with the pool",
        "not pulled in full yet" => "`papers pull` takes the changes of the pool",
        _ => return None,
    })
}

/// The fields of an entry that an exchange compares (not the file links, nor the records of papers).
fn fields(e: &Entry) -> BTreeMap<String, String> {
    e.fields
        .iter()
        .filter(|f| {
            let k = f.key.to_lowercase();
            k != "file" && k != "ids" && !k.starts_with("papers-")
        })
        .map(|f| (f.key.to_lowercase(), f.value.clone()))
        .collect()
}

/// The lower-case keys of the entries of bibtex texts (not `@comment`, `@string`, `@preamble`): `@type{key,` at the
/// start of a line.
fn entry_keys<'a>(texts: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for text in texts {
        for line in text.split('\n') {
            let Some(rest) = line.strip_prefix('@') else {
                continue;
            };
            let rest = rest.trim_start();
            let kind: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if kind.is_empty() {
                continue;
            }
            let rest = rest[kind.len()..].trim_start();
            let Some(rest) = rest.strip_prefix(['{', '(']) else {
                continue;
            };
            let rest = rest.trim_start();
            let key: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != ',')
                .collect();
            let after = rest[key.len()..].trim_start();
            if key.is_empty() || !after.starts_with(',') {
                continue;
            }
            if matches!(
                kind.to_lowercase().as_str(),
                "comment" | "string" | "preamble"
            ) {
                continue;
            }
            keys.insert(key.to_lowercase());
        }
    }
    keys
}

fn read_text(path: Option<&str>) -> String {
    path.filter(|p| exists(p))
        .and_then(|p| crate::bib::read_file(std::path::Path::new(p)).ok())
        .unwrap_or_default()
}

/// The pairs of a JSON object in the order they are in the text (as Python's dictionaries keep them): the
/// key, and the text of the value as a value of `loads`. Empty if the text is not an object.
fn ordered_pairs(text: &str) -> Vec<(String, serde_json::Value)> {
    let chars: Vec<char> = text.chars().collect();
    if loads(text).is_err() {
        return Vec::new();
    }
    let mut at = 0;
    let skip = |at: &mut usize| {
        while *at < chars.len() && chars[*at].is_whitespace() {
            *at += 1;
        }
    };
    // the end of the JSON string that starts at `at` (the closing quote)
    let string_end = |at: usize| {
        let mut i = at + 1;
        while i < chars.len() && chars[i] != '"' {
            i += if chars[i] == '\\' { 2 } else { 1 };
        }
        i
    };
    skip(&mut at);
    if chars.get(at) != Some(&'{') {
        return Vec::new();
    }
    at += 1;
    let mut pairs: Vec<(String, serde_json::Value)> = Vec::new();
    loop {
        skip(&mut at);
        if chars.get(at) != Some(&'"') {
            break; // the closing brace
        }
        let end = string_end(at);
        let key_text: String = chars[at..=end].iter().collect();
        let Ok(serde_json::Value::String(key)) = loads(&key_text) else {
            return Vec::new();
        };
        at = end + 1;
        skip(&mut at);
        at += 1; // the colon
        skip(&mut at);
        let start = at;
        let mut depth = 0;
        while at < chars.len() {
            match chars[at] {
                '"' => at = string_end(at),
                '{' | '[' => depth += 1,
                '}' | ']' if depth > 0 => depth -= 1,
                ',' | '}' if depth == 0 => break,
                _ => {}
            }
            at += 1;
        }
        let raw: String = chars[start..at].iter().collect();
        let Ok(value) = loads(&raw) else {
            return Vec::new();
        };
        match pairs.iter_mut().find(|(k, _)| *k == key) {
            Some(pair) => pair.1 = value,
            None => pairs.push((key, value)),
        }
        skip(&mut at);
        if chars.get(at) == Some(&',') {
            at += 1;
        }
    }
    pairs
}

/// A key's value set in a list of pairs: in place if the key is there, else at the end.
fn set_in_order(pairs: &mut Vec<(String, String)>, key: String, value: String) {
    match pairs.iter_mut().find(|(k, _)| *k == key) {
        Some(pair) => pair.1 = value,
        None => pairs.push((key, value)),
    }
}

/// The pieces of a pool as they are written: `library`, `pool`, `origin` in this order.
fn dump_pieces(value: &serde_json::Value) -> String {
    if let serde_json::Value::Object(map) = value
        && map.len() == 3
        && let (Some(library), Some(pool), Some(origin)) =
            (map.get("library"), map.get("pool"), map.get("origin"))
    {
        let dump = crate::config::json::dumps_compact;
        return format!(
            "{{\"library\": {}, \"pool\": {}, \"origin\": {}}}",
            dump(library),
            dump(pool),
            dump(origin)
        );
    }
    crate::config::json::dumps_compact(value)
}

/// The pieces of the library, its pool and its origin file for that pool, when status last found them
/// in sync: a piece that did not change on any side holds entries still in sync.
struct Pieces {
    path: Option<String>,
    /// side -> digest -> piece
    now: BTreeMap<&'static str, BTreeMap<String, String>>,
    then: Option<serde_json::Map<String, serde_json::Value>>,
}

fn pieces_of(text: &str) -> BTreeMap<String, String> {
    piece_digests(text)
        .into_iter()
        .map(|(d, p)| (d, p.to_string()))
        .collect()
}

impl Pieces {
    fn new(
        config: &Config,
        pool_bibtex: Option<&str>,
        origin_path: Option<&str>,
        id: &str,
    ) -> Pieces {
        let mut copy = config.clone();
        let path = match History::open(&mut copy, false, false) {
            Ok(Some(history)) => Some(join(&history.project.dir, "exchanges.json")),
            _ => None,
        };
        let mut now = BTreeMap::new();
        now.insert("library", pieces_of(&read_text(config.bibtex.as_deref())));
        now.insert("pool", pieces_of(&read_text(pool_bibtex)));
        now.insert("origin", pieces_of(&read_text(origin_path)));
        let then = path
            .as_deref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|t| loads(&t).ok())
            .and_then(|v| match v {
                serde_json::Value::Object(map) => map.get(id).cloned(),
                _ => None,
            })
            .and_then(|v| match v {
                serde_json::Value::Object(map) => Some(map),
                _ => None,
            });
        Pieces { path, now, then }
    }

    /// `(keys of the library, keys of the pool)` of the entries in pieces that changed since, or none (not
    /// known: compare everything).
    fn changed(&self) -> Option<(BTreeSet<String>, BTreeSet<String>)> {
        let then = self.then.as_ref()?;
        // pieces gone from the pool (an entry removed): their keys are not known
        let pool_then: BTreeSet<&str> = then
            .get("pool")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default();
        if pool_then.iter().any(|d| !self.now["pool"].contains_key(*d)) {
            return None;
        }
        let new = |side: &str| -> Vec<&str> {
            let known: BTreeSet<&str> = then
                .get(side)
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                .unwrap_or_default();
            self.now[side]
                .iter()
                .filter(|(d, _)| !known.contains(d.as_str()))
                .map(|(_, p)| p.as_str())
                .collect()
        };
        let library = entry_keys(new("library").into_iter().chain(new("origin")));
        let pool = entry_keys(new("pool"));
        Some((library, pool))
    }

    /// After status found the library and the pool in sync (`id`: the id of the pool now).
    fn save(&mut self, origin_path: Option<&str>, id: &str) -> Result<()> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        self.now
            .insert("origin", pieces_of(&read_text(origin_path)));
        // the pools already there (in the order of the file), then this one
        let mut data: Vec<(String, String)> = Vec::new();
        if let Some(text) = std::fs::read(&path)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
        {
            for (pool, value) in ordered_pairs(&text) {
                set_in_order(&mut data, pool, dump_pieces(&value));
            }
        }
        let side = |name: &str| -> String {
            let digests: Vec<String> = self.now[name].keys().map(|d| quote(d)).collect();
            format!("[{}]", digests.join(", "))
        };
        set_in_order(
            &mut data,
            id.to_string(),
            format!(
                "{{\"library\": {}, \"pool\": {}, \"origin\": {}}}",
                side("library"),
                side("pool"),
                side("origin")
            ),
        );
        let text = format!(
            "{{{}}}",
            data.iter()
                .map(|(id, v)| format!("{}: {v}", quote(id)))
                .collect::<Vec<_>>()
                .join(", ")
        );
        write_atomic(&path, &text)
    }
}

/// The pool of a library in `papers status`: its setting, and how it stands with the library (see
/// [`exchange_state`]); and the hint for the last line of `papers status`, if any.
pub fn status_line(config: &Config, palette: Palette) -> (String, Option<String>) {
    let Some(path) = config.pool_path() else {
        return ("none".to_string(), None);
    };
    let text = config.pool_label().unwrap_or_default();
    let mut hint_line = None;
    let note = if config.is_pool_library() {
        "this library: it has no pool".to_string()
    } else if !exists(&path) {
        format!("{}missing{}", palette.warning, palette.end)
    } else {
        match exchange_state(config, &path) {
            Ok((note, hint)) => {
                hint_line = hint;
                note
            }
            Err(_) => format!("{}not a library{}", palette.warning, palette.end),
        }
    };
    (format!("{text} ({note})"), hint_line)
}

/// How a library stands with its pool: `in sync`, `pool changed`, `library changed` or `both changed` (an entry
/// exchanged with the pool differs from its origin copy on that side, as a pull or a push would find it), or not
/// exchanged yet; and the hint of the state. A pool whose file changed without any difference for the library (the
/// same change on both sides, or entries of other libraries) is in sync: its modification time is recorded, as by a
/// pull. An error if the pool is not a library.
pub fn exchange_state(
    config: &Config,
    path: &str,
) -> std::result::Result<(String, Option<String>), crate::pools::PoolError> {
    let mut exchange = Exchange::for_status(config);
    let pool = exchange.pools.resolve(path, None)?;
    compare(config, &mut exchange, pool)
        .map_err(|e| crate::pools::PoolError(e.message().to_string()))
}

fn compare(
    config: &Config,
    exchange: &mut Exchange,
    pool: PoolIx,
) -> Result<(String, Option<String>)> {
    let first = exchange.pool_state(pool);
    if first == "not pulled from yet" || first == "not pulled in full yet" {
        return Ok((first.to_string(), hint(first).map(str::to_string)));
    }
    let in_sync = || Ok(("in sync".to_string(), None));
    if !config.bibtex.as_deref().is_some_and(isfile) {
        return in_sync();
    }
    let id = exchange.pools.at(pool).id.clone();
    let mtime = exchange.pools.at(pool).bibtex_mtime();
    let moved = exchange.origin.pulled(&id) != mtime;
    let origin_path = exchange.origin.file_of(&id);
    let pool_bibtex = exchange.pools.at(pool).config.bibtex.clone();
    let mut pieces = Pieces::new(config, pool_bibtex.as_deref(), origin_path.as_deref(), &id);
    let changed = pieces.changed();
    if let Some((a, b)) = &changed
        && a.is_empty()
        && b.is_empty()
    {
        // no piece changed on any side since status found them in sync
        return in_sync();
    }
    // the library as pull finds it: the broken links to pool files repaired (in memory)
    let library = crate::command::pool::load_library(config)?;
    // the records of a path id move to the identifier of the pool, in memory only: nothing is saved
    exchange.begin_moves();
    let id = exchange.pools.at(pool).id.clone();
    let pool_relative = exchange.pools.at_mut(pool).library()?.relative_to.clone();
    let keys: Vec<String> = library.entries().map(|e| e.key.clone()).collect();
    let (mut pool_changed, mut library_changed) = (false, false);
    for key in keys {
        let Some(e) = library.get(&key).cloned() else {
            continue;
        };
        let Some(record) = exchange.origin.get(&e, Some(&id)) else {
            continue;
        };
        if let Some((library_keys, pool_keys)) = &changed
            && !library_keys.contains(&e.key.to_lowercase())
            && !pool_keys.contains(&record.pool_key.to_lowercase())
        {
            continue; // in pieces that changed on no side since they were in sync
        }
        if !moved
            && fields(&e) == fields(&record.entry)
            && e.entry_type.to_lowercase() == record.entry.entry_type.to_lowercase()
        {
            continue; // neither side changed (the pool file did not change)
        }
        let (pe, record) = exchange.pool_entry(&library, pool, &e)?;
        let Some(pe) = pe else {
            pool_changed = true; // removed from the pool
            continue;
        };
        let record = record.expect("a record");
        let pool_entry = exchange.pool_entry_of(pool, pe)?;
        let incoming = exchange_copy(&pool_entry, &pool_relative, &library.relative_to, None);
        let pulled = Merge::exchange(
            e.clone(),
            incoming,
            Some(&library.relative_to),
            Some(&record.entry),
        );
        if !pulled.conflicts().is_empty() {
            pool_changed = true;
            library_changed = true;
        } else if !pulled.additions().is_empty()
            || !pulled.new_files.is_empty()
            || !exchange
                .pool_removed(&library, &e, pe, Some(&record), pool)?
                .is_empty()
        {
            pool_changed = true;
        }
        let outgoing = exchange.outgoing(&library, &e, pool, pe, Some(&record))?;
        let mut base = record.entry.clone();
        base.key = pool_entry.key.clone();
        let pushed = Merge::exchange(
            pool_entry.clone(),
            outgoing,
            Some(&pool_relative),
            Some(&base),
        );
        if !pushed.conflicts().is_empty()
            || !pushed.additions().is_empty()
            || !pushed.new_files.is_empty()
        {
            library_changed = true;
        }
        if pool_changed && library_changed {
            break;
        }
    }
    if pool_changed || library_changed {
        let state = match (pool_changed, library_changed) {
            (true, true) => "both changed",
            (true, false) => "pool changed",
            _ => "library changed",
        };
        return Ok((state.to_string(), hint(state).map(str::to_string)));
    }
    pieces.save(origin_path.as_deref(), &id)?;
    in_sync()
}
