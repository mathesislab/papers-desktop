//! The JSON text of CSL-JSON and BibJSON (spec `formats.md` 8): what
//! `json.dumps(value, indent=2, ensure_ascii=False)` writes, and a newline.

use crate::history::ojson::J;

/// A JSON string: the non-ASCII characters as they are, `"`, `\`, `\n`, `\r`,
/// `\t`, `\b`, `\f` escaped, the other control characters as `\u00xx`.
fn quote(s: &str, out: &mut String) {
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
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write(value: &J, out: &mut String, level: usize) {
    match value {
        J::Null => out.push_str("null"),
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Int(n) => out.push_str(&n.to_string()),
        J::Float(f) => out.push_str(&crate::config::json::py_float_repr(*f)),
        J::Str(s) => quote(s, out),
        J::Arr(items) if items.is_empty() => out.push_str("[]"),
        J::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                out.push_str(if i > 0 { ",\n" } else { "\n" });
                out.push_str(&"  ".repeat(level + 1));
                write(item, out, level + 1);
            }
            out.push('\n');
            out.push_str(&"  ".repeat(level));
            out.push(']');
        }
        J::Obj(items) if items.is_empty() => out.push_str("{}"),
        J::Obj(items) => {
            out.push('{');
            for (i, (key, item)) in items.iter().enumerate() {
                out.push_str(if i > 0 { ",\n" } else { "\n" });
                out.push_str(&"  ".repeat(level + 1));
                quote(key, out);
                out.push_str(": ");
                write(item, out, level + 1);
            }
            out.push('\n');
            out.push_str(&"  ".repeat(level));
            out.push('}');
        }
    }
}

/// The text of a JSON value, with the newline that ends the file.
pub fn dumps(value: &J) -> String {
    let mut out = String::new();
    write(value, &mut out, 0);
    out.push('\n');
    out
}

/// Give a key of an object a value: in place if it has it, else at the end (a Python dict).
pub(crate) fn put(object: &mut Vec<(String, J)>, key: &str, value: J) {
    match object.iter_mut().find(|(k, _)| k == key) {
        Some((_, v)) => *v = value,
        None => object.push((key.to_string(), value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout() {
        let v = J::Obj(vec![
            ("a".into(), J::Arr(vec![])),
            ("b".into(), J::Obj(vec![])),
            ("c".into(), J::Arr(vec![J::Int(1), J::str("é\"/\u{1}\n")])),
        ]);
        assert_eq!(
            dumps(&v),
            "{\n  \"a\": [],\n  \"b\": {},\n  \"c\": [\n    1,\n    \"é\\\"/\\u0001\\n\"\n  ]\n}\n"
        );
    }
}
