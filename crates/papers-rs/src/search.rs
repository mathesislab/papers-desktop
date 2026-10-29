//! The selection of `papers list`: the words and the fields searched (substring,
//! exact with wildcards, or fuzzy), the tags, the files, the entries to review,
//! the duplicates, `--invert` (`papers.cli.entry_cmds.listcmd`, `papers.duplicate`).
//!
//! The fuzzy match is rapidfuzz's `token_set_ratio` ([`token_set_ratio`], by the
//! `rapidfuzz` crate's Indel distance, checked against the Python package).
//!
//! ```
//! use papers_rs::bib::Bib;
//! use papers_rs::library::Library;
//! use papers_rs::search::Search;
//!
//! let bib = Bib::parse("@article{a, title = {Ice sheets}, year = 2020}\n@article{b, title = {Sea level}, year = 2019}\n");
//! let entries: Vec<_> = bib.entries().collect();
//! let found = Search { title: vec!["ice".into()], ..Search::default() }.select(&entries, None, "/").unwrap();
//! assert_eq!(found.len(), 1);
//! assert_eq!(found[0].key, "a");
//! ```

use std::collections::HashMap;

use crate::bib::Entry;
use crate::bib::encoding::{family_names, parse_keywords};
use crate::bib::pystr::{is_space, is_word, strip};
use crate::config::paths::exists;
use crate::entry::{files, fnmatch, tag_matches, val};
use crate::identity::{Distinct, Identifiers, duplicate_groups, normalize_doi, normalize_text};
use crate::names::{is_decimal, is_valid_key};

/// The ratio of `--duplicates --fuzzy` when `--fuzzy-ratio` does not say.
pub const FUZZY_RATIO: i64 = 80;

/// The ratio of the fuzzy search of a field when `--fuzzy-ratio` does not say.
pub const FUZZY_SEARCH_RATIO: i64 = 50;

/// `--by`: the field that the duplicates share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    Key,
    Doi,
    Title,
}

impl By {
    pub fn parse(name: &str) -> Option<By> {
        match name {
            "key" => Some(By::Key),
            "doi" => Some(By::Doi),
            "title" => Some(By::Title),
            _ => None,
        }
    }
}

/// What `papers list` selects. A list that is empty was not given.
#[derive(Debug, Clone, Default)]
pub struct Search {
    /// the words of the full search
    pub words: Vec<String>,
    pub strict: bool,
    pub fuzzy: bool,
    pub fuzzy_ratio: Option<i64>,
    pub invert: bool,
    pub any: bool,
    pub author: Vec<String>,
    pub first_author: Vec<String>,
    pub year: Vec<String>,
    pub title: Vec<String>,
    pub abstract_text: Vec<String>,
    pub key: Vec<String>,
    pub doi: Vec<String>,
    pub keywords: Vec<String>,
    pub by: Option<By>,
    pub duplicates: bool,
    pub has_file: bool,
    pub no_file: bool,
    pub broken_file: bool,
    pub review_required: bool,
}

/// `rapidfuzz.fuzz.token_set_ratio(a, b, score_cutoff=cutoff)`: 0 if the score is below the cutoff.
pub fn token_set_ratio(a: &str, b: &str, cutoff: f64) -> f64 {
    use rapidfuzz::distance::indel;
    fn tokens(s: &str) -> Vec<String> {
        s.split(is_space)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect()
    }
    let norm = |dist: usize, lensum: usize| -> f64 {
        let score = if lensum > 0 {
            100.0 - 100.0 * dist as f64 / lensum as f64
        } else {
            100.0
        };
        if score >= cutoff { score } else { 0.0 }
    };
    let mut tokens_a = tokens(a);
    let mut tokens_b = tokens(b);
    tokens_a.sort();
    tokens_a.dedup();
    tokens_b.sort();
    tokens_b.dedup();
    if tokens_a.is_empty() || tokens_b.is_empty() {
        return 0.0;
    }
    let intersect: Vec<&String> = tokens_a.iter().filter(|t| tokens_b.contains(t)).collect();
    let diff_ab: Vec<&String> = tokens_a.iter().filter(|t| !tokens_b.contains(t)).collect();
    let diff_ba: Vec<&String> = tokens_b.iter().filter(|t| !tokens_a.contains(t)).collect();
    if !intersect.is_empty() && (diff_ab.is_empty() || diff_ba.is_empty()) {
        return 100.0;
    }
    let join = |v: &[&String]| v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
    let (ab, ba) = (join(&diff_ab), join(&diff_ba));
    let ab_len = ab.chars().count();
    let ba_len = ba.chars().count();
    let sect_len = join(&intersect).chars().count();
    let sect_ab_len = sect_len + usize::from(sect_len != 0) + ab_len;
    let sect_ba_len = sect_len + usize::from(sect_len != 0) + ba_len;
    let lensum = sect_ab_len + sect_ba_len;
    let cutoff_distance = (lensum as f64 * (1.0 - cutoff / 100.0)).ceil();
    let dist = indel::distance(ab.chars(), ba.chars());
    let mut result = 0.0;
    if dist as f64 <= cutoff_distance {
        result = norm(dist, lensum);
    }
    if sect_len == 0 {
        return result;
    }
    let sect_ab_ratio = norm(usize::from(sect_len != 0) + ab_len, sect_len + sect_ab_len);
    let sect_ba_ratio = norm(usize::from(sect_len != 0) + ba_len, sect_len + sect_ba_len);
    result.max(sect_ab_ratio).max(sect_ba_ratio)
}

