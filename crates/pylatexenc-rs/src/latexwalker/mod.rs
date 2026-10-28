//! Parse LaTeX into a tree of nodes (port of `pylatexenc.latexwalker`).
//!
//! ```
//! use pylatexenc::latexwalker::{LatexWalker, NodeKind};
//!
//! let w = LatexWalker::new(r"Hi \emph{there}!");
//! let (nodes, pos, len) = w.get_latex_nodes(0).unwrap();
//! assert_eq!((pos, len), (0, 16));
//! assert_eq!(nodes[1].macroname(), Some("emph"));
//! assert_eq!(nodes[1].latex_verbatim(), r"\emph{there}");
//! assert!(matches!(nodes[2].kind, NodeKind::Chars { .. }));
//! ```

use std::fmt;
use std::sync::{Arc, OnceLock};

mod defaultspecs;
mod json;
mod nodes;
mod source;
mod walker;

pub use json::{node_to_json, nodelist_to_json};
pub use nodes::{LatexNode, NodeKind, NodeType};
pub use source::{ParsingState, ParsingStateChanges, SourceText};
pub use walker::{
    GetNodesOptions, LatexWalker, LatexWalkerOptions, MAX_NESTING_DEPTH, TokenOptions, get_latex_braced_group, get_latex_environment,
    get_latex_expression, get_latex_maybe_optional_arg, get_latex_nodes, get_token,
};

use crate::macrospec::{
    ArgsParser, EnvironmentSpec, LatexContextDb, MacroSpec, MacroStandardArgsParser, SpecialsSpec, VerbatimArgType,
    VerbatimArgsParser, CategoryPosition,
};
use crate::pystr::{repr, strip};

/// `LatexWalkerParseError`: the text cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatexWalkerParseError {
    pub msg: String,
    /// position (in chars) in the text
    pub pos: Option<usize>,
    pub lineno: Option<usize>,
    pub colno: Option<usize>,
    pub input_source: Option<String>,
    /// the open constructs, innermost last: `(what, pos, lineno, colno)`
    pub open_contexts: Vec<(String, usize, usize, usize)>,
}

impl LatexWalkerParseError {
    /// `LatexWalkerParseError(msg, pos=pos)`, without line and column numbers.
    pub fn new(msg: impl Into<String>, pos: Option<usize>) -> Self {
        LatexWalkerParseError {
            msg: msg.into(),
            pos,
            lineno: None,
            colno: None,
            input_source: None,
            open_contexts: Vec::new(),
        }
    }

    fn fmt_pos(pos: Option<usize>, lineno: Option<usize>, colno: Option<usize>) -> String {
        match (lineno, colno) {
            (Some(l), Some(c)) => format!("@({l},{c})"),
            (Some(l), None) => format!("@{l}"),
            _ => format!("@ char {}", pos.map(|p| p.to_string()).unwrap_or_else(|| "None".to_string())),
        }
    }
}

impl fmt::Display for LatexWalkerParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut msg = self.msg.clone();
        if let Some(src) = &self.input_source {
            msg.push_str(&format!("  in {src}"));
        }
        write!(f, "{msg} {}", Self::fmt_pos(self.pos, self.lineno, self.colno))?;
        if !self.open_contexts.is_empty() {
            write!(f, "\nOpen LaTeX blocks:\n")?;
            for (what, pos, lineno, colno) in self.open_contexts.iter().rev() {
                let loc = Self::fmt_pos(Some(*pos), Some(*lineno), Some(*colno));
                writeln!(f, "        {loc:>10}  {what}")?;
            }
        }
        Ok(())
    }
}

/// What can go wrong when walking: Python's `LatexWalkerParseError` and
/// `LatexWalkerEndOfStream`, and the other exceptions (`ValueError`, `IndexError`, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LatexWalkerError {
    /// `LatexWalkerParseError`
    Parse(LatexWalkerParseError),
    /// `LatexWalkerEndOfStream(final_space)`: the end of the text was reached (the whitespace
    /// that was before it is `final_space`)
    EndOfStream { final_space: String },
    /// Another exception of Python's, with its message (e.g. `ValueError: ...`): not handled by
    /// tolerant parsing.
    Other(String),
}

impl From<LatexWalkerParseError> for LatexWalkerError {
    fn from(e: LatexWalkerParseError) -> Self {
        LatexWalkerError::Parse(e)
    }
}

impl fmt::Display for LatexWalkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LatexWalkerError::Parse(e) => e.fmt(f),
            LatexWalkerError::EndOfStream { .. } => Ok(()),
            LatexWalkerError::Other(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for LatexWalkerError {}

/// The kinds of tokens (`LatexToken.tok`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Char,
    Macro,
    Comment,
    BraceOpen,
    BraceClose,
    MathmodeInline,
    MathmodeDisplay,
    BeginEnvironment,
    EndEnvironment,
    Specials,
}

