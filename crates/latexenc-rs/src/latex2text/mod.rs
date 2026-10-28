//! Convert LaTeX code to plain text (port of `pylatexenc.latex2text`).
//!
//! ```
//! use latexenc::latex2text::{LatexNodes2Text, MathMode};
//!
//! let l2t = LatexNodes2Text::new();
//! assert_eq!(l2t.latex_to_text(r"\textbf{Caf\'e} $x\geq\alpha$").unwrap(), "Café x≥α");
//!
//! let keep_math = LatexNodes2Text::builder().math_mode(MathMode::Verbatim).build().unwrap();
//! assert_eq!(keep_math.latex_to_text(r#"{M\"u}ller and $\alpha$"#).unwrap(), "Müller and $\\alpha$");
//! ```
//!
//! Like in Python, the *text* specs (what a macro becomes in text) are in a database of their
//! own ([`TextContextDb`], from [`get_default_latex_context_db`]), while the *parsing* of the
//! LaTeX is done by a [`crate::latexwalker::LatexWalker`] with its own context (the macro
//! arguments are known from there): `latex_context` of [`LatexNodes2Text`] only gives the text.

use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

mod defaults;
mod defaultspecs;
mod pyformat;

pub(crate) use defaults::{DISCARD_FALSE, DISCARD_TRUE, DISCARD_UNSET, DefaultFn, DefaultRepl};
pub use defaults::{
    fmt_equation_environment, fmt_input_macro, fmt_math_text_style, fmt_matrix_environment_node,
    fmt_placeholder_node, get_default_latex_context_db, placeholder_node_formatter,
};

use crate::latexwalker::{LatexNode, LatexWalker, LatexWalkerError, LatexWalkerOptions, NodeKind};
use crate::macrospec::{ContextDb, HasEnvironmentName, HasMacroName, HasSpecialsChars};
use crate::pystr::{is_space, strip};
use pyformat::{PercentArgs, PercentError, percent_format};

/// The type of the context database of latex2text: text specs.
pub type TextContextDb = ContextDb<MacroTextSpec, EnvironmentTextSpec, SpecialsTextSpec>;

/// What can fail when converting to text: the walker (if tolerant parsing is off) and the
/// exceptions Python would raise otherwise (a missing key in a `%(2)s` replacement, an
/// `IndexError` in a replacement function, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Latex2TextError {
    /// the walker failed
    Walker(LatexWalkerError),
    /// `KeyError`
    Key(String),
    /// `AttributeError`
    Attribute(String),
    /// `ValueError`
    Value(String),
    /// `IndexError`
    Index(String),
    /// `TypeError`
    Type(String),
}

impl fmt::Display for Latex2TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Latex2TextError::Walker(e) => e.fmt(f),
            Latex2TextError::Key(m) => write!(f, "KeyError: {m}"),
            Latex2TextError::Attribute(m) => write!(f, "AttributeError: {m}"),
            Latex2TextError::Value(m) => write!(f, "ValueError: {m}"),
            Latex2TextError::Index(m) => write!(f, "IndexError: {m}"),
            Latex2TextError::Type(m) => write!(f, "TypeError: {m}"),
        }
    }
}

impl std::error::Error for Latex2TextError {}

impl From<LatexWalkerError> for Latex2TextError {
    fn from(e: LatexWalkerError) -> Self {
        Latex2TextError::Walker(e)
    }
}

type Res<T> = Result<T, Latex2TextError>;

/// The type of a replacement function (a callable `simplify_repl`): it gets the node and the
/// converter, and returns the text. (Python's callables may take `l2tobj`, `macroname`, ...
/// by name; here they always get the node, which has those.)
pub type TextFn = dyn Fn(&LatexNode, &LatexNodes2Text) -> Res<String> + Send + Sync;

/// A `simplify_repl`: a replacement string or a function.
///
/// A string is used as is, except if it contains `%`: then it is a Python `%`-format string
/// that gets the converted arguments of the macro: `%s` the arguments in order, or `%(1)s`,
/// `%(2)s`, ... by number (for an environment, `%s` is the body, `%(body)s` too). An empty
/// string is as no replacement at all (as in Python).
#[derive(Clone)]
pub enum Repl {
    Str(String),
    Func(Arc<TextFn>),
}

impl Repl {
    /// A replacement function (Python: a callable).
    pub fn func<F>(f: F) -> Repl
    where
        F: Fn(&LatexNode, &LatexNodes2Text) -> String + Send + Sync + 'static,
    {
        Repl::Func(Arc::new(move |n, l| Ok(f(n, l))))
    }

    /// A replacement function that can fail.
    pub fn try_func<F>(f: F) -> Repl
    where
        F: Fn(&LatexNode, &LatexNodes2Text) -> Res<String> + Send + Sync + 'static,
    {
        Repl::Func(Arc::new(f))
    }
}

impl fmt::Debug for Repl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Repl::Str(s) => write!(f, "{s:?}"),
            Repl::Func(_) => f.write_str("<function>"),
        }
    }
}

impl From<&str> for Repl {
    fn from(s: &str) -> Self {
        Repl::Str(s.to_string())
    }
}

