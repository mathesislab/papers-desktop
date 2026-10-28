//! The walker itself (`pylatexenc.latexwalker.LatexWalker`).
//!
//! The control flow mirrors pylatexenc 2.11, including its tolerant parsing: pylatexenc
//! signals problems with two exceptions (`LatexWalkerEndOfStream`, `LatexWalkerParseError`) and
//! recovers from them at well-defined points; here they are the variants of
//! [`LatexWalkerError`], and recovery happens at the same points, so malformed input gives the
//! same tree.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use super::{
    LatexNode, LatexToken, LatexWalkerError, LatexWalkerParseError, NodeKind, ParsingState, ParsingStateChanges,
    SourceText, TokenKind, default_latex_context_db,
};
use crate::macrospec::{ArgsParserExtra, LatexContextDb, ParseArgsResult, ParsedMacroArgs};
use crate::pystr::{is_alpha, is_space, is_word};
use crate::util::LineNumbersCalculator;

type Res<T> = Result<T, LatexWalkerError>;

/// The options of a [`LatexWalker`] (the keyword arguments of `LatexWalker(s, **kwargs)`).
#[derive(Debug, Clone)]
pub struct LatexWalkerOptions {
    /// the database of macros, environments and specials (default: the default database)
    pub latex_context: Option<Arc<LatexContextDb>>,
    /// recover from errors instead of failing (default `true`)
    pub tolerant_parsing: bool,
    /// fail on a closing brace where an expression is expected (only has an effect if
    /// `tolerant_parsing` is off; default `false`)
    pub strict_braces: bool,
}

impl Default for LatexWalkerOptions {
    fn default() -> Self {
        LatexWalkerOptions { latex_context: None, tolerant_parsing: true, strict_braces: false }
    }
}

impl LatexWalkerOptions {
    pub fn new() -> Self {
        Self::default()
    }
    /// Set the `latex_context`.
    pub fn latex_context(mut self, db: impl Into<Arc<LatexContextDb>>) -> Self {
        self.latex_context = Some(db.into());
        self
    }
    /// Set `tolerant_parsing`.
    pub fn tolerant_parsing(mut self, v: bool) -> Self {
        self.tolerant_parsing = v;
        self
    }
    /// Set `strict_braces`.
    pub fn strict_braces(mut self, v: bool) -> Self {
        self.strict_braces = v;
        self
    }
}

/// The options of `get_token()` besides the position.
#[derive(Debug, Clone)]
pub struct TokenOptions<'a> {
    /// further pairs of chars to be read as `brace_open`/`brace_close` tokens (`{`, `}` always are)
    pub include_brace_chars: Vec<(char, char)>,
    /// read `\begin{...}` and `\end{...}` as environment tokens (default `true`)
    pub environments: bool,
    /// the parsing state (default: the default state of the walker)
    pub parsing_state: Option<&'a Arc<ParsingState>>,
    /// read `[` and `]` as chars (default `true`), else as braces
    pub brackets_are_chars: bool,
}

impl Default for TokenOptions<'_> {
    fn default() -> Self {
        TokenOptions { include_brace_chars: Vec::new(), environments: true, parsing_state: None, brackets_are_chars: true }
    }
}

/// The options of `get_latex_nodes()` besides the position.
#[derive(Debug, Clone, Default)]
pub struct GetNodesOptions<'a> {
    /// stop at this closing brace: `}`, `]`, `)`, `>` or two chars, the opening and closing brace
    pub stop_upon_closing_brace: Option<String>,
    /// stop at `\end{...}` of this environment
    pub stop_upon_end_environment: Option<String>,
    /// stop at this closing math mode delimiter (`$`, `$$`, `\)`, `\]`)
    pub stop_upon_closing_mathmode: Option<String>,
    /// stop after reading this many nodes (`None` or 0: no limit)
    pub read_max_nodes: Option<usize>,
    /// the parsing state (default: the default state of the walker)
    pub parsing_state: Option<&'a Arc<ParsingState>>,
}

impl<'a> GetNodesOptions<'a> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn stop_upon_closing_brace(mut self, v: &str) -> Self {
        self.stop_upon_closing_brace = Some(v.to_string());
        self
    }
    pub fn stop_upon_end_environment(mut self, v: &str) -> Self {
        self.stop_upon_end_environment = Some(v.to_string());
        self
    }
    pub fn stop_upon_closing_mathmode(mut self, v: &str) -> Self {
        self.stop_upon_closing_mathmode = Some(v.to_string());
        self
    }
    pub fn read_max_nodes(mut self, v: usize) -> Self {
        self.read_max_nodes = Some(v);
        self
    }
    pub fn parsing_state(mut self, v: &'a Arc<ParsingState>) -> Self {
        self.parsing_state = Some(v);
        self
    }
}

/// `LatexWalker`: parses a LaTeX text into nodes.
#[derive(Debug)]
pub struct LatexWalker {
    /// the text
    pub s: Arc<SourceText>,
    line_no_calc: OnceLock<LineNumbersCalculator>,
    /// log (at `debug` level) the nodes as they are made
    pub debug_nodes: bool,
    /// the parsing state at the beginning of the text
    pub default_parsing_state: Arc<ParsingState>,
    /// recover from errors instead of failing
    pub tolerant_parsing: bool,
    strict_braces: AtomicBool,
    /// how deep `get_latex_nodes` is nested now
    depth: AtomicUsize,
}

/// The deepest nesting of groups, environments and math that is read (Python raises a
/// `RecursionError` at a similar depth, 165 to 330 levels with its default recursion limit; a
/// Rust stack must not overflow).
pub const MAX_NESTING_DEPTH: usize = 250;

/// the stack that must remain, and the size of a new segment when it does not
const STACK_RED_ZONE: usize = 192 * 1024;
const STACK_SEGMENT: usize = 2 * 1024 * 1024;