impl TokenKind {
    /// The Python name: `char`, `macro`, `comment`, `brace_open`, `brace_close`,
    /// `mathmode_inline`, `mathmode_display`, `begin_environment`, `end_environment`, `specials`.
    pub fn as_str(self) -> &'static str {
        match self {
            TokenKind::Char => "char",
            TokenKind::Macro => "macro",
            TokenKind::Comment => "comment",
            TokenKind::BraceOpen => "brace_open",
            TokenKind::BraceClose => "brace_close",
            TokenKind::MathmodeInline => "mathmode_inline",
            TokenKind::MathmodeDisplay => "mathmode_display",
            TokenKind::BeginEnvironment => "begin_environment",
            TokenKind::EndEnvironment => "end_environment",
            TokenKind::Specials => "specials",
        }
    }
}

/// `LatexToken`: a token of the text.
///
/// `arg` is the char (or `\n\n`, a paragraph break) of a `char` token, the macro name (without
/// the backslash) of a `macro`, the text of a comment (without the `%`), the brace, the math mode
/// delimiter, the environment name of `begin_environment`/`end_environment`, the `specials_chars`
/// of a `specials` token (whose spec is then in `specials_spec`).
#[derive(Debug, Clone)]
pub struct LatexToken {
    pub tok: TokenKind,
    pub arg: String,
    pub specials_spec: Option<Arc<SpecialsSpec>>,
    pub pos: usize,
    pub len: usize,
    pub pre_space: String,
    /// only for `macro` and `comment` tokens
    pub post_space: String,
}

impl LatexToken {
    pub fn new(tok: TokenKind, arg: impl Into<String>, pos: usize, len: usize, pre_space: impl Into<String>) -> Self {
        LatexToken {
            tok,
            arg: arg.into(),
            specials_spec: None,
            pos,
            len,
            pre_space: pre_space.into(),
            post_space: String::new(),
        }
    }

    pub fn with_post_space(mut self, post_space: impl Into<String>) -> Self {
        self.post_space = post_space.into();
        self
    }
}

impl PartialEq for LatexToken {
    fn eq(&self, other: &Self) -> bool {
        self.tok == other.tok
            && self.arg == other.arg
            && self.pos == other.pos
            && self.len == other.len
            && self.pre_space == other.pre_space
            && (!matches!(self.tok, TokenKind::Macro | TokenKind::Comment) || self.post_space == other.post_space)
    }
}

fn build_parser(argspec: &str, flags: u8, amm: &str) -> Arc<dyn ArgsParser> {
    if flags & 2 != 0 {
        return Arc::new(VerbatimArgsParser::new(VerbatimArgType::VerbMacro));
    }
    if flags & 4 != 0 {
        return Arc::new(VerbatimArgsParser::new(VerbatimArgType::VerbatimEnvironment));
    }
    let mut p = MacroStandardArgsParser::new(argspec).optional_arg_no_space(flags & 1 != 0);
    if !amm.is_empty() {
        p = p.args_math_mode(
            amm.chars()
                .map(|c| match c {
                    'T' => Some(true),
                    'F' => Some(false),
                    _ => None,
                })
                .collect(),
        );
    }
    Arc::new(p)
}

/// `get_default_latex_context_db()`: the default macros, environments and specials, in the
/// categories `latex-base`, `amsmath`, `latex-ethuebung`, ... of pylatexenc. A new database, to
/// modify as needed.
pub fn get_default_latex_context_db() -> LatexContextDb {
    default_latex_context_db().as_ref().clone()
}

/// The default context database, built once and shared (what a [`LatexWalker`] uses when none
/// is given).
pub fn default_latex_context_db() -> Arc<LatexContextDb> {
    static DB: OnceLock<Arc<LatexContextDb>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = LatexContextDb::new();
        for (cat, macros, envs, specials) in defaultspecs::CATEGORIES {
            let macros: Vec<MacroSpec> = macros
                .iter()
                .map(|(name, argspec, flags, amm)| MacroSpec {
                    macroname: name.to_string(),
                    args_parser: build_parser(argspec, *flags, amm),
                })
                .collect();
            let envs: Vec<EnvironmentSpec> = envs
                .iter()
                .map(|(name, argspec, flags, amm, is_math)| EnvironmentSpec {
                    environmentname: name.to_string(),
                    args_parser: build_parser(argspec, *flags, amm),
                    is_math_mode: *is_math,
                })
                .collect();
            let specials: Vec<SpecialsSpec> = specials.iter().map(|s| SpecialsSpec::new(*s)).collect();
            db.add_context_category(cat, macros, envs, specials, CategoryPosition::Append)
                .expect("the default categories are distinct");
        }
        Arc::new(db)
    })
    .clone()
}

/// `nodelist_to_latex(nodelist)`: the LaTeX code of a list of nodes (rebuilt from the nodes, so
/// it is not always the original text).
pub fn nodelist_to_latex(nodelist: &[LatexNode]) -> String {
    let opt: Vec<Option<&LatexNode>> = nodelist.iter().map(Some).collect();
    nodelist_to_latex_opt(&opt)
}

