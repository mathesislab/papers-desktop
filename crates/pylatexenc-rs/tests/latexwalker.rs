//! pylatexenc's own test suite for the latexwalker (`test/test_latexwalker.py`, v2.11), in Rust.
//!
//! Notes on the port:
//! - positions count chars, as in Python (`find()` helpers below);
//! - `LatexNode`'s equality ignores the parsing state (Python compares it by identity, but its
//!   tests also pass the state that the nodes get), and `ParsedMacroArgs` compares the argspec too;
//! - a macro read as an expression (the argument of another macro) is `bare_expression`;
//! - the deprecated `macro_dict=` option of `LatexWalker` does not exist: the test that uses it
//!   builds the same context by hand.

use std::sync::Arc;

use pylatexenc::latexwalker::{
    GetNodesOptions, LatexNode, LatexToken, LatexWalker, LatexWalkerError, LatexWalkerOptions, NodeKind,
    ParsingState, ParsingStateChanges, TokenKind, TokenOptions, get_default_latex_context_db,
};
use pylatexenc::macrospec::{
    ArgsParser, CategoryPosition, EnvironmentSpec, MacroSpec, MacroStandardArgsParser, ParseArgsResult,
    ParsedMacroArgs, SpecialsSpec, std_macro, std_macro_argspec,
};

type Ps = Arc<ParsingState>;

// ------------------------------------------------------------------------------ helpers

fn find(s: &str, pat: &str) -> usize {
    find_from(s, pat, 0)
}

/// `s.find(pat, from)`, in chars
fn find_from(s: &str, pat: &str, from: usize) -> usize {
    let chars: Vec<char> = s.chars().collect();
    let p: Vec<char> = pat.chars().collect();
    (from..=chars.len() - p.len()).find(|&i| chars[i..i + p.len()] == p[..]).expect("pattern not found")
}

/// `s.rfind(pat)`, in chars
fn rfind(s: &str, pat: &str) -> usize {
    let chars: Vec<char> = s.chars().collect();
    let p: Vec<char> = pat.chars().collect();
    (0..=chars.len() - p.len()).rev().find(|&i| chars[i..i + p.len()] == p[..]).expect("pattern not found")
}

fn clen(s: &str) -> usize {
    s.chars().count()
}

fn tok(kind: TokenKind, arg: &str, pos: usize, len: usize, pre_space: &str) -> LatexToken {
    LatexToken::new(kind, arg, pos, len, pre_space)
}

fn chars_node(ps: &Ps, chars: &str, pos: usize, len: usize) -> LatexNode {
    LatexNode::new(NodeKind::Chars { chars: chars.to_string() }, ps.clone(), pos, len)
}

fn group(ps: &Ps, delims: (&str, &str), nodelist: Vec<LatexNode>, pos: usize, len: usize) -> LatexNode {
    LatexNode::new(
        NodeKind::Group { nodelist, delimiters: (delims.0.to_string(), delims.1.to_string()) },
        ps.clone(),
        pos,
        len,
    )
}

fn comment(ps: &Ps, comment: &str, post: &str, pos: usize, len: usize) -> LatexNode {
    LatexNode::new(
        NodeKind::Comment { comment: comment.to_string(), comment_post_space: post.to_string() },
        ps.clone(),
        pos,
        len,
    )
}

/// A macro node; `nodeargd` as `LatexMacroNode(nodeargd=...)` (its default is empty args).
fn mac(ps: &Ps, name: &str, nodeargd: Option<ParsedMacroArgs>, post: &str, pos: usize, len: usize) -> LatexNode {
    LatexNode::new(
        NodeKind::Macro {
            macroname: name.to_string(),
            nodeargd,
            macro_post_space: post.to_string(),
            bare_expression: false,
        },
        ps.clone(),
        pos,
        len,
    )
}

/// A macro node read as an expression (the argument of another macro).
fn bare_mac(ps: &Ps, name: &str, pos: usize, len: usize) -> LatexNode {
    let mut n = mac(ps, name, None, "", pos, len);
    if let NodeKind::Macro { bare_expression, .. } = &mut n.kind {
        *bare_expression = true;
    }
    n
}

fn no_args() -> Option<ParsedMacroArgs> {
    Some(ParsedMacroArgs::default())
}

fn args(argspec: &str, argnlist: Vec<Option<LatexNode>>) -> Option<ParsedMacroArgs> {
    Some(ParsedMacroArgs::new(argspec, argnlist))
}

fn env(
    ps: &Ps,
    name: &str,
    nodelist: Vec<LatexNode>,
    nodeargd: Option<ParsedMacroArgs>,
    pos: usize,
    len: usize,
) -> LatexNode {
    LatexNode::new(NodeKind::Environment { environmentname: name.to_string(), nodelist, nodeargd }, ps.clone(), pos, len)
}

fn specials(ps: &Ps, chars: &str, pos: usize, len: usize) -> LatexNode {
    LatexNode::new(NodeKind::Specials { specials_chars: chars.to_string(), nodeargd: None }, ps.clone(), pos, len)
}

fn math(
    ps: &Ps,
    displaytype: &str,
    nodelist: Vec<LatexNode>,
    delims: (&str, &str),
    pos: usize,
    len: usize,
) -> LatexNode {
    LatexNode::new(
        NodeKind::Math {
            displaytype: displaytype.to_string(),
            nodelist,
            delimiters: (delims.0.to_string(), delims.1.to_string()),
        },
        ps.clone(),
        pos,
        len,
    )
}

fn some(n: LatexNode) -> Option<LatexNode> {
    Some(n)
}

fn tok_at(lw: &LatexWalker, pos: usize, ps: Option<&Ps>) -> LatexToken {
    lw.get_token_with(pos, &TokenOptions { parsing_state: ps, ..Default::default() }).unwrap()
}

fn nodes_at(lw: &LatexWalker, pos: usize, opts: &GetNodesOptions) -> (Vec<LatexNode>, usize, usize) {
    lw.get_latex_nodes_with(pos, opts).unwrap()
}

fn sub_math_ps(ps: &Ps, delim: &str) -> Ps {
    Arc::new(ps.sub_context(&ParsingStateChanges::new().in_math_mode(true).math_mode_delimiter(Some(delim))))
}

fn nodelist(n: &LatexNode) -> &[LatexNode] {
    n.nodelist().unwrap()
}

/// `node.nodeargd.argnlist[i]`
fn arg(n: &LatexNode, i: usize) -> &LatexNode {
    n.nodeargd().unwrap().argnlist[i].as_ref().unwrap()
}

fn is_parse_error<T: std::fmt::Debug>(r: Result<T, LatexWalkerError>) -> bool {
    matches!(r, Err(LatexWalkerError::Parse(_)))
}

fn tolerant(s: &str) -> LatexWalker {
    LatexWalker::with_options(s, LatexWalkerOptions::default().tolerant_parsing(true))
}

