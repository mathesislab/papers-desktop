//! The splitter of bibtexparser v2 (`bibtexparser.splitter`), ported as it
//! is: the same marks, the same state, so that what it cannot read is the
//! same, and so are the lines it reports. It cuts a text into blocks and
//! keeps the values as written (enclosed); `read` then runs the middlewares.

use std::collections::HashSet;

use super::pystr::{is_space, is_word, strip};
use super::{Block, Entry, Field, StringDef, Unreadable, UnreadableKind};

/// A mark: a place where the splitter looks. `{ } " , = \n` (not after a
/// backslash), the start of a block (`@type` followed by `{` or `(`), and in a
/// block opened by `(`, the `)`.
#[derive(Clone, Copy)]
struct Mark {
    start: usize,
    end: usize,
}

/// The parsing of a block was aborted: its text ends at `end_index`.
struct Abort {
    end_index: usize,
}

type Res<T> = Result<T, Abort>;

struct Splitter<'a> {
    s: &'a str,
    /// where the next mark is looked for
    pos: usize,
    /// in a block opened by `(`: `)` is a mark
    paren: bool,
    unaccepted: Option<Mark>,
    current_char_index: usize,
    current_line: i64,
    /// `}` or `)`, whichever closes the block being parsed
    closing: char,
    implicit_start: Option<usize>,
}

/// What the splitter makes of a block before the keys are checked.
enum Piece {
    Entry {
        entry: Entry,
        line: i64,
        raw: String,
    },
    Str {
        string: StringDef,
        line: i64,
        raw: String,
    },
    Other(Block),
}

