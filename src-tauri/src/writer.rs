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

pub(crate) use command::interactive_login_path;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use papers_core::{Added, Change, Item, same_tag, under, with_files, without_files};
use papers_rs::command::report::Changes;
use papers_rs::bib::Entry;
use papers_rs::config::MachineConfig;
use papers_rs::fetch::download::is_download;
use papers_rs::history::store::to_system_trash;
use papers_rs::names::{Format as KeyFormat, append_abc};
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
    /// why the library cannot be written, when it is known (e.g. it is not a papers library)
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
    /// the failure left a change written (the first of two): the library is read again
    #[serde(skip)]
    reload: bool,
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
            reload: false,
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
            reload: false,
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
            reload: false,
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

    /// `papers add FILE --on-duplicate ... [--attach PDF --move|--copy|--link]`: the entries of
    /// BibTeX added, with a PDF for them when the options have one
    fn add(&self, folder: &Path, text: &str, options: &AddOptions) -> Result<Changes, WriteError>;

    /// `papers add --key KEY --attach PDF --move|--copy|--link`: a PDF added to the files of an
    /// entry of the library, placed as the mode says (none: the `files` setting of the library)
    fn attach(
        &self,
        folder: &Path,
        key: &str,
        pdf: &Path,
        files: Option<FilesMode>,
    ) -> Result<Changes, WriteError>;

    /// `papers rm KEY... --keep-files`, and the `files` of these entries deleted, those that no
    /// other entry links to: kept in the history, which undo brings back. Built in, in the same
    /// step as the entries; a papers command deletes them first (`papers file rm`), a step of
    /// their own
    fn remove(&self, folder: &Path, keys: &[String], files: &[PathBuf]) -> Result<Changes, WriteError>;

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
    /// where the files that a plain .bib's removed entries leave go: the system's trash; the
    /// tests give a folder of their own
    trash: Mutex<Option<PathBuf>>,
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

    /// A file to the trash, from where the user can take it back. Never deleted for good: where
    /// there is no trash, the file stays and the error says so.
    fn trash(&self, file: &Path) -> Result<(), WriteError> {
        let failed = |e: String| WriteError::failed(format!("{}: not moved to the trash: {e}", file.display()));
        if let Some(folder) = &*self.trash.lock().unwrap() {
            let name = file.file_name().unwrap_or_default();
            return fs::rename(file, folder.join(name)).map_err(|e| failed(e.to_string()));
        }
        let ready = cfg!(target_os = "linux")
            || (cfg!(target_os = "macos") && std::env::home_dir().is_some_and(|h| h.join(".Trash").is_dir()));
        if !ready {
            return Err(failed("this system has no trash the app can use".into()));
        }
        to_system_trash(&file.to_string_lossy());
        Ok(())
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
                    reload: false,
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
struct Pasted(tempfile::NamedTempFile);

impl Pasted {
    /// The text in a new file of its own (two writes at once never share
    /// one), removed when the `Pasted` goes.
    fn new(text: &str) -> Result<Pasted, WriteError> {
        let failed = |e: std::io::Error| WriteError::failed(format!("a temporary file: {e}"));
        let mut file = tempfile::Builder::new()
            .prefix("papers-desktop-")
            .suffix(".bib")
            .tempfile()
            .map_err(failed)?;
        std::io::Write::write_all(&mut file, text.as_bytes()).map_err(failed)?;
        Ok(Pasted(file))
    }

    fn path(&self) -> &Path {
        self.0.path()
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
            Err(e) => e.kind == ErrorKind::Changed || e.reload,
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

/// Add a PDF to a paper of the library, with the changes of `text` to its entry first, if any
/// (the entry as edited: its key, tags and files stay).
#[tauri::command]
pub async fn attach_pdf(
    app: AppHandle,
    id: String,
    text: Option<String>,
    pdf: PathBuf,
    files: Option<FilesMode>,
) -> Result<(), WriteError> {
    write(app, move |state| attach(state, &id, text.as_deref(), &pdf, files)).await
}

/// What a PDF added to a paper of a .bib file is done with by default (see `default_files`).
#[tauri::command]
pub async fn default_files_mode(app: AppHandle, bib: PathBuf) -> Result<Option<FilesMode>, WriteError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        default_files(&state, &bib)
    })
    .await
    .map_err(|e| WriteError::failed(e.to_string()))?
}

/// The key the library gives a new entry (what `--update-key` makes of it when adding): its key
/// template, papers' default for a plain .bib; a key that the .bib has already gets a letter.
#[tauri::command]
pub async fn library_key(app: AppHandle, bib: PathBuf, text: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        library_key_of(&state, &bib, &text)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// How entries are added to a papers library; a plain .bib has no merge and
/// no key template: its entries are added as they are.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AddOptions {
    /// an entry that is a paper of the library already, by papers' rule
    pub on_duplicate: OnDuplicate,
    /// the keys of the new entries made by the library's key template
    /// (papers `--update-key`)
    pub update_key: bool,
    /// a PDF for the entry (one entry only): placed in a papers library as `files` says, linked
    /// where it is in a plain .bib
    pub pdf: Option<PathBuf>,
    /// what to do with the PDF in a papers library; none: the `files` setting of the library
    pub files: Option<FilesMode>,
}

/// papers `--move`, `--copy`, `--link`: what is done with a file added to an entry. A file is
/// placed in the files directory under the name the library's template makes (move and copy:
/// papers has no placing without naming), or left where it is and linked there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilesMode {
    Move,
    Copy,
    Link,
}

impl FilesMode {
    /// The option of papers
    fn flag(self) -> &'static str {
        match self {
            FilesMode::Move => "--move",
            FilesMode::Copy => "--copy",
            FilesMode::Link => "--link",
        }
    }
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