/// The pairs of entries whose authors and titles score at least `ratio` (`fuzzy_pairs`), as
/// the positions of the two entries: for listing only, papers never merges on this.
pub fn fuzzy_pairs(entries: &[&Entry], ratio: i64) -> Vec<(usize, usize)> {
    let ids: Vec<(usize, String)> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let ids = Identifiers::of(e);
            (i, format!("{} {}", ids.author, ids.title))
        })
        .filter(|(_, s)| !strip(s).is_empty())
        .collect();
    let mut pairs = Vec::new();
    for (n, (a, sa)) in ids.iter().enumerate() {
        for (b, sb) in &ids[n + 1..] {
            if token_set_ratio(sa, sb, ratio as f64) != 0.0 {
                pairs.push((*a, *b));
            }
        }
    }
    pairs
}

// DOIs
// ----

const DATA_REPOSITORY_PREFIXES: [&str; 7] = [
    "10.5281", "10.5061", "10.6084", "10.5066", "10.17882", "10.1594", "10.5067",
];

/// `[-._;()/:a-z0-9]`
fn doi_char(c: char) -> bool {
    c.is_ascii_lowercase()
        || c.is_ascii_digit()
        || matches!(c, '-' | '.' | '_' | ';' | '(' | ')' | '/' | ':')
}

/// The end of `10\.\d{4,9}/[-._;()/:a-z0-9]+` that starts at `i`.
fn doi_at(chars: &[char], i: usize) -> Option<usize> {
    if chars.get(i..i + 3)? != ['1', '0', '.'] {
        return None;
    }
    let mut j = i + 3;
    while j < chars.len() && is_decimal(chars[j]) {
        j += 1;
    }
    let digits = j - (i + 3);
    if !(4..=9).contains(&digits) || chars.get(j) != Some(&'/') {
        return None;
    }
    let start = j + 1;
    let mut k = start;
    while k < chars.len() && doi_char(chars[k]) {
        k += 1;
    }
    (k > start).then_some(k)
}

/// `REGEXP.findall`: the DOIs not glued to a preceding word, number, dot or hyphen.
fn find_dois(chars: &[char]) -> Vec<String> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let glued = i > 0 && (is_word(chars[i - 1]) || matches!(chars[i - 1], '.' | '-'));
        if !glued && let Some(end) = doi_at(chars, i) {
            found.push(chars[i..end].iter().collect());
            i = end;
        } else {
            i += 1;
        }
    }
    found
}

/// The DOIs that follow `prefix` (a literal text) in the text.
fn find_after(chars: &[char], prefix: &[char]) -> Vec<String> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i..].starts_with(prefix)
            && let Some(end) = doi_at(chars, i + prefix.len())
        {
            found.push(chars[i + prefix.len()..end].iter().collect());
            i = end;
        } else {
            i += 1;
        }
    }
    found
}

