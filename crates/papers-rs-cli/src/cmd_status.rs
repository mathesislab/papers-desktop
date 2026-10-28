//! `papers status`: where the library, its bibtex and its files are, and how it stands with its
//! pool (`papers_rs::pools::state`), with a hint of what to do about it on the
//! last line (`papers/config.py: Config.status`).

use std::fs;
use std::io::IsTerminal;

use papers_rs::bib::{Bib, read_file};
use papers_rs::config::dirs::cache_dir;
use papers_rs::config::paths::{exists, isdir, relpath, short_home};
use papers_rs::config::{Config, Origin};
use papers_rs::entry;
use papers_rs::error::{PAPERS_VERSION, Result};
use papers_rs::history::History;
use papers_rs::log;
use papers_rs::pools::pool_files_dir;
use papers_rs::pools::state::{Palette, status_line};

use crate::parse::Leaf;

/// The colour codes of the output: empty strings when it has no colours (`papers.utils.bcolors`).
struct Colors {
    warning: &'static str,
    fail: &'static str,
    okblue: &'static str,
    bold: &'static str,
    end: &'static str,
}

impl Colors {
    /// `color_enabled`: `--color always` or `never`, else (auto) only when the standard output is
    /// a terminal and the `NO_COLOR` environment variable is not set.
    fn new(choice: Option<&str>) -> Colors {
        let on = match choice {
            Some("always") => true,
            Some("never") => false,
            _ => std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()) && std::io::stdout().is_terminal(),
        };
        if on {
            Colors { warning: "\x1b[93m", fail: "\x1b[91m", okblue: "\x1b[94m", bold: "\x1b[1m", end: "\x1b[0m" }
        } else {
            Colors { warning: "", fail: "", okblue: "", bold: "", end: "" }
        }
    }
}

pub struct StatusArgs {
    color: Option<String>,
    check_files: bool,
    verbose: bool,
}

impl StatusArgs {
    pub fn from_leaf(leaf: &Leaf) -> StatusArgs {
        StatusArgs {
            color: leaf.string("color"),
            check_files: !leaf.flag("no_check_files"),
            // the deprecated --info is -v
            verbose: leaf.flag("verbose") || leaf.flag("info"),
        }
    }
}

/// The number and the size (bytes) of the PDFs under a folder (`check_filesdir`: `os.walk`, the
/// links to folders not followed).
fn check_filesdir(folder: &str) -> (usize, u64) {
    let (mut count, mut size) = (0, 0);
    let Ok(entries) = fs::read_dir(folder) else { return (0, 0) };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            let (c, s) = check_filesdir(&name);
            count += c;
            size += s;
        } else if name.to_lowercase().ends_with(".pdf")
            && let Ok(meta) = fs::metadata(&path)
            && !meta.is_dir()
        {
            count += 1;
            size += meta.len();
        }
    }
    (count, size)
}

