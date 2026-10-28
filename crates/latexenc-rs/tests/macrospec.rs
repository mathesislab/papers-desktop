//! Port of pylatexenc 2.11's `test/test_macrospec.py`.

use std::sync::Arc;

use latexenc::latex2text::LatexNodes2Text;
use latexenc::latexwalker::{
    self, LatexNode, LatexWalker, LatexWalkerError, LatexWalkerOptions, NodeKind, ParsingState, TokenKind,
};
use latexenc::macrospec::{
    ArgsParser, CategoryPosition, EnvironmentSpec, LatexContextDb, MacroSpec, MacroStandardArgsParser,
    ParseArgsResult, ParsedMacroArgs, std_environment, std_environment_argspec, std_macro, std_macro_argspec,
};

type PS = Arc<ParsingState>;

fn chars(ps: &PS, chars: &str, pos: usize, len: usize) -> LatexNode {
    LatexNode::new(NodeKind::Chars { chars: chars.to_string() }, ps.clone(), pos, len)
}

fn group(ps: &PS, open: &str, close: &str, nodelist: Vec<LatexNode>, pos: usize, len: usize) -> LatexNode {
    LatexNode::new(
        NodeKind::Group { nodelist, delimiters: (open.to_string(), close.to_string()) },
        ps.clone(),
        pos,
        len,
    )
}

fn pma(argspec: &str, argnlist: Vec<Option<LatexNode>>) -> ParsedMacroArgs {
    ParsedMacroArgs::new(argspec, argnlist)
}

/// `assertPMAEqual`
fn assert_pma_equal(a: &ParsedMacroArgs, b: &ParsedMacroArgs) {
    assert_eq!(a.argnlist.len(), b.argnlist.len(), "ParsedMacroArgs are different, lengths differ");
    assert_eq!(a.argspec, b.argspec);
    for j in 0..a.argnlist.len() {
        assert_eq!(a.argnlist[j], b.argnlist[j], "Mismatch for item {j} of parsed arguments");
    }
}

fn parse(lw: &LatexWalker, s: &MacroStandardArgsParser, pos: usize, ps: Option<&PS>) -> ParsedMacroArgs {
    s.parse_args(lw, pos, ps).unwrap().argd.unwrap()
}

// ---------------------------------------------------------------- TestMacroStandardArgsParser

#[test]
fn test_marg_0() {
    let lw = LatexWalker::new(r"{ab}");
    let s = MacroStandardArgsParser::new("{");
    let ps = lw.make_parsing_state();
    let argd = parse(&lw, &s, 0, Some(&ps));
    assert_pma_equal(
        &argd,
        &pma("{", vec![Some(group(&ps, "{", "}", vec![chars(&ps, "ab", 1, 2)], 0, 4))]),
    );
}

#[test]
fn test_marg_1() {
    let lw = LatexWalker::new(r"\cmd ab");
    let s = MacroStandardArgsParser::new("{");
    let ps = lw.make_parsing_state();
    let argd = parse(&lw, &s, r"\cmd".len(), Some(&ps));
    assert_pma_equal(&argd, &pma("{", vec![Some(chars(&ps, "a", r"\cmd".len() + 1, 1))]));
}

#[test]
fn test_oarg_0() {
    let lw = LatexWalker::new(r"\cmd[ab] xyz");
    let s = MacroStandardArgsParser::new("[");
    let ps = lw.make_parsing_state();
    let argd = parse(&lw, &s, r"\cmd".len(), Some(&ps));
    assert_pma_equal(
        &argd,
        &pma("[", vec![Some(group(&ps, "[", "]", vec![chars(&ps, "ab", 5, 2)], 4, 4))]),
    );
}

#[test]
fn test_oarg_1() {
    let lw = LatexWalker::new(r"\cmd xyz");
    let s = MacroStandardArgsParser::new("[");
    let argd = parse(&lw, &s, r"\cmd".len(), None);
    assert_pma_equal(&argd, &pma("[", vec![None]));
}

#[test]
fn test_star_0() {
    let lw = LatexWalker::new(r"\cmd xyz");
    let s = MacroStandardArgsParser::new("*");
    let argd = parse(&lw, &s, r"\cmd".len(), None);
    assert_pma_equal(&argd, &pma("*", vec![None]));
}

#[test]
fn test_star_1() {
    let lw = LatexWalker::new(r"\cmd* xyz");
    let s = MacroStandardArgsParser::new("*");
    let ps = lw.make_parsing_state();
    let argd = parse(&lw, &s, r"\cmd".len(), Some(&ps));
    assert_pma_equal(&argd, &pma("*", vec![Some(chars(&ps, "*", 4, 1))]));
}

