//! pylatexenc's own `test_latex2text.py` (v2.11), in Rust.

use latexenc::latex2text::{LatexNodes2Text, LatexNodes2TextBuilder, MathMode, StrictLatexSpaces};
use latexenc::latexwalker::LatexWalker;
use latexenc::pystr::is_space;

fn nodes_to_text(l2t: &LatexNodes2Text, latex: &str) -> String {
    let (nodes, _, _) = LatexWalker::new(latex).get_latex_nodes(0).unwrap();
    l2t.nodelist_to_text(&nodes).unwrap()
}

/// `re.sub(r'\s+', ' ', a).strip()`
fn collapse_ws(a: &str) -> String {
    a.split(is_space).filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" ")
}

fn assert_eq_up_to_whitespace(a: &str, b: &str) {
    assert_eq!(collapse_ws(a), collapse_ws(b), "\nleft:  {a:?}\nright: {b:?}");
}

fn b() -> LatexNodes2TextBuilder {
    LatexNodes2Text::builder()
}

#[test]
fn test_basic() {
    assert_eq!(nodes_to_text(&LatexNodes2Text::new(), r"\textbf{A}"), "A");

    let latex = r"\textit{hi there!} This is {\em an equation}:
\begin{equation}
    x + y i = 0
\end{equation}

where $i$ is the ``imaginary unit.''
";
    assert_eq_up_to_whitespace(
        &nodes_to_text(&LatexNodes2Text::new(), latex),
        "hi there! This is an equation:

    x + y i = 0

where i is the \u{201c}imaginary unit.\u{201d}
",
    );
    let with_delims = b().math_mode(MathMode::WithDelimiters).build().unwrap();
    assert_eq_up_to_whitespace(
        &nodes_to_text(&with_delims, latex),
        "hi there! This is an equation:
\\begin{equation}
    x + y i = 0
\\end{equation}
where $i$ is the \u{201c}imaginary unit.\u{201d}
",
    );
    assert_eq!(
        nodes_to_text(&LatexNodes2Text::new(), latex),
        LatexNodes2Text::new().latex_to_text(latex).unwrap()
    );
}

#[test]
fn test_accents() {
    let l2t = LatexNodes2Text::new();
    assert_eq!(nodes_to_text(&l2t, r"Fran\c cais"), "Fran\u{e7}ais");
    assert_eq!(nodes_to_text(&l2t, r"Fr\'en{\'{e}}tique"), "Fr\u{e9}n\u{e9}tique");
    let with_delims = b().math_mode(MathMode::WithDelimiters).build().unwrap();
    // NFC of "=" and COMBINING LONG SOLIDUS OVERLAY is U+2260
    assert_eq!(nodes_to_text(&with_delims, r"$1 \not= 2$"), "$1 \u{2260} 2$");
}

#[test]
fn test_keep_braced_groups() {
    let l2t = b().keep_braced_groups(true).build().unwrap();
    assert_eq!(
        nodes_to_text(&l2t, r"\textit{Voil\`a du texte}. Il est \'{e}crit {en fran{\c{c}}ais}"),
        "Voil\u{e0} du texte. Il est \u{e9}crit {en fran\u{e7}ais}"
    );
    let l2t = b().keep_braced_groups(true).keep_braced_groups_minlen(4).build().unwrap();
    assert_eq!(nodes_to_text(&l2t, "A{XYZ}{ABCD}"), "AXYZ{ABCD}");
    let l2t = b().keep_braced_groups(true).keep_braced_groups_minlen(0).build().unwrap();
    assert_eq!(nodes_to_text(&l2t, "{A}{XYZ}{ABCD}"), "{A}{XYZ}{ABCD}");
}

// Handling of spaces

