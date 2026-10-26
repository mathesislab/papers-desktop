//! The words of a displayed text, folded for matching, and the spans that
//! show which of them a search matched.

use std::collections::BTreeSet;
use std::ops::Range;

use deunicode::deunicode;
use serde::{Deserialize, Serialize};

/// How a query word matches a word of the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Matching {
    /// from the start of the word: "rob" matches "Robinson"
    #[default]
    WordStart,
    /// also inside the word, from three letters: "stab" matches "Multistability"
    InsideWords,
}

impl Matching {
    pub(crate) fn matches(self, word: &str, w: &str) -> bool {
        word.starts_with(w) || (self == Matching::InsideWords && w.len() >= 3 && word.contains(w))
    }
}

/// A piece of a displayed text, marked when a search matched it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Span {
    pub text: String,
    pub mark: bool,
}

impl Span {
    pub(crate) fn new(text: impl Into<String>, mark: bool) -> Span {
        Span {
            text: text.into(),
            mark,
        }
    }

    pub(crate) fn plain(text: impl Into<String>) -> Span {
        Span::new(text, false)
    }
}

/// Lowercase ASCII, accents and ligatures spelt out: "Müller" → "muller".
pub(crate) fn fold(s: &str) -> String {
    deunicode(s).to_lowercase()
}

/// The words of a folded text.
pub(crate) fn words(folded: &str) -> Vec<String> {
    folded
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

#[derive(Debug)]
struct Word {
    range: Range<usize>,
    folded: String,
}

/// A displayed text and its words.
#[derive(Debug)]
pub(crate) struct Text {
    text: String,
    words: Vec<Word>,
}

impl Text {
    pub fn new(text: &str) -> Text {
        let mut words = Vec::new();
        let mut start = None;
        let end = std::iter::once((text.len(), ' '));
        for (i, c) in text.char_indices().chain(end) {
            match (c.is_alphanumeric(), start) {
                (true, None) => start = Some(i),
                (false, Some(s)) => {
                    let folded: String = fold(&text[s..i])
                        .chars()
                        .filter(char::is_ascii_alphanumeric)
                        .collect();
                    if !folded.is_empty() {
                        words.push(Word {
                            range: s..i,
                            folded,
                        });
                    }
                    start = None;
                }
                _ => {}
            }
        }
        Text {
            text: text.to_string(),
            words,
        }
    }

    /// The words that `w` matches.
    pub fn find(&self, w: &str, matching: Matching) -> Vec<usize> {
        let found = self.words.iter().enumerate();
        found
            .filter(|(_, word)| matching.matches(&word.folded, w))
            .map(|(i, _)| i)
            .collect()
    }

    /// The words of each occurrence of a phrase: consecutive words, the last
    /// one matched like a single word (it may be typed in part).
    pub fn find_phrase(&self, phrase: &[String], matching: Matching) -> Vec<usize> {
        let n = phrase.len();
        if n == 0 || n > self.words.len() {
            return Vec::new();
        }
        (0..=self.words.len() - n)
            .filter(|&i| {
                phrase.iter().enumerate().all(|(j, w)| {
                    let word = &self.words[i + j].folded;
                    if j + 1 < n {
                        word == w
                    } else {
                        matching.matches(word, w)
                    }
                })
            })
            .flat_map(|i| i..i + n)
            .collect()
    }

    /// The text in spans, the `marked` words marked.
    pub fn spans(&self, marked: &BTreeSet<usize>) -> Vec<Span> {
        let mut spans = Vec::new();
        let mut at = 0;
        for range in marked.iter().map(|&i| &self.words[i].range) {
            if range.start > at {
                spans.push(Span::plain(&self.text[at..range.start]));
            }
            spans.push(Span::new(&self.text[range.clone()], true));
            at = range.end;
        }
        if at < self.text.len() {
            spans.push(Span::plain(&self.text[at..]));
        }
        spans
    }
}
