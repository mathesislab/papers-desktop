//! A project: the history of one library in a store (spec/history.md,
//! sections 2.5 to 2.7 and 3): its states, a tree, and the heads of its
//! places.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;

use super::ojson::J;
use super::ops::FileOp;
use super::store::{Ref, Store, now, write_bytes};
use crate::config::paths::{dirname, exists, join};
use crate::error::{Error, Result};

/// The text of a state: the pieces of a bibtex (or of a kept file), and the
/// sha256 of the whole text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub pieces: Vec<String>,
    pub sha: String,
}

/// A link from a state of a library to the state that the same command made
/// in another library (its pool): `pools` of the state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolLink {
    /// The number of the state in the project of the other library.
    pub state: u64,
    /// The configuration file of the other library.
    pub config: String,
}

/// What makes a new state.
#[derive(Debug, Clone)]
pub struct NewState {
    pub parent: Option<u64>,
    pub time: f64,
    /// `start`, `outside` or `command`.
    pub kind: String,
    pub message: String,
    /// The operations as stored (paths relative to the bibtex directory when inside it).
    pub ops: Vec<FileOp>,
    pub deleted: Vec<String>,
    pub bibtex: Text,
    /// The kept `.papers/` files: name (relative to the bibtex directory) and pieces.
    pub files: Vec<(String, Vec<String>)>,
}

/// A state, as the JSON object it is (the keys in order, the keys that this
/// implementation does not know kept).
#[derive(Debug, Clone, PartialEq)]
pub struct State(pub J);

impl State {
    pub fn new(parts: NewState) -> State {
        let files = J::Obj(
            parts
                .files
                .iter()
                .map(|(name, pieces)| (name.clone(), J::strs(pieces)))
                .collect(),
        );
        State(J::Obj(vec![
            (
                "parent".into(),
                parts.parent.map_or(J::Null, |p| J::Int(p as i64)),
            ),
            ("time".into(), J::Float(parts.time)),
            ("kind".into(), J::str(parts.kind)),
            ("message".into(), J::str(parts.message)),
            (
                "ops".into(),
                J::Arr(parts.ops.iter().map(FileOp::to_j).collect()),
            ),
            ("deleted".into(), J::strs(&parts.deleted)),
            (
                "bibtex".into(),
                J::Obj(vec![
                    ("pieces".into(), J::strs(&parts.bibtex.pieces)),
                    ("sha".into(), J::str(&parts.bibtex.sha)),
                ]),
            ),
            ("files".into(), files),
        ]))
    }

    pub fn parent(&self) -> Option<u64> {
        self.0.get("parent").and_then(J::as_u64)
    }

    pub fn time(&self) -> f64 {
        self.0.get("time").and_then(J::as_f64).unwrap_or(0.0)
    }

    pub fn kind(&self) -> &str {
        self.0.get("kind").and_then(J::as_str).unwrap_or("")
    }

    pub fn message(&self) -> &str {
        self.0.get("message").and_then(J::as_str).unwrap_or("")
    }

    /// The file operations, oldest first (paths as stored).
    pub fn ops(&self) -> Vec<FileOp> {
        self.0
            .get("ops")
            .and_then(J::as_array)
            .map(|a| a.iter().filter_map(FileOp::from_j).collect())
            .unwrap_or_default()
    }

    /// The checksums of the file objects the state keeps.
    pub fn deleted(&self) -> Vec<String> {
        self.0.get("deleted").map(J::strings).unwrap_or_default()
    }

    pub fn bibtex_pieces(&self) -> Vec<String> {
        self.0
            .get("bibtex")
            .and_then(|b| b.get("pieces"))
            .map(J::strings)
            .unwrap_or_default()
    }

    pub fn bibtex_sha(&self) -> Option<&str> {
        self.0
            .get("bibtex")
            .and_then(|b| b.get("sha"))
            .and_then(J::as_str)
    }

    /// The bibtex of the state, if it has a checksum.
    pub fn bibtex(&self) -> Option<Text> {
        Some(Text {
            pieces: self.bibtex_pieces(),
            sha: self.bibtex_sha()?.to_string(),
        })
    }

    /// The kept `.papers/` files (name, pieces), in order; none when the
    /// state has no `files`.
    pub fn files(&self) -> Option<Vec<(String, Vec<String>)>> {
        match self.0.get("files")? {
            J::Obj(items) => Some(
                items
                    .iter()
                    .map(|(k, v)| (k.clone(), v.strings()))
                    .collect(),
            ),
            _ => None,
        }
    }

    pub fn file(&self, name: &str) -> Option<Vec<String>> {
        self.0.get("files")?.get(name).map(J::strings)
    }

