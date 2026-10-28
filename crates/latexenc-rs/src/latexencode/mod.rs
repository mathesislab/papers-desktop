//! Encode unicode text as LaTeX (port of `pylatexenc.latexencode`).
//!
//! ```
//! use latexenc::latexencode::{UnicodeToLatexEncoder, UnicodeToLatexConversionRule, ConversionRuleSpec};
//!
//! assert_eq!(latexenc::latexencode::unicode_to_latex("é–α"), r"\'e{\textendash}\ensuremath{\alpha}");
//!
//! // use the 'unicode-xml' table
//! let enc = UnicodeToLatexEncoder::builder()
//!     .conversion_rules(vec![ConversionRuleSpec::Builtin("unicode-xml".into())])
//!     .build()
//!     .unwrap();
//! assert_eq!(enc.unicode_to_latex("é").unwrap(), r"\'{e}");
//! ```

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use unicode_normalization::UnicodeNormalization;

mod partial;
mod uni2latexmap;
mod uni2latexmap_xml;

pub use partial::PartialLatexToLatexEncoder;

use crate::pystr::str_is_alpha;

/// What can go wrong when encoding: the options are not valid, or the policy for unknown chars
/// is `fail`, or a rule fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LatexEncodeError {
    /// Python's `ValueError`/`TypeError`
    Value(String),
    /// the walker used by a rule failed
    Walker(crate::latexwalker::LatexWalkerError),
}

impl fmt::Display for LatexEncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LatexEncodeError::Value(m) => f.write_str(m),
            LatexEncodeError::Walker(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for LatexEncodeError {}

type Res<T> = Result<T, LatexEncodeError>;

/// `get_builtin_uni2latex_dict()`: the table of the `defaults` rule, sorted by code point.
pub fn get_builtin_uni2latex_dict() -> &'static [(u32, &'static str)] {
    uni2latexmap::TABLE
}

/// The table of the `unicode-xml` rule, sorted by code point.
pub fn get_builtin_uni2latex_xml_dict() -> &'static [(u32, &'static str)] {
    uni2latexmap_xml::TABLE
}

/// A dictionary rule: code points to replacements (`RULE_DICT`).
#[derive(Debug, Clone)]
pub enum DictRule {
    /// a built-in table, sorted by code point
    Static(&'static [(u32, &'static str)]),
    Map(HashMap<u32, String>),
}

impl DictRule {
    /// From pairs of chars and replacements.
    pub fn from_chars<'a>(pairs: impl IntoIterator<Item = (char, &'a str)>) -> Self {
        DictRule::Map(pairs.into_iter().map(|(c, r)| (c as u32, r.to_string())).collect())
    }

    fn get(&self, o: u32) -> Option<&str> {
        match self {
            DictRule::Static(t) => t.binary_search_by_key(&o, |&(k, _)| k).ok().map(|i| t[i].1),
            DictRule::Map(m) => m.get(&o).map(String::as_str),
        }
    }
}

/// How the replacement of a regex rule is made from the match.
#[derive(Clone)]
pub enum RegexRepl {
    /// a template as for Python's `m.expand()`: `\1`, `\g<name>`, `\\`, `\n`, ...
    Template(String),
    /// a function of the match (a callable in Python)
    Func(Arc<dyn Fn(&fancy_regex::Captures<'_>) -> String + Send + Sync>),
}

impl fmt::Debug for RegexRepl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegexRepl::Template(t) => write!(f, "{t:?}"),
            RegexRepl::Func(_) => f.write_str("<function>"),
        }
    }
}

/// One `(compiled_regular_expression, replacement)` of a regex rule (`RULE_REGEX`).
///
/// The regex is matched at the current position (as Python's `regex.match(s, pos)`); lookbehind
/// sees the text before it. Regexes are those of the [`fancy_regex`] crate, close to Python's
/// `re` (lookaround, backreferences); the classes like `\w` follow Unicode properties, which
/// differ a little from Python's.
#[derive(Debug, Clone)]
pub struct RegexRule {
    pub regex: fancy_regex::Regex,
    pub repl: RegexRepl,
}

