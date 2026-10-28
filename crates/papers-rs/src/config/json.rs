//! JSON as Python's `json` module reads and writes it: the error messages of
//! the decoder, and `json.dumps(sort_keys=True, indent=2, separators=(',', ': '))`
//! (ASCII only, keys sorted) for the files that papers writes.
//!
//! Also the Python forms of a value (`str()`, `repr()`, `==`) that papers
//! prints in `papers config`.

use serde_json::{Map, Number, Value};

/// What `json.JSONDecodeError` says: `MSG: line L column C (char N)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError {
    pub message: String,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

struct Parser<'a> {
    s: &'a [char],
}

type Fail = (String, usize); // message, position

fn decode_error(doc: &[char], (msg, pos): Fail) -> DecodeError {
    let lineno = doc[..pos.min(doc.len())].iter().filter(|c| **c == '\n').count() + 1;
    let colno = match doc[..pos.min(doc.len())].iter().rposition(|c| *c == '\n') {
        Some(i) => pos - i,
        None => pos + 1,
    };
    DecodeError { message: format!("{msg}: line {lineno} column {colno} (char {pos})") }
}

/// `json.loads(text)`.
pub fn loads(text: &str) -> Result<Value, DecodeError> {
    let chars: Vec<char> = text.chars().collect();
    let parser = Parser { s: &chars };
    parser.decode().map_err(|fail| decode_error(&chars, fail))
}

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

