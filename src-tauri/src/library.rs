//! The .bib files of the library, made and brought in: a new papers library in a folder, a
//! papers library there opened, and the entries of a file imported into a .bib. The welcome
//! window does the first two at the first start, the app any time after.
//!
//! A new library is made as `papers init` makes it (papers-rs `resolve_install` and
//! `apply_install`, the checks of `papers init` before): self-contained, its PDFs in a folder of
//! its own, or shallow (`papers init --shallow`), its pool the main library, papers' default
//! library, whose PDFs it shares. A library made or opened joins the .bib files of the settings;
//! a self-contained one is the papers folder of the settings when it has none, and papers' default
//! library when papers has none.

use std::fs;
use std::path::{Path, PathBuf};

use papers_rs::bib::{Bib, Block, Entry};
use papers_rs::config::paths::{join, short_home};
use papers_rs::config::{
    CONFIG_FILE_LOCAL, Config, DEFAULT_LIBRARY, MachineConfig, NO_POOL, library_config_file,
    library_without_pool,
};
use papers_rs::entry;
use papers_rs::formats::{Format, file_format, import::loads};
use papers_rs::install::{InstallOptions, apply_install, resolve_install};
use papers_rs::library::parse_added_bibtex;
use papers_rs::names::Format as KeyFormat;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::settings::{Settings, expand, papers_library};
use crate::writer::{self, AddOptions, FilesMode, WriteError};

/// Where the PDFs of a new library go, as the page says it.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Files {
    /// a folder of the library's own (created if missing; a relative path is relative to the
    /// library): a self-contained library
    Folder { path: PathBuf },
    /// the files folder of the main library, its pool: a shallow library
    Pool,
}

/// What the welcome window offers first.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDefaults {
    /// the papers folder of the settings, else `~/Papers`
    papers_folder: PathBuf,
    /// the folder of papers' default library, the pool a new library may have; none: papers has
    /// none
    main_library: Option<PathBuf>,
}

/// What `library_import` did.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// the entries added
    added: usize,
    /// left out: the library has the paper already (papers' rule; a plain .bib: the same key)
    duplicates: usize,
    /// links to files that are not there, left out
    missing_pdfs: usize,
}

/// The global configuration of papers: the one of the tests, else the user's.
fn machine(state: &AppState) -> Result<MachineConfig, String> {
    match state.writers.machine() {
        Some(machine) => Ok(machine),
        None => MachineConfig::load(None).map_err(|e| e.message().to_string()),
    }
}

/// A change of the .bib files of the settings, on a thread of its own; then the library is read
/// again, as after the settings window saves them, and the windows told (`status`).
async fn change<T: Send + 'static>(
    app: AppHandle,
    op: impl FnOnce(&Path, MachineConfig) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let done = op(&state.settings_path, machine(&state)?)?;
        crate::load(&app);
        Ok(done)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Make a papers library in `folder` (created if missing), its .bib named `bib` (`.bib` added if
/// missing), its PDFs where `files` says; refused if the folder has a library, or the .bib, already.
/// The .bib joins the library; returns it.
#[tauri::command]
pub async fn library_create(
    app: AppHandle,
    folder: PathBuf,
    bib: String,
    files: Files,
) -> Result<PathBuf, String> {
    change(app, move |settings, machine| {
        create(settings, machine, &folder, &bib, &files)
    })
    .await
}

/// Bring the papers library of `folder` into the library: its .bib, which is returned.
#[tauri::command]
pub async fn library_open(app: AppHandle, folder: PathBuf) -> Result<PathBuf, String> {
    change(app, move |settings, machine| {
        open(settings, machine, &folder)
    })
    .await
}

/// Add the entries of `file` (BibTeX, RIS, CSL-JSON) to the .bib file `bib` of the library (see
/// `import`).
#[tauri::command]
pub async fn library_import(
    app: AppHandle,
    bib: PathBuf,
    file: PathBuf,
) -> Result<ImportReport, String> {
    writer::write(app, move |state| import(state, &bib, &file))
        .await
        .map_err(|e| e.message)
}

/// Where a new library goes, and the main library, for the welcome window.
#[tauri::command]
pub async fn library_defaults(app: AppHandle) -> Result<LibraryDefaults, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        defaults(&state.settings_path, &machine(&state)?)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The defaults of `library_defaults`, from the settings of the file `settings`.
fn defaults(settings: &Path, machine: &MachineConfig) -> Result<LibraryDefaults, String> {
    let settings = Settings::load_with(settings, Some(machine))?;
    Ok(LibraryDefaults {
        papers_folder: settings.papers_dir().unwrap_or_else(|| expand("~/Papers")),
        main_library: papers_library(Some(machine))
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf),
    })
}