impl RegexRule {
    /// A rule with a template replacement. Fails if the pattern does not compile.
    pub fn new(pattern: &str, template: &str) -> Res<Self> {
        let regex = fancy_regex::Regex::new(pattern).map_err(|e| LatexEncodeError::Value(e.to_string()))?;
        Ok(RegexRule { regex, repl: RegexRepl::Template(template.to_string()) })
    }

    /// A rule with a replacement function.
    pub fn with_func<F>(pattern: &str, f: F) -> Res<Self>
    where
        F: Fn(&fancy_regex::Captures<'_>) -> String + Send + Sync + 'static,
    {
        let regex = fancy_regex::Regex::new(pattern).map_err(|e| LatexEncodeError::Value(e.to_string()))?;
        Ok(RegexRule { regex, repl: RegexRepl::Func(Arc::new(f)) })
    }
}

/// The text and position given to a callable rule.
#[derive(Debug, Clone, Copy)]
pub struct EncodeInput<'a> {
    /// the whole text (after NFC normalization)
    pub text: &'a str,
    /// the same as chars
    pub chars: &'a [char],
    /// the current position, in chars
    pub pos: usize,
    /// the current position, in bytes of `text`
    pub byte_pos: usize,
}

/// A callable rule: gets the encoder and the position; returns `Some((number of chars consumed,
/// replacement))`, or `None` if it does not apply here.
pub type RuleCallable =
    dyn Fn(&UnicodeToLatexEncoder, &EncodeInput<'_>) -> Res<Option<(usize, String)>> + Send + Sync;

/// What a conversion rule is (`rule_type` and `rule`).
#[derive(Clone)]
pub enum Rule {
    /// `RULE_DICT`: a dictionary of code points to replacements
    Dict(DictRule),
    /// `RULE_REGEX`: a list of `(regex, replacement)`; the first that matches is used
    Regex(Vec<RegexRule>),
    /// `RULE_CALLABLE`
    Callable(Arc<RuleCallable>),
}

impl fmt::Debug for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rule::Dict(_) => f.write_str("Dict(..)"),
            Rule::Regex(r) => write!(f, "Regex({r:?})"),
            Rule::Callable(_) => f.write_str("Callable(..)"),
        }
    }
}

/// How the replacements are protected against a following letter (`replacement_latex_protection`).
#[derive(Clone)]
pub enum ReplacementLatexProtection {
    /// `none`: as they are
    None,
    /// `braces`: `{\macro}` if the replacement ends with a macro with a name (the default)
    Braces,
    /// `braces-almost-all`: in braces if it starts with a backslash
    BracesAlmostAll,
    /// `braces-all`: always in braces
    BracesAll,
    /// `braces-after-macro`: `\macro{}` if it ends with a macro with a name
    BracesAfterMacro,
    /// a function from the replacement to the protected replacement
    Custom(Arc<dyn Fn(&str) -> String + Send + Sync>),
}

impl fmt::Debug for ReplacementLatexProtection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplacementLatexProtection::None => f.write_str("None"),
            ReplacementLatexProtection::Braces => f.write_str("Braces"),
            ReplacementLatexProtection::BracesAlmostAll => f.write_str("BracesAlmostAll"),
            ReplacementLatexProtection::BracesAll => f.write_str("BracesAll"),
            ReplacementLatexProtection::BracesAfterMacro => f.write_str("BracesAfterMacro"),
            ReplacementLatexProtection::Custom(_) => f.write_str("Custom(..)"),
        }
    }
}

impl std::str::FromStr for ReplacementLatexProtection {
    type Err = LatexEncodeError;
    /// From the Python names (`braces-all`, or `braces_all`).
    fn from_str(s: &str) -> Res<Self> {
        match s.replace('-', "_").as_str() {
            "none" => Ok(Self::None),
            "braces" => Ok(Self::Braces),
            "braces_almost_all" => Ok(Self::BracesAlmostAll),
            "braces_all" => Ok(Self::BracesAll),
            "braces_after_macro" => Ok(Self::BracesAfterMacro),
            _ => Err(LatexEncodeError::Value(format!("Invalid replacement_latex_protection: {s}"))),
        }
    }
}

