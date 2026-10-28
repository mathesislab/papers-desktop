//! Paths as Python's `os.path` writes them: strings, normalised lexically
//! (`abspath`) or through the symbolic links (`realpath`). papers stores and
//! prints paths in these forms, so the library does the same.

use std::fs;

const SEP: char = '/';

/// `os.path.expanduser('~')`: `$HOME`.
pub fn home() -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() => home,
        _ => "/".to_string(),
    }
}

/// `os.path.expanduser`: `~` and `~/...` (`~user` is left as it is).
pub fn expanduser(path: &str) -> String {
    let home = home();
    let home = home.trim_end_matches(SEP);
    if path == "~" {
        return if home.is_empty() { "/".to_string() } else { home.to_string() };
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return format!("{home}/{rest}");
    }
    path.to_string()
}

pub fn is_abs(path: &str) -> bool {
    path.starts_with(SEP)
}

/// `os.path.join(a, b)`.
pub fn join(a: &str, b: &str) -> String {
    if is_abs(b) {
        b.to_string()
    } else if a.is_empty() || a.ends_with(SEP) {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// `os.path.dirname`.
pub fn dirname(path: &str) -> String {
    let i = path.rfind(SEP).map(|i| i + 1).unwrap_or(0);
    let head = &path[..i];
    if !head.is_empty() && head.chars().any(|c| c != SEP) {
        head.trim_end_matches(SEP).to_string()
    } else {
        head.to_string()
    }
}

/// `os.path.basename`.
pub fn basename(path: &str) -> String {
    let i = path.rfind(SEP).map(|i| i + 1).unwrap_or(0);
    path[i..].to_string()
}

/// `os.path.normpath`.
pub fn normpath(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    let initial = if path.starts_with("//") && !path.starts_with("///") {
        2
    } else if path.starts_with(SEP) {
        1
    } else {
        0
    };
    let mut comps: Vec<&str> = Vec::new();
    for comp in path.split(SEP) {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp != ".."
            || (initial == 0 && comps.is_empty())
            || comps.last().is_some_and(|last| *last == "..")
        {
            comps.push(comp);
        } else if !comps.is_empty() {
            comps.pop();
        }
    }
    let joined = comps.join("/");
    let result = format!("{}{joined}", "/".repeat(initial));
    if result.is_empty() {
        ".".to_string()
    } else {
        result
    }
}

/// `os.getcwd()`.
pub fn getcwd() -> String {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "/".to_string())
}

/// `os.path.abspath`.
pub fn abspath(path: &str) -> String {
    if is_abs(path) {
        normpath(path)
    } else {
        normpath(&join(&getcwd(), path))
    }
}

/// `os.path.relpath(path, start)`.
pub fn relpath(path: &str, start: &str) -> String {
    let start_abs = abspath(start);
    let path_abs = abspath(path);
    let start_list: Vec<&str> = start_abs.split(SEP).filter(|c| !c.is_empty()).collect();
    let path_list: Vec<&str> = path_abs.split(SEP).filter(|c| !c.is_empty()).collect();
    let common = start_list
        .iter()
        .zip(path_list.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut rel: Vec<&str> = vec![".."; start_list.len() - common];
    rel.extend(&path_list[common..]);
    if rel.is_empty() {
        ".".to_string()
    } else {
        rel.join("/")
    }
}

/// `os.path.realpath` (not strict: what does not exist is kept as written,
/// `..` removes the component before it).
pub fn realpath(path: &str) -> String {
    let mut seen_links = 0;
    let start = if is_abs(path) {
        path.to_string()
    } else {
        join(&getcwd(), path)
    };
    let mut pending: Vec<String> = start
        .split(SEP)
        .filter(|c| !c.is_empty() && *c != ".")
        .map(str::to_string)
        .rev()
        .collect();
    let mut resolved: Vec<String> = Vec::new();
    while let Some(comp) = pending.pop() {
        if comp == ".." {
            resolved.pop();
            continue;
        }
        let candidate = format!("/{}", resolved.iter().chain([&comp]).cloned().collect::<Vec<_>>().join("/"));
        match fs::symlink_metadata(&candidate) {
            Ok(meta) if meta.file_type().is_symlink() => {
                seen_links += 1;
                if seen_links > 40 {
                    // a loop: keep the rest as it is
                    resolved.push(comp);
                    resolved.extend(pending.drain(..).rev());
                    break;
                }
                match fs::read_link(&candidate) {
                    Ok(target) => {
                        let target = target.to_string_lossy().into_owned();
                        if is_abs(&target) {
                            resolved.clear();
                        }
                        for part in target.split(SEP).filter(|c| !c.is_empty() && *c != ".").rev() {
                            pending.push(part.to_string());
                        }
                    }
                    Err(_) => resolved.push(comp),
                }
            }
            _ => resolved.push(comp),
        }
    }
    format!("/{}", resolved.join("/"))
}

/// `a path under the home directory as ~/...`, as papers writes it in the
/// global configuration and in its messages.
pub fn short_home(path: &str) -> String {
    let home = expanduser("~");
    let path = abspath(path);
    if path == home || path.starts_with(&format!("{home}{SEP}")) {
        format!("~{}", &path[home.len()..])
    } else {
        path
    }
}

/// `path` is `folder` or inside it (real paths).
pub fn is_inside(path: &str, folder: &str) -> bool {
    let path = realpath(path);
    let folder = realpath(folder);
    path == folder || path.starts_with(&format!("{}{SEP}", folder.trim_end_matches(SEP)))
}

pub fn exists(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

pub fn isfile(path: &str) -> bool {
    fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
}

pub fn isdir(path: &str) -> bool {
    fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normpath_like_python() {
        assert_eq!(normpath(""), ".");
        assert_eq!(normpath("a//b/./c/.."), "a/b");
        assert_eq!(normpath("/.."), "/");
        assert_eq!(normpath("//a"), "//a");
        assert_eq!(normpath("///a"), "/a");
        assert_eq!(normpath("../../a"), "../../a");
        assert_eq!(normpath("a/../.."), "..");
    }

    #[test]
    fn dirname_basename_like_python() {
        assert_eq!(dirname("/a/b"), "/a");
        assert_eq!(dirname("a"), "");
        assert_eq!(dirname("/a"), "/");
        assert_eq!(dirname("a/b/"), "a/b");
        assert_eq!(dirname(".papers/config.json"), ".papers");
        assert_eq!(basename("/a/b/"), "");
        assert_eq!(basename("/a/b.c"), "b.c");
    }

    #[test]
    fn relpath_like_python() {
        assert_eq!(relpath("/a/b/c", "/a"), "b/c");
        assert_eq!(relpath("/a", "/a/b/c"), "../..");
        assert_eq!(relpath("/a/x", "/a/b"), "../x");
        assert_eq!(relpath("/a", "/a"), ".");
    }

    #[test]
    fn join_like_python() {
        assert_eq!(join("a", "b"), "a/b");
        assert_eq!(join("a/", "b"), "a/b");
        assert_eq!(join("a", "/b"), "/b");
        assert_eq!(join("", "b"), "b");
        assert_eq!(join(".", ".papers/config.json"), "./.papers/config.json");
    }

    #[test]
    fn realpath_follows_links_and_keeps_missing_parts() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let root = root.to_string_lossy().into_owned();
        fs::create_dir(format!("{root}/real")).unwrap();
        std::os::unix::fs::symlink(format!("{root}/real"), format!("{root}/link")).unwrap();
        assert_eq!(realpath(&format!("{root}/link/x/../y")), format!("{root}/real/y"));
        assert_eq!(realpath(&format!("{root}/missing/../real")), format!("{root}/real"));
    }
}
