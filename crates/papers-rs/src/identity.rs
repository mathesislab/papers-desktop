//! Whether two entries are the same paper (`papers.duplicate`,
//! `papers.identifiers`): the identifiers of an entry, the rule that compares
//! them, the pairs the user said are different (`.papers/distinct.json`), and
//! the search for duplicates.
//!
//! [`same_paper`] compares, in this order, the DOI, the arXiv identifier, the
//! ISBN (two books only), and the family names of the authors with the title
//! (all normalized: case, braces, LaTeX and accents, spaces). The first
//! identifier that both entries have decides: equal values are the same paper,
//! different ones two papers. A title guard turns `same` into `ask` for equal
//! identifiers when the types and the titles differ.
//!
//! ```
//! use papers_rs::bib::Entry;
//! use papers_rs::identity::{Kind, same_paper};
//!
//! let mut a = Entry::new("article", "a");
//! a.set("doi", "10.1000/ABC");
//! let mut b = Entry::new("article", "b");
//! b.set("doi", "https://doi.org/10.1000/abc");
//! let verdict = same_paper(&a, &b, None);
//! assert_eq!(verdict.kind, Kind::Same);
//! assert_eq!(verdict.reason, "same DOI 10.1000/abc");
//! ```
//!
//! The normalization depends on the table of [`crate::names::ascii`] (papers:
//! AnyAscii, the same table). The digits of the identifiers (ISBN, arXiv, a key that
//! starts with one) are the ASCII ones, `0`-`9`.

use std::collections::{BTreeSet, HashMap};

use serde_json::{Map, Value};

use crate::bib::Entry;
use crate::bib::encoding::{Sep, split_depth0};
use crate::bib::latex::latex_to_unicode;
use crate::bib::pystr::{is_space, strip};
use crate::config::json::{dumps_sorted, loads};
use crate::config::paths::{dirname, exists, join};
use crate::entry::val;
use crate::log;
use crate::names::ascii;

// normalization
// -------------

/// A text normalized for comparison: LaTeX as unicode ([`latex_to_unicode`]),
/// ASCII, without braces, lower case, single spaces (`normalize_text`).
///
/// ```
/// use papers_rs::identity::normalize_text;
/// assert_eq!(normalize_text("The {CO2}  M{\\\"u}ller Problem"), "the co2 muller problem");
/// ```
pub fn normalize_text(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let s = if s.is_ascii() && !s.contains(['\\', '{', '}']) {
        s.to_string()
    } else {
        latex_to_unicode(s, true)
    };
    let s = ascii(&s);
    let s: String = s.chars().filter(|&c| c != '{' && c != '}').collect();
    s.to_lowercase()
        .split(is_space)
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The names of an `author` field, split at `and` between spaces (`\s+and\s+`)
/// outside braces (`{Barnes and Noble}` is one name), stripped, the empty ones
/// dropped (`split_names`).
pub fn split_names(field: &str) -> Vec<String> {
    split_depth0(field, Sep::And)
        .into_iter()
        .filter(|n| !n.is_empty())
        .collect()
}

/// The family name and the given names of a name: `Smith, John`, `John
/// Smith`, `{van der Berg}, Jane` (`parse_name`). The given names may be empty.
///
/// ```
/// use papers_rs::identity::parse_name;
/// assert_eq!(parse_name("Smith, John"), ("Smith".into(), "John".into()));
/// assert_eq!(parse_name("John {van der Berg}"), ("{van der Berg}".into(), "John".into()));
/// ```
pub fn parse_name(name: &str) -> (String, String) {
    let name = strip(name);
    let mut depth = 0i64;
    for (i, c) in name.char_indices() {
        // the first comma outside braces
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                return (
                    strip(&name[..i]).to_string(),
                    strip(&name[i + 1..]).to_string(),
                );
            }
            _ => {}
        }
    }
    // `^(.*?)\s*(\{[^{}]*\}|\S+)$` (DOTALL: the words may be on several lines): the last word, a braced group kept together
    let mut ends: Vec<usize> = name.char_indices().map(|(i, _)| i).collect();
    ends.push(name.len());
    for &l in &ends {
        let rest = name[l..].trim_start_matches(is_space);
        if rest.is_empty() {
            continue;
        }
        let braced = rest.starts_with('{')
            && rest.ends_with('}')
            && !rest[1..rest.len() - 1].contains(['{', '}']);
        if braced || !rest.contains(is_space) {
            return (rest.to_string(), strip(&name[..l]).to_string());
        }
    }
    (name.to_string(), String::new())
}

/// The normalized family names of an author field.
pub fn family_name_ids(field: &str) -> Vec<String> {
    split_names(field)
        .iter()
        .map(|n| normalize_text(&parse_name(n).0))
        .collect()
}

// identifiers
// -----------

/// Case-insensitive (ASCII) `starts_with` on characters; the length of the prefix in bytes.
fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// A DOI without `doi.org/` or `doi:` in front, in its original case (the case
/// matters in the identifier of an arXiv DOI); none if it is not a DOI.
pub fn strip_doi(text: &str) -> Option<String> {
    let text = strip(text);
    let mut rest = text;
    'prefix: {
        for scheme in ["https://", "http://"] {
            if let Some(after) = strip_prefix_ci(text, scheme) {
                for host in ["dx.doi.org/", "doi.org/"] {
                    if let Some(r) = strip_prefix_ci(after, host) {
                        rest = r;
                        break 'prefix;
                    }
                }
            }
        }
        if let Some(after) = strip_prefix_ci(text, "doi:") {
            rest = after.trim_start_matches(is_space);
        }
    }
    let doi = strip(rest);
    (doi.starts_with("10.") && doi.contains('/')).then(|| doi.to_string())
}

