//! `papers tag`: the tags of the entries (the `keywords` field, nested with
//! `/`): add, remove, rename a tag in the whole library, and the listing with
//! the number of entries of each tag (`papers.cli.entry_cmds.tagcmd`).

use std::cmp::Reverse;
use std::collections::HashMap;

use crate::bib::Entry;
use crate::entry::{tag_counts, tag_parents};
use crate::library::Library;

/// Add tags to the entries of these keys, then remove tags: the keys of the entries to report.
pub fn add_remove(library: &mut Library, keys: &[String], add: &[String], remove: &[String]) {
    library.add_tags(keys, add);
    library.remove_tags(keys, remove);
}

/// Rename a tag (and the tags under it) in the whole library: how many entries changed.
pub fn rename(library: &mut Library, old: &str, new: &str) -> usize {
    library.rename_tag(old, new).len()
}

/// The line that says it (`renamed tag OLD to NEW in N entries`).
pub fn renamed_line(old: &str, new: &str, count: usize) -> String {
    format!("renamed tag {old} to {new} in {count} entries")
}

/// The tags with their number of entries, the nested tags indented under their
/// parent, the most used first, then by name (`print_tag_tree`).
pub fn tag_tree<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Vec<String> {
    let counts = tag_counts(entries);
    let count: HashMap<&str, usize> = counts.iter().map(|(t, n)| (t.as_str(), *n)).collect();
    let mut children: HashMap<Option<String>, Vec<&str>> = HashMap::new();
    for (tag, _) in &counts {
        let parent = tag_parents(tag).pop();
        children.entry(parent).or_default().push(tag);
    }
    let mut lines = Vec::new();
    fn show(
        parent: Option<&str>,
        depth: usize,
        children: &HashMap<Option<String>, Vec<&str>>,
        count: &HashMap<&str, usize>,
        lines: &mut Vec<String>,
    ) {
        let mut tags: Vec<&str> = children
            .get(&parent.map(str::to_string))
            .cloned()
            .unwrap_or_default();
        tags.sort_by_key(|t| (Reverse(count[t]), t.to_lowercase()));
        for tag in tags {
            let name = match parent {
                Some(p) => tag.chars().skip(p.chars().count() + 1).collect::<String>(),
                None => tag.to_string(),
            };
            lines.push(format!("{}{name} ({})", "  ".repeat(depth), count[tag]));
            show(Some(tag), depth + 1, children, count, lines);
        }
    }
    show(None, 0, &children, &count, &mut lines);
    lines
}