fn strict(s: &str) -> LatexWalker {
    LatexWalker::with_options(s, LatexWalkerOptions::default().tolerant_parsing(false))
}

// ------------------------------------------------------------------------------ tests

#[test]
fn test_get_token() {
    let latextext = r#"Text \`accent and \textbf{bold text} and $\vec b$ vector \& also Fran\c cois
\begin{enumerate}[(i)]
\item Hi there!  % here goes a comment
\item[a] Hello!  @@@
     \end{enumerate}
\mymacro

New paragraph
"#;
    let lw = LatexWalker::new(latextext);

    assert_eq!(lw.get_token(0).unwrap(), tok(TokenKind::Char, "T", 0, 1, ""));
    assert_eq!(lw.get_token(1).unwrap(), tok(TokenKind::Char, "e", 1, 1, ""));
    let p = find(latextext, r"\`");
    assert_eq!(lw.get_token(p).unwrap(), tok(TokenKind::Macro, "`", p, 2, ""));
    let p = find(latextext, r"\textbf") - 1; // pre space
    assert_eq!(lw.get_token(p).unwrap(), tok(TokenKind::Macro, "textbf", p + 1, 7, " "));
    let p = find(latextext, r"\vec"); // post-space
    assert_eq!(
        lw.get_token(p).unwrap(),
        tok(TokenKind::Macro, "vec", p, 5, "").with_post_space(" ")
    );
    let p = find(latextext, r"\&") - 1; // pre-space and *no* post-space
    assert_eq!(
        lw.get_token(p).unwrap(),
        tok(TokenKind::Macro, "&", p + 1, 2, " ").with_post_space("")
    );
    let p = find(latextext, r"\begin");
    assert_eq!(
        lw.get_token_with(p, &TokenOptions { environments: false, ..Default::default() }).unwrap(),
        tok(TokenKind::Macro, "begin", p, 6, "")
    );
    let p = find(latextext, r"\begin");
    assert_eq!(
        lw.get_token(p).unwrap(),
        tok(TokenKind::BeginEnvironment, "enumerate", p, clen(r"\begin{enumerate}"), "")
    );
    let p = find(latextext, "@@@") + 3; // pre space to \end
    assert_eq!(
        lw.get_token(p).unwrap(),
        tok(TokenKind::EndEnvironment, "enumerate", p + 6, clen(r"\end{enumerate}"), "\n     ")
    );
    let p = find(latextext, "%") - 1;
    assert_eq!(
        lw.get_token(p).unwrap(),
        tok(TokenKind::Comment, " here goes a comment", p + 1, clen("% here goes a comment\n"), " ")
            .with_post_space("\n")
    );
    let p = find(latextext, "{");
    assert_eq!(lw.get_token(p).unwrap(), tok(TokenKind::BraceOpen, "{", p, 1, ""));
    let p = find(latextext, "}");
    assert_eq!(lw.get_token(p).unwrap(), tok(TokenKind::BraceClose, "}", p, 1, ""));

    let brackets = |chars: bool| TokenOptions { brackets_are_chars: chars, ..Default::default() };
    let p = find(latextext, "[");
    assert_eq!(lw.get_token_with(p, &brackets(false)).unwrap(), tok(TokenKind::BraceOpen, "[", p, 1, ""));
    let p = find(latextext, "]");
    assert_eq!(lw.get_token_with(p, &brackets(false)).unwrap(), tok(TokenKind::BraceClose, "]", p, 1, ""));
    let p = find(latextext, "[");
    assert_eq!(lw.get_token_with(p, &brackets(true)).unwrap(), tok(TokenKind::Char, "[", p, 1, ""));
    let p = find(latextext, "]");
    assert_eq!(lw.get_token_with(p, &brackets(true)).unwrap(), tok(TokenKind::Char, "]", p, 1, ""));
    let p = find(latextext, r"\mymacro");
    // no post_space because of paragraph
    assert_eq!(
        lw.get_token(p).unwrap(),
        tok(TokenKind::Macro, "mymacro", p, 8, "").with_post_space("")
    );
}

#[test]
fn test_get_token_mathmodes() {
    let latextext = r"
Here is an inline expression like $\vec{x} + \hat p$ and a display equation $$
   ax + b = y
$$ and another, with a subtle inner math mode:
\[ cx^2+z=-d\quad\text{if $x<0$} \]
And a final inline math mode \(\mbox{Prob}(\mbox{some event if $x>0$})=1\).
";
    let lw = LatexWalker::new(latextext);

    let p = find(latextext, "$");
    assert_eq!(lw.get_token(p).unwrap(), tok(TokenKind::MathmodeInline, "$", p, 1, ""));
    let p = find(latextext, r" \(");
    assert_eq!(lw.get_token(p).unwrap(), tok(TokenKind::MathmodeInline, r"\(", p + 1, 2, " "));
    let p = find(latextext, r"\)");
    assert_eq!(lw.get_token(p).unwrap(), tok(TokenKind::MathmodeInline, r"\)", p, 2, ""));

    let p = find(latextext, "$$");
    assert_eq!(lw.get_token(p).unwrap(), tok(TokenKind::MathmodeDisplay, "$$", p, 2, ""));

    let p = find(latextext, "\n\\[");
    assert_eq!(lw.get_token(p).unwrap(), tok(TokenKind::MathmodeDisplay, r"\[", p + 1, 2, "\n"));

    let p = find(latextext, r"\]");
    assert_eq!(lw.get_token(p).unwrap(), tok(TokenKind::MathmodeDisplay, r"\]", p, 2, ""));
}

#[test]
fn test_get_token_mathmodes_dollardollar() {
    // pos counter:  |0        |10       |20       |30     |38
    let latextext = r"x$\dagger$$\dagger$$$A=B\mbox{$b=a$}$$";

    let lw = LatexWalker::new(latextext);
    let ps = lw.make_parsing_state();

    let p = 1;
    assert_eq!(tok_at(&lw, p, Some(&ps)), tok(TokenKind::MathmodeInline, "$", p, 1, ""));
    let p = 9;
    assert_eq!(
        tok_at(&lw, p, Some(&sub_math_ps(&ps, "$"))),
        tok(TokenKind::MathmodeInline, "$", p, 1, "")
    );
    let p = 10;
    assert_eq!(lw.get_token(p).unwrap(), tok(TokenKind::MathmodeInline, "$", p, 1, ""));
    let p = 18;
    assert_eq!(
        tok_at(&lw, p, Some(&sub_math_ps(&ps, "$"))),
        tok(TokenKind::MathmodeInline, "$", p, 1, "")
    );
    let p = 19;
    assert_eq!(lw.get_token(p).unwrap(), tok(TokenKind::MathmodeDisplay, "$$", p, 2, ""));
    let p = 30;
    assert_eq!(lw.get_token(p).unwrap(), tok(TokenKind::MathmodeInline, "$", p, 1, ""));
    let p = 34;
    assert_eq!(
        tok_at(&lw, p, Some(&sub_math_ps(&ps, "$"))),
        tok(TokenKind::MathmodeInline, "$", p, 1, "")
    );
    let p = 36;
    assert_eq!(
        tok_at(&lw, p, Some(&sub_math_ps(&ps, "$$"))),
        tok(TokenKind::MathmodeDisplay, "$$", p, 2, "")
    );
}