    /// The links with the steps of other libraries made by the same command.
    pub fn pools(&self) -> Vec<(String, PoolLink)> {
        match self.0.get("pools") {
            Some(J::Obj(items)) => items
                .iter()
                .filter_map(|(id, link)| {
                    Some((
                        id.clone(),
                        PoolLink {
                            state: link.get("state")?.as_u64()?,
                            config: link
                                .get("config")
                                .and_then(J::as_str)
                                .unwrap_or("")
                                .to_string(),
                        },
                    ))
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// In the state of a pool: the directory of the bibtex of the library of the command.
    pub fn from(&self) -> Option<&str> {
        self.0
            .get("from")
            .and_then(J::as_str)
            .filter(|s| !s.is_empty())
    }

    /// The states that a revert takes back.
    pub fn reverts(&self) -> Vec<u64> {
        self.0.get("reverts").map(J::numbers).unwrap_or_default()
    }

    /// What the state refers to: the pieces of its bibtex and of its kept
    /// files, and the file objects of `deleted`.
    pub fn refs(&self) -> Vec<Ref> {
        let mut refs: Vec<Ref> = self.bibtex_pieces().into_iter().map(Ref::Piece).collect();
        if let Some(J::Obj(items)) = self.0.get("files") {
            for (_, pieces) in items {
                refs.extend(pieces.strings().into_iter().map(Ref::Piece));
            }
        }
        refs.extend(self.deleted().into_iter().map(Ref::File));
        refs
    }

    /// Add the links with other libraries (`pools`).
    pub fn set_pools(&mut self, pools: &[(String, PoolLink)]) {
        let j = J::Obj(
            pools
                .iter()
                .map(|(id, l)| {
                    (
                        id.clone(),
                        J::Obj(vec![
                            ("state".into(), J::Int(l.state as i64)),
                            ("config".into(), J::str(&l.config)),
                        ]),
                    )
                })
                .collect(),
        );
        self.0.set("pools", j);
    }

    pub fn set_reverts(&mut self, reverts: &[u64]) {
        self.0.set("reverts", J::nums(reverts));
    }

    pub fn set_message(&mut self, message: &str) {
        self.0.set("message", J::str(message));
    }

    pub fn set_from(&mut self, from: &str) {
        self.0.set("from", J::str(from));
    }

    /// Replace the operations (the checksums of `deleted` follow).
    pub fn set_ops(&mut self, ops: &[FileOp], deleted: &[String]) {
        self.0
            .set("ops", J::Arr(ops.iter().map(FileOp::to_j).collect()));
        self.0.set("deleted", J::strs(deleted));
    }

    pub fn set_deleted(&mut self, deleted: &[String]) {
        self.0.set("deleted", J::strs(deleted));
    }
}

/// The head of a place: its current state and its redo list.
#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    pub state: u64,
    /// The states that redo goes to, the next one first.
    pub redo: Vec<u64>,
    pub time: f64,
}

/// A place of `heads.json`, as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaceHead {
    pub place: String,
    pub state: Option<u64>,
    pub redo: Vec<u64>,
    pub time: f64,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub dir: String,
    states_dir: String,
    states: BTreeMap<u64, State>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn read_json(path: &str) -> Option<J> {
    let bytes = fs::read(path).ok()?;
    J::parse(std::str::from_utf8(&bytes).ok()?)
}

impl Project {
    /// The project `id` of a store, with the states that are there now.
    pub fn open(store: &Store, id: &str) -> Result<Project> {
        if !valid_id(id) {
            return Err(Error::other(format!(
                "invalid identifier of a library: '{id}'"
            )));
        }
        let dir = join(&store.projects(), id);
        let states_dir = join(&dir, "states");
        let mut project = Project {
            id: id.to_string(),
            dir,
            states_dir,
            states: BTreeMap::new(),
        };
        project.reload();
        Ok(project)
    }

    /// Read the states again (another command may have added some).
    pub fn reload(&mut self) {
        self.states.clear();
        let Ok(names) = fs::read_dir(&self.states_dir) else {
            return;
        };
        for name in names.flatten() {
            let name = name.file_name().to_string_lossy().into_owned();
            let Some(digits) = name.strip_suffix(".json") else {
                continue;
            };
            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let Ok(n) = digits.parse::<u64>() else {
                continue;
            };
            if let Some(j) = read_json(&join(&self.states_dir, &name))
                && j.is_object()
            {
                self.states.insert(n, State(j));
            }
        }
    }

    pub fn exists(&self) -> bool {
        crate::config::paths::isdir(&self.dir)
    }

    // states
    // ------

    pub fn states(&self) -> &BTreeMap<u64, State> {
        &self.states
    }

    pub fn state(&self, n: u64) -> Option<&State> {
        self.states.get(&n)
    }

    fn path(&self, n: u64) -> String {
        join(&self.states_dir, &format!("{n:06}.json"))
    }

    /// Write a new state, under the number after the largest one (created
    /// exclusively: another command may add one at the same time); its number.
    pub fn add(&mut self, state: State) -> Result<u64> {
        fs::create_dir_all(&self.states_dir)
            .map_err(|e| Error::other(format!("{}: {e}", self.states_dir)))?;
        let mut n = self.states.keys().next_back().copied().unwrap_or(0) + 1;
        loop {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.path(n))
            {
                Ok(mut file) => {
                    file.write_all(state.0.dumps().as_bytes())
                        .map_err(|e| Error::other(format!("{}: {e}", self.path(n))))?;
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
                Err(e) => return Err(Error::other(format!("{}: {e}", self.path(n)))),
            }
        }
        self.states.insert(n, state);
        Ok(n)
    }

    /// Rewrite the state `n` (e.g. the file operations done after its save).
    pub fn update(&mut self, n: u64, state: State) -> Result<()> {
        write_bytes(&self.path(n), state.0.dumps().as_bytes())?;
        self.states.insert(n, state);
        Ok(())
    }

    pub fn drop_state(&mut self, n: u64) {
        let _ = fs::remove_file(self.path(n));
        self.states.remove(&n);
    }

    /// The states from `n` back to the oldest one kept: `n`, its parent, …
    pub fn line(&self, n: u64) -> Vec<u64> {
        let mut line = Vec::new();
        let mut at = Some(n);
        while let Some(m) = at {
            let Some(state) = self.states.get(&m) else {
                break;
            };
            if line.contains(&m) {
                break;
            }
            line.push(m);
            at = state.parent();
        }
        line
    }

    /// From the state `a` to the state `b`: the states left going back (`a`
    /// first, down to before the common ancestor) and the states entered
    /// going forward (`b` last).
    pub fn path_between(&self, a: u64, b: u64) -> (Vec<u64>, Vec<u64>) {
        let up = self.line(a);
        let down = self.line(b);
        let common = up.iter().find(|n| down.contains(n)).copied();
        let back = match common {
            Some(c) => up[..up.iter().position(|n| *n == c).unwrap_or(up.len())].to_vec(),
            None => up,
        };
        let mut forward = match common {
            Some(c) => down[..down.iter().position(|n| *n == c).unwrap_or(down.len())].to_vec(),
            None => down,
        };
        forward.reverse();
        (back, forward)
    }

    // places
    // ------

    fn heads_file(&self) -> String {
        join(&self.dir, "heads.json")
    }

    fn heads_json(&self) -> Vec<(String, J)> {
        match read_json(&self.heads_file()) {
            Some(J::Obj(items)) => items,
            _ => Vec::new(),
        }
    }

    fn write_heads(&self, heads: Vec<(String, J)>) -> Result<()> {
        write_bytes(&self.heads_file(), J::Obj(heads).dumps().as_bytes())
    }

    /// The places of the project, as stored.
    pub fn heads(&self) -> Vec<PlaceHead> {
        self.heads_json()
            .into_iter()
            .map(|(place, h)| PlaceHead {
                place,
                state: h.get("state").and_then(J::as_u64),
                redo: h.get("redo").map(J::numbers).unwrap_or_default(),
                time: h.get("time").and_then(J::as_f64).unwrap_or(0.0),
            })
            .collect()
    }

    /// The head of a place, if its state exists; the redo states that no
    /// longer exist are left out.
    pub fn head(&self, place: &str) -> Option<Head> {
        let h = self.heads().into_iter().find(|h| h.place == place)?;
        let state = h.state.filter(|n| self.states.contains_key(n))?;
        Some(Head {
            state,
            redo: h
                .redo
                .into_iter()
                .filter(|n| self.states.contains_key(n))
                .collect(),
            time: h.time,
        })
    }

    pub fn set_head(&self, place: &str, n: u64, redo: &[u64]) -> Result<()> {
        let mut heads = self.heads_json();
        let head = J::Obj(vec![
            ("state".into(), J::Int(n as i64)),
            ("redo".into(), J::nums(redo)),
            ("time".into(), J::Float(now())),
        ]);
        match heads.iter_mut().find(|(p, _)| p == place) {
            Some((_, h)) => *h = head,
            None => heads.push((place.to_string(), head)),
        }
        self.write_heads(heads)
    }

    /// Forget a place; the number of places left.
    pub fn remove_head(&self, place: &str) -> Result<usize> {
        let mut heads = self.heads_json();
        heads.retain(|(p, _)| p != place);
        let left = heads.len();
        self.write_heads(heads)?;
        Ok(left)
    }

    /// Forget the places that no longer exist (a library moved or deleted).
    pub fn forget_missing_places(&self) -> Result<Vec<String>> {
        let mut heads = self.heads_json();
        let gone: Vec<String> = heads
            .iter()
            .filter(|(p, _)| !exists(p))
            .map(|(p, _)| p.clone())
            .collect();
        if !gone.is_empty() {
            heads.retain(|(p, _)| !gone.contains(p));
            self.write_heads(heads)?;
        }
        Ok(gone)
    }

    /// The state a new place starts from: the most recent one whose bibtex
    /// has this checksum, or else the most recent one (none if the project
    /// has no state).
    pub fn start_state(&self, sha: &str) -> Option<u64> {
        let same: Vec<u64> = self
            .states
            .iter()
            .filter(|(_, s)| s.bibtex_sha() == Some(sha))
            .map(|(n, _)| *n)
            .collect();
        let pool: Vec<u64> = if same.is_empty() {
            self.states.keys().copied().collect()
        } else {
            same
        };
        pool.into_iter().max_by(|a, b| {
            self.states[a]
                .time()
                .total_cmp(&self.states[b].time())
                .then(a.cmp(b))
        })
    }

    /// Delete the history of the project (the pieces and files go at the
    /// next prune).
    pub fn remove(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
        self.states.clear();
    }

    /// The directory of the states of the project.
    pub fn states_dir(&self) -> &str {
        &self.states_dir
    }

    pub fn parent_dir(&self) -> String {
        dirname(&self.dir)
    }

    /// The checksums of the file objects that the states of the project keep.
    pub fn deleted_refs(&self) -> BTreeSet<String> {
        self.states.values().flat_map(|s| s.deleted()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::store::sha256;

    fn state(parent: Option<u64>, time: f64, sha: &str) -> State {
        State::new(NewState {
            parent,
            time,
            kind: "command".into(),
            message: "papers x".into(),
            ops: vec![],
            deleted: vec![],
            bibtex: Text {
                pieces: vec![],
                sha: sha.into(),
            },
            files: vec![],
        })
    }

    #[test]
    fn states_lines_and_paths() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_string_lossy().into_owned());
        let mut p = store.project("abc").unwrap();
        assert!(Store::project(&store, "a/b").is_err());
        let a = p.add(state(None, 1.0, "a")).unwrap();
        let b = p.add(state(Some(a), 2.0, "b")).unwrap();
        let c = p.add(state(Some(b), 3.0, "c")).unwrap();
        let d = p.add(state(Some(b), 4.0, "d")).unwrap();
        assert_eq!((a, b, c, d), (1, 2, 3, 4));
        assert!(std::path::Path::new(&format!("{}/states/000001.json", p.dir)).exists());
        assert_eq!(p.line(c), vec![3, 2, 1]);
        assert_eq!(p.path_between(c, d), (vec![3], vec![4]));
        assert_eq!(p.path_between(c, a), (vec![3, 2], vec![]));
        assert_eq!(p.path_between(a, c), (vec![], vec![2, 3]));
        assert_eq!(p.start_state("b"), Some(2));
        assert_eq!(p.start_state("zzz"), Some(4));
        // a state added by another command is not overwritten
        let mut q = store.project("abc").unwrap();
        let e = p.add(state(Some(a), 5.0, "e")).unwrap();
        assert_eq!(e, 5);
        let mut stale = Project::open(&store, "abc").unwrap();
        stale.states.remove(&5);
        assert_eq!(stale.add(state(None, 6.0, "f")).unwrap(), 6);
        q.reload();
        assert_eq!(q.states().len(), 6);
        // heads: the order of the places stays, redo lists skip the states that are gone
        p.set_head("/x/a.bib", 3, &[4, 9]).unwrap();
        p.set_head("/x/b.bib", 1, &[]).unwrap();
        p.set_head("/x/a.bib", 2, &[3]).unwrap();
        let heads = p.heads();
        assert_eq!(
            heads.iter().map(|h| h.place.as_str()).collect::<Vec<_>>(),
            vec!["/x/a.bib", "/x/b.bib"]
        );
        assert_eq!(p.head("/x/a.bib").unwrap().redo, vec![3]);
        assert_eq!(p.head("/nowhere"), None);
        assert_eq!(p.remove_head("/x/b.bib").unwrap(), 1);
        assert_eq!(
            p.forget_missing_places().unwrap(),
            vec!["/x/a.bib".to_string()]
        );
        assert_eq!(sha256(b"").len(), 64);
    }
}
