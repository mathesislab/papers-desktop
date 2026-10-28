//! pylatexenc's own `test_latexencode.py` and `test_latexencode_all.py` (v2.11), in Rust.
//!
//! Not ported: `test_latex_string_class` (the `latex_string_class` option is not in the Rust API)
//! and the checks of the logged warnings (`assertLogs` is a no-op in the Python file too).

use std::collections::BTreeSet;

use latexenc::latexencode::{
    ConversionRuleSpec, DictRule, PartialLatexToLatexEncoder, RegexRule, ReplacementLatexProtection,
    UnicodeToLatexConversionRule, UnicodeToLatexEncoder, UnknownCharPolicy, get_builtin_conversion_rules, utf8tolatex,
};

/// the input of the `test_basic_*` tests
fn input() -> &'static str {
    "\"\u{c0} votre sant\u{e9}!\" s'exclama le ma\u{ee}tre de maison \u{e0} 100%."
}

fn enc(b: latexenc::latexencode::UnicodeToLatexEncoderBuilder) -> UnicodeToLatexEncoder {
    b.build().unwrap()
}

fn eb() -> latexenc::latexencode::UnicodeToLatexEncoderBuilder {
    UnicodeToLatexEncoder::builder()
}

#[test]
fn test_basic_0() {
    let u = UnicodeToLatexEncoder::new();
    assert_eq!(
        u.unicode_to_latex(input()).unwrap(),
        "''\\`A votre sant\\'e!'' s'exclama le ma{\\^\\i}tre de maison \\`a 100\\%."
    );
}

#[test]
fn test_basic_1() {
    let u = enc(eb().non_ascii_only(true).replacement_latex_protection(ReplacementLatexProtection::BracesAll));
    assert_eq!(
        u.unicode_to_latex(input()).unwrap(),
        "\"{\\`A} votre sant{\\'e}!\" s'exclama le ma{\\^\\i}tre de maison {\\`a} 100%."
    );
}

#[test]
fn test_basic_2() {
    let u = enc(eb().replacement_latex_protection(ReplacementLatexProtection::BracesAfterMacro));
    assert_eq!(
        u.unicode_to_latex(input()).unwrap(),
        "''\\`A votre sant\\'e!'' s'exclama le ma\\^\\i{}tre de maison \\`a 100\\%."
    );
}

#[test]
fn test_basic_2a() {
    // Issue #44
    let u = enc(eb().replacement_latex_protection(ReplacementLatexProtection::BracesAfterMacro));
    assert_eq!(
        u.unicode_to_latex("Jab\u{142}o\u{144}ski, Fran\u{e7}ois, \u{27e8}.\u{27e9}, ~").unwrap(),
        "Jab\\l{}o\\'nski, Fran\\c{c}ois, \\ensuremath{\\langle}.\\ensuremath{\\rangle}, \\textasciitilde{}"
    );
}

#[test]
fn test_basic_2b() {
    let u = enc(eb().replacement_latex_protection(ReplacementLatexProtection::None));
    assert_eq!(
        u.unicode_to_latex(input()).unwrap(),
        "''\\`A votre sant\\'e!'' s'exclama le ma\\^\\itre de maison \\`a 100\\%."
    );
}

const ASCII_CHARS_CONVERT: &str = " \" # $ % & \\ _ { } ~ ";

#[test]
fn test_basic_2c() {
    let u = enc(eb().non_ascii_only(true));
    assert_eq!(u.unicode_to_latex(ASCII_CHARS_CONVERT).unwrap(), ASCII_CHARS_CONVERT);
}

#[test]
fn test_basic_2d() {
    let u = enc(eb().non_ascii_only(false));
    assert_eq!(
        u.unicode_to_latex(ASCII_CHARS_CONVERT).unwrap(),
        " '' \\# \\$ \\% \\& {\\textbackslash} \\_ \\{ \\} {\\textasciitilde} "
    );
}