#[test]
fn test_spaces_strictlatex() {
    let do_test = |tex: &str, uni: &str, math_mode: Option<MathMode>| {
        let mut bb = b().strict_latex_spaces(StrictLatexSpaces::Bool(true));
        if let Some(m) = math_mode {
            bb = bb.math_mode(m);
        }
        assert_eq!(bb.build().unwrap().latex_to_text(tex).unwrap(), uni, "For TeX=r'{tex}'");
    };

    // from https://github.com/phfaist/pylatexenc/issues/11
    let chars = [
        (r#"\"{o} "#, "\u{f6} "),
        (r#"{\"o} "#, "\u{f6} "),
        (r"\L ", "\u{141}"),
        (r"{\L} ", "\u{141} "),
        ("u ", "u "),
    ];
    // combinations_with_replacement(chars, 3)
    for i in 0..chars.len() {
        for j in i..chars.len() {
            for k in j..chars.len() {
                let tex: String = [chars[i].0, chars[j].0, chars[k].0].concat();
                let uni: String = [chars[i].1, chars[j].1, chars[k].1].concat();
                do_test(tex.trim(), uni.trim(), None);
            }
        }
    }

    // from https://github.com/phfaist/pylatexenc/issues/15
    let v = Some(MathMode::Verbatim);
    do_test(r"$\alpha$ $\beta$ $\gamma$", r"$\alpha$ $\beta$ $\gamma$", v);
    do_test(r"$\gamma$ detector", r"$\gamma$ detector", v);
    do_test(r"$\gamma$ $\gamma$ coincidence", r"$\gamma$ $\gamma$ coincidence", v);
}

#[test]
fn test_spaces_strictlatex_options() {
    let do_test = |tex: &str, uni: &str, strict: StrictLatexSpaces, keep_comments: bool| {
        let l2t = b().strict_latex_spaces(strict).keep_comments(keep_comments).build().unwrap();
        assert_eq!(l2t.latex_to_text(tex).unwrap(), uni);
    };
    let preset = StrictLatexSpaces::preset;
    let testlatex = r"{A} {B} \L \AA xyz:
inline math $\alpha\beta \gamma = x + i y$
line with comment % comment here
	  indented line.
\begin{equation}
    \zeta = a + i b
\end{equation}
the end.";

    do_test(
        testlatex,
        "A B \u{141}\u{c5}xyz:
inline math \u{3b1}\u{3b2}\u{3b3} = x + i y
line with comment % comment here
	  indented line.

    \u{3b6} = a + i b

the end.",
        StrictLatexSpaces::Bool(false),
        true,
    );
    do_test(
        testlatex,
        "A B \u{141}\u{c5}xyz:
inline math \u{3b1}\u{3b2}\u{3b3} = x + i y
line with comment\x20
	  indented line.

    \u{3b6} = a + i b

the end.",
        StrictLatexSpaces::Bool(false),
        false,
    );
    do_test(
        testlatex,
        "AB\u{141}\u{c5} xyz:
inline math \u{3b1}\u{3b2}\u{3b3} = x + i y
line with comment % comment here
	  indented line.

    \u{3b6} = a + i b

the end.",
        preset("based-on-source"),
        true,
    );
    do_test(
        testlatex,
        "AB\u{141}\u{c5} xyz:
inline math \u{3b1}\u{3b2}\u{3b3} = x + i y
line with comment\x20
	  indented line.

    \u{3b6} = a + i b

the end.",
        preset("based-on-source"),
        false,
    );
    do_test(
        testlatex,
        "A B \u{141}\u{c5}xyz:
inline math \u{3b1}\u{3b2}\u{3b3} = x + i y
line with comment % comment here
	  indented line.

    \u{3b6} = a + i b

the end.",
        preset("macros"),
        true,
    );
    do_test(
        testlatex,
        "A B \u{141}\u{c5}xyz:
inline math \u{3b1}\u{3b2}\u{3b3} = x + i y
line with comment\x20
	  indented line.

    \u{3b6} = a + i b

the end.",
        preset("macros"),
        false,
    );
    do_test(
        testlatex,
        "A B \u{141}\u{c5}xyz:
inline math \u{3b1}\u{3b2}\u{3b3} = x + i y
line with comment % comment here
indented line.

    \u{3b6} = a + i b

the end.",
        preset("except-in-equations"),
        true,
    );
    do_test(
        testlatex,
        "A B \u{141}\u{c5}xyz:
inline math \u{3b1}\u{3b2}\u{3b3} = x + i y
line with comment indented line.

    \u{3b6} = a + i b

the end.",
        preset("except-in-equations"),
        false,
    );
    do_test(
        testlatex,
        "A B \u{141}\u{c5}xyz:
inline math \u{3b1}\u{3b2}\u{3b3}= x + i y
line with comment % comment here
indented line.

    \u{3b6}= a + i b

the end.",
        StrictLatexSpaces::Bool(true),
        true,
    );
    do_test(
        testlatex,
        "A B \u{141}\u{c5}xyz:
inline math \u{3b1}\u{3b2}\u{3b3}= x + i y
line with comment indented line.

    \u{3b6}= a + i b

the end.",
        StrictLatexSpaces::Bool(true),
        false,
    );
}

#[test]
fn test_spaces_basedonsource() {
    // from https://github.com/phfaist/pylatexenc/issues/11 --- earlier behavior is called
    // 'based-on-source' in pylatexenc 2.x
    let l2t = b().strict_latex_spaces(StrictLatexSpaces::preset("based-on-source")).build().unwrap();
    let do_test = |tex: &str, uni: &str| {
        assert_eq!(l2t.latex_to_text(tex).unwrap(), uni, "For TeX=r'{tex}'");
    };
    let o = "\u{f6}";
    do_test(r#"\"{o} \"{o} \"{o}"#, "\u{f6}\u{f6}\u{f6}");
    do_test(r#"\"{o} \"{o} {\"o}"#, "\u{f6}\u{f6}\u{f6}");
    do_test(r#"\"{o} \"{o} \L"#, "\u{f6}\u{f6}\u{141}");
    do_test(r#"\"{o} \"{o} {\L}"#, "\u{f6}\u{f6}\u{141}");
    do_test(r#"\"{o} \"{o} u"#, "\u{f6}\u{f6} u");
    do_test(r#"\"{o} {\"o} {\"o}"#, "\u{f6}\u{f6}\u{f6}");
    do_test(r#"\"{o} {\"o} \L"#, "\u{f6}\u{f6}\u{141}");
    do_test(r#"\"{o} {\"o} {\L}"#, "\u{f6}\u{f6}\u{141}");
    do_test(r#"\"{o} {\"o} u"#, "\u{f6}\u{f6} u");
    do_test(r#"\"{o} \L \L"#, &format!("{o}\u{141}\u{141}"));
    do_test(r#"\"{o} \L {\L}"#, &format!("{o}\u{141}\u{141}"));
    do_test(r#"\"{o} \L u"#, &format!("{o}\u{141} u"));
    do_test(r#"\"{o} {\L} {\L}"#, &format!("{o}\u{141}\u{141}"));
    do_test(r#"\"{o} {\L} u"#, &format!("{o}\u{141} u"));
    do_test(r#"\"{o} u u"#, &format!("{o} u u"));
    do_test(r#"{\"o} {\"o} {\"o}"#, "\u{f6}\u{f6}\u{f6}");
    do_test(r#"{\"o} {\"o} \L"#, "\u{f6}\u{f6}\u{141}");
    do_test(r#"{\"o} {\"o} {\L}"#, "\u{f6}\u{f6}\u{141}");
    do_test(r#"{\"o} {\"o} u"#, "\u{f6}\u{f6} u");
    do_test(r#"{\"o} \L \L"#, "\u{f6}\u{141}\u{141}");
    do_test(r#"{\"o} \L {\L}"#, "\u{f6}\u{141}\u{141}");
    do_test(r#"{\"o} \L u"#, "\u{f6}\u{141} u");
    do_test(r#"{\"o} {\L} {\L}"#, "\u{f6}\u{141}\u{141}");
    do_test(r#"{\"o} {\L} u"#, "\u{f6}\u{141} u");
    do_test(r#"{\"o} u u"#, "\u{f6} u u");
    do_test(r"\L \L \L", "\u{141}\u{141}\u{141}");
    do_test(r"\L \L {\L}", "\u{141}\u{141}\u{141}");
    do_test(r"\L \L u", "\u{141}\u{141} u");
    do_test(r"\L {\L} {\L}", "\u{141}\u{141}\u{141}");
    do_test(r"\L {\L} u", "\u{141}\u{141} u");
    do_test(r"\L u u", "\u{141} u u");
    do_test(r"{\L} {\L} {\L}", "\u{141}\u{141}\u{141}");
    do_test(r"{\L} {\L} u", "\u{141}\u{141} u");
    do_test(r"{\L} u u", "\u{141} u u");
    do_test("u u u", "u u u");
}

#[test]
fn test_spacing_specials() {
    assert_eq_up_to_whitespace(
        &LatexNodes2Text::new().latex_to_text(r"``Hello,'' \emph{she} said.").unwrap(),
        "\u{201c}Hello,\u{201d} she said.",
    );
}

fn testdir() -> std::path::PathBuf {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    std::fs::canonicalize(p).unwrap()
}

#[test]
fn test_input() {
    let latex = r"ABCDEF fdksanfkld safnkd anfklsa

\input{test_input_1.tex}

MORENKFDNSN";
    let correct_text = r"ABCDEF fdksanfkld safnkd anfklsa

hi there! This is an equation:

    x + y i = 0

where i is the imaginary unit.

MORENKFDNSN";
    let mut l2t = LatexNodes2Text::new();
    l2t.set_tex_input_directory(testdir().to_str().unwrap(), None, true);
    assert_eq_up_to_whitespace(&nodes_to_text(&l2t, latex), correct_text);

    let latex = r"ABCDEF fdksanfkld safnkd anfklsa

\input{test_input_1}

MORENKFDNSN";
    assert_eq_up_to_whitespace(&nodes_to_text(&l2t, latex), correct_text);

    let latex = r"ABCDEF fdksanfkld safnkd anfklsa

\input{../test_input_1}

MORENKFDNSN";
    let correct_text_unsafe = correct_text;
    let correct_text_safe = r"ABCDEF fdksanfkld safnkd anfklsa

MORENKFDNSN";

    // make sure that the \input{} directive failed to include the file.
    let mut l2t = LatexNodes2Text::new();
    l2t.set_tex_input_directory(testdir().join("dummy").to_str().unwrap(), None, true);
    assert_eq_up_to_whitespace(&nodes_to_text(&l2t, latex), correct_text_safe);
    // but without the strict_input flag, it can access it.
    l2t.set_tex_input_directory(testdir().join("dummy").to_str().unwrap(), None, false);
    assert_eq_up_to_whitespace(&nodes_to_text(&l2t, latex), correct_text_unsafe);
}

const MATH_LATEX: &str = r"
If $\alpha=1$ and \(\beta=2\), then
\[
  \beta=2\alpha\ ,
\]
or, equivalently,
$$ \alpha = \frac1{\beta}\ .$$
";

#[test]
fn test_mathmodes_00() {
    let correct_text = "
If \u{3b1}=1 and \u{3b2}=2, then

    \u{3b2}=2\u{3b1} ,

or, equivalently,

    \u{3b1} = 1/\u{3b2} .

";
    let l2t = b().math_mode(MathMode::Text).build().unwrap();
    assert_eq_up_to_whitespace(&l2t.latex_to_text(MATH_LATEX).unwrap(), correct_text);
}

#[test]
fn test_mathmodes_01() {
    let correct_text = r"
If $α=1$ and \(β=2\), then
\[
    β=2α ,
\]
or, equivalently,
$$
    α = 1/β .
$$
";
    let l2t = b().math_mode(MathMode::WithDelimiters).build().unwrap();
    assert_eq_up_to_whitespace(&l2t.latex_to_text(MATH_LATEX).unwrap(), correct_text);
}

#[test]
fn test_mathmodes_02() {
    let l2t = b().math_mode(MathMode::Verbatim).build().unwrap();
    // math stays verbatim
    assert_eq_up_to_whitespace(&l2t.latex_to_text(MATH_LATEX).unwrap(), MATH_LATEX);
}

// test text filling etc.

#[test]
fn test_text_filling() {
    let l2t = b().fill_text(Some(20)).strict_latex_spaces(StrictLatexSpaces::Bool(true)).build().unwrap();
    assert_eq!(
        l2t.latex_to_text(
            r"
Hello world.  This   is
some weirdly formatted \textbf{text} which
will appear much    better after running latex2text."
        )
        .unwrap(),
        r"Hello world.  This
is some weirdly
formatted text which
will appear much
better after running
latex2text."
    );
}

#[test]
fn test_text_filling_init_end_par() {
    let l2t = b().fill_text(Some(80)).strict_latex_spaces(StrictLatexSpaces::Bool(true)).build().unwrap();
    assert_eq!(
        l2t.latex_to_text(
            r"

  Hello \emph{world}.  % comment
more text.

"
        )
        .unwrap(),
        "\n\nHello world. more text.\n\n"
    );
    assert_eq!(
        l2t.latex_to_text(
            r"
  Hello \emph{world}.  % comment
more text.

"
        )
        .unwrap(),
        "Hello world. more text.\n\n"
    );
}

#[test]
fn test_empty_pars() {
    let l2t = b().fill_text(Some(10)).strict_latex_spaces(StrictLatexSpaces::Bool(true)).build().unwrap();
    assert_eq!(
        l2t.latex_to_text(
            r"
A car once was very fast.

Another car came by.  And then some space:



Note the few space tokens in the otherwise empty line above.
"
        )
        .unwrap(),
        r"A car once
was very
fast.

Another
car came
by.  And
then some
space:

Note the
few space
tokens in
the
otherwise
empty line
above. "
    );
}

// test replacement strings

#[test]
fn test_repl_item() {
    // exact replacement text may change in the future (e.g. within {enumerate} environments)
    assert_eq!(
        LatexNodes2Text::new()
            .latex_to_text(
                r"
\begin{itemize}
\item First item
\item[b] The item ``B''
\item Last item
\end{itemize}
"
                .trim()
            )
            .unwrap(),
        "

  * First item

  b The item \u{201c}B\u{201d}

  * Last item
"
    );
}

#[test]
fn test_repl_placeholders() {
    // (the matrix environments are now approximated, as of pylatexenc 2.8)
    assert_eq_up_to_whitespace(
        &LatexNodes2Text::new().latex_to_text(r"\includegraphics[width=3in]{fig/some_graphics.png}").unwrap(),
        "< g r a p h i c s >",
    );
}

#[test]
fn test_repl_eqn() {
    let l2t = b().strict_latex_spaces(StrictLatexSpaces::preset("except-in-equations")).build().unwrap();
    for env in [
        "equation", "equation*", "eqnarray", "eqnarray*", "align", "align*", "multline", "multline*", "gather",
        "gather*", "dmath", "dmath*",
    ] {
        assert_eq_up_to_whitespace(
            &l2t.latex_to_text(&format!(r"\begin{{{env}}} e \approx 2.718 \end{{{env}}}")).unwrap(),
            "e \u{2248} 2.718",
        );
    }
}

#[test]
fn test_repl_matrix_environment() {
    for (env, arg) in [("array", "{lll}"), ("pmatrix", ""), ("bmatrix", ""), ("smallmatrix", "")] {
        assert_eq_up_to_whitespace(
            &LatexNodes2Text::new()
                .latex_to_text(&format!(r"\begin{{{env}}}{arg}1 &   2 & abcdef\\ 3 & 4\end{{{env}}}"))
                .unwrap(),
            "[      1      2 abcdef;      3      4 ]",
        );
    }
}

#[test]
fn test_repl_doc_title() {
    // test that \title/\author/\date work and produce something reasonable (exact output might
    // change in the future)
    assert_eq_up_to_whitespace(
        &LatexNodes2Text::new()
            .latex_to_text(
                r"
\title{The Title}
\author{The Author(s)}
\date{July 4, 2020}
\maketitle
",
            )
            .unwrap(),
        r"
The Title
    The Author(s)
    July 4, 2020
=================
",
    );
    // missing all \title, \author, \date: the date is today's (in UTC, here); the test of
    // Python computes it in the local time zone
    let out = LatexNodes2Text::new().latex_to_text("\n\\maketitle\n").unwrap();
    let lines: Vec<&str> = out.trim().lines().collect();
    assert_eq!(lines.len(), 4, "{out:?}");
    assert_eq!(lines[0], r"[NO \title GIVEN]");
    assert_eq!(lines[1], r"    [NO \author GIVEN]");
    let today = lines[2].strip_prefix("    ").unwrap();
    let (month, rest) = today.split_once(' ').unwrap();
    assert!(
        ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"]
            .contains(&month),
        "{today}"
    );
    let (day, year) = rest.split_once(", ").unwrap();
    assert!(day.parse::<u32>().is_ok() && year.parse::<u32>().is_ok(), "{today}");
    let width = (4 + r"[NO \author GIVEN]".chars().count()).max(4 + today.chars().count());
    assert_eq!(lines[3], "=".repeat(width));
}

#[test]
fn test_math_alphabets() {
    let gen_latex = |macroname: &str| format!("\\{macroname}{{-ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz-}}");
    let l2t = LatexNodes2Text::new();
    let cases = [
        ("mathbf", "-𝐀𝐁𝐂𝐃𝐄𝐅𝐆𝐇𝐈𝐉𝐊𝐋𝐌𝐍𝐎𝐏𝐐𝐑𝐒𝐓𝐔𝐕𝐖𝐗𝐘𝐙 𝐚𝐛𝐜𝐝𝐞𝐟𝐠𝐡𝐢𝐣𝐤𝐥𝐦𝐧𝐨𝐩𝐪𝐫𝐬𝐭𝐮𝐯𝐰𝐱𝐲𝐳-"),
        ("mathit", "-𝐴𝐵𝐶𝐷𝐸𝐹𝐺𝐻𝐼𝐽𝐾𝐿𝑀𝑁𝑂𝑃𝑄𝑅𝑆𝑇𝑈𝑉𝑊𝑋𝑌𝑍 𝑎𝑏𝑐𝑑𝑒𝑓𝑔ℎ𝑖𝑗𝑘𝑙𝑚𝑛𝑜𝑝𝑞𝑟𝑠𝑡𝑢𝑣𝑤𝑥𝑦𝑧-"),
        ("mathsf", "-𝖠𝖡𝖢𝖣𝖤𝖥𝖦𝖧𝖨𝖩𝖪𝖫𝖬𝖭𝖮𝖯𝖰𝖱𝖲𝖳𝖴𝖵𝖶𝖷𝖸𝖹 𝖺𝖻𝖼𝖽𝖾𝖿𝗀𝗁𝗂𝗃𝗄𝗅𝗆𝗇𝗈𝗉𝗊𝗋𝗌𝗍𝗎𝗏𝗐𝗑𝗒𝗓-"),
        ("mathbb", "-𝔸𝔹ℂ𝔻𝔼𝔽𝔾ℍ𝕀𝕁𝕂𝕃𝕄ℕ𝕆ℙℚℝ𝕊𝕋𝕌𝕍𝕎𝕏𝕐ℤ 𝕒𝕓𝕔𝕕𝕖𝕗𝕘𝕙𝕚𝕛𝕜𝕝𝕞𝕟𝕠𝕡𝕢𝕣𝕤𝕥𝕦𝕧𝕨𝕩𝕪𝕫-"),
        ("mathtt", "-𝙰𝙱𝙲𝙳𝙴𝙵𝙶𝙷𝙸𝙹𝙺𝙻𝙼𝙽𝙾𝙿𝚀𝚁𝚂𝚃𝚄𝚅𝚆𝚇𝚈𝚉 𝚊𝚋𝚌𝚍𝚎𝚏𝚐𝚑𝚒𝚓𝚔𝚕𝚖𝚗𝚘𝚙𝚚𝚛𝚜𝚝𝚞𝚟𝚠𝚡𝚢𝚣-"),
        ("mathcal", "-𝒜ℬ𝒞𝒟ℰℱ𝒢ℋℐ𝒥𝒦ℒℳ𝒩𝒪𝒫𝒬ℛ𝒮𝒯𝒰𝒱𝒲𝒳𝒴𝒵 𝒶𝒷𝒸𝒹ℯ𝒻ℊ𝒽𝒾𝒿𝓀𝓁𝓂𝓃ℴ𝓅𝓆𝓇𝓈𝓉𝓊𝓋𝓌𝓍𝓎𝓏-"),
        ("mathscr", "-𝒜ℬ𝒞𝒟ℰℱ𝒢ℋℐ𝒥𝒦ℒℳ𝒩𝒪𝒫𝒬ℛ𝒮𝒯𝒰𝒱𝒲𝒳𝒴𝒵 𝒶𝒷𝒸𝒹ℯ𝒻ℊ𝒽𝒾𝒿𝓀𝓁𝓂𝓃ℴ𝓅𝓆𝓇𝓈𝓉𝓊𝓋𝓌𝓍𝓎𝓏-"),
        ("mathfrak", "-𝔄𝔅ℭ𝔇𝔈𝔉𝔊ℌℑ𝔍𝔎𝔏𝔐𝔑𝔒𝔓𝔔ℜ𝔖𝔗𝔘𝔙𝔚𝔛𝔜ℨ 𝔞𝔟𝔠𝔡𝔢𝔣𝔤𝔥𝔦𝔧𝔨𝔩𝔪𝔫𝔬𝔭𝔮𝔯𝔰𝔱𝔲𝔳𝔴𝔵𝔶𝔷-"),
    ];
    for (name, expected) in cases {
        assert_eq!(l2t.latex_to_text(&gen_latex(name)).unwrap(), expected, "{name}");
    }
}