/// the result of reading one token in `get_latex_nodes`
enum ReadOutcome {
    Continue,
    End,
    /// tolerant parsing: the end of the text, with the whitespace before it
    EndOfStream(String),
}

struct PosPointer {
    pos: usize,
    parsing_state: Arc<ParsingState>,
    lastchars: String,
    lastchars_pos: Option<usize>,
}

impl PosPointer {
    fn push_lastchars(&mut self, pos: usize, chars: &str) {
        self.lastchars.push_str(chars);
        if self.lastchars_pos.is_none() {
            self.lastchars_pos = Some(pos);
        }
    }

    fn flush_lastchars(&mut self) -> (usize, String) {
        let res = (self.lastchars_pos.unwrap_or(0), std::mem::take(&mut self.lastchars));
        self.lastchars_pos = None;
        res
    }
}

struct NodesConfig {
    stop_upon_closing_brace: Option<String>,
    stop_upon_end_environment: Option<String>,
    stop_upon_closing_mathmode: Option<String>,
    read_max_nodes: usize,
    include_brace_chars: Vec<(char, char)>,
}

fn clen(s: &str) -> usize {
    s.chars().count()
}

impl LatexWalker {
    /// `LatexWalker(s)`: with the default context, tolerant parsing.
    pub fn new(s: &str) -> Self {
        Self::with_options(s, LatexWalkerOptions::default())
    }

    /// `LatexWalker(s, latex_context=..., tolerant_parsing=..., strict_braces=...)`
    pub fn with_options(s: &str, options: LatexWalkerOptions) -> Self {
        let src = Arc::new(SourceText::new(s));
        let latex_context = options.latex_context.unwrap_or_else(default_latex_context_db);
        LatexWalker {
            default_parsing_state: Arc::new(ParsingState::new(src.clone(), latex_context)),
            s: src,
            line_no_calc: OnceLock::new(),
            debug_nodes: false,
            tolerant_parsing: options.tolerant_parsing,
            strict_braces: AtomicBool::new(options.strict_braces),
            depth: AtomicUsize::new(0),
        }
    }

    /// `strict_braces`
    pub fn strict_braces(&self) -> bool {
        self.strict_braces.load(Ordering::Relaxed)
    }

    /// `make_parsing_state()`: the default parsing state.
    pub fn make_parsing_state(&self) -> Arc<ParsingState> {
        self.default_parsing_state.clone()
    }

    /// `make_parsing_state(**kwargs)`: the default parsing state with some changes.
    pub fn make_parsing_state_with(&self, changes: &ParsingStateChanges) -> Arc<ParsingState> {
        Arc::new(self.default_parsing_state.sub_context(changes))
    }

    /// `parse_flags()`
    pub fn parse_flags(&self) -> LatexWalkerOptions {
        LatexWalkerOptions {
            latex_context: Some(self.default_parsing_state.latex_context.clone()),
            tolerant_parsing: self.tolerant_parsing,
            strict_braces: self.strict_braces(),
        }
    }

    /// `pos_to_lineno_colno(pos)`: `(lineno, colno)` (lines from 1, columns from 0).
    pub fn pos_to_lineno_colno(&self, pos: usize) -> (usize, usize) {
        self.line_no_calc
            .get_or_init(|| LineNumbersCalculator::new(self.s.chars()))
            .pos_to_lineno_colno(pos)
    }

    fn parse_error(&self, msg: impl Into<String>, pos: usize) -> LatexWalkerError {
        let (l, c) = self.pos_to_lineno_colno(pos);
        let mut e = LatexWalkerParseError::new(msg, Some(pos));
        e.lineno = Some(l);
        e.colno = Some(c);
        LatexWalkerError::Parse(e)
    }

    fn report_ignore_parse_error(&self, e: &LatexWalkerError) {
        log::info!("Ignoring parse error (tolerant parsing mode): {e}");
    }

    /// `make_node(node_class, parsing_state=..., pos=..., len=...)`
    pub fn make_node(&self, kind: NodeKind, parsing_state: Arc<ParsingState>, pos: usize, len: usize) -> LatexNode {
        let node = LatexNode::new(kind, parsing_state, pos, len);
        if self.debug_nodes {
            log::debug!("New node: {node:?}");
        }
        node
    }

    // ------------------------------------------------------------------------------ tokens

    /// `get_token(pos)`: the token at `pos`, with the default options.
    pub fn get_token(&self, pos: usize) -> Res<LatexToken> {
        self.get_token_with(pos, &TokenOptions::default())
    }

