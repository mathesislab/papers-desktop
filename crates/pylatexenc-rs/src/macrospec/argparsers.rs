//! Argument parsers of macros, environments and specials (port of
//! `pylatexenc.macrospec._argparsers`).

use std::fmt;
use std::sync::Arc;

use crate::latexwalker::{
    LatexNode, LatexWalker, LatexWalkerError, LatexWalkerParseError, NodeKind, ParsingState,
};
use crate::pystr::is_space;

/// `ParsedMacroArgs` (and `ParsedVerbatimArgs`): the arguments a parser read.
///
/// `argnlist` has one entry per char of `argspec`: the node of the argument, or `None` for an
/// optional argument (`[`) or star (`*`) that is absent.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParsedMacroArgs {
    pub argspec: String,
    pub argnlist: Vec<Option<LatexNode>>,
    /// `ParsedVerbatimArgs.verbatim_text`: set by the verbatim parsers
    pub verbatim_text: Option<String>,
    /// `ParsedVerbatimArgs.verbatim_delimiters`
    pub verbatim_delimiters: Option<(char, char)>,
}

impl ParsedMacroArgs {
    /// `ParsedMacroArgs(argspec=..., argnlist=...)`
    pub fn new(argspec: impl Into<String>, argnlist: Vec<Option<LatexNode>>) -> Self {
        ParsedMacroArgs { argspec: argspec.into(), argnlist, verbatim_text: None, verbatim_delimiters: None }
    }

    /// `ParsedVerbatimArgs(verbatim_chars_node, verbatim_delimiters)`
    pub fn verbatim(verbatim_chars_node: LatexNode, verbatim_delimiters: Option<(char, char)>) -> Self {
        let text = verbatim_chars_node.chars().unwrap_or("").to_string();
        ParsedMacroArgs {
            argspec: "{".to_string(),
            argnlist: vec![Some(verbatim_chars_node)],
            verbatim_text: Some(text),
            verbatim_delimiters,
        }
    }

    /// `legacy_nodeoptarg_nodeargs`: `(nodeoptarg, nodeargs)` as `pylatexenc 1.x` gave them: if
    /// the argspec is (stars, then) one `[` and then only `{`, the optional argument and the
    /// mandatory ones; otherwise `None` and all the arguments.
    pub fn legacy_nodeoptarg_nodeargs(&self) -> (Option<&LatexNode>, &[Option<LatexNode>]) {
        let nskip = self.argspec.chars().take_while(|&c| c == '*').count();
        let mut rest = self.argspec.chars().skip(nskip);
        if rest.next() == Some('[') && rest.all(|c| c == '{') {
            let opt = self.argnlist.get(nskip).and_then(|x| x.as_ref());
            let args = self.argnlist.get(nskip + 1..).unwrap_or(&[]);
            (opt, args)
        } else {
            (None, &self.argnlist)
        }
    }

    /// The nodes of the arguments that are present.
    pub fn args(&self) -> impl Iterator<Item = &LatexNode> {
        self.argnlist.iter().filter_map(|x| x.as_ref())
    }
}

/// What an args parser returns besides the arguments: the optional `new_parsing_state` (the
/// state after the macro, for the following nodes) and `inner_parsing_state` (the state in the
/// body of an environment).
#[derive(Debug, Clone, Default)]
pub struct ArgsParserExtra {
    pub new_parsing_state: Option<Arc<ParsingState>>,
    pub inner_parsing_state: Option<Arc<ParsingState>>,
}

/// What `parse_args()` returns: the arguments (`None` if none could be parsed) and the
/// position and length of what was read, as `(argd, pos, len[, dict])` in Python.
#[derive(Debug, Clone)]
pub struct ParseArgsResult {
    pub argd: Option<ParsedMacroArgs>,
    pub pos: usize,
    pub len: usize,
    pub extra: ArgsParserExtra,
}

impl ParseArgsResult {
    pub fn new(argd: Option<ParsedMacroArgs>, pos: usize, len: usize) -> Self {
        ParseArgsResult { argd, pos, len, extra: ArgsParserExtra::default() }
    }
}