impl<'a> Splitter<'a> {
    fn text(&self, m: Mark) -> &'a str {
        &self.s[m.start..m.end]
    }

    /// the start of the character before byte `i`
    fn before(&self, i: usize) -> usize {
        self.s[..i].char_indices().next_back().map_or(0, |(j, _)| j)
    }

    /// The next mark of the text (`re.finditer` of the mark patterns).
    fn scan(&mut self) -> Option<Mark> {
        let bytes = self.s.as_bytes();
        let mut i = self.pos;
        while i < bytes.len() {
            let b = bytes[i];
            let single =
                matches!(b, b'{' | b'}' | b'"' | b',' | b'=' | b'\n') || (self.paren && b == b')');
            if single && !(i > 0 && bytes[i - 1] == b'\\') {
                self.pos = i + 1;
                return Some(Mark {
                    start: i,
                    end: i + 1,
                });
            }
            if b == b'@' {
                // `@[\w]*( |\t)*(?=[{(])`
                let mut j = i + 1;
                for c in self.s[j..].chars() {
                    if !is_word(c) {
                        break;
                    }
                    j += c.len_utf8();
                }
                while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] == b'{' || bytes[j] == b'(') {
                    self.pos = j;
                    return Some(Mark { start: i, end: j });
                }
            }
            i += 1;
        }
        self.pos = bytes.len();
        None
    }

    fn next_mark(&mut self, accept_eof: bool) -> Res<Option<Mark>> {
        if let Some(m) = self.unaccepted.take() {
            self.current_char_index = m.start;
            return Ok(Some(m));
        }
        loop {
            match self.scan() {
                None => {
                    self.current_char_index = self.s.len();
                    if !accept_eof {
                        return Err(Abort {
                            end_index: self.current_char_index,
                        });
                    }
                    return Ok(None);
                }
                Some(m) => {
                    self.current_char_index = m.start;
                    if self.text(m) != "\n" {
                        return Ok(Some(m));
                    }
                    self.current_line += 1;
                }
            }
        }
    }

    fn mark(&mut self) -> Res<Mark> {
        Ok(self
            .next_mark(false)?
            .expect("a mark, or an abort at the end of the text"))
    }

    fn reset_block_status(&mut self, current_char_index: usize) {
        self.implicit_start = Some(current_char_index);
    }

    /// Whether position `pos` is at the start of a line (after optional whitespace).
    fn is_at_line_start(&self, pos: usize) -> bool {
        for c in self.s[..pos].chars().rev() {
            if c == '\n' {
                return true;
            }
            if !is_space(c) {
                return false;
            }
        }
        true
    }

    fn end_implicit_comment(&self, end: usize) -> Option<String> {
        let start = self.implicit_start?;
        let comment = if start < end { &self.s[start..end] } else { "" };
        let comment = strip(comment);
        if comment.is_empty() {
            None
        } else {
            Some(comment.to_string())
        }
    }

    fn open_block(&mut self, m: Mark) -> Res<()> {
        if self.s.as_bytes()[m.end] == b'(' {
            self.closing = ')';
            self.paren = true;
            self.pos = m.end + 1;
        } else {
            self.closing = '}';
            self.mark()?;
        }
        Ok(())
    }

    fn close_block(&mut self) {
        if self.closing == ')' {
            let resume = match self.unaccepted {
                Some(m) => m.end,
                None => self.current_char_index + 1,
            };
            self.paren = false;
            self.pos = resume;
            self.closing = '}';
        }
    }

    fn move_to_closing_delimiter(&mut self, track_quotes: bool) -> Res<usize> {
        let mut open = 0usize;
        let mut in_quotes = false;
        let track_quotes = track_quotes && self.closing == ')';
        loop {
            let m = self.mark()?;
            let t = self.text(m);
            if t == "{" {
                open += 1;
            } else if t == "}" && open > 0 {
                open -= 1;
            } else if open == 0 && t == "\"" && track_quotes {
                in_quotes = !in_quotes;
            } else if open == 0 && t.starts_with(self.closing) && t.len() == 1 && !in_quotes {
                return Ok(m.start);
            } else if t.starts_with('@') && self.is_at_line_start(m.start) {
                self.unaccepted = Some(m);
                return Err(Abort {
                    end_index: self.before(m.start),
                });
            }
        }
    }

    /// The end of a field: its `,` or the closing delimiter (put back).
    fn move_to_comma_or_closing_delimiter(&mut self) -> Res<usize> {
        let mut quote_escaped = false;
        let mut open = 0usize;
        loop {
            let m = self.mark()?;
            let t = self.text(m);
            let escaped = quote_escaped || open > 0;
            if t == "\"" && !(open > 0) {
                if quote_escaped {
                    // `{"}` is a literal quote in a quoted value
                    let pos = m.start;
                    let b = self.s.as_bytes();
                    if pos > 0 && pos + 2 < b.len() && b[pos - 1] == b'{' && b[pos + 1] == b'}' {
                        continue;
                    }
                }
                quote_escaped = !quote_escaped;
            } else if t == "{" && !quote_escaped {
                open += 1;
            } else if t == "}" && !quote_escaped && open > 0 {
                open -= 1;
            } else if (t == "," || (t.len() == 1 && t.starts_with(self.closing))) && !escaped {
                // the end of the field: its `,` or the end of the entry
                self.unaccepted = Some(m);
                return Ok(m.start);
            } else if t.starts_with('@') && self.is_at_line_start(m.start) {
                self.unaccepted = Some(m);
                return Err(Abort {
                    end_index: self.before(m.start),
                });
            }
        }
    }

    /// The fields of an entry, from `first_key_start`, and the end of the entry.
    fn move_to_end_of_entry(
        &mut self,
        first_key_start: usize,
    ) -> Res<(Vec<Field>, usize, Vec<String>)> {
        let mut result: Vec<Field> = Vec::new();
        let mut keys: HashSet<String> = HashSet::new();
        let mut duplicates: Vec<String> = Vec::new();
        let mut key_start = first_key_start;
        loop {
            let equals = self.mark()?;
            let et = self.text(equals);
            if et.len() == 1 && et.starts_with(self.closing) {
                let dangling = strip(&self.s[key_start..equals.start]);
                if !dangling.is_empty() {
                    return Err(Abort {
                        end_index: equals.end,
                    });
                }
                return Ok((result, equals.end, duplicates));
            }
            if et != "=" {
                self.unaccepted = Some(equals);
                return Err(Abort {
                    end_index: equals.start,
                });
            }
            let key_end = equals.start;
            let value_start = equals.end;
            let value_end = self.move_to_comma_or_closing_delimiter()?;
            let key = strip(&self.s[key_start..key_end]).to_string();
            let value = strip(&self.s[value_start..value_end]).to_string();
            if !keys.insert(key.clone()) && !duplicates.contains(&key) {
                duplicates.push(key.clone());
            }
            result.push(Field {
                key,
                value,
                verbatim: false,
            });

            let after = self.mark()?;
            let at = self.text(after);
            if at == "," {
                key_start = after.end;
            } else if at.len() == 1 && at.starts_with(self.closing) {
                self.unaccepted = Some(after);
                key_start = after.start;
            } else {
                self.unaccepted = Some(after);
                return Err(Abort {
                    end_index: after.start,
                });
            }
        }
    }

    fn handle_explicit_comment(&mut self, m: Mark) -> Res<Piece> {
        let end = self.move_to_closing_delimiter(false)?;
        Ok(Piece::Other(Block::Comment(
            strip(&self.s[m.end + 1..end]).to_string(),
        )))
    }

    fn handle_preamble(&mut self, m: Mark) -> Res<Piece> {
        let end = self.move_to_closing_delimiter(true)?;
        Ok(Piece::Other(Block::Preamble(
            self.s[m.end + 1..end].to_string(),
        )))
    }

    fn handle_string(&mut self, m: Mark) -> Res<Piece> {
        let line = self.current_line;
        let equals = self.mark()?;
        if self.text(equals) != "=" {
            self.unaccepted = Some(equals);
            return Err(Abort {
                end_index: equals.end,
            });
        }
        let key = strip(&self.s[m.end + 1..equals.start]).to_string();
        let value_start = equals.end;
        let end = self.move_to_closing_delimiter(true)?;
        let value = strip(&self.s[value_start..end]).to_string();
        Ok(Piece::Str {
            string: StringDef {
                key,
                value,
                verbatim: false,
            },
            line,
            raw: self.s[m.start..end + 1].to_string(),
        })
    }

    fn handle_entry(&mut self, m: Mark, m_val: &str) -> Res<Piece> {
        let line = self.current_line;
        let entry_type = strip(&m_val[1..]).to_string();
        let comma = self.mark()?;
        let ct = self.text(comma);
        let (key, fields, end_index, duplicates);
        if ct.len() == 1 && ct.starts_with(self.closing) {
            // an entry without a comma after the key, and without fields
            key = strip(&self.s[m.end + 1..comma.start]).to_string();
            fields = Vec::new();
            end_index = comma.end;
            duplicates = Vec::new();
        } else if ct != "," {
            self.unaccepted = Some(comma);
            return Err(Abort {
                end_index: comma.end,
            });
        } else {
            key = strip(&self.s[m.end + 1..comma.start]).to_string();
            let (f, e, d) = self.move_to_end_of_entry(comma.end)?;
            fields = f;
            end_index = e;
            duplicates = d;
        }
        let raw = self.s[m.start..end_index].to_string();
        if !duplicates.is_empty() {
            let mut keys = duplicates;
            keys.sort();
            return Ok(Piece::Other(Block::Unreadable(Unreadable {
                raw,
                line: line.max(0) as usize,
                kind: UnreadableKind::DuplicateFields(keys),
            })));
        }
        Ok(Piece::Entry {
            entry: Entry {
                entry_type,
                key,
                fields,
            },
            line,
            raw,
        })
    }
}

