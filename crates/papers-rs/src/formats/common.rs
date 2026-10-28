//! What the formats share (`papers.formats`): dates, pages, DOIs, the keys
//! of imported entries (spec `formats.md` 2.4 to 2.7).

use crate::bib::Entry;
use crate::bib::pystr::{is_space, is_word, strip};
use crate::entry::val;
use crate::names::{decimal_value, is_decimal, is_digit};

/// A number written with decimal digits (`int(text)`).
fn int(text: &str) -> i128 {
    text.chars().fold(0i128, |n, c| {
        n.saturating_mul(10)
            .saturating_add(decimal_value(c) as i128)
    })
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// A bibtex month (`jan`, `January`, `5`, `05`) as a number (`parse_month`).
pub fn parse_month(value: &str) -> Option<i128> {
    let v = strip(value).trim_matches(['{', '}']).to_lowercase();
    if !v.is_empty() && v.chars().all(is_digit) {
        let n = int(&v);
        if (1..=12).contains(&n) {
            return Some(n);
        }
    }
    let head: String = v.chars().take(3).collect();
    MONTHS
        .iter()
        .position(|m| *m == head)
        .map(|i| i as i128 + 1)
}

/// The year, month, day numbers of a date text (`_date_parts`): `2013`,
/// `2013-05`, `2013/05/02/`, `2013-05-02T00:00`; the numbers that are 0 are left out.
pub fn date_parts(text: &str) -> Vec<i128> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() && is_space(chars[i]) {
        i += 1;
    }
    let digits = |from: usize, max: usize| -> usize {
        let mut n = 0;
        while n < max && from + n < chars.len() && is_decimal(chars[from + n]) {
            n += 1;
        }
        n
    };
    if digits(i, 4) < 4 {
        return Vec::new();
    }
    let mut groups: Vec<String> = vec![chars[i..i + 4].iter().collect()];
    i += 4;
    for _ in 0..2 {
        if i < chars.len() && (chars[i] == '-' || chars[i] == '/') {
            let n = digits(i + 1, 2);
            if n >= 1 {
                groups.push(chars[i + 1..i + 1 + n].iter().collect());
                i += 1 + n;
                continue;
            }
        }
        break;
    }
    groups.iter().map(|g| int(g)).filter(|&n| n > 0).collect()
}

/// The year, month, day of an entry: its `year` and `month` fields, else its
/// biblatex `date` field (`_entry_date_parts`).
pub fn entry_date_parts(entry: &Entry) -> Vec<i128> {
    let year = strip(val(entry, "year")).trim_matches(['{', '}']);
    if !year.is_empty() && year.chars().all(is_digit) {
        let mut parts = vec![int(year)];
        if let Some(month) = parse_month(val(entry, "month")) {
            parts.push(month);
        }
        return parts;
    }
    date_parts(val(entry, "date"))
}

/// The pieces of a pages text, split at `--`, `-`, `–` or `—` with white space
/// around them (`re.split(r'\s*(?:--|-|–|—)\s*', page)`), the empty ones left out.
fn page_parts(page: &str) -> Vec<String> {
    let chars: Vec<char> = page.chars().collect();
    let mut parts = Vec::new();
    let mut piece = String::new();
    let mut i = 0;
    while i < chars.len() {
        let mut j = i;
        while j < chars.len() && is_space(chars[j]) {
            j += 1;
        }
        let dash = match chars.get(j) {
            Some('-') if chars.get(j + 1) == Some(&'-') => 2,
            Some('-' | '–' | '—') => 1,
            _ => 0,
        };
        if dash == 0 {
            piece.push(chars[i]);
            i += 1;
            continue;
        }
        let mut k = j + dash;
        while k < chars.len() && is_space(chars[k]) {
            k += 1;
        }
        parts.push(std::mem::take(&mut piece));
        i = k;
    }
    parts.push(piece);
    parts.into_iter().filter(|p| !p.is_empty()).collect()
}

