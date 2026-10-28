//! The named configurations of the differential tests: the same as those of
//! `tests/gen_expected.py`, in Rust.

#![allow(dead_code)]

use std::sync::Arc;

use latexenc::latex2text::{
    self, EnvironmentTextSpec, Latex2TextError, LatexNodes2Text, MacroTextSpec, MathMode, Repl, SpecialsTextSpec,
    StrictLatexSpaces, StrictLatexSpacesDict, TextContextDb,
};
use latexenc::latexencode::{
    ConversionRuleSpec, DictRule, LatexEncodeError, PartialLatexToLatexEncoder, RegexRepl, RegexRule,
    ReplacementLatexProtection, Rule, UnicodeToLatexConversionRule, UnicodeToLatexEncoder,
    UnicodeToLatexEncoderBuilder, UnicodeToLatexOptions, UnknownCharPolicy,
};
use latexenc::latexwalker::{LatexWalkerError, LatexWalkerOptions};
use latexenc::macrospec::{
    CategoryPosition, EnvironmentSpec, LatexContextDb, MacroSpec, SpecialsSpec,
};
use serde_json::Value;

pub fn read_json(name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("expected").join(name);
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
}

/// The exception class that the reference raised for a Rust error.
pub fn l2t_err_class(e: &Latex2TextError) -> &'static str {
    match e {
        Latex2TextError::Walker(w) => walker_err_class(w),
        Latex2TextError::Key(_) => "KeyError",
        Latex2TextError::Attribute(_) => "AttributeError",
        Latex2TextError::Value(_) => "ValueError",
        Latex2TextError::Index(_) => "IndexError",
        Latex2TextError::Type(_) => "TypeError",
    }
}

pub fn walker_err_class(e: &LatexWalkerError) -> &'static str {
    match e {
        LatexWalkerError::Parse(_) => "LatexWalkerParseError",
        LatexWalkerError::EndOfStream { .. } => "LatexWalkerEndOfStream",
        LatexWalkerError::Other(m) if m.starts_with("IndexError") => "IndexError",
        LatexWalkerError::Other(m) if m.starts_with("ValueError") => "ValueError",
        LatexWalkerError::Other(_) => "Exception",
    }
}

pub fn enc_err_class(e: &LatexEncodeError) -> &'static str {
    match e {
        LatexEncodeError::Value(_) => "ValueError",
        LatexEncodeError::Walker(w) => walker_err_class(w),
    }
}

// ---------------------------------------------------------------------------------- latex2text

pub fn custom_text_db() -> TextContextDb {
    let mut db = latex2text::get_default_latex_context_db();
    let macros = vec![
        MacroTextSpec::with_repl("url", "%s"),
        MacroTextSpec::with_repl("mymacro", "<%s>"),
        MacroTextSpec::with_repl("textbf", "BOLD(%s)"),
        MacroTextSpec::with_repl("emph", "_%(1)s_"),
        MacroTextSpec::with_repl("two", "%(1)s+%(2)s"),
        MacroTextSpec::with_repl("three", "%(3)s"),
        MacroTextSpec::with_repl("percent", "100%%"),
        MacroTextSpec::with_repl("badfmt", "%d"),
        MacroTextSpec::new("dropit"),
        MacroTextSpec::new("keepargs").discard(false),
        MacroTextSpec::with_repl("hspace", "[SPACE]"),
        MacroTextSpec::with_repl(
            "call",
            Repl::try_func(|n, l| Ok(format!("CALL({})", l.node_arg_to_text(n, 0)?))),
        ),
        MacroTextSpec::with_repl(
            "callname",
            Repl::func(|n, _| format!("NAME={}", n.macroname().unwrap_or(""))),
        ),
        MacroTextSpec::with_repl("callnone", Repl::func(|_, _| String::new())),
        MacroTextSpec::with_repl("ethz", "ETH Z\u{fc}rich"),
    ];
    let envs = vec![
        EnvironmentTextSpec::with_repl("quote", "\"%s\""),
        EnvironmentTextSpec::with_repl("center", "[[%(body)s]]"),
        EnvironmentTextSpec::new("dropenv").discard(true),
        EnvironmentTextSpec::with_repl(
            "callenv",
            Repl::try_func(|n, l| Ok(format!("ENV({})", l.nodelist_to_text(n.nodelist().unwrap())?))),
        ),
        EnvironmentTextSpec::with_repl("badenv", "%(nothere)s"),
    ];
    let specials = vec![
        SpecialsTextSpec::with_repl("--", "DASH"),
        SpecialsTextSpec::with_repl("~", " "),
        SpecialsTextSpec::with_repl("<<", "\u{ab}"),
        SpecialsTextSpec::with_repl(">>", "\u{bb}"),
        SpecialsTextSpec::with_repl("&", "AND"),
    ];
    db.add_context_category("custom", macros, envs, specials, CategoryPosition::Prepend).unwrap();
    db
}

