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
    let data = read_json("latexencode.json");
    check_encode(&data, "inputs", "configs", "latexencode");
    // the regex rules with each class, over the code points where Rust's classes (Unicode
    // properties) are not Python's
    check_encode(&data, "class_inputs", "class_configs", "latexencode regex classes");
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
    check_encode(&data, "regex_unicode", "regex_encode", "fuzz latexencode regex classes");
    check_regex_patterns(&data["regex_patterns"]);
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

// ---------------------------------------------------------------------------------- regex classes

/// Random patterns (classes, sets, lookarounds, anchors, groups, flags): the spans of the groups of
/// the match at each position of some texts, as Python's `re.match(text, pos)` gave them.
fn check_regex_patterns(data: &Value) {
    let pool = strs(&data["pool"]);
    let mut failures = Vec::new();
    let mut total = 0;
    for case in data["patterns"].as_array().unwrap() {
        let pattern = case["pattern"].as_str().unwrap();
        let compiled = latexenc::pyregex::compile(pattern);
        if case.get("error").is_some() {
            total += 1;
            if compiled.is_ok() {
                failures.push(format!("{pattern:?}: Python rejects it, Rust compiles it"));
            }
            continue;
        }
        let re = match compiled {
            Ok(re) => re,
            Err(e) => {
                total += 1;
                failures.push(format!("{pattern:?}: Python compiles it, Rust: {e}"));
                continue;
            }
        };
        for (t, results) in case["texts"].as_array().unwrap().iter().zip(case["results"].as_array().unwrap()) {
            let text = &pool[t.as_u64().unwrap() as usize];
            // char index of each byte offset
            let mut char_of = vec![0u64; text.len() + 1];
            let mut idx = 0;
            for (b, ch) in text.char_indices() {
                char_of[b..b + ch.len_utf8()].fill(idx);
                idx += 1;
            }
            char_of[text.len()] = idx;
            let starts: Vec<usize> = text.char_indices().map(|(b, _)| b).chain([text.len()]).collect();
            for (pos, want) in results.as_array().unwrap().iter().enumerate() {
                total += 1;
                let got = match re.captures_from_pos(text, starts[pos]) {
                    Ok(Some(c)) if c.get(0).unwrap().start() == starts[pos] => Some(
                        (0..c.len())
                            .map(|k| match c.get(k) {
                                Some(m) => json!([char_of[m.start()], char_of[m.end()]]),
                                None => json!([-1, -1]),
                            })
                            .collect::<Vec<Value>>(),
                    ),
                    Ok(_) => None,
                    Err(e) => {
                        failures.push(format!("{pattern:?} on {text:?} at {pos}: {e}"));
                        continue;
                    }
                };
                let want = want.as_array().map(|a| a.to_vec());
                if got != want {
                    failures.push(format!("{pattern:?} on {text:?} at {pos}: expected {want:?}, got {got:?}"));
                }
            }
        }
    }
    report("fuzz regex patterns", total, &failures);
}


/// The text of all the code points (but the surrogates, which a Rust string cannot hold), each
/// followed by `tail`.
fn sweep_text(tail: &str) -> String {
    let mut s = String::new();
    for ch in (0..0x110000u32).filter_map(char::from_u32) {
        s.push(ch);
        s.push_str(tail);
    }
    s
}

/// The code point at a position of the sweep text of a tail of `tail_len` chars.
fn sweep_cp(pos: u64, tail_len: u64) -> u32 {
    let cp = (pos / (1 + tail_len)) as u32;
    if cp >= 0xD800 { cp + 0x800 } else { cp }
}

/// The starts of the matches of the pattern in the text, as ranges of positions (in chars).
fn match_ranges(pattern: &str, text: &str) -> Vec<(u64, u64)> {
    let re = latexenc::pyregex::compile(pattern).unwrap_or_else(|e| panic!("{pattern:?}: {e}"));
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    let (mut byte, mut chars) = (0usize, 0u64);
    for m in re.find_iter(text) {
        let start = m.unwrap().start();
        chars += text[byte..start].chars().count() as u64;
        byte = start;
        match ranges.last_mut() {
            Some(r) if r.1 + 1 == chars => r.1 = chars,
            _ => ranges.push((chars, chars)),
        }
    }
    ranges
}

