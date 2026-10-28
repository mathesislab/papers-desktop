//! The text being walked and the parsing state (`pylatexenc.latexwalker.ParsingState`).

use std::fmt;
use std::sync::Arc;

use crate::macrospec::LatexContextDb;

/// The text of a walker. Python indexes a `str` by code point; so does this crate: every
/// position and length (of tokens, nodes, ...) counts `char`s, not bytes. This keeps the
/// indices equal to pylatexenc's. [`SourceText::byte_offset`] converts to a byte offset.
#[derive(Debug, PartialEq, Eq)]
pub struct SourceText {
    text: String,
    chars: Vec<char>,
}

impl SourceText {
    pub fn new(s: &str) -> Self {
        SourceText { text: s.to_string(), chars: s.chars().collect() }
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The text as chars.
    pub fn chars(&self) -> &[char] {
        &self.chars
    }

    /// `len(s)`: the number of chars.
    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// `s[i]`.
    ///
    /// # Panics
    /// If `i` is out of range.
    pub fn char_at(&self, i: usize) -> char {
        self.chars[i]
    }

    /// `s[i]`, `None` if out of range.
    pub fn get(&self, i: usize) -> Option<char> {
        self.chars.get(i).copied()
    }

    /// `s[start:end]` (clamped like Python's slices).
    pub fn slice_range(&self, start: usize, end: usize) -> String {
        let n = self.chars.len();
        let (a, b) = (start.min(n), end.min(n));
        if a >= b {
            return String::new();
        }
        self.chars[a..b].iter().collect()
    }

    /// `s[pos:pos + len]`
    pub fn slice(&self, pos: usize, len: usize) -> String {
        self.slice_range(pos, pos.saturating_add(len))
    }

    /// `s.startswith(pat, pos)`
    pub fn starts_with(&self, pat: &str, pos: usize) -> bool {
        let mut i = pos;
        for c in pat.chars() {
            match self.chars.get(i) {
                Some(&x) if x == c => i += 1,
                _ => return false,
            }
        }
        true
    }

    /// `s.find(pat, from)` (in chars)
    pub fn find(&self, pat: &str, from: usize) -> Option<usize> {
        let p: Vec<char> = pat.chars().collect();
        if p.is_empty() {
            return (from <= self.chars.len()).then_some(from);
        }
        if p.len() > self.chars.len() {
            return None;
        }
        (from..=self.chars.len() - p.len()).find(|&i| self.chars[i..i + p.len()] == p[..])
    }

    /// `s.find(c, from)` (in chars)
    pub fn find_char(&self, c: char, from: usize) -> Option<usize> {
        self.chars.get(from..)?.iter().position(|&x| x == c).map(|i| i + from)
    }

    /// The byte offset in the text of the char position `pos` (the length of the text if
    /// `pos` is past the end).
    pub fn byte_offset(&self, pos: usize) -> usize {
        self.chars.iter().take(pos).map(|c| c.len_utf8()).sum()
    }
}

/// `ParsingState`: what the walker knows while reading a list of nodes: the text, the latex
/// context (the database of macro specs), and whether it is in math mode.
#[derive(Clone)]
pub struct ParsingState {
    pub s: Arc<SourceText>,
    pub latex_context: Arc<LatexContextDb>,
    pub in_math_mode: bool,
    pub math_mode_delimiter: Option<String>,
}

impl fmt::Debug for ParsingState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParsingState")
            .field("in_math_mode", &self.in_math_mode)
            .field("math_mode_delimiter", &self.math_mode_delimiter)
            .finish_non_exhaustive()
    }
}

/// Fields to change in [`ParsingState::sub_context`] (the keyword arguments of `sub_context()`).
#[derive(Debug, Clone, Default)]
pub struct ParsingStateChanges {
    pub s: Option<Arc<SourceText>>,
    pub latex_context: Option<Arc<LatexContextDb>>,
    pub in_math_mode: Option<bool>,
    /// `Some(None)` sets the delimiter to `None`
    pub math_mode_delimiter: Option<Option<String>>,
}

impl ParsingStateChanges {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn in_math_mode(mut self, v: bool) -> Self {
        self.in_math_mode = Some(v);
        self
    }
    pub fn math_mode_delimiter(mut self, v: Option<&str>) -> Self {
        self.math_mode_delimiter = Some(v.map(str::to_string));
        self
    }
    pub fn latex_context(mut self, v: Arc<LatexContextDb>) -> Self {
        self.latex_context = Some(v);
        self
    }
    pub fn s(mut self, v: Arc<SourceText>) -> Self {
        self.s = Some(v);
        self
    }
}

impl ParsingState {
    /// `ParsingState(s=..., latex_context=...)`, not in math mode.
    pub fn new(s: Arc<SourceText>, latex_context: Arc<LatexContextDb>) -> Self {
        ParsingState { s, latex_context, in_math_mode: false, math_mode_delimiter: None }
    }

    /// `sub_context(**kwargs)`: a copy with some fields changed. If the result is not in math
    /// mode, its `math_mode_delimiter` is `None` (with a warning if one was given).
    pub fn sub_context(&self, changes: &ParsingStateChanges) -> ParsingState {
        let mut p = self.clone();
        if let Some(s) = &changes.s {
            p.s = s.clone();
        }
        if let Some(c) = &changes.latex_context {
            p.latex_context = c.clone();
        }
        if let Some(m) = changes.in_math_mode {
            p.in_math_mode = m;
        }
        if let Some(d) = &changes.math_mode_delimiter {
            p.math_mode_delimiter = d.clone();
        }
        if !p.in_math_mode && p.math_mode_delimiter.is_some() {
            if changes.math_mode_delimiter.is_some() {
                log::warn!(
                    "ParsingState: You set math_mode_delimiter={:?} but in_math_mode is False",
                    p.math_mode_delimiter
                );
            }
            p.math_mode_delimiter = None;
        }
        p
    }
}
