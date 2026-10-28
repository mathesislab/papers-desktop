//! The writer of each .bib file of the library. A papers library is written
//! by papers, so that a change from the app or a terminal has the same
//! format, naming and history: by papers-rs, built in (the default), or by a
//! papers command, as the settings say; they share the files, the history
//! and the lock, so either can undo a step of the other. Any other .bib is
//! written by the app's own writer, which rewrites only the entries changed.
//! With a papers command that is not found (3.0.0b4 or later), a papers
//! library is read-only.
//!
//! The two ways of writing a papers library are the two implementations of
//! [`PapersWriter`]: [`builtin::Builtin`] and [`command::PapersCommand`]. The
//! commands of this module group the changes by library and hand them to the
//! writer of the library.

mod builtin;
mod command;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use papers_core::{Added, Change, Item, same_tag, under};
use papers_rs::command::report::Changes;
use papers_rs::config::MachineConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::settings::{WriterChoice, expand};
use builtin::Builtin;
use command::{Found, PAPERS_MIN_NAME, PapersCommand};

/// How a .bib file is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// a papers library, by papers-rs, built in
    PapersRs,
    /// a papers library, by a papers command
    Papers,
    /// by the app
    Native,
    /// a papers library, read-only: the papers command is not found, or too old
    PapersMissing,
}

/// The writer of a .bib file, for the page.
#[derive(Debug, Clone, Serialize)]
pub struct Writer {
    pub bib: PathBuf,
    pub kind: Kind,
    /// of the papers command found, for a papers library
    pub version: Option<String>,
    /// why the library cannot be written, when it is known (e.g. a pool, which
    /// papers-rs does not support yet)
    pub problem: Option<String>,
}

/// An error of a write, of a kind the page tells apart.
#[derive(Debug, Serialize)]
pub struct WriteError {
    kind: ErrorKind,
    message: String,
    /// for `Kept`: what an edit of an entry changed, "key", "tags" or "file"
    #[serde(skip_serializing_if = "Option::is_none")]
    what: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorKind {
    /// a papers library, and the papers command is not found or too old
    PapersMissing,
    /// the .bib changed since it was read: the library is read again
    Changed,
    /// an edit of the BibTeX of an entry changed what it keeps: see `what`
    Kept,
    Failed,
}

impl WriteError {
    fn failed(message: impl Into<String>) -> WriteError {
        WriteError {
            kind: ErrorKind::Failed,
            message: message.into(),
            what: None,
        }
    }

    /// papers not found, or the version found too old
    fn missing(version: Option<&str>) -> WriteError {
        let message = match version {
            Some(v) => format!(
                "This library is written by papers {PAPERS_MIN_NAME} or later, \
                 and papers {v} is installed"
            ),
            None => "This library is written by papers, which is not found".to_string(),
        };
        WriteError {
            kind: ErrorKind::PapersMissing,
            message,
            what: None,
        }
    }
}

impl From<papers_core::Error> for WriteError {
    fn from(e: papers_core::Error) -> WriteError {
        let (kind, what) = match e {
            papers_core::Error::Changed { .. } => (ErrorKind::Changed, None),
            papers_core::Error::Kept { what } => (ErrorKind::Kept, Some(what)),
            _ => (ErrorKind::Failed, None),
        };
        WriteError {
            kind,
            message: e.to_string(),
            what,
        }
    }
}

/// What writes the papers libraries: the commands of papers, one backend or
/// the other, which must leave the same files and history. Each takes the
/// folder of the library; the reports tell the entries changed.
trait PapersWriter: Send + Sync {
    fn kind(&self) -> Kind;

    /// of the papers command
    fn version(&self) -> Option<String>;

    /// Why this library cannot be written, if it is known without trying.
    fn refuses(&self, _folder: &Path) -> Option<String> {
        None
    }

    /// `papers tag KEY... --add TAG... --rm TAG...`
    fn tag(
        &self,
        folder: &Path,
        keys: &[String],
        add: &[String],
        remove: &[String],
    ) -> Result<Changes, WriteError>;

    /// `papers tag --rename OLD NEW`: a tag as written, and the tags under it
    fn rename_tag(&self, folder: &Path, old: &str, new: &str) -> Result<Changes, WriteError>;

    /// `papers edit KEY --from -`: the BibTeX of an entry replaced
    fn edit(&self, folder: &Path, key: &str, text: &str) -> Result<Changes, WriteError>;

    /// `papers add FILE --on-duplicate ...`: the entries of BibTeX added
    fn add(&self, folder: &Path, text: &str, options: AddOptions) -> Result<Changes, WriteError>;

    /// `papers rm KEY... --keep-files`
    fn remove(&self, folder: &Path, keys: &[String]) -> Result<Changes, WriteError>;

