//! `papers check`: fix the entries of a library (author names, the encoding of the fields, the DOI,
//! the fields fetched from crossref, the key), and merge the entries that are the same paper
//! (`papers.cli.entry_cmds.checkcmd`, `papers.bib.Biblio.fix_entry`, `papers.bib.Biblio.check_duplicates`).
//!
//! The fixes apply to one entry after the other, in the order of the options of papers: names,
//! encoding, DOI, fetch, key (with a question after each entry that changed, unless `-y/--yes`:
//! the entry as it was is put back if the answer is not yes). The duplicates are merged after all
//! of them. What the command prints is given to the [`Say`] of the session; the changes are then
//! those of [`super::report::print_changes`].

use std::collections::BTreeMap;

use crate::bib::encoding::{family_names, standard_name};
use crate::bib::latex_table::string_to_latex;
use crate::bib::unicode::convert_entry_to_unicode;
use crate::bib::{Bib, Entry};
use crate::config::json::py_repr_str;
use crate::difflib::{ndiff, splitlines_keepends};
use crate::entry::val;
use crate::error::{Error, Result};
use crate::extract::doi::parse_doi;
use crate::fetch::Fetcher;
use crate::identity::{Identifiers, family_name_ids};
use crate::library::Library;
use crate::log;
use crate::merge::{OnDuplicate, Resolve};
use crate::names::{ascii, is_valid_key};

use super::edit::entries_text;
use super::report::content_equal;
use super::session::{Say, Session};

/// The encoding of the fields that `--encoding` converts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Unicode,
    Latex,
}

impl Encoding {
    pub fn parse(name: &str) -> Option<Encoding> {
        match name {
            "unicode" => Some(Encoding::Unicode),
            "latex" => Some(Encoding::Latex),
            _ => None,
        }
    }
}

/// The fixes of [`fix_entry`]; the default is that of `Biblio.fix_entry` (names and DOIs are
/// fixed), `papers check` passes its options explicitly.
#[derive(Debug, Clone)]
pub struct FixOptions {
    pub fix_doi: bool,
    pub fetch: bool,
    pub fetch_all: bool,
    pub fix_key: bool,
    pub auto_key: bool,
    pub key_ascii: bool,
    pub encoding: Option<Encoding>,
    pub format_name: bool,
    /// Ask before an entry is changed (`papers check` without `-y`).
    pub interactive: bool,
}

impl Default for FixOptions {
    fn default() -> FixOptions {
        FixOptions {
            fix_doi: true,
            fetch: false,
            fetch_all: false,
            fix_key: false,
            auto_key: false,
            key_ascii: false,
            encoding: None,
            format_name: true,
            interactive: false,
        }
    }
}

/// Where the bibtex of a DOI or of a search comes from: crossref (arXiv, doi.org), or, in the
/// tests, what was recorded. The error is the text of the exception of papers.
pub trait Fetch {
    /// `fetch_bibtex_by_doi`
    fn by_doi(&self, doi: &str) -> std::result::Result<String, String>;
    /// `fetch_bibtex_by_fulltext_crossref('', title=..., author=...)`
    fn by_title_and_authors(
        &self,
        title: &str,
        authors: &str,
    ) -> std::result::Result<String, String>;
}

impl Fetch for Fetcher {
    fn by_doi(&self, doi: &str) -> std::result::Result<String, String> {
        self.fetch_bibtex_by_doi(doi).map_err(|e| e.message)
    }

    fn by_title_and_authors(
        &self,
        title: &str,
        authors: &str,
    ) -> std::result::Result<String, String> {
        self.fetch_bibtex_by_fulltext_crossref("", Some(title), Some(authors), 5)
            .map_err(|e| e.message)
    }
}

/// The fields that `--encoding latex` leaves as they are (`LATEX_SKIP`).
const LATEX_SKIP: [&str; 8] = [
    "abstract", "file", "doi", "url", "eprint", "isbn", "issn", "ids",
];

/// The ratio of titles (0 to 100) from which a search result is taken for the entry
/// (`TITLE_MATCH_RATIO`).
const TITLE_MATCH_RATIO: f64 = 90.0;