impl ReplacementLatexProtection {
    fn apply(&self, repl: &str) -> String {
        // k = repl.rfind('\\'); k >= 0 and repl[k+1:].isalpha()
        let ends_with_named_macro = || match repl.rfind('\\') {
            Some(k) => str_is_alpha(&repl[k + 1..]),
            None => false,
        };
        match self {
            Self::None => repl.to_string(),
            Self::Braces => {
                if ends_with_named_macro() {
                    format!("{{{repl}}}")
                } else {
                    repl.to_string()
                }
            }
            Self::BracesAlmostAll => {
                if repl.starts_with('\\') {
                    format!("{{{repl}}}")
                } else {
                    repl.to_string()
                }
            }
            Self::BracesAll => format!("{{{repl}}}"),
            Self::BracesAfterMacro => {
                if ends_with_named_macro() {
                    format!("{repl}{{}}")
                } else {
                    repl.to_string()
                }
            }
            Self::Custom(f) => f(repl),
        }
    }
}

/// The type of a custom `unknown_char_policy`: gets the encoder and the char, returns the LaTeX.
pub type UnknownCharFn = dyn Fn(&UnicodeToLatexEncoder, char) -> Res<String> + Send + Sync;

/// What to do with a char that no rule knows (`unknown_char_policy`).
#[derive(Clone)]
pub enum UnknownCharPolicy {
    /// `keep`: leave the char as it is (the default)
    Keep,
    /// `replace`: `{\bfseries ?}`
    Replace,
    /// `ignore`: drop it
    Ignore,
    /// `fail`: an error
    Fail,
    /// `unihex`: `\ensuremath{\langle}\texttt{U+XXXX}\ensuremath{\rangle}`
    Unihex,
    /// a function of the encoder and the char
    Custom(Arc<UnknownCharFn>),
}

impl fmt::Debug for UnknownCharPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnknownCharPolicy::Keep => f.write_str("Keep"),
            UnknownCharPolicy::Replace => f.write_str("Replace"),
            UnknownCharPolicy::Ignore => f.write_str("Ignore"),
            UnknownCharPolicy::Fail => f.write_str("Fail"),
            UnknownCharPolicy::Unihex => f.write_str("Unihex"),
            UnknownCharPolicy::Custom(_) => f.write_str("Custom(..)"),
        }
    }
}

impl std::str::FromStr for UnknownCharPolicy {
    type Err = LatexEncodeError;
    /// From the Python names: `keep`, `replace`, `ignore`, `fail`, `unihex`.
    fn from_str(s: &str) -> Res<Self> {
        match s.replace('-', "_").as_str() {
            "keep" => Ok(Self::Keep),
            "replace" => Ok(Self::Replace),
            "ignore" => Ok(Self::Ignore),
            "fail" => Ok(Self::Fail),
            "unihex" => Ok(Self::Unihex),
            _ => Err(LatexEncodeError::Value(format!("Invalid unknown_char_policy: {s}"))),
        }
    }
}

/// `UnicodeToLatexConversionRule(rule_type, rule, replacement_latex_protection)`
#[derive(Debug, Clone)]
pub struct UnicodeToLatexConversionRule {
    pub rule: Rule,
    /// the protection for the replacements of this rule (default: the one of the encoder)
    pub replacement_latex_protection: Option<ReplacementLatexProtection>,
}

impl UnicodeToLatexConversionRule {
    /// A rule with the protection of the encoder.
    pub fn new(rule: Rule) -> Self {
        UnicodeToLatexConversionRule { rule, replacement_latex_protection: None }
    }

    /// A rule with its own protection.
    pub fn with_protection(rule: Rule, protection: ReplacementLatexProtection) -> Self {
        UnicodeToLatexConversionRule { rule, replacement_latex_protection: Some(protection) }
    }

    /// `RULE_DICT` rule
    pub fn dict(d: DictRule) -> Self {
        Self::new(Rule::Dict(d))
    }

    /// `RULE_REGEX` rule
    pub fn regex(rules: Vec<RegexRule>) -> Self {
        Self::new(Rule::Regex(rules))
    }

