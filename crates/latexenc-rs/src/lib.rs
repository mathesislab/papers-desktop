//! A Rust port of [pylatexenc](https://github.com/phfaist/pylatexenc) 2.11 by Philippe Faist.
//!
//! The modules mirror those of the Python package:
//!
//! - [`latexwalker`]: parse LaTeX into a tree of nodes (tolerant of errors, like pylatexenc);
//! - [`macrospec`]: the specifications of macros, environments and "specials" that tell the
//!   walker how to read their arguments, and the context database that holds them;
//! - [`latex2text`]: convert LaTeX code to plain unicode text (`LatexNodes2Text`);
//! - [`latexencode`]: encode unicode text as LaTeX (`UnicodeToLatexEncoder`).
//!
//! The names are those of the Python package (Rust-cased where the language needs it), and
//! the results are the same as pylatexenc's; see the README for the differences.
//!
//! ```
//! use latexenc::latex2text::LatexNodes2Text;
//! use latexenc::latexencode::unicode_to_latex;
//!
//! let text = LatexNodes2Text::new().latex_to_text(r#"Fran\c{c}ois M{\"u}ller \textendash{} $\alpha$"#).unwrap();
//! assert_eq!(text, "François Müller – α");
//! assert_eq!(unicode_to_latex("Müller – α"), r#"M\"uller {\textendash} \ensuremath{\alpha}"#);
//! ```
//!
//! # The configurations of bibtexparser and papers
//!
//! bibtexparser's `LatexDecodingMiddleware` decodes with a `LatexNodes2Text` whose context has
//! `\url{...}` replaced by its argument, and its math kept verbatim; papers uses
//! `LatexNodes2Text(math_mode='verbatim')`:
//!
//! ```
//! use latexenc::latex2text::{self, LatexNodes2Text, MacroTextSpec, MathMode};
//! use latexenc::macrospec::CategoryPosition;
//!
//! let mut db = latex2text::get_default_latex_context_db();
//! db.add_macro_category(
//!     "bibtexparse-default-context",
//!     vec![MacroTextSpec::with_repl("url", "%s")],
//!     CategoryPosition::Prepend,
//! )
//! .unwrap();
//! let decoder = LatexNodes2Text::builder().latex_context(db).math_mode(MathMode::Verbatim).build().unwrap();
//! assert_eq!(decoder.latex_to_text(r#"{\"U}ber \url{http://a.b/c_d} $x_i$"#).unwrap(), "Über http://a.b/c_d $x_i$");
//!
//! let papers = LatexNodes2Text::builder().math_mode(MathMode::Verbatim).build().unwrap();
//! assert_eq!(papers.latex_to_text(r"\url{http://a.b}").unwrap(), "<http://a.b>");
//! ```
//!
//! and its `LatexEncodingMiddleware` encodes with regex rules (keep `$...$`, wrap URLs in
//! `\url{}`) before the default table; see `tests/common/mod.rs` for that encoder.

pub mod latex2text;
pub mod latexencode;
pub mod latexwalker;
pub mod macrospec;
pub mod pyregex;
pub mod pystr;
mod pystr_tables;
mod textwrap;
mod util;

pub use util::LineNumbersCalculator;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_main_types_are_send_and_sync() {
        fn is_send_sync<T: Send + Sync>() {}
        is_send_sync::<latexwalker::LatexWalker>();
        is_send_sync::<latexwalker::LatexNode>();
        is_send_sync::<macrospec::LatexContextDb>();
        is_send_sync::<latex2text::LatexNodes2Text>();
        is_send_sync::<latex2text::TextContextDb>();
        is_send_sync::<latexencode::UnicodeToLatexEncoder>();
        is_send_sync::<latexencode::PartialLatexToLatexEncoder>();
    }
}