impl From<String> for Repl {
    fn from(s: String) -> Self {
        Repl::Str(s)
    }
}

impl Repl {
    /// Python truthiness: a non-empty string or any function
    fn is_set(&self) -> bool {
        match self {
            Repl::Str(s) => !s.is_empty(),
            Repl::Func(_) => true,
        }
    }
}

/// `MacroTextSpec(macroname, simplify_repl, discard)`: what a macro becomes in text.
///
/// If `simplify_repl` is set, it gives the text; else the macro is discarded (if `discard`), or
/// replaced by the text of its arguments.
#[derive(Debug, Clone)]
pub struct MacroTextSpec {
    pub macroname: String,
    pub simplify_repl: Option<Repl>,
    pub discard: bool,
}

impl MacroTextSpec {
    /// `MacroTextSpec(macroname)`: discarded.
    pub fn new(macroname: impl Into<String>) -> Self {
        MacroTextSpec { macroname: macroname.into(), simplify_repl: None, discard: true }
    }

    /// `MacroTextSpec(macroname, simplify_repl=...)`
    pub fn with_repl(macroname: impl Into<String>, simplify_repl: impl Into<Repl>) -> Self {
        MacroTextSpec { macroname: macroname.into(), simplify_repl: Some(simplify_repl.into()), discard: true }
    }

    /// Set `discard`.
    pub fn discard(mut self, discard: bool) -> Self {
        self.discard = discard;
        self
    }
}

impl HasMacroName for MacroTextSpec {
    fn macroname(&self) -> &str {
        &self.macroname
    }
}

/// `EnvironmentTextSpec(environmentname, simplify_repl, discard)`
#[derive(Debug, Clone)]
pub struct EnvironmentTextSpec {
    pub environmentname: String,
    pub simplify_repl: Option<Repl>,
    pub discard: bool,
}

impl EnvironmentTextSpec {
    /// `EnvironmentTextSpec(environmentname)`: the text of its body.
    pub fn new(environmentname: impl Into<String>) -> Self {
        EnvironmentTextSpec { environmentname: environmentname.into(), simplify_repl: None, discard: false }
    }

    /// `EnvironmentTextSpec(environmentname, simplify_repl=...)`
    pub fn with_repl(environmentname: impl Into<String>, simplify_repl: impl Into<Repl>) -> Self {
        EnvironmentTextSpec {
            environmentname: environmentname.into(),
            simplify_repl: Some(simplify_repl.into()),
            discard: false,
        }
    }

    /// Set `discard`.
    pub fn discard(mut self, discard: bool) -> Self {
        self.discard = discard;
        self
    }
}

impl HasEnvironmentName for EnvironmentTextSpec {
    fn environmentname(&self) -> &str {
        &self.environmentname
    }
}

/// `SpecialsTextSpec(specials_chars, simplify_repl)`. (As in Python, a spec without a
/// replacement is an error when used: there is no `discard` on specials.)
#[derive(Debug, Clone)]
pub struct SpecialsTextSpec {
    pub specials_chars: String,
    pub simplify_repl: Option<Repl>,
}

impl SpecialsTextSpec {
    pub fn new(specials_chars: impl Into<String>) -> Self {
        SpecialsTextSpec { specials_chars: specials_chars.into(), simplify_repl: None }
    }

    /// `SpecialsTextSpec(specials_chars, simplify_repl)`
    pub fn with_repl(specials_chars: impl Into<String>, simplify_repl: impl Into<Repl>) -> Self {
        SpecialsTextSpec { specials_chars: specials_chars.into(), simplify_repl: Some(simplify_repl.into()) }
    }
}

impl HasSpecialsChars for SpecialsTextSpec {
    fn specials_chars(&self) -> &str {
        &self.specials_chars
    }
}

/// How the math in the text is converted (`math_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MathMode {
    /// `'text'`: converted to text like the rest (the default)
    #[default]
    Text,
    /// `'with-delimiters'`: converted, with the math delimiters around it
    WithDelimiters,
    /// `'verbatim'`: left as it is in the LaTeX code
    Verbatim,
    /// `'remove'`: removed
    Remove,
}

impl std::str::FromStr for MathMode {
    type Err = Latex2TextError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "text" => Ok(MathMode::Text),
            "with-delimiters" => Ok(MathMode::WithDelimiters),
            "verbatim" => Ok(MathMode::Verbatim),
            "remove" => Ok(MathMode::Remove),
            _ => Err(Latex2TextError::Value(
                "math_mode= option must be one of 'text', 'with-delimiters', 'verbatim', 'remove'".to_string(),
            )),
        }
    }
}

/// `strict_latex_spaces`: whether the spaces of the LaTeX source are kept as they are.
///
/// Gives the presets and dictionary forms of the Python option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrictLatexSpaces {
    /// `None`: all `False`, no change in equations
    None,
    /// `False`: the preset `macros` (the default); `True`: everything strict
    Bool(bool),
    /// a preset by name: `on`, `off`, `based-on-source`, `macros`, `except-in-equations`
    Preset(String),
    /// the dictionary form
    Dict(StrictLatexSpacesDict),
}