pub fn walker_db() -> LatexContextDb {
    let mut db = latexenc::latexwalker::get_default_latex_context_db();
    let macros = vec![
        MacroSpec::new("mymacro", "{"),
        MacroSpec::new("two", "{{"),
        MacroSpec::new("three", "{{{"),
        MacroSpec::new("call", "[{"),
        MacroSpec::new("keepargs", "*[{"),
        MacroSpec::new("percent", ""),
        MacroSpec::new("badfmt", "{"),
        MacroSpec::new("dropit", "{"),
    ];
    let envs = vec![
        EnvironmentSpec::new("quote", ""),
        EnvironmentSpec::new("callenv", "{"),
        EnvironmentSpec::new("mymath", "").math_mode(true),
    ];
    let specials = vec![SpecialsSpec::new("--"), SpecialsSpec::new("<<"), SpecialsSpec::new(">>")];
    db.add_context_category("custom", macros, envs, specials, CategoryPosition::Prepend).unwrap();
    db
}

/// bibtexparser's `LatexDecodingMiddleware` decoder (`keep_braced_groups`, `keep_math_mode`).
pub fn bibtexparser_decoder(keep_braced_groups: bool, keep_math_mode: bool) -> LatexNodes2Text {
    let mut db = latex2text::get_default_latex_context_db();
    db.add_macro_category(
        "bibtexparse-default-context",
        vec![MacroTextSpec::with_repl("url", "%s")],
        CategoryPosition::Prepend,
    )
    .unwrap();
    LatexNodes2Text::builder()
        .latex_context(db)
        .keep_braced_groups(keep_braced_groups)
        .math_mode(if keep_math_mode { MathMode::Verbatim } else { MathMode::Text })
        .build()
        .unwrap()
}

/// What papers does (`papers.formats._latex_to_unicode`).
pub fn papers_latex_to_unicode(l2t: &LatexNodes2Text, value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    if !value.chars().any(|c| "\\{}$~%&".contains(c)) {
        return latexenc::pystr::strip(value).to_string();
    }
    match l2t.latex_to_text(value) {
        Ok(s) => latexenc::pystr::strip(&s).to_string(),
        Err(_) => value.replace(['{', '}'], ""),
    }
}

pub type L2tFn = Box<dyn Fn(&str) -> Result<String, Latex2TextError>>;

fn conv(l2t: Result<LatexNodes2Text, Latex2TextError>, flags: LatexWalkerOptions) -> Result<L2tFn, Latex2TextError> {
    let l2t = l2t?;
    Ok(Box::new(move |s| l2t.latex_to_text_with(s, flags.clone())))
}

fn l2t(b: latex2text::LatexNodes2TextBuilder) -> Result<LatexNodes2Text, Latex2TextError> {
    b.build()
}

fn b() -> latex2text::LatexNodes2TextBuilder {
    LatexNodes2Text::builder()
}

fn sd() -> StrictLatexSpacesDict {
    StrictLatexSpacesDict::new()
}

