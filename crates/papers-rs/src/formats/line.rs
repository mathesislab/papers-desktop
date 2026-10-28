//! The lines of `papers list` (`papers.encoding.format_key`, `format_entry`): the key, the title
//! and what the entry has (DOI, files, tags); with colours and terminal hyperlinks (OSC 8) or without.

use crate::bib::Entry;
use crate::bib::encoding::parse_keywords;
use crate::config::paths::realpath;
use crate::entry::{files, val};

const BOLD: &str = "\u{1b}[1m";
const BLUE: &str = "\u{1b}[94m";
const GREEN: &str = "\u{1b}[92m";
const YELLOW: &str = "\u{1b}[93m";
const END: &str = "\u{1b}[0m";

/// How the output is written: with colours and hyperlinks, or as plain text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    pub color: bool,
}

impl Style {
    fn code(self, code: &'static str) -> &'static str {
        if self.color { code } else { "" }
    }

    /// `ansi_link`: the label as a terminal hyperlink to `uri` (the label only without colours).
    pub fn link(self, uri: &str, label: &str) -> String {
        if self.color {
            format!("\u{1b}]8;;{uri}\u{1b}\\{label}\u{1b}]8;;\u{1b}\\")
        } else {
            label.to_string()
        }
    }
}

/// The value of a field of an entry as `get_entry_val(e, name, '')` has it: `ID` is the key, `ENTRYTYPE` the type.
pub fn field_value<'a>(entry: &'a Entry, name: &str) -> &'a str {
    match name {
        "ID" => &entry.key,
        "ENTRYTYPE" => &entry.entry_type,
        name => val(entry, name),
    }
}

/// The key with a colon, in blue, in bold once for each file (`format_key`); nothing with `no_key`.
pub fn format_key(entry: &Entry, no_key: bool, style: Style) -> String {
    if no_key {
        return String::new();
    }
    let n = files(entry, None).len();
    format!(
        "{}{}{}:{}",
        style.code(BOLD).repeat(n),
        style.code(BLUE),
        entry.key,
        style.code(END)
    )
}

/// The deepest directory that holds all the paths (`os.path.commonpath`, for absolute paths).
fn common_path(paths: &[String]) -> String {
    let split: Vec<Vec<&str>> = paths
        .iter()
        .map(|p| {
            p.split('/')
                .filter(|c| !c.is_empty() && *c != ".")
                .collect()
        })
        .collect();
    let mut common: Vec<&str> = split[0].clone();
    for other in &split[1..] {
        let n = common.iter().zip(other).take_while(|(a, b)| a == b).count();
        common.truncate(n);
    }
    format!("/{}", common.join("/"))
}

/// The line of an entry: `KEY: TITLE (doi:..., file:1, tags)` (`format_entry`); `relative_to`
/// is what the file links are relative to. With a `prefix`, `Added -> ` and so on in front.
pub fn format_entry(
    entry: &Entry,
    relative_to: &str,
    no_key: bool,
    prefix: &str,
    style: Style,
) -> String {
    let title = val(entry, "title");
    let tit: String = title.chars().take(60).collect::<String>()
        + if title.chars().count() > 60 {
            "..."
        } else {
            ""
        };
    let mut info: Vec<String> = Vec::new();
    let doi = val(entry, "doi");
    if !doi.is_empty() {
        info.push(style.link(&format!("https://doi.org/{doi}"), &format!("doi:{doi}")));
    }
    let paths = files(entry, Some(relative_to));
    let n = paths.len();
    if n > 0 {
        let target = if n == 1 {
            realpath(&paths[0])
        } else {
            realpath(&common_path(&paths))
        };
        let label = format!("{}:{n}", if n == 1 { "file" } else { "files" });
        info.push(format!(
            "{}{}{}",
            style.code(GREEN),
            style.link(&format!("file:///{target}"), &label),
            style.code(END)
        ));
    }
    if !val(entry, "keywords").is_empty() {
        info.push(format!(
            "{}{}{}",
            style.code(YELLOW),
            parse_keywords(val(entry, "keywords")).join(" | "),
            style.code(END)
        ));
    }
    let infotag = if info.is_empty() {
        String::new()
    } else {
        format!("({})", info.join(", "))
    };
    let prefixtag = if prefix.is_empty() {
        String::new()
    } else {
        format!("{}{prefix} -> {}", style.code(YELLOW), style.code(END))
    };
    format!(
        "{prefixtag}{} {tit} {infotag}",
        format_key(entry, no_key, style)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bib::Bib;

    #[test]
    fn lines() {
        let bib = Bib::parse(
            "@article{a, title = {Ice}, doi = {10.1/x}, file = {:f/a.pdf:pdf}, keywords = {x, y/z}}\n@article{b, title = {B}}",
        );
        let e: Vec<_> = bib.entries().collect();
        let plain = Style::default();
        assert_eq!(
            format_entry(e[0], "/lib", false, "", plain),
            "a: Ice (doi:10.1/x, file:1, x | y/z)"
        );
        assert_eq!(format_entry(e[1], "/lib", false, "", plain), "b: B ");
        assert_eq!(format_entry(e[1], "/lib", true, "", plain), " B ");
        assert!(
            format_entry(e[0], "/lib", false, "", Style { color: true })
                .starts_with("\u{1b}[1m\u{1b}[94ma:\u{1b}[0m")
        );
    }
}