#[test]
fn test_get_latex_expression() {
    let latextext = r#"Text and \`accent and \textbf{bold text} and $\vec b$ more stuff for Fran\c cois
\begin{enumerate}[(i)]
\item Hi there!  % here goes a comment
\item[a] Hello!  @@@
     \end{enumerate}
?`Some ``specials,'' too & more?
"#;
    let lw = tolerant(latextext);
    let ps = lw.make_parsing_state();

    assert_eq!(
        lw.get_latex_expression(0, None, Some(&ps)).unwrap(),
        (chars_node(&ps, "T", 0, 1), 0, 1)
    );
    let p = find(latextext, r"\`");
    assert_eq!(lw.get_latex_expression(p, None, Some(&ps)).unwrap(), (bare_mac(&ps, "`", p, 2), p, 2));
    let p = find(latextext, "{");
    assert_eq!(
        lw.get_latex_expression(p, None, Some(&ps)).unwrap(),
        (group(&ps, ("{", "}"), vec![chars_node(&ps, "bold text", p + 1, 9)], p, 11), p, 11)
    );
    let p = find(latextext, "%") - 2; // check: correctly skips comments also after space
    let q = p + 2 + clen("% here goes a comment\n");
    assert_eq!(lw.get_latex_expression(p, None, Some(&ps)).unwrap(), (bare_mac(&ps, "item", q, 5), q, 5));
    // check correct behavior if directly on brace close
    let p = find(latextext, "}");
    assert_eq!(
        lw.get_latex_expression(p, Some(true), Some(&ps)).unwrap(),
        (chars_node(&ps, "", p, 0), p, 0)
    );
    let lw2 = strict(latextext);
    let ps2 = lw2.make_parsing_state();
    assert_eq!(
        lw2.get_latex_expression(p, Some(false), Some(&ps2)).unwrap(),
        (chars_node(&ps2, "", p, 0), p, 0)
    );
    assert!(is_parse_error(lw2.get_latex_expression(p, Some(true), Some(&ps2))));

    let p = find(latextext, "?`");
    assert_eq!(lw.get_latex_expression(p, None, Some(&ps)).unwrap(), (specials(&ps, "?`", p, 2), p, 2));
}

#[test]
fn test_get_latex_maybe_optional_arg() {
    let latextext = r#"Text and \`accent and \textbf{bold text} and $\vec b$ more stuff for Fran\c cois
\begin{enumerate}[(i)]
\item Hi there!  % here goes a comment
\item[a] Hello!  @@@
     \end{enumerate}
Indeed thanks to \cite[Lemma 3]{Author}, we know that...
"#;
    let lw = strict(latextext);
    let ps = lw.make_parsing_state();

    let p = find(latextext, r"\textbf") + clen(r"\textbf");
    assert_eq!(lw.get_latex_maybe_optional_arg(p, Some(&ps)).unwrap(), None);
    let p = find(latextext, r"\cite") + clen(r"\cite");
    assert_eq!(
        lw.get_latex_maybe_optional_arg(p, Some(&ps)).unwrap(),
        Some((
            group(&ps, ("[", "]"), vec![chars_node(&ps, "Lemma 3", p + 1, clen("Lemma 3"))], p, 9),
            p,
            9
        ))
    );
}

#[test]
fn test_get_latex_braced_group() {
    let latextext = r#"Text and \`accent and \textbf{bold text} and $\vec b$ more stuff for Fran\c cois
\begin{enumerate}[(i)]
\item Hi there!  % here goes a comment
\item[a] Hello!  @@@
     \end{enumerate}
Indeed thanks to \cite[Lemma 3]{Author}, we know that...
Also: {\itshape some italic text}.
"#;
    let lw = strict(latextext);
    let ps = lw.make_parsing_state();

    let p = find(latextext, "Also: {") + clen("Also:"); // points on space after 'Also:'
    let l = clen(r"{\itshape some italic text}");
    let good = (
        group(
            &ps,
            ("{", "}"),
            vec![
                mac(&ps, "itshape", no_args(), " ", p + 2, clen(r"\itshape ")),
                chars_node(&ps, "some italic text", p + 16 - 5, 32 - 16),
            ],
            p + 1,
            l,
        ),
        p + 1,
        l,
    );
    assert_eq!(lw.get_latex_braced_group(p, "{", Some(&ps)).unwrap(), good);
    assert_eq!(lw.get_latex_braced_group(p + 1, "{", Some(&ps)).unwrap(), good);

    let p = find(latextext, "[(i)]");
    assert_eq!(
        lw.get_latex_braced_group(p, "[", Some(&ps)).unwrap(),
        (group(&ps, ("[", "]"), vec![chars_node(&ps, "(i)", p + 1, 3)], p, 5), p, 5)
    );
}

#[test]
fn test_get_latex_environment() {
    let latextext = r#"Text and \`accent and \textbf{bold text} and $\vec b$ more stuff for Fran\c cois
\begin{enumerate}[(i)]
\item Hi there!  % here goes a comment
 \item[a] Hello!  @@@
     \end{enumerate}
Indeed thanks to \cite[Lemma 3]{Author}, we know that...
Also: {\itshape some italic text}.
"#;
    let lw = strict(latextext);
    let ps = lw.make_parsing_state();

    let p = find(latextext, r"\begin{enumerate}");
    let total = find(latextext, r"\end{enumerate}") + clen(r"\end{enumerate}") - p;
    let good = (
        env(
            &ps,
            "enumerate",
            vec![
                chars_node(&ps, "\n", p + 22, 1),
                mac(&ps, "item", args("[", vec![None]), " ", p + 23, 6),
                chars_node(&ps, "Hi there!  ", p + 23 + 6, 17 - 6),
                comment(&ps, " here goes a comment", "\n ", p + 23 + 17, 38 - 17 + 2),
                mac(
                    &ps,
                    "item",
                    args(
                        "[",
                        vec![some(group(
                            &ps,
                            ("[", "]"),
                            vec![chars_node(&ps, "a", p + 23 + 39 + 7, 1)],
                            p + 23 + 39 + 6,
                            3,
                        ))],
                    ),
                    "",
                    p + 23 + 39 + 1,
                    9 - 1,
                ),
                chars_node(&ps, " Hello!  @@@\n     ", p + 23 + 39 + 9, 22 + 5 - 9),
            ],
            args(
                "[",
                vec![some(group(&ps, ("[", "]"), vec![chars_node(&ps, "(i)", p + 18, 21 - 18)], p + 17, 22 - 17))],
            ),
            p,
            total,
        ),
        p,
        total,
    );
    assert_eq!(lw.get_latex_environment(p, Some("enumerate"), Some(&ps)).unwrap(), good);
    assert_eq!(lw.get_latex_environment(p, None, Some(&ps)).unwrap(), good);

    assert!(is_parse_error(lw.get_latex_environment(p, Some("XYZNFKLD-WRONG"), Some(&ps))));
}

