//! A project: the history of one library in a store (design/history.md,
//! sections 2.5 to 2.7 and 3): its states, a tree, and the heads of its
//! places. A state is named by its identifier, the sha256 of what it is
//! ([`state_id`]); the numbered states of papers 3.0.0b5 and earlier are
//! upgraded when a project is opened ([`Store::upgrade`]).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;

use super::ojson::J;
use super::ops::FileOp;
use super::store::{Ref, Store, now, sha256, write_bytes};
use crate::config::paths::{dirname, exists, join};
use crate::error::{Error, Result};

/// The identifier of a state: 64 lowercase hexadecimal characters (see [`state_id`]).
pub type StateId = String;

/// The version line of the text that a state identifier hashes.
pub const STATE_VERSION: &str = "papers-state 1";
/// The messages show this many characters of an identifier.
pub const SHORT: usize = 8;
/// A step is named by an identifier or a start of it of at least this many characters.
pub const PREFIX: usize = 4;

/// An identifier as the messages show it: its first [`SHORT`] characters.
pub fn short(id: &str) -> &str {
    match id.char_indices().nth(SHORT) {
        Some((at, _)) => &id[..at],
        None => id,
    }
}

/// Whether a name is the identifier of a state: 64 lowercase hexadecimal characters.
pub fn is_state_id(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The identifier of a state: what the state is (its parent, its time to the microsecond, the
/// checksum of its bibtex, the pieces of the other files) and nothing that a later command may
/// change (the file operations, the deleted files, the labels).
pub fn state_id(state: &State) -> StateId {
    let parent = state.parent().unwrap_or_else(|| "-".to_string());
    let mut text = format!(
        "{STATE_VERSION}\nparent {parent}\ntime {:.6}\nbibtex {}\n",
        state.time(),
        state.bibtex_sha().unwrap_or("")
    );
    let mut files = state.files().unwrap_or_default();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, pieces) in files {
        text.push_str(&format!("file {name} {}\n", pieces.join(",")));
    }
    sha256(text.as_bytes())
}

/// The text of a state: the pieces of a bibtex (or of a kept file), and the
/// sha256 of the whole text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub pieces: Vec<String>,
    pub sha: String,
}

/// The state of a pool that a link names: its identifier, or (a link of papers 3.0.0b5 and earlier
/// to a pool in another store, see [`Store::upgrade`]) its number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkedState {
    Id(StateId),
    Number(u64),
}

impl LinkedState {
    /// As the messages show it.
    pub fn shown(&self) -> String {
        match self {
            LinkedState::Id(id) => short(id).to_string(),
            LinkedState::Number(n) => n.to_string(),
        }
    }

    fn to_j(&self) -> J {
        match self {
            LinkedState::Id(id) => J::str(id),
            LinkedState::Number(n) => J::Int(*n as i64),
        }
    }
}

/// The `base` of a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Base {
    /// No `base` (a link written before papers 3.0.0b6): the parent of the state.
    Absent,
    /// `null`: the linked step was the first of its project.
    Null,
    Id(StateId),
}

/// A link from a state of a library to the state that the same command made
/// in another library (its pool): `pools` of the state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolLink {
    /// The last state that the command made in the project of the other library.
    pub state: LinkedState,
    /// The state of the other library before the command (before the first of its saves).
    pub base: Base,
    /// The configuration file of the other library.
    pub config: String,
}

impl PoolLink {
    /// A link written now.
    pub fn new(state: StateId, base: Option<StateId>, config: String) -> PoolLink {
        PoolLink {
            state: LinkedState::Id(state),
            base: base.map_or(Base::Null, Base::Id),
            config,
        }
    }
}

/// What makes a new state.
#[derive(Debug, Clone)]
pub struct NewState {
    pub parent: Option<StateId>,
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
            ("parent".into(), parts.parent.map_or(J::Null, J::Str)),
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