/// Like [`nodelist_to_latex`] for a list that may have `None` entries (such as the `argnlist`
/// of parsed arguments), which are skipped.
pub fn nodelist_to_latex_opt(nodelist: &[Option<&LatexNode>]) -> String {
    fn add_args(nodeargd: Option<&crate::macrospec::ParsedMacroArgs>) -> String {
        let Some(a) = nodeargd else { return String::new() };
        let mut s = String::new();
        for (argt, argn) in a.argspec.chars().zip(a.argnlist.iter()) {
            match argt {
                '*' | '[' | '{' => s.push_str(&nodelist_to_latex_opt(&[argn.as_ref()])),
                _ => {}
            }
        }
        s
    }
    let mut latex = String::new();
    for n in nodelist.iter().flatten() {
        match &n.kind {
            NodeKind::Chars { chars } => latex.push_str(chars),
            NodeKind::Macro { macroname, nodeargd, macro_post_space, .. } => {
                latex.push('\\');
                latex.push_str(macroname);
                latex.push_str(macro_post_space);
                latex.push_str(&add_args(nodeargd.as_ref()));
            }
            NodeKind::Specials { specials_chars, nodeargd } => {
                latex.push_str(specials_chars);
                latex.push_str(&add_args(nodeargd.as_ref()));
            }
            NodeKind::Comment { comment, comment_post_space } => {
                latex.push('%');
                latex.push_str(comment);
                latex.push_str(comment_post_space);
            }
            NodeKind::Group { nodelist, delimiters } | NodeKind::Math { nodelist, delimiters, .. } => {
                latex.push_str(&delimiters.0);
                latex.push_str(&nodelist_to_latex(nodelist));
                latex.push_str(&delimiters.1);
            }
            NodeKind::Environment { environmentname, nodelist, nodeargd } => {
                latex.push_str(&format!("\\begin{{{environmentname}}}"));
                latex.push_str(&add_args(nodeargd.as_ref()));
                latex.push_str(&nodelist_to_latex(nodelist));
                latex.push_str(&format!("\\end{{{environmentname}}}"));
            }
        }
    }
    latex
}

/// `put_in_braces(brace_char, thestring)`
pub fn put_in_braces(brace_char: &str, thestring: &str) -> String {
    match brace_char {
        "{" => format!("{{{thestring}}}"),
        "[" => format!("[{thestring}]"),
        "(" => format!("({thestring})"),
        "<" => format!("<{thestring}>"),
        _ => format!("{brace_char}{thestring}{brace_char}"),
    }
}

/// `disp_node(n, indent, context, skip_group)`: a tree-like description of a node (Python
/// prints it; this returns the text, one line per node, each ending with a newline).
pub fn disp_node(n: Option<&LatexNode>, indent: usize, context: &str) -> String {
    enum Child<'a> {
        Nodes(&'a str, Vec<Option<&'a LatexNode>>),
    }
    let mut out = String::new();
    let mut children: Vec<Child> = Vec::new();
    let title;
    let comment = String::new();

    fn args_children<'a>(n: &'a LatexNode, children: &mut Vec<Child<'a>>) {
        let Some(a) = n.nodeargd() else { return };
        for (argt, argn) in a.argspec.chars().zip(a.argnlist.iter()) {
            let t = match argt {
                '[' => "[.]: ",
                '{' => "{.}: ",
                '*' => "<*>:   ",
                _ => "<unknown>: ",
            };
            children.push(Child::Nodes(t, vec![argn.as_ref()]));
        }
    }

    match n {
        None => title = "<None>".to_string(),
        Some(n) => match &n.kind {
            NodeKind::Chars { chars } => title = repr(chars),
            NodeKind::Macro { macroname, .. } => {
                title = format!("\\{macroname}");
                args_children(n, &mut children);
            }
            NodeKind::Specials { specials_chars, .. } => {
                title = format!("{specials_chars} (specials)");
                args_children(n, &mut children);
            }
            NodeKind::Comment { comment, .. } => title = format!("%{}", strip(comment)),
            NodeKind::Group { nodelist, .. } => {
                title = "Group: ".to_string();
                children.push(Child::Nodes("* ", nodelist.iter().map(Some).collect()));
            }
            NodeKind::Environment { environmentname, nodelist, .. } => {
                title = format!("\\begin{{{environmentname}}}");
                args_children(n, &mut children);
                children.push(Child::Nodes("* ", nodelist.iter().map(Some).collect()));
            }
            NodeKind::Math { displaytype, nodelist, delimiters } => {
                title = format!("{}{} math{}", delimiters.0, displaytype, delimiters.1);
                children.push(Child::Nodes("* ", nodelist.iter().map(Some).collect()));
            }
        },
    }
    out.push_str(&format!("{}{}{}  {}\n", " ".repeat(indent), context, title, comment));
    for c in children {
        match c {
            Child::Nodes(ctx, nl) => {
                for nn in nl {
                    out.push_str(&disp_node(nn, indent + 4, ctx));
                }
            }
        }
    }
    out
}
