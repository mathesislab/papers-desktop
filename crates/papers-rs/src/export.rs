//! `papers export` (`papers.pools.export`, `papers.cli.pool_cmds.export_entries`): entries
//! written in a directory as a new library, to hand over (bibtex, files, configuration: a
//! self-contained library with its own files and no pool), and in the other formats
//! (`papers.ris`, `papers.json`, `papers.bib.json`), with the file links relative to the directory.
//!
//! The history: the new library has its own (its first state is empty, its second is the
//! export, recorded as a step that came from the library of the command), and when the command
//! copied files, the library of the command records them as a step that links to the new one.
//!
//! The copies of the files are done as papers does them: named with the name template of the
//! new library, a hard link where the file system allows.

use std::fs;

use crate::bib::Entry;
use crate::bib::pystr::strip;
use crate::config::paths::{abspath, basename, dirname, exists, isdir, join, realpath, relpath};
use crate::config::settings::{NO_POOL, SETTINGS};
use crate::config::{CONFIG_FILE_LOCAL, Config};
use crate::entry::{files, set_files};
use crate::error::{Error, Result};
use crate::formats::{Format, dumps};
use crate::history::{FileOp, History, OpKind, PoolLink, Step};
use crate::identity::Distinct;
use crate::install::{InstallOptions, apply_install, resolve_install};
use crate::library::{self, Action, Library, Policy};
use crate::log;
use crate::names::{Format as NameFormat, FormatKind};

/// What `--files` says to do with the files of the entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesMode {
    /// copied into `DIR/files` (a hard link when possible)
    Copy,
    /// the links point to the original files
    Link,
    /// the entries have no file links
    None,
}

impl FilesMode {
    pub fn parse(name: &str) -> Option<FilesMode> {
        match name {
            "copy" => Some(FilesMode::Copy),
            "link" => Some(FilesMode::Link),
            "none" => Some(FilesMode::None),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FilesMode::Copy => "copy",
            FilesMode::Link => "link",
            FilesMode::None => "none",
        }
    }
}

/// The names of the formats of `--format`: `bib`, `json` and `bib.json` are other forms.
fn format_named(name: &str) -> Option<Format> {
    match name {
        "bib" => Some(Format::Bibtex),
        "json" => Some(Format::CslJson),
        "bib.json" => Some(Format::BibJson),
        id => Format::from_id(id),
    }
}

/// The formats of one `--format` value: names separated by commas, `all` for all of them
/// (`_export_formats`). The error is the message of the usage error.
pub fn parse_formats(value: &str) -> std::result::Result<Vec<Format>, String> {
    let mut formats = Vec::new();
    for name in value
        .split(',')
        .map(|v| strip(v).to_lowercase())
        .filter(|v| !v.is_empty())
    {
        if name == "all" {
            formats.extend(crate::formats::FORMATS);
        } else if let Some(format) = format_named(&name) {
            formats.push(format);
        } else {
            return Err(format!(
                "unknown format {} (choose from bib, ris, csl-json, bibjson, all)",
                crate::config::json::py_repr_str(&name)
            ));
        }
    }
    Ok(formats)
}

/// What to export and where.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub dir: String,
    /// the formats written, once each, in the order of [`crate::formats::FORMATS`]
    pub formats: Vec<Format>,
    pub files: FilesMode,
    /// the command line, for the history: `papers export DIR...`
    pub message: String,
    pub dry_run: bool,
}

/// What an export did, for the history of the library of the command.
#[derive(Default)]
pub struct Exported {
    /// the file operations of the command (the copies of the files)
    pub ops: Vec<FileOp>,
    /// the step recorded in the history of the new library, to link from the step of the command
    pub link: Option<(String, PoolLink)>,
}

/// The operations of the library model as the history records them.
fn history_ops(ops: &[library::FileOp]) -> Vec<FileOp> {
    ops.iter()
        .map(|op| {
            let kind = match op.op {
                library::OpKind::Moved => OpKind::Moved,
                library::OpKind::Copied => OpKind::Copied,
            };
            FileOp::new(kind, &op.src, Some(&op.dst), op.checksum.clone())
        })
        .collect()
}

/// The name of the bibtex of an exported library: that of the library (`export_bibtex_name`).
pub fn export_bibtex_name(config: &Config) -> String {
    config
        .bibtex
        .as_deref()
        .filter(|b| !b.is_empty())
        .map(basename)
        .unwrap_or_else(|| Format::Bibtex.filename().to_string())
}

/// The new library in `dest` (`create_library`): self-contained, with its files directory.
fn create_library(dest: &str, config: &Config, bibtex: &str) -> Result<Config> {
    let options = InstallOptions {
        bibtex: Some(join(dest, bibtex)),
        filesdir: Some(join(dest, "files")),
        pool: Some(NO_POOL.to_string()),
        ..InstallOptions::default()
    };
    let mut plan = resolve_install(
        Config::new(config.machine.clone()),
        &options,
        &join(dest, CONFIG_FILE_LOCAL),
    )?;
    fs::create_dir_all(dest).map_err(|e| Error::other(format!("{dest}: {e}")))?;
    log::println(&format!("create the library {dest}"));
    apply_install(&mut plan)?;
    let file = plan
        .config
        .file
        .clone()
        .unwrap_or_else(|| join(dest, CONFIG_FILE_LOCAL));
    Config::load(&file, config.machine.clone(), None)
}