#[test]
fn test_get_latex_nodes() {
    let latextext = r#"Text and \`accent and \textbf{bold text} and $\vec b$ more stuff for Fran\c cois
\begin{enumerate}[(i)]
\item Hi there!  % here goes a comment
\item[a] Hello!  @@@
     \end{enumerate}
Indeed thanks to \cite[Lemma 3]{Author}, we know that...
Also: {\itshape some italic text}.
"#;
    let lw = strict(latextext);
    let ps = lw.make_parsing_state();
    let o = GetNodesOptions::new().parsing_state(&ps);

    let p = find(latextext, "Also: {");
    assert_eq!(
        nodes_at(&lw, p, &o),
        (
            vec![
                chars_node(&ps, "Also: ", p, 6),
                group(
                    &ps,
                    ("{", "}"),
                    vec![
                        mac(&ps, "itshape", no_args(), " ", p + 7, 16 - 7),
                        chars_node(&ps, "some italic text", p + 16, 32 - 16),
                    ],
                    p + 6,
                    33 - 6,
                ),
                chars_node(&ps, ".\n", p + 33, 2),
            ],
            p,
            clen(latextext) - p
        )
    );

    let p = find(latextext, "Also: {") + clen("Also: {"); // points inside right after open brace
    assert_eq!(
        nodes_at(&lw, p, &GetNodesOptions::new().stop_upon_closing_brace("}").parsing_state(&ps)),
        (
            vec![
                mac(&ps, "itshape", no_args(), " ", p, 16 - 7),
                chars_node(&ps, "some italic text", p + 16 - 7, 32 - 16),
            ],
            p,
            clen(r"\itshape some italic text}")
        )
    );

    // test our own macro lists etc. (`macro_dict={'cite': std_macro('cite', False, 4)}` of Python:
    // the environments of the default context, and a category with the macro)
    let pindeed = find(latextext, "Indeed thanks to");
    let lineindeed: String = latextext.chars().skip(pindeed).take_while(|&c| c != '\n').collect();
    let default = get_default_latex_context_db();
    let mut ctx = default.filter_context(&[], &[], &["environments"]);
    ctx.add_context_category(
        "custom",
        vec![std_macro("cite", false, 4)],
        default.iter_environment_specs(None).unwrap().into_iter().cloned().collect::<Vec<_>>(),
        Vec::<SpecialsSpec>::new(),
        CategoryPosition::Append,
    )
    .unwrap();
    let lw2 = LatexWalker::with_options(
        &lineindeed,
        LatexWalkerOptions::default().tolerant_parsing(false).latex_context(ctx),
    );
    let ps2 = lw2.make_parsing_state();
    assert_eq!(
        nodes_at(&lw2, 0, &GetNodesOptions::new().parsing_state(&ps2)),
        (
            vec![
                chars_node(&ps2, "Indeed thanks to ", 0, 17),
                mac(
                    &ps2,
                    "cite",
                    args(
                        "{{{{",
                        vec![
                            some(chars_node(&ps2, "[", 22, 1)),
                            some(chars_node(&ps2, "L", 23, 1)),
                            some(chars_node(&ps2, "e", 24, 1)),
                            some(chars_node(&ps2, "m", 25, 1)),
                        ]
                    ),
                    "",
                    17,
                    26 - 17
                ),
                chars_node(&ps2, "ma 3]", 26, 31 - 26),
                group(&ps2, ("{", "}"), vec![chars_node(&ps2, "Author", 32, 38 - 32)], 31, 39 - 31),
                chars_node(&ps2, ", we know that...", 39, 56 - 39),
            ],
            0,
            clen(&lineindeed)
        )
    );
}