    /// `papers undo`, or with `back` false `papers redo`
    fn step(&self, folder: &Path, back: bool) -> Result<(), WriteError>;
}

/// A papers library: its folder, and what writes it.
struct PapersLibrary {
    folder: PathBuf,
    writer: Arc<dyn PapersWriter>,
}

/// The papers library of each .bib file of a write that has one.
struct Targets {
    libraries: HashMap<PathBuf, PapersLibrary>,
}

impl Targets {
    /// The papers library of a .bib file; none: the app writes it.
    fn papers(&self, bib: &Path) -> Option<&PapersLibrary> {
        self.libraries.get(bib)
    }
}

#[derive(Default)]
struct History {
    undo: Vec<Change>,
    redo: Vec<Change>,
}

/// What writes the papers libraries, as the settings say.
enum Backend {
    Ready(Arc<dyn PapersWriter>),
    /// a papers command, not found or too old: its version
    Missing(Option<String>),
}

/// The writers of the .bib files.
#[derive(Default)]
pub struct Writers {
    /// what writes the papers libraries
    choice: Mutex<WriterChoice>,
    /// the papers command of the settings; empty: the one the login shell finds
    setting: Mutex<PathBuf>,
    /// the papers command found, with the setting it was found for
    found: Mutex<Option<(PathBuf, PapersCommand)>>,
    /// the global configuration of papers for the built-in writer; none: the
    /// user's (the tests give the history a folder of its own)
    machine: Mutex<Option<MachineConfig>>,
    /// what to undo and redo of the files the app writes, this session
    history: Mutex<HashMap<PathBuf, History>>,
    /// one write at a time: each starts from the library read after the last
    writing: Mutex<()>,
}

impl Writers {
    /// What writes the papers libraries, and the papers command of the
    /// settings, which a command is.
    pub fn configure(&self, choice: WriterChoice, command: PathBuf) {
        *self.choice.lock().unwrap() = choice;
        *self.setting.lock().unwrap() = command;
    }

    /// The papers command of the settings, else the one the login shell
    /// finds; kept once found. One not found is looked for again the next
    /// time: it may have been installed since.
    fn command(&self) -> Found {
        let setting = self.setting.lock().unwrap().clone();
        if let Some((found_for, papers)) = &*self.found.lock().unwrap()
            && *found_for == setting
        {
            return Found::Usable(papers.clone());
        }
        let found = PapersCommand::find(&setting);
        if let Found::Usable(papers) = &found {
            *self.found.lock().unwrap() = Some((setting, papers.clone()));
        }
        found
    }

    /// Forget the papers command found: it is gone, or changed.
    fn forget_command(&self) {
        *self.found.lock().unwrap() = None;
    }

    /// What writes the papers libraries.
    fn backend(&self) -> Backend {
        match *self.choice.lock().unwrap() {
            WriterChoice::BuiltIn => {
                let machine = self.machine.lock().unwrap().clone();
                Backend::Ready(Arc::new(Builtin::new(machine)))
            }
            WriterChoice::Command => match self.command() {
                Found::Usable(papers) => Backend::Ready(Arc::new(papers)),
                Found::Missing(version) => Backend::Missing(version),
            },
        }
    }

    /// The writer of each .bib file, papers looked for once.
    fn writers(&self, bibs: &[PathBuf]) -> Vec<Writer> {
        let mut backend = None;
        bibs.iter()
            .map(|bib| {
                let writer = |kind, version, problem| Writer {
                    bib: bib.clone(),
                    kind,
                    version,
                    problem,
                };
                let Some(folder) = papers_folder(bib) else {
                    return writer(Kind::Native, None, None);
                };
                match backend.get_or_insert_with(|| self.backend()) {
                    Backend::Ready(papers) => {
                        writer(papers.kind(), papers.version(), papers.refuses(&folder))
                    }
                    Backend::Missing(version) => writer(Kind::PapersMissing, version.clone(), None),
                }
            })
            .collect()
    }

    /// The papers libraries of the .bib files of a write, which a papers
    /// library without its writer refuses.
    fn targets<'a>(&self, bibs: impl IntoIterator<Item = &'a Path>) -> Result<Targets, WriteError> {
        let bibs: BTreeSet<&Path> = bibs.into_iter().collect();
        let mut backend = None;
        let mut libraries = HashMap::new();
        for bib in bibs {
            let Some(folder) = papers_folder(bib) else {
                continue;
            };
            match backend.get_or_insert_with(|| self.backend()) {
                Backend::Ready(writer) => {
                    let writer = Arc::clone(writer);
                    libraries.insert(bib.to_path_buf(), PapersLibrary { folder, writer });
                }
                Backend::Missing(version) => return Err(WriteError::missing(version.as_deref())),
            }
        }
        Ok(Targets { libraries })
    }

    /// Keep the changes the app wrote, to undo them.
    fn done(&self, changes: Vec<Change>) {
        let mut history = self.history.lock().unwrap();
        for change in changes {
            let steps = history.entry(change.bib().to_path_buf()).or_default();
            steps.redo.clear();
            steps.undo.push(change);
        }
    }

    /// Undo the last change the app wrote to a .bib file, or redo the last
    /// one undone, unless the file changed since.
    fn step(&self, bib: &Path, back: bool) -> Result<(), WriteError> {
        let mut history = self.history.lock().unwrap();
        let steps = history.entry(bib.to_path_buf()).or_default();
        let (what, from, to) = if back {
            ("undo", &mut steps.undo, &mut steps.redo)
        } else {
            ("redo", &mut steps.redo, &mut steps.undo)
        };
        let Some(change) = from.pop() else {
            return Err(WriteError::failed(format!("nothing to {what}")));
        };
        let done = if back { change.undo() } else { change.redo() };
        match done {
            Ok(()) => {
                to.push(change);
                Ok(())
            }
            Err(papers_core::Error::Changed { path }) => {
                *steps = History::default();
                Err(WriteError {
                    kind: ErrorKind::Changed,
                    message: format!(
                        "{}: changed since the app wrote it; nothing to {what}",
                        path.display()
                    ),
                    what: None,
                })
            }
            Err(e) => {
                from.push(change);
                Err(e.into())
            }
        }
    }
}

/// The folder of the papers library of a .bib file: its folder, when that
/// holds `.papers/config.json` whose `bibtex` names the file.
fn papers_folder(bib: &Path) -> Option<PathBuf> {
    let folder = bib.parent()?;
    let config = fs::read_to_string(folder.join(".papers/config.json")).ok()?;
    let config: Value = serde_json::from_str(&config).ok()?;
    let named = folder.join(expand(config["bibtex"].as_str()?));
    let same = match (fs::canonicalize(&named), fs::canonicalize(bib)) {
        (Ok(a), Ok(b)) => a == b,
        _ => named == bib,
    };
    same.then(|| folder.to_path_buf())
}

