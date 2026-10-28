//! The records of crossref (and of doi.org, as crossref-like records) as bibtex:
//! `papers.fetch.crossref_to_bibtex` and what it uses; the score of a result of a search.
//!
//! A record is a JSON value; what papers does on it is Python code on a dict, with its
//! exceptions where the record is not what is expected (an empty title list, an
//! `institution` that is a list): they are the `Other` errors, which papers does not catch.

use serde_json::Value;

use super::fuzz::token_set_ratio;
use super::{ErrorKind, FetchError};
use crate::bib::{Bib, Block, Entry, Field};
use crate::config::json::{py_str, py_truthy};

fn other(message: impl Into<String>) -> FetchError {
    FetchError {
        kind: ErrorKind::Other,
        message: message.into(),
    }
}

/// `message.get(key)`: the record is a dict.
fn get<'a>(message: &'a Value, key: &str) -> Option<&'a Value> {
    message.get(key)
}

pub(super) fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// `value[0]`, of a value that is a list (or a string).
fn index0(value: &Value) -> Result<Value, FetchError> {
    match value {
        Value::Array(a) => a
            .first()
            .cloned()
            .ok_or_else(|| other("list index out of range")),
        Value::String(s) => s
            .chars()
            .next()
            .map(|c| Value::String(c.to_string()))
            .ok_or_else(|| other("string index out of range")),
        Value::Object(_) => Err(other("0")),
        v => Err(other(format!(
            "'{}' object is not subscriptable",
            type_name(v)
        ))),
    }
}

/// `message.get(key, [''])[0]`
fn first(message: &Value, key: &str) -> Result<Value, FetchError> {
    match get(message, key) {
        None => Ok(Value::String(String::new())),
        Some(v) => index0(v),
    }
}

/// `message.get(key, '')`
fn text(message: &Value, key: &str) -> Value {
    get(message, key)
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()))
}

/// `map_crossref_to_bibtex_type`
pub fn map_crossref_to_bibtex_type(crossref_type: &str) -> &'static str {
    match crossref_type {
        "journal-article" => "article",
        "book" => "book",
        "book-chapter" => "incollection",
        "proceedings-article" | "conference-paper" => "inproceedings",
        "report" => "techreport",
        "thesis" => "phdthesis",
        _ => "misc",
    }
}

/// `format_authors`: the authors as BibTeX writes them, `Family, Given and ...`.
pub fn format_authors(authors: &Value) -> Result<String, FetchError> {
    let list: Vec<&Value> = match authors {
        Value::Array(a) => a.iter().collect(),
        // the characters of a string, the keys of a dict
        Value::String(s) if s.is_empty() => return Ok(String::new()),
        Value::Object(o) if o.is_empty() => return Ok(String::new()),
        Value::String(_) | Value::Object(_) => {
            return Err(other("'str' object has no attribute 'get'"));
        }
        v => return Err(other(format!("'{}' object is not iterable", type_name(v)))),
    };
    let mut out: Vec<String> = Vec::new();
    for author in list {
        let Value::Object(_) = author else {
            return Err(other(format!(
                "'{}' object has no attribute 'get'",
                type_name(author)
            )));
        };
        let given = author
            .get("given")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new()));
        let family = author
            .get("family")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new()));
        out.push(if py_truthy(&family) {
            format!("{}, {}", py_str(&family), py_str(&given))
        } else {
            py_str(&given)
        });
    }
    Ok(out.join(" and "))
}

/// `message.get('institution', {}).get('name', '')`
fn institution_name(message: &Value) -> Result<Value, FetchError> {
    match get(message, "institution") {
        None => Ok(Value::String(String::new())),
        Some(Value::Object(o)) => Ok(o
            .get("name")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new()))),
        Some(v) => Err(other(format!(
            "'{}' object has no attribute 'get'",
            type_name(v)
        ))),
    }
}

/// `crossref_to_bibtex`: the bibtex of a crossref record, as papers writes a library (the
/// fields in alphabetical order, the empty ones dropped).
pub fn crossref_to_bibtex(message: &Value) -> Result<String, FetchError> {
    Ok(Bib {
        blocks: vec![Block::Entry(crossref_to_entry(message)?)],
    }
    .format())
}