impl StrictLatexSpaces {
    pub fn preset(name: &str) -> Self {
        StrictLatexSpaces::Preset(name.to_string())
    }
}

/// The dictionary form of `strict_latex_spaces`, with the keys `between-macro-and-chars`,
/// `between-latex-constructs`, `after-comment` and `in-equations`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StrictLatexSpacesDict {
    pub between_macro_and_chars: bool,
    pub between_latex_constructs: bool,
    pub after_comment: bool,
    /// the setting to use inside equations (`None`: the same as outside)
    pub in_equations: Option<Box<StrictLatexSpaces>>,
}

impl StrictLatexSpacesDict {
    /// All `False`, `in_equations` `None`.
    pub fn new() -> Self {
        Self::default()
    }
    pub fn between_macro_and_chars(mut self, v: bool) -> Self {
        self.between_macro_and_chars = v;
        self
    }
    pub fn between_latex_constructs(mut self, v: bool) -> Self {
        self.between_latex_constructs = v;
        self
    }
    pub fn after_comment(mut self, v: bool) -> Self {
        self.after_comment = v;
        self
    }
    pub fn in_equations(mut self, v: StrictLatexSpaces) -> Self {
        self.in_equations = Some(Box::new(v));
        self
    }
}

/// `_parse_strict_latex_spaces_dict()`
fn parse_strict_latex_spaces_dict(s: &StrictLatexSpaces) -> Res<StrictLatexSpacesDict> {
    use StrictLatexSpaces as S;
    let d = |a: bool, b: bool, c: bool, e: Option<S>| StrictLatexSpacesDict {
        between_macro_and_chars: a,
        between_latex_constructs: b,
        after_comment: c,
        in_equations: e.map(Box::new),
    };
    match s {
        S::None => Ok(StrictLatexSpacesDict::default()),
        S::Bool(false) => Ok(d(true, true, false, Some(S::preset("based-on-source")))),
        S::Bool(true) => Ok(d(true, true, true, Some(S::Bool(true)))),
        S::Dict(dict) => Ok(dict.clone()),
        S::Preset(name) => match name.as_str() {
            "on" => parse_strict_latex_spaces_dict(&S::Bool(true)),
            "off" => parse_strict_latex_spaces_dict(&S::Bool(false)),
            "based-on-source" => Ok(d(false, false, false, None)),
            "macros" => Ok(d(true, true, false, Some(S::preset("based-on-source")))),
            "except-in-equations" => Ok(d(true, true, true, Some(S::preset("based-on-source")))),
            // 'default' is deprecated, but is not found by the lookup before the deprecation
            // branch (pylatexenc 2.11): an error like any other unknown preset
            other => Err(Latex2TextError::Value(format!("invalid value for strict_latex_spaces preset: {other}"))),
        },
    }
}

/// The state that the `\title`, `\author`, `\date` replacements keep for `\maketitle`.
#[derive(Debug, Clone, Default)]
pub(crate) struct DocState {
    pub title: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
}

/// `LatexNodes2Text`: converts LaTeX (or a list of nodes) to text.
///
/// Build one with [`LatexNodes2Text::new`] (the defaults) or [`LatexNodes2Text::builder`] (the
/// options of `LatexNodes2Text(latex_context=..., **flags)`).
pub struct LatexNodes2Text {
    /// the text specs
    pub latex_context: Arc<TextContextDb>,
    tex_input_directory: Option<String>,
    latex_walker_init_args: LatexWalkerOptions,
    strict_input: bool,
    /// how math is converted
    pub math_mode: MathMode,
    /// keep the comments (`%...`) in the text
    pub keep_comments: bool,
    strict_latex_spaces: Mutex<StrictLatexSpacesDict>,
    /// keep the braces of the groups `{...}` (if their text has at least `keep_braced_groups_minlen` chars)
    pub keep_braced_groups: bool,
    /// see `keep_braced_groups` (default 2)
    pub keep_braced_groups_minlen: usize,
    /// fill the text to this width (`fill_text=True` is 80)
    pub fill_text: Option<usize>,
    pub(crate) doc_state: Mutex<DocState>,
}

impl fmt::Debug for LatexNodes2Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LatexNodes2Text")
            .field("math_mode", &self.math_mode)
            .field("keep_comments", &self.keep_comments)
            .field("keep_braced_groups", &self.keep_braced_groups)
            .field("keep_braced_groups_minlen", &self.keep_braced_groups_minlen)
            .field("fill_text", &self.fill_text)
            .finish_non_exhaustive()
    }
}

impl Clone for LatexNodes2Text {
    fn clone(&self) -> Self {
        LatexNodes2Text {
            latex_context: self.latex_context.clone(),
            tex_input_directory: self.tex_input_directory.clone(),
            latex_walker_init_args: self.latex_walker_init_args.clone(),
            strict_input: self.strict_input,
            math_mode: self.math_mode,
            keep_comments: self.keep_comments,
            strict_latex_spaces: Mutex::new(self.strict_latex_spaces.lock().unwrap().clone()),
            keep_braced_groups: self.keep_braced_groups,
            keep_braced_groups_minlen: self.keep_braced_groups_minlen,
            fill_text: self.fill_text,
            doc_state: Mutex::new(self.doc_state.lock().unwrap().clone()),
        }
    }
}

