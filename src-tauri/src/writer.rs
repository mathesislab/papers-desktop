//! The writer of each .bib file of the library. A papers library is written
//! by the papers command, so that a change from the app or a terminal has
//! the same format, naming and history; any other .bib by the app's own
//! writer, which rewrites only the entries changed. A papers library without
//! papers (3.0.0b4 or later) is read-only.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::{ErrorKind as IoErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use papers_core::{Added, Change, Item, same_tag, under};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::settings::expand;

/// The first version of papers with the options used here (`edit --from`,
/// `--porcelain`), as `release` orders it.
const PAPERS_MIN: [u32; 5] = [3, 0, 0, BETA, 4];
const PAPERS_MIN_NAME: &str = "3.0.0b4";

/// How a .bib file is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// by the papers command
    Papers,
    /// by the app
    Native,
    /// a papers library, read-only: papers is not found, or too old
    PapersMissing,
}

/// The writer of a .bib file, for the page.
#[derive(Debug, Clone, Serialize)]
pub struct Writer {
    pub bib: PathBuf,
    pub kind: Kind,
    /// of the papers command found, for a papers library
    pub version: Option<String>,
}

/// An error of a write, of a kind the page tells apart.
#[derive(Debug, Serialize)]
pub struct WriteError {
    kind: ErrorKind,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorKind {
    /// a papers library, and papers is not found or too old
    PapersMissing,
    /// the .bib changed since it was read: the library is read again
    Changed,
    Failed,
}

impl WriteError {
    fn failed(message: impl Into<String>) -> WriteError {
        WriteError {
            kind: ErrorKind::Failed,
            message: message.into(),
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
        }
    }
}

impl From<papers_core::Error> for WriteError {
    fn from(e: papers_core::Error) -> WriteError {
        let kind = match e {
            papers_core::Error::Changed { .. } => ErrorKind::Changed,
            _ => ErrorKind::Failed,
        };
        WriteError {
            kind,
            message: e.to_string(),
        }
    }
}

/// The papers command, its version, and the PATH of the login shell to run
/// it with: papers runs git (and git-lfs) from it.
#[derive(Debug, Clone)]
struct Papers {
    path: PathBuf,
    version: String,
    shell_path: Option<String>,
}

/// The papers command looked for.
enum Found {
    Usable(Papers),
    /// not found, or too old: its version
    Missing(Option<String>),
}

/// A papers library: its folder, and the papers command that writes it.
struct PapersLibrary {
    folder: PathBuf,
    papers: Papers,
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

/// The writers of the .bib files.
#[derive(Default)]
pub struct Writers {
    /// the papers command of the settings; empty: the one the login shell finds
    setting: Mutex<PathBuf>,
    /// the papers command found, with the setting it was found for
    found: Mutex<Option<(PathBuf, Papers)>>,
    /// what to undo and redo of the files the app writes, this session
    history: Mutex<HashMap<PathBuf, History>>,
    /// one write at a time: each starts from the library read after the last
    writing: Mutex<()>,
}

impl Writers {
    /// The papers command of the settings.
    pub fn set_papers(&self, command: PathBuf) {
        *self.setting.lock().unwrap() = command;
    }

    /// The papers command of the settings, else the one the login shell
    /// finds; kept once found. One not found is looked for again the next
    /// time: it may have been installed since.
    fn papers(&self) -> Found {
        let setting = self.setting.lock().unwrap().clone();
        if let Some((found_for, papers)) = &*self.found.lock().unwrap()
            && *found_for == setting
        {
            return Found::Usable(papers.clone());
        }
        let shell_path = login_path();
        let path = if setting.as_os_str().is_empty() {
            shell_path.as_deref().and_then(in_path)
        } else {
            Some(setting.clone())
        };
        let Some(path) = path else {
            return Found::Missing(None);
        };
        let Some(version) = version(&path) else {
            return Found::Missing(None);
        };
        if release(&version).is_none_or(|release| release < PAPERS_MIN) {
            return Found::Missing(Some(version));
        }
        let papers = Papers {
            path,
            version,
            shell_path,
        };
        *self.found.lock().unwrap() = Some((setting, papers.clone()));
        Found::Usable(papers)
    }