/// The configuration of that name (as in `gen_expected.py`): a function from LaTeX to text, or
/// the error of making it.
pub fn l2t_config(name: &str) -> Result<L2tFn, Latex2TextError> {
    use StrictLatexSpaces as S;
    let d = LatexWalkerOptions::default();
    let nontol = || LatexWalkerOptions::default().tolerant_parsing(false);
    match name {
        "default" => conv(l2t(b()), d),
        "default_strict_parse" => conv(l2t(b()), nontol()),
        "default_strict_braces" => conv(l2t(b()), nontol().strict_braces(true)),
        "math_text" => conv(l2t(b().math_mode(MathMode::Text)), d),
        "math_with_delimiters" => conv(l2t(b().math_mode(MathMode::WithDelimiters)), d),
        "math_verbatim" => conv(l2t(b().math_mode(MathMode::Verbatim)), d),
        "math_remove" => conv(l2t(b().math_mode(MathMode::Remove)), d),
        "keep_comments" => conv(l2t(b().keep_comments(true)), d),
        "keep_comments_strict" => conv(l2t(b().keep_comments(true).strict_latex_spaces(S::Bool(true))), d),
        "strict_true" => conv(l2t(b().strict_latex_spaces(S::Bool(true))), d),
        "strict_false" => conv(l2t(b().strict_latex_spaces(S::Bool(false))), d),
        "strict_none" => conv(l2t(b().strict_latex_spaces(S::None)), d),
        "strict_on" => conv(l2t(b().strict_latex_spaces(S::preset("on"))), d),
        "strict_off" => conv(l2t(b().strict_latex_spaces(S::preset("off"))), d),
        "strict_based_on_source" => conv(l2t(b().strict_latex_spaces(S::preset("based-on-source"))), d),
        "strict_macros" => conv(l2t(b().strict_latex_spaces(S::preset("macros"))), d),
        "strict_except_in_equations" => conv(l2t(b().strict_latex_spaces(S::preset("except-in-equations"))), d),
        "strict_default_preset" => conv(l2t(b().strict_latex_spaces(S::preset("default"))), d),
        "strict_invalid_preset" => conv(l2t(b().strict_latex_spaces(S::preset("nope"))), d),
        "strict_dict_a" => conv(l2t(b().strict_latex_spaces(S::Dict(sd().between_macro_and_chars(true)))), d),
        "strict_dict_b" => conv(
            l2t(b().strict_latex_spaces(S::Dict(sd().between_latex_constructs(true).after_comment(true)))),
            d,
        ),
        "strict_dict_c" => conv(
            l2t(b()
                .strict_latex_spaces(S::Dict(sd().between_latex_constructs(true).in_equations(S::Bool(true))))
                .math_mode(MathMode::WithDelimiters)),
            d,
        ),
        "strict_dict_d" => conv(
            l2t(b().strict_latex_spaces(S::Dict(sd().between_macro_and_chars(true).in_equations(S::Dict(
                sd().between_latex_constructs(true).after_comment(true),
            ))))),
            d,
        ),
        "strict_dict_e" => conv(l2t(b().strict_latex_spaces(S::Dict(sd().in_equations(S::preset("macros"))))), d),
        "strict_true_math_with_delimiters" => {
            conv(l2t(b().strict_latex_spaces(S::Bool(true)).math_mode(MathMode::WithDelimiters)), d)
        }
        "strict_macros_math_with_delimiters" => {
            conv(l2t(b().strict_latex_spaces(S::preset("macros")).math_mode(MathMode::WithDelimiters)), d)
        }
        "strict_based_math_verbatim" => {
            conv(l2t(b().strict_latex_spaces(S::preset("based-on-source")).math_mode(MathMode::Verbatim)), d)
        }
        "braced_groups" => conv(l2t(b().keep_braced_groups(true)), d),
        "braced_groups_min1" => conv(l2t(b().keep_braced_groups(true).keep_braced_groups_minlen(1)), d),
        "braced_groups_min0" => conv(l2t(b().keep_braced_groups(true).keep_braced_groups_minlen(0)), d),
        "braced_groups_min5" => conv(l2t(b().keep_braced_groups(true).keep_braced_groups_minlen(5)), d),
        "fill_80" => conv(l2t(b().fill_text(Some(80))), d),
        "fill_40" => conv(l2t(b().fill_text(Some(40))), d),
        "fill_12" => conv(l2t(b().fill_text(Some(12))), d),
        "fill_5" => conv(l2t(b().fill_text(Some(5))), d),
        "fill_40_math_verbatim" => conv(l2t(b().fill_text(Some(40)).math_mode(MathMode::Verbatim)), d),
        "fill_40_math_with_delimiters" => conv(l2t(b().fill_text(Some(40)).math_mode(MathMode::WithDelimiters)), d),
        "fill_40_strict" => conv(l2t(b().fill_text(Some(40)).strict_latex_spaces(S::Bool(true)).keep_comments(true)), d),
        "fill_0" => conv(l2t(b().fill_text(Some(0))), d),
        "all_options" => conv(
            l2t(b()
                .keep_comments(true)
                .keep_braced_groups(true)
                .math_mode(MathMode::WithDelimiters)
                .strict_latex_spaces(S::preset("except-in-equations"))
                .fill_text(Some(30))),
            d,
        ),
        "custom_context" => conv(l2t(b().latex_context(custom_text_db())), d),
        "custom_context_verbatim" => conv(l2t(b().latex_context(custom_text_db()).math_mode(MathMode::Verbatim)), d),
        "custom_context_walker" => {
            conv(l2t(b().latex_context(custom_text_db())), LatexWalkerOptions::default().latex_context(walker_db()))
        }
        "empty_context" => conv(l2t(b().latex_context(TextContextDb::new())), d),
        "bibtexparser" => conv(Ok(bibtexparser_decoder(false, true)), d),
        "bibtexparser_text_math" => conv(Ok(bibtexparser_decoder(false, false)), d),
        "bibtexparser_keep_braces" => conv(Ok(bibtexparser_decoder(true, true)), d),
        "bibtexparser_keep_braces_text_math" => conv(Ok(bibtexparser_decoder(true, false)), d),
        "papers" => {
            let l = l2t(b().math_mode(MathMode::Verbatim))?;
            Ok(Box::new(move |s| Ok(papers_latex_to_unicode(&l, s))))
        }
        other => panic!("no Rust version of the latex2text configuration {other}"),
    }
}

