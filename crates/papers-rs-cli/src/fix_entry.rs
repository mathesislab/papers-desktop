//! `papers list --fetch`: the entries refreshed from what the DOI or the title says
//! (`Biblio.fix_entry(e, fix_doi=True, fix_key=True, fetch_all=True, interactive=True)`): author
//! names formatted, the DOI cleaned, the metadata fetched (crossref, arXiv) and put over the
//! fields, a key made if the one it has is no key; for each entry that changed, the diff and the
//! question `update? [Y/n]`.

use std::collections::{BTreeMap, BTreeSet};

use papers_rs::bib::encoding::{family_names, standard_name};
use papers_rs::bib::{Bib, Block, Entry};
use papers_rs::command::Session;
use papers_rs::command::report::content_equal;
use papers_rs::config::dirs::cache_dir;
use papers_rs::entry::val;
use papers_rs::error::Error;
use papers_rs::extract::doi::parse_doi;
use papers_rs::fetch::Fetcher;
use papers_rs::identity::{Identifiers, family_name_ids};
use papers_rs::library::Library;
use papers_rs::log;
use papers_rs::names::is_valid_key;

use crate::cmd_write::Failure;
use crate::difflib::ndiff;

/// `TITLE_MATCH_RATIO`: how alike the titles of the entry and of a search result must be, in per cent.
const TITLE_MATCH_RATIO: f64 = 90.0;

/// `search_result_mismatch`: why a bibliographic search result is not the paper of the entry, or
/// nothing when it is: the normalized titles are equal or score at least the ratio, and the
/// authors have a family name in common.
fn search_result_mismatch(e: &Entry, found: &Entry) -> String {
    let (ta, tb) = (Identifiers::of(e).title, Identifiers::of(found).title);
    if ta.is_empty() || tb.is_empty() {
        return "no title".to_string();
    }
    if ta != tb {
        // rapidfuzz of Rust gives 0 to 1, that of Python 0 to 100
        let score = 100.0 * rapidfuzz::fuzz::ratio(ta.chars(), tb.chars());
        if score < TITLE_MATCH_RATIO {
            return format!("different title ({score:.0}% similar)");
        }
    }
    let names =
        |e: &Entry| -> BTreeSet<String> { family_name_ids(val(e, "author")).into_iter().collect() };
    let common: BTreeSet<String> = names(e).intersection(&names(found)).cloned().collect();
    if common.iter().all(|n| n.is_empty()) {
        return "no author in common".to_string();
    }
    String::new()
}

/// What `fix_entry` is asked to do.
#[derive(Clone, Copy)]
struct Fix {
    fix_doi: bool,
    fetch_all: bool,
    fix_key: bool,
}

/// The entries of this run, refreshed one after the other.
struct Refresher {
    fetcher: Option<Fetcher>,
}

impl Refresher {
    fn fetcher(&mut self) -> Result<&Fetcher, Error> {
        if self.fetcher.is_none() {
            self.fetcher =
                Some(Fetcher::new(&cache_dir()).map_err(|error| Error::other(error.message))?);
        }
        Ok(self.fetcher.as_ref().expect("the fetcher"))
    }

    /// `fix_entry` on one entry of the library (the library is read for a new key, and for nothing else).
    fn fix(&mut self, library: Option<&Library>, e: &mut Entry, fix: Fix) -> Result<(), Error> {
        let name = e.key.clone();
        // the names of authors and editors
        for k in ["author", "editor"] {
            if let Some(old) = e.get(k).map(str::to_string) {
                let new = standard_name(&old);
                if new != old {
                    log::info(&format!("{name}: {k} name formatted"));
                }
                e.set(k, new);
            }
        }
        if fix.fix_doi {
            match e.get("doi").map(str::to_string).filter(|d| !d.is_empty()) {
                Some(old) => {
                    let doi = match parse_doi(&format!("doi:{old}")) {
                        Ok(doi) => doi,
                        Err(_) => {
                            // keep the DOI as it is, and go on with the other fixes
                            log::warning(&format!("{name}: failed to fix doi: {old}"));
                            old.clone()
                        }
                    };
                    if doi.to_lowercase() != old.to_lowercase() {
                        log::info(&format!("{name}: fix doi: {old} ==> {doi}"));
                        e.set("doi", doi);
                    } else {
                        log::debug(&format!("{name}: doi OK"));
                    }
                }
                None => log::debug(&format!("{name}: no DOI")),
            }
        }
        if fix.fetch_all {
            self.fetch(e, &name)?;
        }
        if let Some(library) = library
            && fix.fix_key
            && !is_valid_key(&e.key)
        {
            let key = library.generate_key(e)?;
            if e.key != key {
                log::info(&format!("update key {} => {key}", e.key));
                e.key = key;
            }
        }
        Ok(())
    }

