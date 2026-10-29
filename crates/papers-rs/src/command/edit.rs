//! `papers edit`: change an entry or several: fields (`--set`, `--unset`),
//! the key, a replacement from a file or text (`--from`), or the text of the
//! entries in an editor (`papers.cli.common`, `papers.cli.entry_cmds.editcmd`).
//!
//! An edit that is not valid does not change the entries: the error says so
//! (`-- keeping original entries`).

use crate::bib::{Bib, Block, Entry};
use crate::config::json::py_repr_str;
use crate::entry::{files, set_files, val};
use crate::error::{Error, Result};
use crate::library::Library;
use crate::log;

use crate::formats::{Format, import::loads};

/// The fields that `--set` and `--unset` do not change, and what does.
fn not_set(name: &str) -> Option<&'static str> {
    match name.to_lowercase().as_str() {
        "id" => Some("`papers edit KEY --key NEW`"),
        "file" => Some("`papers file attach` and `detach`"),
        _ => None,
    }
}

/// The name of a bibtex field: `^[A-Za-z][\w-]*$`.
fn is_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

/// Set the fields `values` of the entries of these keys, and remove the fields
/// `unset`; an error for a malformed name, the key or the file links (`set_fields`).
pub fn set_fields(
    library: &mut Library,
    keys: &[String],
    values: &[(String, String)],
    unset: &[String],
) -> Result<()> {
    for name in values.iter().map(|(n, _)| n).chain(unset) {
        if let Some(by) = not_set(name) {
            return Err(Error::exit(format!(
                "{name}: changed by {by}, not --set or --unset"
            )));
        }
    }
    // papers checks the form of the names of --set only (not of --unset)
    for (name, _) in values {
        // a field named `key` would be read as the key: --set key=... is --key
        if name.to_lowercase() == "key" {
            return Err(Error::exit(format!(
                "{name}: the key is changed by {}, not --set",
                not_set("id").unwrap_or_default()
            )));
        }
        if !is_field_name(name) {
            return Err(Error::exit(format!(
                "{} is not a field name",
                py_repr_str(name)
            )));
        }
    }
    for key in keys {
        let Some(e) = library.get_mut(key) else {
            continue;
        };
        for (name, value) in values {
            e.set(&name.to_lowercase(), value.clone());
        }
        for name in unset {
            e.remove(&name.to_lowercase());
        }
    }
    Ok(())
}

/// The entries of an edited text (bibtex, or CSL-JSON, BibJSON); an error if
/// it does not parse, has no entry, or has a malformed field name (`parse_edited`).
pub fn parse_edited(text: &str, format: Format) -> Result<Vec<Entry>> {
    let rejected = |what: String| Error::exit(format!("{what} -- keeping original entries"));
    let entries: Vec<Entry> = if format == Format::Bibtex {
        let bib = Bib::parse(text);
        if bib.blocks.iter().any(|b| matches!(b, Block::Unreadable(_))) {
            return Err(rejected(
                "some edited blocks could not be parsed as bibtex".to_string(),
            ));
        }
        bib.entries().cloned().collect()
    } else {
        loads(text, format).map_err(|error| {
            rejected(format!(
                "the text could not be read as {} ({error})",
                format.id()
            ))
        })?
    };
    if entries.is_empty() {
        return Err(rejected("the edited text contains no entry".to_string()));
    }
    for e in &entries {
        let bad: Vec<String> = e
            .fields
            .iter()
            .filter(|f| !is_field_name(&f.key))
            .map(|f| py_repr_str(&f.key))
            .collect();
        if !bad.is_empty() {
            return Err(rejected(format!(
                "edited entry {} has malformed field name(s) [{}]",
                py_repr_str(&e.key),
                bad.join(", ")
            )));
        }
    }
    Ok(entries)
}

/// The text of entries as the editor shows it: bibtex, in the order given.
pub fn entries_text(entries: &[Entry]) -> String {
    Bib {
        blocks: entries.iter().cloned().map(Block::Entry).collect(),
    }
    .format()
}

/// The text editor of a command: the `editor` setting, else `$VISUAL`, else
/// `$EDITOR`, else `vi` (a command that may have arguments and must wait until
/// the file is closed).
#[derive(Debug, Clone, Default)]
pub struct Editor {
    /// The `editor` setting of the library.
    pub command: Option<String>,
    /// The editor reads the terminal (`/dev/tty`) instead of the standard input, which is not one
    /// (the keys of the entries came from a pipe).
    pub tty: bool,
}

