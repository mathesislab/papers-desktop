//! What papers reads in the values of an entry (`papers.encoding`,
//! `papers.entries`): the names of the authors, the keywords, and the `file`
//! field (`:path:type;:path:type`).

use std::path::{Component, Path, PathBuf};

use super::pystr::{is_space, strip};

/// The separators that a value is split on outside braces ([`split_depth0`]).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sep {
    /// `\s+and\s+`: the `and` between authors
    And,
    /// `,`
    Comma,
    /// `\s+`
    Space,
}

impl Sep {
    /// The length of the separator that starts at byte `i` of `value`, if one
    /// does (`re.match(pattern, value, i)`).
    fn matches(self, value: &str, i: usize) -> Option<usize> {
        let rest = &value[i..];
        let spaces = |s: &str| s.len() - s.trim_start_matches(is_space).len();
        match self {
            Sep::Comma => rest.starts_with(',').then_some(1),
            Sep::Space => Some(spaces(rest)).filter(|&n| n > 0),
            Sep::And => {
                let lead = spaces(rest);
                let after = rest[lead..].strip_prefix("and")?;
                let trail = spaces(after);
                (lead > 0 && trail > 0).then_some(lead + 3 + trail)
            }
        }
    }
}

/// A value split on a separator that is outside braces, each part stripped
/// (`split_depth0`): a `{...}` group is one word whatever it contains. The
/// depth never goes below 0: an unmatched `}` does not stop the split.
///
/// ```
/// use papers_rs::bib::encoding::{Sep, split_depth0};
/// assert_eq!(
///     split_depth0("{Barnes and Noble} and Smith, J.", Sep::And),
///     ["{Barnes and Noble}", "Smith, J."]
/// );
/// ```
pub fn split_depth0(value: &str, sep: Sep) -> Vec<String> {
    let mut parts: Vec<&str> = Vec::new();
    let (mut depth, mut start, mut i) = (0i64, 0, 0);
    while i < value.len() {
        let c = value[i..].chars().next().unwrap();
        match c {
            '{' => depth += 1,
            '}' => depth = (depth - 1).max(0), // a stray } does not stop the split
            _ if depth == 0 => {
                if let Some(n) = sep.matches(value, i) {
                    parts.push(&value[start..i]);
                    i += n;
                    start = i;
                    continue;
                }
            }
            _ => {}
        }
        i += c.len_utf8();
    }
    parts.push(&value[start..]);
    parts.into_iter().map(|p| strip(p).to_string()).collect()
}

/// Authors as `Last, First and Last, First`: a name with a comma (outside
/// braces) is kept, one without has its last word moved to the front. A part
/// in braces is one word: `John {van der Berg}` is `{van der Berg}, John`,
/// `{World Health Organization}` stays (`standard_name`).
pub fn standard_name(author: &str) -> String {
    let parts: Vec<String> = split_depth0(author, Sep::And)
        .into_iter()
        .filter(|p| !p.is_empty())
        .map(|p| {
            if split_depth0(&p, Sep::Comma).len() > 1 {
                return p;
            }
            let words = split_depth0(&p, Sep::Space);
            match words.split_last() {
                Some((last, first)) if !first.is_empty() => format!("{last}, {}", first.join(" ")),
                _ => p,
            }
        })
        .collect();
    parts.join(" and ")
}

/// The family name of each author; names and commas in braces are not split (`family_names`).
pub fn family_names(author: &str) -> Vec<String> {
    split_depth0(&standard_name(author), Sep::And)
        .iter()
        .map(|n| split_depth0(n, Sep::Comma).swap_remove(0))
        .collect()
}

