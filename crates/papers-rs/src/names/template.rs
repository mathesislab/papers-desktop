//! The name and key templates: Python's `str.format` syntax, the placeholders
//! of an entry (`papers.filename.make_template_fields`) and the options that
//! go with a template (`papers.filename.Format`).
//!
//! The contract is `{name}`, `{{` and `}}` (library.md); like papers, a format
//! specification (`{title:.20}`), a conversion (`{title!r}`) and an index
//! (`{author[0]}`) of a string also work. Attribute access (`{year.real}`),
//! which papers answers with a Python error, is an error here too.

use std::fmt;

use serde_json::Value;

use super::{capitalize, head, head_chars, path_safe, safe_file_name, slugify, title_words};
use crate::bib::Entry;
use crate::bib::encoding::family_names;
use crate::bib::pystr::strip;
use crate::config::json::py_repr_str;
use crate::config::library::Config;

pub const UNKNOWN_AUTHOR: &str = "unknown";
pub const UNKNOWN_YEAR: &str = "0000";
pub const UNKNOWN_TITLE: &str = "";
pub const UNKNOWN_JOURNAL: &str = "";

/// The placeholders, in the order papers lists them in its error.
pub const PLACEHOLDERS: [&str; 14] = [
    "author",
    "Author",
    "AUTHOR",
    "authorX",
    "AuthorX",
    "journal",
    "year",
    "title",
    "Title",
    "key",
    "ID",
    "doi",
    "doi_",
    "doi_or_id",
];

/// A name or key template that cannot be formatted (`TemplateError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateError(pub String);

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TemplateError {}

impl From<TemplateError> for crate::error::Error {
    fn from(e: TemplateError) -> Self {
        crate::error::Error::exit(e.0)
    }
}

/// The values of the placeholders for an entry (see [`make_template_fields`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fields(pub Vec<(&'static str, String)>);

impl Fields {
    /// The text of a placeholder (empty for the journal of an entry without one).
    pub fn get(&self, name: &str) -> Option<String> {
        self.value(name).cloned()
    }

    fn value(&self, name: &str) -> Option<&String> {
        self.0.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }
}

/// Whether a template is for names (of files) or keys: the word in its errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatKind {
    Name,
    Key,
}

impl FormatKind {
    fn word(self) -> &'static str {
        match self {
            FormatKind::Name => "name",
            FormatKind::Key => "key",
        }
    }
}

/// A name or key template and its options (`papers.filename.Format`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    pub kind: FormatKind,
    pub template: String,
    pub author_num: i64,
    pub author_sep: String,
    pub title_word_num: i64,
    pub title_word_size: i64,
    pub title_length: i64,
    pub title_sep: String,
    pub unknown_strict: bool,
    pub unknown_template: Option<String>,
}

impl Format {
    /// The built-in name template of papers: `{authorX}_{year}_{title}`.
    pub fn name() -> Format {
        Format {
            kind: FormatKind::Name,
            template: "{authorX}_{year}_{title}".into(),
            author_num: 2,
            author_sep: "_".into(),
            title_word_num: 100,
            title_word_size: 1,
            title_length: 100,
            title_sep: "-".into(),
            unknown_strict: false,
            unknown_template: Some("unknown_{doi_or_id}".into()),
        }
    }

    /// The built-in key template of papers: `{author}{year}`.
    pub fn key() -> Format {
        Format {
            kind: FormatKind::Key,
            template: "{author}{year}".into(),
            unknown_template: Some("{doi_}".into()),
            ..Format::name()
        }
    }

