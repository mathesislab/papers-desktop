//! A library entry as the panel shows and searches it.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::csl;
use crate::search::Marks;
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
    pub key: String,
    /// CSL type, e.g. `article-journal`
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

    pub(crate) fn from_csl(e: csl::Entry) -> Item {
        let names = if e.author.is_empty() {
            &e.editor
        } else {
            &e.author
        };
        let families: Vec<String> = names.iter().filter_map(family).collect();
        let authors = names.iter().filter_map(display_name).collect();
        let year = e.issued.as_ref().and_then(csl::Date::year);
        let container = e
            .container_title
            .as_deref()
            .map(clean)
            .filter(|s| !s.is_empty());
        let key = e.citation_key.unwrap_or(e.id);
        let title = e.title.as_deref().map(clean).unwrap_or_default();
        let keywords = e
            .keyword
            .as_deref()
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .map(String::from)
            .collect();
        let mut item = Item {
            title: if title.is_empty() { key.clone() } else { title },
            key,
            kind: e.kind,
            authors,
            year,
            container,
            doi: e.doi.filter(|s| !s.is_empty()),
            url: e.url.filter(|s| !s.is_empty()),
            keywords,
            files: e.custom.file.into_iter().map(PathBuf::from).collect(),
            subtitle: String::new(),
            families,
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

fn family(n: &csl::Name) -> Option<String> {
    let family = match (&n.particle, &n.family, &n.literal) {
        (Some(p), Some(f), _) => format!("{p} {f}"),
        (None, Some(f), _) => f.clone(),
        (_, None, Some(l)) => l.clone(),
        _ => return None,
    };
    Some(clean(&family)).filter(|s| !s.is_empty())
}

fn display_name(n: &csl::Name) -> Option<String> {
    let family = family(n)?;
    Some(match n.given.as_deref().map(clean) {
        Some(given) if !given.is_empty() => format!("{given} {family}"),
        _ => family,
    })
}

/// Strip the bibtex braces left in a field and collapse whitespace.
fn clean(s: &str) -> String {
    s.replace(['{', '}'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