/// A DOI in lower case without `doi.org/` or `doi:`, for comparison; none if it is not a DOI.
pub fn normalize_doi(text: &str) -> Option<String> {
    strip_doi(text).map(|d| d.to_lowercase())
}

/// The length of the run of ASCII digits (`[0-9]`) at the start (`\d` is any Unicode digit: not here).
fn digit_run(s: &str) -> usize {
    s.chars().take_while(char::is_ascii_digit).count()
}

fn skip_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[i..],
        None => "",
    }
}

/// `(v[0-9]+)?$`
fn version_to_end(s: &str) -> bool {
    match s.strip_prefix(['v', 'V']) {
        Some(digits) => !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()),
        None => s.is_empty(),
    }
}

/// What follows `http(s)://arxiv.org/abs/` or `/pdf/`.
fn arxiv_url_rest(text: &str) -> Option<&str> {
    for scheme in ["https://", "http://"] {
        if let Some(after) =
            strip_prefix_ci(text, scheme).and_then(|a| strip_prefix_ci(a, "arxiv.org/"))
        {
            for kind in ["abs/", "pdf/"] {
                if let Some(rest) = strip_prefix_ci(after, kind) {
                    return Some(rest);
                }
            }
        }
    }
    None
}

/// An arXiv identifier without version and `arXiv:` in front (`2101.01234`,
/// `hep-th/9901001`), in lower case; none if it is not one.
pub fn normalize_arxiv(text: &str) -> Option<String> {
    let text = strip(text);
    let s = match strip_prefix_ci(text, "arxiv:") {
        Some(after) => after.trim_start_matches(is_space),
        None => arxiv_url_rest(text).unwrap_or(text),
    };
    let mut s = strip(s);
    if s.to_lowercase().ends_with(".pdf") {
        s = &s[..s.len() - 4];
    }
    // new: NNNN.NNNN or NNNN.NNNNN, then a version
    let n = digit_run(s);
    if n == 4 {
        let after = skip_chars(s, 4);
        if let Some(after) = after.strip_prefix('.') {
            let m = digit_run(after);
            if (m == 4 || m == 5) && version_to_end(skip_chars(after, m)) {
                let id: String = s.chars().take(4 + 1 + m).collect();
                return Some(id.to_lowercase());
            }
        }
    }
    // old: archive/NNNNNNN or archive.XX/NNNNNNN, then a version
    let mut chars = s.char_indices().peekable();
    let (_, first) = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let mut end = first.len_utf8();
    for (i, c) in chars {
        if c.is_ascii_alphabetic() || c == '-' {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    let mut rest = &s[end..];
    if let Some(after) = rest.strip_prefix('.') {
        let two: Vec<char> = after.chars().take(2).collect();
        if two.len() == 2 && two.iter().all(char::is_ascii_alphabetic) {
            rest = &after[2..];
        }
    }
    let after = rest.strip_prefix('/')?;
    if digit_run(after) == 7 && version_to_end(skip_chars(after, 7)) {
        let id_len =
            s.len() - rest.len() + 1 + after.char_indices().nth(7).map_or(after.len(), |(i, _)| i);
        return Some(s[..id_len].to_lowercase());
    }
    None
}

/// The ISBN-13 of an ISBN-10 or ISBN-13 (hyphens and spaces allowed, check
/// digit verified); none if it is not one.
///
/// ```
/// use papers_rs::identity::normalize_isbn;
/// assert_eq!(normalize_isbn("0-306-40615-2").as_deref(), Some("9780306406157"));
/// assert_eq!(normalize_isbn("ISBN: 978-0-306-40615-7").as_deref(), Some("9780306406157"));
/// assert_eq!(normalize_isbn("0-306-40615-3"), None);
/// ```
pub fn normalize_isbn(text: &str) -> Option<String> {
    let s: String = strip(text)
        .chars()
        .filter(|&c| !is_space(c) && c != '-')
        .collect();
    let s = if s.get(..4).is_some_and(|h| h.eq_ignore_ascii_case("isbn")) {
        s[4..].trim_start_matches(':').to_string()
    } else {
        s
    };
    let s = s.to_uppercase();
    let chars: Vec<char> = s.chars().collect();
    let digit = |c: char| c.to_digit(10).unwrap_or(0);
    let weigh13 = |chars: &[char]| -> u32 {
        chars
            .iter()
            .enumerate()
            .map(|(i, &c)| digit(c) * if i % 2 == 0 { 1 } else { 3 })
            .sum()
    };
    if chars.len() == 10
        && chars[..9].iter().all(char::is_ascii_digit)
        && (chars[9].is_ascii_digit() || chars[9] == 'X')
    {
        let digits: Vec<u32> = chars
            .iter()
            .map(|&c| if c == 'X' { 10 } else { digit(c) })
            .collect();
        let sum: u32 = digits
            .iter()
            .enumerate()
            .map(|(i, d)| (10 - i as u32) * d)
            .sum();
        if !sum.is_multiple_of(11) {
            return None;
        }
        let mut core: Vec<char> = "978".chars().collect();
        core.extend(&chars[..9]);
        let check = (10 - weigh13(&core) % 10) % 10;
        let mut out: String = core.iter().collect();
        out.push_str(&check.to_string());
        return Some(out);
    }
    if chars.len() == 13 && chars.iter().all(char::is_ascii_digit) {
        if !weigh13(&chars).is_multiple_of(10) {
            return None;
        }
        return Some(s);
    }
    None
}

/// The arXiv identifier of an arXiv DOI (`10.48550/arXiv.<id>`), as written in the DOI.
pub fn arxiv_of_doi(doi: &str) -> Option<String> {
    let doi = strip(doi);
    let rest = strip_prefix_ci(doi, "10.48550/arxiv.")?;
    // `(.+)$`: at least one character, no newline, `$` before a last newline
    let line = match rest.find('\n') {
        Some(i) if i + 1 == rest.len() => &rest[..i],
        Some(_) => return None,
        None => rest,
    };
    (!line.is_empty()).then(|| line.to_string())
}

fn eprint_arxiv(entry: &Entry) -> Option<String> {
    let prefix = match val(entry, "archiveprefix") {
        "" => val(entry, "archivePrefix"),
        p => p,
    };
    if matches!(strip(prefix).to_lowercase().as_str(), "" | "arxiv") {
        return normalize_arxiv(val(entry, "eprint"));
    }
    None
}

fn doi_arxiv(entry: &Entry) -> Option<String> {
    let doi = normalize_doi(val(entry, "doi"))?;
    normalize_arxiv(&arxiv_of_doi(&doi)?)
}

/// The arXiv identifier of an entry: of its `eprint` field (arXiv or no
/// `archivePrefix`), else of an arXiv DOI (empty if none).
pub fn entry_arxiv(entry: &Entry) -> String {
    eprint_arxiv(entry)
        .or_else(|| doi_arxiv(entry))
        .unwrap_or_default()
}

/// The normalized identifiers of an entry (`Identifiers`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Identifiers {
    /// the type, in lower case
    pub entry_type: String,
    pub doi: String,
    pub arxiv: String,
    /// the ISBN-13s of a book (the other types have none: a chapter carries the ISBN of its book)
    pub isbns: BTreeSet<String>,
    /// the normalized family names, joined by spaces
    pub author: String,
    pub title: String,
    /// `author|title`, empty when one is missing
    pub author_title: String,
}

impl Identifiers {
    pub fn of(entry: &Entry) -> Identifiers {
        let entry_type = entry.entry_type.to_lowercase();
        let isbns = if entry_type == "book" {
            val(entry, "isbn")
                .split([',', ';'])
                .filter(|v| !strip(v).is_empty())
                .filter_map(normalize_isbn)
                .collect()
        } else {
            BTreeSet::new()
        };
        let author = family_name_ids(val(entry, "author")).join(" ");
        let title = normalize_text(val(entry, "title"));
        let author_title = if !author.is_empty() && !title.is_empty() {
            format!("{author}|{title}")
        } else {
            String::new()
        };
        Identifiers {
            entry_type,
            doi: normalize_doi(val(entry, "doi")).unwrap_or_default(),
            arxiv: entry_arxiv(entry),
            isbns,
            author,
            title,
            author_title,
        }
    }

    /// What `distinct.json` records of the entry.
    pub fn identity(&self) -> Identity {
        Identity {
            doi: (!self.doi.is_empty()).then(|| self.doi.clone()),
            arxiv: (!self.arxiv.is_empty()).then(|| self.arxiv.clone()),
            isbn: (!self.isbns.is_empty())
                .then(|| self.isbns.iter().cloned().collect::<Vec<_>>().join(",")),
            author_title: (!self.author_title.is_empty()).then(|| self.author_title.clone()),
            // no authors: the title tells apart entries that share an identifier
            title: (self.author_title.is_empty() && !self.title.is_empty())
                .then(|| self.title.clone()),
        }
    }
}

/// The identifiers of an entry as `distinct.json` records them (`identity`):
/// the normalized DOI, arXiv identifier, ISBN-13s (a book's, sorted, joined
/// with `,`), and `authortitle` (or `title`, without authors).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Identity {
    pub doi: Option<String>,
    pub arxiv: Option<String>,
    pub isbn: Option<String>,
    pub author_title: Option<String>,
    pub title: Option<String>,
}