impl Parser<'_> {
    fn at(&self, i: usize) -> Option<char> {
        self.s.get(i).copied()
    }

    fn skip_ws(&self, mut i: usize) -> usize {
        while self.at(i).is_some_and(is_ws) {
            i += 1;
        }
        i
    }

    fn decode(&self) -> Result<Value, Fail> {
        if self.at(0) == Some('\u{feff}') {
            return Err(("Unexpected UTF-8 BOM (decode using utf-8-sig)".into(), 0));
        }
        let start = self.skip_ws(0);
        let (value, end) = self.scan_once(start).map_err(|pos| ("Expecting value".to_string(), pos))??;
        let end = self.skip_ws(end);
        if end != self.s.len() {
            return Err(("Extra data".into(), end));
        }
        Ok(value)
    }

    /// The outer error is `StopIteration(pos)` ("Expecting value"), the inner
    /// one a decoding error.
    fn scan_once(&self, i: usize) -> Result<Result<(Value, usize), Fail>, usize> {
        let Some(c) = self.at(i) else { return Err(i) };
        let rest = |lit: &str| self.s[i..].starts_with(&lit.chars().collect::<Vec<_>>());
        match c {
            '"' => Ok(self.string(i + 1).map(|(s, end)| (Value::String(s), end))),
            '{' => Ok(self.object(i + 1)),
            '[' => Ok(self.array(i + 1)),
            'n' if rest("null") => Ok(Ok((Value::Null, i + 4))),
            't' if rest("true") => Ok(Ok((Value::Bool(true), i + 4))),
            'f' if rest("false") => Ok(Ok((Value::Bool(false), i + 5))),
            'N' if rest("NaN") => Ok(Ok((Value::Null, i + 3))),
            'I' if rest("Infinity") => Ok(Ok((Value::Null, i + 8))),
            '-' if rest("-Infinity") => Ok(Ok((Value::Null, i + 9))),
            _ => self.number(i).map(Ok).ok_or(i),
        }
    }

    fn number(&self, i: usize) -> Option<(Value, usize)> {
        let mut j = i;
        if self.at(j) == Some('-') {
            j += 1;
        }
        match self.at(j) {
            Some('0') => j += 1,
            Some(c) if c.is_ascii_digit() => {
                while self.at(j).is_some_and(|c| c.is_ascii_digit()) {
                    j += 1;
                }
            }
            _ => return None,
        }
        let int_end = j;
        let mut is_float = false;
        if self.at(j) == Some('.') && self.at(j + 1).is_some_and(|c| c.is_ascii_digit()) {
            j += 1;
            while self.at(j).is_some_and(|c| c.is_ascii_digit()) {
                j += 1;
            }
            is_float = true;
        }
        if matches!(self.at(j), Some('e' | 'E')) {
            let mut k = j + 1;
            if matches!(self.at(k), Some('+' | '-')) {
                k += 1;
            }
            if self.at(k).is_some_and(|c| c.is_ascii_digit()) {
                while self.at(k).is_some_and(|c| c.is_ascii_digit()) {
                    k += 1;
                }
                j = k;
                is_float = true;
            }
        }
        let text: String = self.s[i..j].iter().collect();
        let value = if is_float {
            Number::from_f64(text.parse::<f64>().ok()?).map(Value::Number).unwrap_or(Value::Null)
        } else {
            let _ = int_end;
            match text.parse::<i64>() {
                Ok(n) => Value::Number(n.into()),
                Err(_) => match text.parse::<u64>() {
                    Ok(n) => Value::Number(n.into()),
                    Err(_) => Number::from_f64(text.parse::<f64>().ok()?).map(Value::Number).unwrap_or(Value::Null),
                },
            }
        };
        Some((value, j))
    }

    fn hex4(&self, pos: usize) -> Result<u32, Fail> {
        // pos: index of the `u`
        let digits: Option<String> = (1..=4).map(|k| self.at(pos + k)).collect();
        digits
            .filter(|d| d.chars().all(|c| c.is_ascii_hexdigit()))
            .and_then(|d| u32::from_str_radix(&d, 16).ok())
            .ok_or(("Invalid \\uXXXX escape".to_string(), pos))
    }

    fn string(&self, mut end: usize) -> Result<(String, usize), Fail> {
        let begin = end - 1;
        let mut out = String::new();
        loop {
            let Some(c) = self.at(end) else {
                return Err(("Unterminated string starting at".into(), begin));
            };
            end += 1;
            match c {
                '"' => return Ok((out, end)),
                '\\' => {
                    let Some(esc) = self.at(end) else {
                        return Err(("Unterminated string starting at".into(), begin));
                    };
                    if esc != 'u' {
                        let ch = match esc {
                            '"' => '"',
                            '\\' => '\\',
                            '/' => '/',
                            'b' => '\u{8}',
                            'f' => '\u{c}',
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            _ => return Err(("Invalid \\escape".into(), end - 1)),
                        };
                        end += 1;
                        out.push(ch);
                    } else {
                        let uni = self.hex4(end)?;
                        end += 5;
                        let mut code = uni;
                        if (0xd800..=0xdbff).contains(&uni) && self.at(end) == Some('\\') && self.at(end + 1) == Some('u') {
                            let uni2 = self.hex4(end + 1)?;
                            if (0xdc00..=0xdfff).contains(&uni2) {
                                code = 0x10000 + (((uni - 0xd800) << 10) | (uni2 - 0xdc00));
                                end += 6;
                            }
                        }
                        // a lone surrogate cannot be a Rust char
                        out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                    }
                }
                c if (c as u32) < 0x20 => return Err(("Invalid control character at".into(), end - 1)),
                c => out.push(c),
            }
        }
    }

    fn object(&self, mut end: usize) -> Result<(Value, usize), Fail> {
        let mut map = Map::new();
        let mut nextchar = self.at(end);
        if nextchar != Some('"') {
            if nextchar.is_some_and(is_ws) {
                end = self.skip_ws(end);
                nextchar = self.at(end);
            }
            if nextchar == Some('}') {
                return Ok((Value::Object(map), end + 1));
            } else if nextchar != Some('"') {
                return Err(("Expecting property name enclosed in double quotes".into(), end));
            }
        }
        end += 1;
        loop {
            let (key, e) = self.string(end)?;
            end = e;
            if self.at(end) != Some(':') {
                end = self.skip_ws(end);
                if self.at(end) != Some(':') {
                    return Err(("Expecting ':' delimiter".into(), end));
                }
            }
            end += 1;
            end = self.skip_ws(end);
            let (value, e) = match self.scan_once(end) {
                Ok(result) => result?,
                Err(pos) => return Err(("Expecting value".into(), pos)),
            };
            end = e;
            map.insert(key, value);
            let mut nextchar = self.at(end);
            if nextchar.is_some_and(is_ws) {
                end = self.skip_ws(end + 1);
                nextchar = self.at(end);
            }
            end += 1;
            if nextchar == Some('}') {
                break;
            } else if nextchar != Some(',') {
                return Err(("Expecting ',' delimiter".into(), end - 1));
            }
            let comma = end - 1;
            end = self.skip_ws(end);
            let nextchar = self.at(end);
            end += 1;
            if nextchar != Some('"') {
                if nextchar == Some('}') {
                    return Err(("Illegal trailing comma before end of object".into(), comma));
                }
                return Err(("Expecting property name enclosed in double quotes".into(), end - 1));
            }
        }
        Ok((Value::Object(map), end))
    }

    fn array(&self, mut end: usize) -> Result<(Value, usize), Fail> {
        let mut values = Vec::new();
        let mut nextchar = self.at(end);
        if nextchar.is_some_and(is_ws) {
            end = self.skip_ws(end + 1);
            nextchar = self.at(end);
        }
        if nextchar == Some(']') {
            return Ok((Value::Array(values), end + 1));
        }
        loop {
            let (value, e) = match self.scan_once(end) {
                Ok(result) => result?,
                Err(pos) => return Err(("Expecting value".into(), pos)),
            };
            end = e;
            values.push(value);
            let mut nextchar = self.at(end);
            if nextchar.is_some_and(is_ws) {
                end = self.skip_ws(end + 1);
                nextchar = self.at(end);
            }
            end += 1;
            if nextchar == Some(']') {
                break;
            } else if nextchar != Some(',') {
                return Err(("Expecting ',' delimiter".into(), end - 1));
            }
            let comma = end - 1;
            end = self.skip_ws(end);
            if self.at(end) == Some(']') {
                return Err(("Illegal trailing comma before end of array".into(), comma));
            }
        }
        Ok((Value::Array(values), end))
    }
}

