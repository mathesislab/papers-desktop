//! The lines of a command: an entry on one line (`format_entry`), and what a
//! command changed in the library (`_print_biblio_diff`): the entries removed,
//! renamed, added and modified, as lines for people, or with `--porcelain`
//! as lines for programs (`formats.md#7`: `removed KEY`, `renamed OLD NEW`,
//! `added KEY`, `modified KEY`, `existing KEY`, by kind and sorted by key).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::bib::Entry;
use crate::bib::encoding::{parse_file, parse_keywords};
use crate::config::paths::{basename, dirname, getcwd, realpath};
use crate::entry::{self, val};
use crate::history::restore::show;
use crate::library::Library;

use super::session::Say;

const HEADER_WARNING: &str = "\x1b[93m";
const OK_BLUE: &str = "\x1b[94m";
const OK_GREEN: &str = "\x1b[92m";
const END: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";

/// An ANSI hyperlink (`ansi_link`): the label only without colors.
fn link(uri: &str, label: &str, color: bool) -> String {
    if color {
        format!("\x1b]8;;{uri}\x1b\\{label}\x1b]8;;\x1b\\")
    } else {
        label.to_string()
    }
}

/// `os.path.commonpath` of absolute paths.
fn common_path(paths: &[String]) -> String {
    let split: Vec<Vec<&str>> = paths
        .iter()
        .map(|p| p.split('/').filter(|c| !c.is_empty()).collect())
        .collect();
    let mut common: Vec<&str> = split.first().cloned().unwrap_or_default();
    for other in &split[1..] {
        let n = common.iter().zip(other).take_while(|(a, b)| a == b).count();
        common.truncate(n);
    }
    format!("/{}", common.join("/"))
}

/// The key of an entry in a line: `key:`, in bold for each file it has, in blue (`format_key`).
pub fn format_key(e: &Entry, no_key: bool, color: bool) -> String {
    if no_key {
        return String::new();
    }
    let n = parse_file(val(e, "file")).len();
    let (bold, blue, end) = if color {
        (BOLD, OK_BLUE, END)
    } else {
        ("", "", "")
    };
    format!("{}{blue}{}:{end}", bold.repeat(n), e.key)
}

/// The one-liner of an entry: `key: title (doi:..., file:1, tag | tag)`, with
/// `prefix -> ` before it (`format_entry`). `relative_to`: what the links of
/// the `file` field are relative to (the library's).
pub fn format_entry(
    relative_to: &str,
    e: &Entry,
    no_key: bool,
    prefix: &str,
    color: bool,
) -> String {
    let title = val(e, "title");
    let mut tit: String = title.chars().take(60).collect();
    if title.chars().count() > 60 {
        tit.push_str("...");
    }
    let (warning, green, end) = if color {
        (HEADER_WARNING, OK_GREEN, END)
    } else {
        ("", "", "")
    };
    let mut info: Vec<String> = Vec::new();
    let doi = val(e, "doi");
    if !doi.is_empty() {
        info.push(link(
            &format!("https://doi.org/{doi}"),
            &format!("doi:{doi}"),
            color,
        ));
    }
    let files = entry::files(e, Some(relative_to));
    let n = files.len();
    if n > 0 {
        let target = if n == 1 {
            realpath(&files[0])
        } else {
            realpath(&common_path(&files))
        };
        let shown = link(
            &format!("file:///{target}"),
            &format!("{}:{n}", if n == 1 { "file" } else { "files" }),
            color,
        );
        info.push(format!("{green}{shown}{end}"));
    }
    if !val(e, "keywords").is_empty() {
        info.push(format!(
            "{warning}{}{end}",
            parse_keywords(val(e, "keywords")).join(" | ")
        ));
    }
    let infotag = if info.is_empty() {
        String::new()
    } else {
        format!("({})", info.join(", "))
    };
    let prefixtag = if prefix.is_empty() {
        String::new()
    } else {
        format!("{warning}{prefix} -> {end}")
    };
    format!(
        "{prefixtag}{} {tit} {infotag}",
        format_key(e, no_key, color)
    )
}