impl Identity {
    /// Nothing known of the entry.
    pub fn is_empty(&self) -> bool {
        *self == Identity::default()
    }

    /// The JSON object of the file.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        for (k, v) in [
            ("doi", &self.doi),
            ("arxiv", &self.arxiv),
            ("isbn", &self.isbn),
            ("authortitle", &self.author_title),
            ("title", &self.title),
        ] {
            if let Some(v) = v {
                m.insert(k.to_string(), Value::String(v.clone()));
            }
        }
        Value::Object(m)
    }

    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "doi" => self.doi.as_deref(),
            "arxiv" => self.arxiv.as_deref(),
            "isbn" => self.isbn.as_deref(),
            "authortitle" => self.author_title.as_deref(),
            "title" => self.title.as_deref(),
            _ => None,
        }
    }
}

// the rule
// --------

/// What [`same_paper`] says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Same,
    Different,
    /// the identifiers are equal but the types and titles differ: the user decides
    Ask,
}

/// The verdict of [`same_paper`]: `reason` says why (`same DOI 10.1000/xyz`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub kind: Kind,
    pub reason: String,
}

impl Verdict {
    fn new(kind: Kind, reason: impl Into<String>) -> Verdict {
        Verdict {
            kind,
            reason: reason.into(),
        }
    }
}