/// An arguments parser: reads the arguments of a macro, environment or specials after it
/// (`parse_args(w, pos, parsing_state)` of the Python args parser objects).
pub trait ArgsParser: fmt::Debug + Send + Sync {
    /// Read the arguments at `pos` of the text of `w`.
    ///
    /// Errors that are `LatexWalkerParseError` or end of stream are handled by the walker
    /// (tolerant parsing: the macro is kept without arguments).
    fn parse_args(
        &self,
        w: &LatexWalker,
        pos: usize,
        parsing_state: Option<&Arc<ParsingState>>,
    ) -> Result<ParseArgsResult, LatexWalkerError>;

    /// The `argspec` (`{` for the verbatim parsers), for the parsers that have one.
    fn argspec(&self) -> Option<&str> {
        None
    }

    /// The parser as a [`MacroStandardArgsParser`], if it is one.
    fn as_standard(&self) -> Option<&MacroStandardArgsParser> {
        None
    }
}

/// `MacroStandardArgsParser(argspec, optional_arg_no_space, args_math_mode)`: arguments given by
/// a string of `*` (an optional star), `[` (an optional `[...]` argument) and `{` (a mandatory
/// argument).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MacroStandardArgsParser {
    pub argspec: String,
    /// do not look for an optional argument `[...]` after whitespace
    pub optional_arg_no_space: bool,
    /// per argument: `Some(true)`/`Some(false)` to read it in/out of math mode, `None` to
    /// inherit the mode (`args_math_mode`); if set, it must have as many entries as the argspec
    pub args_math_mode: Option<Vec<Option<bool>>>,
}

impl MacroStandardArgsParser {
    /// `MacroStandardArgsParser(argspec)`.
    ///
    /// # Panics
    /// If the argspec has chars other than `*`, `[`, `{`; see [`Self::try_new`].
    pub fn new(argspec: &str) -> Self {
        Self::try_new(argspec).expect("invalid argspec")
    }

    /// Like [`Self::new`], returning the error (`TypeError` in Python).
    pub fn try_new(argspec: &str) -> Result<Self, String> {
        if !argspec.chars().all(|x| "*[{".contains(x)) {
            return Err(format!(
                "argspec must be a string containing chars '*', '[', '{{' only: {argspec:?}"
            ));
        }
        Ok(MacroStandardArgsParser { argspec: argspec.to_string(), optional_arg_no_space: false, args_math_mode: None })
    }

    /// The same parser with `optional_arg_no_space` set.
    pub fn optional_arg_no_space(mut self, v: bool) -> Self {
        self.optional_arg_no_space = v;
        self
    }

    /// The same parser with `args_math_mode` set.
    pub fn args_math_mode(mut self, v: Vec<Option<bool>>) -> Self {
        self.args_math_mode = Some(v);
        self
    }
}

impl ArgsParser for MacroStandardArgsParser {
    fn parse_args(
        &self,
        w: &LatexWalker,
        pos: usize,
        parsing_state: Option<&Arc<ParsingState>>,
    ) -> Result<ParseArgsResult, LatexWalkerError> {
        let parsing_state = match parsing_state {
            Some(p) => p.clone(),
            None => w.make_parsing_state(),
        };
        let nargs = self.argspec.chars().count();
        if let Some(amm) = &self.args_math_mode
            && amm.len() != nargs {
                return Err(LatexWalkerError::Other(format!(
                    "ValueError: Invalid args_math_mode={:?} for argspec={:?}!",
                    amm, self.argspec
                )));
            }
        let inner_state = |j: usize| -> Arc<ParsingState> {
            match &self.args_math_mode {
                None => parsing_state.clone(),
                Some(amm) => match amm[j] {
                    None => parsing_state.clone(),
                    Some(m) if m == parsing_state.in_math_mode => parsing_state.clone(),
                    Some(m) => Arc::new(parsing_state.sub_context(
                        &crate::latexwalker::ParsingStateChanges::new().in_math_mode(m),
                    )),
                },
            }
        };
        let mut argnlist = Vec::new();
        let mut p = pos;
        for (j, argt) in self.argspec.chars().enumerate() {
            match argt {
                '{' => {
                    let (node, np, nl) = w.get_latex_expression(p, Some(false), Some(&inner_state(j)))?;
                    p = np + nl;
                    argnlist.push(Some(node));
                }
                '[' => {
                    if self.optional_arg_no_space && p < w.s.len() && is_space(w.s.char_at(p)) {
                        argnlist.push(None);
                        continue;
                    }
                    match w.get_latex_maybe_optional_arg(p, Some(&inner_state(j)))? {
                        None => argnlist.push(None),
                        Some((node, np, nl)) => {
                            p = np + nl;
                            argnlist.push(Some(node));
                        }
                    }
                }
                '*' => {
                    let tok = w.get_token(p)?;
                    if tok.tok == crate::latexwalker::TokenKind::Char && tok.arg.starts_with('*') {
                        argnlist.push(Some(w.make_node(
                            NodeKind::Chars { chars: "*".to_string() },
                            inner_state(j),
                            tok.pos,
                            1,
                        )));
                        p = tok.pos + 1;
                    } else {
                        argnlist.push(None);
                    }
                }
                other => {
                    return Err(LatexWalkerError::Other(format!(
                        "Unknown macro argument kind for macro: {other:?}"
                    )));
                }
            }
        }
        Ok(ParseArgsResult::new(Some(ParsedMacroArgs::new(self.argspec.clone(), argnlist)), pos, p - pos))
    }