    /// The writer of each .bib file, papers looked for once.
    fn writers(&self, bibs: &[PathBuf]) -> Vec<Writer> {
        let mut found = None;
        bibs.iter()
            .map(|bib| {
                let (kind, version) = match papers_folder(bib) {
                    None => (Kind::Native, None),
                    Some(_) => match found.get_or_insert_with(|| self.papers()) {
                        Found::Usable(p) => (Kind::Papers, Some(p.version.clone())),
                        Found::Missing(v) => (Kind::PapersMissing, v.clone()),
                    },
                };
                Writer {
                    bib: bib.clone(),
                    kind,
                    version,
                }
            })
            .collect()
    }

    /// The papers libraries of the .bib files of a write, which a papers
    /// library without papers refuses.
    fn targets<'a>(&self, bibs: impl IntoIterator<Item = &'a Path>) -> Result<Targets, WriteError> {
        let bibs: BTreeSet<&Path> = bibs.into_iter().collect();
        let mut found = None;
        let mut libraries = HashMap::new();
        for bib in bibs {
            let Some(folder) = papers_folder(bib) else {
                continue;
            };
            match found.get_or_insert_with(|| self.papers()) {
                Found::Usable(papers) => {
                    let papers = papers.clone();
                    libraries.insert(bib.to_path_buf(), PapersLibrary { folder, papers });
                }
                Found::Missing(version) => return Err(WriteError::missing(version.as_deref())),
            }
        }
        Ok(Targets { libraries })
    }

    /// Run a papers command in the folder of its library, with `input` on
    /// its stdin; its report of the entries changed. A failure tells what
    /// papers wrote to stderr, without the prefix of its log.
    fn run(
        &self,
        library: &PapersLibrary,
        command: &str,
        args: &[String],
        input: Option<&str>,
    ) -> Result<Report, WriteError> {
        let mut papers = Command::new(&library.papers.path);
        papers
            .arg("-C")
            .arg(&library.folder)
            .args([command, "--porcelain"])
            .args(args)
            .env("NO_COLOR", "1")
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(path) = &library.papers.shell_path {
            papers.env("PATH", path);
        }
        let output = papers.spawn().and_then(|mut child| {
            if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
                stdin.write_all(input.as_bytes())?;
            }
            child.wait_with_output()
        });
        let output = match output {
            Ok(output) => output,
            Err(e) if e.kind() == IoErrorKind::NotFound => {
                *self.found.lock().unwrap() = None;
                return Err(WriteError::missing(None));
            }
            Err(e) => return Err(WriteError::failed(format!("papers: {e}"))),
        };
        if output.status.success() {
            return Ok(Report::of(&String::from_utf8_lossy(&output.stdout)));
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lines: Vec<&str> = stderr
            .lines()
            .map(|l| l.strip_prefix("ERROR:papers:").unwrap_or(l))
            .collect();
        let message = lines.join("\n").trim().to_string();
        Err(WriteError::failed(if message.is_empty() {
            format!("papers {command} {}: {}", args.join(" "), output.status)
        } else {
            message
        }))
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

/// The PATH of the login shell: an app started from the desktop does not
/// have it.
fn login_path() -> Option<String> {
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
    let output = Command::new(shell)
        .args(["-lc", "printf '%s' \"$PATH\""])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !path.is_empty()).then_some(path)
}

/// The papers command in the folders of a PATH.
fn in_path(path: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|folder| folder.join("papers"))
        .find(|papers| papers.is_file())
}

/// The version of a papers command, e.g. "3.0.0b1".
fn version(papers: &Path) -> Option<String> {
    let output = Command::new(papers)
        .arg("--version")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout
        .split_whitespace()
        .find(|w| w.starts_with(|c: char| c.is_ascii_digit()))?;
    Some(version.to_string())
}

const BETA: u32 = 1;

/// A version of papers as ordered: its three release numbers, its stage
/// (alpha 0, beta 1, release candidate 2, final 3) and the number of the
/// stage. A development build is the version it leads to: 3.0.0b4.dev18 is
/// 3.0.0b4.
fn release(version: &str) -> Option<[u32; 5]> {
    let end = version
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(version.len());
    let (numbers, rest) = version.split_at(end);
    let mut numbers = numbers.trim_end_matches('.').split('.');
    let mut number = || numbers.next().map_or(Some(0), |n| n.parse().ok());
    let (major, minor, patch) = (number()?, number()?, number()?);
    let (stage, rest) = [("rc", 2), ("a", 0), ("b", BETA)]
        .into_iter()
        .find_map(|(name, stage)| Some((stage, rest.strip_prefix(name)?)))
        .unwrap_or((3, ""));
    let digits = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let n = rest[..digits].parse().unwrap_or(0);
    Some([major, minor, patch, stage, n])
}

/// What a papers command reports with `--porcelain`: a line per entry,
/// `added KEY`, `modified KEY`, `removed KEY` or `existing KEY`.
struct Report {
    lines: Vec<(String, String)>,
}

impl Report {
    fn of(stdout: &str) -> Report {
        let lines = stdout
            .lines()
            .filter_map(|line| line.split_once(' '))
            .map(|(what, key)| (what.to_string(), key.trim().to_string()))
            .collect();
        Report { lines }
    }