/// A path for papers-rs, which takes strings.
fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The library of `library_create`, made by papers-rs as `papers init FOLDER --bibtex BIB
/// --filesdir PATH` makes it, or `papers init FOLDER --bibtex BIB --shallow`: the configuration
/// names the .bib and the files folder relative to the library (`../` first for a folder outside
/// it), and the pool as `papers init` writes it. Nothing is made when a check fails.
fn create(
    settings: &Path,
    mut machine: MachineConfig,
    folder: &Path,
    bib: &str,
    files: &Files,
) -> Result<PathBuf, String> {
    let message = |e: papers_rs::error::Error| e.message().to_string();
    let folder = expand(&text(folder));
    if !folder.is_absolute() {
        return Err(format!("{}: not an absolute path", folder.display()));
    }
    let name = match bib.trim() {
        name if name.ends_with(".bib") => name.to_string(),
        name => format!("{name}.bib"),
    };
    if name == ".bib" || name.contains('/') {
        return Err(format!("{bib}: not a name for a .bib file"));
    }
    let existing = library_config_file(&text(&folder)).map_err(message)?;
    if folder.join(".papers").exists() || existing.is_some() {
        return Err(format!(
            "{}: a papers library is there already: open it instead",
            folder.display()
        ));
    }
    let bib = folder.join(name);
    if bib.exists() {
        return Err(format!("{}: exists already", bib.display()));
    }
    let (filesdir, pool) = match files {
        Files::Folder { path } => (folder.join(expand(&text(path))), NO_POOL.to_string()),
        // (unused while the library has a pool: where `papers config pool none` copies its files)
        Files::Pool => (folder.join("files"), main_library(&machine)?),
    };
    let options = InstallOptions {
        bibtex: Some(text(&bib)),
        filesdir: Some(text(&filesdir)),
        pool: Some(pool),
        ..InstallOptions::default()
    };
    let target = join(&text(&folder), CONFIG_FILE_LOCAL);
    let mut plan =
        resolve_install(Config::new(machine.clone()), &options, &target).map_err(message)?;
    fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    apply_install(&mut plan).map_err(message)?;

    let own = matches!(files, Files::Folder { .. });
    join_library(settings, &machine, &bib, &folder, own)?;
    if own && machine.default_library().is_none() {
        // as `papers init FOLDER --global`
        machine.set(
            DEFAULT_LIBRARY,
            Some(Value::String(short_home(&text(&folder)))),
        );
        machine.save().map_err(message)?;
    }
    Ok(bib)
}

/// The pool of a shallow library: the main library, papers' default one, which has no pool
/// (`papers init --shallow`).
fn main_library(machine: &MachineConfig) -> Result<String, String> {
    let Some(main) = machine.default_library_path() else {
        return Err("No main library to share: papers has no default library".into());
    };
    library_without_pool(&main, machine, "The main library")
        .map_err(|e| e.message().to_string())?;
    Ok(main)
}

/// The library of `library_open`: the .bib its configuration names.
fn open(settings: &Path, machine: MachineConfig, folder: &Path) -> Result<PathBuf, String> {
    let message = |e: papers_rs::error::Error| e.message().to_string();
    let folder = expand(&text(folder));
    let Some(file) = library_config_file(&text(&folder)).map_err(message)? else {
        return Err(format!(
            "{}: not a papers library (no {CONFIG_FILE_LOCAL})",
            folder.display()
        ));
    };
    let config = Config::load(&file, machine.clone(), None).map_err(message)?;
    let Some(bib) = config.bibtex.as_deref().filter(|b| !b.is_empty()) else {
        return Err(format!(
            "{}: its library has no .bib file",
            folder.display()
        ));
    };
    let bib = PathBuf::from(bib);
    if !bib.is_file() {
        return Err(format!("{}: not found", bib.display()));
    }
    join_library(settings, &machine, &bib, &folder, !config.has_pool())?;
    Ok(bib)
}