#[test]
fn test_star_2() {
    let lw = LatexWalker::new(r"\cmd * xyz");
    let s = MacroStandardArgsParser::new("*");
    let ps = lw.make_parsing_state();
    let argd = parse(&lw, &s, r"\cmd".len(), Some(&ps));
    assert_pma_equal(&argd, &pma("*", vec![Some(chars(&ps, "*", 5, 1))]));
}

#[test]
fn test_combined_0() {
    let lw = LatexWalker::new(r"\cmd{ab}c*");
    let s = MacroStandardArgsParser::new("{*[{*");
    let ps = lw.make_parsing_state();
    let argd = parse(&lw, &s, r"\cmd".len(), Some(&ps));
    assert_pma_equal(
        &argd,
        &pma(
            "{*[{*",
            vec![
                Some(group(&ps, "{", "}", vec![chars(&ps, "ab", 5, 2)], 4, 4)),
                None,
                None,
                Some(chars(&ps, "c", 8, 1)),
                Some(chars(&ps, "*", 9, 1)),
            ],
        ),
    );
}

#[test]
fn test_combined_1() {
    let lw = LatexWalker::new(r"\cmd x[ab]c*");
    let s = MacroStandardArgsParser::new("{*[{*");
    let ps = lw.make_parsing_state();
    let argd = parse(&lw, &s, r"\cmd".len(), Some(&ps));
    assert_pma_equal(
        &argd,
        &pma(
            "{*[{*",
            vec![
                Some(chars(&ps, "x", 5, 1)),
                None,
                Some(group(&ps, "[", "]", vec![chars(&ps, "ab", 7, 2)], 6, 4)),
                Some(chars(&ps, "c", 10, 1)),
                Some(chars(&ps, "*", 11, 1)),
            ],
        ),
    );
}

/// `AbsorbAllDetectedPossibleMacroArgumentsParser`
#[derive(Debug)]
struct AbsorbAllDetectedPossibleMacroArgumentsParser;

impl ArgsParser for AbsorbAllDetectedPossibleMacroArgumentsParser {
    fn parse_args(
        &self,
        w: &LatexWalker,
        pos: usize,
        parsing_state: Option<&PS>,
    ) -> Result<ParseArgsResult, LatexWalkerError> {
        let ps = parsing_state.cloned().unwrap_or_else(|| w.make_parsing_state());
        let mut argspec = String::new();
        let mut argnlist = Vec::new();
        let origpos = pos;
        let mut pos = pos;
        loop {
            // inspect the following token at the given position (skips spaces if necessary)
            let tok = match w.get_token(pos) {
                Ok(t) => t,
                Err(LatexWalkerError::EndOfStream { .. }) => break,
                Err(e) => return Err(e),
            };
            if tok.tok == TokenKind::Char && tok.arg.starts_with('*') {
                argspec.push('*');
                argnlist.push(Some(w.make_node(NodeKind::Chars { chars: "*".to_string() }, ps.clone(), tok.pos, 1)));
                pos = tok.pos + 1;
            } else if tok.tok == TokenKind::Char && tok.arg.starts_with('[') {
                let (node, np, nl) = w
                    .get_latex_maybe_optional_arg(pos, Some(&ps))?
                    .expect("an optional argument");
                pos = np + nl;
                argspec.push('[');
                argnlist.push(Some(node));
            } else if tok.tok == TokenKind::BraceOpen {
                let (node, np, nl) = w.get_latex_expression(pos, Some(false), Some(&ps))?;
                pos = np + nl;
                argspec.push('{');
                argnlist.push(Some(node));
            } else {
                // something else -- we're guessing that it's not a macro argument
                break;
            }
        }
        Ok(ParseArgsResult::new(Some(ParsedMacroArgs::new(argspec, argnlist)), origpos, pos - origpos))
    }
}

#[test]
fn test_custom_argparser_absorb_all_detected_args() {
    let mut lw_db = latexwalker::get_default_latex_context_db();
    lw_db.set_unknown_macro_spec(Some(Arc::new(MacroSpec::with_parser(
        "",
        AbsorbAllDetectedPossibleMacroArgumentsParser,
    ))));
    let opts = || LatexWalkerOptions::default().latex_context(lw_db.clone());

    let res = LatexNodes2Text::new()
        .latex_to_text_with(
            r"
\documentclass{article}
\usepackage{times}
\definecolor{gray}
\RequirePackage{fixltx2e}
",
            opts(),
        )
        .unwrap();
    assert_eq!(res.trim(), "");

    let res = LatexNodes2Text::new()
        .latex_to_text_with(
            r"
\unknownsymbol [A, B] + \anotherunknownsymbol {\textstyle \frac{1}{2}} = 0",
            opts(),
        )
        .unwrap();
    assert_eq!(
        res,
        r"
 +  = 0"
    );
}