/// Remove entries from the library. Their files stay, except the `files` to delete (of the
/// entries; a file another entry links to stays too): in a papers library papers keeps them in
/// its history (undo brings them back), in a plain .bib they go to the trash of the system.
#[tauri::command]
pub async fn remove_entries(app: AppHandle, ids: Vec<String>, files: Vec<PathBuf>) -> Result<(), WriteError> {
    write(app, move |state| remove(state, &ids, &files)).await
}

/// Copy entries to another .bib file of the library, or with `remove` move them: the entries
/// are added to it by its writer, with their files (see `transfer`).
#[tauri::command]
pub async fn transfer_entries(
    app: AppHandle,
    ids: Vec<String>,
    bib: PathBuf,
    remove: bool,
) -> Result<Transferred, WriteError> {
    write(app, move |state| transfer(state, &ids, &bib, remove)).await
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

/// The PDF to add, which must be a file the writers can find from any folder: its absolute path,
/// links resolved (the link to a file left where it is is relative to the library, which papers
/// works out from real paths).
fn checked_pdf(pdf: &Path) -> Result<PathBuf, WriteError> {
    if !pdf.is_absolute() {
        return Err(WriteError::failed(format!("{}: not an absolute path", pdf.display())));
    }
    if !pdf.is_file() {
        return Err(WriteError::failed(format!("{}: not a file", pdf.display())));
    }
    fs::canonicalize(pdf).map_err(|e| WriteError::failed(format!("{}: {e}", pdf.display())))
}

/// A downloaded PDF is in a temporary folder: a plain .bib, which only links files where they
/// are, cannot keep it.
fn check_kept(pdf: &Path) -> Result<(), WriteError> {
    if is_download(&pdf.to_string_lossy()) {
        return Err(WriteError::failed(
            "A plain .bib file has no folder for a downloaded PDF: save the PDF in a folder of its own, and add that",
        ));
    }
    Ok(())
}

fn add(state: &AppState, bib: &Path, text: &str, mut options: AddOptions) -> Result<Added, WriteError> {
    in_library(state, bib)?;
    let keys = papers_core::entry_keys(text)?;
    if let Some(pdf) = &options.pdf {
        if keys.len() != 1 {
            return Err(WriteError::failed("A PDF goes with one entry"));
        }
        options.pdf = Some(checked_pdf(pdf)?);
    }
    let targets = state.writers.targets([bib])?;
    match targets.papers(bib) {
        Some(library) => {
            let report = library.writer.add(&library.folder, text, &options)?;
            // a paper of the library: as it was, or merged with the entry
            let mut existing = report.existing;
            existing.extend(report.modified);
            Ok(Added {
                added: report.added,
                existing,
            })
        }
        None => {
            // a plain .bib: the file linked where it is
            let linked;
            let text = match &options.pdf {
                Some(pdf) => {
                    check_kept(pdf)?;
                    linked = papers_core::with_files(text, std::slice::from_ref(pdf))?;
                    &linked
                }
                None => text,
            };
            let (changes, added) = state.library.read().unwrap().add_entries(bib, text)?;
            state.writers.done(changes);
            Ok(added)
        }
    }
}

/// Add a PDF to a paper of the library, which first takes the changes in `text` (the entry as
/// edited, with the key, tags and files it has); a plain .bib links the file where it is.
fn attach(
    state: &AppState,
    id: &str,
    text: Option<&str>,
    pdf: &Path,
    files: Option<FilesMode>,
) -> Result<(), WriteError> {
    let item = state.item(id).map_err(WriteError::failed)?;
    let pdf = &checked_pdf(pdf)?;
    let targets = state.writers.targets([item.bib.as_path()])?;
    let raw = state.library.read().unwrap().raw(id).map(str::to_string);
    let changed = text.filter(|t| raw.as_deref().is_none_or(|r| t.trim() != r.trim()));
    match targets.papers(&item.bib) {
        Some(library) => {
            // (the edit first: it keeps the files the entry has, and the PDF is not one yet)
            if let Some(text) = changed {
                state.library.read().unwrap().check_edit(id, text)?;
                library.writer.edit(&library.folder, &item.key, text)?;
            }
            library
                .writer
                .attach(&library.folder, &item.key, pdf, files)?;
        }
        None => {
            check_kept(pdf)?;
            let changes = state
                .library
                .read()
                .unwrap()
                .attach_files(id, changed, std::slice::from_ref(&pdf.to_path_buf()))?;
            state.writers.done(changes);
        }
    }
    Ok(())
}

/// What is done with a PDF added to a paper of this .bib by default: the `files` setting of its
/// papers library (move when it has none), link in a library without files folder; none for a
/// plain .bib, which links the file where it is.
fn default_files(state: &AppState, bib: &Path) -> Result<Option<FilesMode>, WriteError> {
    in_library(state, bib)?;
    let Some(folder) = papers_folder(bib) else {
        return Ok(None);
    };
    let machine = state.writers.machine.lock().unwrap().clone();
    Builtin::new(machine).files_mode(&folder).map(Some)
}

/// The key of an entry by a key template, with a letter after it when `taken` has it (without case).
fn made_key(format: &KeyFormat, entry: &Entry, taken: &[String]) -> Result<String, String> {
    let key = format.render(entry).map_err(|e| e.0)?;
    let lower: Vec<&str> = taken.iter().map(String::as_str).collect();
    if lower.iter().any(|k| k.to_lowercase() == key.to_lowercase()) {
        return append_abc(&key, &lower);
    }
    Ok(key)
}

/// The key the library of `bib` gives the first entry of `text`.
fn library_key_of(state: &AppState, bib: &Path, text: &str) -> Result<String, String> {
    in_library(state, bib).map_err(|e| e.message)?;
    let entry = crate::input::entry_of(text, "The BibTeX")?;
    let format = match papers_folder(bib) {
        Some(folder) => {
            let machine = state.writers.machine.lock().unwrap().clone();
            Builtin::new(machine).key_format(&folder).map_err(|e| e.message)?
        }
        None => KeyFormat::key(),
    };
    let taken: Vec<String> = {
        let library = state.library.read().unwrap();
        library.items().iter().filter(|it| it.bib == bib).map(|it| it.key.clone()).collect()
    };
    made_key(&format, &entry, &taken)
}

/// The real path of a file, which two links to it share.
fn real(file: &Path) -> PathBuf {
    fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf())
}

