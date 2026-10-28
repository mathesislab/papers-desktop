//! Deeply nested input must give an error, as pylatexenc raises a `RecursionError`, not
//! overflow the stack (these run on the 2 MiB stack of a test thread).

use pylatexenc::latex2text::{Latex2TextError, LatexNodes2Text, MathMode};
use pylatexenc::latexencode::PartialLatexToLatexEncoder;
use pylatexenc::latexwalker::{LatexWalker, LatexWalkerError, MAX_NESTING_DEPTH};

fn is_recursion_error(e: &Latex2TextError) -> bool {
    matches!(e, Latex2TextError::Walker(LatexWalkerError::Other(m)) if m.starts_with("RecursionError"))
}

#[test]
fn deep_nesting_is_an_error_not_a_crash() {
    let n = 20_000;
    let inputs = [
        format!("{}a{}", "{".repeat(n), "}".repeat(n)),
        format!("{}a{}", "\\emph{".repeat(n), "}".repeat(n)),
        format!("{}x{}", "\\begin{a}".repeat(n), "\\end{a}".repeat(n)),
        format!("{}a", "{".repeat(n)),
        format!("{}a", "$\\text{".repeat(n)),
        format!("{}a", "\\textbf{[".repeat(n)),
        "[".repeat(n),
    ];
    for math_mode in [MathMode::Text, MathMode::Verbatim] {
        let l2t = LatexNodes2Text::builder().math_mode(math_mode).build().unwrap();
        for s in &inputs {
            match l2t.latex_to_text(s) {
                Ok(_) => {}
                Err(e) => assert!(is_recursion_error(&e), "{e}"),
            }
        }
    }
}

#[test]
fn nesting_up_to_the_limit_works() {
    let n = MAX_NESTING_DEPTH - 1;
    let s = format!("{}a{}", "{".repeat(n), "}".repeat(n));
    assert_eq!(LatexNodes2Text::new().latex_to_text(&s).unwrap(), "a");
    let w = LatexWalker::new(&s);
    assert!(w.get_latex_nodes(0).is_ok());
    // the walker can be used again after the error
    let too_deep = format!("{}a{}", "{".repeat(n + 10), "}".repeat(n + 10));
    let w = LatexWalker::new(&too_deep);
    assert!(w.get_latex_nodes(0).is_err());
    assert!(w.get_latex_nodes(0).is_err());
    assert!(LatexWalker::new("{a}").get_latex_nodes(0).is_ok());
}

#[test]
fn partial_encoder_on_long_text() {
    // the partial encoder walks the text for every LaTeX char: long texts are slow, not wrong
    let s = "$x$ \\emph{a} é ".repeat(40);
    let out = PartialLatexToLatexEncoder::new().unicode_to_latex(&s).unwrap();
    assert_eq!(out, "$x$ \\emph{a} \\'e ".repeat(40));
}
