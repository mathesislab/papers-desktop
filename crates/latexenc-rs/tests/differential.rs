//! latexenc-rs against what pylatexenc 2.11 gave (`tests/gen_expected.py`): the corpus
//! through every named configuration of latex2text and latexencode, the node trees and tokens of
//! the walker, every code point through the encoders, and random texts.

mod common;

use common::*;
use latexenc::latexwalker::{
    LatexNode, LatexWalker, LatexWalkerError, LatexWalkerOptions, NodeKind, TokenKind, TokenOptions,
    nodelist_to_latex,
};
use latexenc::macrospec::ParsedMacroArgs;
use serde_json::{Value, json};

fn strs(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

/// Compare an outcome with the recorded one: the output, or the class of the exception (and
/// the message of parse errors).
fn matches_expected(exp: &Value, got: Result<String, (&'static str, String)>) -> Result<(), String> {
    match (exp.get("out"), got) {
        (Some(o), Ok(g)) => {
            if o.as_str().unwrap() == g {
                Ok(())
            } else {
                Err(format!("expected {o}, got {g:?}"))
            }
        }
        (Some(o), Err((c, m))) => Err(format!("expected {o}, got error {c}: {m}")),
        (None, Ok(g)) => Err(format!("expected error {} ({}), got {g:?}", exp["err"], exp["msg"])),
        (None, Err((c, m))) => {
            let ec = exp["err"].as_str().unwrap();
            let ec = if ec == "error" || ec == "PatternError" { "ValueError" } else { ec };
            if ec != c {
                return Err(format!("expected error {ec} ({}), got {c}: {m}", exp["msg"]));
            }
            if c == "LatexWalkerParseError" && exp["msg"].as_str().unwrap() != m {
                return Err(format!("parse error message: expected {}, got {m:?}", exp["msg"]));
            }
            Ok(())
        }
    }
}

fn report(what: &str, total: usize, failures: &[String]) {
    println!("{what}: {total} cases, {} differences", failures.len());
    if !failures.is_empty() {
        let shown: Vec<&String> = failures.iter().take(40).collect();
        panic!("{what}: {} of {total} cases differ:\n{}", failures.len(), shown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"));
    }
}

fn check_l2t(data: &Value, inputs_key: &str, configs_key: &str, label: &str) {
    let inputs = strs(&data[inputs_key]);
    let mut failures = Vec::new();
    let mut total = 0;
    for (name, expected) in data[configs_key].as_object().unwrap() {
        let cfg = l2t_config(name);
        let expected = expected.as_array().unwrap();
        assert_eq!(expected.len(), inputs.len());
        for (input, exp) in inputs.iter().zip(expected) {
            total += 1;
            let got = match &cfg {
                Err(e) => Err((l2t_err_class(e), e.to_string())),
                Ok(f) => f(input).map_err(|e| (l2t_err_class(&e), e.to_string())),
            };
            if let Err(m) = matches_expected(exp, got) {
                failures.push(format!("[{name}] {input:?}: {m}"));
            }
        }
    }
    report(label, total, &failures);
}

fn check_encode(data: &Value, inputs_key: &str, configs_key: &str, label: &str) {
    let inputs = strs(&data[inputs_key]);
    let mut failures = Vec::new();
    let mut total = 0;
    for (name, expected) in data[configs_key].as_object().unwrap() {
        let cfg = enc_config(name);
        let expected = expected.as_array().unwrap();
        assert_eq!(expected.len(), inputs.len());
        for (input, exp) in inputs.iter().zip(expected) {
            total += 1;
            let got = match &cfg {
                Err(e) => Err((enc_err_class(e), e.to_string())),
                Ok(f) => f(input).map_err(|e| (enc_err_class(&e), e.to_string())),
            };
            if let Err(m) = matches_expected(exp, got) {
                failures.push(format!("[{name}] {input:?}: {m}"));
            }
        }
    }
    report(label, total, &failures);
}

// ---------------------------------------------------------------------------------- walker dumps

fn dump_args(a: Option<&ParsedMacroArgs>) -> Value {
    let Some(a) = a else { return Value::Null };
    let mut d = json!({
        "argspec": a.argspec,
        "argnlist": a.argnlist.iter().map(|n| dump_opt(n.as_ref())).collect::<Vec<_>>(),
    });
    if let Some(t) = &a.verbatim_text {
        d["verbatim_text"] = json!(t);
        d["verbatim_delimiters"] = match a.verbatim_delimiters {
            Some((o, c)) => json!([o.to_string(), c.to_string()]),
            None => Value::Null,
        };
    }
    d
}

fn dump_opt(n: Option<&LatexNode>) -> Value {
    n.map(dump).unwrap_or(Value::Null)
}

fn dump_list(l: &[LatexNode]) -> Value {
    Value::Array(l.iter().map(dump).collect())
}

fn dump(n: &LatexNode) -> Value {
    // latex_verbatim is the source between pos and pos + len, in chars
    let chars: Vec<char> = n.parsing_state.s.as_str().chars().collect();
    assert_eq!(n.latex_verbatim(), chars[n.pos..n.pos + n.len].iter().collect::<String>());
    let t = n.node_type().class_name();
    let mut d = json!({"t": t, "pos": n.pos, "len": n.len});
    match &n.kind {
        NodeKind::Chars { chars } => d["chars"] = json!(chars),
        NodeKind::Group { nodelist, delimiters } => {
            d["nodelist"] = dump_list(nodelist);
            d["delimiters"] = json!([delimiters.0, delimiters.1]);
        }
        NodeKind::Comment { comment, comment_post_space } => {
            d["comment"] = json!(comment);
            d["post"] = json!(comment_post_space);
        }
        NodeKind::Macro { macroname, nodeargd, macro_post_space, .. } => {
            d["macroname"] = json!(macroname);
            d["post"] = json!(macro_post_space);
            d["nodeargd"] = dump_args(nodeargd.as_ref());
            d["nodeoptarg"] = dump_opt(n.nodeoptarg());
            d["nodeargs"] = match n.nodeargs() {
                None => Value::Null,
                Some(a) => Value::Array(a.iter().map(|x| dump_opt(x.as_ref())).collect()),
            };
        }
        NodeKind::Environment { environmentname, nodelist, nodeargd } => {
            d["environmentname"] = json!(environmentname);
            d["nodelist"] = dump_list(nodelist);
            d["nodeargd"] = dump_args(nodeargd.as_ref());
            d["optargs"] = Value::Array(n.optargs().unwrap().into_iter().map(dump_opt).collect());
            d["args"] = Value::Array(n.args().unwrap().iter().map(|x| dump_opt(x.as_ref())).collect());
        }
        NodeKind::Specials { specials_chars, nodeargd } => {
            d["specials_chars"] = json!(specials_chars);
            d["nodeargd"] = dump_args(nodeargd.as_ref());
        }
        NodeKind::Math { displaytype, nodelist, delimiters } => {
            d["displaytype"] = json!(displaytype);
            d["nodelist"] = dump_list(nodelist);
            d["delimiters"] = json!([delimiters.0, delimiters.1]);
        }
    }
    d
}

fn dump_error(e: &LatexWalkerError) -> Value {
    match e {
        LatexWalkerError::Parse(p) => json!({
            "err": "LatexWalkerParseError", "msg": e.to_string(), "pos": p.pos, "lineno": p.lineno,
            "colno": p.colno,
            "open_contexts": p.open_contexts.iter().map(|(w, p, l, c)| json!([w, p, l, c])).collect::<Vec<_>>(),
        }),
        LatexWalkerError::EndOfStream { .. } => json!({"err": "LatexWalkerEndOfStream", "msg": ""}),
        LatexWalkerError::Other(m) => json!({"err": walker_err_class(e), "msg": m.split_once(": ").map(|x| x.1).unwrap_or(m)}),
    }
}

fn walk(s: &str, opts: LatexWalkerOptions) -> Value {
    match LatexWalker::with_options(s, opts).get_latex_nodes(0) {
        Ok((nodes, pos, len)) => json!({
            "nodes": dump_list(&nodes), "pos": pos, "len": len, "latex": nodelist_to_latex(&nodes),
        }),
        Err(e) => dump_error(&e),
    }
}

fn tokens(s: &str, opts: &TokenOptions) -> Value {
    let w = LatexWalker::new(s);
    let n = s.chars().count();
    Value::Array(
        (0..=n)
            .map(|pos| match w.get_token_with(pos, opts) {
                Ok(t) => json!({
                    "tok": t.tok.as_str(), "arg": t.arg, "pos": t.pos, "len": t.len, "pre_space": t.pre_space,
                    "post_space": if matches!(t.tok, TokenKind::Macro | TokenKind::Comment) { t.post_space } else { String::new() },
                }),
                Err(e) => dump_error(&e),
            })
            .collect(),
    )
}

fn compare_values(label: &str, input: &str, exp: &Value, got: &Value, failures: &mut Vec<String>) {
    if exp != got {
        let (e, g) = (exp.to_string(), got.to_string());
        let n = e.chars().zip(g.chars()).take_while(|(a, b)| a == b).count();
        let from = n.saturating_sub(60);
        let cut = |s: &str| s.chars().skip(from).take(160).collect::<String>();
        failures.push(format!("[{label}] {input:?}: differ at char {n}:\n  expected ...{}\n  got      ...{}", cut(&e), cut(&g)));
    }
}

fn walker_opts(kind: &str) -> LatexWalkerOptions {
    match kind {
        "tolerant" => LatexWalkerOptions::default(),
        "strict" => LatexWalkerOptions::default().tolerant_parsing(false),
        "strict_braces" => LatexWalkerOptions::default().tolerant_parsing(false).strict_braces(true),
        "custom" => LatexWalkerOptions::default().latex_context(walker_db()),
        other => panic!("walker configuration {other}"),
    }
}

// ---------------------------------------------------------------------------------- tests

#[test]
fn latex2text_corpus() {
    check_l2t(&read_json("latex2text.json"), "inputs", "configs", "latex2text");
}

#[test]
fn latexencode_corpus() {
    check_encode(&read_json("latexencode.json"), "inputs", "configs", "latexencode");
}

#[test]
fn latexwalker_trees() {
    let data = read_json("latexwalker.json");
    let inputs = strs(&data["inputs"]);
    let mut failures = Vec::new();
    let mut total = 0;
    for (kind, expected) in data["walk"].as_object().unwrap() {
        let expected = expected.as_array().unwrap();
        assert_eq!(expected.len(), inputs.len());
        for (input, exp) in inputs.iter().zip(expected) {
            total += 1;
            compare_values(kind, input, exp, &walk(input, walker_opts(kind)), &mut failures);
        }
    }
    report("latexwalker trees", total, &failures);
}

#[test]
fn latexwalker_json() {
    let data = read_json("latexwalker.json");
    let inputs = strs(&data["tokens_inputs"]);
    let expected = strs(&data["json"]);
    assert_eq!(expected.len(), 150);
    for (input, exp) in inputs.iter().zip(&expected) {
        let lw = LatexWalker::new(input);
        let (nodes, _, _) = lw.get_latex_nodes(0).unwrap();
        assert_eq!(&latexenc::latexwalker::nodelist_to_json(&nodes, &lw), exp, "{input:?}");
    }
}

#[test]
fn latexwalker_tokens() {
    let data = read_json("latexwalker.json");
    let inputs = strs(&data["tokens_inputs"]);
    let mut failures = Vec::new();
    let mut total = 0;
    for (kind, expected) in data["tokens"].as_object().unwrap() {
        let opts = match kind.as_str() {
            "default" => TokenOptions::default(),
            "no_environments" => TokenOptions { environments: false, ..Default::default() },
            "brackets" => TokenOptions { brackets_are_chars: false, ..Default::default() },
            "braces" => TokenOptions { include_brace_chars: vec![('(', ')'), ('<', '>')], ..Default::default() },
            other => panic!("token configuration {other}"),
        };
        let expected = expected.as_array().unwrap();
        assert_eq!(expected.len(), inputs.len());
        for (input, exp) in inputs.iter().zip(expected) {
            total += 1;
            compare_values(kind, input, exp, &tokens(input, &opts), &mut failures);
        }
    }
    report("latexwalker tokens", total, &failures);
}

#[test]
fn every_code_point() {
    let data = read_json("codepoints.json");
    let mut failures = Vec::new();
    let mut total = 0;
    for (name, expected) in data.as_object().unwrap() {
        let enc = match name.as_str() {
            "defaults" => enc_config("defaults").unwrap(),
            "unicode_xml" => enc_config("unicode_xml").unwrap(),
            other => panic!("code point configuration {other}"),
        };
        let expected = expected.as_object().unwrap();
        for cp in 0..0x110000u32 {
            let Some(ch) = char::from_u32(cp) else { continue };
            total += 1;
            // unknown chars are kept (with a warning, which is not what is tested)
            let s = ch.to_string();
            let got = enc(&s).unwrap();
            let want = expected.get(&cp.to_string()).map(|v| v.as_str().unwrap()).unwrap_or(&s);
            if got != want {
                failures.push(format!("[{name}] U+{cp:04X}: expected {want:?}, got {got:?}"));
            }
        }
    }
    report("code points", total, &failures);
}

#[test]
fn fuzz() {
    let data = read_json("fuzz.json");
    check_l2t(&data, "latex", "l2t", "fuzz latex2text");
    check_encode(&data, "unicode", "encode", "fuzz latexencode");
    let inputs = strs(&data["walk"]["inputs"]);
    let mut failures = Vec::new();
    let mut total = 0;
    for kind in ["tolerant", "strict"] {
        for (input, exp) in inputs.iter().zip(data["walk"][kind].as_array().unwrap()) {
            total += 1;
            compare_values(kind, input, exp, &walk(input, walker_opts(kind)), &mut failures);
        }
    }
    report("fuzz latexwalker", total, &failures);
}