/// The reasons of the verdicts for entries whose identifiers contradict each other.
pub const CONTRADICTIONS: [&str; 3] = ["different DOIs", "different arXiv ids", "different ISBNs"];

fn guard(a: &Identifiers, b: &Identifiers, reason: String) -> Verdict {
    if a.entry_type != b.entry_type
        && !a.title.is_empty()
        && !b.title.is_empty()
        && a.title != b.title
    {
        return Verdict::new(
            Kind::Ask,
            format!(
                "{reason}, but the entry types ({}, {}) and the titles differ",
                a.entry_type, b.entry_type
            ),
        );
    }
    Verdict::new(Kind::Same, reason)
}

/// [`same_paper`] for entries whose identifiers are known.
pub fn same_paper_ids(a: &Identifiers, b: &Identifiers, distinct: Option<&Distinct>) -> Verdict {
    if let Some(d) = distinct.filter(|d| !d.pairs.is_empty())
        && d.contains_identities(&a.identity(), &b.identity())
    {
        return Verdict::new(
            Kind::Different,
            "marked as not the same paper (.papers/distinct.json)",
        );
    }
    if !a.doi.is_empty() && !b.doi.is_empty() {
        if a.doi != b.doi {
            return Verdict::new(Kind::Different, CONTRADICTIONS[0]);
        }
        return guard(a, b, format!("same DOI {}", a.doi));
    }
    if !a.arxiv.is_empty() && !b.arxiv.is_empty() {
        if a.arxiv != b.arxiv {
            return Verdict::new(Kind::Different, CONTRADICTIONS[1]);
        }
        return guard(a, b, format!("same arXiv id {}", a.arxiv));
    }
    if !a.isbns.is_empty() && !b.isbns.is_empty() {
        return match a.isbns.intersection(&b.isbns).next() {
            None => Verdict::new(Kind::Different, CONTRADICTIONS[2]),
            Some(common) => Verdict::new(Kind::Same, format!("same ISBN {common}")),
        };
    }
    if !a.author_title.is_empty() && a.author_title == b.author_title {
        return Verdict::new(Kind::Same, "same title and authors");
    }
    Verdict::new(Kind::Different, "no common identifier")
}

/// Whether two entries are the same paper (see the module documentation);
/// `distinct` holds the pairs recorded as different.
pub fn same_paper(a: &Entry, b: &Entry, distinct: Option<&Distinct>) -> Verdict {
    same_paper_ids(&Identifiers::of(a), &Identifiers::of(b), distinct)
}

/// Whether an identifier that both entries have differs (DOI, arXiv id or ISBN, see
/// [`same_paper`]; the pairs of `distinct.json` aside).
pub fn contradicts(a: &Entry, b: &Entry) -> bool {
    CONTRADICTIONS.contains(&same_paper(a, b, None).reason.as_str())
}

// not the same paper
// ------------------

/// The file of the pairs, relative to the library directory.
pub const DISTINCT_FILE: &str = ".papers/distinct.json";