// ---------------------------------------------------------------- Test_std_macro

fn argspec_of(spec: &MacroSpec) -> String {
    spec.args_parser.argspec().unwrap().to_string()
}

fn env_argspec_of(spec: &EnvironmentSpec) -> String {
    spec.args_parser.argspec().unwrap().to_string()
}

#[test]
fn std_macro_idiom_0() {
    let spec = std_macro_argspec("cmd", "*[{");
    assert_eq!(spec.macroname, "cmd");
    assert_eq!(argspec_of(&spec), "*[{");
}

#[test]
fn std_macro_idiom_1() {
    let spec = std_macro("cmd", true, 3);
    assert_eq!(spec.macroname, "cmd");
    assert_eq!(argspec_of(&spec), "[{{{");
}

#[test]
fn std_macro_idiom_1b() {
    let spec = std_macro("cmd", false, 3);
    assert_eq!(spec.macroname, "cmd");
    assert_eq!(argspec_of(&spec), "{{{");
}

#[test]
fn std_macro_idiom_1c() {
    // std_macro('cmd', None, '{{{'): an argspec after a false optarg
    let spec = std_macro_argspec("cmd", "{{{");
    assert_eq!(spec.macroname, "cmd");
    assert_eq!(argspec_of(&spec), "{{{");
}

#[test]
fn std_macro_idiom_2() {
    // std_macro(('cmd', '*[{['))
    let spec = std_macro_argspec("cmd", "*[{[");
    assert_eq!(spec.macroname, "cmd");
    assert_eq!(argspec_of(&spec), "*[{[");
}

#[test]
fn std_macro_idiom_2b() {
    // std_macro(('cmd', True, 3))
    let spec = std_macro("cmd", true, 3);
    assert_eq!(spec.macroname, "cmd");
    assert_eq!(argspec_of(&spec), "[{{{");
}

#[test]
fn std_macro_idiom_3() {
    // spec is already a `MacroSpec` -- no-op (here: a clone of it)
    let spec = std_macro("cmd", true, 3).clone();
    assert_eq!(spec.macroname, "cmd");
    assert_eq!(argspec_of(&spec), "[{{{");
}

// ---------------------------------------------------------------- Test_std_environment

#[test]
fn std_environment_idiom_0() {
    let spec = std_environment_argspec("environ", "[*{{", true);
    assert_eq!(spec.environmentname, "environ");
    assert_eq!(env_argspec_of(&spec), "[*{{");
    assert!(spec.is_math_mode);
}

#[test]
fn std_environment_idiom_0b() {
    // std_environment('environ', None, is_math_mode=False): no arguments
    let spec = std_environment_argspec("environ", "", false);
    assert_eq!(spec.environmentname, "environ");
    assert_eq!(env_argspec_of(&spec), "");
    assert!(!spec.is_math_mode);
}

#[test]
fn std_environment_idiom_0c() {
    let spec = std_environment_argspec("environ", "[{", false);
    assert_eq!(spec.environmentname, "environ");
    assert_eq!(env_argspec_of(&spec), "[{");
    assert!(!spec.is_math_mode);
}

#[test]
fn std_environment_idiom_1() {
    let spec = std_environment("environ", true, 3, true);
    assert_eq!(spec.environmentname, "environ");
    assert_eq!(env_argspec_of(&spec), "[{{{");
    assert!(spec.is_math_mode);
}

#[test]
fn std_environment_idiom_2() {
    // std_environment(('environ', '[*{{'), is_math_mode=True)
    let spec = std_environment_argspec("environ", "[*{{", true);
    assert_eq!(spec.environmentname, "environ");
    assert_eq!(env_argspec_of(&spec), "[*{{");
    assert!(spec.is_math_mode);
}

#[test]
fn std_environment_idiom_3() {
    // std_environment(('environ', False, 4))
    let spec = std_environment("environ", false, 4, false);
    assert_eq!(spec.environmentname, "environ");
    assert_eq!(env_argspec_of(&spec), "{{{{");
    assert!(!spec.is_math_mode);
}