    /// `get_token(pos, include_brace_chars=..., environments=..., parsing_state=..., ...)`
    ///
    /// Fails with [`LatexWalkerError::EndOfStream`] at the end of the text.
    pub fn get_token_with(&self, pos: usize, opts: &TokenOptions) -> Res<LatexToken> {
        let parsing_state = opts.parsing_state.unwrap_or(&self.default_parsing_state);
        let mut brace_chars = vec![('{', '}')];
        brace_chars.extend(opts.include_brace_chars.iter().copied());
        if !opts.brackets_are_chars {
            brace_chars.push(('[', ']'));
        }
        let s = &*self.s;
        let n = s.len();
        let mut pos = pos;
        let mut space = String::new();

        while pos < n && is_space(s.char_at(pos)) {
            space.push(s.char_at(pos));
            pos += 1;
            if space.ends_with("\n\n") {
                let pre = &space[..space.len() - 2];
                return Ok(LatexToken::new(TokenKind::Char, "\n\n", pos - 2, 2, pre));
            }
        }
        if pos >= n {
            return Err(LatexWalkerError::EndOfStream { final_space: space });
        }
        let c = s.char_at(pos);

        if c == '\\' {
            if pos + 1 >= n {
                return Err(LatexWalkerError::EndOfStream { final_space: String::new() });
            }
            let mut macro_ = String::new();
            macro_.push(s.char_at(pos + 1));
            let mut isalphamacro = false;
            let mut i = 2;
            if is_alpha(s.char_at(pos + 1)) {
                isalphamacro = true;
                while pos + i < n && is_alpha(s.char_at(pos + i)) {
                    macro_.push(s.char_at(pos + i));
                    i += 1;
                }
            }
            if macro_ == "[" || macro_ == "]" {
                return Ok(LatexToken::new(TokenKind::MathmodeDisplay, format!("\\{macro_}"), pos, i, space));
            }
            if macro_ == "(" || macro_ == ")" {
                return Ok(LatexToken::new(TokenKind::MathmodeInline, format!("\\{macro_}"), pos, i, space));
            }
            if opts.environments && (macro_ == "begin" || macro_ == "end") {
                // re.match(r'^\s*\{([\w* ._-]+)\}', s[pos+i:])
                match self.match_environment_name(pos + i) {
                    None => {
                        let msg = format!("Bad \\{macro_} macro: expected {{environmentname}}");
                        let e = self.parse_error(msg, pos);
                        if self.tolerant_parsing {
                            self.report_ignore_parse_error(&e);
                            return Ok(LatexToken::new(TokenKind::Char, format!("\\{macro_}"), pos, i, space));
                        }
                        return Err(e);
                    }
                    Some((name, mlen)) => {
                        let kind =
                            if macro_ == "begin" { TokenKind::BeginEnvironment } else { TokenKind::EndEnvironment };
                        return Ok(LatexToken::new(kind, name, pos, i + mlen, space));
                    }
                }
            }
            let mut post_space = String::new();
            if isalphamacro {
                while pos + i < n && is_space(s.char_at(pos + i)) {
                    post_space.push(s.char_at(pos + i));
                    i += 1;
                    if post_space.ends_with("\n\n") {
                        post_space.truncate(post_space.len() - 2);
                        i -= 2;
                        break;
                    }
                }
            }
            return Ok(LatexToken::new(TokenKind::Macro, macro_, pos, i, space).with_post_space(post_space));
        }

        if c == '%' {
            // re.compile(r'(\n|\r|\n\r)(?P<extraspace>\s*)').search(s, pos)
            let nl = (pos..n).find(|&k| matches!(s.char_at(k), '\n' | '\r'));
            let (arglen, mlen, mspace);
            if let Some(m_start) = nl {
                let mut m_end = m_start + 1;
                while m_end < n && is_space(s.char_at(m_end)) {
                    m_end += 1;
                }
                let extra_starts_nl = m_end > m_start + 1 && matches!(s.char_at(m_start + 1), '\n' | '\r');
                if extra_starts_nl {
                    arglen = m_start - pos;
                    mlen = m_start - pos;
                    mspace = String::new();
                } else {
                    arglen = m_start - pos;
                    mlen = m_end - pos;
                    mspace = s.slice_range(m_start, m_end);
                }
            } else {
                arglen = n - pos;
                mlen = arglen;
                mspace = String::new();
            }
            return Ok(LatexToken::new(TokenKind::Comment, s.slice_range(pos + 1, pos + arglen), pos, mlen, space)
                .with_post_space(mspace));
        }

        if brace_chars.iter().any(|&(o, _)| o == c) {
            return Ok(LatexToken::new(TokenKind::BraceOpen, c.to_string(), pos, 1, space));
        }
        if brace_chars.iter().any(|&(_, cl)| cl == c) {
            return Ok(LatexToken::new(TokenKind::BraceClose, c.to_string(), pos, 1, space));
        }

        if s.starts_with("$$", pos)
            && !(parsing_state.in_math_mode && parsing_state.math_mode_delimiter.as_deref() == Some("$"))
        {
            return Ok(LatexToken::new(TokenKind::MathmodeDisplay, "$$", pos, 2, space));
        }
        if c == '$' {
            return Ok(LatexToken::new(TokenKind::MathmodeInline, "$", pos, 1, space));
        }

        if let Some(sspec) = parsing_state.latex_context.test_for_specials(s, pos, Some(parsing_state)) {
            let mut t = LatexToken::new(TokenKind::Specials, sspec.specials_chars.clone(), pos, clen(&sspec.specials_chars), space);
            t.specials_spec = Some(sspec.clone());
            return Ok(t);
        }

        Ok(LatexToken::new(TokenKind::Char, c.to_string(), pos, 1, space))
    }

    /// `re.match(r'^\s*\{([\w* ._-]+)\}', s[pos:])`: the environment name and the length matched.
    fn match_environment_name(&self, pos: usize) -> Option<(String, usize)> {
        let s = &*self.s;
        let n = s.len();
        let mut k = pos;
        while k < n && is_space(s.char_at(k)) {
            k += 1;
        }
        if k >= n || s.char_at(k) != '{' {
            return None;
        }
        let name_start = k + 1;
        let mut j = name_start;
        while j < n {
            let c = s.char_at(j);
            if is_word(c) || matches!(c, '*' | ' ' | '.' | '_' | '-') {
                j += 1;
            } else {
                break;
            }
        }
        if j == name_start || j >= n || s.char_at(j) != '}' {
            return None;
        }
        Some((s.slice_range(name_start, j), j + 1 - pos))
    }

    // ------------------------------------------------------------------------------ expressions

    fn with_strict_braces<T>(&self, strict_braces: Option<bool>, f: impl FnOnce() -> T) -> T {
        match strict_braces {
            None => f(),
            Some(v) => {
                let old = self.strict_braces.swap(v, Ordering::Relaxed);
                let r = f();
                self.strict_braces.store(old, Ordering::Relaxed);
                r
            }
        }
    }

