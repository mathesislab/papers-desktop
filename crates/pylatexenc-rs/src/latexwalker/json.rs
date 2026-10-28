//! `make_json_encoder()`: the nodes as JSON, as pylatexenc's `LatexNodesJSONEncoder` writes
//! them with `json.dumps()` (the same text, with the default separators and `ensure_ascii`).

use super::{LatexNode, LatexWalker, NodeKind};
use crate::macrospec::ParsedMacroArgs;

fn json_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (' '..='~').contains(&c) => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{u:04x}"));
                }
            }
        }
    }
    out.push('"');
}

fn json_list<T>(items: &[T], out: &mut String, f: impl Fn(&T, &mut String)) {
    out.push('[');
    for (i, x) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        f(x, out);
    }
    out.push(']');
}

fn json_args(a: Option<&ParsedMacroArgs>, w: &LatexWalker, out: &mut String) {
    match a {
        None => out.push_str("null"),
        Some(a) => {
            out.push_str("{\"argspec\": ");
            json_str(&a.argspec, out);
            out.push_str(", \"argnlist\": ");
            json_list(&a.argnlist, out, |n, out| match n {
                None => out.push_str("null"),
                Some(n) => json_node(n, w, out),
            });
            out.push('}');
        }
    }
}

fn json_nodes(l: &[LatexNode], w: &LatexWalker, out: &mut String) {
    json_list(l, out, |n, out| json_node(n, w, out));
}

fn json_pair(p: (&str, &str), out: &mut String) {
    out.push('[');
    json_str(p.0, out);
    out.push_str(", ");
    json_str(p.1, out);
    out.push(']');
}

fn json_node(n: &LatexNode, w: &LatexWalker, out: &mut String) {
    out.push_str(&format!("{{\"nodetype\": \"{}\", \"pos\": {}, \"len\": {}", n.node_type().class_name(), n.pos, n.len));
    let field = |name: &str, out: &mut String| out.push_str(&format!(", \"{name}\": "));
    match &n.kind {
        NodeKind::Chars { chars } => {
            field("chars", out);
            json_str(chars, out);
        }
        NodeKind::Group { nodelist, delimiters } => {
            field("nodelist", out);
            json_nodes(nodelist, w, out);
            field("delimiters", out);
            json_pair((&delimiters.0, &delimiters.1), out);
        }
        NodeKind::Comment { comment, comment_post_space } => {
            field("comment", out);
            json_str(comment, out);
            field("comment_post_space", out);
            json_str(comment_post_space, out);
        }
        NodeKind::Macro { macroname, nodeargd, macro_post_space, .. } => {
            field("macroname", out);
            json_str(macroname, out);
            field("nodeargd", out);
            json_args(nodeargd.as_ref(), w, out);
            field("macro_post_space", out);
            json_str(macro_post_space, out);
        }
        NodeKind::Environment { environmentname, nodelist, nodeargd } => {
            field("environmentname", out);
            json_str(environmentname, out);
            field("nodelist", out);
            json_nodes(nodelist, w, out);
            field("nodeargd", out);
            json_args(nodeargd.as_ref(), w, out);
        }
        NodeKind::Specials { specials_chars, nodeargd } => {
            field("specials_chars", out);
            json_str(specials_chars, out);
            field("nodeargd", out);
            json_args(nodeargd.as_ref(), w, out);
        }
        NodeKind::Math { displaytype, nodelist, delimiters } => {
            field("displaytype", out);
            json_str(displaytype, out);
            field("nodelist", out);
            json_nodes(nodelist, w, out);
            field("delimiters", out);
            json_pair((&delimiters.0, &delimiters.1), out);
        }
    }
    let (lineno, colno) = w.pos_to_lineno_colno(n.pos);
    out.push_str(&format!(", \"lineno\": {lineno}, \"colno\": {colno}}}"));
}

/// `json.dumps(nodelist, cls=make_json_encoder(latexwalker))`: the nodes as JSON, with the line
/// and column numbers of their positions in the text of the walker.
pub fn nodelist_to_json(nodelist: &[LatexNode], latexwalker: &LatexWalker) -> String {
    let mut out = String::new();
    json_nodes(nodelist, latexwalker, &mut out);
    out
}

/// `json.dumps(node, cls=make_json_encoder(latexwalker))`: one node as JSON.
pub fn node_to_json(node: &LatexNode, latexwalker: &LatexWalker) -> String {
    let mut out = String::new();
    json_node(node, latexwalker, &mut out);
    out
}
