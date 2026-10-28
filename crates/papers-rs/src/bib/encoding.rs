//! What papers reads in the values of an entry (`papers.encoding`,
//! `papers.entries`): the names of the authors, the keywords, and the `file`
//! field (`:path:type;:path:type`).

use std::path::{Component, Path, PathBuf};

use super::pystr::strip;

/// Authors as `Last, First and Last, First`: a name with a comma is kept,
/// one without has its last word moved to the front (`standard_name`).
pub fn standard_name(author: &str) -> String {
    let parts: Vec<String> = author
        .split(" and ")
        .map(|p| {
            let p = strip(p);
            if p.contains(',') {
                return p.to_string();
            }
            match p.rsplit_once(' ') {
                Some((first, last)) => format!("{last}, {first}"),
                None => p.to_string(),
            }
        })
        .collect();
    parts.join(" and ")
}

/// The family name of each author (`family_names`).
pub fn family_names(author: &str) -> Vec<String> {
    standard_name(author)
        .split(" and ")
        .map(|n| n.split(',').next().unwrap_or("").to_string())
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
            let path = parse_one(f);
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
