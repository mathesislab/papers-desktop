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

/// Say what a command did to the library since it was loaded: the entries
/// removed, renamed, added, and, of `touched` (the keys of the entries the
/// command considered), those modified; with `show_existing`, also those that
/// are as they were (`Existing`). With `--porcelain` the lines are the
/// keys only, by kind and sorted by key (`_print_biblio_diff`).
pub fn print_changes(
    library: &Library,
    touched: &[String],
    show_existing: bool,
    notes: &Notes,
    porcelain: bool,
    color: bool,
    say: &Say,
) {
    let Some(loaded) = library.loaded_entries() else {
        return;
    };
    let note = |key: &str| {
        notes
            .get(&key.to_lowercase())
            .map(|n| format!(" [{n}]"))
            .unwrap_or_default()
    };
    let old: BTreeMap<&str, &Entry> = loaded.iter().map(|e| (e.key.as_str(), *e)).collect();
    let new: BTreeMap<&str, &Entry> = library.entries().map(|e| (e.key.as_str(), e)).collect();
    let renamed_pairs = library.renamed_keys();
    let renamed: BTreeMap<&str, &str> = renamed_pairs
        .iter()
        .map(|(old, new)| (new.as_str(), old.as_str()))
        .collect();
    let renamed_old: BTreeSet<&str> = renamed.values().copied().collect();
    let removed: Vec<&str> = old
        .keys()
        .copied()
        .filter(|k| !new.contains_key(k) && !renamed_old.contains(k))
        .collect();
    let added: Vec<&str> = new
        .keys()
        .copied()
        .filter(|k| !old.contains_key(k) && !renamed.contains_key(k))
        .collect();
    let touched: BTreeSet<&str> = touched.iter().map(String::as_str).collect();
    let modified: Vec<&str> = new
        .keys()
        .copied()
        .filter(|k| touched.contains(k) && old.contains_key(k))
        .collect();

    if porcelain {
        for k in &removed {
            say.porcelain(format!("removed {k}"));
        }
        for (new_key, old_key) in &renamed {
            say.porcelain(format!("renamed {old_key} {new_key}"));
        }
        for k in &added {
            say.porcelain(format!("added {k}"));
        }
        for k in &modified {
            if !content_equal(old[k], new[k]) {
                say.porcelain(format!("modified {k}"));
            } else if show_existing {
                say.porcelain(format!("existing {k}"));
            }
        }
        return;
    }

    let loaded_relative_to = library
        .loaded_relative_to()
        .unwrap_or(&library.relative_to)
        .to_string();
    for k in &removed {
        say.line(format_entry(
            &loaded_relative_to,
            old[k],
            false,
            "Removed",
            color,
        ));
    }
    for (new_key, old_key) in &renamed {
        let line = format_entry(&library.relative_to, new[new_key], false, "Renamed", color);
        say.line(format!(
            "{} (was {old_key}){}",
            line.trim_end(),
            note(new_key)
        ));
    }
    for k in &added {
        say.line(format!(
            "{}{}",
            format_entry(&library.relative_to, new[k], false, "Added", color),
            note(k)
        ));
        for line in placed_lines(library, new[k]) {
            say.line(line);
        }
    }
    for k in &modified {
        if !content_equal(old[k], new[k]) {
            say.line(format!(
                "{}{}",
                format_entry(&library.relative_to, new[k], false, "Modified", color),
                note(k)
            ));
            for line in placed_lines(library, new[k]) {
                say.line(line);
            }
        } else if show_existing {
            say.line(format!(
                "{}{}",
                format_entry(&library.relative_to, new[k], false, "Existing", color),
                note(k)
            ));
        }
    }
}