    /// `get_latex_expression(pos, strict_braces, parsing_state)`: a macro, specials, a char or a
    /// braced group (what a macro takes as a mandatory argument), as `(node, pos, len)`.
    pub fn get_latex_expression(
        &self,
        pos: usize,
        strict_braces: Option<bool>,
        parsing_state: Option<&Arc<ParsingState>>,
    ) -> Res<(LatexNode, usize, usize)> {
        let parsing_state = parsing_state.unwrap_or(&self.default_parsing_state);
        self.with_strict_braces(strict_braces, || {
            let tok = self.get_token_with(
                pos,
                &TokenOptions { environments: false, parsing_state: Some(parsing_state), ..Default::default() },
            )?;
            let mk = |kind: NodeKind, p: usize, l: usize| (self.make_node(kind, parsing_state.clone(), p, l), p, l);
            match tok.tok {
                TokenKind::Macro => {
                    if tok.arg == "end" {
                        if !self.tolerant_parsing {
                            return Err(self.parse_error("Expected expression, got \\end", pos));
                        }
                        return Ok(mk(NodeKind::Chars { chars: String::new() }, tok.pos, 0));
                    }
                    Ok(mk(
                        NodeKind::Macro {
                            macroname: tok.arg,
                            nodeargd: None,
                            macro_post_space: tok.post_space,
                            bare_expression: true,
                        },
                        tok.pos,
                        tok.len,
                    ))
                }
                TokenKind::Specials => Ok(mk(
                    NodeKind::Specials { specials_chars: tok.arg, nodeargd: None },
                    tok.pos,
                    tok.len,
                )),
                TokenKind::Comment => self.get_latex_expression(tok.pos + tok.len, None, Some(parsing_state)),
                TokenKind::BraceOpen => self.get_latex_braced_group(tok.pos, "{", Some(parsing_state)),
                TokenKind::BraceClose => {
                    if self.strict_braces() && !self.tolerant_parsing {
                        return Err(self.parse_error(
                            format!("Expected expression, got closing brace '{}'", tok.arg),
                            pos,
                        ));
                    }
                    Ok(mk(NodeKind::Chars { chars: String::new() }, tok.pos, 0))
                }
                TokenKind::Char => Ok(mk(NodeKind::Chars { chars: tok.arg }, tok.pos, tok.len)),
                TokenKind::MathmodeInline | TokenKind::MathmodeDisplay => {
                    if tok.arg.starts_with('\\') {
                        Ok(mk(
                            NodeKind::Macro {
                                macroname: tok.arg,
                                nodeargd: Some(ParsedMacroArgs::default()),
                                macro_post_space: tok.post_space,
                                bare_expression: true,
                            },
                            tok.pos,
                            tok.len,
                        ))
                    } else {
                        Ok(mk(NodeKind::Chars { chars: tok.arg }, tok.pos, tok.len))
                    }
                }
                _ => Err(self.parse_error(format!("Unknown token type: {}", tok.tok.as_str()), pos)),
            }
        })
    }

    /// `get_latex_maybe_optional_arg(pos, parsing_state)`: a `[...]` group at `pos`, if there is
    /// one, as `(node, pos, len)`.
    pub fn get_latex_maybe_optional_arg(
        &self,
        pos: usize,
        parsing_state: Option<&Arc<ParsingState>>,
    ) -> Res<Option<(LatexNode, usize, usize)>> {
        let parsing_state = parsing_state.unwrap_or(&self.default_parsing_state);
        let tok = match self.get_token_with(
            pos,
            &TokenOptions {
                include_brace_chars: vec![('[', ']')],
                environments: false,
                parsing_state: Some(parsing_state),
                brackets_are_chars: true,
            },
        ) {
            Ok(t) => t,
            Err(LatexWalkerError::EndOfStream { .. }) => return Ok(None),
            Err(e) => return Err(e),
        };
        if tok.tok == TokenKind::BraceOpen && tok.arg == "[" {
            return self.get_latex_braced_group(pos, "[", Some(parsing_state)).map(Some);
        }
        Ok(None)
    }

    /// `get_latex_braced_group(pos, brace_type, parsing_state)`: a group with the given braces
    /// (`{`, `[`, `(`, `<` or two chars: opening and closing) at `pos`.
    pub fn get_latex_braced_group(
        &self,
        pos: usize,
        brace_type: &str,
        parsing_state: Option<&Arc<ParsingState>>,
    ) -> Res<(LatexNode, usize, usize)> {
        let parsing_state = parsing_state.unwrap_or(&self.default_parsing_state);
        let (open, close) = match brace_type {
            "{" => ('{', '}'),
            "[" => ('[', ']'),
            "(" => ('(', ')'),
            "<" => ('<', '>'),
            b if b.chars().count() == 2 => {
                let mut it = b.chars();
                (it.next().unwrap(), it.next().unwrap())
            }
            b => {
                return Err(LatexWalkerError::Other(format!(
                    "ValueError: Invalid brace type for get_latex_braced_group(): {b}"
                )));
            }
        };
        let include_brace_chars = if open != '{' { vec![(open, close)] } else { Vec::new() };
        let firsttok = self.get_token_with(
            pos,
            &TokenOptions { include_brace_chars, parsing_state: Some(parsing_state), ..Default::default() },
        )?;
        if firsttok.tok != TokenKind::BraceOpen || firsttok.arg != open.to_string() {
            return Err(self.parse_error(
                format!(
                    "get_latex_braced_group: not an opening brace/bracket: {}",
                    self.s.get(pos).map(String::from).unwrap_or_default()
                ),
                pos,
            ));
        }
        let (nodelist, npos, nlen) = self.get_latex_nodes_with(
            firsttok.pos + firsttok.len,
            &GetNodesOptions {
                stop_upon_closing_brace: Some(format!("{open}{close}")),
                parsing_state: Some(parsing_state),
                ..Default::default()
            },
        )?;
        let len = npos + nlen - firsttok.pos;
        let node = self.make_node(
            NodeKind::Group { nodelist, delimiters: (open.to_string(), close.to_string()) },
            parsing_state.clone(),
            firsttok.pos,
            len,
        );
        Ok((node, firsttok.pos, len))
    }