// writing
// -------

/// A JSON string as `json.dumps` writes it (`ensure_ascii`).
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
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
            ' '..='~' => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

fn number_text(n: &Number) -> String {
    if let Some(i) = n.as_i64() {
        i.to_string()
    } else if let Some(u) = n.as_u64() {
        u.to_string()
    } else {
        py_float_repr(n.as_f64().unwrap_or(0.0))
    }
}

fn write_value(out: &mut String, value: &Value, indent: Option<usize>, level: usize, sort: bool) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&number_text(n)),
        Value::String(s) => out.push_str(&quote(s)),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                    if indent.is_none() {
                        out.push(' ');
                    }
                }
                if let Some(n) = indent {
                    out.push('\n');
                    out.push_str(&" ".repeat(n * (level + 1)));
                }
                write_value(out, item, indent, level + 1, sort);
            }
            if let Some(n) = indent {
                out.push('\n');
                out.push_str(&" ".repeat(n * level));
            }
            out.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            if sort {
                entries.sort_by(|a, b| a.0.cmp(b.0));
            }
            out.push('{');
            for (i, (key, item)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                    if indent.is_none() {
                        out.push(' ');
                    }
                }
                if let Some(n) = indent {
                    out.push('\n');
                    out.push_str(&" ".repeat(n * (level + 1)));
                }
                out.push_str(&quote(key));
                out.push_str(": ");
                write_value(out, item, indent, level + 1, sort);
            }
            if let Some(n) = indent {
                out.push('\n');
                out.push_str(&" ".repeat(n * level));
            }
            out.push('}');
        }
    }
}

/// `json.dumps(value, sort_keys=True, indent=2, separators=(',', ': '))`:
/// what papers writes in its configuration files (no trailing newline).
pub fn dumps_pretty(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, Some(2), 0, true);
    out
}

/// `json.dumps(value)`, keys in the order of the map as given (sorted if the
/// map is).
pub fn dumps_compact(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, None, 0, false);
    out
}

// Python forms of a value
// -----------------------

/// `repr(float)`.
pub fn py_float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let sci = format!("{:e}", f.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let decpt = exp + 1;
    let sign = if f < 0.0 { "-" } else { "" };
    let body = if !(-3..=16).contains(&decpt) {
        let mut m = digits[..1].to_string();
        if digits.len() > 1 {
            m.push('.');
            m.push_str(&digits[1..]);
        }
        format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    } else if decpt <= 0 {
        format!("0.{}{digits}", "0".repeat((-decpt) as usize))
    } else if decpt as usize >= digits.len() {
        format!("{digits}{}.0", "0".repeat(decpt as usize - digits.len()))
    } else {
        format!("{}.{}", &digits[..decpt as usize], &digits[decpt as usize..])
    };
    format!("{sign}{body}")
}

fn is_printable(c: char) -> bool {
    let n = c as u32;
    !(c.is_control()
        || matches!(n, 0xa0 | 0xad | 0x1680 | 0x2000..=0x200f | 0x2028..=0x202f | 0x205f..=0x2064 | 0x3000 | 0xfeff)
        || (0xd800..=0xdfff).contains(&n)
        || (0xe000..=0xf8ff).contains(&n))
}

/// `repr(str)`.
pub fn py_repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c == ' ' || is_printable(c) => out.push(c),
            c if (c as u32) < 0x100 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c if (c as u32) < 0x10000 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push_str(&format!("\\U{:08x}", c as u32)),
        }
    }
    out.push(quote);
    out
}

/// `repr(value)` of the Python value that `json.loads` makes of it.
pub fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => number_text(n),
        Value::String(s) => py_repr_str(s),
        Value::Array(items) => format!("[{}]", items.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter().map(|(k, v)| format!("{}: {}", py_repr_str(k), py_repr(v))).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// `str(value)`.
pub fn py_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => py_repr(other),
    }
}

