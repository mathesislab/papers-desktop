//! The query: words, "phrases" and field filters, every term to match.
//!
//! `robinson "ice sheet" j:cryosphere year:2010-2015`. A filter is a field
//! name and a colon before a word or a phrase; an unknown name is no filter.

use crate::text::{fold, words};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    Any,
    Author,
    /// the first author only
    First,
    Title,
    Year,
    /// journal or book title
    Journal,
    Key,
    Doi,
    /// keywords
    Tag,
}

impl Field {
    fn named(name: &str) -> Option<Field> {
        Some(match name.to_ascii_lowercase().as_str() {
            "author" | "a" => Field::Author,
            "first" | "f" => Field::First,
            "title" | "t" => Field::Title,
            "year" | "y" => Field::Year,
            "journal" | "j" => Field::Journal,
            "key" | "k" => Field::Key,
            "doi" => Field::Doi,
            "tag" => Field::Tag,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Pattern {
    Word(String),
    /// consecutive words
    Phrase(Vec<String>),
    /// a DOI or part of one, anywhere in the DOI (or in a key made of a DOI)
    Doi(String),
    /// the years from..=to, open-ended without a bound
    Years(Option<i32>, Option<i32>),
    /// the first digits of a year: "201" for the 2010s
    YearStart(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Term {
    pub field: Field,
    pub pattern: Pattern,
}

pub(crate) fn parse(query: &str) -> Vec<Term> {
    tokens(query)
        .into_iter()
        .flat_map(|(field, value, quoted)| terms(field, &fold(&value), quoted))
        .collect()
}

/// The (field, value, quoted) tokens of a query.
fn tokens(query: &str) -> Vec<(Field, String, bool)> {
    let chars: Vec<char> = query.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let mut field = Field::Any;
        let name_end = i + chars[i..]
            .iter()
            .take_while(|c| c.is_ascii_alphabetic())
            .count();
        if chars.get(name_end) == Some(&':') {
            let name: String = chars[i..name_end].iter().collect();
            if let Some(named) = Field::named(&name) {
                field = named;
                i = name_end + 1;
            }
        }
        let quoted = chars.get(i) == Some(&'"');
        let (start, end) = if quoted {
            let close = chars[i + 1..].iter().position(|&c| c == '"');
            (i + 1, close.map_or(chars.len(), |p| i + 1 + p))
        } else {
            let space = chars[i..].iter().position(|c| c.is_whitespace());
            (i, space.map_or(chars.len(), |p| i + p))
        };
        tokens.push((field, chars[start..end].iter().collect(), quoted));
        i = if quoted { end + 1 } else { end };
    }
    tokens
}

fn terms(field: Field, value: &str, quoted: bool) -> Vec<Term> {
    let value = value.trim();
    let term = |pattern| Term { field, pattern };
    if value.is_empty() {
        return Vec::new(); // a filter still being typed
    }
    match field {
        Field::Year => vec![term(years(value))],
        Field::Key => vec![term(Pattern::Word(value.to_string()))],
        Field::Doi => vec![term(Pattern::Doi(doi(value).to_string()))],
        Field::Any if !quoted && is_doi(value) => vec![term(Pattern::Doi(doi(value).to_string()))],
        _ => {
            let words = words(value);
            if quoted && words.len() > 1 {
                vec![term(Pattern::Phrase(words))]
            } else {
                words.into_iter().map(|w| term(Pattern::Word(w))).collect()
            }
        }
    }
}

/// `2012`, `2010-2015`, `2020-`, `-1990`, or the start of a year (`201`);
/// anything else matches no year.
fn years(value: &str) -> Pattern {
    let bound = |s: &str| {
        if s.is_empty() {
            Ok(None)
        } else {
            s.parse().map(Some)
        }
    };
    match value.split_once('-') {
        Some((from, to)) => match (bound(from), bound(to)) {
            (Ok(from), Ok(to)) => Pattern::Years(from, to),
            _ => Pattern::YearStart(value.to_string()),
        },
        None => match value.parse() {
            Ok(year) if value.len() == 4 => Pattern::Years(Some(year), Some(year)),
            _ => Pattern::YearStart(value.to_string()),
        },
    }
}

fn is_doi(value: &str) -> bool {
    value.contains('/') || value.starts_with("10.")
}

/// A DOI, or a doi.org link reduced to its DOI.
fn doi(value: &str) -> &str {
    value.rsplit_once("doi.org/").map_or(value, |(_, doi)| doi)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(field: Field, w: &str) -> Term {
        Term {
            field,
            pattern: Pattern::Word(w.into()),
        }
    }

    #[test]
    fn words_and_filters() {
        assert_eq!(
            parse("Robinson a:Calov first:van-der"),
            [
                word(Field::Any, "robinson"),
                word(Field::Author, "calov"),
                word(Field::First, "van"),
                word(Field::First, "der"),
            ]
        );
        // an unknown field is a plain word; an empty filter is nothing yet
        assert_eq!(
            parse("foo:bar author:"),
            [word(Field::Any, "foo"), word(Field::Any, "bar")]
        );
    }

    #[test]
    fn phrases() {
        let phrase = |field| Term {
            field,
            pattern: Pattern::Phrase(vec!["ice".into(), "sheet".into()]),
        };
        assert_eq!(
            parse(r#"t:"Ice sheet" "ice sheet""#),
            [phrase(Field::Title), phrase(Field::Any)]
        );
        // still being typed
        assert_eq!(parse(r#"title:"ice sheet"#), [phrase(Field::Title)]);
        assert_eq!(parse(r#""ice""#), [word(Field::Any, "ice")]);
    }

    #[test]
    fn years_and_dois() {
        let y = |pattern| Term {
            field: Field::Year,
            pattern,
        };
        assert_eq!(parse("y:2012"), [y(Pattern::Years(Some(2012), Some(2012)))]);
        assert_eq!(
            parse("year:2010-2015"),
            [y(Pattern::Years(Some(2010), Some(2015)))]
        );
        assert_eq!(parse("year:2020-"), [y(Pattern::Years(Some(2020), None))]);
        assert_eq!(parse("year:-1990"), [y(Pattern::Years(None, Some(1990)))]);
        assert_eq!(parse("y:201"), [y(Pattern::YearStart("201".into()))]);
        let doi = |field, d: &str| Term {
            field,
            pattern: Pattern::Doi(d.into()),
        };
        assert_eq!(
            parse("https://doi.org/10.1038/X"),
            [doi(Field::Any, "10.1038/x")]
        );
        assert_eq!(
            parse("10.5194 doi:nclimate"),
            [doi(Field::Any, "10.5194"), doi(Field::Doi, "nclimate")]
        );
    }
}