/// Two entries have the same content: the type, the key and the fields
/// (`entry_content_equal`).
pub fn content_equal(a: &Entry, b: &Entry) -> bool {
    let content = |e: &Entry| -> (String, String, BTreeMap<String, String>) {
        let mut fields = BTreeMap::new();
        for f in &e.fields {
            fields.insert(f.key.clone(), f.value.clone());
        }
        (e.entry_type.clone(), e.key.clone(), fields)
    };
    content(a) == content(b)
}

/// The files of an entry that the command moved or copied: from where, to where
/// (`_print_placed`).
fn placed_lines(library: &Library, e: &Entry) -> Vec<String> {
    let cwd = getcwd();
    let placed = library.placed();
    let mut lines = Vec::new();
    for f in entry::files(e, Some(&library.relative_to)) {
        let Some((source, action)) = placed.get(&f) else {
            continue;
        };
        if dirname(source) == dirname(&f) {
            lines.push(format!(
                "    {} --> renamed to {}",
                show(source, &cwd),
                basename(&f)
            ));
        } else {
            lines.push(format!(
                "    {} --> {action} to {}",
                show(source, &cwd),
                show(&f, &cwd)
            ));
        }
    }
    lines
}

/// What the command changed, for [`print_changes`]: the notes to add to the
/// line of an entry, in brackets, by lower-case key (where else it went, or where it came from).
pub type Notes = HashMap<String, String>;

/// What a command changed in the library since it was loaded, by key
/// (`_print_biblio_diff`): the entries removed, renamed, added, and of those
/// the command considered (`touched`), the ones modified and the ones as
/// they were. Each list is sorted by key (`renamed` by the new key).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Changes {
    pub removed: Vec<String>,
    /// (old key, new key)
    pub renamed: Vec<(String, String)>,
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub existing: Vec<String>,
}

impl Changes {
    /// The changes of `library` since it was loaded; none if it was not
    /// loaded (a new library).
    pub fn of(library: &Library, touched: &[String]) -> Option<Changes> {
        let loaded = library.loaded_entries()?;
        let old: BTreeMap<&str, &Entry> = loaded.iter().map(|e| (e.key.as_str(), *e)).collect();
        let new: BTreeMap<&str, &Entry> = library.entries().map(|e| (e.key.as_str(), e)).collect();
        let renamed_pairs = library.renamed_keys();
        let renamed: BTreeMap<&str, &str> = renamed_pairs
            .iter()
            .map(|(old, new)| (new.as_str(), old.as_str()))
            .collect();
        let renamed_old: BTreeSet<&str> = renamed.values().copied().collect();
        let touched: BTreeSet<&str> = touched.iter().map(String::as_str).collect();
        let mut changes = Changes {
            removed: old
                .keys()
                .filter(|k| !new.contains_key(*k) && !renamed_old.contains(*k))
                .map(|k| k.to_string())
                .collect(),
            renamed: renamed
                .iter()
                .map(|(new, old)| (old.to_string(), new.to_string()))
                .collect(),
            added: new
                .keys()
                .filter(|k| !old.contains_key(*k) && !renamed.contains_key(*k))
                .map(|k| k.to_string())
                .collect(),
            ..Changes::default()
        };
        for (k, entry) in &new {
            if !touched.contains(k) {
                continue;
            }
            let Some(before) = old.get(k) else { continue };
            if content_equal(before, entry) {
                changes.existing.push(k.to_string());
            } else {
                changes.modified.push(k.to_string());
            }
        }
        Some(changes)
    }