impl Editor {
    /// The editor of the `editor` setting of a configuration.
    pub fn of(config: &crate::config::Config) -> Editor {
        let setting = config.get("editor");
        Editor {
            command: setting
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            tty: false,
        }
    }

    fn program(&self) -> String {
        self.command
            .clone()
            .filter(|e| !e.is_empty())
            .or_else(|| std::env::var("VISUAL").ok().filter(|e| !e.is_empty()))
            .or_else(|| std::env::var("EDITOR").ok().filter(|e| !e.is_empty()))
            .unwrap_or_else(|| "vi".to_string())
    }
}

/// A text edited in a text editor: the edited text, or `None` if the editor failed.
pub fn edit_text(text: &str, editor: &Editor) -> Option<String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let path = std::env::temp_dir().join(format!("papers-{}-{nanos}.bib", std::process::id()));
    let result = (|| {
        std::fs::write(&path, text).ok()?;
        let quoted = format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
        let mut command = std::process::Command::new("sh");
        command
            .arg("-c")
            .arg(format!("{} {quoted}", editor.program()));
        if editor.tty
            && let Ok(tty) = std::fs::File::open("/dev/tty")
        {
            command.stdin(tty);
        }
        let status = command.status().ok()?;
        if !status.success() {
            log::error("the editor failed");
            return None;
        }
        let bytes = std::fs::read(&path).ok()?;
        let text = String::from_utf8(bytes).ok()?;
        // without the byte order mark that some editors write first
        Some(crate::bib::universal_newlines(crate::bib::without_bom(
            &text,
        )))
    })();
    let _ = std::fs::remove_file(&path);
    result
}

/// The entries edited in a text editor (`edit_in_editor`); an error if the editor
/// failed or the edit is not valid.
pub fn edit_in_editor(entries: &[Entry], editor: &Editor) -> Result<Vec<Entry>> {
    match edit_text(&entries_text(entries), editor) {
        Some(text) => parse_edited(&text, Format::Bibtex),
        None => Err(Error::exit(
            "failed to edit file -- keeping original entries",
        )),
    }
}

/// Edit the entries of these keys in a text editor, and put them back in the library.
pub fn edit_selected(library: &mut Library, keys: &[String], editor: &Editor) -> Result<()> {
    let entries: Vec<Entry> = keys
        .iter()
        .filter_map(|k| library.get(k).cloned())
        .collect();
    let edited = edit_in_editor(&entries, editor)?;
    library.replace_entries(keys, edited)
}

/// The entry of a text in place of the entry of key `key` (`--from`): with its
/// key, and its `file` and `keywords` fields when the new one has none. `label`
/// names the source in the messages (`--from FILE`).
pub fn replace_from_text(
    library: &mut Library,
    key: &str,
    text: &str,
    format: Format,
    label: &str,
) -> Result<()> {
    replace_from_text_in(library, key, text, format, label, None)
}

/// [`replace_from_text`] for the text of a file: its file links are relative to
/// `links_from`, the folder of that file, as with `add` (they are written relative
/// to the library).
pub fn replace_from_text_in(
    library: &mut Library,
    key: &str,
    text: &str,
    format: Format,
    label: &str,
    links_from: Option<&str>,
) -> Result<()> {
    let mut new = parse_edited(text, format)?;
    if new.len() != 1 {
        return Err(Error::exit(format!(
            "{label}: {} entries, --from takes one -- keeping original entries",
            new.len()
        )));
    }
    let mut new = new.remove(0);
    if let Some(base) = links_from
        && new.get("file").is_some()
    {
        let links = files(&new, Some(base));
        set_files(&mut new, &links, &library.relative_to);
    }
    let old = library
        .get(key)
        .cloned()
        .ok_or_else(|| Error::exit(format!("{key}: no entry found")))?;
    new.key = old.key.clone();
    if let Some(from) = links_from
        && new.get("file").is_some()
    {
        let links = crate::entry::files(&new, Some(from));
        crate::entry::set_files(&mut new, &links, &library.relative_to);
    }
    for name in ["file", "keywords"] {
        if new.get(name).is_none() && old.get(name).is_some() {
            new.set(name, val(&old, name));
        }
    }
    library.replace_entries(&[old.key], vec![new])
}

/// Change the key of an entry (`--key`): with `keep_old`, the former key goes to
/// `ids`. The key must be valid and not another entry's.
pub fn set_key(library: &mut Library, key: &str, new: &str, keep_old: bool) -> Result<()> {
    library.set_key(key, new, keep_old)
}
