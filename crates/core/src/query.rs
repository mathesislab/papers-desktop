//! The query: words, "phrases" and field filters, every term to match.
//!
//! `robinson "ice sheet" j:cryosphere year:2010-2015 -review`. A filter is a
//! field name and a colon before a word or a phrase; an unknown name is no
//! filter. A leading `-` excludes the papers that match the term. `tag:` takes
//! a whole tag, in which `*` stands for any run of characters; it finds the
//! tags under it too (`tag:climate` finds `climate/amoc`).
//!
//! Besides the fields of the library, a [`Schema`] declares facets, which
//! take a whole value as `tag:` does (`kind:paper`, `src:"The Cryosphere"`),
//! and numbers, which take a range (`score:2-5`, `score:3-`, `score:-2`).
//!
//! A filter takes alternatives between commas, any of which matches:
//! `src:tc,"Nature Geosci"`, `year:1990-2000,2015`, `-tag:review,draft*`
//! (excludes the papers that match any). A comma inside quotes is part of the
//! value, an empty alternative is none; the words of an alternative are a
//! phrase. A plain word takes no alternatives.

use crate::tag::branch;
use crate::text::{fold, words};

/// A parsed query, for [`Record::check`](crate::Record::check).
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub(crate) terms: Vec<Term>,
}

/// The field names a query takes besides those of the library: facets
/// (`kind:paper`) and numbers (`score:3-`). The library declares none, and
/// an undeclared name is no filter.
#[derive(Debug, Clone, Copy, Default)]
pub struct Schema<'a> {
    pub facets: &'a [&'a str],
    pub numbers: &'a [&'a str],
}

impl Query {
    /// The terms of a query, the facets and numbers it takes declared.
    pub fn parse(query: &str, schema: Schema) -> Query {
        let terms = tokens(query, schema).iter().flat_map(terms).collect();
        Query { terms }
    }

    /// Whether the query has no term: it matches everything.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Field {
    /// title, authors, journal, keywords, key, year, DOI: not the abstract,
    /// which only `abs:` searches (the library would find more otherwise)
    Any,
    Author,
    /// the first author only
    First,
    Title,
    Year,
    /// journal or book title
    Journal,
    Abstract,
    Key,
    Doi,
    /// keywords
    Tag,
    /// a field the entry has: `has:abstract`, or `has:pdf` for a linked file
    Has,
    /// a facet the schema declares, by its lowercase name
    Facet(String),
    /// a number the schema declares, by its lowercase name
    Number(String),
}