/// The entry of `crossref_to_bibtex`: the key is the DOI (`unknown` without one).
pub fn crossref_to_entry(message: &Value) -> Result<Entry, FetchError> {
    let entry_type = message
        .get("type")
        .cloned()
        .unwrap_or_else(|| Value::String("misc".into()));
    let type_str = match &entry_type {
        Value::String(s) => s.as_str(),
        Value::Array(_) | Value::Object(_) => {
            return Err(other(format!(
                "unhashable type: '{}'",
                type_name(&entry_type)
            )));
        }
        _ => "", // None, a number: no type of crossref
    };
    let mapped = map_crossref_to_bibtex_type(type_str);
    let doi = text(message, "DOI");
    let mut fields: Vec<(&str, Value)> = vec![
        ("title", first(message, "title")?),
        (
            "author",
            Value::String(format_authors(
                &get(message, "author")
                    .cloned()
                    .unwrap_or(Value::Array(vec![])),
            )?),
        ),
        ("doi", doi.clone()),
        ("url", text(message, "URL")),
    ];
    // the year: the publication date, in the first of the fields that has one
    let mut date_parts: Option<Value> = None;
    for field in ["published-print", "published-online", "issued"] {
        if let Some(date) = get(message, field)
            && let Some(parts) = date.get("date-parts")
        {
            date_parts = Some(index0(parts)?);
            break;
        }
    }
    let year = match &date_parts {
        Some(parts) if py_truthy(parts) => py_str(&index0(parts)?),
        _ => "0000".to_string(),
    };
    fields.push(("year", Value::String(year)));
    match type_str {
        "journal-article" => {
            fields.push(("journal", first(message, "container-title")?));
            fields.push(("volume", text(message, "volume")));
            fields.push(("number", text(message, "issue")));
            fields.push(("pages", text(message, "page")));
        }
        "book" => {
            fields.push(("publisher", text(message, "publisher")));
            fields.push(("address", text(message, "publisher-location")));
            fields.push((
                "editor",
                Value::String(format_authors(
                    &get(message, "editor")
                        .cloned()
                        .unwrap_or(Value::Array(vec![])),
                )?),
            ));
            fields.push((
                "isbn",
                match get(message, "ISBN") {
                    None => Value::String(String::new()),
                    Some(v) => index0(v)?,
                },
            ));
        }
        "proceedings-article" => {
            fields.push(("booktitle", first(message, "container-title")?));
            fields.push(("publisher", text(message, "publisher")));
            fields.push((
                "editor",
                Value::String(format_authors(
                    &get(message, "editor")
                        .cloned()
                        .unwrap_or(Value::Array(vec![])),
                )?),
            ));
        }
        "report" => {
            fields.push(("institution", institution_name(message)?));
        }
        "thesis" => {
            fields.push(("school", institution_name(message)?));
            fields.push(("type", text(message, "type")));
        }
        _ => {}
    }
    // the empty fields are dropped
    let key = if py_truthy(&doi) {
        py_str(&doi)
    } else {
        "unknown".to_string()
    };
    let mut entry = Entry::new(mapped, key);
    for (name, value) in fields {
        if py_truthy(&value) {
            entry.fields.push(Field::new(name, py_str(&value)));
        }
    }
    Ok(entry)
}

/// `_crossref_score`: how alike a result of the search is to the text (high is alike), and
/// whether it is a float (a record with neither author, title nor abstract has the integer 0).
pub fn crossref_score(txt: &str, record: &Value) -> Result<(f64, bool), FetchError> {
    let mut score = 0.0;
    let mut float = false;
    if let Some(authors) = record.get("author") {
        let list = match authors {
            Value::Array(a) => a.as_slice(),
            _ => &[],
        };
        let mut families: Vec<String> = Vec::new();
        for p in list {
            if let Some(family) = p.get("family") {
                families.push(py_str(family));
            }
        }
        score += token_set_ratio(&families.join(" "), txt);
        float = true;
    }
    if let Some(title) = record.get("title") {
        score += token_set_ratio(&py_str(&index0(title)?), txt);
        float = true;
    }
    if let Some(abstract_) = record.get("abstract") {
        score += token_set_ratio(&py_str(abstract_), txt);
        float = true;
    }
    Ok((score, float))
}
