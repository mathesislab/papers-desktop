//! `PartialLatexToLatexEncoder`: encode the unicode of a text that already has LaTeX code in it.

use super::{
    ConversionRuleSpec, LatexEncodeError, ReplacementLatexProtection, UnicodeToLatexConversionRule,
    UnicodeToLatexEncoder, UnicodeToLatexEncoderBuilder,
};
use crate::latexwalker::{LatexWalker, LatexWalkerOptions, TokenOptions};

/// `PartialLatexToLatexEncoder(keep_latex_chars, conversion_rules, **kwargs)`: like
/// [`UnicodeToLatexEncoder`], but the LaTeX code in the text (macros, braces, math, comments:
/// whatever starts with a char of `keep_latex_chars`) is kept as it is. It uses a walker
/// without tolerant parsing, so a lone `\` at the end of the text is an error.
#[derive(Debug, Clone)]
pub struct PartialLatexToLatexEncoder {
    encoder: UnicodeToLatexEncoder,
    /// the chars that start the LaTeX code to keep (default `\${}^_`)
    pub keep_latex_chars: String,
}

impl PartialLatexToLatexEncoder {
    /// With the default `keep_latex_chars` and options.
    pub fn new() -> Self {
        Self::with_options("\\${}^_", UnicodeToLatexEncoder::builder()).expect("the defaults are valid")
    }

    /// `PartialLatexToLatexEncoder(keep_latex_chars=..., **kwargs)`: the options of the
    /// encoder (including its `conversion_rules`, `["defaults"]` if not set) are those of the
    /// builder.
    pub fn with_options(keep_latex_chars: &str, builder: UnicodeToLatexEncoderBuilder) -> Result<Self, LatexEncodeError> {
        let keep = keep_latex_chars.to_string();
        let keep_for_rule = keep.clone();
        let step = UnicodeToLatexConversionRule::with_protection(
            super::Rule::Callable(std::sync::Arc::new(move |_enc, input| {
                if !keep_for_rule.contains(input.chars[input.pos]) {
                    return Ok(None);
                }
                let lw = LatexWalker::with_options(input.text, LatexWalkerOptions::default().tolerant_parsing(false));
                let tok = lw
                    .get_token_with(input.pos, &TokenOptions { environments: false, ..Default::default() })
                    .map_err(LatexEncodeError::Walker)?;
                let tok_as_latex = format!("{}{}", tok.pre_space, lw.s.slice(tok.pos, tok.len));
                Ok(Some((tok.pos + tok.len - input.pos, tok_as_latex)))
            })),
            ReplacementLatexProtection::None,
        );
        // the partial step goes first, before the rules that were asked for
        let encoder = builder.prepend_rule(ConversionRuleSpec::Rule(step)).build()?;
        Ok(PartialLatexToLatexEncoder { encoder, keep_latex_chars: keep })
    }

    /// `unicode_to_latex(s)`
    pub fn unicode_to_latex(&self, s: &str) -> Result<String, LatexEncodeError> {
        self.encoder.unicode_to_latex(s)
    }

    /// The underlying encoder.
    pub fn encoder(&self) -> &UnicodeToLatexEncoder {
        &self.encoder
    }
}

impl Default for PartialLatexToLatexEncoder {
    fn default() -> Self {
        Self::new()
    }
}
