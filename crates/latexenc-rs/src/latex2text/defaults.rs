//! The default text specs of latex2text (`latex2text/_defaultspecs.py`): the data is generated
//! (`defaultspecs.rs`); the replacement functions are here.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use unicode_normalization::UnicodeNormalization;

use super::defaultspecs::CATEGORIES;
use super::{
    EnvironmentTextSpec, Latex2TextError, LatexNodes2Text, MacroTextSpec, Repl, SpecialsTextSpec, TextContextDb,
};
use crate::latexwalker::LatexNode;
use crate::macrospec::CategoryPosition;
use crate::pystr::{rjust, strip};

type Res<T> = Result<T, Latex2TextError>;

pub(crate) const DISCARD_FALSE: u8 = 0;
pub(crate) const DISCARD_TRUE: u8 = 1;
pub(crate) const DISCARD_UNSET: u8 = 2;

/// The replacement of a default spec, in the generated tables.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DefaultRepl {
    None,
    Str(&'static str),
    /// the date of the day (`_latex_today()`)
    Today,
    Fn(DefaultFn),
}

/// The replacement functions of the default specs (the lambdas and functions of
/// `_defaultspecs.py`).
#[derive(Debug, Clone, Copy)]
pub(crate) enum DefaultFn {
    MathStyle(&'static str),
    InputMacro,
    TexOrPdfString,
    PercentLiteral,
    Accent(&'static str),
    Title,
    Author,
    Date,
    Maketitle,
    Item,
    Href,
    Part,
    Chapter,
    Section,
    Subsection,
    Subsubsection,
    Paragraph,
    Subparagraph,
    MatrixEnvironment,
    EquationEnvironment,
    Placeholder(&'static str),
    FormatUebung,
}

/// `fmt_equation_environment(envnode, l2tobj)`: an equation environment as math.
pub fn fmt_equation_environment(envnode: &LatexNode, l2tobj: &LatexNodes2Text) -> Res<String> {
    l2tobj.math_node_to_text(envnode)
}

/// `fmt_input_macro(macronode, l2tobj)`: `\input{file}` as the text of the file (see
/// [`LatexNodes2Text::set_tex_input_directory`]).
pub fn fmt_input_macro(macronode: &LatexNode, l2tobj: &LatexNodes2Text) -> Res<String> {
    l2tobj.input_node_simplify_repl(macronode)
}

fn do_fmt_placeholder_node(placeholdertext: &str, l2tobj: &LatexNodes2Text, block: bool) -> String {
    let joined: Vec<String> = placeholdertext.chars().map(String::from).collect();
    let txt = format!("< {} >", joined.join(" "));
    if block { l2tobj.fmt_indented_block(&txt, "    ") } else { format!(" {txt} ") }
}

/// `placeholder_node_formatter(placeholdertext, block)`: a replacement that shows a placeholder.
///
/// (As in Python, the text is joined char by char: `< g r a p h i c s >`.)
pub fn placeholder_node_formatter(placeholdertext: &str, block: bool) -> Repl {
    let pht = placeholdertext.to_string();
    Repl::func(move |_n, l2t| do_fmt_placeholder_node(&pht, l2t, block))
}

/// `fmt_placeholder_node(node, l2tobj)`: a placeholder with the name of the node.
pub fn fmt_placeholder_node(node: &LatexNode, l2tobj: &LatexNodes2Text) -> String {
    let name = node
        .macroname()
        .or_else(|| node.environmentname())
        .or_else(|| node.specials_chars())
        .unwrap_or("<unknown>");
    do_fmt_placeholder_node(name, l2tobj, true)
}

/// `fmt_matrix_environment_node(node, l2tobj)`: a matrix environment as `[ 1 2; 3 4 ]`.
pub fn fmt_matrix_environment_node(node: &LatexNode, l2tobj: &LatexNodes2Text) -> Res<String> {
    let mut matrix_rows: Vec<Vec<String>> = Vec::new();
    let mut buffer_this_column: Vec<String> = Vec::new();
    let mut buffer_nodes: Vec<&LatexNode> = Vec::new();

    fn new_column(l2t: &LatexNodes2Text, buffer_nodes: &mut Vec<&LatexNode>, cols: &mut Vec<String>) -> Res<()> {
        if !buffer_nodes.is_empty() {
            cols.push(strip(&l2t.nodelist_to_text_iter(buffer_nodes.iter().map(|n| Some(*n)))?).to_string());
        }
        buffer_nodes.clear();
        Ok(())
    }

    for n in node.nodelist().unwrap_or(&[]) {
        if n.specials_chars() == Some("&") {
            new_column(l2tobj, &mut buffer_nodes, &mut buffer_this_column)?;
            continue;
        }
        if n.macroname() == Some("\\") {
            new_column(l2tobj, &mut buffer_nodes, &mut buffer_this_column)?;
            matrix_rows.push(std::mem::take(&mut buffer_this_column));
            continue;
        }
        buffer_nodes.push(n);
    }
    new_column(l2tobj, &mut buffer_nodes, &mut buffer_this_column)?;
    matrix_rows.push(std::mem::take(&mut buffer_this_column));

    let max_char_width = matrix_rows
        .iter()
        .flatten()
        .map(|x| x.chars().count())
        .max()
        .ok_or_else(|| Latex2TextError::Value("max() arg is an empty sequence".to_string()))?;
    let matrix_contents: Vec<String> = matrix_rows
        .iter()
        .map(|row| row.iter().map(|x| rjust(x, max_char_width)).collect::<Vec<_>>().join(" "))
        .collect();
    Ok(format!("[ {} ]", matrix_contents.join("; ")))
}

fn math_style_offsets(style: &str) -> Option<(u32, u32)> {
    Some(match style {
        "bold" => (119808, 119834),
        "italic" => (119860, 119886),
        "bold-italic" => (119912, 119938),
        "script" => (119964, 119990),
        "bold-script" => (120016, 120042),
        "fraktur" => (120068, 120094),
        "doublestruck" => (120120, 120146),
        "bold-fraktur" => (120172, 120198),
        "sans" => (120224, 120250),
        "sans-bold" => (120276, 120302),
        "sans-italic" => (120328, 120354),
        "sans-bold-italic" => (120380, 120406),
        "monospace" => (120432, 120458),
        _ => return None,
    })
}

fn math_style_exception(style: &str, c: char) -> Option<u32> {
    Some(match (style, c) {
        ("italic", 'h') => 8462,
        ("script", 'B') => 8492,
        ("script", 'E') => 8496,
        ("script", 'F') => 8497,
        ("script", 'H') => 8459,
        ("script", 'I') => 8464,
        ("script", 'L') => 8466,
        ("script", 'M') => 8499,
        ("script", 'R') => 8475,
        ("script", 'e') => 8495,
        ("script", 'g') => 8458,
        ("script", 'o') => 8500,
        ("fraktur", 'C') => 8493,
        ("fraktur", 'H') => 8460,
        ("fraktur", 'I') => 8465,
        ("fraktur", 'R') => 8476,
        ("fraktur", 'Z') => 8488,
        ("doublestruck", 'C') => 8450,
        ("doublestruck", 'H') => 8461,
        ("doublestruck", 'N') => 8469,
        ("doublestruck", 'P') => 8473,
        ("doublestruck", 'Q') => 8474,
        ("doublestruck", 'R') => 8477,
        ("doublestruck", 'Z') => 8484,
        _ => return None,
    })
}

fn fmt_math_style_char(c: char, style: &str) -> char {
    if let Some(z) = math_style_exception(style, c) {
        return char::from_u32(z).expect("valid code point");
    }
    let (offset_up, offset_lo) = math_style_offsets(style).unwrap_or(('A' as u32, 'a' as u32));
    let oc = c as u32;
    if ('A' as u32..='Z' as u32).contains(&oc) {
        return char::from_u32(offset_up + oc - 'A' as u32).expect("valid code point");
    }
    if ('a' as u32..='z' as u32).contains(&oc) {
        return char::from_u32(offset_lo + oc - 'a' as u32).expect("valid code point");
    }
    c
}

/// `fmt_math_text_style(text, style)`: the letters of the text in a math alphabet (`bold`,
/// `italic`, `bold-italic`, `script`, `bold-script`, `fraktur`, `doublestruck`, `bold-fraktur`,
/// `sans`, `sans-bold`, `sans-italic`, `sans-bold-italic`, `monospace`), as `\mathbf` does.
pub fn fmt_math_text_style(text: &str, style: &str) -> String {
    text.chars().map(|c| fmt_math_style_char(c, style)).collect()
}

fn make_accented_char(node: &LatexNode, combining: &str, l2tobj: &LatexNodes2Text) -> Res<String> {
    let c = match node.nodeargs().and_then(|a| a.first()) {
        Some(nodearg) => strip(&l2tobj.nodelist_to_text_iter(std::iter::once(nodearg.as_ref()))?).to_string(),
        None => " ".to_string(),
    };
    Ok(c
        .chars()
        .map(|ch| {
            let ch = match ch {
                '\u{131}' => 'i',
                '\u{237}' => 'j',
                other => other,
            };
            let mut s = String::new();
            s.push(ch);
            s.push_str(combining);
            s.nfc().collect::<String>()
        })
        .collect())
}

/// The date `\today` gives, as pylatexenc writes it (`October 28, 2026`), in UTC.
pub fn latex_today() -> String {
    // `'{dt:%B} {dt.day}, {dt.year}'.format(dt=datetime.datetime.now())`; the local time zone is
    // not known to the standard library: this is the date in UTC
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86400);
    // civil from days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    format!("{} {}, {}", MONTHS[(m - 1) as usize], d, year)
}

fn format_maketitle(title: &str, author: &str, date: &str) -> String {
    let mut s = format!("{title}\n");
    s.push_str(&format!("    {author}\n"));
    s.push_str(&format!("    {date}\n"));
    let w = [title.chars().count(), 4 + author.chars().count(), 4 + date.chars().count()]
        .into_iter()
        .max()
        .unwrap_or(0);
    s.push_str(&"=".repeat(w));
    s.push_str("\n\n");
    s
}

fn argnlist_slice(node: &LatexNode, from: usize, to: usize) -> Res<&[Option<crate::latexwalker::LatexNode>]> {
    let a = node
        .nodeargd()
        .ok_or_else(|| Latex2TextError::Attribute("'NoneType' object has no attribute 'argnlist'".to_string()))?;
    let n = a.argnlist.len();
    Ok(&a.argnlist[from.min(n)..to.min(n)])
}

fn nodeargs_of(node: &LatexNode) -> Res<&[Option<LatexNode>]> {
    node.nodeargs()
        .ok_or_else(|| Latex2TextError::Type("object of type 'NoneType' has no len()".to_string()))
}

fn nodeargs_index(node: &LatexNode, k: usize) -> Res<&Option<LatexNode>> {
    nodeargs_of(node)?
        .get(k)
        .ok_or_else(|| Latex2TextError::Index("list index out of range".to_string()))
}

fn default_fn(f: DefaultFn) -> Repl {
    match f {
        DefaultFn::MathStyle(style) => Repl::try_func(move |n, l| {
            let arg_text = l.node_arg_to_text(n, 0)?;
            Ok(fmt_math_text_style(&arg_text, style))
        }),
        DefaultFn::InputMacro => Repl::try_func(fmt_input_macro),
        DefaultFn::TexOrPdfString => {
            Repl::try_func(|n, l| {
                let a = nodeargs_of(n)?;
                let n_ = a.len();
                l.nodelist_to_text_opt(&a[1.min(n_)..2.min(n_)])
            })
        }
        DefaultFn::PercentLiteral => Repl::func(|_, _| "%".to_string()),
        DefaultFn::Accent(combining) => Repl::try_func(move |n, l| make_accented_char(n, combining, l)),
        DefaultFn::Title => Repl::try_func(|n, l| {
            let t = l.nodelist_to_text_opt(argnlist_slice(n, 0, 1)?)?;
            l.doc_state.lock().unwrap().title = Some(t);
            Ok(String::new())
        }),
        DefaultFn::Author => Repl::try_func(|n, l| {
            let t = l.nodelist_to_text_opt(argnlist_slice(n, 0, 1)?)?;
            l.doc_state.lock().unwrap().author = Some(t);
            Ok(String::new())
        }),
        DefaultFn::Date => Repl::try_func(|n, l| {
            let t = l.nodelist_to_text_opt(argnlist_slice(n, 0, 1)?)?;
            l.doc_state.lock().unwrap().date = Some(t);
            Ok(String::new())
        }),
        DefaultFn::Maketitle => Repl::func(|_, l| {
            let st = l.doc_state.lock().unwrap().clone();
            format_maketitle(
                st.title.as_deref().unwrap_or("[NO \\title GIVEN]"),
                st.author.as_deref().unwrap_or("[NO \\author GIVEN]"),
                &st.date.unwrap_or_else(latex_today),
            )
        }),
        DefaultFn::Item => Repl::try_func(|n, l| match n.nodeoptarg() {
            Some(o) => Ok(format!("\n  {}", l.nodelist_to_text(std::slice::from_ref(o))?)),
            None => Ok("\n  * ".to_string()),
        }),
        DefaultFn::Href => Repl::try_func(|n, l| {
            let a = n
                .nodeargd()
                .ok_or_else(|| Latex2TextError::Attribute("'NoneType' object has no attribute 'argnlist'".to_string()))?;
            let get = |k: usize| {
                a.argnlist.get(k).ok_or_else(|| Latex2TextError::Index("list index out of range".to_string()))
            };
            let text = l.nodelist_to_text_opt(std::slice::from_ref(get(1)?))?;
            let url = l.nodelist_to_text_opt(std::slice::from_ref(get(0)?))?;
            Ok(format!("{text} <{url}>"))
        }),
        DefaultFn::Part => Repl::try_func(|n, l| Ok(format!("\n\nPART: {}\n", l.node_arg_to_text(n, 2)?.to_uppercase()))),
        DefaultFn::Chapter => {
            Repl::try_func(|n, l| Ok(format!("\n\nCHAPTER: {}\n", l.node_arg_to_text(n, 2)?.to_uppercase())))
        }
        DefaultFn::Section => {
            Repl::try_func(|n, l| Ok(format!("\n\n\u{a7} {}\n", l.node_arg_to_text(n, 2)?.to_uppercase())))
        }
        DefaultFn::Subsection => {
            Repl::try_func(|n, l| Ok(format!("\n\n \u{a7}.\u{a7} {}\n", l.node_arg_to_text(n, 2)?)))
        }
        DefaultFn::Subsubsection => {
            Repl::try_func(|n, l| Ok(format!("\n\n  \u{a7}.\u{a7}.\u{a7} {}\n", l.node_arg_to_text(n, 2)?)))
        }
        DefaultFn::Paragraph => Repl::try_func(|n, l| Ok(format!("\n\n  {}\n", l.node_arg_to_text(n, 2)?))),
        DefaultFn::Subparagraph => Repl::try_func(|n, l| Ok(format!("\n\n    {}\n", l.node_arg_to_text(n, 2)?))),
        DefaultFn::MatrixEnvironment => Repl::try_func(fmt_matrix_environment_node),
        DefaultFn::EquationEnvironment => Repl::try_func(fmt_equation_environment),
        DefaultFn::Placeholder(txt) => placeholder_node_formatter(txt, true),
        DefaultFn::FormatUebung => Repl::try_func(|n, l| {
            let first = nodeargs_index(n, 0)?;
            let mut s = format!("\n{}\n", l.nodelist_to_text_opt(std::slice::from_ref(first))?);
            let optarg = nodeargs_index(n, 1)?;
            if optarg.is_some() {
                s.push_str(&format!("[{}]\n", l.nodelist_to_text_opt(std::slice::from_ref(optarg))?));
            }
            Ok(s)
        }),
    }
}

fn default_repl(r: &DefaultRepl) -> Option<Repl> {
    match r {
        DefaultRepl::None => None,
        DefaultRepl::Str(s) => Some(Repl::Str(s.to_string())),
        DefaultRepl::Today => {
            // computed once, like a module-level constant of the Python module
            static TODAY: OnceLock<String> = OnceLock::new();
            Some(Repl::Str(TODAY.get_or_init(latex_today).clone()))
        }
        DefaultRepl::Fn(f) => Some(default_fn(*f)),
    }
}

/// The default context database, built once (what a [`LatexNodes2Text`] uses when it is given no
/// `latex_context`).
pub(crate) fn default_text_context_db() -> Arc<TextContextDb> {
    static DB: OnceLock<Arc<TextContextDb>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = TextContextDb::new();
        for (cat, macros, envs, specials) in CATEGORIES {
            let macros: Vec<MacroTextSpec> = macros
                .iter()
                .map(|(name, repl, discard)| MacroTextSpec {
                    macroname: name.to_string(),
                    simplify_repl: default_repl(repl),
                    discard: *discard == DISCARD_TRUE,
                })
                .collect();
            let envs: Vec<EnvironmentTextSpec> = envs
                .iter()
                .map(|(name, repl, discard)| EnvironmentTextSpec {
                    environmentname: name.to_string(),
                    simplify_repl: default_repl(repl),
                    discard: *discard == DISCARD_TRUE,
                })
                .collect();
            let specials: Vec<SpecialsTextSpec> = specials
                .iter()
                .map(|(name, repl, _)| SpecialsTextSpec {
                    specials_chars: name.to_string(),
                    simplify_repl: default_repl(repl),
                })
                .collect();
            db.add_context_category(cat, macros, envs, specials, CategoryPosition::Append)
                .expect("the default categories are distinct");
        }
        Arc::new(db)
    })
    .clone()
}

