//! Interests: named profiles of weighted terms that score an item by its
//! title and abstract, as paper-feeder does. A term matches whole, in any
//! case, its inner spaces as spaces or hyphens (`ice sheet` matches
//! `ice-sheet`); a pattern is a regex, in any case. An include counts once
//! per field: its weight times the title boost in the title, its weight in
//! the abstract. An exclude in either vetoes: the score is 0.
//!
//! Scores are computed when asked, not stored: the includes of an interest
//! are one `RegexSet`, one pass over each text, so scoring every item is
//! cheap, and an edited interest or an enriched abstract counts at once,
//! with no cache to keep in step.

use std::ops::Range;

use regex::{Regex, RegexBuilder, RegexSet, RegexSetBuilder};
use serde::{Deserialize, Serialize};

use crate::feeds::Feeds;
use crate::model::Item;
use crate::store::{self, CONFIG};
use crate::{Error, Result};

/// A named profile of weighted terms: what it is about, and what it is not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Interest {
    pub name: String,
    #[serde(default)]
    pub include: Vec<Weighted>,
    /// a match in the title or the abstract vetoes the item
    #[serde(default)]
    pub exclude: Vec<Matcher>,
    /// the score from which an item is of the interest (`prof:`)
    #[serde(default = "min_score")]
    pub min_score: f64,
}

/// paper-feeder's `publish_min_score`.
fn min_score() -> f64 {
    3.0
}

/// A term or a pattern, and its weight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weighted {
    #[serde(flatten)]
    pub matcher: Matcher,
    #[serde(default = "weight")]
    pub weight: f64,
    /// what a match is shown as; the term or the pattern if none
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

fn weight() -> f64 {
    1.0
}

impl Weighted {
    /// What a match is shown as.
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(match &self.matcher {
            Matcher::Term(s) | Matcher::Pattern(s) => s,
        })
    }
}

/// What matches in a text: `{"term": "ice sheet"}` or `{"pattern": "\\bGIA\\b"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Matcher {
    /// a word or phrase, whole, in any case; its inner spaces match spaces and hyphens
    Term(String),
    /// a regex, in any case
    Pattern(String),
}

impl Matcher {
    /// The regex: a term's words escaped, `[\s-]+` between them, `\b` around.
    fn regex(&self) -> std::result::Result<String, String> {
        match self {
            Matcher::Term(term) => {
                let words: Vec<String> = term.split_whitespace().map(regex::escape).collect();
                match words.is_empty() {
                    true => Err("an empty term".to_string()),
                    false => Ok(format!(r"\b{}\b", words.join(r"[\s-]+"))),
                }
            }
            Matcher::Pattern(p) if p.is_empty() => Err("an empty pattern".to_string()),
            Matcher::Pattern(p) => Ok(p.clone()),
        }
    }
}

/// The score of an item in an interest, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Score {
    /// the interest's name
    pub interest: String,
    /// 0 when vetoed
    pub score: f64,
    /// the labels of the includes that matched, in the interest's order
    pub terms: Vec<String>,
    /// an exclude matched
    pub excluded: bool,
    /// not vetoed, and the interest's `min_score` or more: the item is of it
    pub reached: bool,
}

/// An include, compiled.
#[derive(Debug, Clone)]
struct Term {
    regex: Regex,
    weight: f64,
    label: String,
}

/// An interest compiled, to score texts.
#[derive(Debug, Clone)]
pub struct Lexicon {
    name: String,
    /// the regexes of `terms`, in their order
    include: RegexSet,
    terms: Vec<Term>,
    exclude: RegexSet,
    title_boost: f64,
    min_score: f64,
}