impl Default for LatexNodes2Text {
    fn default() -> Self {
        Self::new()
    }
}

/// The options of [`LatexNodes2Text`], as the arguments of `LatexNodes2Text(latex_context=None,
/// **flags)`; get it from [`LatexNodes2Text::builder`].
#[derive(Debug, Clone)]
pub struct LatexNodes2TextBuilder {
    latex_context: Option<Arc<TextContextDb>>,
    math_mode: MathMode,
    keep_comments: bool,
    strict_latex_spaces: StrictLatexSpaces,
    keep_braced_groups: bool,
    keep_braced_groups_minlen: usize,
    fill_text: Option<usize>,
}

impl Default for LatexNodes2TextBuilder {
    fn default() -> Self {
        LatexNodes2TextBuilder {
            latex_context: None,
            math_mode: MathMode::Text,
            keep_comments: false,
            strict_latex_spaces: StrictLatexSpaces::Bool(false),
            keep_braced_groups: false,
            keep_braced_groups_minlen: 2,
            fill_text: None,
        }
    }
}

impl LatexNodes2TextBuilder {
    /// `latex_context`: the text specs (default: [`get_default_latex_context_db`]).
    pub fn latex_context(mut self, db: impl Into<Arc<TextContextDb>>) -> Self {
        self.latex_context = Some(db.into());
        self
    }
    /// `math_mode`
    pub fn math_mode(mut self, m: MathMode) -> Self {
        self.math_mode = m;
        self
    }
    /// `keep_comments`
    pub fn keep_comments(mut self, v: bool) -> Self {
        self.keep_comments = v;
        self
    }
    /// `strict_latex_spaces`
    pub fn strict_latex_spaces(mut self, v: StrictLatexSpaces) -> Self {
        self.strict_latex_spaces = v;
        self
    }
    /// `keep_braced_groups`
    pub fn keep_braced_groups(mut self, v: bool) -> Self {
        self.keep_braced_groups = v;
        self
    }
    /// `keep_braced_groups_minlen`
    pub fn keep_braced_groups_minlen(mut self, v: usize) -> Self {
        self.keep_braced_groups_minlen = v;
        self
    }
    /// `fill_text`: the width to fill to (`None` or 0: no filling; `fill_text=True` of Python
    /// is `Some(80)`)
    pub fn fill_text(mut self, width: Option<usize>) -> Self {
        self.fill_text = width.filter(|&w| w > 0);
        self
    }
    /// Make the converter. Fails if `strict_latex_spaces` is not valid.
    pub fn build(self) -> Res<LatexNodes2Text> {
        let strict = parse_strict_latex_spaces_dict(&self.strict_latex_spaces)?;
        Ok(LatexNodes2Text {
            latex_context: self.latex_context.unwrap_or_else(defaults::default_text_context_db),
            tex_input_directory: None,
            latex_walker_init_args: LatexWalkerOptions::default(),
            strict_input: true,
            math_mode: self.math_mode,
            keep_comments: self.keep_comments,
            strict_latex_spaces: Mutex::new(strict),
            keep_braced_groups: self.keep_braced_groups,
            keep_braced_groups_minlen: self.keep_braced_groups_minlen,
            fill_text: self.fill_text,
            doc_state: Mutex::new(DocState::default()),
        })
    }
}

impl LatexNodes2Text {
    /// `LatexNodes2Text()`: the default specs and options.
    pub fn new() -> Self {
        LatexNodes2TextBuilder::default().build().expect("the defaults are valid")
    }

    /// The options, to set as for `LatexNodes2Text(...)`.
    pub fn builder() -> LatexNodes2TextBuilder {
        LatexNodes2TextBuilder::default()
    }

    /// The current `strict_latex_spaces` (resolved to its dictionary form).
    pub fn strict_latex_spaces(&self) -> StrictLatexSpacesDict {
        self.strict_latex_spaces.lock().unwrap().clone()
    }

    /// `set_tex_input_directory(tex_input_directory, latex_walker_init_args, strict_input)`:
    /// the directory from which `\input{...}` and `\include{...}` read files (without it they
    /// give nothing). With `strict_input` (the default in Python), files outside the directory
    /// are not read. `latex_walker_init_args` are the options for the walker that parses the
    /// files.
    pub fn set_tex_input_directory(
        &mut self,
        tex_input_directory: &str,
        latex_walker_init_args: Option<LatexWalkerOptions>,
        strict_input: bool,
    ) {
        self.tex_input_directory = Some(tex_input_directory.to_string());
        self.latex_walker_init_args = latex_walker_init_args.unwrap_or_default();
        self.strict_input = strict_input;
    }