/// A recorded identity matches the identity of an entry when they have the
/// same DOI, arXiv id and ISBN (or both none of them), and the same authors
/// and title (or title) when both have them.
fn side_matches(recorded: &Value, ident: &Identity) -> bool {
    let Some(map) = recorded.as_object().filter(|m| !m.is_empty()) else {
        return false;
    };
    for k in ["doi", "arxiv", "isbn"] {
        let ok = match (map.get(k), ident.get(k)) {
            (None, None) => true,
            (Some(Value::String(r)), Some(i)) => r == i,
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    for k in ["authortitle", "title"] {
        if let (Some(r), Some(i)) = (map.get(k), ident.get(k))
            && r != &Value::String(i.to_string())
        {
            return false;
        }
    }
    true
}

/// The pairs of entries that are not the same paper, by identity, not by key,
/// so that the file means the same in every library (`.papers/distinct.json`
/// of the library folder).
#[derive(Debug, Clone, Default)]
pub struct Distinct {
    /// the pairs as the file has them (two JSON values each)
    pub pairs: Vec<[Value; 2]>,
    pub path: Option<String>,
    /// a pair was added since the file was read or saved
    pub changed: bool,
}

impl Distinct {
    pub fn new() -> Distinct {
        Distinct::default()
    }

    /// The pairs of the library in the folder `libdir` (`Config::libdir`): its `.papers/distinct.json`.
    pub fn for_library(libdir: &str) -> Distinct {
        Distinct::load(&join(libdir, DISTINCT_FILE))
    }

    /// The pairs of a bibtex file without library: the `.papers/distinct.json` next to it.
    pub fn for_bibtex(bibtex: &str) -> Distinct {
        Distinct::for_library(&dirname(&crate::config::paths::abspath(bibtex)))
    }

    /// The pairs of a file: none if it does not exist; a file that cannot be
    /// read is ignored, with a warning.
    pub fn load(path: &str) -> Distinct {
        let mut pairs = Vec::new();
        if exists(path) {
            let read = || -> Result<Vec<[Value; 2]>, String> {
                let bytes =
                    std::fs::read(path).map_err(|e| crate::config::machine::io_message(&e))?;
                let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
                // read as UTF-8, a byte order mark dropped
                let value = loads(crate::bib::without_bom(&text)).map_err(|e| e.message)?;
                let not_list = || "not an object with a list \"distinct\"".to_string();
                let Value::Object(map) = value else {
                    return Err(not_list());
                };
                Ok(match map.get("distinct") {
                    None => Vec::new(),
                    Some(Value::Array(items)) => items
                        .iter()
                        .filter_map(|p| match p.as_array().map(Vec::as_slice) {
                            Some([a, b]) if a.is_object() && b.is_object() => {
                                Some([a.clone(), b.clone()])
                            }
                            _ => None,
                        })
                        .collect(),
                    Some(_) => return Err(not_list()),
                })
            };
            match read() {
                Ok(p) => pairs = p,
                Err(error) => {
                    log::warning(&format!("{path}: could not be read ({error}); ignored"))
                }
            }
        }
        Distinct {
            pairs,
            path: Some(path.to_string()),
            changed: false,
        }
    }

    /// Whether two entries were recorded as different papers (in either order).
    pub fn contains(&self, a: &Entry, b: &Entry) -> bool {
        if self.pairs.is_empty() {
            return false;
        }
        self.contains_identities(
            &Identifiers::of(a).identity(),
            &Identifiers::of(b).identity(),
        )
    }

    pub(crate) fn contains_identities(&self, ia: &Identity, ib: &Identity) -> bool {
        self.pairs.iter().any(|[x, y]| {
            (side_matches(x, ia) && side_matches(y, ib))
                || (side_matches(y, ia) && side_matches(x, ib))
        })
    }

    /// Record that entries `a` and `b` are not the same paper; not if one has
    /// no identity, or if the pair is there already.
    pub fn add(&mut self, a: &Entry, b: &Entry) {
        let (ia, ib) = (Identifiers::of(a).identity(), Identifiers::of(b).identity());
        if ia.is_empty() || ib.is_empty() || self.contains_identities(&ia, &ib) {
            return;
        }
        self.pairs.push([ia.to_json(), ib.to_json()]);
        self.changed = true;
    }

    /// The union with the pairs of another (a pair equal to one, in either
    /// order, is not added again).
    pub fn update(&mut self, other: &Distinct) {
        for pair in &other.pairs {
            let swapped = [pair[1].clone(), pair[0].clone()];
            if !self.pairs.contains(pair) && !self.pairs.contains(&swapped) {
                self.pairs.push(pair.clone());
                self.changed = true;
            }
        }
    }

    /// The text of the file: indent of one space, keys sorted, ASCII, a final newline.
    pub fn to_text(&self) -> String {
        let pairs: Vec<Value> = self
            .pairs
            .iter()
            .map(|[a, b]| Value::Array(vec![a.clone(), b.clone()]))
            .collect();
        let mut map = Map::new();
        map.insert("distinct".to_string(), Value::Array(pairs));
        let mut text = dumps_sorted(&Value::Object(map), 1);
        text.push('\n');
        text
    }

    /// Write the file, if a pair was added (the folder is created if needed;
    /// papers writes it in place, not through a temporary file).
    pub fn save(&mut self) -> std::io::Result<()> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        if !self.changed {
            return Ok(());
        }
        std::fs::create_dir_all(dirname(&path))?;
        std::fs::write(&path, self.to_text())?;
        self.changed = false;
        Ok(())
    }
}

// candidates and groups
// ---------------------

/// The entries by identifier (the DOI, the arXiv id, each ISBN, the authors
/// and title), so that a duplicate search compares an entry only with those
/// that share an identifier. An entry is known by a handle (the library's
/// number for it); the index keeps the position of each entry, the order in
/// which it was first added, and an entry removed and added again keeps it.
#[derive(Debug, Clone, Default)]
pub struct IdentityIndex {
    keys: HashMap<(u8, String), Vec<u64>>,
    entries: HashMap<u64, (usize, Identifiers)>,
    count: usize,
}

fn index_keys(ids: &Identifiers) -> Vec<(u8, String)> {
    let ident = ids.identity();
    let mut keys = Vec::new();
    for (kind, v) in [
        (0u8, ident.doi),
        (1, ident.arxiv),
        (3, ident.author_title),
        (4, ident.title),
    ] {
        if let Some(v) = v {
            keys.push((kind, v));
        }
    }
    keys.extend(ids.isbns.iter().map(|i| (2u8, i.clone())));
    keys
}

impl IdentityIndex {
    pub fn new() -> IdentityIndex {
        IdentityIndex::default()
    }

    /// An index of entries, the handle of each being its number.
    pub fn of<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> IdentityIndex {
        let mut index = IdentityIndex::new();
        for (i, e) in entries.into_iter().enumerate() {
            index.add(i as u64, e);
        }
        index
    }

    /// Add an entry (or its new identifiers, if the handle is known: it keeps its position).
    pub fn add(&mut self, handle: u64, entry: &Entry) {
        let ids = Identifiers::of(entry);
        let position = match self.entries.get(&handle) {
            Some((p, _)) => *p,
            None => {
                self.count += 1;
                self.count - 1
            }
        };
        for key in index_keys(&ids) {
            let matches = self.keys.entry(key).or_default();
            if !matches.contains(&handle) {
                matches.push(handle);
            }
        }
        self.entries.insert(handle, (position, ids));
    }

    /// Take an entry out of the index (not its position, if it is added again).
    pub fn remove(&mut self, handle: u64) {
        if let Some((_, ids)) = self.entries.get(&handle) {
            for key in index_keys(ids) {
                if let Some(matches) = self.keys.get_mut(&key) {
                    matches.retain(|h| *h != handle);
                }
            }
        }
    }

    /// Forget an entry for good (its position too).
    pub fn forget(&mut self, handle: u64) {
        self.remove(handle);
        self.entries.remove(&handle);
    }

    pub fn identifiers(&self, handle: u64) -> Option<&Identifiers> {
        self.entries.get(&handle).map(|(_, ids)| ids)
    }

    pub fn position(&self, handle: u64) -> Option<usize> {
        self.entries.get(&handle).map(|(p, _)| *p)
    }

    /// The handles of the entries that share an identifier with these, but `except`.
    pub fn candidates(&self, ids: &Identifiers, except: Option<u64>) -> Vec<u64> {
        let mut out: Vec<u64> = Vec::new();
        for key in index_keys(ids) {
            for &h in self.keys.get(&key).map_or(&[][..], Vec::as_slice) {
                if Some(h) != except && !out.contains(&h) {
                    out.push(h);
                }
            }
        }
        out
    }
}

/// The entries of the index that are the same paper as `entry` (`same` first,
/// then `ask`, each in the order of the index): the handle and the verdict.
/// The result is that of comparing `entry` with every indexed entry but `except` (the entry
/// itself, if it is in the index).
pub fn find_duplicates(
    entry: &Entry,
    index: &IdentityIndex,
    distinct: Option<&Distinct>,
    except: Option<u64>,
) -> Vec<(u64, Verdict)> {
    let ids = Identifiers::of(entry);
    let mut found: Vec<(u64, Verdict)> = index
        .candidates(&ids, except)
        .into_iter()
        .filter_map(|h| {
            let verdict = same_paper_ids(index.identifiers(h)?, &ids, distinct);
            (verdict.kind != Kind::Different).then_some((h, verdict))
        })
        .collect();
    found.sort_by_key(|(h, v)| (v.kind != Kind::Same, index.position(*h)));
    found
}

/// A group of entries that are the same paper (`Group`): their numbers in
/// the list searched, the first being the entry with most fields (the
/// proposed target of a merge). `kind` is `Same`, or `Ask` when the title
/// guard fires for a pair of the group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub entries: Vec<usize>,
    pub kind: Kind,
    pub reason: String,
}