/// `123-130` (CSL, RIS) as `123--130` (bibtex): two parts only (`_pages_to_bibtex`).
pub fn pages_to_bibtex(page: &str) -> String {
    let page = strip(page);
    let parts = page_parts(page);
    if parts.len() == 2 {
        parts.join("--")
    } else {
        page.to_string()
    }
}

/// The first and the last page of a bibtex `pages` (`_split_pages`).
pub fn split_pages(pages: &str) -> (String, String) {
    let mut parts = page_parts(strip(pages)).into_iter();
    let first = parts.next().unwrap_or_default();
    (first, parts.next().unwrap_or_default())
}

/// The length in bytes of the prefix of a DOI that `_DOI_PREFIX` removes
/// (`http(s)://(dx.)doi.org/` or `doi:` and the white space after it, whatever
/// the case), if the text starts with one.
fn doi_prefix_len(text: &str) -> Option<usize> {
    let starts = |prefix: &str| {
        text.get(..prefix.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
    };
    for prefix in [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "http://dx.doi.org/",
    ] {
        if starts(prefix) {
            return Some(prefix.len());
        }
    }
    if starts("doi:") {
        let rest = &text[4..];
        return Some(4 + rest.len() - rest.trim_start_matches(is_space).len());
    }
    None
}

/// A DOI without `http(s)://(dx.)doi.org/` or `doi:` before it (`_clean_doi`).
pub fn clean_doi(doi: &str) -> String {
    let doi = strip(doi);
    match doi_prefix_len(doi) {
        Some(n) => doi[n..].to_string(),
        None => doi.to_string(),
    }
}

/// Whether a text starts with what `clean_doi` removes (`_DOI_PREFIX.match`).
pub fn has_doi_prefix(text: &str) -> bool {
    doi_prefix_len(text).is_some()
}

/// The key of an imported entry: kept if it looks like a key (not a URL, as
/// the item ids of Zotero, not a number, as the record numbers of EndNote),
/// else empty (`usable_key`).
pub fn usable_key(key: &str) -> String {
    let key = strip(key);
    let mut chars = key.chars();
    let ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| is_word(c) || matches!(c, ':' | '.' | '+' | '-'));
    let item = key
        .strip_prefix("ITEM-")
        .is_some_and(|d| !d.is_empty() && d.chars().all(is_decimal));
    if ok && !item {
        key.to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(date_parts("2013"), [2013]);
        assert_eq!(date_parts(" 2013/05/02/"), [2013, 5, 2]);
        assert_eq!(date_parts("2013-05-02T00:00"), [2013, 5, 2]);
        assert_eq!(date_parts("2013-00-05"), [2013, 5]);
        assert_eq!(date_parts("13-05"), Vec::<i128>::new());
        assert_eq!(parse_month("{Jan}"), Some(1));
        assert_eq!(parse_month("13"), None);
        assert_eq!(parse_month("sept"), Some(9));
    }

    #[test]
    fn pages() {
        assert_eq!(pages_to_bibtex("11 - 29"), "11--29");
        assert_eq!(pages_to_bibtex("1-2-3"), "1-2-3");
        assert_eq!(pages_to_bibtex("11–29"), "11--29");
        assert_eq!(split_pages("e1—e9"), ("e1".to_string(), "e9".to_string()));
        assert_eq!(split_pages("5"), ("5".to_string(), String::new()));
    }

    #[test]
    fn dois_and_keys() {
        assert_eq!(clean_doi(" HTTPS://DX.DOI.ORG/10.1/x "), "10.1/x");
        assert_eq!(clean_doi("doi:  10.1/x"), "10.1/x");
        assert_eq!(usable_key("perrette2013"), "perrette2013");
        assert_eq!(usable_key("ITEM-12"), "");
        assert_eq!(usable_key("2013"), "");
        assert_eq!(usable_key("a b"), "");
    }
}