/// Why a bibliographic search result is not the paper of the entry, or none when it is
/// (`search_result_mismatch`): the normalized titles are the same or alike, and the authors have
/// a family name in common.
pub fn search_result_mismatch(entry: &Entry, found: &Entry) -> Option<String> {
    let (ta, tb) = (Identifiers::of(entry).title, Identifiers::of(found).title);
    if ta.is_empty() || tb.is_empty() {
        return Some("no title".to_string());
    }
    if ta != tb {
        // rapidfuzz of Rust gives 0 to 1, that of Python 0 to 100
        let score = 100.0 * rapidfuzz::fuzz::ratio(ta.chars(), tb.chars());
        if score < TITLE_MATCH_RATIO {
            return Some(format!(
                "different title ({}% similar)",
                py_format_0f(score)
            ));
        }
    }
    let mine = family_name_ids(val(entry, "author"));
    let common = family_name_ids(val(found, "author"))
        .into_iter()
        .any(|name| !name.is_empty() && mine.contains(&name));
    if !common {
        return Some("no author in common".to_string());
    }
    None
}

/// `'{:.0f}'.format(x)` for a score: rounded as Python does, to the even number on a tie.
fn py_format_0f(x: f64) -> String {
    let floor = x.floor();
    let diff = x - floor;
    let rounded = if diff < 0.5 {
        floor
    } else if diff > 0.5 {
        floor + 1.0
    } else if floor % 2.0 == 0.0 {
        floor
    } else {
        floor + 1.0
    };
    format!("{rounded:.0}")
}

/// A copy of an entry with its fields given again, as `entry_copy` does it.
fn entry_copy(entry: &Entry) -> Entry {
    Entry {
        entry_type: entry.entry_type.clone(),
        key: entry.key.clone(),
        fields: entry
            .fields
            .iter()
            .map(|f| crate::bib::Field::new(&f.key, &f.value))
            .collect(),
        source: entry.source.clone(),
    }
}

/// What `fix_entry` compares of an entry and of the fetched one (`strip_e`): the type, and the
/// fields but `file`; of `entry`, only those that `other` has.
fn strip(entry: &Entry, only_of: Option<&Entry>) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert("ENTRYTYPE".to_string(), entry.entry_type.clone());
    for f in &entry.fields {
        if f.key != "file" && only_of.is_none_or(|o| o.get(&f.key).is_some()) {
            map.insert(f.key.clone(), val(entry, &f.key).to_string());
        }
    }
    map
}