    /// `RULE_CALLABLE` rule
    pub fn callable<F>(f: F) -> Self
    where
        F: Fn(&UnicodeToLatexEncoder, &EncodeInput<'_>) -> Res<Option<(usize, String)>> + Send + Sync + 'static,
    {
        Self::new(Rule::Callable(Arc::new(f)))
    }
}

/// `get_builtin_conversion_rules(builtin_name)`: the rules of a built-in set, `defaults` or
/// `unicode-xml`.
pub fn get_builtin_conversion_rules(builtin_name: &str) -> Res<Vec<UnicodeToLatexConversionRule>> {
    match builtin_name {
        "defaults" => Ok(vec![UnicodeToLatexConversionRule::dict(DictRule::Static(uni2latexmap::TABLE))]),
        "unicode-xml" => Ok(vec![UnicodeToLatexConversionRule::dict(DictRule::Static(uni2latexmap_xml::TABLE))]),
        _ => Err(LatexEncodeError::Value(format!("Unknown builtin rule set: {builtin_name}"))),
    }
}

/// An element of `conversion_rules`: the name of a built-in set, or a rule.
#[derive(Debug, Clone)]
pub enum ConversionRuleSpec {
    /// `defaults` or `unicode-xml`
    Builtin(String),
    Rule(UnicodeToLatexConversionRule),
}

impl From<&str> for ConversionRuleSpec {
    fn from(s: &str) -> Self {
        ConversionRuleSpec::Builtin(s.to_string())
    }
}

impl From<UnicodeToLatexConversionRule> for ConversionRuleSpec {
    fn from(r: UnicodeToLatexConversionRule) -> Self {
        ConversionRuleSpec::Rule(r)
    }
}

/// The options of [`UnicodeToLatexEncoder`] (`UnicodeToLatexEncoder(**kwargs)`).
#[derive(Debug, Clone)]
pub struct UnicodeToLatexEncoderBuilder {
    non_ascii_only: bool,
    conversion_rules: Vec<ConversionRuleSpec>,
    replacement_latex_protection: ReplacementLatexProtection,
    unknown_char_policy: UnknownCharPolicy,
    unknown_char_warning: bool,
}

impl Default for UnicodeToLatexEncoderBuilder {
    fn default() -> Self {
        UnicodeToLatexEncoderBuilder {
            non_ascii_only: false,
            conversion_rules: vec![ConversionRuleSpec::Builtin("defaults".to_string())],
            replacement_latex_protection: ReplacementLatexProtection::Braces,
            unknown_char_policy: UnknownCharPolicy::Keep,
            unknown_char_warning: true,
        }
    }
}

impl UnicodeToLatexEncoderBuilder {
    /// `non_ascii_only`: leave the ASCII chars (below 127) alone, without looking at the rules.
    pub fn non_ascii_only(mut self, v: bool) -> Self {
        self.non_ascii_only = v;
        self
    }
    /// `conversion_rules`: the rules, in order of precedence (default: `["defaults"]`).
    pub fn conversion_rules(mut self, v: Vec<ConversionRuleSpec>) -> Self {
        self.conversion_rules = v;
        self
    }
    /// Add a rule before all the others.
    pub fn prepend_rule(mut self, r: ConversionRuleSpec) -> Self {
        self.conversion_rules.insert(0, r);
        self
    }
    /// `replacement_latex_protection` (default `braces`)
    pub fn replacement_latex_protection(mut self, v: ReplacementLatexProtection) -> Self {
        self.replacement_latex_protection = v;
        self
    }
    /// `unknown_char_policy` (default `keep`)
    pub fn unknown_char_policy(mut self, v: UnknownCharPolicy) -> Self {
        self.unknown_char_policy = v;
        self
    }
    /// `unknown_char_warning`: log a warning for each unknown char (default `true`)
    pub fn unknown_char_warning(mut self, v: bool) -> Self {
        self.unknown_char_warning = v;
        self
    }
    /// Make the encoder. Fails if a built-in rule set is unknown.
    pub fn build(self) -> Res<UnicodeToLatexEncoder> {
        let mut rules = Vec::new();
        for spec in self.conversion_rules {
            match spec {
                ConversionRuleSpec::Builtin(name) => rules.extend(get_builtin_conversion_rules(&name)?),
                ConversionRuleSpec::Rule(r) => rules.push(r),
            }
        }
        Ok(UnicodeToLatexEncoder {
            non_ascii_only: self.non_ascii_only,
            rules,
            replacement_latex_protection: self.replacement_latex_protection,
            unknown_char_policy: self.unknown_char_policy,
            unknown_char_warning: self.unknown_char_warning,
        })
    }
}

/// `UnicodeToLatexEncoder`: converts unicode text to LaTeX.
///
/// The text is normalized to NFC; then, at each position, the conversion rules are tried in
/// order; the first that applies gives the replacement (protected as set); else ASCII chars
/// (and newline, tab) are kept, and the other chars go to the `unknown_char_policy`.
#[derive(Debug, Clone)]
pub struct UnicodeToLatexEncoder {
    /// only encode the non-ASCII chars
    pub non_ascii_only: bool,
    /// the conversion rules (`conversion_rules`, with the built-in sets expanded)
    pub rules: Vec<UnicodeToLatexConversionRule>,
    pub replacement_latex_protection: ReplacementLatexProtection,
    pub unknown_char_policy: UnknownCharPolicy,
    pub unknown_char_warning: bool,
}

impl Default for UnicodeToLatexEncoder {
    fn default() -> Self {
        Self::new()
    }
}

/// The state of a regex rule during one conversion: the next match found.
enum NextMatch<'t> {
    Unknown,
    NoMore,
    At(usize, fancy_regex::Captures<'t>),
}

impl UnicodeToLatexEncoder {
    /// `UnicodeToLatexEncoder()`: the `defaults` rules.
    pub fn new() -> Self {
        UnicodeToLatexEncoderBuilder::default().build().expect("the defaults are valid")
    }