    /// `read_input_file(fn)`: the contents of the file for `\input{fn}` (`.tex` and `.latex` are
    /// tried if the name does not exist), or an empty string if it cannot be read.
    pub fn read_input_file(&self, fn_: &str) -> String {
        let Some(dir) = &self.tex_input_directory else { return String::new() };
        let joined = std::path::Path::new(dir).join(fn_);
        let mut fnfull = defaults::realpath(&joined);
        if self.strict_input {
            let dirfull = defaults::realpath(std::path::Path::new(dir));
            if !fnfull.to_string_lossy().starts_with(dirfull.to_string_lossy().as_ref()) {
                log::warn!("Can't access path '{fn_}' leading outside of mandated directory [strict input mode]");
                return String::new();
            }
        }
        for ext in ["tex", "latex"] {
            if !fnfull.exists() {
                let mut with_ext = fnfull.clone().into_os_string();
                with_ext.push(format!(".{ext}"));
                if std::path::Path::new(&with_ext).exists() {
                    fnfull = with_ext.into();
                }
            }
        }
        if !fnfull.is_file() {
            log::warn!("Error, file doesn't exist: '{fn_}'");
            return String::new();
        }
        log::debug!("Reading input file {fnfull:?}");
        match std::fs::read(&fnfull) {
            Ok(bytes) => match String::from_utf8(bytes) {
                // universal newlines, as Python's text mode
                Ok(s) => s.replace("\r\n", "\n").replace('\r', "\n"),
                Err(e) => {
                    log::warn!("Error, can't access '{fn_}': {e}");
                    String::new()
                }
            },
            Err(e) => {
                log::warn!("Error, can't access '{fn_}': {e}");
                String::new()
            }
        }
    }

    pub(crate) fn input_node_simplify_repl(&self, n: &LatexNode) -> Res<String> {
        let nodeargs = n
            .nodeargs()
            .ok_or_else(|| Latex2TextError::Type("object of type 'NoneType' has no len()".to_string()))?;
        if nodeargs.len() != 1 {
            log::warn!("Expected exactly one argument for '\\input' ! Got = {nodeargs:?}");
        }
        let first = nodeargs
            .first()
            .ok_or_else(|| Latex2TextError::Index("list index out of range".to_string()))?;
        let name = self.nodelist_to_text_iter(std::iter::once(first.as_ref()))?;
        let inputtex = self.read_input_file(strip(&name));
        if inputtex.is_empty() {
            return Ok(String::new());
        }
        let (nodes, _, _) = LatexWalker::with_options(&inputtex, self.latex_walker_init_args.clone()).get_latex_nodes(0)?;
        self.nodelist_to_text(&nodes)
    }

    /// `latex_to_text(latex)`: the text of the LaTeX code. The code is parsed by a walker with
    /// the default options (tolerant parsing, default context).
    pub fn latex_to_text(&self, latex: &str) -> Res<String> {
        self.latex_to_text_with(latex, LatexWalkerOptions::default())
    }

    /// `latex_to_text(latex, **parse_flags)`: with the given options for the walker.
    pub fn latex_to_text_with(&self, latex: &str, parse_flags: LatexWalkerOptions) -> Res<String> {
        let (nodes, _, _) = LatexWalker::with_options(latex, parse_flags).get_latex_nodes(0)?;
        self.nodelist_to_text(&nodes)
    }

    /// `nodelist_to_text(nodelist)`
    pub fn nodelist_to_text(&self, nodelist: &[LatexNode]) -> Res<String> {
        self.nodelist_to_text_iter(nodelist.iter().map(Some))
    }

    /// `nodelist_to_text(nodelist)` for a list with absent entries, like the `argnlist` of the
    /// arguments of a macro (an absent entry gives nothing).
    pub fn nodelist_to_text_opt(&self, nodelist: &[Option<LatexNode>]) -> Res<String> {
        self.nodelist_to_text_iter(nodelist.iter().map(|n| n.as_ref()))
    }