/// Write the entries in a directory (`papers export DIR [KEY...]`), as the options say; `library`
/// is the library of the command, `config` its configuration, `entries` the entries to export (in
/// the order given). Prints what papers prints; the history of the new library is recorded here, the
/// file operations and the link are left in `out` for the one of the command.
pub fn export(
    library: &Library,
    config: &Config,
    entries: &[&Entry],
    options: &ExportOptions,
    out: &mut Exported,
) -> Result<()> {
    let dir = &options.dir;
    if exists(dir)
        && (!isdir(dir)
            || fs::read_dir(dir)
                .map(|mut e| e.next().is_some())
                .unwrap_or(true))
    {
        return Err(Error::exit(format!(
            "{dir}: exists and is not an empty directory"
        )));
    }
    if options.dry_run {
        return Err(Error::exit("export does not support --dry-run"));
    }
    let dest = abspath(dir);
    let make_library = options.formats.contains(&Format::Bibtex);
    let source_files = |e: &Entry| files(e, Some(&library.relative_to));

    let mut new: Option<Config> = None;
    let mut target;
    if make_library {
        let mut created = create_library(&dest, config, &export_bibtex_name(config))?;
        for (setting, _) in SETTINGS.iter() {
            if (setting.starts_with("name.") || setting.starts_with("key."))
                && config.source(setting) == "library"
            {
                created.set(setting, config.get(setting))?;
            }
        }
        created.save()?;
        let bibtex = created.bibtex.clone().unwrap_or_default();
        if exists(&bibtex) && strip(&fs::read_to_string(&bibtex).unwrap_or_default()).is_empty() {
            // the empty bibtex of the new library: removed, so that the save of the export writes no backup of it
            let _ = fs::remove_file(&bibtex);
        }
        target = Library::new(
            &bibtex,
            created.filesdir.as_deref(),
            Some(&dirname(&bibtex)),
            NameFormat::from_config(&created, FormatKind::Name),
            NameFormat::from_config(&created, FormatKind::Key),
        );
        target.distinct = Distinct::for_library(&created.libdir());
        new = Some(created);
    } else {
        fs::create_dir_all(&dest).map_err(|e| Error::other(format!("{dest}: {e}")))?;
        target = Library::new(
            &join(&dest, Format::Bibtex.filename()),
            Some(&join(&dest, "files")),
            Some(&dest),
            NameFormat::from_config(config, FormatKind::Name),
            NameFormat::from_config(config, FormatKind::Key),
        );
    }
    for entry in entries {
        // a copy for the new library: its file links written for it, under the same key
        let mut copy = Entry::new(&entry.entry_type, &entry.key);
        copy.fields = entry
            .fields
            .iter()
            .filter(|f| f.key != "file")
            .map(|f| crate::bib::Field::new(&f.key, &f.value))
            .collect();
        let links = source_files(entry);
        if options.files != FilesMode::None && !links.is_empty() {
            set_files(&mut copy, &links, &target.relative_to);
        }
        let has_links = copy.get("file").is_some();
        let key = copy.key.clone();
        target.add_unchecked(copy);
        if options.files == FilesMode::Copy
            && has_links
            && let Err(e) =
                target.rename_entry_files(&key, &Policy::all(Action::Copy), None, None, true)
        {
            out.ops = history_ops(target.file_ops());
            return Err(e);
        }
    }
    if make_library {
        target.distinct.update(&library.distinct);
    }
    out.ops = history_ops(target.file_ops());

    let mut written: Vec<String> = Vec::new();
    if let Some(new) = new.as_mut() {
        // the history of the new library: its start, then the export
        let mut history = match History::open(new, true, false) {
            Ok(h) => h,
            Err(e) => {
                log::error(&format!(
                    "the history of the library could not record its state before the command: {e}"
                ));
                None
            }
        };
        if let Some(h) = history.as_mut()
            && let Err(e) = h.sync_head(None)
        {
            log::error(&format!(
                "the history of the library could not record its state before the command: {e}"
            ));
        }
        let saved = target.save(false)?;
        out.ops = history_ops(&saved.file_ops);
        if let Some(h) = history.as_mut() {
            let step = Step {
                message: options.message.clone(),
                from: Some(dirname(&realpath(&library.bibtex))),
                ..Step::default()
            };
            match h.record(step) {
                Ok(recorded) => out.link = recorded.link,
                Err(e) => log::error(&format!(
                    "the bibliography was saved, but its history failed: {e}"
                )),
            }
        }
        written.push(basename(&target.bibtex));
    }
    for format in crate::formats::FORMATS {
        if format == Format::Bibtex || !options.formats.contains(&format) {
            continue;
        }
        // the save of papers sorts the fields of the entries in place: what is written next sees them sorted
        let sorted: Vec<Entry> = target
            .entries()
            .map(|e| {
                let mut e = e.clone();
                if make_library {
                    e.fields.sort_by(|a, b| a.key.cmp(&b.key));
                }
                e
            })
            .collect();
        let listed: Vec<&Entry> = sorted.iter().collect();
        let text = dumps(&listed, format, &|e| {
            files(e, Some(&target.relative_to))
                .iter()
                .map(|f| relpath(f, &dest))
                .collect()
        });
        let path = join(&dest, format.filename());
        fs::write(&path, text).map_err(|e| Error::other(format!("{path}: {e}")))?;
        written.push(format.filename().to_string());
    }
    log::println(&format!(
        "exported {} entries to {dest} ({}; files: {})",
        entries.len(),
        written.join(", "),
        options.files.name()
    ));
    if !make_library {
        log::println(&format!(
            "{dest} is not a papers library (no {}): --format bib or all makes one",
            export_bibtex_name(config)
        ));
    }
    Ok(())
}