// ---------------------------------------------------------------------------------- latexencode

pub type EncFn = Box<dyn Fn(&str) -> Result<String, LatexEncodeError>>;

fn enc(b: UnicodeToLatexEncoderBuilder) -> Result<EncFn, LatexEncodeError> {
    let e = b.build()?;
    Ok(Box::new(move |s| e.unicode_to_latex(s)))
}

fn eb() -> UnicodeToLatexEncoderBuilder {
    UnicodeToLatexEncoder::builder()
}

fn rules(v: Vec<ConversionRuleSpec>) -> UnicodeToLatexEncoderBuilder {
    eb().conversion_rules(v)
}

fn builtin(n: &str) -> ConversionRuleSpec {
    ConversionRuleSpec::Builtin(n.to_string())
}

fn protection(name: &str) -> Result<ReplacementLatexProtection, LatexEncodeError> {
    name.parse()
}

fn policy(name: &str) -> Result<UnknownCharPolicy, LatexEncodeError> {
    name.parse()
}

/// bibtexparser's `LatexEncodingMiddleware` encoder (`keep_math`, `enclose_urls`).
pub fn bibtexparser_encoder(keep_math: bool, enclose_urls: bool) -> UnicodeToLatexEncoder {
    let mut conversion_rules: Vec<ConversionRuleSpec> = Vec::new();
    if keep_math {
        conversion_rules.push(
            UnicodeToLatexConversionRule::regex(vec![RegexRule::new(r"(?<!\\)(\$.*[^\\]\$)", r"\1").unwrap()]).into(),
        );
    }
    if enclose_urls {
        conversion_rules.push(
            UnicodeToLatexConversionRule::regex(vec![
                RegexRule::new(r"(https?://\S*\.\S*)", r"\\url{\1}").unwrap(),
                RegexRule::new(r"(www.\S*\.\S*)", r"\\url{\1}").unwrap(),
            ])
            .into(),
        );
    }
    conversion_rules.push("defaults".into());
    UnicodeToLatexEncoder::builder().conversion_rules(conversion_rules).build().unwrap()
}

fn dict_rule(pairs: &[(char, &str)], protection: Option<ReplacementLatexProtection>) -> ConversionRuleSpec {
    let rule = Rule::Dict(DictRule::from_chars(pairs.iter().copied()));
    match protection {
        Some(p) => UnicodeToLatexConversionRule::with_protection(rule, p).into(),
        None => UnicodeToLatexConversionRule::new(rule).into(),
    }
}