/// The .bib of a papers library in `folder` among those of the settings; the folder the papers
/// folder of the settings if they have none and the library is self-contained (`own`).
fn join_library(
    path: &Path,
    machine: &MachineConfig,
    bib: &Path,
    folder: &Path,
    own: bool,
) -> Result<(), String> {
    let mut settings = Settings::load_with(path, Some(machine))?;
    if !settings.bibs().iter().any(|b| b == bib) {
        settings.bibtex.push(bib.to_path_buf());
    }
    if own && settings.papers_folder.is_none() {
        settings.papers_folder = Some(folder.to_path_buf());
    }
    settings.save(path)
}

/// The entries of a file to import, as BibTeX to add, and the links to files left out.
struct Imported {
    text: String,
    entries: usize,
    missing: usize,
}

/// The format of a file to import: by its extension, as `papers add` takes it, else by its text
/// (JSON: CSL-JSON; a first tag `TY  -`: RIS; else BibTeX).
fn format_of(file: &Path, text: &str) -> Format {
    let name = file
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if [".bib", ".ris", ".json"].iter().any(|e| name.ends_with(e)) {
        return file_format(&name, None);
    }
    match text.trim_start() {
        t if t.starts_with('[') || t.starts_with('{') => Format::CslJson,
        t if t.starts_with("TY  -") => Format::Ris,
        _ => Format::Bibtex,
    }
}

/// The entries of `file`, read by papers-rs: their links to files that are there made absolute
/// (those of the file are relative to its folder, as `papers add` reads them), the others left
/// out; an entry without key given one by papers' key template.
fn read_entries(file: &Path) -> Result<Imported, String> {
    let shown = file.display();
    let raw = papers_rs::bib::read_file(file).map_err(|e| format!("{shown}: {e}"))?;
    let mut entries: Vec<Entry> = match format_of(file, &raw) {
        Format::Bibtex => parse_added_bibtex(&raw).entries().cloned().collect(),
        format => loads(&raw, format).map_err(|e| format!("{shown}: {e}"))?,
    };
    if entries.is_empty() {
        return Err(format!("{shown}: no entry found"));
    }
    let folder = file.parent().map(text).unwrap_or_default();
    let mut missing = 0;
    let mut taken: Vec<String> = entries.iter().map(|e| e.key.clone()).collect();
    for entry in &mut entries {
        let (there, gone): (Vec<String>, Vec<String>) = entry::files(entry, Some(&folder))
            .into_iter()
            .partition(|f| Path::new(f).is_file());
        missing += gone.len();
        let there: Vec<String> = there
            .into_iter()
            .map(|f| fs::canonicalize(&f).map(|p| text(&p)).unwrap_or(f))
            .collect();
        entry::set_files(entry, &there, "/");
        if entry.key.is_empty() {
            entry.key = writer::made_key(&KeyFormat::key(), entry, &taken)?;
            taken.push(entry.key.clone());
        }
    }
    let count = entries.len();
    let bib = Bib {
        blocks: entries.into_iter().map(Block::Entry).collect(),
    };
    Ok(Imported {
        text: bib.format(),
        entries: count,
        missing,
    })
}

/// Add the entries of `file` to the .bib file `bib` by its writer, as `add_bibtex` adds them: a
/// papers library by papers (`papers add --copy --update-key`, on duplicate skip): the keys made
/// by its key template, the PDFs the entries link copied into its files folder and named by its
/// template; a plain .bib by the app, the PDFs linked where they are, an entry whose key the .bib
/// has left out.
fn import(state: &AppState, bib: &Path, file: &Path) -> Result<ImportReport, WriteError> {
    let imported = read_entries(file).map_err(WriteError::failed)?;
    let options = AddOptions {
        update_key: true,
        files: Some(FilesMode::Copy),
        ..AddOptions::default()
    };
    let added = writer::add(state, bib, &imported.text, options)?
        .added
        .len();
    Ok(ImportReport {
        added,
        duplicates: imported.entries.saturating_sub(added),
        missing_pdfs: imported.missing,
    })
}

#[cfg(test)]
mod tests {
    use papers_rs::config::find_config;
    use serde_json::json;

    use super::*;
    use crate::settings::WriterChoice;
    use crate::writer::tests::{fixture_pdf, reload, state_with};

    /// The global configuration of papers for the tests, in a file of `dir`: the history in a
    /// folder of its own, and the default library `default`.
    fn machine(dir: &Path, default: Option<&Path>) -> MachineConfig {
        let file = dir.join("papers-config.json");
        let mut data = json!({"history-dir": text(&dir.join("history"))});
        if let Some(default) = default {
            data["default"] = json!(text(default));
        }
        fs::write(&file, data.to_string()).unwrap();
        MachineConfig::load(Some(&text(&file))).unwrap()
    }