    /// The keys reported as `what`.
    fn keys(&self, what: &str) -> Vec<String> {
        let keys = self.lines.iter().filter(|(w, _)| w == what);
        keys.map(|(_, key)| key.clone()).collect()
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
pub async fn add_bibtex(app: AppHandle, bib: PathBuf, text: String) -> Result<Added, WriteError> {
    write(app, move |state| add(state, &bib, &text)).await
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
    // a papers command for the keys of a library that change alike
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
        let mut args = keys;
        for (option, tags) in [("--add", added), ("--rm", removed)] {
            if !tags.is_empty() {
                args.push(option.to_string());
                args.extend(tags);
            }
        }
        state.writers.run(library, "tag", &args, None)?;
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
            let args = ["--rename", spelling, &to].map(String::from);
            state.writers.run(library, "tag", &args, None)?;
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
            let args = [item.key, "--from".to_string(), "-".to_string()];
            state.writers.run(library, "edit", &args, Some(text))?;
        }
        None => {
            let changes = state.library.read().unwrap().replace_entry(id, text)?;
            state.writers.done(changes);
        }
    }
    Ok(())
}

fn add(state: &AppState, bib: &Path, text: &str) -> Result<Added, WriteError> {
    in_library(state, bib)?;
    papers_core::entry_keys(text)?;
    let targets = state.writers.targets([bib])?;
    match targets.papers(bib) {
        // papers adds a .bib file: the text in a temporary one
        Some(library) => {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let name = format!("papers-desktop-{}-{nanos}.bib", std::process::id());
            let pasted = std::env::temp_dir().join(name);
            fs::write(&pasted, text)
                .map_err(|e| WriteError::failed(format!("{}: {e}", pasted.display())))?;
            let args = [pasted.to_string_lossy().into()];
            let report = state.writers.run(library, "add", &args, None);
            let _ = fs::remove_file(&pasted);
            let report = report?;
            Ok(Added {
                added: report.keys("added"),
                existing: report.keys("existing"),
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
        let mut args = keys;
        args.push("--keep-files".to_string());
        state.writers.run(library, "rm", &args, None)?;
    }
    Ok(())
}

fn step(state: &AppState, bib: &Path, back: bool) -> Result<(), WriteError> {
    in_library(state, bib)?;
    let targets = state.writers.targets([bib])?;
    match targets.papers(bib) {
        Some(library) => {
            let command = if back { "undo" } else { "redo" };
            state.writers.run(library, command, &[], None).map(|_| ())
        }
        None => state.writers.step(bib, back),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::RwLock;
    use std::sync::atomic::AtomicU64;

    use papers_core::{Library, Matching};

    use super::*;
    use crate::Status;
    use crate::main_window::Layout;
    use crate::recent::Recent;

    const ENTRIES: &str =
        "@article{a,\n title = {A},\n keywords = {AMOC, ice}\n}\n\n@article{b,\n title = {B}\n}\n";

    /// The state of the app over a library of .bib files in `dir`.
    fn state(dir: &Path, bibs: &[PathBuf], papers: &str) -> AppState {
        let writers = Writers::default();
        writers.set_papers(PathBuf::from(papers));
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

    /// A papers library in `dir`: its .bib and its configuration.
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
        assert_eq!(
            release("3.0.0b4.dev18+g6d8b3f390"),
            Some([3, 0, 0, BETA, 4])
        );
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
        let report = Report::of("added c\nexisting a\nmodified b\n\n");
        assert_eq!(report.keys("added"), ["c"]);
        assert_eq!(report.keys("existing"), ["a"]);
        assert!(report.keys("removed").is_empty());
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
        let state = state(dir.path(), std::slice::from_ref(&bib), "");
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
        let state = state(dir.path(), std::slice::from_ref(&bib), "");
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

    /// With the papers of the setting `PAPERS`, else the login shell's:
    /// `cargo test -- --ignored`.
    #[test]
    #[ignore = "runs papers 3.0.0b4 or later"]
    fn papers_library_written_by_papers() {
        let dir = tempfile::tempdir().unwrap();
        let papers = std::env::var("PAPERS").unwrap_or_default();
        let state = state(dir.path(), &[], &papers);
        let Found::Usable(found) = state.writers.papers() else {
            panic!("papers {PAPERS_MIN_NAME} or later not found");
        };
        let installed = Command::new(&found.path)
            .current_dir(dir.path())
            .args(["install", "--bibtex", "papers.bib", "--files", "files"])
            .args(["--no-prompt", "--no-git"])
            .status()
            .unwrap();
        assert!(installed.success());
        let bib = dir.path().join("papers.bib");
        *state.library.write().unwrap() = Library::load(std::slice::from_ref(&bib)).unwrap();
        let text = |bib: &Path| fs::read_to_string(bib).unwrap();

        let added = add(&state, &bib, ENTRIES).unwrap();
        assert_eq!(added.added, ["a", "b"]);
        reload(&state);
        // the same paper (by papers' rule: its DOI), or another with a key taken
        let c = "@article{c,\n doi = {10.1/c},\n title = {C}\n}";
        assert_eq!(add(&state, &bib, c).unwrap().added, ["c"]);
        reload(&state);
        assert_eq!(add(&state, &bib, c).unwrap().existing, ["c"]);
        reload(&state);
        let error = add(&state, &bib, "@article{b,\n title = {Other}\n}").unwrap_err();
        assert!(
            error.message.contains("already in the library"),
            "{}",
            error.message
        );

        tag(&state, &strings(&["a", "b"]), &strings(&["new"]), &[]).unwrap();
        reload(&state);
        assert!(text(&bib).contains("keywords = {new}"), "{}", text(&bib));
        // a tag renamed with the tags under it, one papers rename of each
        tag(&state, &strings(&["a"]), &strings(&["climate/amoc"]), &[]).unwrap();
        reload(&state);
        let deep = strings(&["climate/amoc/tipping"]);
        tag(&state, &strings(&["b"]), &deep, &[]).unwrap();
        reload(&state);
        rename(&state, "Climate", "ocean").unwrap();
        reload(&state);
        let renamed = text(&bib);
        assert!(renamed.contains("ocean/amoc"), "{renamed}");
        assert!(renamed.contains("ocean/amoc/tipping"), "{renamed}");
        assert!(!renamed.contains("climate"), "{renamed}");
        let a = state.library.read().unwrap().raw("a").unwrap().to_string();
        replace(&state, "a", &a.replace("{A}", "{A2}")).unwrap();
        reload(&state);
        assert!(text(&bib).contains("title = {A2}"), "{}", text(&bib));
        let error = replace(&state, "a", &a.replace("{a,", "{a2,")).unwrap_err();
        assert!(error.message.contains("key"), "{}", error.message);
        // a text that does not parse
        let error = replace(&state, "a", "@article{a, title = {A3}").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Failed);

        remove(&state, &strings(&["b"])).unwrap();
        reload(&state);
        assert!(!text(&bib).contains("@article{b"), "{}", text(&bib));
    }
}