/// The DOIs after `www.NAME.org/PATH/`, `PATH` made of `[a-z/]`.
fn find_after_www(chars: &[char]) -> Vec<String> {
    let mut found = Vec::new();
    let mut i = 0;
    'scan: while i < chars.len() {
        if chars[i..].starts_with(&['w', 'w', 'w', '.']) {
            let mut j = i + 4;
            while j < chars.len() && chars[j].is_ascii_lowercase() {
                j += 1;
            }
            if j > i + 4 && chars[j..].starts_with(&['.', 'o', 'r', 'g', '/']) {
                let start = j + 5;
                let mut k = start;
                while k < chars.len() && (chars[k].is_ascii_lowercase() || chars[k] == '/') {
                    k += 1;
                }
                // `[a-z/]+/`: at least one character, then the slash that ends the run
                if k >= start + 2
                    && chars[k - 1] == '/'
                    && let Some(end) = doi_at(chars, k)
                {
                    found.push(chars[k..end].iter().collect());
                    i = end;
                    continue 'scan;
                }
            }
        }
        i += 1;
    }
    found
}

/// A text with the invisible characters removed, the dashes and slashes normalized,
/// in lower case, and the DOIs that a line break splits joined (`_normalize_doi_text`).
fn normalize_doi_text(text: &str) -> Vec<char> {
    let clean: String = text
        .chars()
        .filter(|&c| c != '\u{200b}' && c != '\u{ad}')
        .map(|c| match c {
            '\u{2013}' | '\u{2014}' => '-',
            '\u{2}' => '/',
            c => c,
        })
        .collect();
    let mut chars: Vec<char> = clean.to_lowercase().chars().collect();
    // `\s*\n\s*`: a run of white space with a line break in it, from `i`
    let linebreak_run = |chars: &[char], i: usize| -> Option<usize> {
        let mut j = i;
        while j < chars.len() && is_space(chars[j]) {
            j += 1;
        }
        chars[i..j].contains(&'\n').then_some(j)
    };
    // (10\.)\s*\n\s*(\d{4,9}/) and (10\.1073/pnas\.)\s*\n\s*(\d), then the dash at the end of the line
    for rule in 0..3 {
        let mut out: Vec<char> = Vec::with_capacity(chars.len());
        let mut i = 0;
        while i < chars.len() {
            let mut joined: Option<(usize, usize)> = None; // (end of the kept prefix, where the text goes on)
            match rule {
                0 => {
                    if chars[i..].starts_with(&['1', '0', '.'])
                        && let Some(j) = linebreak_run(&chars, i + 3)
                    {
                        let mut k = j;
                        while k < chars.len() && is_decimal(chars[k]) {
                            k += 1;
                        }
                        if (4..=9).contains(&(k - j)) && chars.get(k) == Some(&'/') {
                            joined = Some((i + 3, j));
                        }
                    }
                }
                1 => {
                    let prefix: Vec<char> = "10.1073/pnas.".chars().collect();
                    if chars[i..].starts_with(&prefix)
                        && let Some(j) = linebreak_run(&chars, i + prefix.len())
                        && chars.get(j).is_some_and(|c| is_decimal(*c))
                    {
                        joined = Some((i + prefix.len(), j));
                    }
                }
                _ => {
                    if let Some(end) = doi_at(&chars, i) {
                        // `[class]+-` then the line break: the DOI ends with a hyphen, which is the last of
                        // the run of the class (the regex backtracks to it)
                        if end >= i + 2
                            && chars[end - 1] == '-'
                            && let Some(j) = linebreak_run(&chars, end)
                            && chars
                                .get(j)
                                .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                        {
                            joined = Some((end, j));
                        }
                    }
                }
            }
            match joined {
                Some((keep, next)) => {
                    out.extend(&chars[i..keep]);
                    i = next;
                }
                None => {
                    out.push(chars[i]);
                    i += 1;
                }
            }
        }
        chars = out;
    }
    chars
}

