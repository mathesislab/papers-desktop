//! The DOIs of a text: `papers.extract` (`parse_dois`, the metadata of a PDF as
//! text) and what it uses of `re`, written out. Python's `re` is not available
//! (the lookbehind of the DOI pattern, `\w`, `\d` and `\s` of a `str`), so each
//! pattern is a scan of the characters that finds what the regular expression
//! finds: the leftmost match, the greedy repetitions with their backtracking
//! where they have any, `\w` as `str.isalnum()` and `_`, `\d` as `str.isdecimal()`,
//! `\s` as `str.isspace()`.

use std::fmt;

use crate::bib::pystr::{is_alnum, is_space, strip};
use crate::names::{ascii, is_decimal};

/// `DOIParsingError` (a `ValueError` of papers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoiParsingError(pub String);

impl fmt::Display for DoiParsingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DoiParsingError {}

/// The prefixes of the DOIs of data repositories (Zenodo, Dryad, figshare, USGS,
/// SEANOE, PANGAEA, NSIDC): a data DOI cited in a paper is not the paper's own.
pub const DATA_REPOSITORY_PREFIXES: [&str; 7] = [
    "10.5281", "10.5061", "10.6084", "10.5066", "10.17882", "10.1594", "10.5067",
];

pub fn is_data_repository_doi(doi: &str) -> bool {
    let lower = doi.to_lowercase();
    let prefix = lower.split('/').next().unwrap_or("");
    DATA_REPOSITORY_PREFIXES.contains(&prefix)
}

type Chars = Vec<char>;

fn chars(s: &str) -> Chars {
    s.chars().collect()
}

fn string(c: &[char]) -> String {
    c.iter().collect()
}

/// `[-._;()/:a-z0-9]`, without `re.IGNORECASE`.
fn in_class(c: char) -> bool {
    matches!(c, '-' | '.' | '_' | ';' | '(' | ')' | '/' | ':' | 'a'..='z' | '0'..='9')
}

/// The same class under `re.IGNORECASE`: it also takes the upper case letters, and the
/// four characters that Python's `re` folds into a letter of it.
fn in_class_ci(c: char) -> bool {
    in_class(c)
        || c.is_ascii_uppercase()
        || matches!(c, '\u{130}' | '\u{131}' | '\u{17f}' | '\u{212a}')
}

/// Whether `c` matches the ASCII character `p` under `re.IGNORECASE`.
fn eq_ci(p: char, c: char) -> bool {
    if p == c {
        return true;
    }
    if !p.is_ascii_alphabetic() {
        return false;
    }
    let l = p.to_ascii_lowercase();
    c.to_ascii_lowercase() == l
        || (l == 'i' && matches!(c, '\u{130}' | '\u{131}'))
        || (l == 's' && c == '\u{17f}')
        || (l == 'k' && c == '\u{212a}')
}

/// Whether `t[i..]` starts with the ASCII `s`.
fn starts(t: &[char], i: usize, s: &str) -> bool {
    s.chars().enumerate().all(|(n, p)| t.get(i + n) == Some(&p))
}

fn starts_ci(t: &[char], i: usize, s: &str) -> Option<usize> {
    let mut k = i;
    for p in s.chars() {
        match t.get(k) {
            Some(&c) if eq_ci(p, c) => k += 1,
            _ => return None,
        }
    }
    Some(k)
}

/// `10\.\d{4,9}/` at `i`: the index after the slash.
fn registrant(t: &[char], i: usize) -> Option<usize> {
    if !starts(t, i, "10.") {
        return None;
    }
    let mut k = i + 3;
    let from = k;
    while k < t.len() && is_decimal(t[k]) {
        k += 1;
    }
    // \d{4,9}/ : more digits than that cannot be followed by the slash
    if (4..=9).contains(&(k - from)) && t.get(k) == Some(&'/') {
        Some(k + 1)
    } else {
        None
    }
}

/// `10\.\d{4,9}/[-._;()/:a-z0-9]+` at `i`: the end of the match.
fn doi_at(t: &[char], i: usize, class: fn(char) -> bool) -> Option<usize> {
    let mut k = registrant(t, i)?;
    let from = k;
    while k < t.len() && class(t[k]) {
        k += 1;
    }
    (k > from).then_some(k)
}