pub fn status(args: StatusArgs, config: Config) -> Result<()> {
    let colors = Colors::new(args.color.as_deref());
    let fmt_path = |p: &str| if config.is_library { relpath(p, ".") } else { p.to_string() };

    let library_exists = config.file.as_deref().is_some_and(exists);
    let status = if library_exists {
        let file = config.file.as_deref().unwrap_or_default();
        let note = if config.origin == Some(Origin::Default) {
            format!("(default library, set in {})", short_home(config.machine.file.as_deref().unwrap_or("")))
        } else if config.origin == Some(Origin::V2Global) {
            "(global configuration of papers v2: run `papers migrate`)".to_string()
        } else {
            String::new()
        };
        format!("{} {note}", if config.is_library { config.configdir() } else { file.to_string() })
    } else {
        format!("{}(no library found: create one with `papers init`){}", colors.warning, colors.end)
    };
    let header = format!("{}{}{}", colors.bold, format!("papers library: {status}").trim_end(), colors.end);

    let mut items: Vec<(&str, String)> = Vec::new();
    let mut hint: Option<String> = None;
    if config.file.is_some() || config.bibtex.is_some() {
        items.push(("bibtex", bibtex_status(&config, &colors, args.check_files, &fmt_path)));
        items.push(("files", files_status(&config, &colors, args.check_files, &fmt_path)));
        if config.file.is_some() && config.shallow() {
            // the line of the pool (when it was last pulled from, what to do next)
            let palette = Palette { warning: colors.warning, end: colors.end };
            let (line, pool_hint) = status_line(&config, palette);
            items.push(("pool", line));
            hint = pool_hint;
        }
        if args.verbose {
            items.push(("history", history_summary(&config)));
        }
    }
    if args.verbose {
        let file = match config.file.as_deref() {
            Some(file) if exists(file) => fmt_path(file),
            _ => format!("{}none{}", colors.warning, colors.end),
        };
        items.push(("configuration file", file));
        items.push(("cache directory", short_home(&cache_dir())));
        items.push(("version", PAPERS_VERSION.to_string()));
    }

    let width = items.iter().map(|(label, _)| label.chars().count()).max().map_or(0, |w| w + 1);
    let mut lines = vec![header];
    for (label, value) in &items {
        lines.push(format!("* {:<width$} {value}", format!("{label}:")));
    }
    if let Some(hint) = hint {
        lines.push(hint);
    } else if config.file.is_some() {
        lines.push("`papers config` lists and sets the settings".to_string());
    }
    log::println(&lines.join("\n"));
    if config.file.is_none() {
        let default = match config.machine.default_library_path() {
            Some(default) => format!("{} (not a library)", short_home(&default)),
            None => "none".to_string(),
        };
        log::println(&format!("* default library: {default}"));
    }
    Ok(())
}

/// The bibtex of the library, and its state.
fn bibtex_status(config: &Config, colors: &Colors, check_files: bool, fmt_path: &dyn Fn(&str) -> String) -> String {
    let Some(bibtex) = config.bibtex.as_deref() else {
        return format!("None{} (unset){}", colors.warning, colors.end);
    };
    let status = if !exists(bibtex) {
        format!("{} (missing){}", colors.warning, colors.end)
    } else if check_files {
        match read_file(std::path::Path::new(bibtex)) {
            Ok(text) => {
                let bib = Bib::parse(&text);
                let entries: Vec<_> = bib.entries().collect();
                if entries.is_empty() {
                    format!("{} (empty){}", colors.warning, colors.end)
                } else {
                    let links: usize = entries.iter().map(|e| entry::files(e, None).len()).sum();
                    format!("{} ({} entries, {links} file links){}", colors.okblue, entries.len(), colors.end)
                }
            }
            Err(_) => format!("{} (corrupted){}", colors.fail, colors.end),
        }
    } else if fs::metadata(bibtex).map(|m| m.len() == 0).unwrap_or(false) {
        format!("{} (empty){}", colors.warning, colors.end)
    } else {
        String::new()
    };
    format!("{}{status}", fmt_path(bibtex))
}

/// Where the files of the library are.
fn files_status(config: &Config, colors: &Colors, check_files: bool, fmt_path: &dyn Fn(&str) -> String) -> String {
    if config.shallow() {
        // the files of a shallow library are in its pool
        return match pool_files_dir(config) {
            Some(dir) => format!("in the pool, {}", short_home(&dir)),
            None => "in the pool".to_string(),
        };
    }
    let Some(filesdir) = config.filesdir.as_deref() else {
        return "linked where they are (no files directory)".to_string();
    };
    let mut notes = vec!["the files directory of the library".to_string()];
    if !exists(filesdir) {
        notes.push(format!("{}missing{}", colors.warning, colors.end));
    } else if isdir(filesdir) && fs::read_dir(filesdir).map(|mut d| d.next().is_none()).unwrap_or(false) {
        notes.push(format!("{}empty{}", colors.warning, colors.end));
    } else if check_files {
        let (count, size) = check_filesdir(filesdir);
        notes.push(format!("{}{count} files, {:.1} MB{}", colors.okblue, size as f64 / (1024.0 * 1024.0), colors.end));
    }
    format!("{} ({})", fmt_path(filesdir), notes.join(", "))
}

/// The history of the library, for `papers status -v`: `papers history` tells more.
fn history_summary(config: &Config) -> String {
    let mut config = config.clone();
    let steps = match History::open(&mut config, false, false) {
        Ok(Some(history)) => history.project.states().len(),
        _ => 0,
    };
    format!("{steps} step{} (`papers history`)", if steps == 1 { "" } else { "s" })
}