/// Every code point through every class (`\w \W \d \D \s \S`, `\b`, `\B`: alone, in a set, in a
/// negated set, with the flags a and i, ...): where the regex matches, compared with Python's `re`.
#[test]
fn regex_classes_every_code_point() {
    let data = read_json("regex_classes.json");
    let texts: Vec<(String, u64, String)> = data["texts"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, tail)| {
            let tail = tail.as_str().unwrap();
            (name.clone(), tail.chars().count() as u64, sweep_text(tail))
        })
        .collect();
    let cases = data["cases"].as_array().unwrap();
    // the sweeps are long: the cases on several threads
    let next = std::sync::atomic::AtomicUsize::new(0);
    let failures = std::sync::Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(cases.len());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                while let Some(case) = cases.get(next.fetch_add(1, std::sync::atomic::Ordering::SeqCst)) {
                    let pattern = case["pattern"].as_str().unwrap();
                    let (_, tail_len, text) =
                        texts.iter().find(|(n, _, _)| n == case["text"].as_str().unwrap()).unwrap();
                    let want: Vec<(u64, u64)> = case["matches"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|r| (r[0].as_u64().unwrap(), r[1].as_u64().unwrap()))
                        .collect();
                    let got = match_ranges(pattern, text);
                    if got != want {
                        let pos = got.iter().zip(&want).find(|(g, w)| g != w).map_or(0, |(g, w)| g.0.min(w.0));
                        failures.lock().unwrap().push(format!(
                            "{pattern:?} on {}: {} ranges, expected {}; they differ at U+{:04X} (got {:?}, expected {:?})",
                            case["text"],
                            got.len(),
                            want.len(),
                            sweep_cp(pos, *tail_len),
                            got.iter().find(|r| r.1 >= pos),
                            want.iter().find(|r| r.1 >= pos),
                        ));
                    }
                }
            });
        }
    });
    let mut failures = failures.into_inner().unwrap();
    failures.sort();
    report(
        &format!("regex classes, {} patterns over every code point", cases.len()),
        cases.len() * (0x110000 - 0x800),
        &failures,
    );
}

/// The code points where the classes of the engine (`fancy_regex` without translation: Unicode
/// properties) are not those of Python: written to `tests/regex_class_diff.json`, which
/// `gen_expected.py` reads to make the strings of the regex rules. Run it when the dependency or
/// the tables change:
///
///     cargo test -p latexenc-rs --test differential -- --ignored regenerate_class_diff
#[test]
#[ignore = "regenerates tests/regex_class_diff.json"]
fn regenerate_class_diff() {
    use latexenc::pystr::{is_decimal, is_space, is_word};
    type Class = (&'static str, &'static str, fn(char) -> bool);
    let classes: [Class; 3] =
        [("w", r"^\w$", is_word), ("d", r"^\d$", is_decimal), ("s", r"^\s$", is_space)];
    let mut out = serde_json::Map::new();
    for (name, pattern, python) in classes {
        let engine = fancy_regex::Regex::new(pattern).unwrap();
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        let mut buf = String::new();
        for ch in (0..0x110000u32).filter_map(char::from_u32) {
            buf.clear();
            buf.push(ch);
            if engine.is_match(&buf).unwrap() != python(ch) {
                let cp = ch as u32;
                match ranges.last_mut() {
                    Some(r) if r.1 + 1 == cp => r.1 = cp,
                    _ => ranges.push((cp, cp)),
                }
            }
        }
        let n: u32 = ranges.iter().map(|r| r.1 - r.0 + 1).sum();
        println!("\\{name}: {n} code points differ ({} ranges)", ranges.len());
        out.insert(name.to_string(), json!(ranges));
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("regex_class_diff.json");
    std::fs::write(path, serde_json::to_string(&Value::Object(out)).unwrap() + "\n").unwrap();
}