/// `_pick_doi`: the most likely article DOI among the matches (in order of appearance).
fn pick_doi(matches: &[String], text: &[char]) -> String {
    let mut doi = matches[0].clone();
    if matches.len() > 1 {
        let prefixes: [&[char]; 2] = [
            &['d', 'o', 'i', '.', 'o', 'r', 'g', '/'],
            &['d', 'x', '.', 'd', 'o', 'i', '.', 'o', 'r', 'g', '/'],
        ];
        let mut lists = vec![find_after_www(text)];
        lists.extend(prefixes.iter().map(|p| find_after(text, p)));
        for url_matches in lists {
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
    // `\.s\d+\.?$`: a supplemental DOI, and its base
    fn supplemental_base(m: &str) -> Option<&str> {
        let m = m.strip_suffix('.').unwrap_or(m);
        let trimmed = m.trim_end_matches(is_decimal);
        if trimmed.len() == m.len() {
            return None;
        }
        trimmed.strip_suffix(".s")
    }
    if matches.len() > 1
        && doi == matches[0]
        && let Some(base) = supplemental_base(&matches[0])
    {
        for m in &matches[1..] {
            if m.starts_with(base) && supplemental_base(m).is_none() {
                doi = m.clone();
                break;
            }
        }
    }
    doi
}

/// `_clean_doi` of `papers.extract`: what is not part of the DOI cut off; `None` if too short.
fn cut_doi(doi: &str) -> Option<String> {
    const SUFFIXES: [&str; 13] = [
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
        "supplement",
    ];
    let mut doi: Vec<char> = doi.chars().collect();
    let lower: Vec<char> = doi.iter().flat_map(|c| c.to_lowercase()).collect();
    let lower_same = lower.len() == doi.len();
    for suffix in SUFFIXES {
        let s: Vec<char> = suffix.chars().collect();
        // the position of the suffix in the lower-cased text (the same as in the text, but for rare characters)
        let pos = if lower_same {
            (0..lower.len()).find(|&i| lower[i..].starts_with(&s))
        } else {
            None
        };
        let Some(pos) = pos.filter(|&p| p > 0) else {
            continue;
        };
        if suffix.chars().all(char::is_alphabetic) {
            let prev = doi[pos - 1];
            if prev.is_alphanumeric() || matches!(prev, '.' | '-' | '_' | '/') {
                doi.truncate(pos);
                break;
            }
        } else {
            doi.truncate(pos);
            break;
        }
    }
    let mut doi: String = doi.into_iter().collect();
    doi = doi.trim_end_matches(['.', ':']).to_string();
    while doi.ends_with(')') && doi.matches(')').count() > doi.matches('(').count() {
        doi.pop();
    }
    (doi.chars().count() > 8).then_some(doi)
}

/// The DOIs found in a text, the most likely article DOI first (`parse_dois`, without the tail
/// of a document); `None` if there is none.
pub fn parse_dois(text: &str) -> Option<Vec<String>> {
    let chars = normalize_doi_text(text);
    let mut matches: Vec<String> = Vec::new();
    for m in find_dois(&chars) {
        if !matches.contains(&m) {
            matches.push(m);
        }
    }
    if matches.is_empty() {
        // arxiv:\s*(\d{4}\.\d{4,5})
        let tail: String = chars.iter().collect();
        let mut from = 0;
        while let Some(p) = tail[from..].find("arxiv:") {
            let rest = tail[from + p + 6..].trim_start_matches(is_space);
            let digits =
                |s: &str, max: usize| s.chars().take(max).take_while(|c| is_decimal(*c)).count();
            if digits(rest, 4) == 4 && rest[4..].starts_with('.') {
                let n = digits(&rest[5..], 5);
                if n >= 4 {
                    return Some(vec![format!("10.48550/arXiv.{}", &rest[..5 + n])]);
                }
            }
            from += p + 6;
        }
        return None;
    }
    let is_data = |m: &str| {
        DATA_REPOSITORY_PREFIXES.contains(&m.to_lowercase().split('/').next().unwrap_or(""))
    };
    let mut articles: Vec<String> = matches.iter().filter(|m| !is_data(m)).cloned().collect();
    if articles.is_empty() {
        articles = matches.clone();
    }
    let data: Vec<String> = matches
        .iter()
        .filter(|m| !articles.contains(m))
        .cloned()
        .collect();
    let doi = pick_doi(&articles, &chars);
    let mut candidates = vec![doi.clone()];
    candidates.extend(articles.iter().filter(|m| **m != doi).cloned());
    candidates.extend(data);
    let mut dois: Vec<String> = Vec::new();
    for candidate in candidates {
        if let Some(c) = cut_doi(&candidate)
            && !dois.contains(&c)
        {
            dois.push(c);
        }
    }
    (!dois.is_empty()).then_some(dois)
}

/// Whether a DOI is one that papers finds in itself (`isvaliddoi`).
pub fn is_valid_doi(doi: &str) -> bool {
    match parse_dois(&format!("doi:{doi}")) {
        Some(dois) => doi.to_lowercase() == dois[0].to_lowercase(),
        None => false,
    }
}

// the search
// ----------

impl Search {
    /// `_match(word, target, ...)` of one pattern.
    fn match_one(&self, word: &str, target: &str, fuzzy: bool, substring: bool) -> bool {
        let (word, target) = (word.to_lowercase(), target.to_lowercase());
        if fuzzy {
            let ratio = self.fuzzy_ratio.unwrap_or(FUZZY_SEARCH_RATIO);
            word.contains(&target) || token_set_ratio(&word, &target, ratio as f64) >= ratio as f64
        } else if substring {
            word.contains(&target)
        } else {
            fnmatch(&word, &target)
        }
    }

    /// `_longmatch`: all the patterns (any of them with `--any`) match the word, as substrings,
    /// or fuzzily with `--fuzzy`. Both sides are compared as the same-paper rule compares text
    /// (`normalize_text`: LaTeX decoded, ASCII, no braces, lower case, single spaces): `Muller`
    /// finds `M{\"u}ller` and `Müller`.
    fn long_match(&self, word: &str, targets: &[String], strict: bool) -> bool {
        let word = normalize_text(word);
        let one = |t: &String| self.match_one(&word, &normalize_text(t), self.fuzzy, !strict);
        if self.any {
            targets.iter().any(one)
        } else {
            targets.iter().all(one)
        }
    }

    /// The entries to review (`_requiresreview`): an invalid key or DOI, no author, title or year.
    fn requires_review(entry: &Entry) -> bool {
        !is_valid_key(&entry.key)
            || entry.get("doi").is_some_and(|d| !is_valid_doi(d))
            || entry.get("author").is_none()
            || entry.get("title").is_none()
            || entry.get("year").is_none()
    }

    /// The entries that share the key, the DOI or the title (`groups_by_field`): the groups of two
    /// or more, in the order of their first entry, as the positions of their entries.
    fn groups_of(field: By, entries: &[&Entry]) -> Vec<usize> {
        let value = |e: &Entry| -> String {
            match field {
                By::Key => e.key.to_lowercase(),
                By::Doi => normalize_doi(val(e, "doi")).unwrap_or_default(),
                By::Title => val(e, "title")
                    .to_lowercase()
                    .chars()
                    .filter(|c| is_word(*c) && *c != '_')
                    .collect(),
            }
        };
        let mut order: Vec<String> = Vec::new();
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            let v = value(e);
            if !v.is_empty() {
                if !groups.contains_key(&v) {
                    order.push(v.clone());
                }
                groups.entry(v).or_default().push(i);
            }
        }
        order
            .iter()
            .filter(|v| groups[*v].len() > 1)
            .flat_map(|v| groups[v].iter().copied())
            .collect()
    }

    /// The entries that the search selects among `entries`, in their order. `distinct` is the
    /// pairs of `.papers/distinct.json`, `relative_to` what the file links are relative to.
    /// `Err` is the message of a usage error (`--by` and `--fuzzy` together).
    pub fn select<'a>(
        &self,
        entries: &[&'a Entry],
        distinct: Option<&Distinct>,
        relative_to: &str,
    ) -> Result<Vec<&'a Entry>, String> {
        let all: Vec<&'a Entry> = entries.to_vec();
        let mut current: Vec<usize> = (0..all.len()).collect();
        let keep =
            |current: &mut Vec<usize>, f: &dyn Fn(&Entry) -> bool| current.retain(|&i| f(all[i]));
        if self.review_required {
            keep(&mut current, &Search::requires_review);
        }
        if self.has_file {
            keep(&mut current, &|e| !val(e, "file").is_empty());
        }
        if self.no_file {
            keep(&mut current, &|e| val(e, "file").is_empty());
        }
        if self.broken_file {
            keep(&mut current, &|e| {
                !val(e, "file").is_empty() && files(e, Some(relative_to)).iter().any(|f| !exists(f))
            });
        }
        let strict = self.strict;
        if !self.doi.is_empty() {
            keep(&mut current, &|e| {
                e.get("doi")
                    .is_some_and(|d| self.long_match(d, &self.doi, strict))
            });
        }
        if !self.key.is_empty() {
            keep(&mut current, &|e| {
                self.long_match(&e.key, &self.key, strict)
            });
        }
        if !self.year.is_empty() {
            keep(&mut current, &|e| {
                e.get("year")
                    .is_some_and(|d| self.long_match(d, &self.year, strict))
            });
        }
        if !self.first_author.is_empty() {
            keep(&mut current, &|e| {
                e.get("author").is_some_and(|a| {
                    self.long_match(&family_names(a)[0], &self.first_author, strict)
                })
            });
        }
        if !self.author.is_empty() {
            keep(&mut current, &|e| {
                e.get("author").is_some_and(|a| {
                    self.long_match(&family_names(a).join(" "), &self.author, strict)
                })
            });
        }
        if !self.title.is_empty() {
            keep(&mut current, &|e| {
                e.get("title")
                    .is_some_and(|d| self.long_match(d, &self.title, strict))
            });
        }
        if !self.abstract_text.is_empty() {
            keep(&mut current, &|e| {
                e.get("abstract")
                    .is_some_and(|d| self.long_match(d, &self.abstract_text, strict))
            });
        }
        if !self.keywords.is_empty() {
            if self.fuzzy {
                keep(&mut current, &|e| {
                    e.get("keywords")
                        .is_some_and(|d| self.long_match(d, &self.keywords, strict))
                });
            } else {
                // whole tags, not part of the keywords field; a tag also finds the tags under it
                let has_tag = |e: &Entry, target: &String| {
                    parse_keywords(val(e, "keywords"))
                        .iter()
                        .any(|tag| tag_matches(tag, target))
                };
                keep(&mut current, &|e| {
                    if self.any {
                        self.keywords.iter().any(|t| has_tag(e, t))
                    } else {
                        self.keywords.iter().all(|t| has_tag(e, t))
                    }
                });
            }
        }
        if !self.words.is_empty() {
            // the words are searched as substrings (or fuzzily), never as patterns
            keep(&mut current, &|e| {
                self.long_match(&fullsearch_string(e), &self.words, false)
            });
        }
        let mut selected: Vec<&'a Entry> = current.iter().map(|&i| all[i]).collect();
        if let Some(by) = self.by {
            if self.fuzzy {
                return Err("--by and --fuzzy are mutually exclusive".to_string());
            }
            let positions = Search::groups_of(by, &selected);
            selected = positions.into_iter().map(|i| selected[i]).collect();
        } else if self.duplicates {
            let mut listed: Vec<usize> = Vec::new();
            for group in duplicate_groups(&selected, distinct) {
                for i in group.entries {
                    if !listed.contains(&i) {
                        listed.push(i);
                    }
                }
            }
            if self.fuzzy {
                for (a, b) in fuzzy_pairs(&selected, self.fuzzy_ratio.unwrap_or(FUZZY_RATIO)) {
                    for i in [a, b] {
                        if !listed.contains(&i) {
                            listed.push(i);
                        }
                    }
                }
            }
            selected = listed.into_iter().map(|i| selected[i]).collect();
        }
        if self.invert {
            let listed: Vec<*const Entry> = selected.iter().map(|e| *e as *const Entry).collect();
            selected = all
                .iter()
                .copied()
                .filter(|e| !listed.contains(&(*e as *const Entry)))
                .collect();
        }
        Ok(selected)
    }
}

