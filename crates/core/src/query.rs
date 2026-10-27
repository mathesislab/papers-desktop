//! The query: words, "phrases" and field filters, every term to match.
//!
//! `robinson "ice sheet" j:cryosphere year:2010-2015 -review`. A filter is a
//! field name and a colon before a word or a phrase; an unknown name is no
//! filter. A leading `-` excludes the papers that match the term. `tag:` takes
//! a whole tag, in which `*` stands for any run of characters.

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
    /// a field the entry has: `has:abstract`, or `has:pdf` for a linked file
    Has,
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
            "has" => Field::Has,
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
    /// a whole tag, as the pieces between its `*` wildcards: one piece, no wildcard
    Tag(Vec<String>),
}

impl Pattern {
    /// Whether a year pattern takes `year`; the other patterns take none.
    pub fn matches_year(&self, year: i32) -> bool {
        match self {
            Pattern::Years(from, to) => {
                from.is_none_or(|f| year >= f) && to.is_none_or(|t| year <= t)
            }
            Pattern::YearStart(start) => year.to_string().starts_with(start.as_str()),
            _ => false,
        }
    }

    /// Whether a tag pattern takes a folded `tag`, as a whole; the other
    /// patterns take none.
    pub fn matches_tag(&self, tag: &str) -> bool {
        let Pattern::Tag(pieces) = self else {
            return false;
        };
        let [first, middle @ .., last] = pieces.as_slice() else {
            return pieces.first().is_some_and(|p| p == tag); // no wildcard
        };
        let Some(mut rest) = tag.strip_prefix(first.as_str()) else {
            return false;
        };
        for piece in middle {
            match rest.find(piece.as_str()) {
                Some(at) => rest = &rest[at + piece.len()..],
                None => return false,
            }
        }
        rest.ends_with(last.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Term {
    pub field: Field,
    pub pattern: Pattern,
    /// the papers that match are excluded
    pub negated: bool,
}

struct Token {
    negated: bool,
    field: Field,
    value: String,
    quoted: bool,
}

pub(crate) fn parse(query: &str) -> Vec<Term> {
    tokens(query).iter().flat_map(terms).collect()
}

fn tokens(query: &str) -> Vec<Token> {
    let chars: Vec<char> = query.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let negated = chars[i] == '-';
        if negated {
            i += 1;
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
        let value = chars[start..end].iter().collect();
        tokens.push(Token {
            negated,
            field,
            value,
            quoted,
        });
        i = if quoted { end + 1 } else { end };
    }
    tokens
}

fn terms(token: &Token) -> Vec<Term> {
    let Token {
        negated,
        field,
        quoted,
        ..
    } = *token;
    let folded = fold(&token.value);
    let value = folded.trim();
    let term = |pattern| Term {
        field,
        pattern,
        negated,
    };
    if value.is_empty() {
        return Vec::new(); // a filter still being typed
    }
    match field {
        Field::Year => vec![term(years(value))],
        Field::Key | Field::Has => vec![term(Pattern::Word(value.to_string()))],
        Field::Doi => vec![term(Pattern::Doi(doi(value).to_string()))],
        Field::Tag => vec![term(Pattern::Tag(
            value.split('*').map(String::from).collect(),
        ))],
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
            negated: false,
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
        assert_eq!(parse("has:PDF"), [word(Field::Has, "pdf")]);
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
            negated: false,
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
            negated: false,
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
            negated: false,
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

    #[test]
    fn tags() {
        let tag = |pieces: &[&str], negated| Term {
            field: Field::Tag,
            pattern: Pattern::Tag(pieces.iter().map(|p| p.to_string()).collect()),
            negated,
        };
        assert_eq!(
            parse(r#"tag:AMOC tag:amoc* tag:*Amoc tag:a*c -tag:"Mass Balance" tag:"#),
            [
                tag(&["amoc"], false),
                tag(&["amoc", ""], false),
                tag(&["", "amoc"], false),
                tag(&["a", "c"], false),
                tag(&["mass balance"], true),
            ]
        );
    }

    #[test]
    fn negation() {
        let not = |field, pattern| Term {
            field,
            pattern,
            negated: true,
        };
        assert_eq!(
            parse(r#"-review -a:smith -"ice sheet" -y:2012 y:-1990 -"#),
            [
                not(Field::Any, Pattern::Word("review".into())),
                not(Field::Author, Pattern::Word("smith".into())),
                not(
                    Field::Any,
                    Pattern::Phrase(vec!["ice".into(), "sheet".into()])
                ),
                not(Field::Year, Pattern::Years(Some(2012), Some(2012))),
                Term {
                    field: Field::Year,
                    pattern: Pattern::Years(None, Some(1990)),
                    negated: false,
                },
            ]
        );
    }

    #[test]
    fn years_match() {
        assert!(Pattern::Years(Some(2010), None).matches_year(2026));
        assert!(!Pattern::Years(None, Some(2010)).matches_year(2011));
        assert!(Pattern::YearStart("19".into()).matches_year(1990));
        assert!(!Pattern::Word("2012".into()).matches_year(2012));
    }

    #[test]
    fn tags_match() {
        let tag = |pattern: &str, tag: &str| {
            let terms = parse(&format!("tag:{pattern}"));
            terms[0].pattern.matches_tag(tag)
        };
        // the whole tag, not the start of a word
        assert!(tag("amoc", "amoc"));
        assert!(!tag("ice", "ice-review"));
        assert!(!tag("amoc", "amoc-paper"));
        assert!(!tag("amoc", "paper-amoc"));
        // wildcards: prefix, suffix, contains, inside
        assert!(tag("amoc*", "amoc") && tag("amoc*", "amoc-paper"));
        assert!(!tag("amoc*", "paper-amoc"));
        assert!(tag("*amoc", "amoc") && tag("*amoc", "paper-amoc"));
        assert!(!tag("*amoc", "amoc-paper"));
        assert!(tag("*amoc*", "my-amoc-paper"));
        assert!(!tag("*amoc*", "amo"));
        assert!(tag("a*c", "ac") && tag("a*c", "amoc"));
        assert!(!tag("a*c", "amoc2"));
        assert!(tag("a*b*c", "a-b-c") && !tag("a*b*c", "a-c-b"));
        // the ends of a wildcard do not overlap
        assert!(!tag("ab*ba", "aba"));
        assert!(tag("*", "anything"));
        assert!(!Pattern::Word("amoc".into()).matches_tag("amoc"));
    }
}