/// Remove the entries `ids`, and delete the `files` of them (see `remove_entries`).
fn remove(state: &AppState, ids: &[String], files: &[PathBuf]) -> Result<(), WriteError> {
    let items = items(state, ids)?;
    let targets = state
        .writers
        .targets(items.iter().map(|it| it.bib.as_path()))?;
    let wanted: BTreeSet<PathBuf> = files.iter().map(|f| real(f)).collect();
    let mut native = Vec::new();
    // by library: the keys of the entries, and the files to delete that they link to
    let mut keys: BTreeMap<&Path, (&PapersLibrary, Vec<String>, Vec<PathBuf>)> = BTreeMap::new();
    for item in &items {
        match targets.papers(&item.bib) {
            None => native.push(item.id.clone()),
            Some(library) => {
                let (_, keys, delete) = keys
                    .entry(item.bib.as_path())
                    .or_insert((library, Vec::new(), Vec::new()));
                keys.push(item.key.clone());
                delete.extend(item.files.iter().filter(|f| wanted.contains(&real(f))).cloned());
            }
        }
    }
    let changes = state.library.read().unwrap().remove_entries(&native)?;
    state.writers.done(changes);
    for (library, keys, delete) in keys.into_values() {
        library.writer.remove(&library.folder, &keys, &delete)?;
    }
    trash_files(state, &items, &native, &wanted)?;
    Ok(())
}

/// The `wanted` files (real paths) of the entries of a plain .bib removed (`native`: their ids)
/// that no other entry links to, moved to the trash. A file that is not there is left out.
fn trash_files(state: &AppState, items: &[Item], native: &[String], wanted: &BTreeSet<PathBuf>) -> Result<(), WriteError> {
    let removed: BTreeSet<&str> = native.iter().map(String::as_str).collect();
    let kept: BTreeSet<PathBuf> = {
        let library = state.library.read().unwrap();
        let others = library.items().iter().filter(|it| !removed.contains(it.id.as_str()));
        others.flat_map(|it| it.files.iter().map(|f| real(f))).collect()
    };
    let mut done = BTreeSet::new();
    for file in items.iter().filter(|it| removed.contains(it.id.as_str())).flat_map(|it| &it.files) {
        let file = real(file);
        if wanted.contains(&file) && file.is_file() && !kept.contains(&file) && done.insert(file.clone()) {
            state.writers.trash(&file)?;
        }
    }
    Ok(())
}

/// What `transfer_entries` did, by the keys of the entries.
#[derive(Debug, Default, Serialize)]
pub struct Transferred {
    /// added to the other .bib
    pub added: Vec<String>,
    /// left out: it has the paper already (the same key, or for a papers library the same paper)
    pub existing: Vec<String>,
    /// removed from their .bib files, for a move: those added
    pub removed: usize,
    /// links to files that are not there, left out
    pub missing: usize,
}