/// The number of fields of an entry, the type and the key counting as fields.
fn n_fields(e: &Entry) -> usize {
    2 + e.fields.len()
}

/// The groups of two entries or more that are the same paper
/// (`duplicate_groups`): entries linked by `same` or `ask` verdicts are put
/// together, and a group never holds two entries that are different papers (a
/// copy without DOI that matches two entries with different DOIs by authors
/// and title goes with the first of them, most fields first).
pub fn duplicate_groups(entries: &[&Entry], distinct: Option<&Distinct>) -> Vec<Group> {
    let index = IdentityIndex::of(entries.iter().copied());
    let ids: Vec<&Identifiers> = (0..entries.len())
        .map(|i| index.identifiers(i as u64).unwrap())
        .collect();
    let same = |i: usize, j: usize| same_paper_ids(ids[i], ids[j], distinct);
    let mut parent: Vec<usize> = (0..entries.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for (i, ids_i) in ids.iter().enumerate() {
        for other in index.candidates(ids_i, Some(i as u64)) {
            let j = other as usize;
            if j > i && same(i, j).kind != Kind::Different {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                parent[rj] = ri;
            }
        }
    }
    let mut components: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..entries.len() {
        let root = find(&mut parent, i);
        components.entry(root).or_default().push(i);
    }
    let mut groups = Vec::new();
    for (_, mut component) in components {
        component.sort_by_key(|&i| std::cmp::Reverse(n_fields(entries[i])));
        let mut parts: Vec<Vec<usize>> = Vec::new();
        'entry: for e in component {
            for part in parts.iter_mut() {
                if part.iter().all(|&x| same(x, e).kind != Kind::Different) {
                    part.push(e);
                    continue 'entry;
                }
            }
            parts.push(vec![e]);
        }
        for part in parts {
            if part.len() < 2 {
                continue;
            }
            let (mut kind, mut reason) = (Kind::Same, String::new());
            for (n, &a) in part.iter().enumerate() {
                for &b in &part[n + 1..] {
                    let v = same(a, b);
                    if v.kind == Kind::Ask && kind == Kind::Same {
                        kind = Kind::Ask;
                        reason = v.reason;
                    } else if reason.is_empty() {
                        reason = v.reason;
                    }
                }
            }
            groups.push(Group {
                entries: part,
                kind,
                reason,
            });
        }
    }
    groups
}

// naming an entry
// ----------------

/// What an identifier on the command line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdKind {
    Key,
    Doi,
    Arxiv,
    Isbn,
}

/// What the commands that take an entry accept (`ACCEPTED`).
pub const ACCEPTED: &str = "key, DOI, arXiv id or ISBN";

