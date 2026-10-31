//! The journals to subscribe to: those of the library's papers, counted by
//! their titles folded; OpenAlex's journals, searched by name, those looked
//! up kept in `journals.json`; and what follows each.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::http::{Access, Http, get_json};
use crate::normalize::{canonical_issn, fold};
use crate::openalex::{self, Source, Sources};
use crate::store::{self, JOURNALS};
use crate::suggested::{Suggested, suggested_journal};

/// The fewest papers of a journal of the library suggested.
pub const LEAST_PAPERS: usize = 3;

/// The most journals of the library suggested.
pub const MOST_JOURNALS: usize = 15;

/// An ISSN in a text, with or without its hyphen.
static ISSN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{4}-?\d{3}[\dXx]\b").unwrap());

/// A journal's title as compared: its braces out, folded as [`fold`] (case,
/// accents, punctuation, spaces).
pub fn journal_key(title: &str) -> String {
    fold(&title.replace(['{', '}'], ""))
}

/// The ISSNs of a text (`0022-1430, 1727-5652`), canonical.
fn issns_in(text: &str) -> Vec<String> {
    ISSN.find_iter(text)
        .filter_map(|m| canonical_issn(m.as_str()))
        .collect()
}

/// A journal of the library's papers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryJournal {
    /// as most of its papers write it
    pub title: String,
    pub papers: usize,
    /// the ISSNs its papers give, canonical, the most given first
    pub issns: Vec<String>,
}

/// A journal's papers, as counted.
#[derive(Default)]
struct Counted {
    papers: usize,
    /// each way its title is written, how often, and when first
    titles: HashMap<String, (usize, usize)>,
    issns: HashMap<String, (usize, usize)>,
}

/// Counts one more, first seen at `at`.
fn count(counts: &mut HashMap<String, (usize, usize)>, key: String, at: usize) {
    counts.entry(key).or_insert((0, at)).0 += 1;
}

/// The keys, the most counted first, then the first seen.
fn most_first(counts: HashMap<String, (usize, usize)>) -> Vec<String> {
    let mut counts: Vec<(String, (usize, usize))> = counts.into_iter().collect();
    counts.sort_by_key(|(_, (n, at))| (std::cmp::Reverse(*n), *at));
    counts.into_iter().map(|(key, _)| key).collect()
}

/// The journals of the library's papers, from each paper's journal and ISSN
/// field: those of [`LEAST_PAPERS`] or more, the most papers first (then by
/// title), [`MOST_JOURNALS`] at most.
pub fn library_journals<'a>(
    papers: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
) -> Vec<LibraryJournal> {
    let mut journals: HashMap<String, Counted> = HashMap::new();
    for (at, (title, issn)) in papers.into_iter().enumerate() {
        let key = journal_key(title);
        if key.is_empty() {
            continue;
        }
        let counted = journals.entry(key).or_default();
        counted.papers += 1;
        let written = title.replace(['{', '}'], "").trim().to_string();
        count(&mut counted.titles, written, at);
        for issn in issn.map(issns_in).unwrap_or_default() {
            count(&mut counted.issns, issn, at);
        }
    }
    let mut journals: Vec<(String, LibraryJournal)> = journals
        .into_iter()
        .filter(|(_, counted)| counted.papers >= LEAST_PAPERS)
        .map(|(key, counted)| {
            let journal = LibraryJournal {
                title: most_first(counted.titles).swap_remove(0),
                papers: counted.papers,
                issns: most_first(counted.issns),
            };
            (key, journal)
        })
        .collect();
    journals.sort_by(|(a, ja), (b, jb)| jb.papers.cmp(&ja.papers).then_with(|| a.cmp(b)));
    journals.truncate(MOST_JOURNALS);
    journals.into_iter().map(|(_, journal)| journal).collect()
}

/// A journal, as OpenAlex knows it, or as the library's papers give its ISSN.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    pub title: String,
    /// the ISSN its works are swept by in Crossref: its linking ISSN, else its first
    pub issn: Option<String>,
    /// every ISSN of it, print and electronic, canonical
    #[serde(default)]
    pub issns: Vec<String>,
    pub publisher: Option<String>,
    /// its home page
    pub homepage: Option<String>,
    /// its works in OpenAlex
    pub works: Option<u64>,
}

impl Journal {
    /// The journal of a title and the ISSNs its papers give, the first swept.
    fn of_issns(title: &str, issns: &[String]) -> Journal {
        Journal {
            title: title.to_string(),
            issn: issns.first().cloned(),
            issns: issns.to_vec(),
            publisher: None,
            homepage: None,
            works: None,
        }
    }
}