#[test]
fn test_get_latex_nodes_mathmodes() {
    let latextext = r"
Here is an inline expression like $\vec{x} + \hat p$ and a display equation $$
   ax + b = y
$$ and another, with a subtle inner math mode:
\[ cx^2+z=-d\quad\text{if $x<0$} \]
And a final inline math mode \(\mbox{Prob}(\mbox{some event if \(x>0\)})=1\).
";
    let lw = strict(latextext);
    let ps = lw.make_parsing_state();

    let p = find(latextext, "$");
    let good = |ps: &Ps| -> Vec<LatexNode> {
        vec![
            mac(
                ps,
                "vec",
                args("{", vec![some(group(ps, ("{", "}"), vec![chars_node(ps, "x", 1 + 40, 1)], 1 + 39, 3))]),
                "",
                1 + 35,
                42 - 35,
            ),
            chars_node(ps, " + ", 1 + 42, 45 - 42),
            mac(ps, "hat", args("{", vec![some(chars_node(ps, "p", 1 + 50, 1))]), " ", 1 + 45, 51 - 45),
        ]
    };
    let ps_math = lw.make_parsing_state_with(&ParsingStateChanges::new().in_math_mode(true));
    assert!(ps_math.in_math_mode);
    let (nodes, pos, len) =
        nodes_at(&lw, p + 1, &GetNodesOptions::new().stop_upon_closing_mathmode("$").parsing_state(&ps_math));
    assert_eq!((nodes, pos, len), (good(&ps_math), p + 1, clen(r"\vec{x} + \hat p$"))); // len includes closing token
    // check that first node is math mode node when parsing starting from & including first '$'
    let nodes = nodes_at(&lw, p, &GetNodesOptions::new().parsing_state(&ps)).0;
    let ps_inner = nodelist(&nodes[0])[0].parsing_state.clone(); // inner state -- math mode -- get this
    assert!(ps_inner.in_math_mode);
    assert_eq!(
        nodes[0],
        math(&ps, "inline", good(&ps_inner), ("$", "$"), p, clen(r"$\vec{x} + \hat p$"))
    );

    let p = find(latextext, "$$");
    assert_eq!(
        nodes_at(&lw, p + 2, &GetNodesOptions::new().stop_upon_closing_mathmode("$$").parsing_state(&ps)),
        (
            vec![chars_node(&ps, "\n   ax + b = y\n", p + 2, clen("\n   ax + b = y\n"))],
            p + 2,
            clen("\n   ax + b = y\n$$")
        ) // len includes closing token
    );
    // check that first node is math mode node when parsing including the math mode start
    let nodes = nodes_at(&lw, p, &GetNodesOptions::new().parsing_state(&ps)).0;
    let ps_inner = nodelist(&nodes[0])[0].parsing_state.clone(); // inner "math mode" state
    assert_eq!(
        nodes[0],
        math(
            &ps,
            "display",
            vec![chars_node(&ps_inner, "\n   ax + b = y\n", p + 2, clen("\n   ax + b = y\n"))],
            ("$$", "$$"),
            p,
            clen("$$\n   ax + b = y\n$$")
        )
    );

    let p = find(latextext, r"\[");
    let good = move |ps: &Ps, ps2: &Ps, ps3: &Ps| -> Vec<LatexNode> {
        vec![
            chars_node(ps, " cx^2+z=-d", p + 2, 12 - 2),
            mac(ps, "quad", no_args(), "", p + 12, 17 - 12),
            mac(
                ps,
                "text",
                args(
                    "{",
                    vec![some(group(
                        ps2,
                        ("{", "}"),
                        vec![
                            chars_node(ps2, "if ", p + 23, 26 - 23),
                            math(ps2, "inline", vec![chars_node(ps3, "x<0", p + 27, 3)], ("$", "$"), p + 26, 31 - 26),
                        ],
                        p + 22,
                        32 - 22,
                    ))],
                ),
                "",
                p + 17,
                32 - 17,
            ),
            chars_node(ps, " ", p + 32, 1),
        ]
    };
    let (nodes, pos, len) =
        nodes_at(&lw, p + 2, &GetNodesOptions::new().stop_upon_closing_mathmode(r"\]").parsing_state(&ps_math));
    let ps2 = arg(&nodes[2], 0).parsing_state.clone(); // "inner text mode" state
    let ps3 = nodelist(&nodelist(arg(&nodes[2], 0))[1])[0].parsing_state.clone();
    assert!(!ps2.in_math_mode);
    assert!(ps3.in_math_mode);
    assert_eq!(
        (nodes, pos, len),
        (good(&ps_math, &ps2, &ps3), p + 2, find_from(latextext, r"\]", p + 2) - (p + 2) + 2) // len includes closing token
    );
    // check that first node is math mode node when parsing including the math mode start
    let nodes = nodes_at(&lw, p, &GetNodesOptions::new().parsing_state(&ps)).0;
    let ps1 = nodelist(&nodes[0])[0].parsing_state.clone();
    let ps2 = arg(&nodelist(&nodes[0])[2], 0).parsing_state.clone();
    let ps3 = nodelist(&nodelist(arg(&nodelist(&nodes[0])[2], 0))[1])[0].parsing_state.clone();
    assert_eq!(
        nodes[0],
        math(&ps, "display", good(&ps1, &ps2, &ps3), (r"\[", r"\]"), p, find_from(latextext, r"\]", p + 2) + 2 - p)
    );

    let p = find(latextext, r"\(");
    let good = move |ps1: &Ps, ps2a: &Ps, ps2b: &Ps, ps3: &Ps| -> Vec<LatexNode> {
        vec![
            mac(
                ps1,
                "mbox",
                args(
                    "{",
                    vec![some(group(
                        ps2a,
                        ("{", "}"),
                        vec![chars_node(ps2a, "Prob", p - 29 + 37, 41 - 37)],
                        p - 29 + 36,
                        42 - 36,
                    ))],
                ),
                "",
                p - 29 + 31,
                42 - 31,
            ),
            chars_node(ps1, "(", p - 29 + 42, 1),
            mac(
                ps1,
                "mbox",
                args(
                    "{",
                    vec![some(group(
                        ps2b,
                        ("{", "}"),
                        vec![
                            chars_node(ps2b, "some event if ", p - 29 + 49, 63 - 49),
                            math(
                                ps2b,
                                "inline",
                                vec![chars_node(ps3, "x>0", p - 29 + 65, 3)],
                                (r"\(", r"\)"),
                                p - 29 + 63,
                                70 - 63,
                            ),
                        ],
                        p - 29 + 48,
                        71 - 48,
                    ))],
                ),
                "",
                p - 29 + 43,
                71 - 43,
            ),
            chars_node(ps1, ")=1", p - 29 + 71, 3),
        ]
    };
    let (nodes, pos, len) =
        nodes_at(&lw, p + 2, &GetNodesOptions::new().stop_upon_closing_mathmode(r"\)").parsing_state(&ps_math));
    let ps2a = arg(&nodes[0], 0).parsing_state.clone();
    assert!(!ps2a.in_math_mode);
    let ps2b = arg(&nodes[2], 0).parsing_state.clone();
    assert!(!ps2b.in_math_mode);
    let ps3 = nodelist(&nodelist(arg(&nodes[2], 0))[1])[0].parsing_state.clone();
    assert!(ps3.in_math_mode);
    assert_eq!(
        (nodes, pos, len),
        (good(&ps_math, &ps2a, &ps2b, &ps3), p + 2, rfind(latextext, r"\)") - (p + 2) + 2) // len includes closing token
    );
    // artificially parse expression in text mode to see that \mbox{} copies
    // the state object instead of creating a new one
    let (nodes, pos, len) =
        nodes_at(&lw, p + 2, &GetNodesOptions::new().stop_upon_closing_mathmode(r"\)").parsing_state(&ps));
    let ps2a = arg(&nodes[0], 0).parsing_state.clone();
    assert!(!ps2a.in_math_mode);
    let ps2b = arg(&nodes[2], 0).parsing_state.clone();
    assert!(!ps2b.in_math_mode);
    let ps3 = nodelist(&nodelist(arg(&nodes[2], 0))[1])[0].parsing_state.clone();
    assert!(ps3.in_math_mode);
    assert_eq!(
        (nodes, pos, len),
        (good(&ps, &ps2a, &ps2b, &ps3), p + 2, rfind(latextext, r"\)") - (p + 2) + 2) // len includes closing token
    );
    // check that first node is math mode node when parsing including the math mode start
    let (nodes, _, _) = nodes_at(&lw, p, &GetNodesOptions::new().parsing_state(&ps));
    let inner = nodelist(&nodes[0]);
    let ps1 = inner[0].parsing_state.clone();
    let ps2a = arg(&inner[0], 0).parsing_state.clone();
    assert!(!ps2a.in_math_mode);
    let ps2b = arg(&inner[2], 0).parsing_state.clone();
    assert!(!ps2b.in_math_mode);
    let ps3 = nodelist(&nodelist(arg(&inner[2], 0))[1])[0].parsing_state.clone();
    assert!(ps3.in_math_mode);
    assert_eq!(
        nodes[0],
        math(&ps, "inline", good(&ps1, &ps2a, &ps2b, &ps3), (r"\(", r"\)"), p, rfind(latextext, r"\)") + 2 - p)
    );
}