    /// The template of a configuration: its `name.*` or `key.*` settings.
    pub fn from_config(config: &Config, kind: FormatKind) -> Format {
        let word = kind.word();
        let mut format = match kind {
            FormatKind::Name => Format::name(),
            FormatKind::Key => Format::key(),
        };
        let get = |field: &str| config.get(&format!("{word}.{field}"));
        let text = |v: Value, default: &str| match v {
            Value::String(s) => s,
            Value::Null => default.to_string(),
            other => crate::config::json::py_str(&other),
        };
        let int = |v: Value, default: i64| v.as_i64().unwrap_or(default);
        format.template = text(get("template"), &format.template);
        format.author_num = int(get("author-num"), format.author_num);
        format.author_sep = text(get("author-sep"), &format.author_sep);
        format.title_word_num = int(get("title-word-num"), format.title_word_num);
        format.title_word_size = int(get("title-word-size"), format.title_word_size);
        format.title_length = int(get("title-length"), format.title_length);
        format.title_sep = text(get("title-sep"), &format.title_sep);
        format.unknown_strict = crate::config::json::py_truthy(&get("unknown-strict"));
        format.unknown_template = match get("unknown-template") {
            Value::String(s) => Some(s),
            _ => None,
        };
        format
    }

    /// An entry is unknown when its author is missing or `unknown`, its year
    /// is missing or `0000`, or its title is missing or empty (any of the
    /// three, or all three with `unknown_strict`).
    pub fn is_unknown(&self, entry: &Entry) -> bool {
        let conditions = [
            entry.get("author").unwrap_or(UNKNOWN_AUTHOR) == UNKNOWN_AUTHOR,
            entry.get("year").unwrap_or(UNKNOWN_YEAR) == UNKNOWN_YEAR,
            entry.get("title").unwrap_or(UNKNOWN_TITLE) == UNKNOWN_TITLE,
        ];
        if self.unknown_strict {
            conditions.iter().all(|c| *c)
        } else {
            conditions.iter().any(|c| *c)
        }
    }

    /// The name or key of an entry: the unknown template if the entry is unknown
    /// and there is one, else the template. A name is made valid for every
    /// system ([`safe_file_name`]), without the empty first folder of a template
    /// that does not start with `/` (`{journal}/...` without journal).
    pub fn render(&self, entry: &Entry) -> Result<String, TemplateError> {
        let fields = make_template_fields(entry, self);
        let (template, name) = match self.unknown_template.as_deref().filter(|t| !t.is_empty()) {
            Some(unknown) if self.is_unknown(entry) => (
                unknown,
                format_template(unknown, &fields, &format!("{} unknown", self.kind.word()))?,
            ),
            _ => (
                self.template.as_str(),
                format_template(&self.template, &fields, self.kind.word())?,
            ),
        };
        if self.kind != FormatKind::Name {
            return Ok(name);
        }
        let name = if template.starts_with('/') {
            name.as_str()
        } else {
            name.trim_start_matches('/')
        };
        Ok(safe_file_name(name))
    }
}

/// `{a and b}` of the authors, cut at three: `A`, `A and B`, `A et al`.
fn cite_author(names: &[String]) -> String {
    match names.len() {
        0 => String::new(), // papers fails on an index; there is always one name
        1 => names[0].clone(),
        2 => names.join(" and "),
        _ => format!("{} et al", names[0]),
    }
}

