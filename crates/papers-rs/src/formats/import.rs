//! Reading the formats (`papers.formats.loads`, spec `formats.md` 3.3, 4.2, 4.5, 5.2): the text of a
//! CSL-JSON, RIS or BibJSON file as bibtex entries, with the quirks of papers' code (and of the
//! `rispy` reader for RIS).

use super::bibjson::{NAMESPACE, SKIPPED};
use super::common::{clean_doi, date_parts, has_doi_prefix, pages_to_bibtex, usable_key};
use super::names::{Name, bibtex_name_of_ris, format_bibtex_names};
use crate::bib::encoding::format_file;
use crate::bib::pystr::{is_space, strip};
use crate::bib::{Entry, Field};
use crate::config::json::{loads as json_loads, py_float_repr, py_repr_str};
use crate::formats::Format;
use crate::history::ojson::J;
use crate::log;

// Python's `str` of a JSON value
// ------------------------------

fn py_repr(value: &J) -> String {
    match value {
        J::Null => "None".into(),
        J::Bool(b) => if *b { "True" } else { "False" }.into(),
        J::Int(n) => n.to_string(),
        J::Float(f) => py_float_repr(*f),
        J::Str(s) => py_repr_str(s),
        J::Arr(items) => format!(
            "[{}]",
            items.iter().map(py_repr).collect::<Vec<_>>().join(", ")
        ),
        J::Obj(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|(k, v)| format!("{}: {}", py_repr_str(k), py_repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// `str(value)`.
fn py_str(value: &J) -> String {
    match value {
        J::Str(s) => s.clone(),
        v => py_repr(v),
    }
}

/// Python's truth of a JSON value.
fn truthy(value: Option<&J>) -> bool {
    match value {
        None | Some(J::Null) => false,
        Some(J::Bool(b)) => *b,
        Some(J::Int(n)) => *n != 0,
        Some(J::Float(f)) => *f != 0.0,
        Some(J::Str(s)) => !s.is_empty(),
        Some(J::Arr(a)) => !a.is_empty(),
        Some(J::Obj(o)) => !o.is_empty(),
    }
}

/// The text of a JSON text, or the message of the error: `not a CSL-JSON file: Expecting value: ...`.
fn parse(text: &str, label: &str, numbers_as_text: bool) -> Result<J, String> {
    let parsed = if numbers_as_text {
        J::parse_numbers_as_text(text)
    } else {
        J::parse(text)
    };
    match parsed {
        Some(value) => Ok(value),
        None => {
            let why = match json_loads(text) {
                Err(e) => e.message,
                Ok(_) => "Expecting value: line 1 column 1 (char 0)".to_string(),
            };
            Err(format!("not a {label} file: {why}"))
        }
    }
}

// the entry made
// --------------

/// The fields of an entry being made: in the order they were first given, a Python dict.
#[derive(Default)]
struct Fields(Vec<(String, String)>);

impl Fields {
    fn set(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = value,
            None => self.0.push((key.to_string(), value)),
        }
    }

    fn get(&self, key: &str) -> &str {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map_or("", |(_, v)| v.as_str())
    }

    fn has(&self, key: &str) -> bool {
        self.0.iter().any(|(k, _)| k == key)
    }
}

/// The fields where `&`, `%`, `#` and `_` are not escaped (`_RAW_FIELDS`, the keywords and the date of
/// the access).
const NOT_ESCAPED: [&str; 8] = [
    "url", "doi", "file", "eprint", "isbn", "issn", "keywords", "urldate",
];

/// A text with the `&`, `%`, `#` and `_` that no backslash precedes escaped for LaTeX (`_LATEX_SPECIAL`).
fn latex_escaped(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut previous = None;
    for c in value.chars() {
        if matches!(c, '&' | '%' | '#' | '_') && previous != Some('\\') {
            out.push('\\');
        }
        out.push(c);
        previous = Some(c);
    }
    out
}

/// `_entry`: the fields that are not empty (with `escape`, `&` `%` `#` `_` escaped for LaTeX: RIS
/// and CSL-JSON hold plain text), and the file links.
fn make_entry(
    entry_type: &str,
    key: &str,
    fields: Fields,
    files: &[String],
    escape: bool,
) -> Entry {
    let mut entry = Entry::new(entry_type, key);
    entry.fields = fields
        .0
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| {
            let v = if escape && !NOT_ESCAPED.contains(&k.as_str()) {
                latex_escaped(&v)
            } else {
                v
            };
            Field::new(k, v)
        })
        .collect();
    if !files.is_empty() {
        entry.fields.retain(|f| f.key != "file");
        entry.fields.push(Field::new("file", format_file(files)));
    }
    entry
}

/// The paths of the file links of an imported item (`_file_paths`): a list, or a text separated
/// by `;`; `file://` URIs are converted, the links that are not files are left out with a warning.
fn file_paths(value: Option<&J>) -> Vec<String> {
    let items: Vec<String> = match value {
        None => return Vec::new(),
        Some(v) if !truthy(Some(v)) => return Vec::new(),
        Some(J::Str(s)) => s.split(';').map(str::to_string).collect(),
        Some(J::Arr(items)) => items.iter().map(py_str).collect(),
        Some(J::Obj(items)) => items.iter().map(|(k, _)| k.clone()).collect(),
        Some(other) => vec![py_str(other)],
    };
    let mut paths = Vec::new();
    for item in items {
        let mut v = strip(&item).to_string();
        if v.is_empty() {
            continue;
        }
        if let Some(rest) = v.strip_prefix("file://") {
            // urlparse: the host up to the next slash, then the path up to `?` or `#`
            let path = rest.find('/').map_or("", |i| &rest[i..]);
            let path = path.split(['?', '#']).next().unwrap_or("");
            v = unquote(path);
        } else if has_scheme(&v) {
            log::warning(&format!("file link left out (not a local file): {v}"));
            continue;
        } else if v.matches(':').count() >= 2 && !is_drive_path(&v) {
            // description:path:type
            let parts: Vec<&str> = v.split(':').collect();
            v = parts[1..parts.len() - 1].join(":");
        }
        paths.push(v);
    }
    paths
}

/// `^[a-z][a-z0-9+.\-]*://` (any case).
fn has_scheme(v: &str) -> bool {
    let Some((scheme, _)) = v.split_once("://") else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// `^[A-Za-z]:[\\/]`
fn is_drive_path(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// `urllib.parse.unquote`.
fn unquote(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(byte) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The year and the month of date numbers (`_fields_from_parts`).
fn set_date(fields: &mut Fields, parts: &[i128]) {
    if let Some(year) = parts.first() {
        fields.set("year", year.to_string());
    }
    if let Some(month) = parts.get(1) {
        fields.set("month", month.to_string());
    }
}

// CSL-JSON
// --------

/// The bibtex type of a CSL type (`CSL_TO_BIBTEX_TYPE`).
pub fn csl_to_bibtex_type(csl: &str) -> &'static str {
    match csl {
        "article-journal" | "article-magazine" | "article-newspaper" => "article",
        "article" => "misc",
        "book" => "book",
        "pamphlet" => "booklet",
        "chapter" | "entry-encyclopedia" | "entry-dictionary" => "incollection",
        "paper-conference" => "inproceedings",
        "thesis" => "phdthesis",
        "report" => "techreport",
        "webpage" | "post" | "post-weblog" => "online",
        "dataset" => "dataset",
        "software" => "software",
        "patent" => "patent",
        "manuscript" => "unpublished",
        "document" => "misc",
        _ => "misc",
    }
}

/// The text of a CSL value (`_csl_text`): the first of a list, empty for null.
fn csl_text(v: &J) -> String {
    let v = match v {
        J::Arr(items) => match items.first() {
            Some(first) => first,
            None => return String::new(),
        },
        v => v,
    };
    match v {
        J::Null => String::new(),
        v => py_str(v),
    }
}

/// The numbers of date parts up to the first missing or zero one (`_leading_parts`: `[2013, 0, 5]`
/// is the year 2013 only); `None` for a part that is not a number (the date is then read as text).
fn leading_parts(parts: &[J]) -> Option<Vec<i128>> {
    let mut numbers = Vec::new();
    for x in parts {
        if matches!(x, J::Null) || strip(&py_str(x)).is_empty() {
            break;
        }
        let n = match x {
            J::Int(n) => *n as i128,
            J::Str(s) => s.trim().parse::<i128>().ok()?,
            J::Float(f) if f.is_finite() => *f as i128,
            J::Bool(b) => *b as i128,
            _ => return None,
        };
        if n <= 0 {
            break;
        }
        numbers.push(n);
    }
    Some(numbers)
}

/// The date numbers of a CSL date (`_csl_date`): `date-parts`, else `raw`, else `literal`, or a date text.
fn csl_date(value: Option<&J>) -> Vec<i128> {
    match value {
        Some(J::Obj(_)) => {
            let object = value.unwrap();
            if let Some(J::Arr(parts)) = object.get("date-parts")
                && let Some(J::Arr(first)) = parts.first()
                && !first.is_empty()
                && let Some(numbers) = leading_parts(first)
            {
                return numbers;
            }
            let text = ["raw", "literal"]
                .iter()
                .find_map(|k| object.get(k).filter(|v| truthy(Some(v))))
                .map(py_str)
                .unwrap_or_default();
            date_parts(&text)
        }
        Some(other) => date_parts(&py_str(other)),
        None => date_parts(""),
    }
}

/// The names of a CSL `author` (`format_bibtex_names` of the list): a name is a dict of parts, or a text.
fn csl_names(value: &J) -> String {
    let part = |o: &J, k: &str| -> Option<String> { o.get(k).map(py_str) };
    let names: Vec<Name> = match value {
        J::Arr(items) => items
            .iter()
            .filter(|n| truthy(Some(n)))
            .map(|n| csl_name(n, &part))
            .collect(),
        J::Str(s) => s
            .chars()
            .map(|c| Name {
                literal: Some(c.to_string()),
                ..Name::default()
            })
            .collect(),
        J::Obj(items) => items
            .iter()
            .map(|(k, _)| Name {
                literal: Some(k.clone()),
                ..Name::default()
            })
            .collect(),
        _ => Vec::new(),
    };
    format_bibtex_names(&names)
}

fn csl_name(n: &J, part: &dyn Fn(&J, &str) -> Option<String>) -> Name {
    match n {
        J::Obj(_) => Name {
            literal: part(n, "literal"),
            family: part(n, "family"),
            given: part(n, "given"),
            non_dropping_particle: part(n, "non-dropping-particle"),
            dropping_particle: part(n, "dropping-particle"),
            suffix: part(n, "suffix"),
        },
        other => Name {
            literal: Some(py_str(other)),
            ..Name::default()
        },
    }
}

/// The field of the container title (`container-title`, `T2`): the `booktitle` of the parts of a book
/// or of proceedings, the `journal` of any other type.
fn container_field(entry_type: &str) -> &'static str {
    if matches!(entry_type, "incollection" | "inproceedings") {
        "booktitle"
    } else {
        "journal"
    }
}

/// A bibtex entry of a CSL-JSON item (`csl_to_entry`); its key is empty if the item has none usable.
pub fn csl_to_entry(item: &J) -> Entry {
    let get = |k: &str| item.get(k);
    let csl_type = get("type").map_or("document".to_string(), py_str);
    let mut entry_type = csl_to_bibtex_type(&csl_type);
    if csl_type == "thesis"
        && get("genre")
            .map(py_str)
            .unwrap_or_default()
            .to_lowercase()
            .contains("master")
    {
        entry_type = "mastersthesis";
    }
    let mut fields = Fields::default();
    for name in ["author", "editor", "translator"] {
        if truthy(get(name)) {
            fields.set(name, csl_names(get(name).unwrap()));
        }
    }
    for (bib, csl) in [
        ("title", "title"),
        ("volume", "volume"),
        ("doi", "DOI"),
        ("url", "URL"),
        ("isbn", "ISBN"),
        ("issn", "ISSN"),
        ("publisher", "publisher"),
        ("address", "publisher-place"),
        ("abstract", "abstract"),
        ("note", "note"),
        ("edition", "edition"),
        ("language", "language"),
        ("series", "collection-title"),
        ("pmid", "PMID"),
        ("pmcid", "PMCID"),
    ] {
        if let Some(v) =
            get(csl).filter(|v| !matches!(v, J::Null) && !matches!(v, J::Str(s) if s.is_empty()))
        {
            fields.set(bib, csl_text(v));
        }
    }
    if fields.has("doi") {
        let doi = clean_doi(fields.get("doi"));
        fields.set("doi", doi);
    }
    let container = get("container-title").map(csl_text).unwrap_or_default();
    if !container.is_empty() {
        fields.set(container_field(entry_type), container);
    }
    let number = if truthy(get("issue")) {
        get("issue")
    } else {
        get("number")
    };
    if let Some(n) =
        number.filter(|v| !matches!(v, J::Null) && !matches!(v, J::Str(s) if s.is_empty()))
    {
        fields.set("number", csl_text(n));
    }
    if truthy(get("page")) {
        fields.set("pages", pages_to_bibtex(&py_str(get("page").unwrap())));
    }
    if fields.has("publisher") && !fields.get("publisher").is_empty() {
        let target = match entry_type {
            "phdthesis" | "mastersthesis" => Some("school"),
            "techreport" => Some("institution"),
            _ => None,
        };
        if let Some(target) = target {
            let publisher = fields.get("publisher").to_string();
            fields.0.retain(|(k, _)| k != "publisher");
            fields.set(target, publisher);
        }
    }
    if truthy(get("genre")) && entry_type == "techreport" {
        fields.set("type", csl_text(get("genre").unwrap()));
    }
    if truthy(get("keyword")) {
        let keywords = match get("keyword").unwrap() {
            J::Arr(items) => items.iter().map(py_str).collect::<Vec<_>>().join(", "),
            other => py_str(other),
        };
        let words: Vec<&str> = keywords
            .split([',', ';'])
            .map(strip)
            .filter(|w| !w.is_empty())
            .collect();
        fields.set("keywords", words.join(", "));
    }
    set_date(&mut fields, &csl_date(get("issued")));
    let accessed = csl_date(get("accessed"));
    if !accessed.is_empty() {
        fields.set(
            "urldate",
            accessed
                .iter()
                .map(|x| format!("{x:02}"))
                .collect::<Vec<_>>()
                .join("-"),
        );
    }
    let mut files = file_paths(get("file"));
    files.extend(file_paths(get("custom").and_then(|c| c.get("file"))));
    let key = {
        let k = usable_key(
            &get("citation-key")
                .filter(|v| truthy(Some(v)))
                .map(py_str)
                .unwrap_or_default(),
        );
        if k.is_empty() {
            usable_key(
                &get("id")
                    .filter(|v| truthy(Some(v)))
                    .map(py_str)
                    .unwrap_or_default(),
            )
        } else {
            k
        }
    };
    make_entry(entry_type, &key, fields, &files, true)
}

fn loads_csl(text: &str) -> Result<Vec<Entry>, String> {
    let data = parse(text, "CSL-JSON", false)?;
    let items: Vec<J> = match data {
        J::Obj(_) => match data.get("items") {
            Some(J::Arr(items)) => items.clone(),
            Some(_) => return Err("not a CSL-JSON file: a list of items is expected".into()),
            None => vec![data.clone()],
        },
        J::Arr(items) => items,
        _ => return Err("not a CSL-JSON file: a list of items is expected".into()),
    };
    if !items.iter().all(|i| matches!(i, J::Obj(_))) {
        return Err("not a CSL-JSON file: a list of items is expected".into());
    }
    Ok(items.iter().map(csl_to_entry).collect())
}

// BibJSON
// -------

fn bibjson_text(value: &J) -> String {
    match value {
        J::Obj(_) => bibjson_text(value.get("name").unwrap_or(&J::Str(String::new()))),
        J::Arr(items) => items
            .iter()
            .filter(|v| !matches!(v, J::Null))
            .map(bibjson_text)
            .collect::<Vec<_>>()
            .join(", "),
        J::Bool(b) => if *b { "true" } else { "false" }.into(),
        v => py_str(v),
    }
}

/// A bibtex entry of a BibJSON record (`bibjson_to_entry`).
pub fn bibjson_to_entry(record: &J) -> Entry {
    let J::Obj(items) = record else {
        return Entry::new("misc", "");
    };
    let mut fields = Fields::default();
    let prefix = format!("{NAMESPACE}:");
    for (name, value) in items {
        let name = match name.strip_prefix(&prefix) {
            // a bibtex field named as a key of BibJSON
            Some(field) => field,
            None if name.starts_with('_') || SKIPPED.contains(&name.as_str()) => continue,
            None => name,
        };
        if matches!(value, J::Null) {
            continue;
        }
        match value {
            J::Arr(list) if name == "author" || name == "editor" => fields.set(
                name,
                list.iter()
                    .filter(|v| !matches!(v, J::Null))
                    .map(bibjson_text)
                    .collect::<Vec<_>>()
                    .join(" and "),
            ),
            _ => fields.set(name, bibjson_text(value)),
        }
    }
    if !fields.has("doi")
        && let Some(J::Arr(identifiers)) = record.get("identifier")
    {
        // the first identifier of type doi decides, whether it has an id or not
        let first = identifiers.iter().find(|i| {
            matches!(i, J::Obj(_))
                && i.get("type").map(py_str).unwrap_or_default().to_lowercase() == "doi"
        });
        if let Some(doi) = first
            .and_then(|i| i.get("id"))
            .filter(|id| truthy(Some(id)))
        {
            fields.set("doi", clean_doi(&py_str(doi)));
        }
    }
    let key = {
        let k = usable_key(
            &record
                .get("id")
                .filter(|v| truthy(Some(v)))
                .map(py_str)
                .unwrap_or_default(),
        );
        if k.is_empty() {
            usable_key(
                &record
                    .get("cid")
                    .filter(|v| truthy(Some(v)))
                    .map(py_str)
                    .unwrap_or_default(),
            )
        } else {
            k
        }
    };
    let entry_type = match record.get("type") {
        Some(v) if truthy(Some(v)) => py_str(v).to_lowercase(),
        _ => "misc".to_string(),
    };
    make_entry(
        &entry_type,
        &key,
        fields,
        &file_paths(record.get("file")),
        false,
    )
}

fn loads_bibjson(text: &str) -> Result<Vec<Entry>, String> {
    // the numbers as written
    let data = parse(text, "BibJSON", true)?;
    let records: Vec<J> = match data {
        J::Obj(_) => match data.get("records") {
            Some(J::Arr(items)) => items.clone(),
            Some(_) => {
                return Err(
                    "not a BibJSON file: a collection {\"records\": [...]} or a list of records is expected".into(),
                );
            }
            None => vec![data.clone()],
        },
        J::Arr(items) => items,
        _ => {
            return Err(
                "not a BibJSON file: a collection {\"records\": [...]} or a list of records is expected".into(),
            );
        }
    };
    if !records.iter().all(|i| matches!(i, J::Obj(_))) {
        return Err("not a BibJSON file: a collection {\"records\": [...]} or a list of records is expected".into());
    }
    Ok(records.iter().map(bibjson_to_entry).collect())
}

// RIS
// ---

/// A tag of a RIS record, as `rispy` holds it: a text, or a list of texts.
#[derive(Debug, Clone)]
enum Value {
    One(String),
    Many(Vec<String>),
}

/// A RIS record: the names of the tags (`rispy`'s `TAG_KEY_MAPPING` with the editors added), in order.
#[derive(Debug, Clone, Default)]
struct Record(Vec<(&'static str, Value)>);

impl Record {
    fn find(&mut self, name: &str) -> Option<&mut Value> {
        self.0.iter_mut().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    fn get(&self, name: &str) -> Option<&Value> {
        self.0.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    fn list(&self, name: &str) -> Vec<String> {
        match self.get(name) {
            None => Vec::new(),
            Some(Value::One(s)) => vec![s.clone()],
            Some(Value::Many(v)) => v.clone(),
        }
    }

    /// The first value that is not blank, stripped (`_first`).
    fn first(&self, names: &[&str]) -> String {
        for name in names {
            for v in self.list(name) {
                if !strip(&v).is_empty() {
                    return strip(&v).to_string();
                }
            }
        }
        String::new()
    }

    fn truthy(&self, name: &str) -> bool {
        match self.get(name) {
            None => false,
            Some(Value::One(s)) => !s.is_empty(),
            Some(Value::Many(v)) => !v.is_empty(),
        }
    }
}

/// The name of a RIS tag and whether it is a list: `rispy`'s mapping, with `ED`, `L1` and `SN` as lists.
fn ris_tag(tag: &str) -> Option<(&'static str, bool)> {
    Some(match tag {
        "TY" => ("type_of_reference", false),
        "A1" => ("first_authors", true),
        "A2" => ("secondary_authors", true),
        "A3" => ("tertiary_authors", true),
        "A4" => ("subsidiary_authors", true),
        "AB" => ("abstract", false),
        "AD" => ("author_address", false),
        "AN" => ("accession_number", false),
        "AU" => ("authors", true),
        "C1" => ("custom1", false),
        "C2" => ("custom2", false),
        "C3" => ("custom3", false),
        "C4" => ("custom4", false),
        "C5" => ("custom5", false),
        "C6" => ("custom6", false),
        "C7" => ("custom7", false),
        "C8" => ("custom8", false),
        "CA" => ("caption", false),
        "CN" => ("call_number", false),
        "CY" => ("place_published", false),
        "DA" => ("date", false),
        "DB" => ("name_of_database", false),
        "DO" => ("doi", false),
        "DP" => ("database_provider", false),
        "ET" => ("edition", false),
        "EP" => ("end_page", false),
        "ID" => ("id", false),
        "IS" => ("number", false),
        "J2" => ("alternate_title1", false),
        "JA" => ("alternate_title2", false),
        "JF" => ("alternate_title3", false),
        "JO" => ("journal_name", false),
        "KW" => ("keywords", true),
        "L1" => ("file_attachments1", true),
        "L2" => ("file_attachments2", false),
        "L4" => ("figure", false),
        "LA" => ("language", false),
        "LB" => ("label", false),
        "M1" => ("note", false),
        "M3" => ("type_of_work", false),
        "N1" => ("notes", true),
        "N2" => ("notes_abstract", false),
        "NV" => ("number_of_volumes", false),
        "OP" => ("original_publication", false),
        "PB" => ("publisher", false),
        "PY" => ("year", false),
        "RI" => ("reviewed_item", false),
        "RN" => ("research_notes", false),
        "RP" => ("reprint_edition", false),
        "SE" => ("section", false),
        "SN" => ("issn", true),
        "SP" => ("start_page", false),
        "ST" => ("short_title", false),
        "T1" => ("primary_title", false),
        "T2" => ("secondary_title", false),
        "T3" => ("tertiary_title", false),
        "TA" => ("translated_author", false),
        "TI" => ("title", false),
        "TT" => ("translated_title", false),
        "UR" => ("urls", true),
        "VL" => ("volume", false),
        "Y1" => ("publication_year", false),
        "Y2" => ("access_date", false),
        "ED" => ("editors", true),
        _ => return None,
    })
}

/// One line of a RIS file: the tag and the content, or none for a continuation line (`parse_line`).
fn ris_line(line: &str) -> (Option<String>, String) {
    let chars: Vec<char> = line.chars().collect();
    let is_tag = chars.len() >= 5
        && chars[2..5] == [' ', ' ', '-']
        && chars[0].is_alphabetic()
        && chars[..2].iter().any(|c| c.is_uppercase())
        && !chars[..2].iter().any(|c| c.is_lowercase());
    if is_tag {
        let content: String = chars.get(6..).unwrap_or(&[]).iter().collect();
        (
            Some(chars[..2].iter().collect()),
            strip(&content).to_string(),
        )
    } else {
        (None, strip(line).to_string())
    }
}

/// What `rispy.loads(text, skip_unknown_tags=True)` makes of the records; an error is the `KeyError`
/// of a continuation line that follows the tag of another record (`'title'`).
fn read_ris(text: &str) -> Result<Vec<Record>, String> {
    let mut lines = text.split('\n');
    let mut result: Vec<Record> = Vec::new();
    let mut last_tag: Option<String> = None;
    let start = |lines: &mut std::str::Split<'_, char>| -> Option<Record> {
        for line in lines {
            if line.starts_with("TY") {
                return Some(Record(vec![(
                    "type_of_reference",
                    Value::One(ris_line(line).1),
                )]));
            }
        }
        None
    };
    let Some(mut record) = start(&mut lines) else {
        return Ok(result);
    };
    while let Some(line) = lines.next() {
        let (tag, content) = ris_line(line);
        let Some(tag) = tag else {
            add_tag(&mut record, last_tag.as_deref(), content, true)?;
            continue;
        };
        if tag == "ER" {
            result.push(record);
            match start(&mut lines) {
                Some(next) => {
                    record = next;
                    continue;
                }
                None => return Ok(result),
            }
        }
        add_tag(&mut record, Some(&tag), content, false)?;
        last_tag = Some(tag);
    }
    Ok(result)
}

fn add_tag(
    record: &mut Record,
    tag: Option<&str>,
    content: String,
    multiline: bool,
) -> Result<(), String> {
    let Some((name, is_list)) = tag.and_then(ris_tag) else {
        return Ok(());
    };
    let tag = tag.unwrap_or_default();
    let content: Value = if tag == "UR" {
        Value::Many(content.split(';').map(|i| strip(i).to_string()).collect())
    } else {
        Value::One(content)
    };
    let items = |v: Value| -> Vec<String> {
        match v {
            Value::One(s) => vec![s],
            Value::Many(v) => v,
        }
    };
    if is_list {
        match record.find(name) {
            Some(Value::Many(existing)) => existing.extend(items(content)),
            Some(Value::One(existing)) => {
                let mut all = vec![existing.clone()];
                all.extend(items(content));
                *record.find(name).unwrap() = Value::Many(all);
            }
            None => record.0.push((name, Value::Many(items(content)))),
        }
    } else if !multiline {
        if record.find(name).is_none() {
            record.0.push((name, content));
        }
    } else {
        match record.find(name) {
            None => return Err(format!("'{name}'")),
            Some(Value::One(existing)) => {
                if let Value::One(more) = content {
                    *existing = format!("{existing} {more}");
                }
            }
            Some(Value::Many(existing)) => existing.extend(items(content)),
        }
    }
    Ok(())
}

/// The bibtex type of a RIS type (`RIS_TO_BIBTEX_TYPE`).
pub fn ris_to_bibtex_type(ris: &str) -> &'static str {
    match ris {
        "JOUR" | "JFULL" | "EJOUR" | "MGZN" | "NEWS" | "INPR" => "article",
        "BOOK" | "EBOOK" | "EDBOOK" => "book",
        "PAMP" => "booklet",
        "CHAP" | "ECHAP" | "ENCYC" | "DICT" => "incollection",
        "CPAPER" | "CONF" => "inproceedings",
        "THES" => "phdthesis",
        "RPRT" | "GOVDOC" => "techreport",
        "GEN" => "misc",
        "ELEC" | "BLOG" => "online",
        "DATA" | "DBASE" => "dataset",
        "COMP" => "software",
        "UNPB" | "MANSCPT" => "unpublished",
        "PAT" => "patent",
        _ => "misc",
    }
}

fn ris_to_entry(record: &Record) -> Entry {
    let ris_type = {
        let t = record
            .list("type_of_reference")
            .into_iter()
            .next()
            .unwrap_or_else(|| "GEN".to_string());
        strip(&t).to_uppercase()
    };
    let mut entry_type = ris_to_bibtex_type(&ris_type);
    if ris_type == "THES"
        && record
            .first(&["type_of_work"])
            .to_lowercase()
            .contains("master")
    {
        entry_type = "mastersthesis";
    }
    let mut fields = Fields::default();
    let mut authors = record.list("authors");
    authors.extend(record.list("first_authors"));
    if !authors.is_empty() {
        let names: Vec<String> = authors
            .iter()
            .filter(|a| !strip(a).is_empty())
            .map(|a| bibtex_name_of_ris(a))
            .collect();
        fields.set("author", names.join(" and "));
    }
    let mut editors = record.list("editors");
    if editors.is_empty()
        && [
            "CHAP", "ECHAP", "CPAPER", "CONF", "BOOK", "EDBOOK", "ENCYC", "DICT",
        ]
        .contains(&ris_type.as_str())
    {
        editors = record.list("secondary_authors");
    }
    if !editors.is_empty() {
        let names: Vec<String> = editors
            .iter()
            .filter(|a| !strip(a).is_empty())
            .map(|a| bibtex_name_of_ris(a))
            .collect();
        fields.set("editor", names.join(" and "));
    }
    let translators = record.list("subsidiary_authors");
    if !translators.is_empty() {
        let names: Vec<String> = translators
            .iter()
            .filter(|a| !strip(a).is_empty())
            .map(|a| bibtex_name_of_ris(a))
            .collect();
        fields.set("translator", names.join(" and "));
    }
    fields.set("title", record.first(&["title", "primary_title"]));
    let container = record.first(&[
        "secondary_title",
        "alternate_title3",
        "journal_name",
        "alternate_title2",
        "alternate_title1",
    ]);
    if !container.is_empty() {
        fields.set(container_field(entry_type), container);
    }
    if record.truthy("tertiary_title") {
        fields.set("series", record.first(&["tertiary_title"]));
    }
    let mut parts = date_parts(&record.first(&["year", "publication_year", "date"]));
    let date = date_parts(&record.first(&["date", "publication_year"]));
    if !parts.is_empty() && date.len() > 1 && date[0] == parts[0] {
        parts = date;
    }
    set_date(&mut fields, &parts);
    fields.set("volume", record.first(&["volume"]));
    fields.set("number", record.first(&["number"]));
    let (start, end) = (record.first(&["start_page"]), record.first(&["end_page"]));
    if !start.is_empty() && !end.is_empty() && start != end {
        fields.set("pages", format!("{start}--{end}"));
    } else if !start.is_empty() {
        fields.set("pages", pages_to_bibtex(&start));
    }
    fields.set("doi", clean_doi(&record.first(&["doi"])));
    let all_urls = record.list("urls");
    let urls: Vec<&String> = all_urls
        .iter()
        .filter(|u| !strip(u).is_empty() && !has_doi_prefix(strip(u)))
        .collect();
    if let Some(url) = urls.first() {
        fields.set("url", strip(url).to_string());
    }
    // no DO line: the DOI of a doi.org link, also next to another URL
    let doi_urls: Vec<&String> = all_urls
        .iter()
        .filter(|u| has_doi_prefix(strip(u)))
        .collect();
    if fields.get("doi").is_empty()
        && let Some(url) = doi_urls.first()
    {
        fields.set("doi", clean_doi(url));
    }
    fields.set("urldate", record.first(&["access_date"]));
    for sn in record.list("issn") {
        let first_word = sn.split(is_space).find(|w| !w.is_empty()).unwrap_or("");
        let digits = first_word
            .chars()
            .filter(|c| c.is_ascii_digit() || matches!(c, 'X' | 'x'))
            .count();
        let name = if digits == 8 { "issn" } else { "isbn" };
        if !fields.has(name) {
            fields.set(name, strip(&sn).to_string());
        }
    }
    let publisher = record.first(&["publisher"]);
    if !publisher.is_empty() {
        fields.set(
            match entry_type {
                "phdthesis" | "mastersthesis" => "school",
                "techreport" => "institution",
                _ => "publisher",
            },
            publisher,
        );
    }
    fields.set("address", record.first(&["place_published"]));
    fields.set("abstract", record.first(&["abstract", "notes_abstract"]));
    let mut keywords: Vec<String> = Vec::new();
    for k in record.list("keywords") {
        keywords.extend(
            k.split([';', ','])
                .map(strip)
                .filter(|w| !w.is_empty())
                .map(str::to_string),
        );
    }
    if !keywords.is_empty() {
        fields.set("keywords", keywords.join(", "));
    }
    let notes: Vec<String> = record
        .list("notes")
        .iter()
        .map(|n| strip(n).to_string())
        .filter(|n| !n.is_empty())
        .collect();
    if !notes.is_empty() {
        fields.set("note", notes.join("; "));
    }
    fields.set("edition", record.first(&["edition"]));
    fields.set("language", record.first(&["language"]));
    if matches!(entry_type, "techreport" | "phdthesis") {
        fields.set("type", record.first(&["type_of_work"]));
    }
    let attachments = record.list("file_attachments1");
    let files = file_paths(Some(&J::Arr(attachments.into_iter().map(J::Str).collect())));
    make_entry(
        entry_type,
        &usable_key(&record.first(&["id"])),
        fields,
        &files,
        true,
    )
}

/// A line of a RIS tag as `_RIS_TAG` takes it: two characters (a capital, then a capital or a digit),
/// two spaces and a dash.
fn is_tag_line(line: &str) -> bool {
    let b = line.as_bytes();
    b.len() >= 5
        && b[0].is_ascii_uppercase()
        && (b[1].is_ascii_uppercase() || b[1].is_ascii_digit())
        && b[2..5] == *b"  -"
}

/// The RIS text without the blank lines inside a record and the lines without tag between `TY` and
/// the first tag (`_ris_without_stray_lines`: `rispy` reads them as the continuation of the last tag
/// of the previous record, an error for a single tag such as `TI`, an empty keyword for `KW`); a last
/// record without `ER` gets one (rispy drops it otherwise).
fn without_stray_lines(text: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let (mut inside, mut tagged) = (false, false);
    for line in text.split('\n') {
        if is_tag_line(line) {
            if line.starts_with("TY  -") {
                (inside, tagged) = (true, false);
            } else {
                tagged = true;
                if line.starts_with("ER  -") {
                    inside = false;
                }
            }
        } else if inside && (strip(line).is_empty() || !tagged) {
            continue;
        }
        lines.push(line);
    }
    if inside {
        lines.push("ER  - ");
    }
    lines.join("\n")
}

fn loads_ris(text: &str) -> Result<Vec<Entry>, String> {
    let text = without_stray_lines(&text.trim_start_matches('\u{feff}').replace("\r\n", "\n"));
    let records = read_ris(&text)?;
    if records.is_empty() && !strip(&text).is_empty() {
        return Err("not a RIS file: no record found (a record starts with TY  - )".into());
    }
    Ok(records.iter().map(ris_to_entry).collect())
}

/// The entries of a text in a format other than bibtex (`loads`). The error is the message of papers'
/// `ValueError` (or the `KeyError` of the RIS reader, `'title'`).
pub fn loads(text: &str, format: Format) -> Result<Vec<Entry>, String> {
    match format {
        Format::CslJson => loads_csl(text),
        Format::BibJson => loads_bibjson(text),
        Format::Ris => loads_ris(text),
        Format::Bibtex => Err("unknown format: bibtex".into()),
    }
}