    /// The default library that the global configuration of papers in `dir` names.
    fn default_of(dir: &Path) -> Option<String> {
        machine_file(dir).default_library()
    }

    fn machine_file(dir: &Path) -> MachineConfig {
        MachineConfig::load(Some(&text(&dir.join("papers-config.json")))).unwrap()
    }

    /// The configuration of the library of a folder, as papers finds it.
    fn config_of(folder: &Path) -> Config {
        find_config(&text(folder), MachineConfig::empty()).unwrap()
    }

    fn real(path: &Path) -> String {
        text(&fs::canonicalize(path).unwrap())
    }

    #[test]
    fn a_library_with_its_files_folder_inside_or_outside() {
        let dir = tempfile::tempdir().unwrap();
        let dir = fs::canonicalize(dir.path()).unwrap();
        let settings = dir.join("settings.json");
        let papers = dir.join("Papers");
        let files = Files::Folder {
            path: "pdfs".into(),
        };
        let bib = create(&settings, machine(&dir, None), &papers, "papers", &files).unwrap();
        assert_eq!(bib, papers.join("papers.bib"));
        assert_eq!(fs::read_to_string(&bib).unwrap(), "");
        assert!(papers.join("pdfs").is_dir());
        // as papers writes it, and reads it
        let js: Value =
            serde_json::from_str(&fs::read_to_string(papers.join(CONFIG_FILE_LOCAL)).unwrap())
                .unwrap();
        assert_eq!(
            (&js["bibtex"], &js["filesdir"]),
            (&json!("papers.bib"), &json!("pdfs"))
        );
        assert_eq!(js["pool"], "none");
        let config = config_of(&papers);
        assert!(config.is_library && !config.has_pool());
        assert_eq!(
            config.filesdir.as_deref(),
            Some(real(&papers.join("pdfs")).as_str())
        );
        // the .bib in the library, its folder the papers folder, the default library of papers
        let read = Settings::load_with(&settings, Some(&machine_file(&dir))).unwrap();
        assert_eq!(read.bibs(), std::slice::from_ref(&bib));
        assert_eq!(read.papers_dir(), Some(papers.clone()));
        assert_eq!(default_of(&dir), Some(text(&papers)));

        // a second library, its files outside it: relative to it still, as papers writes it; the
        // papers folder and the default library stay
        let other = dir.join("work/refs");
        let pdfs = dir.join("store/pdfs");
        let files = Files::Folder { path: pdfs.clone() };
        let second = create(&settings, machine_file(&dir), &other, "refs.bib", &files).unwrap();
        assert!(pdfs.is_dir());
        let js: Value =
            serde_json::from_str(&fs::read_to_string(other.join(CONFIG_FILE_LOCAL)).unwrap())
                .unwrap();
        assert_eq!(js["filesdir"], "../../store/pdfs");
        assert_eq!(
            config_of(&other).filesdir.as_deref(),
            Some(real(&pdfs).as_str())
        );
        let read = Settings::load_with(&settings, Some(&machine_file(&dir))).unwrap();
        assert_eq!(read.bibs(), [bib, second]);
        assert_eq!(read.papers_dir(), Some(papers.clone()));
        assert_eq!(default_of(&dir), Some(text(&papers)));
    }