/// The placeholders of an entry for a format (`make_template_fields`):
///
/// - `author`: the slugs of the family names of the first `author_num` authors, joined with `author_sep`;
///   `Author`: capitalized; `AUTHOR`: upper case;
///   `authorX`: the first, the first and the second, the first et al; `AuthorX`: capitalized;
/// - `title`: the words of the slug of the title ([`title_words`]), joined with `title_sep`; `Title`: capitalized;
/// - `journal`, `year`, `key`, `ID`, `doi`: the field as it is, without path separators
///   ([`path_safe`]), empty for a journal that is missing, `0000` for a year;
/// - `doi_`: the slug of the DOI; `doi_or_id`: that of the DOI, or of the key
///   when there is no DOI (none, empty, or whitespace only).
pub fn make_template_fields(entry: &Entry, format: &Format) -> Fields {
    let author = entry.get("author").unwrap_or(UNKNOWN_AUTHOR).to_lowercase();
    let names: Vec<String> = family_names(&author).iter().map(|n| slugify(n)).collect();
    let first = head(&names, format.author_num);
    let author_joined = first.join(&format.author_sep);
    let author_cap = first
        .iter()
        .map(|n| capitalize(n))
        .collect::<Vec<_>>()
        .join(&format.author_sep);
    let caps: Vec<String> = names.iter().map(|n| capitalize(n)).collect();
    let author_x_cap = cite_author(&caps).replace(' ', &format.author_sep);
    let author_x = author_x_cap.to_lowercase();

    let journal = path_safe(entry.get("journal").unwrap_or(UNKNOWN_JOURNAL));
    let year = path_safe(entry.get("year").unwrap_or(UNKNOWN_YEAR));

    let (title, title_cap) = match entry.get("title") {
        Some(t) if format.title_word_num != 0 && !t.is_empty() => {
            let slug = slugify(t);
            let words: Vec<&str> = slug.split('-').collect();
            let words = title_words(
                &words,
                format.title_length,
                format.title_word_size,
                format.title_word_num,
                &format.title_sep,
            );
            let caps: Vec<String> = words.iter().map(|w| capitalize(w)).collect();
            (words.join(&format.title_sep), caps.join(&format.title_sep))
        }
        _ => (UNKNOWN_TITLE.to_string(), UNKNOWN_TITLE.to_string()),
    };

    let key = path_safe(&entry.key);
    let doi = entry.get("doi");
    Fields(vec![
        ("author", author_joined.clone()),
        ("Author", author_cap),
        ("AUTHOR", author_joined.to_uppercase()),
        ("authorX", author_x),
        ("AuthorX", author_x_cap),
        ("journal", journal),
        ("year", year),
        ("title", title),
        ("Title", title_cap),
        ("key", key.clone()),
        ("ID", key),
        ("doi", path_safe(doi.unwrap_or(""))),
        ("doi_", slugify(doi.unwrap_or(""))),
        // an empty doi, or one of whitespace, is no doi
        (
            "doi_or_id",
            slugify(doi.filter(|d| !strip(d).is_empty()).unwrap_or(&entry.key)),
        ),
    ])
}

// Python's str.format
// -------------------

/// What `str.format` raises.
#[derive(Debug, PartialEq, Eq)]
enum FormatError {
    /// `KeyError`: an unknown name
    Key(String),
    /// `IndexError`
    Index,
    /// `ValueError`: the message
    Value(String),
    /// the other errors of Python (`AttributeError`, `TypeError`), which papers does not catch
    Other(String),
}

/// `template.format(**fields)` as papers calls it: an unknown placeholder is a
/// [`TemplateError`] that lists the available ones, so is a template that
/// cannot be formatted. `kind` is the word in the message (`name`, `key`,
/// `name unknown`).
pub fn format_template(
    template: &str,
    fields: &Fields,
    kind: &str,
) -> Result<String, TemplateError> {
    match format(template, fields, 2) {
        Ok(text) => Ok(text),
        Err(FormatError::Key(name)) => Err(unknown_placeholder(&name, template, kind)),
        Err(FormatError::Index) => Err(unknown_placeholder("", template, kind)),
        Err(FormatError::Value(message) | FormatError::Other(message)) => {
            Err(TemplateError(format!(
                "invalid {kind} template {}: {message}",
                py_repr_str(template)
            )))
        }
    }
}

fn unknown_placeholder(name: &str, template: &str, kind: &str) -> TemplateError {
    let available = PLACEHOLDERS
        .iter()
        .map(|f| format!("{{{f}}}"))
        .collect::<Vec<_>>()
        .join(", ");
    TemplateError(format!(
        "unknown placeholder {{{name}}} in the {kind} template {}; available: {available}",
        py_repr_str(template)
    ))
}