/// Copy entries to the .bib file `bib`, or with `remove_source` move them (copy, then remove
/// those that were added from their .bib files; their files stay unless they were moved).
///
/// The entries are added by the writer of `bib`: a papers library by papers (`add`, on duplicate
/// skip), which copies the files the entries link to into its files folder; a plain .bib by the
/// app, which links the files where they are. A move then removes the entries from their .bib
/// files: from a papers library with its files, when the target is a papers library too (they are
/// in one place then; papers keeps them in its history, and undo brings them back); the files of
/// a plain .bib, and those a plain .bib is the target of, stay where they are. An entry the
/// target has is not added, nor removed: the same key, or for papers the same paper
/// (`existing`). A link to a file that is not there is left out.
fn transfer(state: &AppState, ids: &[String], bib: &Path, remove_source: bool) -> Result<Transferred, WriteError> {
    in_library(state, bib)?;
    let items = items(state, ids)?;
    let sources = items.iter().map(|it| it.bib.as_path()).filter(|_| remove_source);
    let targets = state.writers.targets(sources.chain([bib]))?;
    let mut result = Transferred::default();
    // the papers to add: not those the target has, by key
    let mut taken: BTreeSet<String> = {
        let library = state.library.read().unwrap();
        library.items().iter().filter(|it| it.bib == bib).map(|it| it.key.to_lowercase()).collect()
    };
    let mut todo: Vec<&Item> = Vec::new();
    for item in &items {
        if taken.insert(item.key.to_lowercase()) {
            todo.push(item);
        } else {
            result.existing.push(item.key.clone());
        }
    }
    // the text to add of each: the entry, its links to files replaced by those that are there
    // (where they are: the writer of a papers library places them)
    let mut missing = 0;
    let mut text_of = |item: &Item| -> Result<String, WriteError> {
        let raw = state.library.read().unwrap().raw(&item.id).map(str::to_string);
        let raw = raw.ok_or_else(|| WriteError::failed(format!("{}: not in the library", item.id)))?;
        let text = without_files(&raw)?;
        let (there, gone): (Vec<&PathBuf>, Vec<&PathBuf>) = item.files.iter().partition(|f| f.is_file());
        missing += gone.len();
        let files: Vec<PathBuf> = there.into_iter().map(|f| fs::canonicalize(f).unwrap_or_else(|_| f.clone())).collect();
        Ok(if files.is_empty() { text } else { with_files(&text, &files)? })
    };
    let mut added: BTreeSet<String> = BTreeSet::new();
    match targets.papers(bib) {
        Some(library) => {
            // (papers moves no file of another papers library: copied)
            let texts = todo.iter().map(|it| text_of(it)).collect::<Result<Vec<_>, _>>()?;
            if !texts.is_empty() {
                let options = AddOptions { files: Some(FilesMode::Copy), ..AddOptions::default() };
                let report = library.writer.add(&library.folder, &texts.join("\n\n"), &options)?;
                added.extend(report.added.iter().map(|k| k.to_lowercase()));
            }
        }
        None => {
            let texts = todo.iter().map(|it| text_of(it)).collect::<Result<Vec<_>, _>>()?;
            if !texts.is_empty() {
                let (changes, report) = state.library.read().unwrap().add_entries(bib, &texts.join("\n\n"))?;
                state.writers.done(changes);
                added.extend(report.added.iter().map(|k| k.to_lowercase()));
            }
        }
    }
    result.missing = missing;
    let (new, left): (Vec<&Item>, Vec<&Item>) = todo.iter().copied().partition(|it| added.contains(&it.key.to_lowercase()));
    result.added = new.iter().map(|it| it.key.clone()).collect();
    result.existing.extend(left.iter().map(|it| it.key.clone()));
    if remove_source && !new.is_empty() {
        // the files of a papers library go with its entries when the target is one (undo of the
        // source brings them back); a plain .bib's files are the user's, and stay
        let (papers, plain): (Vec<&Item>, Vec<&Item>) = new.iter().copied().partition(|it| targets.papers(&it.bib).is_some());
        let into_papers = targets.papers(bib).is_some();
        for (group, delete_files) in [(papers, into_papers), (plain, false)] {
            if group.is_empty() {
                continue;
            }
            let ids: Vec<String> = group.iter().map(|it| it.id.clone()).collect();
            let files: Vec<PathBuf> = if delete_files { group.iter().flat_map(|it| it.files.clone()).collect() } else { Vec::new() };
            if let Err(mut e) = remove(state, &ids, &files) {
                e.message = format!("Added to {}, but not removed from the library it came from: {}", bib.display(), e.message);
                e.reload = true;
                return Err(e);
            }
            result.removed += ids.len();
        }
    }
    Ok(result)
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

    /// Texts written at the same moment, as by two writes at once, each in a
    /// file of its own (a name from the time alone was shared at times).
    #[test]
    fn pasted_texts_never_share_a_file() {
        let pasted: Vec<Pasted> = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..16)
                .map(|i| scope.spawn(move || Pasted::new(&format!("@article{{k{i},\n}}\n")).unwrap()))
                .collect();
            threads.into_iter().map(|t| t.join().unwrap()).collect()
        });
        let paths: BTreeSet<&Path> = pasted.iter().map(Pasted::path).collect();
        assert_eq!(paths.len(), 16);
        for (i, p) in pasted.iter().enumerate() {
            assert_eq!(fs::read_to_string(p.path()).unwrap(), format!("@article{{k{i},\n}}\n"));
        }
        let first = pasted[0].path().to_path_buf();
        drop(pasted);
        assert!(!first.exists());
    }

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
            text_size: Default::default(),
            preview: Default::default(),
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

    fn fixture_pdf(to: &Path, fixture: &str) -> PathBuf {
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/papers-rs/tests/fixtures/pdf");
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(fixtures.join(fixture), to).unwrap();
        to.to_path_buf()
    }

    #[test]
    fn native_links_a_pdf_where_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let bib = dir.path().join("refs.bib");
        fs::write(&bib, ENTRIES).unwrap();
        let state = state(dir.path(), std::slice::from_ref(&bib), WriterChoice::BuiltIn, "");
        let id = |key: &str| state.library.read().unwrap().find(&bib, key).unwrap().id.clone();
        let pdf = fixture_pdf(&dir.path().join("dl/x.pdf"), "no-doi.pdf");
        // (the writer links the file by its real path)
        let pdf = fs::canonicalize(pdf).unwrap();
        // a new entry with its PDF: a link to it, whatever the files mode says, and the file stays
        let options = AddOptions {
            pdf: Some(pdf.clone()),
            files: Some(FilesMode::Move),
            ..AddOptions::default()
        };
        let added = add(&state, &bib, "@article{c,\n title = {C}\n}", options).unwrap();
        assert_eq!(added.added, ["c"]);
        reload(&state);
        let link = format!("file = {{:{}:PDF}}", pdf.display());
        assert!(fs::read_to_string(&bib).unwrap().contains(&link));
        assert!(pdf.is_file());
        assert_eq!(state.library.read().unwrap().get(&id("c")).unwrap().files, std::slice::from_ref(&pdf));

        // a PDF for a paper that is there, with the changes of its entry
        let other = fs::canonicalize(fixture_pdf(&dir.path().join("dl/y.pdf"), "two-pages.pdf")).unwrap();
        let raw = state.library.read().unwrap().raw(&id("a")).unwrap().to_string();
        attach(&state, &id("a"), Some(&raw.replace("{A}", "{A2}")), &other, None).unwrap();
        reload(&state);
        let text = fs::read_to_string(&bib).unwrap();
        assert!(text.contains("title = {A2}") && text.contains(&format!("{{:{}:PDF}}", other.display())), "{text}");
        // the undo of the app takes it back as one step
        step(&state, &bib, true).unwrap();
        assert!(!fs::read_to_string(&bib).unwrap().contains("A2"));

        // a downloaded PDF is in a folder that does not stay: a plain .bib cannot keep it
        let held = Path::new(&papers_rs::fetch::download::temp_dir()).join(format!("papers_dl_test{}", std::process::id()));
        let download = fixture_pdf(&held.join("z.pdf"), "no-doi.pdf");
        let error = attach(&state, &id("b"), None, &download, None).unwrap_err();
        assert!(error.message.contains("downloaded PDF"), "{}", error.message);
        fs::remove_dir_all(held).unwrap();
    }

    #[test]
    fn a_plain_bib_has_papers_default_key_template() {
        let dir = tempfile::tempdir().unwrap();
        let bib = dir.path().join("refs.bib");
        fs::write(&bib, "@article{smith2020,\n title = {S}\n}\n").unwrap();
        let state = state(dir.path(), std::slice::from_ref(&bib), WriterChoice::BuiltIn, "");
        let entry = |author: &str| format!("@article{{10.1/x,\n author = {{{author}}},\n title = {{T}},\n year = {{2020}}\n}}");
        assert_eq!(library_key_of(&state, &bib, &entry("Brown, Ann")).unwrap(), "brown2020");
        assert_eq!(library_key_of(&state, &bib, &entry("Smith, Jane")).unwrap(), "smith2020b");
        // not a .bib of the library
        assert!(library_key_of(&state, &dir.path().join("other.bib"), &entry("Brown, Ann")).is_err());
    }

    #[test]
    fn a_papers_library_has_its_own_key_template() {
        let dir = tempfile::tempdir().unwrap();
        let bib = init_library(dir.path());
        let config = dir.path().join(".papers/config.json");
        let mut js: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
        js["keyformat"] = json!({"template": "{author}_{year}"});
        fs::write(&config, js.to_string()).unwrap();
        let state = state(dir.path(), std::slice::from_ref(&bib), WriterChoice::BuiltIn, "");
        let entry = "@article{10.1/x,\n author = {Grusdt, Fabian and Perrette, Mahe},\n title = {T},\n year = {2026}\n}";
        assert_eq!(library_key_of(&state, &bib, entry).unwrap(), "grusdt_perrette_2026");
    }

    #[test]
    fn the_default_files_mode_is_the_setting_of_the_library() {
        let dir = tempfile::tempdir().unwrap();
        let bib = init_library(dir.path());
        let state = state(dir.path(), std::slice::from_ref(&bib), WriterChoice::BuiltIn, "");
        // move, unless the library says otherwise (`ask` needs a terminal)
        assert_eq!(default_files(&state, &bib).unwrap(), Some(FilesMode::Move));
        let config = dir.path().join(".papers/config.json");
        let set = |files: &str| {
            let mut js: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
            js["files"] = json!(files);
            fs::write(&config, js.to_string()).unwrap();
        };
        for (setting, mode) in [("copy", FilesMode::Copy), ("link", FilesMode::Link), ("ask", FilesMode::Move)] {
            set(setting);
            assert_eq!(default_files(&state, &bib).unwrap(), Some(mode), "{setting}");
        }
        // a plain .bib links the file where it is
        let plain = dir.path().join("plain.bib");
        fs::write(&plain, ENTRIES).unwrap();
        let state = self::state(dir.path(), &[bib.clone(), plain.clone()], WriterChoice::BuiltIn, "");
        assert_eq!(default_files(&state, &plain).unwrap(), None);
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
        let error = remove(&state, &strings(&["a"]), &[]).unwrap_err();
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
            ..AddOptions::default()
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

        // a PDF for a new entry, copied, moved or linked, named by the library's template
        let dl = bib.parent().unwrap().join("dl");
        fs::create_dir(&dl).unwrap();
        let pdf = |name: &str, fixture: &str| {
            let path = dl.join(name);
            let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../crates/papers-rs/tests/fixtures/pdf");
            fs::copy(fixtures.join(fixture), &path).unwrap();
            path
        };
        let entry = |key: &str, author: &str, year: u32| {
            format!("@article{{{key},\n author = {{{author}}},\n title = {{T}},\n year = {{{year}}}\n}}")
        };
        let with = |path: &Path, files| AddOptions {
            pdf: Some(path.to_path_buf()),
            files: Some(files),
            update_key: true,
            ..AddOptions::default()
        };
        let placed = |name: &str| bib.parent().unwrap().join("files").join(name);
        let copied = pdf("copied.pdf", "no-doi.pdf");
        let new = entry("K1", "Brown, Ann", 2021);
        let added = add(state, bib, &new, with(&copied, FilesMode::Copy)).unwrap();
        assert_eq!(added.added, ["brown2021"]);
        reload(state);
        assert!(copied.is_file() && placed("brown_2021_t.pdf").is_file());
        assert!(text(bib).contains("file = {:files/brown_2021_t.pdf:pdf}"), "{}", text(bib));
        let moved = pdf("moved.pdf", "two-pages.pdf");
        add(state, bib, &entry("K2", "Green, Bob", 2022), with(&moved, FilesMode::Move)).unwrap();
        reload(state);
        assert!(!moved.exists() && placed("green_2022_t.pdf").is_file());
        let linked = pdf("linked.pdf", "text-doi.pdf");
        add(state, bib, &entry("K3", "White, Cy", 2023), with(&linked, FilesMode::Link)).unwrap();
        reload(state);
        assert!(linked.is_file() && !placed("white_2023_t.pdf").exists());
        assert!(text(bib).contains("file = {:dl/linked.pdf:pdf}"), "{}", text(bib));
        // the same paper as one the library has, with fields to take and a PDF: merged and attached
        let more = pdf("more.pdf", "info-doi.pdf");
        let same = "@article{c,\n doi = {10.1/c},\n volume = {9},\n title = {C2}\n}";
        let options = AddOptions {
            on_duplicate: OnDuplicate::Keep,
            ..with(&more, FilesMode::Copy)
        };
        add(state, bib, same, options).unwrap();
        reload(state);
        assert!(text(bib).contains("volume = {9}"), "{}", text(bib));
        let id = |key: &str| state.library.read().unwrap().find(bib, key).unwrap().id.clone();
        let files = |key: &str| state.library.read().unwrap().get(&id(key)).unwrap().files.clone();
        assert_eq!(files("c").len(), 1, "{}", text(bib));
        // more than one entry cannot have the PDF, and the PDF must be there
        let error = add(state, bib, ENTRIES, with(&more, FilesMode::Copy)).unwrap_err();
        assert!(error.message.contains("one entry"), "{}", error.message);
        let gone = dl.join("gone.pdf");
        let error = add(state, bib, &entry("K4", "Gray, Di", 2024), with(&gone, FilesMode::Copy)).unwrap_err();
        assert!(error.message.contains("not a file"), "{}", error.message);

        // a PDF added to a paper that is there: placed as asked, and the entry is the one edited
        let doc = pdf("attached.pdf", "real-text-doi.pdf");
        attach(state, &id("a"), None, &doc, Some(FilesMode::Copy)).unwrap();
        reload(state);
        let a = files("a");
        assert!(a.len() == 1 && a[0].is_file() && doc.is_file(), "{a:?}");
        let raw = state.library.read().unwrap().raw(&id("b")).unwrap().to_string();
        let doc = pdf("second.pdf", "real-xmp-doi.pdf");
        let edited = raw.replace("{B}", "{B2}");
        attach(state, &id("b"), Some(&edited), &doc, Some(FilesMode::Move)).unwrap();
        reload(state);
        assert!(text(bib).contains("title = {B2}") && !doc.exists(), "{}", text(bib));
        assert_eq!(files("b").len(), 1);
        // the key and tags stay: a text that changes them is refused, and the PDF is not added
        let third = pdf("third.pdf", "no-doi.pdf");
        let raw = state.library.read().unwrap().raw(&id("b")).unwrap().to_string();
        let error = attach(state, &id("b"), Some(&raw.replace("{b,", "{x,")), &third, None).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Kept);
        assert!(third.is_file());
        let error = attach(state, &id("b"), None, Path::new("relative.pdf"), None).unwrap_err();
        assert!(error.message.contains("absolute"), "{}", error.message);

        // the key the library gives a new entry: its template, a letter after it when it is taken
        let fetched = "@article{10.1/e,\n author = {Smith, Jane},\n title = {E},\n year = {2020}\n}";
        assert_eq!(library_key_of(state, bib, fetched).unwrap(), "smith2020b");
        let lee = fetched.replace("Smith, Jane", "Lee, Ann").replace("2020", "2025");
        assert_eq!(library_key_of(state, bib, &lee).unwrap(), "lee2025");
        assert!(library_key_of(state, bib, "x").is_err());

        // the same paper as "c": the fields that differ, the ones chosen written, as one edit
        // (with the PDF, as one edit and one attach)
        let same = "@article{c_new,\n doi = {10.1/c},\n title = {C2},\n journal = {Other},\n pages = {1--2}\n}";
        let library_entry = |key: &str| {
            let raw = state.library.read().unwrap().raw(&id(key)).unwrap().to_string();
            crate::input::entry_of(&raw, "c").unwrap()
        };
        let new = crate::input::entry_of(same, "new").unwrap();
        let found = crate::compare::differences(&library_entry("c"), &new);
        assert_eq!(found.iter().map(|d| d.field.as_str()).collect::<Vec<_>>(), ["journal", "pages"]);
        let merged = crate::compare::merged(&library_entry("c"), &new, &strings(&["pages"]));
        replace(state, &id("c"), &merged).unwrap();
        reload(state);
        assert!(text(bib).contains("pages = {1--2}") && text(bib).contains("journal = {J}"), "{}", text(bib));
        let merged = crate::compare::merged(&library_entry("c"), &new, &strings(&["journal"]));
        let more = pdf("merged.pdf", "real-text-doi.pdf");
        attach(state, &id("c"), Some(&merged), &more, Some(FilesMode::Copy)).unwrap();
        reload(state);
        assert!(text(bib).contains("journal = {Other}"), "{}", text(bib));
        assert_eq!(files("c").len(), 2, "{}", text(bib));

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
        remove(state, &strings(&["b"]), &[]).unwrap();
        reload(state);
        let removed = text(bib);
        assert!(!removed.contains("@article{b,"), "{removed}");
        step(state, bib, true).unwrap();
        assert_eq!(text(bib), before);
        reload(state);
        step(state, bib, false).unwrap();
        assert_eq!(text(bib), removed);
        reload(state);
        deleting(state, bib);
    }

    /// The steps of the history that a removal with files is: one built in, two by a papers command.
    fn undo_steps(state: &AppState) -> usize {
        if *state.writers.choice.lock().unwrap() == WriterChoice::Command { 2 } else { 1 }
    }

    /// An entry with two PDFs in the files folder, removed with its files kept, then with one
    /// deleted: papers keeps the file in its history, and undo brings it back. The built-in writer
    /// deletes it in the step of the entry; a papers command in a step before it.
    fn deleting(state: &AppState, bib: &Path) {
        let dir = bib.parent().unwrap();
        let text = || fs::read_to_string(bib).unwrap();
        let pdf = fixture_pdf(&dir.join("dl/reed.pdf"), "no-doi.pdf");
        let options = AddOptions {
            pdf: Some(pdf),
            files: Some(FilesMode::Copy),
            ..AddOptions::default()
        };
        let entry = "@article{reed,\n author = {Reed, Sue},\n title = {Gone},\n year = {2030}\n}";
        add(state, bib, entry, options).unwrap();
        reload(state);
        let id = state.library.read().unwrap().find(bib, "reed").unwrap().id.clone();
        let second = fixture_pdf(&dir.join("dl/reed2.pdf"), "two-pages.pdf");
        attach(state, &id, None, &second, Some(FilesMode::Copy)).unwrap();
        reload(state);
        let files = state.library.read().unwrap().find(bib, "reed").unwrap().files.clone();
        assert!(files.len() == 2 && files.iter().all(|f| f.is_file()), "{}", text());
        let [first, second] = [files[0].clone(), files[1].clone()];
        let with = text();
        let steps = undo_steps(state);

        remove(state, std::slice::from_ref(&id), &[]).unwrap();
        reload(state);
        assert!(!text().contains("@article{reed,") && first.is_file() && second.is_file());
        step(state, bib, true).unwrap();
        reload(state);
        assert_eq!(text(), with);

        // only the second is deleted: the first stays
        remove(state, std::slice::from_ref(&id), std::slice::from_ref(&second)).unwrap();
        reload(state);
        assert!(!text().contains("@article{reed,") && first.is_file() && !second.exists(), "{}", text());
        for _ in 0..steps {
            step(state, bib, true).unwrap();
            reload(state);
        }
        assert_eq!(text(), with);
        assert!(first.is_file() && second.is_file(), "undo brings the file back");

        // both
        remove(state, std::slice::from_ref(&id), &files).unwrap();
        reload(state);
        assert!(!text().contains("@article{reed,") && !first.exists() && !second.exists(), "{}", text());
        for _ in 0..steps {
            step(state, bib, true).unwrap();
            reload(state);
        }
        assert_eq!(text(), with);
        assert!(first.is_file() && second.is_file());
    }

    #[test]
    fn native_removal_keeps_files_or_trashes_those_no_other_entry_links() {
        let dir = tempfile::tempdir().unwrap();
        let bib = dir.path().join("refs.bib");
        let [one, two] = ["a.pdf", "c.pdf"].map(|n| fs::canonicalize(fixture_pdf(&dir.path().join("dl").join(n), "no-doi.pdf")).unwrap());
        let link = |p: &Path| format!("file = {{:{}:PDF}}", p.display());
        let entries = format!(
            "@article{{a,\n title = {{A}},\n {}\n}}\n\n@article{{b,\n title = {{B}},\n {}\n}}\n\n@article{{c,\n title = {{C}},\n {}\n}}\n",
            link(&one), link(&one), link(&two)
        );
        fs::write(&bib, &entries).unwrap();
        let state = state(dir.path(), std::slice::from_ref(&bib), WriterChoice::BuiltIn, "");
        let trash = dir.path().join("trash");
        fs::create_dir(&trash).unwrap();
        *state.writers.trash.lock().unwrap() = Some(trash.clone());

        // a file another entry links to stays, with the entry deleted with its files
        remove(&state, &strings(&["a"]), std::slice::from_ref(&one)).unwrap();
        reload(&state);
        assert!(one.is_file() && !fs::read_to_string(&bib).unwrap().contains("@article{a,"));
        // the files kept: nothing is trashed
        remove(&state, &strings(&["c"]), &[]).unwrap();
        reload(&state);
        assert!(two.is_file() && fs::read_dir(&trash).unwrap().count() == 0);
        // the last entry that links a file: it goes to the trash, and the entry comes back with undo
        remove(&state, &strings(&["b"]), std::slice::from_ref(&one)).unwrap();
        reload(&state);
        assert!(!one.exists() && trash.join("a.pdf").is_file());
        step(&state, &bib, true).unwrap();
        assert!(fs::read_to_string(&bib).unwrap().contains("@article{b,"));
    }

    /// Three .bib files, two papers libraries and a plain one, which start empty.
    fn three_libraries(dir: &Path) -> [PathBuf; 3] {
        let library = |name: &str| {
            let folder = dir.join(name);
            fs::create_dir(&folder).unwrap();
            init_library(&folder)
        };
        let plain = dir.join("plain.bib");
        fs::write(&plain, "").unwrap();
        [library("a"), library("b"), plain]
    }

    /// Papers copied and moved between the .bib files, with their files.
    fn transfers(state: &AppState, [a, b, plain]: &[PathBuf; 3]) {
        let text = |bib: &Path| fs::read_to_string(bib).unwrap();
        let folder = |bib: &Path| bib.parent().unwrap().to_path_buf();
        let entry = |key: &str, author: &str, year: u32| {
            format!("@article{{{key},\n author = {{{author}}},\n title = {{Of {key}}},\n year = {{{year}}},\n keywords = {{tag}}\n}}")
        };
        let id = |bib: &Path, key: &str| state.library.read().unwrap().find(bib, key).unwrap().id.clone();
        let with_pdf = |bib: &Path, key: &str, author: &str, year: u32, name: &str, fixture: &str| {
            let pdf = fixture_pdf(&folder(bib).join("dl").join(name), fixture);
            let options = AddOptions {
                pdf: Some(pdf.clone()),
                files: Some(FilesMode::Copy),
                ..AddOptions::default()
            };
            add(state, bib, &entry(key, author, year), options).unwrap();
            reload(state);
            pdf
        };
        let files = |bib: &Path, key: &str| state.library.read().unwrap().find(bib, key).unwrap().files.clone();
        // in a: x with a PDF and y without; in the plain .bib: p with a PDF where it is
        with_pdf(a, "x", "Xu, Li", 2001, "x.pdf", "no-doi.pdf");
        add(state, a, &entry("y", "Young, Bo", 2002), AddOptions::default()).unwrap();
        reload(state);
        let p_pdf = fs::canonicalize(with_pdf(plain, "p", "Park, Jo", 2003, "p.pdf", "two-pages.pdf")).unwrap();

        // a copy to a papers library: the entry as it is (its tags), the PDF copied into its files
        let ids = || vec![id(a, "x"), id(a, "y")];
        let done = transfer(state, &ids(), b, false).unwrap();
        assert_eq!((done.added.as_slice(), done.existing.len(), done.removed), (strings(&["x", "y"]).as_slice(), 0, 0));
        reload(state);
        let copied = files(b, "x");
        assert!(copied.len() == 1 && copied[0].starts_with(folder(b).join("files")) && copied[0].is_file(), "{}", text(b));
        assert!(text(b).contains("keywords = {tag}"), "{}", text(b));
        assert!(files(a, "x")[0].is_file() && text(a).contains("@article{x,"), "the source stays as it was");
        assert!(files(b, "y").is_empty());

        // again: the papers are in b already, reported and not changed
        let before = text(b);
        let done = transfer(state, &ids(), b, false).unwrap();
        assert!(done.added.is_empty() && done.existing == strings(&["x", "y"]), "{done:?}");
        reload(state);
        assert_eq!(text(b), before);

        // a copy to a plain .bib links the PDF where it is
        let done = transfer(state, &[id(a, "x")], plain, false).unwrap();
        assert_eq!(done.added, ["x"]);
        reload(state);
        assert_eq!(files(plain, "x"), [fs::canonicalize(&files(a, "x")[0]).unwrap()]);
        assert!(text(plain).contains("@article{x,") && text(plain).contains("@article{p,"));

        // a move from a plain .bib: copied into the files of b, and gone from the plain .bib; the
        // PDF it linked stays where it was
        let done = transfer(state, &[id(plain, "p")], b, true).unwrap();
        assert_eq!((done.added.as_slice(), done.removed), (strings(&["p"]).as_slice(), 1));
        reload(state);
        let there = files(b, "p");
        assert!(there.len() == 1 && there[0].starts_with(folder(b).join("files")), "{}", text(b));
        assert!(p_pdf.is_file() && !text(plain).contains("@article{p,"));

        // a move between papers libraries takes the PDF with it: copied, then deleted from the source
        with_pdf(a, "z", "Zhu, Wei", 2004, "z.pdf", "text-doi.pdf");
        let old = files(a, "z")[0].clone();
        let done = transfer(state, &[id(a, "z")], b, true).unwrap();
        assert_eq!((done.added.as_slice(), done.removed), (strings(&["z"]).as_slice(), 1));
        reload(state);
        assert!(!old.exists() && !text(a).contains("@article{z,"), "{}", text(a));
        let moved = files(b, "z");
        assert!(moved.len() == 1 && moved[0].starts_with(folder(b).join("files")) && moved[0].is_file());
        // undone in the source: the entry and its file are back (a papers command: two steps, the files'
        // and the entry's)
        for _ in 0..undo_steps(state) {
            step(state, a, true).unwrap();
            reload(state);
        }
        assert!(old.is_file() && text(a).contains("@article{z,"), "{}", text(a));

        // a move of papers the target has: they stay in the source
        let done = transfer(state, &[id(a, "x"), id(a, "y")], b, true).unwrap();
        assert!(done.added.is_empty() && done.removed == 0, "{done:?}");
        reload(state);
        assert!(text(a).contains("@article{x,") && text(a).contains("@article{y,"));
        // a link to a file that is not there is left out, and counted
        let mut broken = text(plain);
        broken.push_str(&format!("
@article{{m,
 title = {{M}},
 file = {{:{}/nowhere.pdf:PDF}}
}}
", folder(plain).display()));
        fs::write(plain, broken).unwrap();
        reload(state);
        let done = transfer(state, &[id(plain, "m")], a, false).unwrap();
        assert!(done.added == ["m"] && done.missing == 1, "{done:?}");
        reload(state);
        assert!(files(a, "m").is_empty());

        // the step of the app undoes a copy: the papers added by one transfer go together
        let undone = text(b);
        let long = entry("w", "Wu, Xin", 2005);
        add(state, plain, &long, AddOptions::default()).unwrap();
        reload(state);
        transfer(state, &[id(plain, "w")], b, false).unwrap();
        reload(state);
        assert!(text(b).contains("@article{w,"));
        step(state, b, true).unwrap();
        reload(state);
        assert_eq!(text(b), undone);
    }

    #[test]
    fn built_in_moves_and_copies_between_libraries() {
        let dir = tempfile::tempdir().unwrap();
        let bibs = three_libraries(dir.path());
        let state = state(dir.path(), &bibs, WriterChoice::BuiltIn, "");
        transfers(&state, &bibs);
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
    fn built_in_writes_a_library_with_a_pool_and_pushes_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let pool = dir.path().join("pool");
        fs::create_dir(&pool).unwrap();
        let pool_bib = init_library(&pool);
        let library = dir.path().join("library");
        fs::create_dir(&library).unwrap();
        let bib = init_library(&library);
        let config = library.join(".papers/config.json");
        let mut js: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
        js["pool"] = json!("../pool");
        js["auto-push"] = json!(true);
        fs::write(&config, js.to_string()).unwrap();

        let state = state(
            dir.path(),
            std::slice::from_ref(&bib),
            WriterChoice::BuiltIn,
            "",
        );
        // no problem is announced: a shallow library is written as the others
        let writers = state.writers.writers(std::slice::from_ref(&bib));
        assert_eq!(writers[0].kind, Kind::PapersRs);
        assert_eq!(writers[0].problem, None);
        // an entry added to the library goes to the pool too (its new entries always do)
        let text = "@article{c,\n author = {Doe, Jane},\n title = {C},\n year = {2024}\n}\n";
        let added = add(&state, &bib, text, AddOptions::default()).unwrap();
        assert_eq!(added.added, vec!["c".to_string()]);
        assert!(fs::read_to_string(&bib).unwrap().contains("@article{c,"));
        // (under the key that the key template of the pool gives)
        assert!(fs::read_to_string(&pool_bib).unwrap().contains("@article{doe2024,"));
        // the step of the pool goes with the library's: one undo takes both back
        step(&state, &bib, true).unwrap();
        assert!(!fs::read_to_string(&bib).unwrap().contains("@article{c,"));
        assert!(!fs::read_to_string(&pool_bib).unwrap().contains("@article{doe2024,"));
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

    /// With the papers of the setting `PAPERS`, else the login shell's: `cargo test -- --ignored`.
    #[test]
    #[ignore = "runs papers 3.0.0b4 or later"]
    fn command_moves_and_copies_between_libraries() {
        let dir = tempfile::tempdir().unwrap();
        let (papers, _) = papers_command(dir.path());
        let bibs = three_libraries(dir.path());
        let state = state(dir.path(), &bibs, WriterChoice::Command, &papers);
        transfers(&state, &bibs);
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
