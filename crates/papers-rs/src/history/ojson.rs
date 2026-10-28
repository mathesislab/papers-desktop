//! JSON that keeps the order of the keys, as Python's `json` does for a dict.
//!
//! The files of the history are written with `json.dump(indent=1)` in the
//! order of the keys of the dict (`parent`, `time`, `kind`, ...), not sorted;
//! a state that is rewritten keeps its keys, in their order, even those that
//! this implementation does not know. `serde_json::Value` sorts its keys, so
//! the store has a value of its own.

use crate::config::json::{py_float_repr, quote};

#[derive(Debug, Clone, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<J>),
    /// An object: its keys in order, without repeats.
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn str(s: impl Into<String>) -> J {
        J::Str(s.into())
    }

    pub fn strs<S: AsRef<str>>(items: &[S]) -> J {
        J::Arr(items.iter().map(|s| J::str(s.as_ref())).collect())
    }

    pub fn nums(items: &[u64]) -> J {
        J::Arr(items.iter().map(|n| J::Int(*n as i64)).collect())
    }

    pub fn opt_str(s: &Option<String>) -> J {
        s.as_ref().map_or(J::Null, J::str)
    }

    /// The value of a key of an object.
    pub fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Set a key of an object: in place if it has it, else at the end (a
    /// Python dict).
    pub fn set(&mut self, key: &str, value: J) {
        if let J::Obj(items) = self {
            match items.iter_mut().find(|(k, _)| k == key) {
                Some((_, v)) => *v = value,
                None => items.push((key.to_string(), value)),
            }
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<J> {
        if let J::Obj(items) = self {
            let at = items.iter().position(|(k, _)| k == key)?;
            return Some(items.remove(at).1);
        }
        None
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A non-negative whole number (a state number); a whole float counts, as
    /// in Python (`1.0 == 1`).
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            J::Int(n) if *n >= 0 => Some(*n as u64),
            J::Float(f) if *f >= 0.0 && f.fract() == 0.0 && *f < 1e18 => Some(*f as u64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            J::Int(n) => Some(*n as f64),
            J::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[J]> {
        match self {
            J::Arr(items) => Some(items),
            _ => None,
        }
    }

    pub fn is_object(&self) -> bool {
        matches!(self, J::Obj(_))
    }

    /// The strings of an array (the other items are left out).
    pub fn strings(&self) -> Vec<String> {
        self.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|j| j.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The whole numbers of an array.
    pub fn numbers(&self) -> Vec<u64> {
        self.as_array()
            .map(|a| a.iter().filter_map(J::as_u64).collect())
            .unwrap_or_default()
    }

    /// `json.loads`, or `None` when the text is not JSON.
    pub fn parse(text: &str) -> Option<J> {
        let mut parser = Parser {
            s: text.as_bytes(),
            i: 0,
        };
        parser.ws();
        let value = parser.value(0)?;
        parser.ws();
        (parser.i == parser.s.len()).then_some(value)
    }

    /// `json.dumps(value, indent=1)` (ASCII only, no final newline).
    pub fn dumps(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, level: usize) {
        match self {
            J::Null => out.push_str("null"),
            J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            J::Int(n) => out.push_str(&n.to_string()),
            J::Float(f) if f.is_finite() => out.push_str(&py_float_repr(*f)),
            J::Float(f) => out.push_str(if f.is_nan() {
                "NaN"
            } else if *f > 0.0 {
                "Infinity"
            } else {
                "-Infinity"
            }),
            J::Str(s) => out.push_str(&quote(s)),
            J::Arr(items) if items.is_empty() => out.push_str("[]"),
            J::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    out.push_str(if i > 0 { ",\n" } else { "\n" });
                    out.push_str(&" ".repeat(level + 1));
                    item.write(out, level + 1);
                }
                out.push('\n');
                out.push_str(&" ".repeat(level));
                out.push(']');
            }
            J::Obj(items) if items.is_empty() => out.push_str("{}"),
            J::Obj(items) => {
                out.push('{');
                for (i, (key, item)) in items.iter().enumerate() {
                    out.push_str(if i > 0 { ",\n" } else { "\n" });
                    out.push_str(&" ".repeat(level + 1));
                    out.push_str(&quote(key));
                    out.push_str(": ");
                    item.write(out, level + 1);
                }
                out.push('\n');
                out.push_str(&" ".repeat(level));
                out.push('}');
            }
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

const MAX_DEPTH: usize = 200;

impl Parser<'_> {
    fn ws(&mut self) {
        while matches!(self.s.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, word: &str) -> bool {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Option<J> {
        if depth > MAX_DEPTH {
            return None;
        }
        match *self.s.get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut items: Vec<(String, J)> = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Some(J::Obj(items));
                }
                loop {
                    self.ws();
                    let key = self.string()?;
                    self.ws();
                    if self.s.get(self.i) != Some(&b':') {
                        return None;
                    }
                    self.i += 1;
                    self.ws();
                    let value = self.value(depth + 1)?;
                    match items.iter_mut().find(|(k, _)| *k == key) {
                        Some((_, v)) => *v = value,
                        None => items.push((key, value)),
                    }
                    self.ws();
                    match self.s.get(self.i)? {
                        b',' => self.i += 1,
                        b'}' => {
                            self.i += 1;
                            return Some(J::Obj(items));
                        }
                        _ => return None,
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Some(J::Arr(items));
                }
                loop {
                    self.ws();
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    match self.s.get(self.i)? {
                        b',' => self.i += 1,
                        b']' => {
                            self.i += 1;
                            return Some(J::Arr(items));
                        }
                        _ => return None,
                    }
                }
            }
            b'"' => self.string().map(J::Str),
            b't' => self.eat("true").then_some(J::Bool(true)),
            b'f' => self.eat("false").then_some(J::Bool(false)),
            b'n' => self.eat("null").then_some(J::Null),
            b'N' => self.eat("NaN").then_some(J::Float(f64::NAN)),
            b'I' => self.eat("Infinity").then_some(J::Float(f64::INFINITY)),
            _ => self.number(),
        }
    }

    fn number(&mut self) -> Option<J> {
        if self.eat("-Infinity") {
            return Some(J::Float(f64::NEG_INFINITY));
        }
        let start = self.i;
        if self.s.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        let digits = |p: &mut Self| {
            let from = p.i;
            while matches!(p.s.get(p.i), Some(b'0'..=b'9')) {
                p.i += 1;
            }
            p.i - from
        };
        if digits(self) == 0 {
            return None;
        }
        let mut float = false;
        if self.s.get(self.i) == Some(&b'.') {
            self.i += 1;
            if digits(self) == 0 {
                return None;
            }
            float = true;
        }
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if digits(self) == 0 {
                return None;
            }
            float = true;
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).ok()?;
        if float {
            text.parse().ok().map(J::Float)
        } else {
            match text.parse() {
                Ok(n) => Some(J::Int(n)),
                Err(_) => text.parse().ok().map(J::Float),
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        if self.s.get(self.i) != Some(&b'"') {
            return None;
        }
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = *self.s.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let e = *self.s.get(self.i)?;
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xd800..0xdc00).contains(&first)
                                && self.s[self.i..].starts_with(b"\\u")
                            {
                                let save = self.i;
                                self.i += 2;
                                match self.hex4() {
                                    Some(low) if (0xdc00..0xe000).contains(&low) => {
                                        0x10000 + ((first - 0xd800) << 10) + (low - 0xdc00)
                                    }
                                    _ => {
                                        self.i = save;
                                        first
                                    }
                                }
                            } else {
                                first
                            };
                            char::from_u32(code).unwrap_or('\u{fffd}')
                        }
                        _ => return None,
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                c if c < 0x20 => return None,
                c => out.push(c),
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.s.get(self.i..self.i + 4)?;
        let text = std::str::from_utf8(digits).ok()?;
        let n = u32::from_str_radix(text, 16).ok()?;
        self.i += 4;
        Some(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_order_of_the_keys_stays_and_the_layout_is_pythons() {
        let text = "{\n \"parent\": null,\n \"time\": 1793189487.9236956,\n \"ops\": [],\n \"bibtex\": {\n  \"pieces\": [\n   \"ab\"\n  ],\n  \"sha\": \"é\"\n },\n \"files\": {}\n}";
        let j = J::parse(text).unwrap();
        assert_eq!(j.get("time"), Some(&J::Float(1793189487.9236956)));
        assert_eq!(j.dumps(), text.replace('é', "\\u00e9"));
    }

    #[test]
    fn what_is_not_json_is_none() {
        assert!(J::parse("{").is_none());
        assert!(J::parse("{} x").is_none());
        assert!(J::parse("").is_none());
        assert_eq!(
            J::parse("[1, 2.0, \"a\\ud83d\\ude00\"]").unwrap().dumps(),
            "[\n 1,\n 2.0,\n \"a\\ud83d\\ude00\"\n]"
        );
    }

    #[test]
    fn a_repeated_key_keeps_its_place() {
        let j = J::parse(r#"{"a": 1, "b": 2, "a": 3}"#).unwrap();
        assert_eq!(
            j,
            J::Obj(vec![("a".into(), J::Int(3)), ("b".into(), J::Int(2))])
        );
    }
}