impl From<Source> for Journal {
    fn from(source: Source) -> Journal {
        let mut issns: Vec<String> = Vec::new();
        let all = source.issn_l.iter().chain(source.issn.iter().flatten());
        for issn in all.filter_map(|issn| canonical_issn(issn)) {
            if !issns.contains(&issn) {
                issns.push(issn);
            }
        }
        let blank = |text: Option<String>| text.filter(|t| !t.trim().is_empty());
        Journal {
            title: source.display_name,
            issn: issns.first().cloned(),
            issns,
            publisher: blank(source.host_organization_name),
            homepage: blank(source.homepage_url),
            works: source.works_count,
        }
    }
}

/// The journals of an OpenAlex search, the best first.
fn search(http: &dyn Http, access: &Access, text: &str) -> Result<Vec<Source>, String> {
    let url = openalex::journals_url(text, access);
    let sources: Option<Sources> = get_json(http, &url)?;
    Ok(sources.unwrap_or_default().results)
}

/// The journal of a name in OpenAlex: of its search, the first whose title
/// or an alternate title folds to the name's, one with an ISSN before one
/// without; none if no title is the name.
fn journal_named(http: &dyn Http, access: &Access, name: &str) -> Result<Option<Journal>, String> {
    let key = journal_key(name);
    let named = |s: &Source| {
        let mut titles =
            std::iter::once(&s.display_name).chain(s.alternate_titles.iter().flatten());
        titles.any(|title| journal_key(title) == key)
    };
    let found = search(http, access, name)?.into_iter().filter(named);
    let (with_issn, without): (Vec<Journal>, Vec<Journal>) =
        found.map(Journal::from).partition(|j| j.issn.is_some());
    Ok(with_issn.into_iter().chain(without).next())
}

/// What follows a journal of the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Follow {
    /// a subscription of the suggested list: the same title or ISSN
    Suggested(Suggested),
    /// a sweep of Crossref by its ISSN, if it has one
    Journal(Journal),
    /// OpenAlex knows no journal of its name
    Unknown,
    /// not looked up yet
    Pending,
}

impl Follow {
    /// What follows a journal found: its suggestion, else its ISSN.
    fn of(journal: &Journal) -> Follow {
        match suggested_journal(&journal.title, &journal.issns) {
            Some(suggested) => Follow::Suggested(suggested),
            None => Follow::Journal(journal.clone()),
        }
    }
}

/// `journals.json` of a feeds folder: the journals looked up in OpenAlex by
/// name, under the name folded ([`journal_key`]); null for none found. A
/// journal is asked for once.
pub struct Journals {
    dir: PathBuf,
    known: BTreeMap<String, Option<Journal>>,
}

impl Journals {
    /// The journals looked up of a feeds folder (created if it is not there).
    pub fn open(dir: &Path) -> crate::Result<Journals> {
        let dir = store::folder(dir)?;
        Ok(Journals {
            known: store::read(&dir, JOURNALS)?,
            dir,
        })
    }

    /// What follows a journal of the library, from what is known: the
    /// suggestion of the same title or ISSN, a sweep by the ISSN its papers
    /// give, what its lookup found; pending if it was not looked up.
    pub fn follow(&self, journal: &LibraryJournal) -> Follow {
        if let Some(suggested) = suggested_journal(&journal.title, &journal.issns) {
            return Follow::Suggested(suggested);
        }
        if !journal.issns.is_empty() {
            return Follow::Journal(Journal::of_issns(&journal.title, &journal.issns));
        }
        match self.known.get(&journal_key(&journal.title)) {
            None => Follow::Pending,
            Some(None) => Follow::Unknown,
            Some(Some(found)) => Follow::of(found),
        }
    }

    /// What follows a journal of a name, looked up in OpenAlex unless it was:
    /// the suggestion of the journal found, else its ISSN; unknown if none
    /// was found. What OpenAlex answers is kept; a request without an answer
    /// is an error, and asked again the next time.
    pub fn look_up(
        &mut self,
        name: &str,
        http: &dyn Http,
        access: &Access,
    ) -> Result<Follow, String> {
        let key = journal_key(name);
        let found = match self.known.get(&key) {
            Some(found) => found.clone(),
            None => {
                let found = journal_named(http, access, name)?;
                self.known.insert(key, found.clone());
                store::write(&self.dir, JOURNALS, &self.known).map_err(|e| e.to_string())?;
                found
            }
        };
        Ok(found.as_ref().map_or(Follow::Unknown, Follow::of))
    }
}