/// The text of the full search of an entry (`_fullsearch_string`): the type, the key and the
/// values of the fields, by the name of the field, joined by spaces.
fn fullsearch_string(entry: &Entry) -> String {
    let mut items: Vec<(&str, &str)> = vec![
        ("ENTRYTYPE", entry.entry_type.as_str()),
        ("ID", entry.key.as_str()),
    ];
    items.extend(
        entry
            .fields
            .iter()
            .map(|f| (f.key.as_str(), f.value.as_str())),
    );
    items.sort_by(|a, b| a.0.cmp(b.0));
    items.iter().map(|(_, v)| *v).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores() {
        assert_eq!(
            token_set_ratio("fuzzy was a bear", "fuzzy fuzzy was a bear", 0.0),
            100.0
        );
        let s = token_set_ratio(
            "fuzzy was a bear but not a dog",
            "fuzzy was a bear but not a cat",
            0.0,
        );
        assert!((s - 92.3076923076923).abs() < 1e-6, "{s}");
        assert_eq!(token_set_ratio("", "a", 0.0), 0.0);
        assert_eq!(token_set_ratio("abc def", "xyz uvw", 90.0), 0.0);
    }

    #[test]
    fn dois() {
        assert!(is_valid_doi("10.5194/esd-4-11-2013"));
        assert!(!is_valid_doi("abc"));
        assert!(!is_valid_doi(""));
        assert!(!is_valid_doi("10.12/x"));
        assert!(!is_valid_doi("10.1234/a.pdf"));
    }
}