/// The keywords of a `keywords` value: split at the commas, stripped, the empty ones dropped.
pub fn parse_keywords(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(strip)
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// The path of one file of a `file` field: `path`, `path:type` or
/// `description:path:type`, where the path of the last form may itself
/// contain colons.
fn parse_one(file: &str) -> &str {
    let parts: Vec<&str> = file.split(':').collect();
    match parts.len() {
        1 => file,
        2 => parts[0],
        n => {
            // the part of the text between the first and the last colon
            let first = parts[0].len() + 1;
            let last = file.len() - parts[n - 1].len() - 1;
            &file[first..last]
        }
    }
}

/// The paths of a `file` field, as written (`parse_file`). The links that are
/// empty are left out, those with an empty path too, with a warning.
pub fn parse_file(file: &str) -> Vec<String> {
    file.split(';')
        .filter(|f| !strip(f).is_empty())
        .filter_map(|f| {
            let path = strip(parse_one(f));
            if path.is_empty() {
                crate::log::warning(&format!(
                    "empty file path in file field (ignored): {}",
                    crate::config::json::py_repr_str(f)
                ));
                return None;
            }
            Some(path.to_string())
        })
        .collect()
}

/// The absolute paths of a `file` field whose paths are relative to
/// `relative_to` (`parse_file(file, relative_to)`): joined and normalized, the links not followed.
pub fn parse_file_relative(file: &str, relative_to: &Path) -> Vec<PathBuf> {
    parse_file(file)
        .iter()
        .map(|f| normpath(&relative_to.join(f)))
        .collect()
}

/// The `file` field of some paths (`format_file`): each as `:path:type`, the
/// type being the extension.
pub fn format_file(files: &[String]) -> String {
    files
        .iter()
        .map(|f| format!(":{f}:{}", extension(f)))
        .collect::<Vec<_>>()
        .join(";")
}

/// The `file` field of some absolute paths, written relative to `relative_to`
/// (`format_file(files, relative_to)`); with `/` as `relative_to`, as they are.
pub fn format_file_relative(files: &[PathBuf], relative_to: &Path) -> String {
    let files: Vec<String> = files
        .iter()
        .map(|p| {
            let p = if relative_to == Path::new("/") {
                normpath(p)
            } else {
                normpath(&relpath(p, relative_to))
            };
            p.to_string_lossy().into_owned()
        })
        .collect();
    format_file(&files)
}

/// A `file` field whose paths were relative to `from` made relative to `to`
/// (`update_file_path`).
pub fn update_file_path(file: &str, from: &Path, to: &Path) -> String {
    format_file_relative(&parse_file_relative(file, from), to)
}

/// `os.path.splitext(path)[1].strip('.')`
fn extension(path: &str) -> String {
    let base_start = path.rfind('/').map_or(0, |i| i + 1);
    let base = &path[base_start..];
    // a leading dot of the base name is not an extension
    let stem_len = base.len() - base.trim_start_matches('.').len();
    match base[stem_len..].rfind('.') {
        Some(i) => base[stem_len + i..].trim_matches('.').to_string(),
        None => String::new(),
    }
}

/// `os.path.normpath` on a path: `.` and `..` and repeated separators collapsed, lexically.
pub fn normpath(path: &Path) -> PathBuf {
    let absolute = path.has_root();
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => parts.push(p.to_os_string()),
            Component::ParentDir => {
                if parts.last().is_some_and(|l| l != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..".into());
                }
            }
            _ => {}
        }
    }
    let mut out = PathBuf::new();
    if absolute {
        out.push("/");
    }
    out.extend(parts);
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// `os.path.relpath(path, start)` of two absolute paths.
fn relpath(path: &Path, start: &Path) -> PathBuf {
    let path = normpath(path);
    let start = normpath(start);
    let p: Vec<_> = path.components().collect();
    let s: Vec<_> = start.components().collect();
    let common = p.iter().zip(&s).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in common..s.len() {
        out.push("..");
    }
    for c in &p[common..] {
        out.push(c.as_os_str());
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(standard_name("Smith, John"), "Smith, John");
        assert_eq!(
            standard_name("John Smith and Jane Doe"),
            "Smith, John and Doe, Jane"
        );
        assert_eq!(standard_name("Cher"), "Cher");
        assert_eq!(
            family_names("Perrette, M. and Yool, A."),
            vec!["Perrette", "Yool"]
        );
        assert_eq!(
            family_names("Doe, Jane and Smith, John"),
            vec!["Doe", "Smith"]
        );
    }

    /// The brace depth never goes below 0: a stray `}` does not stop the split.
    #[test]
    fn a_stray_closing_brace() {
        assert_eq!(split_depth0("a} and b", Sep::And), ["a}", "b"]);
        assert_eq!(
            split_depth0("a} and {b and c} and d", Sep::And),
            ["a}", "{b and c}", "d"]
        );
        assert_eq!(standard_name("x} and y z"), "x} and z, y");
    }

    #[test]
    fn keywords() {
        assert_eq!(
            parse_keywords("kiwi, ocean, climate"),
            vec!["kiwi", "ocean", "climate"]
        );
        assert_eq!(
            parse_keywords(" sea-level ,  projections "),
            vec!["sea-level", "projections"]
        );
        assert!(parse_keywords("").is_empty());
    }

    #[test]
    fn file_field() {
        let one = |s| parse_file(s);
        assert_eq!(
            one("file.pdf:/path/to/file.pdf:pdf"),
            vec!["/path/to/file.pdf"]
        );
        assert_eq!(one(":/path/to/file.pdf:pdf"), vec!["/path/to/file.pdf"]);
        assert_eq!(one("/path/to/file.pdf:pdf"), vec!["/path/to/file.pdf"]);
        assert_eq!(one("/path/to/file.pdf"), vec!["/path/to/file.pdf"]);
        assert_eq!(one(":/path/to/file.pdf:"), vec!["/path/to/file.pdf"]);
        assert_eq!(
            one(":/path/to/file1.pdf:pdf;:/path/to/file2.pdf:pdf"),
            vec!["/path/to/file1.pdf", "/path/to/file2.pdf"]
        );
        // issue #95: the path itself may contain colons
        assert_eq!(
            one(":/path/to/American Economic Journal: Applied Economics.pdf:pdf"),
            vec!["/path/to/American Economic Journal: Applied Economics.pdf"]
        );
        assert_eq!(
            one("desc:/path/with:colon.pdf:pdf"),
            vec!["/path/with:colon.pdf"]
        );
        // issue #91: an empty path is ignored
        assert!(one("::pdf").is_empty());
        assert_eq!(one(":/path/to/file.pdf:pdf;"), vec!["/path/to/file.pdf"]);
    }

    #[test]
    fn format_file_field() {
        assert_eq!(
            format_file(&["/path/to/file.pdf".into()]),
            ":/path/to/file.pdf:pdf"
        );
        assert_eq!(
            format_file(&["/path/to/file1.pdf".into(), "/path/to/file2.pdf".into()]),
            ":/path/to/file1.pdf:pdf;:/path/to/file2.pdf:pdf"
        );
        assert_eq!(
            format_file(&[
                "noext".into(),
                ".hidden".into(),
                "a.b.tar.gz".into(),
                "x.".into()
            ]),
            ":noext:;:.hidden:;:a.b.tar.gz:gz;:x.:"
        );
    }

    #[test]
    fn file_paths_relative() {
        let abs = parse_file_relative(":files/a.pdf:pdf", Path::new("/lib"));
        assert_eq!(abs, vec![PathBuf::from("/lib/files/a.pdf")]);
        assert_eq!(
            format_file_relative(&abs, Path::new("/lib")),
            ":files/a.pdf:pdf"
        );
        assert_eq!(
            format_file_relative(&abs, Path::new("/lib/sub")),
            ":../files/a.pdf:pdf"
        );
        assert_eq!(
            format_file_relative(&abs, Path::new("/")),
            ":/lib/files/a.pdf:pdf"
        );
        assert_eq!(
            update_file_path(":a.pdf:pdf", Path::new("/x"), Path::new("/")),
            ":/x/a.pdf:pdf"
        );
    }
}