/// `(?<![\w.\-])(10\.\d{4,9}/[-._;()/:a-z0-9]+)`: `findall`.
fn find_dois(t: &[char]) -> Vec<String> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if t[i] == '1'
            && (i == 0 || !(is_alnum(t[i - 1]) || matches!(t[i - 1], '_' | '.' | '-')))
            && let Some(end) = doi_at(t, i, in_class)
        {
            found.push(string(&t[i..end]));
            i = end;
            continue;
        }
        i += 1;
    }
    found
}

/// The maximal run of whitespace at `i`: its end, and whether it holds a newline.
fn whitespace_run(t: &[char], i: usize) -> (usize, bool) {
    let mut k = i;
    let mut newline = false;
    while k < t.len() && is_space(t[k]) {
        newline |= t[k] == '\n';
        k += 1;
    }
    (k, newline)
}

/// `re.sub(r'(10\.)\s*\n\s*(\d{4,9}/)', r'\1\2', t)`
fn join_registrant(t: &[char]) -> Chars {
    let mut out = Chars::new();
    let mut i = 0;
    while i < t.len() {
        if starts(t, i, "10.") {
            let (j, newline) = whitespace_run(t, i + 3);
            if newline && j > i + 3 {
                // after the whitespace: \d{4,9}/
                let mut k = j;
                while k < t.len() && is_decimal(t[k]) {
                    k += 1;
                }
                if (4..=9).contains(&(k - j)) && t.get(k) == Some(&'/') {
                    out.extend("10.".chars());
                    out.extend_from_slice(&t[j..=k]);
                    i = k + 1;
                    continue;
                }
            }
        }
        out.push(t[i]);
        i += 1;
    }
    out
}

/// `re.sub(r'(10\.1073/pnas\.)\s*\n\s*(\d)', r'\1\2', t)`
fn join_pnas(t: &[char]) -> Chars {
    const HEAD: &str = "10.1073/pnas.";
    let n = HEAD.chars().count();
    let mut out = Chars::new();
    let mut i = 0;
    while i < t.len() {
        if starts(t, i, HEAD) {
            let (j, newline) = whitespace_run(t, i + n);
            if newline && j < t.len() && is_decimal(t[j]) {
                out.extend(HEAD.chars());
                out.push(t[j]);
                i = j + 1;
                continue;
            }
        }
        out.push(t[i]);
        i += 1;
    }
    out
}

/// `re.sub(r'(10\.\d{4,9}/[-._;()/:a-z0-9]+-)\s*\n\s*([a-z0-9])', r'\1\2', t)`
fn join_hyphenated(t: &[char]) -> Chars {
    let mut out = Chars::new();
    let mut i = 0;
    while i < t.len() {
        if t[i] == '1'
            && let Some(from) = registrant(t, i)
        {
            let mut end = from;
            while end < t.len() && in_class(t[end]) {
                end += 1;
            }
            // the group ends with a hyphen, and what follows is the whitespace
            if end - from >= 2 && t[end - 1] == '-' {
                let (j, newline) = whitespace_run(t, end);
                if newline && j < t.len() && matches!(t[j], 'a'..='z' | '0'..='9') {
                    out.extend_from_slice(&t[i..end]);
                    out.push(t[j]);
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(t[i]);
        i += 1;
    }
    out
}

/// `_normalize_doi_text`: the text in lower case, without the invisible characters, with the
/// DOIs that a line end cuts joined.
pub fn normalize_doi_text(txt: &str) -> String {
    string(&normalize_chars(txt))
}

fn normalize_chars(txt: &str) -> Chars {
    // U+200B zero-width space, U+00AD soft hyphen, U+2013 en dash, U+2014 em dash,
    // \x02 sometimes used instead of the slash
    let clean: String = txt
        .replace(['\u{200b}', '\u{ad}'], "")
        .replace(['\u{2013}', '\u{2014}'], "-")
        .replace('\x02', "/");
    let lower = chars(&clean.to_lowercase());
    join_hyphenated(&join_pnas(&join_registrant(&lower)))
}

/// `ARXIV.search`: `arxiv:\s*(\d{4}\.\d{4,5})`
fn find_arxiv(t: &[char]) -> Option<String> {
    let mut i = 0;
    while i + 6 <= t.len() {
        if starts(t, i, "arxiv:") {
            let (mut k, _) = whitespace_run(t, i + 6);
            let a = k;
            while k < t.len() && k < a + 4 && is_decimal(t[k]) {
                k += 1;
            }
            if k == a + 4 && t.get(k) == Some(&'.') {
                let b = k + 1;
                let mut e = b;
                while e < t.len() && e < b + 5 && is_decimal(t[e]) {
                    e += 1;
                }
                if e - b >= 4 {
                    return Some(string(&t[a..e]));
                }
            }
        }
        i += 1;
    }
    None
}

/// `LABELED_REGEXP.findall`: `(?:doi:?\s*|doi\.org/)(10\.\d{4,9}/[-._;()/:a-z0-9]+)`
fn find_labeled(t: &[char]) -> Vec<String> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if starts(t, i, "doi") {
            let mut k = i + 3;
            if t.get(k) == Some(&':') {
                k += 1;
            }
            let (k, _) = whitespace_run(t, k);
            let end = doi_at(t, k, in_class).or_else(|| {
                if starts(t, i, "doi.org/") {
                    doi_at(t, i + 8, in_class)
                } else {
                    None
                }
            });
            let start = if doi_at(t, k, in_class).is_some() {
                k
            } else {
                i + 8
            };
            if let Some(end) = end {
                found.push(string(&t[start..end]));
                i = end;
                continue;
            }
        }
        i += 1;
    }
    found
}

/// `doi\.org/(DOI)` and `dx\.doi\.org/(DOI)`: the DOIs after the host.
fn find_after(t: &[char], host: &str) -> Vec<String> {
    let n = host.chars().count();
    let mut found = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if starts(t, i, host)
            && let Some(end) = doi_at(t, i + n, in_class)
        {
            found.push(string(&t[i + n..end]));
            i = end;
            continue;
        }
        i += 1;
    }
    found
}