    fn argspec(&self) -> Option<&str> {
        Some(&self.argspec)
    }

    fn as_standard(&self) -> Option<&MacroStandardArgsParser> {
        Some(self)
    }
}

/// `VerbatimArgsParser(verbatim_arg_type)`: which kind of verbatim argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbatimArgType {
    /// `'verbatim-environment'`: the body up to `\end{verbatim}`
    VerbatimEnvironment,
    /// `'verb-macro'`: `\verb|...|`, the next non-space char delimits the argument
    VerbMacro,
}

/// `VerbatimArgsParser`: the argument is the text up to a delimiter, read verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerbatimArgsParser {
    pub verbatim_arg_type: VerbatimArgType,
}

impl VerbatimArgsParser {
    pub fn new(verbatim_arg_type: VerbatimArgType) -> Self {
        VerbatimArgsParser { verbatim_arg_type }
    }
}

impl ArgsParser for VerbatimArgsParser {
    fn parse_args(
        &self,
        w: &LatexWalker,
        pos: usize,
        parsing_state: Option<&Arc<ParsingState>>,
    ) -> Result<ParseArgsResult, LatexWalkerError> {
        let ps = match parsing_state {
            Some(p) => p.clone(),
            None => w.make_parsing_state(),
        };
        let s = &w.s;
        match self.verbatim_arg_type {
            VerbatimArgType::VerbatimEnvironment => {
                let Some(endverbpos) = s.find("\\end{verbatim}", pos) else {
                    return Err(LatexWalkerParseError::new("Cannot find matching \\end{verbatim}", Some(pos)).into());
                };
                let len = endverbpos - pos;
                let node = w.make_node(NodeKind::Chars { chars: s.slice(pos, len) }, ps, pos, len);
                Ok(ParseArgsResult::new(Some(ParsedMacroArgs::verbatim(node, None)), pos, len))
            }
            VerbatimArgType::VerbMacro => {
                let mut pos = pos;
                // `w.s[pos]` raises an IndexError in Python if there is nothing after \verb
                if pos >= s.len() {
                    return Err(LatexWalkerError::Other("IndexError: string index out of range".to_string()));
                }
                while is_space(s.char_at(pos)) {
                    pos += 1;
                    if pos >= s.len() {
                        return Err(LatexWalkerParseError::new("Missing argument to \\verb command", Some(pos)).into());
                    }
                }
                let verbdelimchar = s.char_at(pos);
                let beginpos = pos + 1;
                let Some(endpos) = s.find_char(verbdelimchar, beginpos) else {
                    return Err(LatexWalkerParseError::new(
                        "End of stream reached while reading argument to \\verb command",
                        Some(pos),
                    )
                    .into());
                };
                let verbarg = s.slice(beginpos, endpos - beginpos);
                let node = w.make_node(NodeKind::Chars { chars: verbarg }, ps, beginpos, endpos - beginpos);
                Ok(ParseArgsResult::new(
                    Some(ParsedMacroArgs::verbatim(node, Some((verbdelimchar, verbdelimchar)))),
                    pos,
                    endpos + 1 - pos,
                ))
            }
        }
    }

    fn argspec(&self) -> Option<&str> {
        Some("{")
    }
}