#[test]
fn test_basic_callable_replacement_latex_protection() {
    let u = enc(eb().replacement_latex_protection(ReplacementLatexProtection::Custom(std::sync::Arc::new(|s| {
        format!("{{***{{{s}}}***}}")
    }))));
    assert_eq!(
        u.unicode_to_latex(input()).unwrap(),
        "{***{''}***}{***{\\`A}***} votre sant{***{\\'e}***}!{***{''}***} s'exclama le ma{***{\\^\\i}***}tre de maison {***{\\`a}***} 100{***{\\%}***}."
    );
}

const THAI: &str = "A unicode character: \u{e18}";

#[test]
fn test_basic_3() {
    // generates warnings -- that's good
    let u = enc(eb().unknown_char_policy(UnknownCharPolicy::Keep));
    assert_eq!(u.unicode_to_latex(THAI).unwrap(), THAI); // unchanged
}

#[test]
fn test_basic_3b() {
    let u = enc(eb().unknown_char_policy(UnknownCharPolicy::Replace));
    assert_eq!(u.unicode_to_latex(THAI).unwrap(), "A unicode character: {\\bfseries ?}");
}

#[test]
fn test_basic_3c() {
    let u = enc(eb().unknown_char_policy(UnknownCharPolicy::Unihex).unknown_char_warning(false));
    assert_eq!(
        u.unicode_to_latex(THAI).unwrap(),
        "A unicode character: \\ensuremath{\\langle}\\texttt{U+0E18}\\ensuremath{\\rangle}"
    );
}

/// the rules of `test_rules_00` and `test_rules_01` that come before the built-in ones
fn rules_head() -> Vec<ConversionRuleSpec> {
    vec![
        UnicodeToLatexConversionRule::dict(DictRule::from_chars([('\u{c0}', r"{{\`{A}}}"), ('%', r"\textpercent")]))
            .into(),
        UnicodeToLatexConversionRule::regex(vec![
            RegexRule::new("v(otre)", r"n\1").unwrap(),
            RegexRule::new("(?i)s'exclama", "s'exprima").unwrap(),
            RegexRule::new("\u{ee}", r"{\^i}").unwrap(),
        ])
        .into(),
    ]
}

/// `acallable` of `test_rules_00`
fn acallable() -> ConversionRuleSpec {
    UnicodeToLatexConversionRule::callable(|_u, input| {
        if input.chars[input.pos] == '\u{e9}' {
            return Ok(Some((1, r"{\'{e}}".to_string())));
        }
        if input.chars[input.pos..].iter().take(3).collect::<String>() == "..." {
            return Ok(Some((3, r"\ldots".to_string())));
        }
        Ok(None)
    })
    .into()
}

fn rules_input() -> &'static str {
    "\"\u{c0} votre sant\u{e9}!\" s'exclama le ma\u{ee}tre de maison ... \u{e0} 100%."
}

#[test]
fn test_rules_00() {
    let mut rules = rules_head();
    rules.extend(get_builtin_conversion_rules("defaults").unwrap().into_iter().map(ConversionRuleSpec::from));
    rules.push(acallable());
    let u = enc(eb().conversion_rules(rules));
    assert_eq!(
        u.unicode_to_latex(rules_input()).unwrap(),
        "''{{\\`{A}}} notre sant\\'e!'' s'exprima le ma{\\^i}tre de maison {\\ldots} \\`a 100{\\textpercent}."
    );
}

#[test]
fn test_rules_01() {
    let mut rules = rules_head();
    rules.push("unicode-xml".into()); // expand built-in rule names
    rules.push(acallable());
    let u = enc(eb().conversion_rules(rules));
    assert_eq!(
        u.unicode_to_latex(rules_input()).unwrap(),
        "\"{{\\`{A}}} notre sant\\'{e}!\" s'exprima le ma{\\^i}tre de maison {\\ldots} \\`{a} 100{\\textpercent}."
    );
}

const RULES_INPUT_2: &str =
    "* \"\u{c0} votre sant\u{e9}!\" s'exclama\u{b2} le ma\u{ee}tre de maison \u{e0} 100%.";