/// `www\.[a-z]+\.org/[a-z/]+/(DOI)`
fn find_www(t: &[char]) -> Vec<String> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if starts(t, i, "www.") {
            let mut k = i + 4;
            let from = k;
            while k < t.len() && t[k].is_ascii_lowercase() {
                k += 1;
            }
            if k > from && starts(t, k, ".org/") {
                let q = k + 5;
                let mut run = q;
                while run < t.len() && (t[run].is_ascii_lowercase() || t[run] == '/') {
                    run += 1;
                }
                // [a-z/]+ takes the run, then gives back until a slash is followed by the DOI
                let mut e = run;
                let mut hit = None;
                while e > q + 1 {
                    e -= 1;
                    if t[e] == '/'
                        && let Some(end) = doi_at(t, e + 1, in_class)
                    {
                        hit = Some((e + 1, end));
                        break;
                    }
                }
                if let Some((s, end)) = hit {
                    found.push(string(&t[s..end]));
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }
    found
}

/// `re.search(r'\.s\d+\.?$', s)`: where the match starts.
fn supplemental(s: &[char]) -> Option<usize> {
    (0..s.len()).find(|&j| {
        if s[j] != '.' || s.get(j + 1) != Some(&'s') {
            return false;
        }
        let mut k = j + 2;
        let from = k;
        while k < s.len() && is_decimal(s[k]) {
            k += 1;
        }
        if k == from {
            return false;
        }
        if s.get(k) == Some(&'.') {
            k += 1;
        }
        k == s.len()
    })
}

/// `_pick_doi`: the most likely article DOI among the matches (in order of appearance).
fn pick_doi(matches: &[String], txt_lower: &[char]) -> String {
    let mut doi = matches[0].clone();
    // DOIs in a URL are more likely the article's own than the citations
    if matches.len() > 1 {
        // the three URL patterns, in the order of papers
        let url_patterns = [
            find_www(txt_lower),
            find_after(txt_lower, "doi.org/"),
            find_after(txt_lower, "dx.doi.org/"),
        ];
        for url_matches in url_patterns {
            for m in matches {
                if url_matches.contains(m) {
                    doi = m.clone();
                    break;
                }
            }
            if url_matches.contains(&doi) {
                break;
            }
        }
    }
    // the supplement of an article: prefer the article
    if matches.len() > 1 && doi == matches[0] {
        let first = chars(&matches[0]);
        if let Some(at) = supplemental(&first) {
            let base = string(&first[..at]);
            for m in &matches[1..] {
                if m.starts_with(&base) && supplemental(&chars(m)).is_none() {
                    doi = m.clone();
                    break;
                }
            }
        }
    }
    doi
}

/// What follows a DOI in the text and is not part of it.
const SUFFIXES: [&str; 12] = [
    "/-/dcsupplemental",
    "/-/dc",
    "(0123456789",
    ".pdf",
    "preprint",
    "received",
    "published",
    "edited",
    "advance",
    "full",
    "abstract",
    "-supplement",
];

fn find_sub(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    (0..hay.len().checked_sub(needle.len() - 1)?).find(|&i| hay[i..i + needle.len()] == *needle)
}

/// `_clean_doi`
fn clean_doi(doi: &str) -> Result<String, DoiParsingError> {
    let mut doi = chars(doi);
    let lower = chars(&string(&doi).to_lowercase());
    for suffix in SUFFIXES.iter().chain(["supplement"].iter()) {
        let Some(pos) = find_sub(&lower, &chars(suffix)) else {
            continue;
        };
        if pos == 0 {
            continue;
        }
        if suffix.chars().all(|c| c.is_ascii_alphabetic()) {
            // word-only suffixes follow a valid DOI character
            let prev = doi.get(pos - 1).copied().unwrap_or('\0');
            if is_alnum(prev) || matches!(prev, '.' | '-' | '_' | '/') {
                doi.truncate(pos);
                break;
            }
        } else {
            doi.truncate(pos);
            break;
        }
    }
    while matches!(doi.last(), Some('.' | ':')) {
        doi.pop();
    }
    // the parentheses of a DOI are balanced
    while doi.last() == Some(&')')
        && doi.iter().filter(|&&c| c == ')').count() > doi.iter().filter(|&&c| c == '(').count()
    {
        doi.pop();
    }
    if doi.len() <= 8 {
        return Err(DoiParsingError(format!(
            "failed to extract doi: {}",
            string(&doi)
        )));
    }
    Ok(string(&doi))
}

/// `parse_dois`: the DOIs found in the text, the most likely article DOI first. The DOIs of
/// data repositories come last when others are present. `tail` is the end of the document
/// (the cover page that publishers append): its labeled DOIs (`doi:`, a doi.org URL) are added
/// after those of the text.
pub fn parse_dois(txt: &str, tail: &str) -> Result<Vec<String>, DoiParsingError> {
    let mut tail_dois: Vec<String> = Vec::new();
    for candidate in find_labeled(&normalize_chars(tail)) {
        if let Ok(candidate) = clean_doi(&candidate)
            && !tail_dois.contains(&candidate)
        {
            tail_dois.push(candidate);
        }
    }
    let dois = match parse_dois_of_text(txt) {
        Ok(dois) => dois,
        Err(error) => {
            if tail_dois.is_empty() {
                return Err(error);
            }
            Vec::new()
        }
    };
    let mut all = dois.clone();
    all.extend(tail_dois.into_iter().filter(|d| !dois.contains(d)));
    Ok(all)
}

fn parse_dois_of_text(txt: &str) -> Result<Vec<String>, DoiParsingError> {
    let txt_lower = normalize_chars(txt);
    let found = find_dois(&txt_lower);
    if found.is_empty() {
        // the arXiv pattern
        if let Some(id) = find_arxiv(&txt_lower) {
            return Ok(vec![format!("10.48550/arXiv.{id}")]);
        }
        return Err(DoiParsingError("no DOI found in the text".into()));
    }
    let mut matches: Vec<String> = Vec::new();
    for m in found {
        if !matches.contains(&m) {
            matches.push(m);
        }
    }
    let articles: Vec<String> = matches
        .iter()
        .filter(|m| !is_data_repository_doi(m))
        .cloned()
        .collect();
    let articles = if articles.is_empty() {
        matches.clone()
    } else {
        articles
    };
    let data: Vec<String> = matches
        .iter()
        .filter(|m| !articles.contains(m))
        .cloned()
        .collect();
    let doi = pick_doi(&articles, &txt_lower);
    let mut candidates = vec![doi.clone()];
    candidates.extend(articles.iter().filter(|m| **m != doi).cloned());
    candidates.extend(data);
    let mut dois: Vec<String> = Vec::new();
    for candidate in candidates {
        if let Ok(candidate) = clean_doi(&candidate)
            && !dois.contains(&candidate)
        {
            dois.push(candidate);
        }
    }
    if dois.is_empty() {
        return Err(DoiParsingError(format!("failed to extract doi: {doi}")));
    }
    Ok(dois)
}

/// `parse_doi`: the first of `parse_dois`.
pub fn parse_doi(txt: &str) -> Result<String, DoiParsingError> {
    Ok(parse_dois(txt, "")?.remove(0))
}

/// `isvaliddoi`: the text is a DOI, up to its case.
pub fn isvaliddoi(doi: &str) -> bool {
    match parse_doi(&format!("doi:{doi}")) {
        Ok(doi2) => doi.to_lowercase() == doi2.to_lowercase(),
        Err(_) => false,
    }
}

/// `_parse_doi_from_metadata_string`: the DOI of the XMP metadata of a PDF.
pub fn doi_from_xmp(xmp: &str) -> Option<String> {
    const TAGS: [(&str, &str); 4] = [
        ("<prism:doi>", "</prism:doi>"),
        ("<dc:identifier>doi:", "</dc:identifier>"),
        ("<pdfx:doi>", "</pdfx:doi>"),
        ("<crossmark:DOI>", "</crossmark:DOI>"),
    ];
    let t = chars(xmp);
    for (open, close) in TAGS {
        for i in 0..t.len() {
            let Some(k) = starts_ci(&t, i, open) else {
                continue;
            };
            // (10\.\d{4,9}/[class]+), then the closing tag: the class stops at the `<`
            if let Some(end) = doi_at(&t, k, in_class_ci)
                && starts_ci(&t, end, close).is_some()
            {
                return Some(string(&t[k..end]));
            }
        }
    }
    None
}

/// `_parse_doi_from_info_fields`: the DOI in the subject, the keywords or the title of the
/// document information (`re.search`, ignoring the case).
pub fn doi_from_info_fields(subject: &str, keywords: &str, title: &str) -> Option<String> {
    for value in [subject, keywords, title] {
        let t = chars(value);
        for i in 0..t.len() {
            if let Some(end) = doi_at(&t, i, in_class_ci) {
                return Some(string(&t[i..end]));
            }
        }
    }
    None
}

/// `str.split()`
pub fn py_split(s: &str) -> Vec<&str> {
    s.split(is_space).filter(|w| !w.is_empty()).collect()
}

fn normalize_for_match(s: &str) -> String {
    // the markup of crossref titles, e.g. CO<sub>2</sub>: `<[^>]+>`
    let t = chars(s);
    let mut plain = Chars::new();
    let mut i = 0;
    while i < t.len() {
        if t[i] == '<'
            && let Some(close) = t[i + 1..].iter().position(|&c| c == '>')
            && close >= 1
        {
            i += close + 2;
            continue;
        }
        plain.push(t[i]);
        i += 1;
    }
    ascii(&string(&plain))
        .to_lowercase()
        .chars()
        .filter(|c| matches!(c, 'a'..='z' | '0'..='9'))
        .collect()
}

/// `title_in_text`: the title appears in the text, ignoring case, accents, spacing and
/// punctuation (so that line breaks and hyphenation do not matter).
pub fn title_in_text(title: &str, txt: &str) -> bool {
    let title = normalize_for_match(title);
    if title.chars().count() < 10 {
        return true; // too short to tell
    }
    normalize_for_match(txt).contains(&title)
}

/// The least letters and digits of a title that `title_in_text_strict` takes as telling.
pub const STRICT_TITLE_LENGTH: usize = 20;

/// The title appears in the text, as `title_in_text` has it, but a title of less than
/// [`STRICT_TITLE_LENGTH`] letters and digits is too short to tell (papers: less than ten, and it
/// lets those pass). For a search of OpenAlex (papers-rs, not in papers), which has hits whose
/// title is garbled, or is the name of a journal or a word that any text has.
pub fn title_in_text_strict(title: &str, txt: &str) -> bool {
    let title = normalize_for_match(title);
    title.chars().count() >= STRICT_TITLE_LENGTH && normalize_for_match(txt).contains(&title)
}

/// `query_text`: the words of the text for a full-text query, without the paragraphs that
/// start with "reference". The error is the message of papers' `ValueError`.
pub fn query_text(txt: &str, max_query_words: i64) -> Result<String, String> {
    let query: Vec<&str> = txt
        .split("\n\n")
        .filter(|p| !p.to_lowercase().starts_with("reference"))
        .collect();
    let query_txt = query.join(" ");
    let words = py_split(strip(&query_txt));
    let n = words.len() as i64;
    let end = if max_query_words >= 0 {
        max_query_words.min(n)
    } else {
        (n + max_query_words).max(0)
    } as usize;
    let query_txt = words[..end].join(" ");
    if py_split(&query_txt).len() < 3 {
        return Err(format!(
            "needs at least 3 query words, got: {}",
            crate::config::json::py_repr_str(&query_txt)
        ));
    }
    Ok(query_txt)
}