/// The encoder configuration of that name (as in `gen_expected.py`).
pub fn enc_config(name: &str) -> Result<EncFn, LatexEncodeError> {
    let xml = || builtin("unicode-xml");
    let defaults = || builtin("defaults");
    if let Some(p) = name.strip_prefix("protection_xml_") {
        return enc(rules(vec![xml()]).replacement_latex_protection(protection(p)?));
    }
    match name {
        "defaults" => enc(eb()),
        "function" => Ok(Box::new(|s| Ok(latexenc::latexencode::unicode_to_latex(s)))),
        "function_non_ascii_only" => Ok(Box::new(|s| {
            latexenc::latexencode::unicode_to_latex_with(
                s,
                &UnicodeToLatexOptions { non_ascii_only: true, ..Default::default() },
            )
        })),
        "function_options" => Ok(Box::new(|s| {
            latexenc::latexencode::unicode_to_latex_with(
                s,
                &UnicodeToLatexOptions {
                    replacement_latex_protection: ReplacementLatexProtection::BracesAll,
                    unknown_char_policy: UnknownCharPolicy::Replace,
                    unknown_char_warning: false,
                    ..Default::default()
                },
            )
        })),
        "unicode_xml" => enc(rules(vec![xml()])),
        "unicode_xml_defaults" => enc(rules(vec![xml(), defaults()])),
        "defaults_unicode_xml" => enc(rules(vec![defaults(), xml()])),
        "non_ascii_only" => enc(eb().non_ascii_only(true)),
        "non_ascii_only_xml" => enc(rules(vec![xml()]).non_ascii_only(true)),
        "protection_none" => enc(eb().replacement_latex_protection(protection("none")?)),
        "protection_braces" => enc(eb().replacement_latex_protection(protection("braces")?)),
        "protection_braces_almost_all" => enc(eb().replacement_latex_protection(protection("braces-almost-all")?)),
        "protection_braces_all" => enc(eb().replacement_latex_protection(protection("braces-all")?)),
        "protection_braces_after_macro" => {
            enc(eb().replacement_latex_protection(protection("braces-after-macro")?))
        }
        "protection_underscore" => enc(eb().replacement_latex_protection(protection("braces_all")?)),
        "protection_invalid" => enc(eb().replacement_latex_protection(protection("nope")?)),
        "protection_callable" => enc(eb().replacement_latex_protection(ReplacementLatexProtection::Custom(
            Arc::new(|r| format!("<{r}>")),
        ))),
        "unknown_keep" => enc(eb().unknown_char_policy(policy("keep")?)),
        "unknown_replace" => enc(eb().unknown_char_policy(policy("replace")?)),
        "unknown_ignore" => enc(eb().unknown_char_policy(policy("ignore")?)),
        "unknown_fail" => enc(eb().unknown_char_policy(policy("fail")?)),
        "unknown_unihex" => enc(eb().unknown_char_policy(policy("unihex")?)),
        "unknown_invalid" => enc(eb().unknown_char_policy(policy("nope")?)),
        "unknown_callable" => enc(eb().unknown_char_policy(UnknownCharPolicy::Custom(Arc::new(|_, ch| {
            Ok(format!("[U+{:04X}]", ch as u32))
        })))),
        "unknown_callable_u2l" => enc(eb().unknown_char_policy(UnknownCharPolicy::Custom(Arc::new(|_, _| {
            Ok("<UnicodeToLatexEncoder>".to_string())
        })))),
        "unknown_nowarn" => enc(eb().unknown_char_warning(false).unknown_char_policy(policy("unihex")?)),
        "invalid_builtin" => enc(rules(vec![builtin("nope")])),
        "bibtexparser" => {
            let e = bibtexparser_encoder(true, true);
            Ok(Box::new(move |s| e.unicode_to_latex(s)))
        }
        n if n.starts_with("bibtexparser_math") => {
            let km = n.contains("math1");
            let eu = n.contains("urls1");
            let e = bibtexparser_encoder(km, eu);
            Ok(Box::new(move |s| e.unicode_to_latex(s)))
        }
        "rule_dict" => enc(rules(vec![
            dict_rule(&[('\u{e9}', "E-ACUTE"), ('x', "\\times"), ('{', "<")], None),
            defaults(),
        ])),
        "rule_dict_alone" => enc(rules(vec![dict_rule(&[('\u{e9}', "E-ACUTE"), ('x', "\\times")], None)])),
        "rule_dict_protection" => enc(rules(vec![
            dict_rule(&[('\u{e9}', "\\acute e"), ('x', "\\x")], Some(ReplacementLatexProtection::BracesAll)),
            defaults(),
        ])),
        "rule_dict_protection_none" => enc(rules(vec![
            dict_rule(&[('\u{e9}', "\\acute"), ('x', "\\x")], Some(ReplacementLatexProtection::None)),
            defaults(),
        ])),
        "rule_regex_template" => enc(rules(vec![
            UnicodeToLatexConversionRule::regex(vec![
                RegexRule::new(r"(?P<word>(?<![A-Za-z0-9_])[A-Z]{2,}(?![A-Za-z0-9_]))", r"\\textsc{\g<word>}")?,
                RegexRule::new(r"(\d+)-(\d+)", r"\1\\textendash{}\2")?,
                RegexRule::new(r"\\([A-Za-z0-9_]+)", r"<\1>")?,
                RegexRule::new(r"\u00e9+", r"e\\n\\\\x")?,
            ])
            .into(),
            defaults(),
        ])),
        "rule_regex_callable" => enc(rules(vec![
            UnicodeToLatexConversionRule::regex(vec![RegexRule::with_func(r"[aeiou]+", |m| {
                m.get(0).unwrap().as_str().to_uppercase()
            })?])
            .into(),
            defaults(),
        ])),
        "rule_regex_lookbehind" => enc(rules(vec![
            UnicodeToLatexConversionRule::regex(vec![
                RegexRule::with_func(r"(?<![\\a-z])a[a-z]*", |m| format!("[{}]", m.get(0).unwrap().as_str()))?,
                RegexRule::new(r"^x", "X")?,
            ])
            .into(),
            defaults(),
        ])),
        "rule_regex_group_error" => enc(rules(vec![
            UnicodeToLatexConversionRule::regex(vec![RegexRule::new(r"q", r"\1")?]).into(),
            defaults(),
        ])),
        "rule_callable" => enc(rules(vec![
            UnicodeToLatexConversionRule::callable(|_, input| {
                let s: String = input.chars[input.pos..].iter().take(2).collect();
                if s == "ab" {
                    return Ok(Some((2, "AB".to_string())));
                }
                if input.chars[input.pos] == '\u{e9}' {
                    return Ok(Some((1, "\\acute{e}".to_string())));
                }
                Ok(None)
            })
            .into(),
            defaults(),
        ])),
        "rule_callable_u2l" => enc(rules(vec![
            UnicodeToLatexConversionRule::callable(|_, input| {
                if input.chars[input.pos] == 'z' {
                    return Ok(Some((1, "zUni".to_string())));
                }
                Ok(None)
            })
            .into(),
            defaults(),
        ])),
        "rule_callable_many" => enc(rules(vec![
            UnicodeToLatexConversionRule::callable(|_, input| {
                let s: String = input.chars[input.pos..].iter().take(3).collect();
                Ok((s == "abc").then(|| (3, "X".to_string())))
            })
            .into(),
            dict_rule(&[('a', "A")], None),
            defaults(),
        ])),
        "partial" => {
            let e = PartialLatexToLatexEncoder::new();
            Ok(Box::new(move |s| e.unicode_to_latex(s)))
        }
        "partial_xml" => {
            let e = PartialLatexToLatexEncoder::with_options("\\${}^_", rules(vec![xml()]))?;
            Ok(Box::new(move |s| e.unicode_to_latex(s)))
        }
        "partial_keep_dollar" => {
            let e = PartialLatexToLatexEncoder::with_options("$", eb())?;
            Ok(Box::new(move |s| e.unicode_to_latex(s)))
        }
        "partial_keep_braces" => {
            let e = PartialLatexToLatexEncoder::with_options("{}", eb().non_ascii_only(true))?;
            Ok(Box::new(move |s| e.unicode_to_latex(s)))
        }
        "partial_protection" => {
            let e = PartialLatexToLatexEncoder::with_options(
                "\\${}^_",
                eb().replacement_latex_protection(ReplacementLatexProtection::BracesAll),
            )?;
            Ok(Box::new(move |s| e.unicode_to_latex(s)))
        }
        "utf8tolatex" => Ok(Box::new(|s| latexenc::latexencode::utf8tolatex(s, false, true, false, false))),
        "utf8tolatex_non_ascii_only" => {
            Ok(Box::new(|s| latexenc::latexencode::utf8tolatex(s, true, true, false, false)))
        }
        "utf8tolatex_nobrackets" => {
            Ok(Box::new(|s| latexenc::latexencode::utf8tolatex(s, false, false, false, false)))
        }
        "utf8tolatex_substitute" => {
            Ok(Box::new(|s| latexenc::latexencode::utf8tolatex(s, false, true, true, false)))
        }
        "utf8tolatex_fail" => Ok(Box::new(|s| latexenc::latexencode::utf8tolatex(s, false, true, false, true))),
        other => panic!("no Rust version of the latexencode configuration {other}"),
    }
}

#[allow(unused)]
fn _unused(_: RegexRepl) {}