/// The line diff of two entries as bibtex, in the marks of `ndiff` (`entry_diff`); with `color`, the
/// lines are painted: green for those added, red for those removed, yellow for the marks.
pub fn entry_diff(old: &Entry, new: &Entry, color: bool) -> String {
    let a = splitlines_keepends(&entries_text(std::slice::from_ref(old)));
    let b = splitlines_keepends(&entries_text(std::slice::from_ref(new)));
    let diff = ndiff(&a, &b).concat();
    let (green, red, yellow, end) = if color {
        ("\x1b[92m", "\x1b[91m", "\x1b[93m", "\x1b[0m")
    } else {
        ("", "", "", "")
    };
    crate::bib::pystr::splitlines(&diff)
        .into_iter()
        .map(|line| match line.chars().next() {
            Some('+') => format!("{green}{line}{end}"),
            Some('-') => format!("{red}{line}{end}"),
            Some('?') => format!("{yellow}{line}{end}"),
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The header of the question of an entry that changes.
pub fn update_header(color: bool) -> String {
    if color {
        "\x1b[94m*** UPDATE ***\x1b[0m".to_string()
    } else {
        "*** UPDATE ***".to_string()
    }
}

/// Everything of `fix_entry` but the question: the fixes of the options, in place.
/// `library` has the keys that a new key must not take.
pub fn fix_entry_fields(
    library: &Library,
    e: &mut Entry,
    options: &FixOptions,
    fetcher: Option<&dyn Fetch>,
) -> Result<()> {
    if options.format_name {
        for k in ["author", "editor"] {
            if let Some(old) = e.get(k).map(str::to_string) {
                let new = standard_name(&old);
                e.set(k, new.clone());
                if new != old {
                    log::info(&format!("{}: {k} name formatted", e.key));
                }
            }
        }
    }

    if let Some(encoding) = options.encoding {
        log::debug(&format!("{}: update encoding", e.key));
        match encoding {
            Encoding::Unicode => convert_entry_to_unicode(e)
                .map_err(|error| Error::other(format!("{}: {error}", e.key)))?,
            Encoding::Latex => {
                // all the fields but the abstract, and the paths and identifiers
                // (`string_to_latex` escapes `_` and `~`)
                let mut keys: Vec<String> = Vec::new();
                for f in &e.fields {
                    if !LATEX_SKIP.contains(&f.key.as_str())
                        && f.key == f.key.to_lowercase()
                        && !keys.contains(&f.key)
                    {
                        keys.push(f.key.clone());
                    }
                }
                for k in keys {
                    let value = string_to_latex(val(e, &k));
                    e.set(&k, value);
                }
            }
        }
    }

    if options.fix_doi {
        match e.get("doi").filter(|d| !d.is_empty()).map(str::to_string) {
            Some(old) => {
                let doi = match parse_doi(&format!("doi:{old}")) {
                    Ok(doi) => doi,
                    Err(_) => {
                        // keep the DOI as it is, and go on with the other fixes
                        log::warning(&format!("{}: failed to fix doi: {old}", e.key));
                        old.clone()
                    }
                };
                if doi.to_lowercase() != old.to_lowercase() {
                    log::info(&format!("{}: fix doi: {old} ==> {doi}", e.key));
                    e.set("doi", doi);
                } else {
                    log::debug(&format!("{}: doi OK", e.key));
                }
            }
            None => log::debug(&format!("{}: no DOI", e.key)),
        }
    }

    if options.fetch || options.fetch_all {
        fetch_entry(library, e, options, fetcher)?;
    }

    if (options.fix_key || options.auto_key) && (options.auto_key || !is_valid_key(&e.key)) {
        let key = library.generate_key(e)?;
        if e.key != key {
            log::info(&format!("update key {} => {key}", e.key));
            e.key = key;
        }
    }

    if options.key_ascii {
        e.key = ascii(&e.key);
    }
    Ok(())
}

/// The fetch of `fix_entry`: by DOI, or (`--fetch-all`, without DOI) by a search of the title and
/// authors; the fields of the record replace those of the entry.
fn fetch_entry(
    library: &Library,
    e: &mut Entry,
    options: &FixOptions,
    fetcher: Option<&dyn Fetch>,
) -> Result<()> {
    let mut bibtex: Option<String> = None;
    let mut searched = false;
    let doi = val(e, "doi").to_string();
    let title = val(e, "title").to_string();
    let author = val(e, "author").to_string();
    if !doi.is_empty() {
        log::info(&format!("{}: fetch doi: {doi}", e.key));
        match fetcher {
            Some(fetcher) => match fetcher.by_doi(&doi) {
                Ok(text) => bibtex = Some(text),
                Err(error) => log::warning(&format!("...failed to fetch bibtex (doi): {error}")),
            },
            None => log::warning("...failed to fetch bibtex (doi): no network"),
        }
    } else if !title.is_empty() && !author.is_empty() && options.fetch_all {
        let family = family_names(&author).join(" ");
        log::info(&format!(
            "{}: fetch-all: {{'title': {}, 'author': {}}}",
            e.key,
            py_repr_str(&title),
            py_repr_str(&family)
        ));
        match fetcher {
            Some(fetcher) => match fetcher.by_title_and_authors(&title, &family) {
                Ok(text) => {
                    bibtex = Some(text);
                    searched = true;
                }
                Err(error) => {
                    log::warning(&format!("...failed to fetch/update bibtex (all): {error}"))
                }
            },
            None => log::warning("...failed to fetch/update bibtex (all): no network"),
        }
    }

    let Some(text) = bibtex.filter(|b| !b.is_empty()) else {
        return Ok(());
    };
    let Some(mut e2) = Bib::parse(&text).entries().next().cloned() else {
        // papers: IndexError, a traceback
        return Err(Error::other("list index out of range"));
    };
    let fixes = FixOptions {
        fix_doi: true,
        format_name: true,
        encoding: options.encoding,
        ..FixOptions::default()
    };
    fix_entry_fields(library, &mut e2, &fixes, None)?;
    // the best search result may be another paper
    if searched && let Some(mismatch) = search_result_mismatch(e, &e2) {
        log::warning(&format!(
            "...fetch-all: rejected search result ({mismatch}): {} ({})",
            val(&e2, "title"),
            e2.get("doi").unwrap_or("no doi")
        ));
        return Ok(());
    }
    if strip(e, Some(&e2)) != strip(&e2, None) {
        log::info("...fetch-update entry");
        e.entry_type = e2.entry_type.clone();
        for f in &e2.fields {
            if f.key != "file" {
                e.set(&f.key, val(&e2, &f.key).to_string());
            }
        }
    } else {
        log::info("...fetch-update: already up to date");
    }
    Ok(())
}

/// `Biblio.fix_entry` on an entry of the library, with the question after it if it changed
/// (`options.interactive`): `confirm(entry before, entry after)` says whether to keep the changes;
/// if not, the entry is as it was. The key of the entry after.
pub fn fix_entry(
    library: &mut Library,
    key: &str,
    options: &FixOptions,
    fetcher: Option<&dyn Fetch>,
    confirm: &mut dyn FnMut(&Entry, &Entry) -> Result<bool>,
) -> Result<Option<String>> {
    let Some(entry) = library.get(key) else {
        return Ok(None);
    };
    let old = entry_copy(entry);
    let mut e = old.clone();
    fix_entry_fields(library, &mut e, options, fetcher)?;
    if options.interactive && !content_equal(&old, &e) && !confirm(&old, &e)? {
        log::info("cancel changes");
        e = old;
    }
    let now = e.key.clone();
    if let Some(target) = library.get_mut(key) {
        *target = e;
    }
    Ok(Some(now))
}

/// `fix_entry` on the entries of these keys of a session's library, one after the other, with the
/// question `update? [Y/n]` (and the diff) after each that changed if `options.interactive`.
/// The keys the entries have after.
pub fn fix_entries(
    session: &mut Session,
    keys: &[String],
    options: &FixOptions,
    fetcher: Option<&dyn Fetch>,
) -> Result<Vec<String>> {
    let (say, color) = (session.say.clone(), session.color());
    let prompt = session.prompt();
    let mut confirm = |old: &Entry, new: &Entry| -> Result<bool> {
        say.line(update_header(color));
        say.line(entry_diff(old, new, color));
        let answer = {
            let mut prompt = prompt.lock().expect("the prompt");
            prompt.input("update? [Y/n] ")
        }
        .ok_or_else(super::session::no_input_error)?;
        Ok(matches!(answer.to_lowercase().as_str(), "" | "y"))
    };
    let mut now = Vec::new();
    for key in keys {
        now.extend(fix_entry(
            &mut session.library,
            key,
            options,
            fetcher,
            &mut confirm,
        )?);
    }
    Ok(now)
}

/// What `papers check` was asked.
#[derive(Debug, Clone)]
pub struct CheckRequest {
    /// The entries to check (identifiers, `-` already read); none: all of them.
    pub selection: Vec<String>,
    /// The fixes (`interactive` is not used: it is `!yes`).
    pub fix: FixOptions,
    /// `-y/--yes`: no question.
    pub yes: bool,
    /// `--duplicates`: merge the entries that are the same paper.
    pub duplicates: bool,
    /// `--on-duplicate` or `-m/--mode` was given (they apply with `--duplicates` only).
    pub on_duplicate_given: bool,
}

/// `papers check` on the library of a session: the fixes of the entries, then the duplicates.
/// The keys of the entries to report (all of them) are returned; saving the library is for the caller.
pub fn check(
    session: &mut Session,
    request: &CheckRequest,
    fetcher: Option<&dyn Fetch>,
    resolver: &mut dyn FnMut() -> Box<dyn Resolve>,
) -> Result<Vec<String>> {
    let keys: Vec<String> = if request.selection.is_empty() {
        session.library.entries().map(|e| e.key.clone()).collect()
    } else {
        session.library.select(&request.selection)?
    };
    let options = FixOptions {
        interactive: !request.yes,
        ..request.fix.clone()
    };
    fix_entries(session, &keys, &options, fetcher)?;

    if request.duplicates {
        let mut resolver = resolver();
        merge_duplicates(&mut session.library, resolver.as_mut(), &session.say)?;
    } else if request.on_duplicate_given {
        log::warning("--on-duplicate and -m/--mode only apply with --duplicates");
    }
    Ok(session.library.entries().map(|e| e.key.clone()).collect())
}

/// `check --duplicates`: the groups merged, one line for each (`merged into KEY: KEYS (kept in ids)`).
pub fn merge_duplicates(
    library: &mut Library,
    resolver: &mut dyn Resolve,
    say: &Say,
) -> Result<()> {
    library.check_duplicates_with(resolver, &mut |target, others| {
        say.line(format!(
            "merged into {target}: {} (kept in ids)",
            others.join(", ")
        ));
    })?;
    Ok(())
}

/// The resolver of the duplicates that `check` takes when no `--on-duplicate` is given
/// (`on_duplicate_mode`): `skip` with `-y/--yes`, else `ask` in a terminal and `error` without one.
pub fn default_duplicate_mode(yes: bool, interactive: bool) -> OnDuplicate {
    if yes {
        OnDuplicate::Skip
    } else if interactive {
        OnDuplicate::Ask
    } else {
        OnDuplicate::Error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_are_rounded_to_even() {
        assert_eq!(py_format_0f(87.5), "88");
        assert_eq!(py_format_0f(86.5), "86");
        assert_eq!(py_format_0f(89.4999), "89");
        assert_eq!(py_format_0f(89.51), "90");
    }
}
