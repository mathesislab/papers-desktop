//! `papers export DIR [KEY...]`: the entries written as a new library in DIR, and in the other
//! formats. The work is `papers_rs::export`; this is the command line and the history of the
//! library of the command.

use papers_rs::bib::Entry;
use papers_rs::config::Config;
use papers_rs::error::Error;
use papers_rs::export::{ExportOptions, Exported, FilesMode, export, parse_formats};
use papers_rs::formats::{FORMATS, Format};
use papers_rs::history::{History, Step};
use papers_rs::library::Library;
use papers_rs::log;

use crate::cmd_config::Failure;
use crate::cmd_list::{check_install, read_selection};
use crate::parse::Leaf;

/// The formats of the values of an option (`--format`, `--export-format`), once each in the order of
/// `FORMATS`; `bibtex` alone without the option.
pub fn formats_of(option: &str, values: &[String]) -> Result<Vec<Format>, Failure> {
    if values.is_empty() {
        return Ok(vec![Format::Bibtex]);
    }
    let mut all = Vec::new();
    for value in values {
        all.extend(
            parse_formats(value)
                .map_err(|message| Failure::Usage(format!("argument {option}: {message}")))?,
        );
    }
    Ok(FORMATS.into_iter().filter(|f| all.contains(f)).collect())
}

/// What the history calls the command: `papers` and its arguments, joined with spaces.
fn message() -> String {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    format!("papers {}", args.join(" "))
}

/// Export the entries and record what the command did to the files in the history of the library.
pub fn export_entries(
    config: &Config,
    library: &Library,
    entries: &[&Entry],
    dir: &str,
    formats: Vec<Format>,
    files: FilesMode,
    dry_run: bool,
) -> Result<(), Error> {
    // the library as it is before the command, for its first state if it has no history yet
    let mut before_config = config.clone();
    let before = if dry_run {
        None
    } else {
        History::before_command(&mut before_config)
    };
    let options = ExportOptions {
        dir: dir.to_string(),
        formats,
        files,
        message: message(),
        dry_run,
    };
    let mut out = Exported::default();
    let result = export(library, config, entries, &options, &mut out);
    // the files that the command copied join a state of the library of the command, linked to the new library's
    if !out.ops.is_empty() && !dry_run {
        let count = out.ops.len();
        let mut own = config.clone();
        let recorded = History::open(&mut own, true, false).and_then(|history| {
            let Some(mut history) = history else {
                return Ok(());
            };
            history.sync_head(before.as_ref())?;
            let step = Step {
                message: message(),
                ops: out.ops.clone(),
                pools: out.link.clone().into_iter().collect(),
                ..Step::default()
            };
            history.record(step).map(|_| ())
        });
        if let Err(error) = recorded {
            log::error(&format!(
                "{count} file operation(s) not recorded for undo: {error}"
            ));
        }
    }
    result
}

pub fn export_cmd(leaf: &Leaf, config: Config) -> Result<(), Failure> {
    let formats = formats_of("--format", &leaf.strings("format"))?;
    let config = check_install(leaf, config, true)?;
    let library = papers_rs::command::pool::load_library(&config)?;
    let args = leaf.strings("entries");
    let entries: Vec<&Entry> = if args.is_empty() {
        library.entries().collect()
    } else {
        let keys = library.select(&read_selection(&args))?;
        keys.iter().filter_map(|k| library.get(k)).collect()
    };
    let files = FilesMode::parse(&leaf.string("files").unwrap_or_else(|| "copy".to_string()))
        .unwrap_or(FilesMode::Copy);
    let dir = leaf.string("dir").unwrap_or_default();
    export_entries(
        &config,
        &library,
        &entries,
        &dir,
        formats,
        files,
        leaf.flag("dry_run"),
    )?;
    Ok(())
}