#[test]
fn test_rules_02() {
    // based on test_basic_0()
    let u = enc(eb().conversion_rules(vec!["defaults".into()]));
    assert_eq!(
        u.unicode_to_latex(RULES_INPUT_2).unwrap(),
        "* ''\\`A votre sant\\'e!'' s'exclama{\\texttwosuperior} le ma{\\^\\i}tre de maison \\`a 100\\%."
    );
}

#[test]
fn test_rules_03() {
    let u = enc(eb().conversion_rules(vec!["unicode-xml".into()]));
    assert_eq!(
        u.unicode_to_latex(RULES_INPUT_2).unwrap(),
        "{\\ast} \"\\`{A} votre sant\\'{e}!\" s{\\textquotesingle}exclama{^2} le ma\\^{\\i}tre de maison \\`{a} 100\\%."
    );
}

#[test]
fn test_issue_no21() {
    // test for https://github.com/phfaist/pylatexenc/issues/21
    let acronyms = RegexRule::new(r"\b[A-Z]{2,}\w*\b", "").unwrap().regex;
    let capitalize_acronyms = UnicodeToLatexConversionRule::callable(move |_u, input| {
        let c = input.chars[input.pos];
        if c == '{' || c == '}' {
            // preserve existing braces
            return Ok(Some((1, c.to_string())));
        }
        // `regex.match(s, pos)`: a match that starts at pos
        match acronyms.find_from_pos(input.text, input.byte_pos) {
            Ok(Some(m)) if m.start() == input.byte_pos => {
                Ok(Some((m.as_str().chars().count(), format!("{{{}}}", m.as_str()))))
            }
            _ => Ok(None),
        }
    });
    let mut rules: Vec<ConversionRuleSpec> = vec![capitalize_acronyms.into()];
    rules.extend(get_builtin_conversion_rules("defaults").unwrap().into_iter().map(ConversionRuleSpec::from));
    let u = enc(eb().conversion_rules(rules));
    let input = "Title with {Some} ABC acronyms LIKe this.";
    assert_eq!(u.unicode_to_latex(input).unwrap(), "Title with {Some} {ABC} acronyms {LIKe} this.");

    let mut rules: Vec<ConversionRuleSpec> = vec![
        UnicodeToLatexConversionRule::regex(vec![
            RegexRule::new(r"([{}])", r"\1").unwrap(), // keep existing braces
            RegexRule::new(r"\b([A-Z]{2,}\w*)\b", r"{\1}").unwrap(),
        ])
        .into(),
    ];
    rules.extend(get_builtin_conversion_rules("defaults").unwrap().into_iter().map(ConversionRuleSpec::from));
    let u = enc(eb().conversion_rules(rules));
    assert_eq!(u.unicode_to_latex(input).unwrap(), "Title with {Some} {ABC} acronyms {LIKe} this.");
}

// TestPartialLatexEncode

#[test]
fn test_partial_simple_cases() {
    let u = PartialLatexToLatexEncoder::new();
    assert_eq!(
        u.unicode_to_latex(r"Check ~ \that{macros are fully & 100% \textbf{pr{\'e}s\`ervées}}.").unwrap(),
        r"Check {\textasciitilde} \that{macros are fully \& 100\% \textbf{pr{\'e}s\`erv\'ees}}."
    );
    assert_eq!(
        u.unicode_to_latex(r"{JILA} {SrI} optical lattice clock with uncertainty of $2.0 \times 10^{-18}$").unwrap(),
        r"{JILA} {SrI} optical lattice clock with uncertainty of $2.0 \times 10^{-18}$"
    );
}

#[test]
fn test_partial_custom_conversion_rules() {
    let u = PartialLatexToLatexEncoder::with_options(
        "\\${}^_",
        eb().conversion_rules(vec![
            UnicodeToLatexConversionRule::regex(vec![
                // this rule should never kick in because braces are preserved as keep_latex_chars
                RegexRule::new(r"([{}])", r"BRACE<\1>").unwrap(),
                // other replacement rules
                RegexRule::new(r"\b([A-Z]{2,}\w*)\b", r"{\1}").unwrap(),
            ])
            .into(),
            "defaults".into(),
        ]),
    )
    .unwrap();
    assert_eq!(
        u.unicode_to_latex(r"Title with {Some} ABC àcr\^onyms LIKe this.").unwrap(),
        r"Title with {Some} {ABC} \`acr\^onyms {LIKe} this."
    );
}