    pub fn parent(&self) -> Option<StateId> {
        self.0.get("parent").and_then(J::as_str).map(str::to_string)
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
                    let state = match link.get("state")? {
                        J::Str(id) => LinkedState::Id(id.clone()),
                        number => LinkedState::Number(number.as_u64()?),
                    };
                    let base = match link.get("base") {
                        None => Base::Absent,
                        Some(J::Str(id)) => Base::Id(id.clone()),
                        Some(_) => Base::Null,
                    };
                    Some((
                        id.clone(),
                        PoolLink {
                            state,
                            base,
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
    pub fn reverts(&self) -> Vec<StateId> {
        self.0.get("reverts").map(J::strings).unwrap_or_default()
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
                    let mut link = vec![("state".to_string(), l.state.to_j())];
                    match &l.base {
                        Base::Absent => {}
                        Base::Null => link.push(("base".into(), J::Null)),
                        Base::Id(base) => link.push(("base".into(), J::str(base))),
                    }
                    link.push(("config".into(), J::str(&l.config)));
                    (id.clone(), J::Obj(link))
                })
                .collect(),
        );
        self.0.set("pools", j);
    }

    pub fn set_reverts(&mut self, reverts: &[StateId]) {
        self.0.set("reverts", J::strs(reverts));
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
    pub state: StateId,
    /// The states that redo goes to, the next one first.
    pub redo: Vec<StateId>,
    pub time: f64,
}

/// A place of `heads.json`, as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaceHead {
    pub place: String,
    pub state: Option<StateId>,
    pub redo: Vec<StateId>,
    pub time: f64,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub dir: String,
    states_dir: String,
    store: Store,
    states: BTreeMap<StateId, State>,
    /// The state files that cannot be read (not a JSON object, e.g. emptied by a power loss):
    /// not states, but still named (spec/history.md 2.9).
    damaged: BTreeSet<StateId>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub(super) fn read_json(path: &str) -> Option<J> {
    let bytes = fs::read(path).ok()?;
    J::parse(std::str::from_utf8(&bytes).ok()?)
}

/// The number of a file of a numbered state (`000001.json`), of an earlier version.
pub(super) fn state_number(name: &str) -> Option<u64> {
    let digits = name.strip_suffix(".json")?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn listing(folder: &str) -> Vec<String> {
    fs::read_dir(folder)
        .map(|d| {
            d.flatten()
                .map(|n| n.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
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
            store: store.clone(),
            states: BTreeMap::new(),
            damaged: BTreeSet::new(),
        };
        project.reload();
        Ok(project)
    }

    /// Read the states again (another command may have added some). The numbered states of an
    /// earlier version are upgraded first (see [`Store::upgrade`]).
    pub fn reload(&mut self) {
        self.states.clear();
        self.damaged.clear();
        let mut names = listing(&self.states_dir);
        if names.iter().any(|n| state_number(n).is_some()) {
            let _ = self.store.upgrade();
            names = listing(&self.states_dir);
        }
        for name in names {
            let Some(id) = name.strip_suffix(".json").filter(|id| is_state_id(id)) else {
                continue;
            };
            match read_json(&join(&self.states_dir, &name)) {
                Some(j) if j.is_object() => {
                    self.states.insert(id.to_string(), State(j));
                }
                _ => {
                    self.damaged.insert(id.to_string());
                }
            }
        }
    }

    pub fn exists(&self) -> bool {
        crate::config::paths::isdir(&self.dir)
    }

    // states
    // ------

    pub fn states(&self) -> &BTreeMap<StateId, State> {
        &self.states
    }

    pub fn state(&self, n: &str) -> Option<&State> {
        self.states.get(n)
    }

    /// The identifiers of the state files that cannot be read (not a JSON object).
    pub fn damaged(&self) -> &BTreeSet<StateId> {
        &self.damaged
    }

    /// Whether the state file of `n` cannot be read.
    pub fn is_damaged(&self, n: &str) -> bool {
        self.damaged.contains(n)
    }

    /// The identifier of the state that an earlier version numbered `n` (a link from a state of
    /// another store, see [`Store::upgrade`]); none if it is not known.
    pub fn upgraded(&self, n: &LinkedState) -> Option<StateId> {
        match n {
            LinkedState::Id(id) => Some(id.clone()),
            LinkedState::Number(n) => self
                .store
                .numbers()
                .get(&self.id)
                .and_then(|ids| ids.get(&n.to_string()))
                .cloned(),
        }
    }

    /// The identifier of the state that `reference` names: the identifier, or a unique start of
    /// it of at least [`PREFIX`] characters (without case, spaces dropped); a damaged state file
    /// is named too. `Ok(None)` if no state has it; an error (the message lists them) if several
    /// have it.
    pub fn resolve(&self, reference: &str) -> std::result::Result<Option<StateId>, String> {
        let reference = crate::bib::pystr::strip(reference).to_lowercase();
        if self.states.contains_key(&reference) || self.damaged.contains(&reference) {
            return Ok(Some(reference));
        }
        if reference.chars().count() < PREFIX
            || !reference
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Ok(None);
        }
        let known: BTreeSet<&StateId> = self.states.keys().chain(&self.damaged).collect();
        let found: Vec<&StateId> = known
            .into_iter()
            .filter(|n| n.starts_with(&reference))
            .collect();
        match found.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some((*one).clone())),
            several => Err(format!(
                "{reference} names several steps: {}",
                several
                    .iter()
                    .map(|n| short(n))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    fn path(&self, n: &str) -> String {
        join(&self.states_dir, &format!("{n}.json"))
    }

    /// Write a new state, under its identifier ([`state_id`]), created exclusively (the same state,
    /// written already by another command, is not written again); the identifier.
    pub fn add(&mut self, state: State) -> Result<StateId> {
        fs::create_dir_all(&self.states_dir)
            .map_err(|e| Error::other(format!("{}: {e}", self.states_dir)))?;
        let n = state_id(&state);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.path(&n))
        {
            Ok(mut file) => file
                .write_all(state.0.dumps().as_bytes())
                .map_err(|e| Error::other(format!("{}: {e}", self.path(&n))))?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(Error::other(format!("{}: {e}", self.path(&n)))),
        }
        self.states.insert(n.clone(), state);
        Ok(n)
    }

    /// Rewrite the state `n` (e.g. the file operations done after its save).
    pub fn update(&mut self, n: &str, state: State) -> Result<()> {
        write_bytes(&self.path(n), state.0.dumps().as_bytes())?;
        self.states.insert(n.to_string(), state);
        self.damaged.remove(n);
        Ok(())
    }

    pub fn drop_state(&mut self, n: &str) {
        let _ = fs::remove_file(self.path(n));
        self.states.remove(n);
        self.damaged.remove(n);
    }

    /// The states from `n` back to the oldest one kept: `n`, its parent, …
    pub fn line(&self, n: &str) -> Vec<StateId> {
        let mut line: Vec<StateId> = Vec::new();
        let mut at = Some(n.to_string());
        while let Some(m) = at {
            let Some(state) = self.states.get(&m) else {
                break;
            };
            if line.contains(&m) {
                break;
            }
            at = state.parent();
            line.push(m);
        }
        line
    }

    /// From the state `a` to the state `b`: the states left going back (`a`
    /// first, down to before the common ancestor) and the states entered
    /// going forward (`b` last).
    pub fn path_between(&self, a: &str, b: &str) -> (Vec<StateId>, Vec<StateId>) {
        let up = self.line(a);
        let down = self.line(b);
        let common = up.iter().find(|n| down.contains(n)).cloned();
        let back = match &common {
            Some(c) => up[..up.iter().position(|n| n == c).unwrap_or(up.len())].to_vec(),
            None => up,
        };
        let mut forward = match &common {
            Some(c) => down[..down.iter().position(|n| n == c).unwrap_or(down.len())].to_vec(),
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
                state: h.get("state").and_then(J::as_str).map(str::to_string),
                redo: h.get("redo").map(J::strings).unwrap_or_default(),
                time: h.get("time").and_then(J::as_f64).unwrap_or(0.0),
            })
            .collect()
    }

    /// The head of a place, if its state exists; the redo states that no
    /// longer exist are left out (a damaged one stays: redo stops there).
    pub fn head(&self, place: &str) -> Option<Head> {
        let h = self.heads().into_iter().find(|h| h.place == place)?;
        let state = h.state.filter(|n| self.states.contains_key(n))?;
        Some(Head {
            state,
            redo: h
                .redo
                .into_iter()
                .filter(|n| self.states.contains_key(n) || self.damaged.contains(n))
                .collect(),
            time: h.time,
        })
    }

    pub fn set_head(&self, place: &str, n: &str, redo: &[StateId]) -> Result<()> {
        let mut heads = self.heads_json();
        let head = J::Obj(vec![
            ("state".into(), J::str(n)),
            ("redo".into(), J::strs(redo)),
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
    /// has no state); the largest identifier among states of one time.
    pub fn start_state(&self, sha: &str) -> Option<StateId> {
        let same: Vec<&StateId> = self
            .states
            .iter()
            .filter(|(_, s)| s.bibtex_sha() == Some(sha))
            .map(|(n, _)| n)
            .collect();
        let pool: Vec<&StateId> = if same.is_empty() {
            self.states.keys().collect()
        } else {
            same
        };
        pool.into_iter()
            .max_by(|a, b| {
                self.states[*a]
                    .time()
                    .total_cmp(&self.states[*b].time())
                    .then(a.cmp(b))
            })
            .cloned()
    }

    /// Delete the history of the project (the pieces and files go at the
    /// next prune).
    pub fn remove(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
        self.states.clear();
        self.damaged.clear();
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

    /// The path of the file of a state (the retention reads its modification time).
    pub(super) fn state_path(&self, n: &str) -> String {
        self.path(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::store::sha256;

    fn state(parent: Option<&str>, time: f64, sha: &str) -> State {
        State::new(NewState {
            parent: parent.map(str::to_string),
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
    fn the_identifier_of_a_state() {
        let mut s = state(None, 1793189487.9236956, "e3b0");
        let first = state_id(&s);
        assert_eq!(first.len(), 64);
        // what the specification hashes: the version, the parent, the time (6 decimals), the bibtex, the files
        let text = "papers-state 1\nparent -\ntime 1793189487.923696\nbibtex e3b0\n";
        assert_eq!(first, sha256(text.as_bytes()));
        // the operations, the message and the labels are not part of it
        s.set_message("papers other");
        s.set_ops(&[], &["x".to_string()]);
        s.set_reverts(&["y".to_string()]);
        assert_eq!(state_id(&s), first);
        let t = State::new(NewState {
            parent: Some(first.clone()),
            time: 2.5,
            kind: "command".into(),
            message: String::new(),
            ops: vec![],
            deleted: vec![],
            bibtex: Text {
                pieces: vec![],
                sha: "ab".into(),
            },
            files: vec![
                (".papers/files.json".into(), vec!["p1".into(), "p2".into()]),
                (".papers/distinct.json".into(), vec!["p3".into()]),
            ],
        });
        let text = format!(
            "papers-state 1\nparent {first}\ntime 2.500000\nbibtex ab\nfile .papers/distinct.json p3\nfile .papers/files.json p1,p2\n"
        );
        assert_eq!(state_id(&t), sha256(text.as_bytes()));
        assert_eq!(short(&first), &first[..8]);
    }

    #[test]
    fn states_lines_and_paths() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_string_lossy().into_owned());
        let mut p = store.project("abc").unwrap();
        assert!(Store::project(&store, "a/b").is_err());
        let a = p.add(state(None, 1.0, "a")).unwrap();
        let b = p.add(state(Some(&a), 2.0, "b")).unwrap();
        let c = p.add(state(Some(&b), 3.0, "c")).unwrap();
        let d = p.add(state(Some(&b), 4.0, "d")).unwrap();
        assert!(std::path::Path::new(&format!("{}/states/{a}.json", p.dir)).exists());
        assert_eq!(p.line(&c), vec![c.clone(), b.clone(), a.clone()]);
        assert_eq!(p.path_between(&c, &d), (vec![c.clone()], vec![d.clone()]));
        assert_eq!(p.path_between(&c, &a), (vec![c.clone(), b.clone()], vec![]));
        assert_eq!(p.path_between(&a, &c), (vec![], vec![b.clone(), c.clone()]));
        assert_eq!(p.start_state("b"), Some(b.clone()));
        assert_eq!(p.start_state("zzz"), Some(d.clone()));
        // the same state, written again, is the same file
        assert_eq!(p.add(state(None, 1.0, "a")).unwrap(), a);
        assert_eq!(p.states().len(), 4);
        // a state added by another command is read after a reload
        let mut q = store.project("abc").unwrap();
        let e = p.add(state(Some(&a), 5.0, "e")).unwrap();
        q.reload();
        assert_eq!(q.states().len(), 5);
        assert!(q.state(&e).is_some());
        // heads: the order of the places stays, redo lists skip the states that are gone
        p.set_head("/x/a.bib", &c, &[d.clone(), "9".repeat(64)])
            .unwrap();
        p.set_head("/x/b.bib", &a, &[]).unwrap();
        p.set_head("/x/a.bib", &b, std::slice::from_ref(&c))
            .unwrap();
        let heads = p.heads();
        assert_eq!(
            heads.iter().map(|h| h.place.as_str()).collect::<Vec<_>>(),
            vec!["/x/a.bib", "/x/b.bib"]
        );
        assert_eq!(p.head("/x/a.bib").unwrap().redo, vec![c.clone()]);
        assert_eq!(p.head("/nowhere"), None);
        assert_eq!(p.remove_head("/x/b.bib").unwrap(), 1);
        assert_eq!(
            p.forget_missing_places().unwrap(),
            vec!["/x/a.bib".to_string()]
        );
        assert_eq!(sha256(b"").len(), 64);
    }

    #[test]
    fn naming_a_state() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_string_lossy().into_owned());
        let mut p = store.project("abc").unwrap();
        let a = p.add(state(None, 1.0, "a")).unwrap();
        assert_eq!(p.resolve(&a).unwrap(), Some(a.clone()));
        assert_eq!(p.resolve(&a[..4]).unwrap(), Some(a.clone()));
        assert_eq!(
            p.resolve(&format!(" {} ", a[..6].to_uppercase())).unwrap(),
            Some(a.clone())
        );
        // too short, not hexadecimal, no such state
        assert_eq!(p.resolve(&a[..3]).unwrap(), None);
        assert_eq!(p.resolve("zzzz").unwrap(), None);
        assert_eq!(p.resolve("").unwrap(), None);
        // several states with one start: refused
        let mut ids = vec![a.clone()];
        for t in 2..400 {
            ids.push(p.add(state(None, t as f64, "x")).unwrap());
        }
        let twins = ids
            .iter()
            .find(|i| ids.iter().filter(|j| j[..3] == i[..3]).count() > 1)
            .expect("two of 400 identifiers start alike");
        let found = ids.iter().filter(|j| j[..3] == twins[..3]).count();
        assert!(found > 1);
        assert_eq!(p.resolve(&ids[5]).unwrap(), Some(ids[5].clone()));
    }

    #[test]
    fn links_of_pools() {
        let mut s = state(None, 1.0, "a");
        let links = vec![
            (
                "p1".to_string(),
                PoolLink::new("a".repeat(64), Some("b".repeat(64)), "/x/c.json".into()),
            ),
            (
                "p2".to_string(),
                PoolLink::new("c".repeat(64), None, "/y/c.json".into()),
            ),
        ];
        s.set_pools(&links);
        assert_eq!(s.pools(), links);
        // a link of an earlier version: a number, no base
        let old = J::parse(r#"{"pools": {"p": {"state": 3, "config": "/c"}}}"#).unwrap();
        let state = State(old);
        let pools = state.pools();
        assert_eq!(pools[0].1.state, LinkedState::Number(3));
        assert_eq!(pools[0].1.base, Base::Absent);
    }
}