    /// The options, to set as for `UnicodeToLatexEncoder(...)`.
    pub fn builder() -> UnicodeToLatexEncoderBuilder {
        UnicodeToLatexEncoderBuilder::default()
    }

    /// `unicode_to_latex(s)`
    pub fn unicode_to_latex(&self, s: &str) -> Res<String> {
        let s: String = s.nfc().collect();
        let chars: Vec<char> = s.chars().collect();
        let mut latex = String::new();
        let mut pos = 0usize;
        let mut byte_pos = 0usize;
        // regex rules: the next match of each regex (an unanchored search finds the leftmost
        // match from a position, which is also the match at its start)
        let mut nexts: Vec<Vec<NextMatch>> = self
            .rules
            .iter()
            .map(|r| match &r.rule {
                Rule::Regex(rr) => rr.iter().map(|_| NextMatch::Unknown).collect(),
                _ => Vec::new(),
            })
            .collect();

        while pos < chars.len() {
            if self.non_ascii_only && (chars[pos] as u32) < 127 {
                latex.push(chars[pos]);
                byte_pos += chars[pos].len_utf8();
                pos += 1;
                continue;
            }
            let input = EncodeInput { text: &s, chars: &chars, pos, byte_pos };
            let mut applied: Option<(usize, String, &UnicodeToLatexConversionRule)> = None;
            for (ri, rule) in self.rules.iter().enumerate() {
                let r = match &rule.rule {
                    Rule::Dict(d) => d.get(chars[pos] as u32).map(|r| (1, r.to_string())),
                    Rule::Regex(items) => self.apply_rule_regex(items, &mut nexts[ri], &input)?,
                    Rule::Callable(f) => f(self, &input)?,
                };
                if let Some((n, repl)) = r {
                    applied = Some((n, repl, rule));
                    break;
                }
            }
            match applied {
                Some((numchars, repl, rule)) => {
                    let protected = match &rule.replacement_latex_protection {
                        Some(p) => p.apply(&repl),
                        None => self.replacement_latex_protection.apply(&repl),
                    };
                    latex.push_str(&protected);
                    let end = (pos + numchars).min(chars.len());
                    byte_pos += chars[pos..end].iter().map(|c| c.len_utf8()).sum::<usize>();
                    pos += numchars;
                }
                None => {
                    let ch = chars[pos];
                    let o = ch as u32;
                    if (32..=127).contains(&o) || matches!(ch, '\n' | '\r' | '\t') {
                        latex.push(ch);
                    } else {
                        if self.unknown_char_warning {
                            log::warn!("No known latex representation for character: U+{o:04X} - \u{2018}{ch}\u{2019}");
                        }
                        latex.push_str(&self.do_unknown_char(ch)?);
                    }
                    byte_pos += ch.len_utf8();
                    pos += 1;
                }
            }
        }
        Ok(latex)
    }