    /// `get_latex_environment(pos, environmentname, parsing_state)`: the environment starting at
    /// `pos` with `\begin{...}` (with the given name, if any), as `(node, pos, len)`.
    pub fn get_latex_environment(
        &self,
        pos: usize,
        environmentname: Option<&str>,
        parsing_state: Option<&Arc<ParsingState>>,
    ) -> Res<(LatexNode, usize, usize)> {
        let parsing_state = parsing_state.unwrap_or(&self.default_parsing_state);
        let startpos = pos;
        let firsttok =
            self.get_token_with(pos, &TokenOptions { parsing_state: Some(parsing_state), ..Default::default() })?;
        if firsttok.tok != TokenKind::BeginEnvironment || environmentname.is_some_and(|n| firsttok.arg != n) {
            return Err(self.parse_error(
                format!(
                    "get_latex_environment: expected \\begin{{{}}}: {}",
                    environmentname.unwrap_or("<environment name>"),
                    firsttok.arg
                ),
                pos,
            ));
        }
        let environmentname = environmentname.map(str::to_string).unwrap_or_else(|| firsttok.arg.clone());
        let pos = firsttok.pos + firsttok.len;
        let env_spec = parsing_state.latex_context.get_environment_spec(&environmentname);
        let argsresult = match env_spec {
            None => Ok(ParseArgsResult::new(Some(ParsedMacroArgs::new("", Vec::new())), pos, 0)),
            Some(spec) => spec.parse_args(self, pos, Some(parsing_state)),
        };
        let argsresult = match argsresult {
            Ok(r) => r,
            Err(e @ (LatexWalkerError::EndOfStream { .. } | LatexWalkerError::Parse(_))) => {
                self.exchandle_parse_subexpression(
                    e,
                    firsttok.pos,
                    &format!("arguments of environment \"\\begin{{{environmentname}}}\""),
                )?;
                ParseArgsResult::new(None, pos, 0)
            }
            Err(e) => return Err(e),
        };
        let ParseArgsResult { argd, pos: apos, len: alen, extra: adic } = argsresult;
        let pos = apos + alen;
        let mut parsing_state_inner = adic.inner_parsing_state.unwrap_or_else(|| parsing_state.clone());
        if env_spec.is_some_and(|e| e.is_math_mode) {
            parsing_state_inner = Arc::new(parsing_state.sub_context(
                &ParsingStateChanges::new()
                    .in_math_mode(true)
                    .math_mode_delimiter(Some(&format!("{{{environmentname}}}"))),
            ));
        }
        let (nodelist, npos, nlen) = self.get_latex_nodes_with(
            pos,
            &GetNodesOptions {
                stop_upon_end_environment: Some(environmentname.clone()),
                parsing_state: Some(&parsing_state_inner),
                ..Default::default()
            },
        )?;
        let len = npos + nlen - startpos;
        let node = self.make_node(
            NodeKind::Environment { environmentname, nodelist, nodeargd: argd },
            parsing_state.clone(),
            startpos,
            len,
        );
        Ok((node, startpos, len))
    }

    /// `_exchandle_parse_subexpression`: record where the error happened; `Ok` if tolerant
    /// parsing ignores it, else the error to raise.
    fn exchandle_parse_subexpression(&self, e: LatexWalkerError, tok_pos: usize, what: &str) -> Res<()> {
        let mut pe = match e {
            LatexWalkerError::EndOfStream { .. } => {
                let (l, c) = self.pos_to_lineno_colno(tok_pos);
                let mut p = LatexWalkerParseError::new(format!("End of input while parsing {what}"), Some(tok_pos));
                p.lineno = Some(l);
                p.colno = Some(c);
                p
            }
            LatexWalkerError::Parse(p) => p,
            other => return Err(other),
        };
        if let Some(p) = pe.pos
            && pe.lineno.is_none() && pe.colno.is_none() {
                let (l, c) = self.pos_to_lineno_colno(p);
                pe.lineno = Some(l);
                pe.colno = Some(c);
            }
        let (l, c) = self.pos_to_lineno_colno(tok_pos);
        pe.open_contexts.push((what.to_string(), tok_pos, l, c));
        let e = LatexWalkerError::Parse(pe);
        if self.tolerant_parsing {
            self.report_ignore_parse_error(&e);
            return Ok(());
        }
        Err(e)
    }

    fn add_open_context(&self, e: LatexWalkerError, what: String, pos: usize) -> LatexWalkerError {
        match e {
            LatexWalkerError::Parse(mut p) => {
                let (l, c) = self.pos_to_lineno_colno(pos);
                p.open_contexts.push((what, pos, l, c));
                LatexWalkerError::Parse(p)
            }
            other => other,
        }
    }

    // ------------------------------------------------------------------------------ nodes

    /// `get_latex_nodes(pos)`: all the nodes from `pos` to the end of the text, as
    /// `(nodelist, pos, len)`.
    pub fn get_latex_nodes(&self, pos: usize) -> Res<(Vec<LatexNode>, usize, usize)> {
        self.get_latex_nodes_with(pos, &GetNodesOptions::default())
    }