#[test]
fn std_environment_idiom_3b() {
    // std_environment(('environ', None, '{{{{'))
    let spec = std_environment_argspec("environ", "{{{{", false);
    assert_eq!(spec.environmentname, "environ");
    assert_eq!(env_argspec_of(&spec), "{{{{");
    assert!(!spec.is_math_mode);
}

#[test]
fn std_environment_idiom_4() {
    // spec is already an `EnvironmentSpec` -- no-op (here: a clone of it)
    let spec = std_environment_argspec("environ", "{*{{{", true).clone();
    assert_eq!(spec.environmentname, "environ");
    assert_eq!(env_argspec_of(&spec), "{*{{{");
    assert!(spec.is_math_mode);
}

// ---------------------------------------------------------------- TestLatexContextDb

fn no_envs() -> Vec<EnvironmentSpec> {
    Vec::new()
}

fn no_macros() -> Vec<MacroSpec> {
    Vec::new()
}

fn no_specials() -> Vec<latexenc::macrospec::SpecialsSpec> {
    Vec::new()
}

#[test]
fn test_can_get_macro_spec() {
    let mut db = LatexContextDb::new();
    db.set_unknown_macro_spec(Some(Arc::new(MacroSpec::new("<unknown>", ""))));
    db.add_context_category(
        "cat1",
        vec![std_macro_argspec("aaa", "{"), std_macro_argspec("bbb", "[{[")],
        no_envs(),
        no_specials(),
        CategoryPosition::Append,
    )
    .unwrap();
    db.add_context_category(
        "cat2",
        vec![std_macro_argspec("aaa", "[{"), std_macro_argspec("ccc", "")],
        no_envs(),
        no_specials(),
        CategoryPosition::Append,
    )
    .unwrap();
    db.add_context_category(
        "cat3",
        vec![std_macro_argspec("ccc", "*{"), std_macro_argspec("ddd", "{{[")],
        no_envs(),
        no_specials(),
        CategoryPosition::Prepend,
    )
    .unwrap();

    assert_eq!(db.categories(), ["cat3", "cat1", "cat2"]);

    let check = |name: &str, macroname: &str, argspec: &str| {
        let m = db.get_macro_spec(name).unwrap();
        assert_eq!(m.macroname, macroname);
        assert_eq!(argspec_of(m), argspec);
    };
    check("aaa", "aaa", "{");
    check("bbb", "bbb", "[{[");
    check("ccc", "ccc", "*{");
    check("ddd", "ddd", "{{[");
    // default for unknown
    check("nonexistent", "<unknown>", "");
}

#[test]
fn test_can_get_environment_spec() {
    let mut db = LatexContextDb::new();
    db.set_unknown_environment_spec(Some(Arc::new(EnvironmentSpec::new("<unknown>", ""))));
    db.add_context_category(
        "cat1",
        no_macros(),
        vec![std_environment_argspec("eaaa", "{", false), std_environment_argspec("ebbb", "[{[", false)],
        no_specials(),
        CategoryPosition::Append,
    )
    .unwrap();
    db.add_context_category(
        "cat2",
        no_macros(),
        vec![std_environment_argspec("eaaa", "[{", false), std_environment_argspec("eccc", "", false)],
        no_specials(),
        CategoryPosition::Append,
    )
    .unwrap();
    db.add_context_category(
        "cat3",
        no_macros(),
        vec![std_environment_argspec("eccc", "*{", false), std_environment_argspec("eddd", "{{[", false)],
        no_specials(),
        CategoryPosition::Prepend,
    )
    .unwrap();

    assert_eq!(db.categories(), ["cat3", "cat1", "cat2"]);

    let check = |name: &str, envname: &str, argspec: &str| {
        let e = db.get_environment_spec(name).unwrap();
        assert_eq!(e.environmentname, envname);
        assert_eq!(env_argspec_of(e), argspec);
    };
    check("eaaa", "eaaa", "{");
    check("ebbb", "ebbb", "[{[");
    check("eccc", "eccc", "*{");
    check("eddd", "eddd", "{{[");
    // default for unknown
    check("nonexistent", "<unknown>", "");
}