    fn apply_rule_regex<'t>(
        &self,
        items: &[RegexRule],
        nexts: &mut [NextMatch<'t>],
        input: &EncodeInput<'t>,
    ) -> Res<Option<(usize, String)>> {
        for (item, next) in items.iter().zip(nexts.iter_mut()) {
            let stale = match next {
                NextMatch::Unknown => true,
                NextMatch::NoMore => false,
                NextMatch::At(start, _) => *start < input.byte_pos,
            };
            if stale {
                *next = match item.regex.captures_from_pos(input.text, input.byte_pos) {
                    Ok(Some(c)) => {
                        let m = c.get(0).expect("group 0");
                        NextMatch::At(m.start(), c)
                    }
                    Ok(None) => NextMatch::NoMore,
                    Err(e) => {
                        log::warn!("Regex rule failed: {e}");
                        NextMatch::NoMore
                    }
                };
            }
            if let NextMatch::At(start, caps) = next
                && *start == input.byte_pos {
                    let m = caps.get(0).expect("group 0");
                    let consumed = input.text[m.start()..m.end()].chars().count();
                    if consumed == 0 {
                        // Python would loop forever on an empty match; it is not a match here
                        continue;
                    }
                    let repl = match &item.repl {
                        RegexRepl::Func(f) => f(caps),
                        RegexRepl::Template(t) => {
                            expand_template(t, &item.regex, caps).map_err(LatexEncodeError::Value)?
                        }
                    };
                    return Ok(Some((consumed, repl)));
                }
        }
        Ok(None)
    }