/// BibTeX pasted, in a temporary .bib file for `papers add`; removed when dropped.
struct Pasted(PathBuf);

impl Pasted {
    fn new(text: &str) -> Result<Pasted, WriteError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = format!("papers-desktop-{}-{nanos}.bib", std::process::id());
        let path = std::env::temp_dir().join(name);
        fs::write(&path, text)
            .map_err(|e| WriteError::failed(format!("{}: {e}", path.display())))?;
        Ok(Pasted(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Pasted {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// The tags of an item, `add` added and `remove` removed, as tags are
/// compared ("AMOC" is "amoc"): what is added, and what is removed as
/// written in the item.
struct Retag {
    tags: Vec<String>,
    added: Vec<String>,
    removed: Vec<String>,
}

fn retag(tags: &[String], add: &[String], remove: &[String]) -> Retag {
    let (removed, mut kept): (Vec<String>, Vec<String>) = tags
        .iter()
        .cloned()
        .partition(|t| remove.iter().any(|r| same_tag(t, r)));
    let mut added = Vec::new();
    for tag in add.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        if !kept.iter().any(|t| same_tag(t, tag)) {
            kept.push(tag.to_string());
            added.push(tag.to_string());
        }
    }
    Retag {
        tags: kept,
        added,
        removed,
    }
}

/// The items of ids.
fn items(state: &AppState, ids: &[String]) -> Result<Vec<Item>, WriteError> {
    ids.iter()
        .map(|id| state.item(id).map_err(WriteError::failed))
        .collect()
}

fn in_library(state: &AppState, bib: &Path) -> Result<(), WriteError> {
    if state
        .library
        .read()
        .unwrap()
        .bibs()
        .iter()
        .any(|b| b == bib)
    {
        Ok(())
    } else {
        Err(WriteError::failed(format!(
            "{}: not a .bib file of the library",
            bib.display()
        )))
    }
}

/// Run a write on a thread of its own, one at a time. Then read the library
/// again, for the next write to start from the files as written; and after
/// a refusal, for the next try.
async fn write<T: Send + 'static>(
    app: AppHandle,
    op: impl FnOnce(&AppState) -> Result<T, WriteError> + Send + 'static,
) -> Result<T, WriteError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _writing = state.writers.writing.lock().unwrap();
        let result = op(&state);
        if let Err(e) = &result
            && e.kind == ErrorKind::PapersMissing
        {
            state.writers.forget_command();
        }
        let reload = match &result {
            Ok(_) => true,
            Err(e) => e.kind == ErrorKind::Changed,
        };
        if reload {
            crate::load(&app);
        }
        result
    })
    .await
    .map_err(|e| WriteError::failed(e.to_string()))?
}

/// The writer of each .bib file of the library.
#[tauri::command]
pub async fn writers(app: AppHandle) -> Vec<Writer> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let bibs = state.library.read().unwrap().bibs().to_vec();
        state.writers.writers(&bibs)
    })
    .await
    .unwrap_or_default()
}

/// Add tags to items, and remove tags from them.
#[tauri::command]
pub async fn set_tags(
    app: AppHandle,
    ids: Vec<String>,
    add: Vec<String>,
    remove: Vec<String>,
) -> Result<(), WriteError> {
    write(app, move |state| tag(state, &ids, &add, &remove)).await
}

/// Rename a tag in the whole library.
#[tauri::command]
pub async fn rename_tag(app: AppHandle, old: String, new: String) -> Result<(), WriteError> {
    write(app, move |state| rename(state, &old, &new)).await
}

/// Replace the BibTeX of an entry; its key, tags and files stay.
#[tauri::command]
pub async fn replace_entry(app: AppHandle, id: String, text: String) -> Result<(), WriteError> {
    write(app, move |state| replace(state, &id, &text)).await
}

/// Add the entries of pasted BibTeX to a .bib file; returns their keys,
/// added and existing.
#[tauri::command]
pub async fn add_bibtex(
    app: AppHandle,
    bib: PathBuf,
    text: String,
    options: AddOptions,
) -> Result<Added, WriteError> {
    write(app, move |state| add(state, &bib, &text, options)).await
}

/// How entries are added to a papers library; a plain .bib has no merge and
/// no key template: its entries are added as they are.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddOptions {
    /// an entry that is a paper of the library already, by papers' rule
    pub on_duplicate: OnDuplicate,
    /// the keys of the new entries made by the library's key template
    /// (papers `--update-key`)
    pub update_key: bool,
}

/// papers `--on-duplicate`: an added entry that is a paper of the library.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnDuplicate {
    /// the library's entry as it is
    #[default]
    Skip,
    /// merged, the library's values kept where they differ
    Keep,
    /// merged, the added values taken where they differ
    Update,
}

impl OnDuplicate {
    fn arg(self) -> &'static str {
        match self {
            OnDuplicate::Skip => "skip",
            OnDuplicate::Keep => "keep",
            OnDuplicate::Update => "update",
        }
    }
}

/// Remove entries from the library; their files stay.
#[tauri::command]
pub async fn remove_entries(app: AppHandle, ids: Vec<String>) -> Result<(), WriteError> {
    write(app, move |state| remove(state, &ids)).await
}

/// Undo the last change of a .bib file: with papers in a papers library,
/// else the last change the app wrote to it.
#[tauri::command]
pub async fn undo(app: AppHandle, bib: PathBuf) -> Result<(), WriteError> {
    write(app, move |state| step(state, &bib, true)).await
}