fn format(template: &str, fields: &Fields, depth: usize) -> Result<String, FormatError> {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '}' => {
                if chars.get(i + 1) == Some(&'}') {
                    out.push('}');
                    i += 2;
                } else {
                    return Err(FormatError::Value(
                        "Single '}' encountered in format string".into(),
                    ));
                }
            }
            '{' => {
                if i + 1 >= chars.len() {
                    return Err(FormatError::Value(
                        "Single '{' encountered in format string".into(),
                    ));
                }
                if chars[i + 1] == '{' {
                    out.push('{');
                    i += 2;
                    continue;
                }
                let (field, spec, conversion, next) = parse_field(&chars, i + 1)?;
                i = next;
                out.push_str(&render_field(&field, &spec, conversion, fields, depth)?);
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// `parse_field` of CPython's `Objects/stringlib/unicode_format.h`: the field
/// name, the format specification, the conversion, and where the field ends.
#[allow(clippy::type_complexity)]
fn parse_field(
    chars: &[char],
    mut i: usize,
) -> Result<(String, String, Option<char>, usize), FormatError> {
    let start = i;
    let mut c = None;
    while i < chars.len() {
        let ch = chars[i];
        i += 1;
        match ch {
            '{' => return Err(FormatError::Value("unexpected '{' in field name".into())),
            '[' => {
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
                continue;
            }
            '}' | ':' | '!' => {
                c = Some(ch);
                break;
            }
            _ => continue,
        }
    }
    let field: String = chars[start..if c.is_some() { i - 1 } else { i }]
        .iter()
        .collect();
    match c {
        Some(ch @ ('!' | ':')) => {
            let mut conversion = None;
            if ch == '!' {
                if i >= chars.len() {
                    return Err(FormatError::Value(
                        "end of string while looking for conversion specifier".into(),
                    ));
                }
                conversion = Some(chars[i]);
                i += 1;
                if i < chars.len() {
                    let next = chars[i];
                    i += 1;
                    if next == '}' {
                        return Ok((field, String::new(), conversion, i));
                    }
                    if next != ':' {
                        return Err(FormatError::Value(
                            "expected ':' after conversion specifier".into(),
                        ));
                    }
                }
            }
            let spec_start = i;
            let mut count = 1;
            while i < chars.len() {
                let ch = chars[i];
                i += 1;
                match ch {
                    '{' => count += 1,
                    '}' => {
                        count -= 1;
                        if count == 0 {
                            return Ok((
                                field,
                                chars[spec_start..i - 1].iter().collect(),
                                conversion,
                                i,
                            ));
                        }
                    }
                    _ => {}
                }
            }
            Err(FormatError::Value("unmatched '{' in format spec".into()))
        }
        Some(_) => Ok((field, String::new(), None, i)),
        None => Err(FormatError::Value(
            "expected '}' before end of string".into(),
        )),
    }
}

fn render_field(
    field: &str,
    spec: &str,
    conversion: Option<char>,
    fields: &Fields,
    depth: usize,
) -> Result<String, FormatError> {
    // the first part of the name, then `.attribute` and `[index]` parts
    let split = field.find(['.', '[']).unwrap_or(field.len());
    let (first, rest) = field.split_at(split);
    if first.is_empty() || first.chars().all(|c| c.is_ascii_digit()) {
        // automatic or numbered field: there are no positional arguments
        return Err(FormatError::Index);
    }
    let value = fields
        .value(first)
        .ok_or_else(|| FormatError::Key(first.to_string()))?;
    let mut text = value.clone();
    let mut rest = rest;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('.') {
            let end = after.find(['.', '[']).unwrap_or(after.len());
            return Err(FormatError::Other(format!(
                "'str' object has no attribute '{}'",
                &after[..end]
            )));
        }
        let after = &rest[1..];
        let Some(close) = after.find(']') else {
            return Err(FormatError::Value("Missing ']' in format string".into()));
        };
        let index = &after[..close];
        if index.is_empty() {
            return Err(FormatError::Value(
                "Empty attribute in format string".into(),
            ));
        }
        if !index.chars().all(|c| c.is_ascii_digit()) {
            return Err(FormatError::Other(
                "string indices must be integers, not 'str'".into(),
            ));
        }
        let n: usize = index.parse().unwrap_or(usize::MAX);
        text = text.chars().nth(n).ok_or(FormatError::Index)?.to_string();
        rest = &after[close + 1..];
        if !rest.is_empty() && !rest.starts_with(['.', '[']) {
            return Err(FormatError::Value(
                "Only '.' or '[' may follow ']' in format field specifier".into(),
            ));
        }
    }
    let spec = if spec.contains('{') {
        if depth == 0 {
            return Err(FormatError::Value("Max string recursion exceeded".into()));
        }
        format(spec, fields, depth - 1)?
    } else {
        spec.to_string()
    };
    let text = match conversion {
        None | Some('s') => text,
        Some('r') => py_repr_str(&text),
        Some('a') => py_ascii(&text),
        Some(c) => {
            return Err(FormatError::Value(format!(
                "Unknown conversion specifier {}",
                if c.is_ascii_graphic() {
                    c.to_string()
                } else {
                    format!("\\x{:x}", c as u32)
                }
            )));
        }
    };
    format_str(&text, &spec)
}

/// `ascii(str)`: the repr with the non-ASCII characters escaped.
fn py_ascii(s: &str) -> String {
    let mut out = String::new();
    for c in py_repr_str(s).chars() {
        match c as u32 {
            0..=0x7f => out.push(c),
            n @ 0x80..=0xff => out.push_str(&format!("\\x{n:02x}")),
            n @ 0x100..=0xffff => out.push_str(&format!("\\u{n:04x}")),
            n => out.push_str(&format!("\\U{n:08x}")),
        }
    }
    out
}

/// `format(str, spec)`: `[[fill]align][sign][z][#][0][width][,|_][.precision][type]`, of which
/// a string takes the fill, the alignment, the zero, the width, the precision and the type `s`.
fn format_str(text: &str, spec: &str) -> Result<String, FormatError> {
    if spec.is_empty() {
        return Ok(text.to_string());
    }
    let chars: Vec<char> = spec.chars().collect();
    let value = |m: String| FormatError::Value(m);
    let mut i = 0;
    let (mut fill, mut align) = (None, None);
    let is_align = |c: char| matches!(c, '<' | '>' | '=' | '^');
    if chars.len() >= 2 && is_align(chars[1]) {
        fill = Some(chars[0]);
        align = Some(chars[1]);
        i = 2;
    } else if !chars.is_empty() && is_align(chars[0]) {
        align = Some(chars[0]);
        i = 1;
    }
    let sign = matches!(chars.get(i), Some('+' | '-' | ' '));
    if sign {
        i += 1;
    }
    let zcoerce = chars.get(i) == Some(&'z');
    if zcoerce {
        i += 1;
    }
    let alternate = chars.get(i) == Some(&'#');
    if alternate {
        i += 1;
    }
    let mut zero = false;
    if chars.get(i) == Some(&'0') {
        zero = true;
        i += 1;
    }
    let digits = |i: &mut usize| -> Option<usize> {
        let start = *i;
        while chars.get(*i).is_some_and(char::is_ascii_digit) {
            *i += 1;
        }
        (*i > start).then(|| {
            chars[start..*i]
                .iter()
                .collect::<String>()
                .parse()
                .unwrap_or(usize::MAX)
        })
    };
    let width = digits(&mut i);
    let grouping = match chars.get(i) {
        Some(c @ (',' | '_')) => {
            i += 1;
            Some(*c)
        }
        _ => None,
    };
    let mut precision = None;
    if chars.get(i) == Some(&'.') {
        i += 1;
        precision =
            Some(digits(&mut i).ok_or_else(|| value("Format specifier missing precision".into()))?);
    }
    let rest = chars.len() - i;
    if rest > 1 {
        return Err(value(format!(
            "Invalid format specifier '{spec}' for object of type 'str'"
        )));
    }
    let kind = if rest == 1 { chars[i] } else { 's' };
    if let Some(g) = grouping
        && !"defgEG%F".contains(kind)
    {
        return Err(value(format!("Cannot specify '{g}' with '{kind}'.")));
    }
    if kind != 's' {
        return Err(value(format!(
            "Unknown format code '{kind}' for object of type 'str'"
        )));
    }
    if sign {
        return Err(value("Sign not allowed in string format specifier".into()));
    }
    if zcoerce {
        return Err(value(
            "Negative zero coercion (z) not allowed in format specifier".into(),
        ));
    }
    if alternate {
        return Err(value(
            "Alternate form (#) not allowed in string format specifier".into(),
        ));
    }
    if align == Some('=') {
        return Err(value(
            "'=' alignment not allowed in string format specifier".into(),
        ));
    }
    let mut text = text.to_string();
    if let Some(p) = precision {
        text = head_chars(&text, p.min(i64::MAX as usize) as i64);
    }
    let width = width.unwrap_or(0);
    let len = text.chars().count();
    if width <= len {
        return Ok(text);
    }
    let fill = fill.unwrap_or(if zero && align.is_none() { '0' } else { ' ' });
    let pad = width - len;
    let (left, right) = match align.unwrap_or('<') {
        '>' => (pad, 0),
        '^' => (pad / 2, pad - pad / 2),
        _ => (0, pad),
    };
    Ok(format!(
        "{}{text}{}",
        fill.to_string().repeat(left),
        fill.to_string().repeat(right)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(fields: &[(&str, &str)]) -> Entry {
        let mut e = Entry::new("article", "smith2020");
        for (k, v) in fields {
            e.set(k, *v);
        }
        e
    }

    #[test]
    fn placeholders() {
        let e = entry(&[
            ("author", "Smith, John and Doe, Jane and Roe, R."),
            ("year", "2020"),
            ("title", "Climate"),
        ]);
        let f = make_template_fields(&e, &Format::name());
        assert_eq!(f.get("author").unwrap(), "smith_doe");
        assert_eq!(f.get("Author").unwrap(), "Smith_Doe");
        assert_eq!(f.get("AUTHOR").unwrap(), "SMITH_DOE");
        assert_eq!(f.get("authorX").unwrap(), "smith_et_al");
        assert_eq!(f.get("AuthorX").unwrap(), "Smith_et_al");
        assert_eq!(f.get("journal").unwrap(), "");
        assert_eq!(f.get("key").unwrap(), "smith2020");
    }

    #[test]
    fn python_format() {
        let e = entry(&[
            ("author", "Smith, John"),
            ("year", "2020"),
            ("title", "A study of ice"),
        ]);
        let r = |t: &str| {
            let mut f = Format::name();
            f.template = t.into();
            f.render(&e)
        };
        assert_eq!(r("{{x}}{year}").unwrap(), "{x}2020");
        assert_eq!(
            r("{title:.5}|{year:>6}|{author:*^9}|{year:06}").unwrap(),
            "a-stu-  2020---smith---202000" // `|` and `*` are not valid on Windows
        );
        assert_eq!(r("{author!r}{year!s}").unwrap(), "'smith'2020");
        assert_eq!(r("{author[0]}").unwrap(), "s");
        assert!(
            r("{year:x}")
                .unwrap_err()
                .0
                .contains("Unknown format code 'x' for object of type 'str'")
        );
        assert!(r("{xxx}").unwrap_err().0.starts_with(
            "unknown placeholder {xxx} in the name template '{xxx}'; available: {author}"
        ));
        assert!(
            r("{}")
                .unwrap_err()
                .0
                .starts_with("unknown placeholder {} in")
        );
        assert_eq!(
            r("{year").unwrap_err().0,
            "invalid name template '{year': expected '}' before end of string"
        );
        assert_eq!(
            r("a}").unwrap_err().0,
            "invalid name template 'a}': Single '}' encountered in format string"
        );
    }
}