    fn do_unknown_char(&self, ch: char) -> Res<String> {
        match &self.unknown_char_policy {
            UnknownCharPolicy::Keep => Ok(ch.to_string()),
            UnknownCharPolicy::Replace => Ok("{\\bfseries ?}".to_string()),
            UnknownCharPolicy::Ignore => Ok(String::new()),
            UnknownCharPolicy::Fail => Err(LatexEncodeError::Value(format!(
                "No known latex representation for character: U+{:04X} - \u{2018}{}\u{2019}",
                ch as u32, ch
            ))),
            UnknownCharPolicy::Unihex => {
                Ok(format!("\\ensuremath{{\\langle}}\\texttt{{U+{:04X}}}\\ensuremath{{\\rangle}}", ch as u32))
            }
            UnknownCharPolicy::Custom(f) => f(self, ch),
        }
    }
}

/// Python's `m.expand(template)`.
fn expand_template(t: &str, regex: &fancy_regex::Regex, caps: &fancy_regex::Captures<'_>) -> Result<String, String> {
    let c: Vec<char> = t.chars().collect();
    let mut out = String::new();
    let group = |i: usize| -> Result<&str, String> {
        if i >= caps.len() {
            return Err(format!("invalid group reference {i}"));
        }
        Ok(caps.get(i).map(|m| m.as_str()).unwrap_or(""))
    };
    let mut i = 0;
    while i < c.len() {
        if c[i] != '\\' {
            out.push(c[i]);
            i += 1;
            continue;
        }
        i += 1;
        let Some(&e) = c.get(i) else { return Err("bad escape (end of pattern)".to_string()) };
        i += 1;
        match e {
            'g' => {
                if c.get(i) != Some(&'<') {
                    return Err("missing <".to_string());
                }
                let Some(end) = c[i..].iter().position(|&x| x == '>') else {
                    return Err("missing >, unterminated name".to_string());
                };
                let name: String = c[i + 1..i + end].iter().collect();
                i += end + 1;
                if let Ok(n) = name.parse::<usize>() {
                    out.push_str(group(n)?);
                } else {
                    if !regex.capture_names().any(|n| n == Some(name.as_str())) {
                        return Err(format!("unknown group name '{name}'"));
                    }
                    out.push_str(caps.name(&name).map(|m| m.as_str()).unwrap_or(""));
                }
            }
            '0' => {
                // octal escape: \0 and up to two more octal digits
                let mut v = 0u32;
                let mut n = 0;
                while n < 2 && c.get(i).is_some_and(|d| ('0'..='7').contains(d)) {
                    v = v * 8 + c[i].to_digit(8).unwrap();
                    i += 1;
                    n += 1;
                }
                out.push(char::from_u32(v).unwrap_or('\0'));
            }
            '1'..='9' => {
                let d1 = e.to_digit(10).unwrap();
                match c.get(i) {
                    Some(&d) if d.is_ascii_digit() => {
                        let d2 = d.to_digit(10).unwrap();
                        if e.is_digit(8) && d.is_digit(8) && c.get(i + 1).is_some_and(|x| ('0'..='7').contains(x)) {
                            // three octal digits
                            let d3 = c[i + 1].to_digit(8).unwrap();
                            let v = d1 * 64 + d2 * 8 + d3;
                            if v > 0o377 {
                                return Err(format!("octal escape value \\{d1}{d2}{d3} outside of range 0-0o377"));
                            }
                            out.push(char::from_u32(v).unwrap_or('\0'));
                            i += 2;
                        } else {
                            out.push_str(group((d1 * 10 + d2) as usize)?);
                            i += 1;
                        }
                    }
                    _ => out.push_str(group(d1 as usize)?),
                }
            }
            'a' => out.push('\u{7}'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'v' => out.push('\u{b}'),
            '\\' => out.push('\\'),
            x if x.is_ascii_alphabetic() => return Err(format!("bad escape \\{x}")),
            x => {
                out.push('\\');
                out.push(x);
            }
        }
    }
    Ok(out)
}

/// The options of [`unicode_to_latex_with`] (the keyword arguments of `unicode_to_latex()`).
#[derive(Debug, Clone)]
pub struct UnicodeToLatexOptions {
    pub non_ascii_only: bool,
    pub replacement_latex_protection: ReplacementLatexProtection,
    pub unknown_char_policy: UnknownCharPolicy,
    pub unknown_char_warning: bool,
}

impl Default for UnicodeToLatexOptions {
    fn default() -> Self {
        UnicodeToLatexOptions {
            non_ascii_only: false,
            replacement_latex_protection: ReplacementLatexProtection::Braces,
            unknown_char_policy: UnknownCharPolicy::Keep,
            unknown_char_warning: true,
        }
    }
}

/// `unicode_to_latex(s)`: with the default options (`defaults` rules, `braces` protection,
/// unknown chars kept, with a warning).
pub fn unicode_to_latex(s: &str) -> String {
    unicode_to_latex_with(s, &UnicodeToLatexOptions::default()).expect("the default policy does not fail")
}

/// `unicode_to_latex(s, non_ascii_only, replacement_latex_protection, unknown_char_policy,
/// unknown_char_warning)`
pub fn unicode_to_latex_with(s: &str, options: &UnicodeToLatexOptions) -> Res<String> {
    UnicodeToLatexEncoder::builder()
        .non_ascii_only(options.non_ascii_only)
        .replacement_latex_protection(options.replacement_latex_protection.clone())
        .unknown_char_policy(options.unknown_char_policy.clone())
        .unknown_char_warning(options.unknown_char_warning)
        .build()?
        .unicode_to_latex(s)
}

/// `utf8tolatex(s, non_ascii_only, brackets, substitute_bad_chars, fail_bad_chars)` (the
/// function of pylatexenc 1.x, deprecated).
pub fn utf8tolatex(
    s: &str,
    non_ascii_only: bool,
    brackets: bool,
    substitute_bad_chars: bool,
    fail_bad_chars: bool,
) -> Res<String> {
    let s: String = s.nfc().collect();
    let mut result = String::new();
    for ch in s.chars() {
        let o = ch as u32;
        if non_ascii_only && o < 127 {
            result.push(ch);
        } else if let Ok(i) = uni2latexmap::TABLE.binary_search_by_key(&o, |&(k, _)| k) {
            let lch = uni2latexmap::TABLE[i].1;
            if brackets && lch.starts_with('\\') {
                result.push_str(&format!("{{{lch}}}"));
            } else {
                result.push_str(lch);
            }
        } else if (32..=127).contains(&o) || matches!(ch, '\n' | '\r' | '\t') {
            result.push(ch);
        } else {
            let msg = format!("Character cannot be encoded into LaTeX: U+{o:04X} - `{ch}'");
            if fail_bad_chars {
                return Err(LatexEncodeError::Value(msg));
            }
            log::warn!("{msg}");
            if substitute_bad_chars {
                result.push_str("{\\bfseries ?}");
            } else {
                result.push(ch);
            }
        }
    }
    Ok(result)
}