    /// `get_latex_nodes(pos, stop_upon_closing_brace=..., stop_upon_end_environment=...,
    /// stop_upon_closing_mathmode=..., read_max_nodes=..., parsing_state=...)`
    ///
    /// Fails with a `LatexWalkerError::Other("RecursionError: ...")` if the text nests more than
    /// [`MAX_NESTING_DEPTH`] levels (Python raises a `RecursionError` too).
    pub fn get_latex_nodes_with(&self, pos: usize, opts: &GetNodesOptions) -> Res<(Vec<LatexNode>, usize, usize)> {
        if self.depth.fetch_add(1, Ordering::Relaxed) >= MAX_NESTING_DEPTH {
            self.depth.fetch_sub(1, Ordering::Relaxed);
            return Err(LatexWalkerError::Other(
                "RecursionError: maximum recursion depth exceeded".to_string(),
            ));
        }
        // the recursion (groups in groups...) grows the stack as needed
        let r = stacker::maybe_grow(STACK_RED_ZONE, STACK_SEGMENT, || self.get_latex_nodes_inner(pos, opts));
        self.depth.fetch_sub(1, Ordering::Relaxed);
        r
    }

    fn get_latex_nodes_inner(&self, pos: usize, opts: &GetNodesOptions) -> Res<(Vec<LatexNode>, usize, usize)> {
        let parsing_state = opts.parsing_state.unwrap_or(&self.default_parsing_state).clone();
        let mut nodelist: Vec<LatexNode> = Vec::new();
        let mut cfg = NodesConfig {
            stop_upon_closing_brace: None,
            stop_upon_end_environment: opts.stop_upon_end_environment.clone().filter(|s| !s.is_empty()),
            stop_upon_closing_mathmode: opts.stop_upon_closing_mathmode.clone(),
            read_max_nodes: opts.read_max_nodes.unwrap_or(0),
            include_brace_chars: Vec::new(),
        };
        if let Some(b) = opts.stop_upon_closing_brace.as_deref().filter(|b| !b.is_empty()) {
            let chars: Vec<char> = b.chars().collect();
            let (opening, closing) = match b {
                "}" => ('{', '}'),
                "]" => ('[', ']'),
                ")" => ('(', ')'),
                ">" => ('<', '>'),
                _ if chars.len() == 2 => (chars[0], chars[1]),
                // no known opening brace (`None` in Python): nothing opens it
                _ => ('\0', *chars.last().unwrap()),
            };
            if closing != '}' {
                cfg.include_brace_chars = vec![(opening, closing)];
            }
            cfg.stop_upon_closing_brace = Some(closing.to_string());
        }
        if cfg.stop_upon_closing_mathmode.is_some() && !parsing_state.in_math_mode {
            log::warn!(
                "Call to LatexWalker.get_latex_nodes(stop_upon_closing_mathmode={:?}) but parsing state has in_math_mode={:?}",
                cfg.stop_upon_closing_mathmode,
                parsing_state.in_math_mode
            );
        }
        let origpos = pos;
        let mut p = PosPointer { pos, parsing_state, lastchars: String::new(), lastchars_pos: None };

        loop {
            let r = self.do_read(&cfg, &mut nodelist, &mut p);
            // `Some(final_space)`: ends, as at the end of the text
            let mut end_of_stream: Option<String> = None;
            let end_now = match r {
                Ok(ReadOutcome::Continue) => false,
                Ok(ReadOutcome::End) => true,
                Ok(ReadOutcome::EndOfStream(fs)) => {
                    end_of_stream = Some(fs);
                    true
                }
                Err(LatexWalkerError::EndOfStream { final_space }) => {
                    if cfg.stop_upon_closing_brace.is_some()
                        || cfg.stop_upon_end_environment.is_some()
                        || cfg.stop_upon_closing_mathmode.is_some()
                    {
                        let expecting = if let Some(b) = &cfg.stop_upon_closing_brace {
                            format!("'{b}'")
                        } else if let Some(e) = &cfg.stop_upon_end_environment {
                            format!("\\end{{{e}}}")
                        } else {
                            format!("'{}'", cfg.stop_upon_closing_mathmode.as_deref().unwrap_or(""))
                        };
                        let (l, c) = self.pos_to_lineno_colno(self.s.len());
                        let mut e = LatexWalkerParseError::new(
                            format!("Unexpected end of stream, was expecting {expecting}"),
                            Some(p.pos),
                        );
                        e.lineno = Some(l);
                        e.colno = Some(c);
                        let e = LatexWalkerError::Parse(e);
                        if self.tolerant_parsing {
                            self.report_ignore_parse_error(&e);
                            true
                        } else {
                            return Err(e);
                        }
                    } else {
                        end_of_stream = Some(final_space);
                        true
                    }
                }
                Err(LatexWalkerError::Parse(e)) => {
                    if self.tolerant_parsing {
                        self.report_ignore_parse_error(&LatexWalkerError::Parse(e));
                        false
                    } else {
                        return Err(LatexWalkerError::Parse(e));
                    }
                }
                Err(e) => return Err(e),
            };
            if end_now {
                if let Some(final_space) = end_of_stream {
                    p.push_lastchars(p.pos, &final_space);
                    p.pos += clen(&final_space);
                }
                if !p.lastchars.is_empty() {
                    let (charspos, chars) = p.flush_lastchars();
                    let len = clen(&chars);
                    let strnode = self.make_node(NodeKind::Chars { chars }, p.parsing_state.clone(), charspos, len);
                    nodelist.push(strnode);
                }
                return Ok((nodelist, origpos, p.pos - origpos));
            }
        }
    }