/// Redo the last change undone of a .bib file.
#[tauri::command]
pub async fn redo(app: AppHandle, bib: PathBuf) -> Result<(), WriteError> {
    write(app, move |state| step(state, &bib, false)).await
}

fn tag(
    state: &AppState,
    ids: &[String],
    add: &[String],
    remove: &[String],
) -> Result<(), WriteError> {
    let items = items(state, ids)?;
    let targets = state
        .writers
        .targets(items.iter().map(|it| it.bib.as_path()))?;
    let mut native = Vec::new();
    // one papers command for the keys of a library that change alike
    let mut commands: BTreeMap<_, (&PapersLibrary, Vec<String>)> = BTreeMap::new();
    for item in &items {
        let new = retag(&item.keywords, add, remove);
        if new.added.is_empty() && new.removed.is_empty() {
            continue;
        }
        match targets.papers(&item.bib) {
            None => native.push((item.id.clone(), new.tags)),
            Some(library) => commands
                .entry((item.bib.as_path(), new.added, new.removed))
                .or_insert((library, Vec::new()))
                .1
                .push(item.key.clone()),
        }
    }
    let changes = state.library.read().unwrap().set_tags(&native)?;
    state.writers.done(changes);
    for ((_, added, removed), (library, keys)) in commands {
        library
            .writer
            .tag(&library.folder, &keys, &added, &removed)?;
    }
    Ok(())
}

/// The tag that renaming `old` to `new` leaves of `tag`: `new` for `old`,
/// `new/rest` for `old/rest`, and nothing for any other tag.
fn renamed(tag: &str, old: &str, new: &str) -> Option<String> {
    if same_tag(tag, old) {
        Some(new.to_string())
    } else {
        under(tag, old).map(|rest| format!("{new}/{rest}"))
    }
}

/// Renames a tag, and the tags under it.
fn rename(state: &AppState, old: &str, new: &str) -> Result<(), WriteError> {
    let new = new.trim();
    if new.is_empty() {
        return Err(WriteError::failed("A tag needs a name"));
    }
    let tagged: Vec<Item> = {
        let library = state.library.read().unwrap();
        let has = |it: &&Item| it.keywords.iter().any(|t| renamed(t, old, new).is_some());
        library.items().iter().filter(has).cloned().collect()
    };
    let targets = state
        .writers
        .targets(tagged.iter().map(|it| it.bib.as_path()))?;
    let mut native = Vec::new();
    // papers renames a tag as written: each spelling of it, and of the tags
    // under it, to what it becomes
    type Renames<'a> = BTreeSet<(&'a str, String)>;
    let mut spellings: BTreeMap<&Path, (&PapersLibrary, Renames)> = BTreeMap::new();
    for item in &tagged {
        match targets.papers(&item.bib) {
            None => {
                let mut tags: Vec<String> = Vec::new();
                for tag in &item.keywords {
                    let tag = renamed(tag, old, new).unwrap_or_else(|| tag.clone());
                    if !tags.iter().any(|t| same_tag(t, &tag)) {
                        tags.push(tag);
                    }
                }
                native.push((item.id.clone(), tags));
            }
            Some(library) => {
                let written = item
                    .keywords
                    .iter()
                    .filter_map(|t| Some((t.as_str(), renamed(t, old, new)?)));
                spellings
                    .entry(item.bib.as_path())
                    .or_insert((library, BTreeSet::new()))
                    .1
                    .extend(written);
            }
        }
    }
    let changes = state.library.read().unwrap().set_tags(&native)?;
    state.writers.done(changes);
    for (library, spellings) in spellings.into_values() {
        // the tags under a tag first, so that a name made by one rename is not
        // renamed again by the next (`a` to `a/b` makes the `a/b` of before)
        for (spelling, to) in spellings.into_iter().rev() {
            library.writer.rename_tag(&library.folder, spelling, &to)?;
        }
    }
    Ok(())
}

fn replace(state: &AppState, id: &str, text: &str) -> Result<(), WriteError> {
    let item = state.item(id).map_err(WriteError::failed)?;
    let targets = state.writers.targets([item.bib.as_path()])?;
    match targets.papers(&item.bib) {
        Some(library) => {
            state.library.read().unwrap().check_edit(id, text)?;
            library.writer.edit(&library.folder, &item.key, text)?;
        }
        None => {
            let changes = state.library.read().unwrap().replace_entry(id, text)?;
            state.writers.done(changes);
        }
    }
    Ok(())
}

fn add(state: &AppState, bib: &Path, text: &str, options: AddOptions) -> Result<Added, WriteError> {
    in_library(state, bib)?;
    papers_core::entry_keys(text)?;
    let targets = state.writers.targets([bib])?;
    match targets.papers(bib) {
        Some(library) => {
            let report = library.writer.add(&library.folder, text, options)?;
            // a paper of the library: as it was, or merged with the entry
            let mut existing = report.existing;
            existing.extend(report.modified);
            Ok(Added {
                added: report.added,
                existing,
            })
        }
        None => {
            let (changes, added) = state.library.read().unwrap().add_entries(bib, text)?;
            state.writers.done(changes);
            Ok(added)
        }
    }
}

fn remove(state: &AppState, ids: &[String]) -> Result<(), WriteError> {
    let items = items(state, ids)?;
    let targets = state
        .writers
        .targets(items.iter().map(|it| it.bib.as_path()))?;
    let mut native = Vec::new();
    let mut keys: BTreeMap<&Path, (&PapersLibrary, Vec<String>)> = BTreeMap::new();
    for item in &items {
        match targets.papers(&item.bib) {
            None => native.push(item.id.clone()),
            Some(library) => keys
                .entry(item.bib.as_path())
                .or_insert((library, Vec::new()))
                .1
                .push(item.key.clone()),
        }
    }
    let changes = state.library.read().unwrap().remove_entries(&native)?;
    state.writers.done(changes);
    for (library, keys) in keys.into_values() {
        library.writer.remove(&library.folder, &keys)?;
    }
    Ok(())
}