/// `get_default_latex_context_db()`: the default text specs, in the categories `latex-base`,
/// `latex-approximations`, `latex-placeholders`, `nonascii-specials`, `advanced-symbols`, ... A
/// new database, to modify as needed (for instance to add a category with
/// `CategoryPosition::Prepend`).
pub fn get_default_latex_context_db() -> TextContextDb {
    default_text_context_db().as_ref().clone()
}

/// `os.path.realpath`: the canonical path, resolving as much as exists.
pub(crate) fn realpath(p: &Path) -> PathBuf {
    if let Ok(c) = std::fs::canonicalize(p) {
        return c;
    }
    // lexical normalisation, then the canonical form of the longest existing ancestor
    let mut norm = PathBuf::new();
    for comp in p.components() {
        match comp {
            std::path::Component::ParentDir => {
                norm.pop();
            }
            std::path::Component::CurDir => {}
            c => norm.push(c.as_os_str()),
        }
    }
    let mut existing = norm.clone();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    while !existing.exists() {
        match existing.file_name() {
            Some(n) => rest.push(n.to_os_string()),
            None => break,
        }
        if !existing.pop() {
            break;
        }
    }
    let mut base = std::fs::canonicalize(&existing).unwrap_or(existing);
    for r in rest.into_iter().rev() {
        base.push(r);
    }
    base
}