    /// The metadata of the DOI, else of a search by title and authors, over the fields of the entry.
    fn fetch(&mut self, e: &mut Entry, name: &str) -> Result<(), Error> {
        let mut bibtex: Option<String> = None;
        let mut searched = false;
        let doi = e.get("doi").unwrap_or("").to_string();
        let (title, author) = (
            e.get("title").unwrap_or("").to_string(),
            e.get("author").unwrap_or("").to_string(),
        );
        if !doi.is_empty() {
            log::info(&format!("{name}: fetch doi: {doi}"));
            match self.fetcher()?.fetch_bibtex_by_doi(&doi) {
                Ok(text) => bibtex = Some(text),
                Err(error) => log::warning(&format!(
                    "...failed to fetch bibtex (doi): {}",
                    error.message
                )),
            }
        } else if !title.is_empty() && !author.is_empty() {
            let family = family_names(&author).join(" ");
            log::info(&format!(
                "{name}: fetch-all: {{'title': {}, 'author': {}}}",
                papers_rs::config::json::py_repr_str(&title),
                papers_rs::config::json::py_repr_str(&family)
            ));
            match self.fetcher()?.fetch_bibtex_by_fulltext_crossref(
                "",
                Some(&title),
                Some(&family),
                5,
            ) {
                Ok(text) => {
                    bibtex = Some(text);
                    searched = true;
                }
                Err(error) => log::warning(&format!(
                    "...failed to fetch/update bibtex (all): {}",
                    error.message
                )),
            }
        }
        let Some(text) = bibtex.filter(|t| !t.trim().is_empty()) else {
            return Ok(());
        };
        let mut found: Entry = Bib::parse(&text)
            .entries()
            .next()
            .cloned()
            .ok_or_else(|| Error::other("list index out of range"))?;
        // the entry fetched is formatted as well
        self.fix(
            None,
            &mut found,
            Fix {
                fix_doi: true,
                fetch_all: false,
                fix_key: false,
            },
        )?;
        // the best search result may be another paper
        if searched {
            let mismatch = search_result_mismatch(e, &found);
            if !mismatch.is_empty() {
                log::warning(&format!(
                    "...fetch-all: rejected search result ({mismatch}): {} ({})",
                    val(&found, "title"),
                    found.get("doi").unwrap_or("no doi")
                ));
                return Ok(());
            }
        }
        let strip = |x: &Entry, only: Option<&Entry>| -> BTreeMap<String, String> {
            let mut map = BTreeMap::new();
            map.insert("ENTRYTYPE".to_string(), x.entry_type.clone());
            for f in &x.fields {
                if f.key != "file" && only.is_none_or(|o| o.get(&f.key).is_some()) {
                    map.insert(f.key.clone(), f.value.clone());
                }
            }
            map
        };
        let fetched = strip(&found, None);
        if strip(e, Some(&found)) != fetched {
            log::info("...fetch-update entry");
            for (k, v) in fetched {
                if k == "ENTRYTYPE" {
                    e.entry_type = v;
                } else {
                    e.set(&k, v);
                }
            }
        } else {
            log::info("...fetch-update: already up to date");
        }
        Ok(())
    }
}

/// `entry_diff`: the line diff of two entries, as bibtex, with colors (the added lines green, the
/// removed red, the marks yellow).
fn entry_diff(old: &Entry, new: &Entry, color: bool) -> String {
    let text = |e: &Entry| {
        Bib {
            blocks: vec![Block::Entry(e.clone())],
        }
        .format()
    };
    let diff = ndiff(&text(old), &text(new));
    // the lines are joined by line feeds, without a last one, with or without the colors
    let paint = |code: &str, line: &str| {
        if color {
            format!("\x1b[{code}m{line}\x1b[0m")
        } else {
            line.to_string()
        }
    };
    diff.lines()
        .map(|line| match line.chars().next() {
            Some('+') => paint("92", line),
            Some('-') => paint("91", line),
            Some('?') => paint("93", line),
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Refresh the entries of these keys (`--fetch`), asking before each change is kept: the keys the
/// entries have after.
pub fn fetch_entries(session: &mut Session, keys: &[String]) -> Result<Vec<String>, Failure> {
    let mut refresher = Refresher { fetcher: None };
    let fix = Fix {
        fix_doi: true,
        fetch_all: true,
        fix_key: true,
    };
    let mut now = Vec::new();
    for key in keys {
        let Some(old) = session.library.get(key).cloned() else {
            continue;
        };
        let mut e = old.clone();
        refresher.fix(Some(&session.library), &mut e, fix)?;
        if !content_equal(&old, &e) {
            let color = session.color();
            let (blue, end) = if color {
                ("\x1b[94m", "\x1b[0m")
            } else {
                ("", "")
            };
            session.say.line(format!("{blue}*** UPDATE ***{end}"));
            session.say.line(entry_diff(&old, &e, color));
            let answer = session.input("update? [Y/n] ")?.to_lowercase();
            if !matches!(answer.as_str(), "" | "y") {
                log::info("cancel changes");
                e = old.clone();
            }
        }
        now.push(e.key.clone());
        if let Some(slot) = session.library.get_mut(key) {
            *slot = e;
        }
    }
    Ok(now)
}