fn step(state: &AppState, bib: &Path, back: bool) -> Result<(), WriteError> {
    in_library(state, bib)?;
    let targets = state.writers.targets([bib])?;
    match targets.papers(bib) {
        Some(library) => library.writer.step(&library.folder, back),
        None => state.writers.step(bib, back),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::RwLock;
    use std::sync::atomic::AtomicU64;

    use papers_core::{Library, Matching};
    use papers_rs::config::{Config, MachineConfig};
    use serde_json::json;

    use super::command::{PAPERS_MIN, release};
    use super::*;
    use crate::Status;
    use crate::main_window::Layout;
    use crate::recent::Recent;

    const ENTRIES: &str =
        "@article{a,\n title = {A},\n keywords = {AMOC, ice}\n}\n\n@article{b,\n title = {B}\n}\n";

    /// The global configuration of papers for the tests: the history in a folder of its own.
    fn machine(history: &Path) -> MachineConfig {
        let mut machine = MachineConfig::empty();
        machine.set("history-dir", Some(json!(history.to_str().unwrap())));
        machine
    }

    /// The state of the app over a library of .bib files in `dir`, whose
    /// papers libraries are written by `choice`, with the papers command
    /// `papers` if that is a command.
    fn state(dir: &Path, bibs: &[PathBuf], choice: WriterChoice, papers: &str) -> AppState {
        state_with(dir, bibs, choice, papers, machine(&dir.join("history")))
    }

    fn state_with(
        dir: &Path,
        bibs: &[PathBuf],
        choice: WriterChoice,
        papers: &str,
        machine: MachineConfig,
    ) -> AppState {
        let writers = Writers::default();
        writers.configure(choice, PathBuf::from(papers));
        *writers.machine.lock().unwrap() = Some(machine);
        AppState {
            settings_path: dir.join("settings.json"),
            matching: RwLock::new(Matching::default()),
            citation_style: RwLock::new(String::new()),
            library: RwLock::new(Library::load(bibs).unwrap()),
            status: RwLock::new(Status::Loading),
            watches: Mutex::new(Vec::new()),
            reloads: AtomicU64::new(0),
            search_item: Mutex::new(None),
            update_item: Mutex::new(None),
            recent: Recent::load(dir.join("recent.json")),
            layout: Layout::new(dir.join("window.json")),
            writers,
        }
    }

    /// The library read again, as after a write.
    fn reload(state: &AppState) {
        let bibs = state.library.read().unwrap().bibs().to_vec();
        *state.library.write().unwrap() = Library::load(&bibs).unwrap();
    }

    fn strings(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    /// A new papers library in `dir`, as `papers init` makes it: an empty .bib
    /// (`papers.bib`) and the configuration. Its .bib file.
    fn init_library(dir: &Path) -> PathBuf {
        let bib = dir.join("papers.bib");
        let mut config = Config::new(MachineConfig::empty());
        config.file = Some(
            dir.join(".papers/config.json")
                .to_str()
                .unwrap()
                .to_string(),
        );
        config.is_library = true;
        config.bibtex = Some(bib.to_str().unwrap().to_string());
        config.filesdir = Some(dir.join("files").to_str().unwrap().to_string());
        config.set("paths", json!("relative")).unwrap();
        config.set("pool", json!("none")).unwrap();
        fs::create_dir(dir.join("files")).unwrap();
        fs::write(&bib, "").unwrap();
        config.save().unwrap();
        bib
    }

    /// A papers library in `dir`: its .bib with `ENTRIES`, and its configuration.
    fn papers_library(dir: &Path) -> PathBuf {
        let bib = dir.join("papers.bib");
        fs::write(&bib, ENTRIES).unwrap();
        fs::create_dir(dir.join(".papers")).unwrap();
        fs::write(
            dir.join(".papers/config.json"),
            r#"{"bibtex": "papers.bib"}"#,
        )
        .unwrap();
        bib
    }

    #[test]
    fn tags_compare_as_counted() {
        let new = retag(
            &strings(&["AMOC", "ice"]),
            &strings(&["amoc", "new", "New", " "]),
            &strings(&["ICE"]),
        );
        assert_eq!(new.tags, ["AMOC", "new"]);
        assert_eq!(new.added, ["new"]);
        assert_eq!(new.removed, ["ice"]);
    }

    #[test]
    fn versions() {
        let new_enough = |v| release(v).is_some_and(|r| r >= PAPERS_MIN);
        assert_eq!(release("3.0.0b4.dev18+g6d8b3f390"), Some([3, 0, 0, 1, 4]));
        assert_eq!(release(PAPERS_MIN_NAME), Some(PAPERS_MIN));
        assert_eq!(release("2.5"), Some([2, 5, 0, 3, 0]));
        assert_eq!(release("dev"), None);
        for v in [
            "3.0.0b4.dev18+g6d8b3f390",
            "3.0.0b10",
            "3.0.0rc1",
            "3.0.0",
            "3.1",
        ] {
            assert!(new_enough(v), "{v}");
        }
        for v in ["3.0.0b3", "3.0.0a5", "2.19.6", "dev"] {
            assert!(!new_enough(v), "{v}");
        }
    }

    #[test]
    fn porcelain_reports() {
        let report = Changes::from_porcelain("added c\nexisting a\nmodified b\n\n");
        assert_eq!(report.added, ["c"]);
        assert_eq!(report.existing, ["a"]);
        assert!(report.removed.is_empty());
    }

    #[test]
    fn papers_library_by_its_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let bib = papers_library(dir.path());
        assert_eq!(papers_folder(&bib).as_deref(), Some(dir.path()));
        // another .bib of the folder
        let other = dir.path().join("other.bib");
        fs::write(&other, "").unwrap();
        assert_eq!(papers_folder(&other), None);
        // the .bib named by its absolute path
        let config = format!(r#"{{"bibtex": {:?}}}"#, other.to_str().unwrap());
        fs::write(dir.path().join(".papers/config.json"), config).unwrap();
        assert_eq!(papers_folder(&other).as_deref(), Some(dir.path()));
        assert_eq!(papers_folder(&bib), None);
    }

    #[test]
    fn native_writes_undo_and_redo() {
        let dir = tempfile::tempdir().unwrap();
        let bib = dir.path().join("refs.bib");
        fs::write(&bib, ENTRIES).unwrap();
        let state = state(
            dir.path(),
            std::slice::from_ref(&bib),
            WriterChoice::BuiltIn,
            "",
        );
        let writers = state.writers.writers(std::slice::from_ref(&bib));
        assert_eq!(writers[0].kind, Kind::Native);

        tag(
            &state,
            &strings(&["a", "b"]),
            &strings(&["new"]),
            &strings(&["amoc"]),
        )
        .unwrap();
        let tagged = fs::read_to_string(&bib).unwrap();
        assert!(tagged.contains("keywords = {ice, new}"), "{tagged}");
        assert!(
            tagged.contains(" title = {B},\n keywords = {new}\n}"),
            "{tagged}"
        );
        reload(&state);
        rename(&state, "NEW", "fresh").unwrap();
        let renamed = fs::read_to_string(&bib).unwrap();
        assert!(renamed.contains("keywords = {ice, fresh}"), "{renamed}");
        reload(&state);

        step(&state, &bib, true).unwrap();
        assert_eq!(fs::read_to_string(&bib).unwrap(), tagged);
        step(&state, &bib, true).unwrap();
        assert_eq!(fs::read_to_string(&bib).unwrap(), ENTRIES);
        let nothing = step(&state, &bib, true).unwrap_err();
        assert_eq!(nothing.message, "nothing to undo");
        step(&state, &bib, false).unwrap();
        assert_eq!(fs::read_to_string(&bib).unwrap(), tagged);

        // not over a change made elsewhere
        fs::write(&bib, ENTRIES).unwrap();
        let changed = step(&state, &bib, false).unwrap_err();
        assert_eq!(changed.kind, ErrorKind::Changed);
        assert_eq!(fs::read_to_string(&bib).unwrap(), ENTRIES);
    }

    #[test]
    fn native_renames_a_subtree() {
        let dir = tempfile::tempdir().unwrap();
        let bib = dir.path().join("refs.bib");
        let entry = |key: &str, tags: &str| format!("@article{{{key},\n keywords = {{{tags}}}\n}}");
        let entries = [
            entry("a", "Climate/AMOC, climate, ice"),
            entry("b", "climate/amoc/tipping, ocean"),
            entry("c", "climate-review"),
        ];
        fs::write(&bib, entries.join("\n\n") + "\n").unwrap();
        let state = state(
            dir.path(),
            std::slice::from_ref(&bib),
            WriterChoice::BuiltIn,
            "",
        );
        let keywords = |key: &str| {
            let text = fs::read_to_string(&bib).unwrap();
            let start = text.find(&format!("@article{{{key},")).unwrap();
            let line = text[start..].lines().nth(1).unwrap().to_string();
            line.trim().to_string()
        };

        // the tag, and the tags under it, the rest as written; not "climate-review"
        rename(&state, "CLIMATE", "earth").unwrap();
        assert_eq!(keywords("a"), "keywords = {earth/AMOC, earth, ice}");
        assert_eq!(keywords("b"), "keywords = {earth/amoc/tipping, ocean}");
        assert_eq!(keywords("c"), "keywords = {climate-review}");
        reload(&state);
        // a rename that makes a tag the item has already
        rename(&state, "earth/amoc", "earth").unwrap();
        assert_eq!(keywords("a"), "keywords = {earth, ice}");
        assert_eq!(keywords("b"), "keywords = {earth/tipping, ocean}");
        // not a tag that only starts like the one renamed
        reload(&state);
        rename(&state, "earth/tip", "sea").unwrap();
        assert_eq!(keywords("b"), "keywords = {earth/tipping, ocean}");
    }

    #[test]
    fn papers_library_without_papers_is_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let bib = papers_library(dir.path());
        let state = state(
            dir.path(),
            std::slice::from_ref(&bib),
            WriterChoice::Command,
            "/nonexistent/papers",
        );
        let writers = state.writers.writers(std::slice::from_ref(&bib));
        assert_eq!(writers[0].kind, Kind::PapersMissing);

        let error = tag(&state, &strings(&["a"]), &strings(&["x"]), &[]).unwrap_err();
        assert_eq!(error.kind, ErrorKind::PapersMissing);
        let error = remove(&state, &strings(&["a"])).unwrap_err();
        assert_eq!(error.kind, ErrorKind::PapersMissing);
        let error = step(&state, &bib, true).unwrap_err();
        assert_eq!(error.kind, ErrorKind::PapersMissing);
        let error = replace(&state, "a", "@article{a, title = {A2}}").unwrap_err();
        assert_eq!(error.kind, ErrorKind::PapersMissing);
        assert_eq!(fs::read_to_string(&bib).unwrap(), ENTRIES);

        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["kind"], "papers-missing");
        let missing = serde_json::to_value(WriteError::missing(Some("2.5"))).unwrap();
        assert_eq!(missing["kind"], "papers-missing");
    }

    /// Every command on the papers library of `bib`, which starts empty, and
    /// what each leaves in the .bib file.
    fn every_command(state: &AppState, bib: &Path) {
        let text = |bib: &Path| fs::read_to_string(bib).unwrap();
        let added = add(state, bib, ENTRIES, AddOptions::default()).unwrap();
        assert_eq!(added.added, ["a", "b"]);
        reload(state);
        // the same paper (by papers' rule: its DOI), or another with a key taken
        let c = "@article{c,\n doi = {10.1/c},\n title = {C}\n}";
        assert_eq!(
            add(state, bib, c, AddOptions::default()).unwrap().added,
            ["c"]
        );
        reload(state);
        assert_eq!(
            add(state, bib, c, AddOptions::default()).unwrap().existing,
            ["c"]
        );
        reload(state);
        let other = "@article{b,\n title = {Other}\n}";
        let error = add(state, bib, other, AddOptions::default()).unwrap_err();
        assert!(
            error.message.contains("already in the library"),
            "{}",
            error.message
        );
        // the same paper with other values: left as it is, filled in, updated
        let c2 = "@article{c,\n doi = {10.1/c},\n journal = {J},\n title = {C2}\n}";
        let merged = |on_duplicate| AddOptions {
            on_duplicate,
            update_key: false,
        };
        // (papers 3.0.0b5 reports no line for a skipped entry)
        add(state, bib, c2, merged(OnDuplicate::Skip)).unwrap();
        reload(state);
        assert!(!text(bib).contains("{J}"), "{}", text(bib));
        assert_eq!(
            add(state, bib, c2, merged(OnDuplicate::Keep))
                .unwrap()
                .existing,
            ["c"]
        );
        reload(state);
        assert!(
            text(bib).contains("journal = {J}") && text(bib).contains("title = {C}"),
            "{}",
            text(bib)
        );
        add(state, bib, c2, merged(OnDuplicate::Update)).unwrap();
        reload(state);
        assert!(text(bib).contains("title = {C2}"), "{}", text(bib));
        // a new entry keyed by the library's template
        let fetched = "@article{FETCHED_2020,\n author = {Smith, Jane},\n doi = {10.1/d},\n title = {D},\n year = {2020}\n}";
        let keyed = AddOptions {
            update_key: true,
            ..AddOptions::default()
        };
        assert_eq!(
            add(state, bib, fetched, keyed).unwrap().added,
            ["smith2020"]
        );
        reload(state);

        tag(state, &strings(&["a", "b"]), &strings(&["new"]), &[]).unwrap();
        reload(state);
        assert!(text(bib).contains("keywords = {new}"), "{}", text(bib));
        // a tag renamed with the tags under it, one papers rename of each
        tag(state, &strings(&["a"]), &strings(&["climate/amoc"]), &[]).unwrap();
        reload(state);
        let deep = strings(&["climate/amoc/tipping"]);
        tag(state, &strings(&["b"]), &deep, &[]).unwrap();
        reload(state);
        rename(state, "Climate", "ocean").unwrap();
        reload(state);
        let renamed = text(bib);
        assert!(renamed.contains("ocean/amoc"), "{renamed}");
        assert!(renamed.contains("ocean/amoc/tipping"), "{renamed}");
        assert!(!renamed.contains("climate"), "{renamed}");
        let a = state.library.read().unwrap().raw("a").unwrap().to_string();
        replace(state, "a", &a.replace("{A}", "{A2}")).unwrap();
        reload(state);
        assert!(text(bib).contains("title = {A2}"), "{}", text(bib));
        let error = replace(state, "a", &a.replace("{a,", "{a2,")).unwrap_err();
        assert!(error.message.contains("key"), "{}", error.message);
        assert_eq!(error.kind, ErrorKind::Kept);
        assert_eq!(serde_json::to_value(&error).unwrap()["what"], "key");
        let tags = replace(state, "a", &a.replace("ocean/amoc", "x")).unwrap_err();
        assert_eq!(tags.what, Some("tags"));
        // a text that does not parse
        let error = replace(state, "a", "@article{a, title = {A3}").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Failed);

        let before = text(bib);
        remove(state, &strings(&["b"])).unwrap();
        reload(state);
        let removed = text(bib);
        assert!(!removed.contains("@article{b"), "{removed}");
        step(state, bib, true).unwrap();
        assert_eq!(text(bib), before);
        reload(state);
        step(state, bib, false).unwrap();
        assert_eq!(text(bib), removed);
        reload(state);
    }

    #[test]
    fn built_in_writes_every_command() {
        let dir = tempfile::tempdir().unwrap();
        let bib = init_library(dir.path());
        let state = state(
            dir.path(),
            std::slice::from_ref(&bib),
            WriterChoice::BuiltIn,
            "",
        );
        let writers = state.writers.writers(std::slice::from_ref(&bib));
        assert_eq!(writers[0].kind, Kind::PapersRs);
        assert_eq!(writers[0].version, None);
        assert_eq!(writers[0].problem, None);
        every_command(&state, &bib);
        // the history is papers': its folder has the steps
        assert!(dir.path().join("history/projects").is_dir());
    }

    #[test]
    fn built_in_does_not_need_a_papers_command() {
        let dir = tempfile::tempdir().unwrap();
        let bib = papers_library(dir.path());
        // the setting is of the other choice, and gone
        let state = state(
            dir.path(),
            std::slice::from_ref(&bib),
            WriterChoice::BuiltIn,
            "/nonexistent/papers",
        );
        tag(&state, &strings(&["a"]), &strings(&["x"]), &[]).unwrap();
        assert!(fs::read_to_string(&bib).unwrap().contains("AMOC, ice, x"));
    }

    #[test]
    fn built_in_says_a_library_with_a_pool_is_not_supported() {
        let dir = tempfile::tempdir().unwrap();
        let pool = dir.path().join("pool");
        fs::create_dir(&pool).unwrap();
        init_library(&pool);
        let library = dir.path().join("library");
        fs::create_dir(&library).unwrap();
        let bib = init_library(&library);
        fs::write(&bib, ENTRIES).unwrap();
        let config = library.join(".papers/config.json");
        let mut js: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
        js["pool"] = json!("../pool");
        fs::write(&config, js.to_string()).unwrap();

        let state = state(
            dir.path(),
            std::slice::from_ref(&bib),
            WriterChoice::BuiltIn,
            "",
        );
        let writers = state.writers.writers(std::slice::from_ref(&bib));
        assert_eq!(writers[0].kind, Kind::PapersRs);
        let problem = writers[0].problem.as_deref().unwrap();
        assert!(
            problem.contains("pool") && problem.contains("Settings"),
            "{problem}"
        );
        let error = tag(&state, &strings(&["a"]), &strings(&["x"]), &[]).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Failed);
        assert!(
            error.message.contains("papers command in Settings"),
            "{}",
            error.message
        );
        assert_eq!(fs::read_to_string(&bib).unwrap(), ENTRIES);
    }

    /// The papers command of `PAPERS` (else the login shell's), as a script
    /// with a home of its own, whose global configuration puts the history in
    /// `dir/history`; the same global configuration, for papers-rs.
    fn papers_command(dir: &Path) -> (String, MachineConfig) {
        use std::os::unix::fs::PermissionsExt;

        let papers = std::env::var("PAPERS").unwrap_or_else(|_| "papers".into());
        let home = dir.join("home");
        let config = if cfg!(target_os = "macos") {
            home.join("Library/Application Support/papers/config.json")
        } else {
            home.join(".config/papers/config.json")
        };
        let mut machine = MachineConfig::load(Some(config.to_str().unwrap())).unwrap();
        machine.set(
            "history-dir",
            Some(json!(dir.join("history").to_str().unwrap())),
        );
        machine.save().unwrap();
        let script = dir.join("papers");
        let text = format!(
            "#!/bin/sh\nexport HOME='{}'\nunset XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME\nexec '{papers}' \"$@\"\n",
            home.display()
        );
        fs::write(&script, text).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        (script.to_str().unwrap().to_string(), machine)
    }

    /// With the papers of the setting `PAPERS`, else the login shell's:
    /// `cargo test -- --ignored`.
    #[test]
    #[ignore = "runs papers 3.0.0b4 or later"]
    fn command_writes_every_command() {
        let dir = tempfile::tempdir().unwrap();
        let (papers, _) = papers_command(dir.path());
        let library = dir.path().join("library");
        fs::create_dir(&library).unwrap();
        let bib = init_library(&library);
        let state = state(
            dir.path(),
            std::slice::from_ref(&bib),
            WriterChoice::Command,
            &papers,
        );
        let writers = state.writers.writers(std::slice::from_ref(&bib));
        assert_eq!(writers[0].kind, Kind::Papers);
        assert!(writers[0].version.is_some());
        every_command(&state, &bib);
    }

    /// The .bib file and the configuration of a library, the history identifier apart.
    fn files_of(library: &Path) -> (String, Value) {
        let bib = fs::read_to_string(library.join("papers.bib")).unwrap();
        let config = fs::read_to_string(library.join(".papers/config.json")).unwrap();
        let mut config: Value = serde_json::from_str(&config).unwrap();
        config.as_object_mut().unwrap().remove("id");
        (bib, config)
    }

    /// The two writers do the same on two copies of a library: the same files,
    /// and a step of one is undone and redone by the other.
    #[test]
    #[ignore = "runs papers 3.0.0b4 or later"]
    fn built_in_and_command_write_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let (papers, machine) = papers_command(dir.path());
        let [built, command] = ["built-in", "command"].map(|name| {
            let library = dir.path().join(name);
            fs::create_dir(&library).unwrap();
            let bib = init_library(&library);
            (library, bib)
        });
        let one = |bib: &PathBuf, choice, papers: &str| {
            state_with(
                dir.path(),
                std::slice::from_ref(bib),
                choice,
                papers,
                machine.clone(),
            )
        };
        let in_process = one(&built.1, WriterChoice::BuiltIn, "");
        let by_command = one(&command.1, WriterChoice::Command, &papers);
        every_command(&in_process, &built.1);
        every_command(&by_command, &command.1);
        let (built_bib, built_config) = files_of(&built.0);
        let (command_bib, command_config) = files_of(&command.0);
        assert_eq!(built_bib, command_bib);
        assert_eq!(built_config, command_config);

        // a step written by one, undone and redone by the other, both ways
        let bib = &built.1;
        let by_papers = one(bib, WriterChoice::Command, &papers);
        let text = || fs::read_to_string(bib).unwrap();
        for (writes, undoes) in [(&in_process, &by_papers), (&by_papers, &in_process)] {
            reload(writes);
            let before = text();
            tag(writes, &strings(&["a"]), &strings(&["crossed"]), &[]).unwrap();
            let after = text();
            assert_ne!(after, before);
            reload(undoes);
            step(undoes, bib, true).unwrap();
            assert_eq!(text(), before);
            reload(writes);
            step(writes, bib, false).unwrap();
            assert_eq!(text(), after);
            reload(undoes);
            step(undoes, bib, true).unwrap();
            assert_eq!(text(), before);
        }
    }
}