impl Lexicon {
    /// An interest compiled; an error names the term or pattern that is no regex.
    pub fn new(interest: &Interest, title_boost: f64) -> Result<Lexicon> {
        let failed = |message: String| Error::Interest {
            name: interest.name.clone(),
            message,
        };
        let regex = |m: &Matcher| m.regex().map_err(failed);
        let includes: Vec<String> = interest
            .include
            .iter()
            .map(|w| regex(&w.matcher))
            .collect::<Result<_>>()?;
        let excludes: Vec<String> = interest.exclude.iter().map(regex).collect::<Result<_>>()?;
        let set = |sources: &[String]| {
            RegexSetBuilder::new(sources)
                .case_insensitive(true)
                .build()
                .map_err(|e| failed(e.to_string()))
        };
        let terms = interest.include.iter().zip(&includes).map(|(w, source)| {
            let regex = RegexBuilder::new(source).case_insensitive(true).build();
            Ok(Term {
                regex: regex.map_err(|e| failed(e.to_string()))?,
                weight: w.weight,
                label: w.label().to_string(),
            })
        });
        Ok(Lexicon {
            name: interest.name.clone(),
            include: set(&includes)?,
            terms: terms.collect::<Result<_>>()?,
            exclude: set(&excludes)?,
            title_boost,
            min_score: interest.min_score,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The score of a title and an abstract (paper-feeder's `score_record`).
    pub fn score(&self, title: &str, r#abstract: &str) -> Score {
        let excluded = self.exclude.is_match(title) || self.exclude.is_match(r#abstract);
        let mut score = 0.0;
        let mut terms = Vec::new();
        if !excluded {
            let (in_title, in_abstract) = (
                self.include.matches(title),
                self.include.matches(r#abstract),
            );
            for (i, term) in self.terms.iter().enumerate() {
                let (t, a) = (in_title.matched(i), in_abstract.matched(i));
                score += if t {
                    term.weight * self.title_boost
                } else {
                    0.0
                };
                score += if a { term.weight } else { 0.0 };
                if t || a {
                    terms.push(term.label.clone());
                }
            }
        }
        Score {
            interest: self.name.clone(),
            score,
            terms,
            excluded,
            reached: !excluded && score >= self.min_score,
        }
    }

    /// Where the includes match in a text, in order, overlapping ones joined.
    pub fn ranges(&self, text: &str) -> Vec<Range<usize>> {
        let matched = self.include.matches(text);
        let found = matched
            .iter()
            .flat_map(|i| self.terms[i].regex.find_iter(text));
        joined(found.map(|m| m.range()).collect())
    }
}

/// Ranges sorted, the overlapping and touching ones joined.
pub(crate) fn joined(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|r| (r.start, r.end));
    let mut out: Vec<Range<usize>> = Vec::new();
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

/// The interests compiled, in their order; their names are distinct.
pub(crate) fn compile(interests: &[Interest], title_boost: f64) -> Result<Vec<Lexicon>> {
    for (i, interest) in interests.iter().enumerate() {
        if interests[..i]
            .iter()
            .any(|other| other.name == interest.name)
        {
            return Err(Error::Interest {
                name: interest.name.clone(),
                message: "a second interest of this name".to_string(),
            });
        }
    }
    interests
        .iter()
        .map(|i| Lexicon::new(i, title_boost))
        .collect()
}

impl Feeds {
    pub fn interests(&self) -> &[Interest] {
        &self.config.interests
    }

    /// The factor of a term's weight in a title.
    pub fn title_boost(&self) -> f64 {
        self.config.title_boost
    }

    /// Sets the interests and the title boost, if every interest compiles.
    pub fn set_interests(&mut self, interests: Vec<Interest>, title_boost: f64) -> Result<()> {
        self.lexicons = compile(&interests, title_boost)?;
        self.config.interests = interests;
        self.config.title_boost = title_boost;
        store::write(&self.dir, CONFIG, &self.config)
    }

    /// The score of an item in each interest, in their order: by its title
    /// and its summary.
    pub fn scores(&self, item: &Item) -> Vec<Score> {
        let r#abstract = item.summary.as_deref().unwrap_or("");
        self.lexicons
            .iter()
            .map(|l| l.score(&item.title, r#abstract))
            .collect()
    }

    /// The interests compiled, in their order.
    pub fn lexicons(&self) -> &[Lexicon] {
        &self.lexicons
    }
}