    #[test]
    fn a_shallow_library_shares_the_files_of_the_main_library() {
        let dir = tempfile::tempdir().unwrap();
        let dir = fs::canonicalize(dir.path()).unwrap();
        let settings = dir.join("settings.json");
        let shallow = dir.join("project");
        // no main library: nothing made
        let error =
            create(&settings, machine(&dir, None), &shallow, "p", &Files::Pool).unwrap_err();
        assert!(error.contains("no default library"), "{error}");
        assert!(!shallow.exists());

        let papers = dir.join("Papers");
        let files = Files::Folder {
            path: "files".into(),
        };
        let main = create(
            &dir.join("main.json"),
            machine(&dir, None),
            &papers,
            "papers",
            &files,
        )
        .unwrap();
        let bib = create(&settings, machine_file(&dir), &shallow, "p", &Files::Pool).unwrap();
        assert_eq!(bib, shallow.join("p.bib"));
        let config = config_of(&shallow);
        assert!(config.shallow());
        assert_eq!(config.pool_path(), Some(text(&papers)));
        // no files folder of its own
        assert!(!shallow.join("files").exists());
        // its .bib in the library (and the main one, papers' default: a first run), the papers
        // folder the main library's
        let read = Settings::load_with(&settings, Some(&machine_file(&dir))).unwrap();
        assert_eq!(read.bibs(), [main, bib.clone()]);
        assert_eq!(read.papers_dir(), Some(papers.clone()));
        assert_eq!(default_of(&dir), Some(text(&papers)));

        // an entry added goes to the pool, its PDF to the pool's files folder
        let state = state_with(
            &dir,
            std::slice::from_ref(&bib),
            WriterChoice::BuiltIn,
            "",
            machine_file(&dir),
        );
        let pdf = fixture_pdf(&dir.join("in/x.pdf"), "no-doi.pdf");
        let entry = "@article{x,\n author = {Doe, Jane},\n title = {X},\n year = {2020},\n file = {x.pdf}\n}\n";
        let file = dir.join("in/x.bib");
        fs::write(&file, entry).unwrap();
        let report = import(&state, &bib, &file).unwrap();
        assert_eq!(report.added, 1);
        assert!(
            fs::read_to_string(papers.join("papers.bib"))
                .unwrap()
                .contains("doe2020")
        );
        assert_eq!(fs::read_dir(papers.join("files")).unwrap().count(), 1);
        assert!(pdf.is_file());
    }

    #[test]
    fn no_library_over_another_nor_over_a_bib() {
        let dir = tempfile::tempdir().unwrap();
        let dir = fs::canonicalize(dir.path()).unwrap();
        let settings = dir.join("settings.json");
        let papers = dir.join("Papers");
        let files = Files::Folder {
            path: "files".into(),
        };
        create(&settings, machine(&dir, None), &papers, "papers", &files).unwrap();
        let again = create(&settings, machine_file(&dir), &papers, "other", &files).unwrap_err();
        assert!(again.contains("already"), "{again}");
        assert!(!papers.join("other.bib").exists());

        let plain = dir.join("plain");
        fs::create_dir(&plain).unwrap();
        fs::write(plain.join("refs.bib"), "").unwrap();
        let over = create(&settings, machine_file(&dir), &plain, "refs.bib", &files).unwrap_err();
        assert!(over.contains("exists already"), "{over}");
        assert!(!plain.join(".papers").exists());
        for name in ["", "a/b"] {
            assert!(create(&settings, machine_file(&dir), &plain, name, &files).is_err());
        }
    }

    #[test]
    fn a_library_opened_joins_the_settings() {
        let dir = tempfile::tempdir().unwrap();
        let dir = fs::canonicalize(dir.path()).unwrap();
        let papers = dir.join("Papers");
        let files = Files::Folder {
            path: "files".into(),
        };
        // made elsewhere (a terminal, another computer); papers has no default library here
        let elsewhere = dir.join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        let made = machine(&elsewhere, None);
        let bib = create(
            &elsewhere.join("settings.json"),
            made,
            &papers,
            "papers",
            &files,
        )
        .unwrap();
        let settings = dir.join("settings.json");
        assert_eq!(open(&settings, machine(&dir, None), &papers).unwrap(), bib);
        let read = Settings::load_with(&settings, Some(&machine_file(&dir))).unwrap();
        assert_eq!(read.bibs(), std::slice::from_ref(&bib));
        assert_eq!(read.papers_dir(), Some(papers.clone()));
        // once
        open(&settings, machine_file(&dir), &papers).unwrap();
        let read = Settings::load_with(&settings, Some(&machine_file(&dir))).unwrap();
        assert_eq!(read.bibs(), [bib]);

        let error = open(&settings, machine_file(&dir), &dir).unwrap_err();
        assert!(error.contains("not a papers library"), "{error}");
    }

    #[test]
    fn defaults_for_the_welcome_window() {
        let dir = tempfile::tempdir().unwrap();
        let dir = fs::canonicalize(dir.path()).unwrap();
        let settings = dir.join("settings.json");
        let none = defaults(&settings, &machine(&dir, None)).unwrap();
        assert_eq!(none.papers_folder, expand("~/Papers"));
        assert_eq!(none.main_library, None);
        let papers = dir.join("Papers");
        let files = Files::Folder {
            path: "files".into(),
        };
        create(&settings, machine_file(&dir), &papers, "papers", &files).unwrap();
        let made = defaults(&settings, &machine_file(&dir)).unwrap();
        assert_eq!(made.papers_folder, papers);
        assert_eq!(made.main_library, Some(papers));
    }

