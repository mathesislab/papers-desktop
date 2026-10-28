//! The order and the text of a saved bibtex: `papers.entries.sort_blocks`
//! and `format_library` (bibtexparser's writer with the default unparse stack,
//! a one-space indent and a single newline between the blocks).

use super::pystr::{splitlines, strip};
use super::read::is_writable_unenclosed;
use std::borrow::Cow;

use super::{Block, Field};

/// The order of the blocks of a saved bibtex: strings, preambles, entries,
/// comments, then what could not be read.
fn type_index(block: &Block) -> usize {
    match block {
        Block::String(_) => 0,
        Block::Preamble(_) => 1,
        Block::Entry(_) => 2,
        Block::Text(_) => 3,
        Block::Comment(_) => 4,
        Block::Unreadable(_) => 5,
    }
}

fn sort_key(block: &Block) -> (usize, &str) {
    let key = match block {
        Block::Entry(e) => e.key.as_str(),
        Block::String(s) => s.key.as_str(),
        Block::Unreadable(u) => u.key(),
        _ => "",
    };
    (type_index(block), key)
}

/// The order of the blocks of a saved bibtex (`papers.entries.sort_blocks`):
/// by type and key, the comments above a block staying with it, the comments
/// at the end sorted by the last of them. The indexes of the blocks, in that order.
pub(super) fn sort_order(blocks: &[Block]) -> Vec<usize> {
    let mut chunks: Vec<Vec<usize>> = Vec::new();
    let mut chunk: Vec<usize> = Vec::new();
    for (i, block) in blocks.iter().enumerate() {
        let is_comment = matches!(block, Block::Text(_) | Block::Comment(_));
        chunk.push(i);
        if !is_comment {
            chunks.push(std::mem::take(&mut chunk));
        }
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    chunks.sort_by(|a, b| {
        sort_key(&blocks[*a.last().unwrap()]).cmp(&sort_key(&blocks[*b.last().unwrap()]))
    });
    chunks.into_iter().flatten().collect()
}

/// Order blocks by type and key, the comments above a block staying with it.
pub(super) fn sort_blocks(blocks: Vec<Block>) -> Vec<Block> {
    let order = sort_order(&blocks);
    let mut blocks: Vec<Option<Block>> = blocks.into_iter().map(Some).collect();
    order
        .into_iter()
        .map(|i| blocks[i].take().unwrap())
        .collect()
}

const PARSING_FAILED: &str = "% WARNING Parsing failed for the following ";

/// Whether `line` is the line that is written above an unreadable block.
fn is_failed_comment(line: &str) -> bool {
    match line
        .strip_prefix(PARSING_FAILED)
        .and_then(|r| r.strip_suffix(" lines."))
    {
        Some(n) => !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

/// Whether a text has such a line in it (`re.search`).
fn has_failed_comment(text: &str) -> bool {
    let mut from = 0;
    while let Some(i) = text[from..].find(PARSING_FAILED) {
        let rest = &text[from + i + PARSING_FAILED.len()..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && rest[digits..].starts_with(" lines.") {
            return true;
        }
        from += i + 1;
    }
    false
}

/// A comment without the lines that were written above an unreadable block
/// at the previous saves (they are written again), or `None` if nothing else is left.
fn without_failed_comments(comment: &str) -> Option<String> {
    if !has_failed_comment(comment) {
        return Some(comment.to_string());
    }
    let lines: Vec<&str> = splitlines(comment)
        .into_iter()
        .filter(|l| !is_failed_comment(strip(l)))
        .collect();
    let joined = lines.join("\n");
    let comment = joined.trim_matches('\n');
    if strip(comment).is_empty() {
        None
    } else {
        Some(comment.to_string())
    }
}

/// A value in its enclosing: braces, except a value that must be written as it is.
fn enclose(value: &str, verbatim: bool) -> String {
    if verbatim && is_writable_unenclosed(value) {
        value.to_string()
    } else {
        format!("{{{value}}}")
    }
}

fn write_field(out: &mut String, field: &Field, last: bool) {
    out.push(' ');
    out.push_str(&field.key);
    out.push_str(" = ");
    out.push_str(&enclose(&field.value, field.verbatim));
    if !last {
        out.push(',');
    }
    out.push('\n');
}

/// The text of the blocks in the order they have.
pub(super) fn format(blocks: &[Block]) -> String {
    let blocks: Vec<Cow<Block>> = blocks
        .iter()
        .filter_map(|b| match b {
            Block::Text(comment) => {
                without_failed_comments(comment).map(|c| Cow::Owned(Block::Text(c)))
            }
            b => Some(Cow::Borrowed(b)),
        })
        .collect();

    let mut out = String::new();
    let n = blocks.len();
    for (i, block) in blocks.iter().enumerate() {
        match block.as_ref() {
            Block::Entry(e) => {
                out.push_str(&format!("@{}{{{},\n", e.entry_type, e.key));
                let mut fields: Vec<&Field> = e.fields.iter().collect();
                fields.sort_by(|a, b| a.key.cmp(&b.key));
                let m = fields.len();
                for (j, f) in fields.into_iter().enumerate() {
                    write_field(&mut out, f, j + 1 == m);
                }
                out.push_str("}\n");
            }
            Block::String(s) => {
                out.push_str(&format!(
                    "@string{{{} = {}}}\n",
                    s.key,
                    enclose(&s.value, s.verbatim)
                ));
            }
            Block::Preamble(v) => out.push_str(&format!("@preamble{{{v}}}\n")),
            Block::Comment(c) => out.push_str(&format!("@comment{{{c}}}\n")),
            Block::Text(c) => {
                out.push_str(c);
                out.push('\n');
            }
            Block::Unreadable(u) => {
                let lines = splitlines(&u.raw).len();
                out.push_str(&format!("{PARSING_FAILED}{lines} lines.\n{}\n", u.raw));
            }
        }
        if i + 1 < n {
            out.push('\n');
        }
    }
    out
}