/// The kind and the normalized value of an identifier of the command line
/// (`parse_identifier`): a DOI, an arXiv id, an ISBN, else a key (in lower
/// case). An argument that starts with a digit and is none of the others is
/// an error (`Library::select_some` takes a key of the library first: only
/// then may a key start with a digit).
pub fn parse_identifier(arg: &str) -> Result<(IdKind, String), String> {
    if let Some(doi) = normalize_doi(arg) {
        return Ok((IdKind::Doi, doi));
    }
    if let Some(arxiv) = normalize_arxiv(arg) {
        return Ok((IdKind::Arxiv, arxiv));
    }
    if let Some(isbn) = normalize_isbn(arg) {
        return Ok((IdKind::Isbn, isbn));
    }
    let key = strip(arg);
    if key.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(format!(
            "{arg}: not a key of the library, nor a DOI, arXiv id or ISBN"
        ));
    }
    Ok((IdKind::Key, key.to_lowercase()))
}

/// The identifiers an entry answers to (`entry_identifiers`): its key, its
/// former keys, its DOI, its arXiv identifiers (of the DOI, and of `eprint` if
/// they differ), its ISBNs (`,` or `;`).
pub fn entry_identifiers(entry: &Entry) -> Vec<(IdKind, String)> {
    let mut ids: Vec<(IdKind, String)> = Vec::new();
    let mut add = |kind: IdKind, v: String| {
        if !ids.contains(&(kind, v.clone())) {
            ids.push((kind, v));
        }
    };
    add(IdKind::Key, entry.key.to_lowercase());
    for alias in crate::entry::ids(entry) {
        add(IdKind::Key, alias.to_lowercase());
    }
    if let Some(doi) = normalize_doi(val(entry, "doi")) {
        add(IdKind::Doi, doi);
    }
    for arxiv in [doi_arxiv(entry), eprint_arxiv(entry)]
        .into_iter()
        .flatten()
    {
        add(IdKind::Arxiv, arxiv);
    }
    for isbn in val(entry, "isbn")
        .split([',', ';'])
        .map(strip)
        .filter(|i| !i.is_empty())
        .filter_map(normalize_isbn)
    {
        add(IdKind::Isbn, isbn);
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, ty: &str, fields: &[(&str, &str)]) -> Entry {
        let mut e = Entry::new(ty, key);
        for (k, v) in fields {
            e.set(k, *v);
        }
        e
    }

    const SMITH: [(&str, &str); 3] = [
        ("author", "Smith, John and Doe, Jane"),
        ("title", "Climate sensitivity revisited"),
        ("year", "2020"),
    ];

    fn with(
        base: &[(&'static str, &'static str)],
        more: &[(&'static str, &'static str)],
    ) -> Vec<(&'static str, &'static str)> {
        base.iter().chain(more).copied().collect()
    }

    fn kind(a: &Entry, b: &Entry) -> Kind {
        same_paper(a, b, None).kind
    }

    #[test]
    fn identifiers() {
        let a = entry(
            "a",
            "article",
            &[("doi", "10.1000/ABC123"), ("title", "One")],
        );
        let b = entry(
            "b",
            "article",
            &[
                ("doi", "https://doi.org/10.1000/abc123"),
                ("title", "Other title"),
            ],
        );
        assert_eq!(
            same_paper(&a, &b, None),
            Verdict::new(Kind::Same, "same DOI 10.1000/abc123")
        );
        let a = entry("a", "article", &with(&SMITH, &[("doi", "10.1000/abc123")]));
        let b = entry(
            "b",
            "misc",
            &with(&SMITH, &[("doi", "10.5194/egusphere-2019-1")]),
        );
        assert_eq!(kind(&a, &b), Kind::Different);
        // arXiv
        let a = entry(
            "a",
            "article",
            &[
                ("eprint", "2101.01234v2"),
                ("archivePrefix", "arXiv"),
                ("title", "One"),
            ],
        );
        let b = entry(
            "b",
            "article",
            &[("eprint", "arXiv:2101.01234"), ("title", "Two")],
        );
        assert_eq!(kind(&a, &b), Kind::Same);
        let a = entry(
            "a",
            "article",
            &[("doi", "10.48550/arXiv.2101.01234"), ("title", "One")],
        );
        assert_eq!(kind(&a, &b), Kind::Same);
        let a = entry(
            "a",
            "article",
            &[
                ("eprint", "2101.01234"),
                ("archivePrefix", "HAL"),
                ("author", "A, B"),
            ],
        );
        let b = entry(
            "b",
            "article",
            &[("eprint", "2101.01234"), ("author", "C, D")],
        );
        assert_eq!(kind(&a, &b), Kind::Different);
        // isbn
        let a = entry(
            "a",
            "book",
            &[("isbn", "0-306-40615-2"), ("title", "A book")],
        );
        let b = entry(
            "b",
            "book",
            &[("isbn", "978-0-306-40615-7"), ("title", "The book")],
        );
        assert_eq!(kind(&a, &b), Kind::Same);
        let b = entry(
            "b",
            "incollection",
            &[("isbn", "0-306-40615-2"), ("title", "Chapter")],
        );
        assert_eq!(kind(&a, &b), Kind::Different);
        // authors and title
        let a = entry(
            "a",
            "article",
            &[("author", "M{\\\"u}ller, H."), ("title", "Über alles")],
        );
        let b = entry(
            "b",
            "article",
            &[("author", "Müller, Hans"), ("title", "{\\\"U}ber alles")],
        );
        assert_eq!(same_paper(&a, &b, None).reason, "same title and authors");
        let a = entry("a", "article", &[("title", "Editorial")]);
        assert_eq!(kind(&a, &a.clone()), Kind::Different);
    }

    #[test]
    fn title_guard() {
        let book = entry(
            "a",
            "book",
            &[("doi", "10.1000/book"), ("title", "Handbook of climate")],
        );
        let chapter = entry(
            "b",
            "incollection",
            &[("doi", "10.1000/book"), ("title", "Sea level rise")],
        );
        let v = same_paper(&book, &chapter, None);
        assert_eq!(v.kind, Kind::Ask);
        assert!(v.reason.contains("entry types"));
        let b = entry(
            "b",
            "article",
            &[("doi", "10.1000/book"), ("title", "Sea level rise")],
        );
        let a = entry(
            "a",
            "article",
            &[("doi", "10.1000/book"), ("title", "Handbook of climate")],
        );
        assert_eq!(kind(&a, &b), Kind::Same);
    }

    #[test]
    fn distinct_pairs() {
        let a = entry("a", "article", &SMITH);
        let b = entry("b", "article", &SMITH);
        let mut d = Distinct::new();
        assert_eq!(same_paper(&a, &b, Some(&d)).kind, Kind::Same);
        d.add(&a, &b);
        assert!(d.changed);
        assert_eq!(same_paper(&b, &a, Some(&d)).kind, Kind::Different);
        assert_eq!(
            d.to_text(),
            "{\n \"distinct\": [\n  [\n   {\n    \"authortitle\": \"smith doe|climate sensitivity revisited\"\n   },\n   {\n    \"authortitle\": \"smith doe|climate sensitivity revisited\"\n   }\n  ]\n ]\n}\n"
        );
        let mut d1 = Distinct::new();
        d1.update(&d);
        d1.update(&d);
        assert_eq!(d1.pairs.len(), 1);
    }

    #[test]
    fn groups() {
        let article = entry(
            "article",
            "article",
            &with(&SMITH, &[("doi", "10.1000/abc123"), ("journal", "Nature")]),
        );
        let copy = entry("copy", "article", &SMITH);
        let preprint = entry(
            "preprint",
            "misc",
            &with(&SMITH, &[("doi", "10.5194/egusphere-2019-1")]),
        );
        let groups = duplicate_groups(&[&copy, &preprint, &article], None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].entries, [2, 0]); // most fields first
        let index = IdentityIndex::of([&copy, &preprint, &article]);
        let found = find_duplicates(&article, &index, None, Some(2));
        assert_eq!(found.iter().map(|(h, _)| *h).collect::<Vec<_>>(), [0]);
    }

    #[test]
    fn names() {
        assert_eq!(split_names("A, B and  C, D\tand\nE"), ["A, B", "C, D", "E"]);
        assert_eq!(split_names("a and and b"), ["a", "and b"]);
        assert_eq!(parse_name(""), (String::new(), String::new()));
        assert_eq!(
            parse_name("Jane {van der Berg}"),
            ("{van der Berg}".into(), "Jane".into())
        );
        assert_eq!(
            normalize_arxiv("hep-th/9901001v3").as_deref(),
            Some("hep-th/9901001")
        );
        assert_eq!(
            normalize_arxiv("math.AG/0601001").as_deref(),
            Some("math.ag/0601001")
        );
        assert_eq!(
            normalize_arxiv("https://arxiv.org/pdf/2101.01234v2.pdf").as_deref(),
            Some("2101.01234")
        );
        assert_eq!(normalize_arxiv("21011.01234"), None);
        assert_eq!(normalize_doi("DOI: 10.1/X").as_deref(), Some("10.1/x"));
        assert_eq!(normalize_doi("10.1"), None);
    }

    /// The words of a name may be on several lines (`parse_name` matches with DOTALL).
    #[test]
    fn a_name_on_several_lines() {
        assert_eq!(parse_name("A\nB C"), ("C".into(), "A\nB".into()));
        assert_eq!(parse_name("Jane\n Doe"), ("Doe".into(), "Jane".into()));
        assert_eq!(
            parse_name("Jane {van\nder Berg}"),
            ("{van\nder Berg}".into(), "Jane".into())
        );
        assert_eq!(family_name_ids("A\nB C"), ["c"]);
    }

    /// The brace depth never goes below 0: a stray `}` does not stop the split (`a} and b`).
    #[test]
    fn a_stray_closing_brace() {
        assert_eq!(split_names("a} and b"), ["a}", "b"]);
        assert_eq!(family_name_ids("a} and b"), ["a", "b"]);
    }

    /// `distinct.json`: a `distinct` that is not an array makes the file unreadable (one warning, no
    /// pairs); the pairs that are not pairs of objects are ignored (bibtex.md 9.4).
    #[test]
    fn a_distinct_file_with_bad_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("distinct.json")
            .to_string_lossy()
            .into_owned();
        for text in [
            "{\"distinct\": 5}",
            "{\"distinct\": null}",
            "{\"distinct\": {}}",
            "[1, 2]",
        ] {
            std::fs::write(&path, text).unwrap();
            assert!(Distinct::load(&path).pairs.is_empty(), "{text}");
        }
        std::fs::write(
            &path,
            "{\"distinct\": [[\"a\", \"b\"], [{\"title\": \"t\"}, 3], [{\"title\": \"t\"}, {\"title\": \"u\"}]]}",
        )
        .unwrap();
        let pairs = Distinct::load(&path).pairs;
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0][1]["title"], "u");
        // an object without `distinct`: no pairs, no warning
        std::fs::write(&path, "{}").unwrap();
        assert!(Distinct::load(&path).pairs.is_empty());
    }
}