    fn do_read(&self, cfg: &NodesConfig, nodelist: &mut Vec<LatexNode>, p: &mut PosPointer) -> Res<ReadOutcome> {
        let full = |nodelist: &Vec<LatexNode>| cfg.read_max_nodes > 0 && nodelist.len() >= cfg.read_max_nodes;

        let tok = match self.get_token_with(
            p.pos,
            &TokenOptions {
                include_brace_chars: cfg.include_brace_chars.clone(),
                parsing_state: Some(&p.parsing_state),
                ..Default::default()
            },
        ) {
            Ok(t) => t,
            Err(LatexWalkerError::EndOfStream { final_space }) => {
                if self.tolerant_parsing {
                    return Ok(ReadOutcome::EndOfStream(final_space));
                }
                return Err(LatexWalkerError::EndOfStream { final_space });
            }
            Err(e) => return Err(e),
        };
        p.pos = tok.pos + tok.len;

        if tok.tok == TokenKind::Char {
            let chars = format!("{}{}", tok.pre_space, tok.arg);
            p.push_lastchars(tok.pos - clen(&tok.pre_space), &chars);
            return Ok(ReadOutcome::Continue);
        }

        if !p.lastchars.is_empty() {
            let (charspos, chars) = p.flush_lastchars();
            let strnode = self.make_node(
                NodeKind::Chars { chars: format!("{chars}{}", tok.pre_space) },
                p.parsing_state.clone(),
                charspos,
                tok.pos - charspos,
            );
            nodelist.push(strnode);
            if full(nodelist) {
                p.pos = tok.pos;
                return Ok(ReadOutcome::End);
            }
        } else if !tok.pre_space.is_empty() {
            let n = clen(&tok.pre_space);
            let spacestrnode = self.make_node(
                NodeKind::Chars { chars: tok.pre_space.clone() },
                p.parsing_state.clone(),
                tok.pos - n,
                n,
            );
            nodelist.push(spacestrnode);
            if full(nodelist) {
                p.pos = tok.pos;
                return Ok(ReadOutcome::End);
            }
        }

        match tok.tok {
            TokenKind::BraceClose => {
                if cfg.stop_upon_closing_brace.as_deref() != Some(tok.arg.as_str()) {
                    return Err(self.parse_error(
                        format!("Unexpected mismatching closing brace: '{}'", tok.arg),
                        tok.pos,
                    ));
                }
                Ok(ReadOutcome::End)
            }
            TokenKind::EndEnvironment => match cfg.stop_upon_end_environment.as_deref() {
                None => Err(self.parse_error(format!("Unexpected closing environment: '{}'", tok.arg), tok.pos)),
                Some(e) if e != tok.arg => Err(self.parse_error(
                    format!("Unexpected mismatching closing environment: '{}', was expecting '{}'", tok.arg, e),
                    tok.pos,
                )),
                Some(_) => Ok(ReadOutcome::End),
            },
            TokenKind::MathmodeInline | TokenKind::MathmodeDisplay => {
                if let Some(stop) = &cfg.stop_upon_closing_mathmode {
                    if &tok.arg == stop {
                        return Ok(ReadOutcome::End);
                    }
                    if tok.arg == "\\)" || tok.arg == "\\]" {
                        return Err(self.parse_error(
                            format!("Mismatching closing math mode: '{}', expected '{}'", tok.arg, stop),
                            tok.pos,
                        ));
                    }
                } else if tok.arg == "\\)" || tok.arg == "\\]" {
                    return Err(self.parse_error(format!("Unexpected closing math mode: '{}'", tok.arg), tok.pos));
                }
                let corresponding_closing_mathmode = match tok.arg.as_str() {
                    "\\(" => "\\)",
                    "\\[" => "\\]",
                    other => other,
                }
                .to_string();
                let displaytype = if tok.arg == "\\(" || tok.arg == "$" { "inline" } else { "display" };
                let parsing_state_inner = Arc::new(p.parsing_state.sub_context(
                    &ParsingStateChanges::new().in_math_mode(true).math_mode_delimiter(Some(&tok.arg)),
                ));
                let (mathinline_nodelist, mpos, mlen) = self
                    .get_latex_nodes_with(
                        p.pos,
                        &GetNodesOptions {
                            stop_upon_closing_mathmode: Some(corresponding_closing_mathmode.clone()),
                            parsing_state: Some(&parsing_state_inner),
                            ..Default::default()
                        },
                    )
                    .map_err(|e| self.add_open_context(e, format!("math mode \"{}\"", tok.arg), tok.pos))?;
                p.pos = mpos + mlen;
                nodelist.push(self.make_node(
                    NodeKind::Math {
                        displaytype: displaytype.to_string(),
                        nodelist: mathinline_nodelist,
                        delimiters: (tok.arg.clone(), corresponding_closing_mathmode),
                    },
                    p.parsing_state.clone(),
                    tok.pos,
                    mpos + mlen - tok.pos,
                ));
                Ok(if full(nodelist) { ReadOutcome::End } else { ReadOutcome::Continue })
            }
            TokenKind::Comment => {
                let commentnode = self.make_node(
                    NodeKind::Comment { comment: tok.arg.clone(), comment_post_space: tok.post_space.clone() },
                    p.parsing_state.clone(),
                    tok.pos,
                    tok.len,
                );
                nodelist.push(commentnode);
                Ok(if full(nodelist) { ReadOutcome::End } else { ReadOutcome::Continue })
            }
            TokenKind::BraceOpen => {
                let (groupnode, bpos, blen) = self
                    .get_latex_braced_group(tok.pos, &tok.arg, Some(&p.parsing_state))
                    .map_err(|e| self.add_open_context(e, "open brace".to_string(), tok.pos))?;
                p.pos = bpos + blen;
                nodelist.push(groupnode);
                Ok(if full(nodelist) { ReadOutcome::End } else { ReadOutcome::Continue })
            }
            TokenKind::BeginEnvironment => {
                let (envnode, epos, elen) = self
                    .get_latex_environment(tok.pos, Some(&tok.arg), Some(&p.parsing_state))
                    .map_err(|e| {
                        self.add_open_context(e, format!("begin environment \"{}\"", tok.arg), tok.pos)
                    })?;
                p.pos = epos + elen;
                nodelist.push(envnode);
                Ok(if full(nodelist) { ReadOutcome::End } else { ReadOutcome::Continue })
            }
            TokenKind::Macro => {
                let macroname = tok.arg.clone();
                let mspec = p.parsing_state.latex_context.get_macro_spec(&macroname);
                let res = match mspec {
                    None => Ok(ParseArgsResult::new(Some(ParsedMacroArgs::new("", Vec::new())), tok.pos + tok.len, 0)),
                    Some(m) => m.parse_args(self, tok.pos + tok.len, Some(&p.parsing_state)),
                };
                let ParseArgsResult { argd: nodeargd, pos: mapos, len: malen, extra: mdic } = match res {
                    Ok(r) => r,
                    Err(e @ (LatexWalkerError::EndOfStream { .. } | LatexWalkerError::Parse(_))) => {
                        self.exchandle_parse_subexpression(
                            e,
                            tok.pos,
                            &format!("arguments of macro \"{macroname}\""),
                        )?;
                        ParseArgsResult::new(None, tok.pos + tok.len, 0)
                    }
                    Err(e) => return Err(e),
                };
                p.pos = mapos + malen;
                let node = self.make_node(
                    NodeKind::Macro {
                        macroname: tok.arg.clone(),
                        nodeargd,
                        macro_post_space: tok.post_space.clone(),
                        bare_expression: false,
                    },
                    p.parsing_state.clone(),
                    tok.pos,
                    p.pos - tok.pos,
                );
                nodelist.push(node);
                let ArgsParserExtra { new_parsing_state, .. } = mdic;
                if let Some(nps) = new_parsing_state {
                    p.parsing_state = nps;
                }
                Ok(if full(nodelist) { ReadOutcome::End } else { ReadOutcome::Continue })
            }
            TokenKind::Specials => {
                let sspec = tok.specials_spec.clone().expect("a specials token has its spec");
                p.pos = tok.pos + tok.len;
                let res = match sspec.parse_args(self, p.pos, Some(&p.parsing_state)) {
                    Ok(r) => r,
                    Err(e @ (LatexWalkerError::EndOfStream { .. } | LatexWalkerError::Parse(_))) => {
                        self.exchandle_parse_subexpression(
                            e,
                            tok.pos,
                            &format!("arguments of specials \"{}\"", sspec.specials_chars),
                        )?;
                        Some(ParseArgsResult::new(None, p.pos, 0))
                    }
                    Err(e) => return Err(e),
                };
                let (nodeargd, spdic) = match res {
                    Some(r) => {
                        p.pos = r.pos + r.len;
                        (r.argd, r.extra)
                    }
                    None => (None, ArgsParserExtra::default()),
                };
                let node = self.make_node(
                    NodeKind::Specials { specials_chars: sspec.specials_chars.clone(), nodeargd },
                    p.parsing_state.clone(),
                    tok.pos,
                    p.pos - tok.pos,
                );
                nodelist.push(node);
                if let Some(nps) = spdic.new_parsing_state {
                    p.parsing_state = nps;
                }
                Ok(if full(nodelist) { ReadOutcome::End } else { ReadOutcome::Continue })
            }
            TokenKind::Char => unreachable!("char tokens are handled above"),
        }
    }
}