#[test]
fn test_get_latex_nodes_mathmodes_dollardollar() {
    // pos counter:  |0        |10       |20       |30     |38
    let latextext = r"x$\dagger$$\dagger$";

    let lw = strict(latextext);
    let ps = lw.make_parsing_state();

    let nodes = nodes_at(&lw, 0, &GetNodesOptions::new().parsing_state(&ps)).0;
    let ps1 = nodelist(&nodes[1])[0].parsing_state.clone();
    let ps2 = nodelist(&nodes[2])[0].parsing_state.clone();

    assert_eq!(
        nodes,
        vec![
            chars_node(&ps, "x", 0, 1),
            math(&ps, "inline", vec![mac(&ps1, "dagger", no_args(), "", 2, 9 - 2)], ("$", "$"), 1, 10 - 1),
            math(&ps, "inline", vec![mac(&ps2, "dagger", no_args(), "", 11, 18 - 11)], ("$", "$"), 10, 19 - 10),
        ]
    );
}

#[test]
fn test_get_latex_nodes_comments() {
    let latextext = "Hello % comment here\n  % more comments \n\nNew paragraph here.\n% comment at end";

    let lw = tolerant(latextext);
    let ps = lw.make_parsing_state();

    assert_eq!(
        nodes_at(&lw, 0, &GetNodesOptions::new().parsing_state(&ps)),
        (
            vec![
                chars_node(&ps, "Hello ", 0, 6),
                comment(&ps, " comment here", "\n  ", 6, 20 - 6 + 1 + 2),
                comment(&ps, " more comments ", "", 21 + 2, 18 - 2),
                chars_node(&ps, "\n\nNew paragraph here.\n", 21 + 18, 1 + 1 + 19 + 1),
                comment(&ps, " comment at end", "", 21 + 18 + 2 + 20, 16),
            ],
            0,
            clen(latextext)
        )
    );

    let latextext = "Line with % a comment here\n\n% line comment on its own\n% and a second line\n\nAlso a \\itshape% comment after a macro\n% and also a second line\nsome italic text.";

    let lw = tolerant(latextext);
    let ps = lw.make_parsing_state();

    assert_eq!(
        nodes_at(&lw, 0, &GetNodesOptions::new().parsing_state(&ps)),
        (
            vec![
                chars_node(&ps, "Line with ", 0, 10),
                comment(&ps, " a comment here", "", 10, 26 - 10),
                chars_node(&ps, "\n\n", 26, 2),
                comment(&ps, " line comment on its own", "\n", 27 + 1, 25 + 1),
                comment(&ps, " and a second line", "", 27 + 1 + 26, 19),
                chars_node(&ps, "\n\nAlso a ", 27 + 1 + 26 + 19, 2 + 7),
                mac(&ps, "itshape", args("", vec![]), "", 27 + 1 + 26 + 20 + 1 + 7, 15 - 7),
                comment(&ps, " comment after a macro", "\n", 27 + 1 + 26 + 20 + 1 + 15, 39 - 15),
                comment(&ps, " and also a second line", "\n", 27 + 1 + 26 + 20 + 1 + 39, 25),
                chars_node(&ps, "some italic text.", 27 + 1 + 26 + 20 + 1 + 39 + 25, 17),
            ],
            0,
            clen(latextext)
        )
    );
}

#[test]
fn test_get_latex_nodes_read_max_nodes() {
    let latextext = r#"Text and \`accent and \textbf{bold text} and $\vec b$ more stuff for Fran\c cois
\begin{enumerate}[(i)]
\item Hi there!  % here goes a comment
\item[a] Hello!  @@@
     \end{enumerate}
Indeed thanks to \cite[Lemma 3]{Author}, we know that...
Also: {\itshape some italic text}.
"#;
    let lw = strict(latextext);
    let ps = lw.make_parsing_state();

    let p = 0;
    assert_eq!(
        nodes_at(&lw, p, &GetNodesOptions::new().read_max_nodes(1).parsing_state(&ps)),
        (vec![chars_node(&ps, "Text and ", p, 33 - 24)], p, 33 - 24)
    );

    let p = find(latextext, "ent and ") + 4; // points on second "and" on first line
    let (nodes, pos, len) = nodes_at(&lw, p, &GetNodesOptions::new().read_max_nodes(5).parsing_state(&ps));
    let ps_inner = nodelist(&nodes[3])[0].parsing_state.clone(); // inner state -- math mode -- get this
    assert!(ps_inner.in_math_mode);
    assert_eq!(
        (nodes, pos, len),
        (
            vec![
                chars_node(&ps, "and ", p, 4),
                mac(
                    &ps,
                    "textbf",
                    args(
                        "{",
                        vec![some(group(
                            &ps,
                            ("{", "}"),
                            vec![chars_node(&ps, "bold text", p + 54 - 42, 9)],
                            p + 53 - 42,
                            11,
                        ))]
                    ),
                    "",
                    p + 46 - 42,
                    64 - 46
                ),
                chars_node(&ps, " and ", p + 64 - 42, 69 - 64),
                math(
                    &ps,
                    "inline",
                    vec![mac(
                        &ps_inner,
                        "vec",
                        args("{", vec![some(chars_node(&ps_inner, "b", p + 75 - 42, 1))]),
                        " ",
                        p + 70 - 42,
                        76 - 70
                    )],
                    ("$", "$"),
                    p + 69 - 42,
                    77 - 69
                ),
                chars_node(&ps, " more stuff for Fran", p + 77 - 42, 97 - 77),
            ],
            p,
            97 - 42
        )
    );
}

fn get_test_latex_data_with_possible_inconsistencies() -> &'static str {
    r#"\documentclass[11pt,a4paper]{article}

\usepackage[utf8]{inputenc}
\usepackage{graphicx}
\usepackage{amsmath}

\begin{document}
D sklfmdklaf dafdaf sah. fjdkslanf \textbf{djksa fnjdka} fdsl\`a; \emph{fnkdsl} a. Fioe
fndjksf.  More {\it italic test} text. Inline math $\vec x$ and
\begin{align}
  \label{eq:Alphas}
  \alpha_{\boldk,\uparrow} =
  \begin{cases} c_{\boldk,\uparrow} & \text{for $k>k_F$} \\
    c_{-\boldk,\downarrow}^\dagger & \text{for $k<k_F$} \end{cases}
  \qquad ; \qquad
  \alpha_{\boldk,\downarrow} =
  \begin{cases} c_{\boldk,\downarrow} & \text{for $k>k_F$} \\
    c_{-\boldk,\uparrow}^\dagger & \text{for $k<k_F$} \end{cases}
  \quad .
\end{align}

\begin{itemize}
\item Show that the $\alpha$, $\alpha^\dagger$'s obey fermionic commutation relations.

  Remember that fermionic creation and annihilation operators $\alpha^\dagger$,
  $\alpha$ obey
  \begin{align}
      \{\alpha^\nodagger_{\boldk,s}, \alpha^\nodagger_{\boldk',s'}\}
      = \{\alpha_{\boldk,s}^\dagger, \alpha_{\boldk',s'}^\dagger\} = 0
      \quad;\quad
      \{\alpha^\nodagger_{\boldk,s}, \alpha_{\boldk',s'}^\dagger\}
      = \delta_{\boldk,\boldk'}\delta_{s,s'}\ .
  \end{align}

\item Argue that eq.~\eqref{eq:Alphas} is a unitary transformation of the creation and annihilation
  operators. Such a transformation is also called a \emph{Bogoliubov transformation}.
  What happens if you act with the annihilators $c_{\boldk,s}$ and
  $\alpha_{\boldk,s}$ on the ground state of the gas?
\end{itemize}

\begin{abc}
  This environment is called `abc,' but I'm going to close it with the wrong "end" command.

  This will confuse the parser:
\end{zzzzz}

What would you think about a stray closing brace: } ?

This is a final sentence. { <-- this brace is not closed.

\end{document}
"#
}

#[test]
fn test_errors() {
    let latextext = get_test_latex_data_with_possible_inconsistencies();

    let lw = strict(latextext);
    assert!(is_parse_error(lw.get_latex_nodes(0)));

    let lw_ok = tolerant(latextext);
    // make sure that it goes through without raising:
    if let Err(e) = lw_ok.get_latex_nodes(0) {
        panic!("get_latex_nodes() failed, but it shouldn't have in tolerant parsing mode!\n{e}");
    }
}

#[test]
fn test_verbatim() {
    let latextext = r"Use the environment \verb+\begin{verbatim}...\end{verbatim}+ to
\begin{verbatim}
typeset \verbatim text with \LaTeX $ escapes \(like this\).
\end{verbatim}
This is it.";

    let lw = LatexWalker::new(latextext);
    let ps = lw.make_parsing_state();

    assert_eq!(
        nodes_at(&lw, 0, &GetNodesOptions::new().parsing_state(&ps)),
        (
            vec![
                chars_node(&ps, "Use the environment ", 0, 20),
                mac(
                    &ps,
                    "verb",
                    Some(ParsedMacroArgs::verbatim(
                        chars_node(&ps, "\\begin{verbatim}...\\end{verbatim}", 26, 33),
                        Some(('+', '+'))
                    )),
                    "",
                    20,
                    40
                ),
                chars_node(&ps, " to\n", 60, 4),
                env(
                    &ps,
                    "verbatim",
                    vec![],
                    Some(ParsedMacroArgs::verbatim(
                        chars_node(
                            &ps,
                            "\ntypeset \\verbatim text with \\LaTeX $ escapes \\(like this\\).\n",
                            80,
                            61
                        ),
                        None
                    )),
                    64,
                    91
                ),
                chars_node(&ps, "\nThis is it.", 155, 12),
            ],
            0,
            167
        )
    );
}

/// `MySimpleNewcommandArgsParser` of the Python test: reads `\newcommand` and gives the walker a
/// new parsing state where the new command exists.
#[derive(Debug)]
struct MySimpleNewcommandArgsParser(MacroStandardArgsParser);

impl ArgsParser for MySimpleNewcommandArgsParser {
    fn parse_args(
        &self,
        w: &LatexWalker,
        pos: usize,
        parsing_state: Option<&Ps>,
    ) -> Result<ParseArgsResult, LatexWalkerError> {
        let mut res = self.0.parse_args(w, pos, parsing_state)?;
        let parsing_state = parsing_state.cloned().unwrap_or_else(|| w.make_parsing_state());
        let argd = res.argd.as_mut().unwrap();
        if argd.argnlist[1].as_ref().unwrap().is_group() {
            let inner = nodelist(argd.argnlist[1].as_ref().unwrap())[0].clone(); // {\command} -> \command
            argd.argnlist[1] = Some(inner);
        }
        let node = argd.argnlist[1].as_mut().unwrap();
        assert!(node.is_macro());
        // hmmm, we should really have a custom parser here to read a single token
        if let NodeKind::Macro { nodeargd, .. } = &mut node.kind {
            *nodeargd = None;
        }
        let newcmdname = node.macroname().unwrap().to_string();
        let numargs: usize = match &argd.argnlist[2] {
            Some(n) => nodelist(n)[0].chars().unwrap().parse().unwrap(),
            None => 0,
        };
        let mut new_latex_context = parsing_state.latex_context.filter_context(&[], &[], &[]);
        new_latex_context
            .add_context_category(
                &format!("newcommand-{newcmdname}"),
                vec![MacroSpec::new(newcmdname.as_str(), &"{".repeat(numargs))],
                Vec::<EnvironmentSpec>::new(),
                Vec::<SpecialsSpec>::new(),
                CategoryPosition::Prepend,
            )
            .unwrap();
        let new_parsing_state =
            parsing_state.sub_context(&ParsingStateChanges::new().latex_context(Arc::new(new_latex_context)));
        res.extra.new_parsing_state = Some(Arc::new(new_parsing_state));
        Ok(res)
    }
}

#[test]
fn test_parsing_state_changes() {
    let latextext = "\\newcommand\\a{AAA}\n\\newcommand{\\b}[2]{BBB #1}\n\nBlah blah blah.\n\nUse macros: \\a{} and \\b{xxx}{yyy}.\n";

    let mut latex_context = get_default_latex_context_db();
    latex_context
        .add_context_category(
            "newcommand-category",
            vec![MacroSpec::with_parser(
                "newcommand",
                MySimpleNewcommandArgsParser(MacroStandardArgsParser::new("*{[[{")),
            )],
            Vec::<EnvironmentSpec>::new(),
            Vec::<SpecialsSpec>::new(),
            CategoryPosition::Prepend,
        )
        .unwrap();

    let lw = LatexWalker::with_options(latextext, LatexWalkerOptions::default().latex_context(latex_context));
    let ps = lw.make_parsing_state();

    let (nodes, npos, nlen) = nodes_at(&lw, 0, &GetNodesOptions::new().parsing_state(&ps));

    let ps_defa = nodes[1].parsing_state.clone();
    let ps_defab = nodes[3].parsing_state.clone();

    assert_eq!((npos, nlen), (0, clen(latextext)));
    assert_eq!(
        nodes,
        vec![
            mac(
                &ps,
                "newcommand",
                args(
                    "*{[[{",
                    vec![
                        None,
                        some(bare_mac(&ps, "a", 11, 2)),
                        None,
                        None,
                        some(group(&ps, ("{", "}"), vec![chars_node(&ps, "AAA", 14, 3)], 13, 5)),
                    ]
                ),
                "",
                0,
                18
            ),
            chars_node(&ps_defa, "\n", 18, 1),
            mac(
                &ps_defa,
                "newcommand",
                args(
                    "*{[[{",
                    vec![
                        None,
                        some(mac(&ps_defa, "b", None, "", 19 + 12, 2)),
                        some(group(&ps_defa, ("[", "]"), vec![chars_node(&ps_defa, "2", 19 + 16, 1)], 19 + 15, 3)),
                        None,
                        some(group(
                            &ps_defa,
                            ("{", "}"),
                            vec![chars_node(&ps_defa, "BBB #1", 19 + 19, 6)],
                            19 + 18,
                            8
                        )),
                    ]
                ),
                "",
                19,
                26
            ),
            chars_node(&ps_defab, "\n\nBlah blah blah.\n\nUse macros: ", 19 + 26, 1 + 1 + 16 + 1 + 12),
            mac(&ps_defab, "a", no_args(), "", 19 + 27 + 1 + 16 + 1 + 12, 2),
            group(&ps_defab, ("{", "}"), vec![], 19 + 27 + 1 + 16 + 1 + 12 + 2, 2),
            chars_node(&ps_defab, " and ", 19 + 27 + 1 + 16 + 1 + 16, 5),
            mac(
                &ps_defab,
                "b",
                args(
                    "{{",
                    vec![
                        some(group(
                            &ps_defab,
                            ("{", "}"),
                            vec![chars_node(&ps_defab, "xxx", 19 + 27 + 1 + 16 + 1 + 24, 3)],
                            19 + 27 + 1 + 16 + 1 + 23,
                            5
                        )),
                        some(group(
                            &ps_defab,
                            ("{", "}"),
                            vec![chars_node(&ps_defab, "yyy", 19 + 27 + 1 + 16 + 1 + 29, 3)],
                            19 + 27 + 1 + 16 + 1 + 28,
                            5
                        )),
                    ]
                ),
                "",
                19 + 27 + 1 + 16 + 1 + 21,
                33 - 21
            ),
            chars_node(&ps_defab, ".\n", 19 + 27 + 1 + 16 + 1 + 33, 2),
        ]
    );
}