impl Field {
    fn named(name: &str, schema: Schema) -> Option<Field> {
        let name = name.to_ascii_lowercase();
        let declared = |names: &[&str]| names.iter().any(|n| n.eq_ignore_ascii_case(&name));
        Some(match name.as_str() {
            "author" | "a" => Field::Author,
            "first" | "f" => Field::First,
            "title" | "t" => Field::Title,
            "year" | "y" => Field::Year,
            "journal" | "j" => Field::Journal,
            "abstract" | "abs" => Field::Abstract,
            "key" | "k" => Field::Key,
            "doi" => Field::Doi,
            "tag" => Field::Tag,
            "has" => Field::Has,
            _ if declared(schema.facets) => Field::Facet(name),
            _ if declared(schema.numbers) => Field::Number(name),
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
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
    /// a whole tag or facet value, as the pieces between its `*` wildcards:
    /// one piece, no wildcard
    Tag(Vec<String>),
    /// the numbers from..=to, open-ended without a bound
    Numbers(Option<f64>, Option<f64>),
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

    /// Whether a number pattern takes `number`; the other patterns take none.
    pub fn matches_number(&self, number: f64) -> bool {
        match self {
            Pattern::Numbers(from, to) => {
                from.is_none_or(|f| number >= f) && to.is_none_or(|t| number <= t)
            }
            _ => false,
        }
    }

    /// Whether a tag pattern takes a folded `tag`: the tag as a whole, or one
    /// of its parents (`climate` takes `climate/amoc`); the other patterns
    /// take none.
    pub fn matches_tag(&self, tag: &str) -> bool {
        branch(tag).any(|name| self.matches_whole(name))
    }

    /// Whether a tag pattern takes a folded tag name or facet value, as a whole.
    pub fn matches_whole(&self, tag: &str) -> bool {
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Term {
    pub field: Field,
    /// any of them: one, but for the alternatives of a filter
    pub patterns: Vec<Pattern>,
    /// the papers that match are excluded
    pub negated: bool,
}

struct Token {
    negated: bool,
    field: Field,
    /// the alternatives of a filter: one without a comma
    values: Vec<Value>,
}

/// A word, or the text of a "phrase".
struct Value {
    text: String,
    quoted: bool,
}

/// The terms of a query of the library: no facet, no number.
pub(crate) fn parse(query: &str) -> Vec<Term> {
    Query::parse(query, Schema::default()).terms
}

fn tokens(query: &str, schema: Schema) -> Vec<Token> {
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
            if let Some(named) = Field::named(&name, schema) {
                field = named;
                i = name_end + 1;
            }
        }
        let commas = field != Field::Any;
        let mut values = Vec::new();
        loop {
            let (value, end) = value(&chars, i, commas);
            values.push(value);
            i = end;
            if !(commas && chars.get(i) == Some(&',')) {
                break;
            }
            i += 1;
        }
        tokens.push(Token {
            negated,
            field,
            values,
        });
    }
    tokens
}

/// The word or "phrase" at `i`, and where it ends: a word ends at a space,
/// and at a comma too with `commas` (between the alternatives of a filter).
fn value(chars: &[char], i: usize, commas: bool) -> (Value, usize) {
    let quoted = chars.get(i) == Some(&'"');
    let (start, end) = if quoted {
        let close = chars[i + 1..].iter().position(|&c| c == '"');
        (i + 1, close.map_or(chars.len(), |p| i + 1 + p))
    } else {
        let stop = |c: &char| c.is_whitespace() || commas && *c == ',';
        let end = chars[i..].iter().position(stop);
        (i, end.map_or(chars.len(), |p| i + p))
    };
    let text = chars[start..end].iter().collect();
    let next = if quoted { end + 1 } else { end };
    (Value { text, quoted }, next.min(chars.len()))
}

/// A term per word of a value; a single term of the alternatives of a filter
/// with commas.
fn terms(token: &Token) -> Vec<Term> {
    let term = |patterns| Term {
        field: token.field.clone(),
        patterns,
        negated: token.negated,
    };
    match token.values.as_slice() {
        [value] => patterns(&token.field, value, false)
            .into_iter()
            .map(|p| term(vec![p]))
            .collect(),
        values => {
            let any: Vec<Pattern> = values
                .iter()
                .flat_map(|v| patterns(&token.field, v, true))
                .collect();
            if any.is_empty() {
                Vec::new()
            } else {
                vec![term(any)]
            }
        }
    }
}

/// The patterns of a value, each to match: the words of a value, but a single
/// pattern of an `alternative`, its words a phrase.
fn patterns(field: &Field, value: &Value, alternative: bool) -> Vec<Pattern> {
    let quoted = value.quoted;
    let folded = fold(&value.text);
    let value = folded.trim();
    if value.is_empty() {
        return Vec::new(); // a filter still being typed
    }
    match field {
        Field::Year => vec![years(value)],
        Field::Key | Field::Has => vec![Pattern::Word(value.to_string())],
        Field::Doi => vec![Pattern::Doi(doi(value).to_string())],
        Field::Tag | Field::Facet(_) => {
            vec![Pattern::Tag(value.split('*').map(String::from).collect())]
        }
        Field::Number(_) => numbers(value).into_iter().collect(),
        Field::Any if !quoted && is_doi(value) => vec![Pattern::Doi(doi(value).to_string())],
        _ => {
            let words = words(value);
            if (quoted || alternative) && words.len() > 1 {
                vec![Pattern::Phrase(words)]
            } else {
                words.into_iter().map(Pattern::Word).collect()
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

/// `2-5`, `3-`, `-2`, or `3`, which is `3-`: a threshold, as a number is
/// seldom exactly one; decimals as `2.5`. Anything else is no filter.
fn numbers(value: &str) -> Option<Pattern> {
    let bound = |s: &str| {
        if s.is_empty() {
            Some(None)
        } else {
            s.parse().ok().map(Some)
        }
    };
    let (from, to) = match value.split_once('-') {
        Some((from, to)) => (bound(from)?, bound(to)?),
        None => (Some(value.parse().ok()?), None),
    };
    Some(Pattern::Numbers(from, to))
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
            patterns: vec![Pattern::Word(w.into())],
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
            patterns: vec![Pattern::Phrase(vec!["ice".into(), "sheet".into()])],
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
            patterns: vec![pattern],
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
            patterns: vec![Pattern::Doi(d.into())],
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
            patterns: vec![Pattern::Tag(pieces.iter().map(|p| p.to_string()).collect())],
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
            patterns: vec![pattern],
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
                    patterns: vec![Pattern::Years(None, Some(1990))],
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
            terms[0].patterns[0].matches_tag(tag)
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
        // the tags under it, but not a tag that only starts like it
        assert!(tag("climate", "climate/amoc"));
        assert!(tag("climate", "climate/amoc/tipping"));
        assert!(!tag("climate", "climate-review"));
        assert!(!tag("climate", "climate-review/amoc"));
        assert!(!tag("climate", "ocean/climate"));
        assert!(tag("climate/amoc", "climate/amoc/tipping"));
        assert!(!tag("climate/amoc", "climate"));
        assert!(!tag("climate/amoc", "climate/amoc-paper"));
        assert!(tag("clim*", "climate/amoc") && tag("*amoc", "ocean/amoc/tipping"));
        assert!(tag("climate/*", "climate/amoc"));
        assert!(!tag("climate/*", "climate"));
    }

    const SCHEMA: Schema = Schema {
        facets: &["kind", "is", "src"],
        numbers: &["score"],
    };

    fn parse_with(query: &str) -> Vec<Term> {
        Query::parse(query, SCHEMA).terms
    }

    #[test]
    fn abstracts() {
        assert_eq!(
            parse(r#"abs:Hysteresis abstract:"ice sheet""#),
            [
                word(Field::Abstract, "hysteresis"),
                Term {
                    field: Field::Abstract,
                    patterns: vec![Pattern::Phrase(vec!["ice".into(), "sheet".into()])],
                    negated: false,
                },
            ]
        );
    }

    #[test]
    fn facets() {
        let facet = |name: &str, pieces: &[&str], negated| Term {
            field: Field::Facet(name.into()),
            patterns: vec![Pattern::Tag(pieces.iter().map(|p| p.to_string()).collect())],
            negated,
        };
        assert_eq!(
            parse_with(r#"Kind:paper -is:dismissed src:"The Cryosphere" src:*sphere is:"#),
            [
                facet("kind", &["paper"], false),
                facet("is", &["dismissed"], true),
                facet("src", &["the cryosphere"], false),
                facet("src", &["", "sphere"], false),
            ]
        );
        // undeclared: plain words, as in the library
        assert_eq!(
            parse("kind:paper score:3"),
            [
                word(Field::Any, "kind"),
                word(Field::Any, "paper"),
                word(Field::Any, "score"),
                word(Field::Any, "3"),
            ]
        );
        assert_eq!(
            parse_with("prof:ice"),
            [word(Field::Any, "prof"), word(Field::Any, "ice")]
        );
        // the fields of the library come first
        assert_eq!(
            Query::parse(
                "tag:amoc",
                Schema {
                    facets: &["tag"],
                    numbers: &[]
                }
            )
            .terms,
            parse("tag:amoc")
        );
    }

    #[test]
    fn numbers() {
        let score = |from, to, negated| Term {
            field: Field::Number("score".into()),
            patterns: vec![Pattern::Numbers(from, to)],
            negated,
        };
        assert_eq!(
            parse_with("score:3- score:-2 score:2-5 -score:3 score:2.5- score:high score:1-x"),
            [
                score(Some(3.0), None, false),
                score(None, Some(2.0), false),
                score(Some(2.0), Some(5.0), false),
                score(Some(3.0), None, true),
                score(Some(2.5), None, false),
            ]
        );
        assert!(Pattern::Numbers(Some(2.0), Some(5.0)).matches_number(5.0));
        assert!(!Pattern::Numbers(Some(2.0), Some(5.0)).matches_number(5.5));
        assert!(Pattern::Numbers(None, Some(2.0)).matches_number(-1.0));
        assert!(!Pattern::Years(Some(2000), None).matches_number(2012.0));
    }

    #[test]
    fn alternatives() {
        let any = |field: Field, patterns: Vec<Pattern>, negated| Term {
            field,
            patterns,
            negated,
        };
        let src = |v: &str| Pattern::Tag(vec![v.into()]);
        let facet = || Field::Facet("src".into());
        // quoted, mixed; a comma inside quotes is part of the value
        assert_eq!(
            parse_with(
                r#"src:tc,grl src:"The Cryosphere","Nature Geoscience" src:tc,"Nature, Geosci""#
            ),
            [
                any(facet(), vec![src("tc"), src("grl")], false),
                any(
                    facet(),
                    vec![src("the cryosphere"), src("nature geoscience")],
                    false
                ),
                any(facet(), vec![src("tc"), src("nature, geosci")], false),
            ]
        );
        // an empty alternative is none: still being typed
        assert_eq!(
            parse_with("src:tc, src:,grl src:, x"),
            [
                any(facet(), vec![src("tc")], false),
                any(facet(), vec![src("grl")], false),
                word(Field::Any, "x"),
            ]
        );
        // excluded: any of them
        assert_eq!(
            parse_with("-kind:paper,preprint"),
            [any(
                Field::Facet("kind".into()),
                vec![src("paper"), src("preprint")],
                true
            )]
        );
        // the words of an alternative are a phrase
        let phrase = |ws: &[&str]| Pattern::Phrase(ws.iter().map(|w| w.to_string()).collect());
        assert_eq!(
            parse(r#"a:robinson,van-der first:"van der",calov"#),
            [
                any(
                    Field::Author,
                    vec![Pattern::Word("robinson".into()), phrase(&["van", "der"])],
                    false
                ),
                any(
                    Field::First,
                    vec![phrase(&["van", "der"]), Pattern::Word("calov".into())],
                    false
                ),
            ]
        );
        // years, numbers (one that is no number left out), tags, DOIs, has:
        assert_eq!(
            parse("year:1990-2000,2015,201"),
            [any(
                Field::Year,
                vec![
                    Pattern::Years(Some(1990), Some(2000)),
                    Pattern::Years(Some(2015), Some(2015)),
                    Pattern::YearStart("201".into()),
                ],
                false
            )]
        );
        assert_eq!(
            parse_with("score:-1,4-,high score:x,y"),
            [any(
                Field::Number("score".into()),
                vec![
                    Pattern::Numbers(None, Some(1.0)),
                    Pattern::Numbers(Some(4.0), None),
                ],
                false
            )]
        );
        assert_eq!(
            parse("tag:amoc,ice* doi:10.5194,nclimate has:pdf,abstract"),
            [
                any(
                    Field::Tag,
                    vec![
                        Pattern::Tag(vec!["amoc".into()]),
                        Pattern::Tag(vec!["ice".into(), "".into()]),
                    ],
                    false
                ),
                any(
                    Field::Doi,
                    vec![
                        Pattern::Doi("10.5194".into()),
                        Pattern::Doi("nclimate".into()),
                    ],
                    false
                ),
                any(
                    Field::Has,
                    vec![
                        Pattern::Word("pdf".into()),
                        Pattern::Word("abstract".into()),
                    ],
                    false
                ),
            ]
        );
        // no comma: as before; plain words take none
        assert_eq!(
            parse("first:van-der robinson,calov foo:a,b"),
            [
                word(Field::First, "van"),
                word(Field::First, "der"),
                word(Field::Any, "robinson"),
                word(Field::Any, "calov"),
                word(Field::Any, "foo"),
                word(Field::Any, "a"),
                word(Field::Any, "b"),
            ]
        );
        assert_eq!(parse(r#""ice, sheet""#), parse(r#""ice sheet""#));
    }
}