/// The database of the `test_filter_context_*` tests.
fn filter_db() -> LatexContextDb {
    let mut db = LatexContextDb::new();
    db.set_unknown_macro_spec(Some(Arc::new(MacroSpec::new("<macro unknown>", ""))));
    db.set_unknown_environment_spec(Some(Arc::new(EnvironmentSpec::new("<env unknown>", ""))));
    db.add_context_category(
        "cat1",
        vec![std_macro_argspec("aaa", "{"), std_macro_argspec("bbb", "[{[")],
        vec![std_environment_argspec("eaaa", "{", false), std_environment_argspec("ebbb", "[{[", false)],
        no_specials(),
        CategoryPosition::Append,
    )
    .unwrap();
    db.add_context_category(
        "cat2",
        vec![std_macro_argspec("aaa", "[{"), std_macro_argspec("ccc", "")],
        vec![std_environment_argspec("eaaa", "[{", false), std_environment_argspec("eccc", "", false)],
        no_specials(),
        CategoryPosition::Append,
    )
    .unwrap();
    db.add_context_category(
        "cat3",
        vec![std_macro_argspec("ccc", "*{"), std_macro_argspec("ddd", "{{[")],
        vec![std_environment_argspec("eccc", "*{", false), std_environment_argspec("eddd", "{{[", false)],
        no_specials(),
        CategoryPosition::Prepend,
    )
    .unwrap();
    db
}

fn check_macro(db: &LatexContextDb, name: &str, macroname: &str, argspec: &str) {
    let m = db.get_macro_spec(name).unwrap();
    assert_eq!(m.macroname, macroname);
    assert_eq!(argspec_of(m), argspec);
}

fn check_env(db: &LatexContextDb, name: &str, envname: &str, argspec: &str) {
    let e = db.get_environment_spec(name).unwrap();
    assert_eq!(e.environmentname, envname);
    assert_eq!(env_argspec_of(e), argspec);
}

#[test]
fn test_filter_context_0() {
    let db = filter_db();
    let db2 = db.filter_context(&["cat1", "cat2"], &[], &[]);
    // this should give 'ccc' from cat2, not cat3
    check_macro(&db2, "ccc", "ccc", "");
    // this should no longer exist
    check_macro(&db2, "ddd", "<macro unknown>", "");

    // this should give 'eccc' from cat2, not cat3
    check_env(&db2, "eccc", "eccc", "");
    // this should no longer exist
    check_env(&db2, "eddd", "<env unknown>", "");

    // "unknowns" should be preserved
    fn same<T>(a: &Option<Arc<T>>, b: &Option<Arc<T>>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
    assert!(same(&db2.unknown_macro_spec, &db.unknown_macro_spec));
    assert!(same(&db2.unknown_environment_spec, &db.unknown_environment_spec));
    assert!(same(&db2.unknown_specials_spec, &db.unknown_specials_spec));
}

#[test]
fn test_filter_context_1() {
    let db = filter_db();
    let db2 = db.filter_context(&[], &["cat3"], &[]);
    // this should give 'ccc' from cat2, not cat3
    check_macro(&db2, "ccc", "ccc", "");
    // this should no longer exist
    check_macro(&db2, "ddd", "<macro unknown>", "");

    // this should give 'eccc' from cat2, not cat3
    check_env(&db2, "eccc", "eccc", "");
    // this should no longer exist
    check_env(&db2, "eddd", "<env unknown>", "");
}

#[test]
fn test_filter_context_2() {
    let db = filter_db();
    let db2 = db.filter_context(&["cat1", "cat3"], &["cat3"], &[]);
    // this should give 'aaa' from cat1
    check_macro(&db2, "aaa", "aaa", "{");
    // this should no longer exist
    check_macro(&db2, "ddd", "<macro unknown>", "");

    // this should give 'eaaa' from cat1
    check_env(&db2, "eaaa", "eaaa", "{");
    // this should no longer exist
    check_env(&db2, "eddd", "<env unknown>", "");
}

#[test]
fn test_filter_context_3() {
    let db = filter_db();
    let db2 = db.filter_context(&["cat1", "cat3"], &["cat3"], &["macros"]);
    // this should give 'aaa' from cat1
    check_macro(&db2, "aaa", "aaa", "{");
    // this should no longer exist
    check_macro(&db2, "ddd", "<macro unknown>", "");

    // no environments should exist any longer
    check_env(&db2, "eaaa", "<env unknown>", "");
    check_env(&db2, "eddd", "<env unknown>", "");
}

#[test]
fn test_filter_context_4() {
    let db = filter_db();
    let db2 = db.filter_context(&["cat1", "cat3"], &["cat3"], &["environments"]);
    // no macros should exist any longer
    check_macro(&db2, "aaa", "<macro unknown>", "");
    check_macro(&db2, "ddd", "<macro unknown>", "");

    // this should give 'eaaa' from cat1
    check_env(&db2, "eaaa", "eaaa", "{");
    // this should no longer exist
    check_env(&db2, "eddd", "<env unknown>", "");
}