// TestUtf8tolatex

#[test]
fn test_utf8tolatex_basic() {
    assert_eq!(
        utf8tolatex(input(), false, true, false, false).unwrap(),
        "''{\\`A} votre sant{\\'e}!'' s'exclama le ma{\\^\\i}tre de maison {\\`a} 100{\\%}."
    );
    assert_eq!(
        utf8tolatex(input(), true, true, false, false).unwrap(),
        "\"{\\`A} votre sant{\\'e}!\" s'exclama le ma{\\^\\i}tre de maison {\\`a} 100%."
    );
    // TODO in pylatexenc: be clever and avoid breaking macros like this ("\itre")
    assert_eq!(
        utf8tolatex(input(), false, false, false, false).unwrap(),
        "''\\`A votre sant\\'e!'' s'exclama le ma\\^\\itre de maison \\`a 100\\%."
    );

    assert_eq!(utf8tolatex(ASCII_CHARS_CONVERT, true, true, false, false).unwrap(), ASCII_CHARS_CONVERT);
    assert_eq!(
        utf8tolatex(ASCII_CHARS_CONVERT, false, true, false, false).unwrap(),
        " '' {\\#} {\\$} {\\%} {\\&} {\\textbackslash} {\\_} {\\{} {\\}} {\\textasciitilde} "
    );

    // generates warnings -- that's good
    assert_eq!(utf8tolatex(THAI, false, true, false, false).unwrap(), THAI); // unchanged
    assert_eq!(
        utf8tolatex(THAI, false, true, true, false).unwrap(),
        "A unicode character: {\\bfseries ?}"
    );
}

// test_latexencode_all.py

/// The characters of the coverage test that have no unicode name: `unicodedata.name()` fails
/// for them, so the Python test does not look at them (`uni_chars_test_previous.txt` has the
/// encoded text of the others, one line each).
const UNNAMED_ENCODABLE: [u32; 25] = [
    0x9, 0xa, 0xd, 0x7f, 0x1d455, 0x1d49d, 0x1d4a0, 0x1d4a1, 0x1d4a3, 0x1d4a4, 0x1d4a7, 0x1d4a8, 0x1d4ad, 0x1d506,
    0x1d50b, 0x1d50c, 0x1d515, 0x1d51d, 0x1d53a, 0x1d53f, 0x1d545, 0x1d547, 0x1d548, 0x1d549, 0x1d551,
];

#[test]
fn test_all() {
    let u = enc(eb()
        .unknown_char_policy(UnknownCharPolicy::Fail)
        .replacement_latex_protection(ReplacementLatexProtection::BracesAlmostAll));
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("uni_chars_test_previous.txt");
    let reference = std::fs::read_to_string(path).unwrap();

    // every line of the reference: "0x%04X %-50s    |%s|\n" with the char encoded
    let mut named: BTreeSet<u32> = BTreeSet::new();
    for line in reference.split_inclusive('\n') {
        let cp = u32::from_str_radix(&line[2..line.find(' ').unwrap()], 16).unwrap();
        let ch = char::from_u32(cp).unwrap();
        // the line the test encodes: the name (ASCII, unchanged by the encoder) and the char
        let bar = line.find('|').unwrap();
        let input_line = format!("{}{}|\n", &line[..=bar], ch);
        assert_eq!(u.unicode_to_latex(&input_line).unwrap(), line, "U+{cp:04X}");
        named.insert(cp);
    }
    assert_eq!(named.len(), 1635);

    // the other characters that can be encoded are only those without a name
    let mut encodable_unnamed = Vec::new();
    for cp in 0..0x110000u32 {
        let Some(ch) = char::from_u32(cp) else { continue };
        if !named.contains(&cp) && u.unicode_to_latex(&ch.to_string()).is_ok() {
            encodable_unnamed.push(cp);
        }
    }
    assert_eq!(encodable_unnamed, UNNAMED_ENCODABLE);
}