    const IMPORTED: &str = "@article{one,\n author = {Doe, Jane},\n title = {One},\n year = {2020},\n file = {pdfs/one.pdf}\n}\n\n@article{two,\n author = {Roe, Rick},\n title = {Two},\n year = {2021},\n file = {pdfs/gone.pdf}\n}\n";

    #[test]
    fn a_bib_imported_with_its_pdfs_and_its_duplicates_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let dir = fs::canonicalize(dir.path()).unwrap();
        let papers = dir.join("Papers");
        let files = Files::Folder {
            path: "files".into(),
        };
        let bib = create(
            &dir.join("settings.json"),
            machine(&dir, None),
            &papers,
            "papers",
            &files,
        )
        .unwrap();
        let state = state_with(
            &dir,
            std::slice::from_ref(&bib),
            WriterChoice::BuiltIn,
            "",
            machine_file(&dir),
        );
        let file = dir.join("export/refs.bib");
        let pdf = fixture_pdf(&dir.join("export/pdfs/one.pdf"), "no-doi.pdf");
        fs::write(&file, IMPORTED).unwrap();
        let report = import(&state, &bib, &file).unwrap();
        let expected = ImportReport {
            added: 2,
            duplicates: 0,
            missing_pdfs: 1,
        };
        assert_eq!(report, expected);
        // the keys of the library's template, the PDF copied and named by its template
        let text = fs::read_to_string(&bib).unwrap();
        assert!(
            text.contains("@article{doe2020,") && text.contains("@article{roe2021,"),
            "{text}"
        );
        let placed: Vec<PathBuf> = fs::read_dir(papers.join("files"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(placed.len(), 1);
        assert!(
            text.contains(&*placed[0].file_name().unwrap().to_string_lossy()),
            "{text}"
        );
        assert!(pdf.is_file());
        assert!(!text.contains("gone.pdf"));
        // again: the same papers, left out
        reload(&state);
        let again = import(&state, &bib, &file).unwrap();
        assert_eq!(
            again,
            ImportReport {
                added: 0,
                duplicates: 2,
                missing_pdfs: 1
            }
        );
        assert_eq!(fs::read_dir(papers.join("files")).unwrap().count(), 1);
    }

    const RIS: &str = "TY  - JOUR\nAU  - Doe, Jane\nTI  - A study of ice\nPY  - 2019\nDO  - 10.1/ice\nER  - \n\nTY  - JOUR\nAU  - Roe, Rick\nTI  - Ocean\nPY  - 2022\nER  - \n";

    #[test]
    fn ris_imported_into_a_papers_library_and_a_plain_bib() {
        let dir = tempfile::tempdir().unwrap();
        let dir = fs::canonicalize(dir.path()).unwrap();
        let papers = dir.join("Papers");
        let files = Files::Folder {
            path: "files".into(),
        };
        let bib = create(
            &dir.join("settings.json"),
            machine(&dir, None),
            &papers,
            "papers",
            &files,
        )
        .unwrap();
        let plain = dir.join("plain.bib");
        fs::write(&plain, "").unwrap();
        let state = state_with(
            &dir,
            &[bib.clone(), plain.clone()],
            WriterChoice::BuiltIn,
            "",
            machine_file(&dir),
        );
        // by its extension, and by its text
        let file = dir.join("refs.ris");
        fs::write(&file, RIS).unwrap();
        let report = import(&state, &bib, &file).unwrap();
        assert_eq!(
            report,
            ImportReport {
                added: 2,
                duplicates: 0,
                missing_pdfs: 0
            }
        );
        let text = fs::read_to_string(&bib).unwrap();
        assert!(
            text.contains("@article{doe2019,") && text.contains("A study of ice"),
            "{text}"
        );
        let unnamed = dir.join("refs.txt");
        fs::write(&unnamed, RIS).unwrap();
        let report = import(&state, &plain, &unnamed).unwrap();
        assert_eq!(report.added, 2);
        reload(&state);
        // a plain .bib leaves out the keys it has
        let again = import(&state, &plain, &unnamed).unwrap();
        assert_eq!(
            again,
            ImportReport {
                added: 0,
                duplicates: 2,
                missing_pdfs: 0
            }
        );
        let text = fs::read_to_string(&plain).unwrap();
        assert!(
            text.contains("Ocean") && text.contains("10.1/ice"),
            "{text}"
        );
    }
}