/// Cut a text into blocks, as bibtexparser's `Splitter.split` does (with the
/// keys of the blocks checked as `Library.add` does: a second block of the
/// same type with a key already there is an unreadable block).
pub(super) fn split(text: &str) -> Vec<Block> {
    // a newline at the beginning simplifies the parsing: "@" starts a block after a newline
    let bibstr = format!("\n{text}");
    let mut sp = Splitter {
        s: &bibstr,
        pos: 0,
        paren: false,
        unaccepted: None,
        current_char_index: 0,
        current_line: -1,
        closing: '}',
        implicit_start: None,
    };
    sp.reset_block_status(0);

    let mut blocks: Vec<Block> = Vec::new();
    let mut entry_keys: HashSet<String> = HashSet::new();
    let mut string_keys: HashSet<String> = HashSet::new();

    while let Ok(Some(m)) = sp.next_mark(true) {
        let m_val = sp.text(m).to_lowercase();
        if !m_val.starts_with('@') {
            continue;
        }
        if let Some(comment) = sp.end_implicit_comment(m.start) {
            blocks.push(Block::Text(comment));
        }
        sp.implicit_start = None;
        let start_line = sp.current_line;
        let handled = sp.open_block(m).and_then(|()| {
            if m_val.starts_with("@comment") {
                sp.handle_explicit_comment(m)
            } else if m_val.starts_with("@preamble") {
                sp.handle_preamble(m)
            } else if m_val.starts_with("@string") {
                sp.handle_string(m)
            } else {
                sp.handle_entry(m, &m_val)
            }
        });
        match handled {
            Ok(Piece::Entry { entry, line, raw }) => {
                if entry_keys.insert(entry.key.clone()) {
                    blocks.push(Block::Entry(entry));
                } else {
                    blocks.push(duplicate(entry.key, raw, line));
                }
            }
            Ok(Piece::Str { string, line, raw }) => {
                if string_keys.insert(string.key.clone()) {
                    blocks.push(Block::String(string));
                } else {
                    blocks.push(duplicate(string.key, raw, line));
                }
            }
            Ok(Piece::Other(block)) => blocks.push(block),
            Err(abort) => {
                let end = abort.end_index.max(m.start);
                blocks.push(Block::Unreadable(Unreadable {
                    raw: sp.s[m.start..end].to_string(),
                    line: start_line.max(0) as usize,
                    kind: UnreadableKind::Syntax,
                }));
            }
        }
        sp.close_block();
        sp.reset_block_status(sp.current_char_index + 1);
    }

    if sp.implicit_start.is_some()
        && let Some(comment) = sp.end_implicit_comment(sp.s.len())
    {
        blocks.push(Block::Text(comment));
    }
    blocks
}

fn duplicate(key: String, raw: String, line: i64) -> Block {
    Block::Unreadable(Unreadable {
        raw,
        line: line.max(0) as usize,
        kind: UnreadableKind::DuplicateKey(key),
    })
}