    /// The lines of `--porcelain`, by kind (`existing` with `show_existing`).
    pub fn porcelain(&self, show_existing: bool) -> Vec<String> {
        let mut lines: Vec<String> = self
            .removed
            .iter()
            .map(|k| format!("removed {k}"))
            .collect();
        lines.extend(
            self.renamed
                .iter()
                .map(|(old, new)| format!("renamed {old} {new}")),
        );
        lines.extend(self.added.iter().map(|k| format!("added {k}")));
        // modified and existing in the order of their keys, as papers prints them
        let mut touched: Vec<(&str, &str)> = self
            .modified
            .iter()
            .map(|k| (k.as_str(), "modified"))
            .collect();
        if show_existing {
            touched.extend(self.existing.iter().map(|k| (k.as_str(), "existing")));
        }
        touched.sort();
        lines.extend(touched.into_iter().map(|(k, what)| format!("{what} {k}")));
        lines
    }

    /// The changes of the `--porcelain` lines of a papers command (formats.md
    /// 7.2); lines of another kind are left out.
    pub fn from_porcelain(text: &str) -> Changes {
        let mut changes = Changes::default();
        for line in text.lines() {
            let mut words = line.split_whitespace();
            match (words.next(), words.next(), words.next()) {
                (Some("removed"), Some(k), None) => changes.removed.push(k.to_string()),
                (Some("renamed"), Some(old), Some(new)) => {
                    changes.renamed.push((old.to_string(), new.to_string()))
                }
                (Some("added"), Some(k), None) => changes.added.push(k.to_string()),
                (Some("modified"), Some(k), None) => changes.modified.push(k.to_string()),
                (Some("existing"), Some(k), None) => changes.existing.push(k.to_string()),
                _ => {}
            }
        }
        changes
    }
}

/// Say what a command did to the library since it was loaded ([`Changes`]);
/// with `show_existing`, also the entries that are as they were
/// (`Existing`). With `--porcelain` the lines are the keys only, by kind and
/// sorted by key (`_print_biblio_diff`).
pub fn print_changes(
    library: &Library,
    touched: &[String],
    show_existing: bool,
    notes: &Notes,
    porcelain: bool,
    color: bool,
    say: &Say,
) {
    let Some(changes) = Changes::of(library, touched) else {
        return;
    };
    if porcelain {
        for line in changes.porcelain(show_existing) {
            say.porcelain(line);
        }
        return;
    }
    let loaded = library.loaded_entries().unwrap_or_default();
    let old: BTreeMap<&str, &Entry> = loaded.iter().map(|e| (e.key.as_str(), *e)).collect();
    let new: BTreeMap<&str, &Entry> = library.entries().map(|e| (e.key.as_str(), e)).collect();
    let note = |key: &str| {
        notes
            .get(&key.to_lowercase())
            .map(|n| format!(" [{n}]"))
            .unwrap_or_default()
    };
    let loaded_relative_to = library
        .loaded_relative_to()
        .unwrap_or(&library.relative_to)
        .to_string();
    for k in &changes.removed {
        say.line(format_entry(
            &loaded_relative_to,
            old[k.as_str()],
            false,
            "Removed",
            color,
        ));
    }
    for (old_key, new_key) in &changes.renamed {
        let line = format_entry(
            &library.relative_to,
            new[new_key.as_str()],
            false,
            "Renamed",
            color,
        );
        say.line(format!(
            "{} (was {old_key}){}",
            line.trim_end(),
            note(new_key)
        ));
    }
    for k in &changes.added {
        let entry = new[k.as_str()];
        say.line(format!(
            "{}{}",
            format_entry(&library.relative_to, entry, false, "Added", color),
            note(k)
        ));
        for line in placed_lines(library, entry) {
            say.line(line);
        }
    }
    // modified and existing in the order of their keys
    let mut touched: Vec<(&str, bool)> = changes
        .modified
        .iter()
        .map(|k| (k.as_str(), true))
        .collect();
    if show_existing {
        touched.extend(changes.existing.iter().map(|k| (k.as_str(), false)));
    }
    touched.sort();
    for (k, modified) in touched {
        let entry = new[k];
        let what = if modified { "Modified" } else { "Existing" };
        say.line(format!(
            "{}{}",
            format_entry(&library.relative_to, entry, false, what, color),
            note(k)
        ));
        if modified {
            for line in placed_lines(library, entry) {
                say.line(line);
            }
        }
    }
}