// ---------------------------------------------------------------------------- module functions

/// `get_token(s, pos, brackets_are_chars, environments, **parse_flags)`
pub fn get_token(
    s: &str,
    pos: usize,
    brackets_are_chars: bool,
    environments: bool,
    parse_flags: LatexWalkerOptions,
) -> Res<LatexToken> {
    LatexWalker::with_options(s, parse_flags).get_token_with(
        pos,
        &TokenOptions { environments, brackets_are_chars, ..Default::default() },
    )
}

/// `get_latex_expression(s, pos, **parse_flags)`
pub fn get_latex_expression(s: &str, pos: usize, parse_flags: LatexWalkerOptions) -> Res<(LatexNode, usize, usize)> {
    LatexWalker::with_options(s, parse_flags).get_latex_expression(pos, None, None)
}

/// `get_latex_maybe_optional_arg(s, pos, **parse_flags)`
pub fn get_latex_maybe_optional_arg(
    s: &str,
    pos: usize,
    parse_flags: LatexWalkerOptions,
) -> Res<Option<(LatexNode, usize, usize)>> {
    LatexWalker::with_options(s, parse_flags).get_latex_maybe_optional_arg(pos, None)
}

/// `get_latex_braced_group(s, pos, brace_type, **parse_flags)`
pub fn get_latex_braced_group(
    s: &str,
    pos: usize,
    brace_type: &str,
    parse_flags: LatexWalkerOptions,
) -> Res<(LatexNode, usize, usize)> {
    LatexWalker::with_options(s, parse_flags).get_latex_braced_group(pos, brace_type, None)
}

/// `get_latex_environment(s, pos, environmentname, **parse_flags)`
pub fn get_latex_environment(
    s: &str,
    pos: usize,
    environmentname: Option<&str>,
    parse_flags: LatexWalkerOptions,
) -> Res<(LatexNode, usize, usize)> {
    LatexWalker::with_options(s, parse_flags).get_latex_environment(pos, environmentname, None)
}

/// `get_latex_nodes(s, pos, stop_upon_closing_brace, ..., **parse_flags)`.
///
/// Like in pylatexenc 2.11, where this function ignores its `pos` argument, the nodes are
/// read from the beginning of the text.
pub fn get_latex_nodes(
    s: &str,
    opts: &GetNodesOptions,
    parse_flags: LatexWalkerOptions,
) -> Res<(Vec<LatexNode>, usize, usize)> {
    LatexWalker::with_options(s, parse_flags).get_latex_nodes_with(
        0,
        &GetNodesOptions {
            stop_upon_closing_brace: opts.stop_upon_closing_brace.clone(),
            stop_upon_end_environment: opts.stop_upon_end_environment.clone(),
            stop_upon_closing_mathmode: opts.stop_upon_closing_mathmode.clone(),
            read_max_nodes: None,
            parsing_state: None,
        },
    )
}