/// Python truthiness.
pub fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_none_or(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// `a == b` in Python (`True == 1`, `1 == 1.0`).
pub fn py_eq(a: &Value, b: &Value) -> bool {
    if let (Some(x), Some(y)) = (as_number(a), as_number(b)) {
        return x == y;
    }
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| py_eq(p, q)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| py_eq(v, w)))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decode_errors_like_python() {
        let msg = |s: &str| loads(s).unwrap_err().message;
        assert_eq!(msg(""), "Expecting value: line 1 column 1 (char 0)");
        assert_eq!(msg("{bad"), "Expecting property name enclosed in double quotes: line 1 column 2 (char 1)");
        assert_eq!(msg("{\"a\" 1}"), "Expecting ':' delimiter: line 1 column 6 (char 5)");
        assert_eq!(msg("{\"a\": 1 \"b\"}"), "Expecting ',' delimiter: line 1 column 9 (char 8)");
        assert_eq!(msg("{} x"), "Extra data: line 1 column 4 (char 3)");
        assert_eq!(msg("{\"a\": 1,}"), "Illegal trailing comma before end of object: line 1 column 8 (char 7)");
        assert_eq!(msg("[1,]"), "Illegal trailing comma before end of array: line 1 column 3 (char 2)");
        assert_eq!(msg("{\"a\n"), "Invalid control character at: line 1 column 4 (char 3)");
        assert_eq!(msg("\"abc"), "Unterminated string starting at: line 1 column 1 (char 0)");
        assert_eq!(msg("\n\n  x"), "Expecting value: line 3 column 3 (char 4)");
        assert_eq!(msg("\"\\x\""), "Invalid \\escape: line 1 column 2 (char 1)");
        assert_eq!(msg("\"\\u12\""), "Invalid \\uXXXX escape: line 1 column 3 (char 2)");
        assert_eq!(msg("[1 2]"), "Expecting ',' delimiter: line 1 column 4 (char 3)");
        assert_eq!(msg("{\"a\": }"), "Expecting value: line 1 column 7 (char 6)");
        assert_eq!(msg("["), "Expecting value: line 1 column 2 (char 1)");
        assert_eq!(msg("{"), "Expecting property name enclosed in double quotes: line 1 column 2 (char 1)");
        assert_eq!(msg("01"), "Extra data: line 1 column 2 (char 1)");
        assert_eq!(msg("\u{feff}{}"), "Unexpected UTF-8 BOM (decode using utf-8-sig): line 1 column 1 (char 0)");
        assert_eq!(msg("[\"a\",\n  ]"), "Illegal trailing comma before end of array: line 1 column 5 (char 4)");
        assert_eq!(msg("[1,\n2,\n x]"), "Expecting value: line 3 column 2 (char 8)");
    }

    #[test]
    fn decodes_values() {
        let v = loads("{\"a\": [1, 2.5, true, null, \"x\\u00e9\\ud83d\\ude00\"], \"b\": {}}").unwrap();
        assert_eq!(v, json!({"a": [1, 2.5, true, null, "x\u{e9}\u{1f600}"], "b": {}}));
    }

    #[test]
    fn dumps_like_python() {
        let v = json!({"version": 3, "b": {"z": 1, "a": [1, 2]}, "e": {}, "l": [], "s": "é\n\"", "n": null, "t": true});
        assert_eq!(
            dumps_pretty(&v),
            "{\n  \"b\": {\n    \"a\": [\n      1,\n      2\n    ],\n    \"z\": 1\n  },\n  \"e\": {},\n  \"l\": [],\n  \"n\": null,\n  \"s\": \"\\u00e9\\n\\\"\",\n  \"t\": true,\n  \"version\": 3\n}"
        );
        assert_eq!(dumps_compact(&json!({"a": 1, "b": [1, "x"]})), "{\"a\": 1, \"b\": [1, \"x\"]}");
        assert_eq!(quote("\u{1f600}\u{7f}"), "\"\\ud83d\\ude00\\u007f\"");
    }

    #[test]
    fn python_forms() {
        assert_eq!(py_float_repr(1.5), "1.5");
        assert_eq!(py_float_repr(100.0), "100.0");
        assert_eq!(py_float_repr(1e16), "1e+16");
        assert_eq!(py_float_repr(1.5e-7), "1.5e-07");
        assert_eq!(py_float_repr(0.0001), "0.0001");
        assert_eq!(py_float_repr(0.00001), "1e-05");
        assert_eq!(py_float_repr(123456789012345678.0), "1.2345678901234568e+17");
        assert_eq!(py_repr_str("it's"), "\"it's\"");
        assert_eq!(py_repr_str("a'b\"c"), "'a\\'b\"c'");
        assert_eq!(py_repr(&json!(["a", true, null, 1.0])), "['a', True, None, 1.0]");
        assert!(py_eq(&json!(true), &json!(1)));
        assert!(py_eq(&json!(500), &json!(500.0)));
        assert!(!py_eq(&json!("1"), &json!(1)));
    }
}
