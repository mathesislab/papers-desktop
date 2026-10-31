//! A library entry as the panel shows and searches it.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use biblatex::{DateValue, Entry, PermissiveType};

use crate::bib::BibEntry;
use crate::fields::{Name, clean_doi, field, names, raw};
use crate::record::Marks;
use crate::text::{Span, Text};

/// How a citation of an item is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cite {
    /// `robinson2012`
    Key,
    /// `[@robinson2012]` (pandoc, texmark)
    Pandoc,
    /// `\cite{robinson2012}`
    Latex,
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    /// the key, unless another .bib file of the library has the same key
    pub id: String,
    pub key: String,
    /// bibtex entry type, e.g. `article`
    pub kind: String,
    pub title: String,
    /// display names, "Given Family"
    pub authors: Vec<String>,
    pub year: Option<i32>,
    /// journal or book title
    pub container: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
    pub keywords: Vec<String>,
    pub files: Vec<PathBuf>,
    /// the .bib file of the entry
    pub bib: PathBuf,
    /// one line under the title: "Feldmann & Levermann · 2017 · Cryosphere"
    pub subtitle: String,
    /// family names with their particle ("van den Broeke"), in author order
    #[serde(skip)]
    pub(crate) families: Vec<String>,
}

impl Item {
    pub fn cite(&self, style: Cite) -> String {
        match style {
            Cite::Key => self.key.clone(),
            Cite::Pandoc => format!("[@{}]", self.key),
            Cite::Latex => format!("\\cite{{{}}}", self.key),
        }
    }

    /// The web page of the item: its DOI link, else its URL.
    pub fn link(&self) -> Option<String> {
        match (&self.doi, &self.url) {
            (Some(doi), _) => Some(format!("https://doi.org/{doi}")),
            (None, url) => url.clone(),
        }
    }

    /// The item of an entry of a .bib file; `id` is unique in the library.
    pub(crate) fn from_entry(e: &BibEntry, id: String) -> Item {
        let entry = &e.entry;
        let people = people(entry);
        let key = entry.key.clone();
        let title = field(entry, &["title"]).unwrap_or_else(|| key.clone());
        let container = field(entry, &["journal", "journaltitle", "booktitle"]);
        let keywords = field(entry, &["keywords"]).unwrap_or_default();
        let mut item = Item {
            id,
            kind: entry.entry_type.to_string(),
            title,
            authors: people.iter().map(display_name).collect(),
            year: year(entry),
            container,
            doi: raw(entry, "doi").map(clean_doi),
            url: raw(entry, "url"),
            keywords: keywords
                .split(',')
                .map(str::trim)
                .filter(|w| !w.is_empty())
                .map(String::from)
                .collect(),
            files: e.files.clone(),
            bib: e.bib.clone(),
            subtitle: String::new(),
            families: people.iter().map(Name::full_family).collect(),
            key,
        };
        let spans = item.subtitle_spans(&Marks::default());
        item.subtitle = spans.into_iter().map(|s| s.text).collect();
        item
    }

    /// The subtitle in spans, the matched words marked; "et al." is marked
    /// when one of the co-authors it stands for matched.
    pub(crate) fn subtitle_spans(&self, marks: &Marks) -> Vec<Span> {
        let author = |a: usize| {
            let words: BTreeSet<usize> = marks
                .authors
                .iter()
                .filter(|(x, _)| *x == a)
                .map(|(_, w)| *w)
                .collect();
            Text::new(&self.families[a]).spans(&words)
        };
        let names = match self.families.len() {
            0 => None,
            1 => Some(author(0)),
            2 => Some([author(0), vec![Span::plain(" & ")], author(1)].concat()),
            _ => {
                let others = marks.authors.iter().any(|&(a, _)| a > 0);
                Some(
                    [
                        author(0),
                        vec![Span::plain(" "), Span::new("et al.", others)],
                    ]
                    .concat(),
                )
            }
        };
        let year = self
            .year
            .map(|y| vec![Span::new(y.to_string(), marks.year)]);
        let container = self
            .container
            .as_deref()
            .map(|c| Text::new(c).spans(&marks.container));
        let parts: Vec<Vec<Span>> = [names, year, container].into_iter().flatten().collect();
        parts.join(&Span::plain(" · "))
    }
}

/// The authors, else the editors.
fn people(entry: &Entry) -> Vec<Name> {
    let authors = names(entry, "author");
    if authors.is_empty() {
        names(entry, "editor")
    } else {
        authors
    }
}

/// "Given Family"
fn display_name(name: &Name) -> String {
    format!("{} {}", name.given, name.full_family())
        .trim()
        .to_string()
}

/// The year of the date field, else the first four-digit number of the year field.
fn year(entry: &Entry) -> Option<i32> {
    if let Ok(PermissiveType::Typed(date)) = entry.date() {
        let (DateValue::At(d)
        | DateValue::After(d)
        | DateValue::Before(d)
        | DateValue::Between(d, _)) = date.value;
        return Some(d.year);
    }
    let year = field(entry, &["year"])?;
    year.split(|c: char| !c.is_ascii_digit())
        .find(|w| w.len() == 4)
        .and_then(|w| w.parse().ok())
}