    fn nodelist_to_text_iter<'a>(&self, nodes: impl IntoIterator<Item = Option<&'a LatexNode>>) -> Res<String> {
        // the recursion (groups in groups...) grows the stack as needed
        stacker::maybe_grow(192 * 1024, 2 * 1024 * 1024, || self.nodelist_to_text_iter_inner(nodes))
    }

    fn nodelist_to_text_iter_inner<'a>(&self, nodes: impl IntoIterator<Item = Option<&'a LatexNode>>) -> Res<String> {
        let mut s = String::new();
        let mut prev_node: Option<&LatexNode> = None;
        for node in nodes {
            if self.is_bare_macro_node(prev_node)?
                && node.is_some_and(LatexNode::is_chars)
                && !self.strict_latex_spaces.lock().unwrap().between_macro_and_chars
            {
                s.push_str(prev_node.and_then(LatexNode::macro_post_space).unwrap_or(""));
            }
            let textcol = match s.rfind('\n') {
                Some(k) => s[k + 1..].chars().count(),
                None => s.chars().count(),
            };
            s.push_str(&self.node_to_text_col(node, textcol)?);
            prev_node = node;
        }
        Ok(s)
    }

    /// `node_to_text(node)`
    pub fn node_to_text(&self, node: Option<&LatexNode>) -> Res<String> {
        self.node_to_text_col(node, 0)
    }

    /// `node_to_text(node, textcol=...)`: `textcol` is the column the text starts at (for
    /// `fill_text`).
    pub fn node_to_text_col(&self, node: Option<&LatexNode>, textcol: usize) -> Res<String> {
        let Some(node) = node else { return Ok(String::new()) };
        match &node.kind {
            NodeKind::Chars { .. } => Ok(self.chars_node_to_text(node, textcol)),
            NodeKind::Comment { .. } => Ok(self.comment_node_to_text(node)),
            NodeKind::Group { .. } => self.group_node_to_text(node),
            NodeKind::Macro { .. } => self.macro_node_to_text(node),
            NodeKind::Environment { .. } => self.environment_node_to_text(node),
            NodeKind::Specials { .. } => self.specials_node_to_text(node),
            NodeKind::Math { .. } => self.math_node_to_text(node),
        }
    }

    /// `chars_node_to_text(node, textcol)`
    pub fn chars_node_to_text(&self, node: &LatexNode, textcol: usize) -> String {
        let mut content = node.chars().unwrap_or("").to_string();
        if let Some(w) = self.fill_text {
            content = self.do_fill_text_width(&content, textcol, w);
        }
        if !self.strict_latex_spaces.lock().unwrap().between_latex_constructs && strip(&content).is_empty() {
            return String::new();
        }
        content
    }

    /// `comment_node_to_text(node)`
    pub fn comment_node_to_text(&self, node: &LatexNode) -> String {
        let comment = node.comment().unwrap_or("");
        let post = node.comment_post_space().unwrap_or("");
        let after_comment = self.strict_latex_spaces.lock().unwrap().after_comment;
        if self.keep_comments {
            if after_comment {
                let nl = if post.is_empty() { "" } else { "\n" };
                format!("%{comment}{nl}")
            } else {
                format!("%{comment}{post}")
            }
        } else if after_comment {
            String::new()
        } else {
            post.to_string()
        }
    }

    /// `group_node_to_text(node)`
    pub fn group_node_to_text(&self, node: &LatexNode) -> Res<String> {
        let contents = self.groupnodecontents_to_text(Some(node))?;
        if self.keep_braced_groups && contents.chars().count() >= self.keep_braced_groups_minlen {
            let (o, c) = node.delimiters().unwrap_or(("{", "}"));
            return Ok(format!("{o}{contents}{c}"));
        }
        Ok(contents)
    }

    /// `macro_node_to_text(node)`
    pub fn macro_node_to_text(&self, node: &LatexNode) -> Res<String> {
        let macroname = node.macroname().unwrap_or("");
        let default_mac = MacroTextSpec::new("");
        let mac: &MacroTextSpec = match self.latex_context.get_macro_spec(macroname) {
            Some(m) => m,
            None => &default_mac,
        };
        if let Some(repl) = mac.simplify_repl.as_ref().filter(|r| r.is_set()) {
            return self.apply_simplify_repl(node, repl, &format!("macro '\\{macroname}'"));
        }
        if mac.discard {
            return Ok(String::new());
        }
        let mut s = String::new();
        if let Some(a) = node.nodeargd() {
            for n in &a.argnlist {
                s.push_str(&self.groupnodecontents_to_text(n.as_ref())?);
            }
        }
        Ok(s)
    }

    /// `environment_node_to_text(node)`
    pub fn environment_node_to_text(&self, node: &LatexNode) -> Res<String> {
        let environmentname = node.environmentname().unwrap_or("");
        let default_env = EnvironmentTextSpec::new("");
        let envdef: &EnvironmentTextSpec = match self.latex_context.get_environment_spec(environmentname) {
            Some(e) => e,
            None => &default_env,
        };
        if let Some(repl) = envdef.simplify_repl.as_ref().filter(|r| r.is_set()) {
            return self.apply_simplify_repl(node, repl, &format!("environment '{environmentname}'"));
        }
        if envdef.discard {
            return Ok(String::new());
        }
        self.nodelist_to_text(node.nodelist().unwrap_or(&[]))
    }

    /// `specials_node_to_text(node)`
    pub fn specials_node_to_text(&self, node: &LatexNode) -> Res<String> {
        let specials_chars = node.specials_chars().unwrap_or("");
        let Some(spec) = self.latex_context.get_specials_spec(specials_chars) else {
            return Ok(specials_chars.to_string());
        };
        match spec.simplify_repl.as_ref().filter(|r| r.is_set()) {
            Some(repl) => self.apply_simplify_repl(node, repl, &format!("specials '{specials_chars}'")),
            None => Err(Latex2TextError::Attribute("'SpecialsTextSpec' object has no attribute 'discard'".to_string())),
        }
    }

    /// `math_node_to_text(node)`: for a math node, or an environment that is math (as the
    /// `equation` replacement does).
    pub fn math_node_to_text(&self, node: &LatexNode) -> Res<String> {
        let is_env = node.is_environment();
        let is_display = is_env || node.displaytype() == Some("display");
        match self.math_mode {
            MathMode::Verbatim => {
                if is_display {
                    Ok(self.fmt_indented_block(&node.latex_verbatim(), ""))
                } else {
                    Ok(node.latex_verbatim())
                }
            }
            MathMode::Remove => Ok(String::new()),
            MathMode::WithDelimiters => {
                let content = self.with_equation_context(|| {
                    Ok(strip(&self.nodelist_to_text(node.nodelist().unwrap_or(&[]))?).to_string())
                })?;
                let (open, close) = match node.delimiters() {
                    Some((o, c)) if !is_env => (o.to_string(), c.to_string()),
                    _ => {
                        let n = node.environmentname().unwrap_or("");
                        (format!("\\begin{{{n}}}"), format!("\\end{{{n}}}"))
                    }
                };
                if is_display {
                    Ok(format!("{open}{}{close}", self.fmt_indented_block(&content, "")))
                } else {
                    Ok(format!("{open}{content}{close}"))
                }
            }
            MathMode::Text => {
                let content = self.with_equation_context(|| {
                    Ok(strip(&self.nodelist_to_text(node.nodelist().unwrap_or(&[]))?).to_string())
                })?;
                if is_display { Ok(self.fmt_indented_block(&content, "    ")) } else { Ok(content) }
            }
        }
    }

    /// `_PushEquationContext`: inside an equation, `strict_latex_spaces` is what `in-equations`
    /// says.
    fn with_equation_context<T>(&self, f: impl FnOnce() -> Res<T>) -> Res<T> {
        let in_eq = self.strict_latex_spaces.lock().unwrap().in_equations.clone();
        let Some(ie) = in_eq else { return f() };
        let new = parse_strict_latex_spaces_dict(&ie)?;
        let old = std::mem::replace(&mut *self.strict_latex_spaces.lock().unwrap(), new);
        let r = f();
        *self.strict_latex_spaces.lock().unwrap() = old;
        r
    }

    /// `do_fill_text(text, textcol)`: fill the text to `fill_text` columns, the first line
    /// starting at column `textcol`. (`fill_text` must be set.)
    pub fn do_fill_text(&self, text: &str, textcol: usize) -> Res<String> {
        let width = self
            .fill_text
            .ok_or_else(|| Latex2TextError::Type("unsupported operand type(s) for -: 'NoneType' and 'int'".to_string()))?;
        Ok(self.do_fill_text_width(text, textcol, width))
    }

    fn do_fill_text_width(&self, text: &str, textcol: usize, width: usize) -> String {
        let head_ws: String = text.chars().take_while(|&c| is_space(c)).collect();
        let head_par = if head_ws.contains("\n\n") { "\n\n" } else { "" };
        let trail_rev: String = text.chars().rev().take_while(|&c| is_space(c)).collect();
        let trail_ws: String = trail_rev.chars().rev().collect();
        let trail_par = if trail_ws.contains("\n\n") { "\n\n" } else { "" };
        let text = strip(text);

        let fill_chunk = |x: &str, textcol: usize| -> String {
            let x = strip(x);
            if textcol as isize >= width as isize - 4 {
                let filled = crate::textwrap::fill(x, width, "").unwrap_or_default();
                format!("\n{filled}")
            } else {
                let filled = crate::textwrap::fill(x, width, &"X".repeat(textcol)).unwrap_or_default();
                filled.chars().skip(textcol).collect()
            }
        };

        // re.split(r'\n{2,}', text)
        let mut rawchunks: Vec<String> = Vec::new();
        let mut cur = String::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\n' {
                let mut j = i;
                while j < chars.len() && chars[j] == '\n' {
                    j += 1;
                }
                if j - i >= 2 {
                    rawchunks.push(std::mem::take(&mut cur));
                    i = j;
                    continue;
                }
            }
            cur.push(chars[i]);
            i += 1;
        }
        rawchunks.push(cur);

        let chunks: Vec<String> = rawchunks
            .iter()
            .enumerate()
            .map(|(j, x)| fill_chunk(x, if j == 0 { textcol } else { 0 }))
            .filter(|c| !strip(c).is_empty())
            .collect();
        let mut r = String::from(head_par);
        if textcol > 0 && !head_ws.is_empty() && head_par.is_empty() {
            r.push(' ');
        }
        r.push_str(&chunks.join("\n\n"));
        if !trail_ws.is_empty() && trail_par.is_empty() {
            r.push(' ');
        }
        r.push_str(trail_par);
        r
    }

    /// `apply_simplify_repl(node, simplify_repl, what)`
    pub fn apply_simplify_repl(&self, node: &LatexNode, simplify_repl: &Repl, what: &str) -> Res<String> {
        let repl = match simplify_repl {
            Repl::Func(f) => return f(node, self),
            Repl::Str(s) => s,
        };
        if repl.contains('%') && repl.chars().count() != 1 {
            let nodeargs: &[Option<LatexNode>] = node.nodeargd().map(|a| a.argnlist.as_slice()).unwrap_or(&[]);
            let has_percent_s = has_percent_s(repl);
            let arg_texts = |this: &Self| -> Res<Vec<String>> {
                nodeargs.iter().map(|n| this.groupnodecontents_to_text(n.as_ref())).collect()
            };
            let numbered = |texts: Vec<String>| -> Vec<(String, String)> {
                texts.into_iter().enumerate().map(|(j, t)| ((1 + j).to_string(), t)).collect()
            };
            let args = if node.is_environment() {
                if has_percent_s {
                    PercentArgs::Tuple(vec![self.nodelist_to_text(node.nodelist().unwrap_or(&[]))?])
                } else {
                    let mut d = numbered(arg_texts(self)?);
                    let body = self.nodelist_to_text(node.nodelist().unwrap_or(&[]))?;
                    match d.iter_mut().find(|(k, _)| k == "body") {
                        Some(slot) => slot.1 = body,
                        None => d.push(("body".to_string(), body)),
                    }
                    PercentArgs::Dict(d)
                }
            } else if has_percent_s {
                PercentArgs::Tuple(arg_texts(self)?)
            } else {
                PercentArgs::Dict(numbered(arg_texts(self)?))
            };
            return match percent_format(repl, &args) {
                Ok(s) => Ok(s),
                Err(PercentError::Key(k)) => Err(Latex2TextError::Key(k)),
                Err(PercentError::Type(_) | PercentError::Value(_)) => {
                    log::warn!("WARNING: Error in configuration: {what} failed its substitution!");
                    Ok(repl.clone())
                }
            };
        }
        Ok(repl.clone())
    }

    /// `_fmt_indented_block(contents, indent)`
    pub(crate) fn fmt_indented_block(&self, contents: &str, indent: &str) -> String {
        let block = format!("\n{indent}{}\n", contents.replace('\n', &format!("\n{indent}")));
        if self.fill_text.is_some() { format!("\n{block}\n") } else { block }
    }

    fn is_bare_macro_node(&self, node: Option<&LatexNode>) -> Res<bool> {
        let Some(n) = node else { return Ok(false) };
        if !n.is_macro() {
            return Ok(false);
        }
        if n.nodeoptarg().is_some() {
            return Ok(false);
        }
        match n.nodeargs() {
            None => Err(Latex2TextError::Type("object of type 'NoneType' has no len()".to_string())),
            Some(a) => Ok(a.is_empty()),
        }
    }

    /// `_groupnodecontents_to_text(groupnode)`: the text of the content of a group (no braces);
    /// of another node, its text.
    pub fn groupnodecontents_to_text(&self, groupnode: Option<&LatexNode>) -> Res<String> {
        match groupnode {
            None => Ok(String::new()),
            Some(n) => match &n.kind {
                NodeKind::Group { nodelist, .. } => self.nodelist_to_text(nodelist),
                _ => self.node_to_text(Some(n)),
            },
        }
    }

    /// `node_arg_to_text(node, k)`: the text of the `k`-th argument of the node (`IndexError` if
    /// it has fewer arguments, nothing if it has none).
    pub fn node_arg_to_text(&self, node: &LatexNode, k: usize) -> Res<String> {
        match node.nodeargd() {
            Some(a) if !a.argnlist.is_empty() => match a.argnlist.get(k) {
                Some(n) => self.groupnodecontents_to_text(n.as_ref()),
                None => Err(Latex2TextError::Index("list index out of range".to_string())),
            },
            _ => Ok(String::new()),
        }
    }

    /// `apply_text_replacements(s, text_replacements)` (deprecated in pylatexenc): replace each
    /// pattern in turn, as plain text.
    pub fn apply_text_replacements(&self, s: &str, text_replacements: &[(&str, &str)]) -> String {
        let mut s = s.to_string();
        for (pattern, replacement) in text_replacements {
            s = s.replace(pattern, replacement);
        }
        s
    }
}