#[test]
fn test_error_dangling_missing_args_0() {
    let lw = strict(r"Test \textbf");
    assert!(is_parse_error(lw.get_latex_nodes(0)));
}

#[test]
fn test_error_dangling_missing_args_1() {
    // missing tabular arguments
    let lw = strict(r"Test \begin{tabular}");
    assert!(is_parse_error(lw.get_latex_nodes(0)));
}

#[test]
fn test_error_dangling_missing_args_2() {
    // missing underscore arguments
    let latextext = "Test _";

    let mut latex_context = get_default_latex_context_db();
    latex_context
        .add_context_category(
            "custom-category",
            Vec::<MacroSpec>::new(),
            Vec::<EnvironmentSpec>::new(),
            vec![SpecialsSpec::with_parser("_", MacroStandardArgsParser::new("{"))],
            CategoryPosition::Prepend,
        )
        .unwrap();

    let lw = LatexWalker::with_options(
        latextext,
        LatexWalkerOptions::default().latex_context(latex_context).tolerant_parsing(false),
    );
    assert!(is_parse_error(lw.get_latex_nodes(0)));
}

#[test]
fn test_bug_000() {
    let latextext = "\n\\documentclass[stuff]{docclass}\n\n\\begin{document}\n% empty document\n\\end{document}\n";

    let lw = strict(latextext);
    let ps = lw.make_parsing_state();

    let p = 0;
    assert_eq!(
        nodes_at(&lw, p, &GetNodesOptions::new().read_max_nodes(3).parsing_state(&ps)),
        (
            vec![
                chars_node(&ps, "\n", 0, 1),
                mac(
                    &ps,
                    "documentclass",
                    args(
                        "[{",
                        vec![
                            some(group(
                                &ps,
                                ("[", "]"),
                                vec![chars_node(&ps, "stuff", 1 + 15, 20 - 15)],
                                1 + 14,
                                21 - 14
                            )),
                            some(group(
                                &ps,
                                ("{", "}"),
                                vec![chars_node(&ps, "docclass", 1 + 22, 30 - 22)],
                                1 + 21,
                                31 - 21
                            )),
                        ]
                    ),
                    "",
                    1,
                    31
                ),
                chars_node(&ps, "\n\n", 1 + 31, 2),
            ],
            p,
            34
        )
    );
}

#[test]
fn test_invalid_environment_macros_0() {
    let lw = strict(r"\begin \item stuff\end{itemize}");
    assert!(is_parse_error(lw.get_latex_nodes(0)));
}

#[test]
fn test_invalid_environment_macros_1() {
    let lw = strict(r"\begin{itemize} \item stuff\end");
    assert!(is_parse_error(lw.get_latex_nodes(0)));
}

#[test]
fn test_invalid_environment_macros_2() {
    let latextext = "\\begin\n  \\item stuff\n\\end";

    let lw = tolerant(latextext);
    let ps = lw.make_parsing_state();

    let p = 0;
    assert_eq!(
        nodes_at(&lw, p, &GetNodesOptions::new().parsing_state(&ps)),
        (
            vec![
                chars_node(&ps, "\\begin\n  ", 0, 9),
                mac(&ps, "item", args("[", vec![None]), " ", 7 + 2, 8 - 2),
                chars_node(&ps, "stuff\n\\end", 7 + 8, 13 + 1 - 8 + 4),
            ],
            p,
            7 + 14 + 4
        )
    );
}

#[test]
fn test_bug_issueno49() {
    // see issue #49
    let doc_content = "\\emph{A}\n\\emph{B}";

    let lw = strict(doc_content);

    let pos = 0;
    let (nodelist, npos, nlen) = nodes_at(&lw, pos, &GetNodesOptions::new().read_max_nodes(1));
    assert_eq!(nodelist.len(), 1);
    let node = &nodelist[0];
    assert_eq!(npos, node.pos);
    assert_eq!(nlen, node.len);

    let pos = find(doc_content, "\n");
    let (nodelist, npos, nlen) = nodes_at(&lw, pos, &GetNodesOptions::new().read_max_nodes(1));
    assert_eq!(nodelist.len(), 1);
    let node = &nodelist[0];
    assert_eq!(npos, node.pos);
    assert_eq!(nlen, node.len);
}

#[test]
fn test_bug_issueno57() {
    // see issue #57
    let mut walker_context = get_default_latex_context_db();
    walker_context
        .add_context_category(
            "mymacros",
            vec![std_macro_argspec("foo", "[{[")],
            Vec::<EnvironmentSpec>::new(),
            Vec::<SpecialsSpec>::new(),
            CategoryPosition::Prepend,
        )
        .unwrap();

    let doc_content = r"\foo{test}";

    let lw = LatexWalker::with_options(
        doc_content,
        LatexWalkerOptions::default().latex_context(walker_context).tolerant_parsing(false),
    );

    // shouldn't raise exception
    let (nodelist, _npos, _nlen) = nodes_at(&lw, 0, &GetNodesOptions::new().read_max_nodes(1));
    assert_eq!(nodelist.len(), 1);
    let node = &nodelist[0];
    assert_eq!(node.macroname(), Some("foo"));
    assert!(node.nodeargd().is_some());
    assert_eq!(node.nodeargd().unwrap().argnlist.len(), 3);
}