fn has_percent_s(repl: &str) -> bool {
    // re.search(r'(^|[^%])(%%)*%s', repl)
    static RE: OnceLock<fancy_regex::Regex> = OnceLock::new();
    RE.get_or_init(|| fancy_regex::Regex::new(r"(^|[^%])(%%)*%s").expect("valid regex"))
        .is_match(repl)
        .unwrap_or(false)
}

/// `latex2text(content, tolerant_parsing, keep_inline_math, keep_comments)` (deprecated in
/// pylatexenc 1.0 in favor of [`LatexNodes2Text`]).
pub fn latex2text(content: &str, tolerant_parsing: bool, keep_inline_math: bool, keep_comments: bool) -> Res<String> {
    let l2t = LatexNodes2Text::builder()
        .math_mode(if keep_inline_math { MathMode::Verbatim } else { MathMode::Text })
        .keep_comments(keep_comments)
        .build()?;
    let (nodes, _, _) = LatexWalker::with_options(content, LatexWalkerOptions::default().tolerant_parsing(tolerant_parsing))
        .get_latex_nodes(0)?;
    l2t.nodelist_to_text(&nodes)
}

/// `latexnodes2text(nodelist, keep_inline_math, keep_comments)` (deprecated in pylatexenc 1.0).
pub fn latexnodes2text(nodelist: &[LatexNode], keep_inline_math: bool, keep_comments: bool) -> Res<String> {
    LatexNodes2Text::builder()
        .math_mode(if keep_inline_math { MathMode::Verbatim } else { MathMode::Text })
        .keep_comments(keep_comments)
        .build()?
        .nodelist_to_text(nodelist)
}
