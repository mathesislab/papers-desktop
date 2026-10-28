//! Python's `str % args` for string arguments (what `simplify_repl % x` does in latex2text).

use crate::pystr::repr;

/// What a formatting failure is in Python.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PercentError {
    /// `TypeError`
    Type(String),
    /// `ValueError`
    Value(String),
    /// `KeyError`
    Key(String),
}

/// The right operand of `%`: a tuple of strings or a mapping from strings to strings.
#[derive(Debug, Clone)]
pub(crate) enum PercentArgs {
    Tuple(Vec<String>),
    Dict(Vec<(String, String)>),
}

fn dict_repr(d: &[(String, String)]) -> String {
    let items: Vec<String> = d.iter().map(|(k, v)| format!("{}: {}", repr(k), repr(v))).collect();
    format!("{{{}}}", items.join(", "))
}

fn ascii_repr(s: &str) -> String {
    let mut out = String::new();
    for c in repr(s).chars() {
        let o = c as u32;
        if o < 0x80 {
            out.push(c);
        } else if o < 0x100 {
            out.push_str(&format!("\\x{o:02x}"));
        } else if o < 0x10000 {
            out.push_str(&format!("\\u{o:04x}"));
        } else {
            out.push_str(&format!("\\U{o:08x}"));
        }
    }
    out
}

/// `fmt % args`
pub(crate) fn percent_format(fmt: &str, args: &PercentArgs) -> Result<String, PercentError> {
    let f: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut argidx = 0usize;
    // a mapping used as the single argument of an unnamed conversion
    let mut dict_consumed = false;
    let incomplete = || PercentError::Value("incomplete format".to_string());
    while i < f.len() {
        if f[i] != '%' {
            out.push(f[i]);
            i += 1;
            continue;
        }
        i += 1;
        // (key)
        let mut key: Option<String> = None;
        if f.get(i) == Some(&'(') {
            let mut depth = 1;
            let start = i + 1;
            let mut j = start;
            while j < f.len() && depth > 0 {
                match f[j] {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            if depth > 0 {
                return Err(PercentError::Value("incomplete format key".to_string()));
            }
            key = Some(f[start..j - 1].iter().collect());
            i = j;
        }
        // flags
        let (mut left, mut _zero) = (false, false);
        while let Some(&c) = f.get(i) {
            match c {
                '-' => left = true,
                '0' => _zero = true,
                '+' | ' ' | '#' => {}
                _ => break,
            }
            i += 1;
        }
        // width and precision (`*` takes an argument: only tuples have them to give)
        let mut width: Option<usize> = None;
        let mut prec: Option<usize> = None;
        let take_star = |argidx: &mut usize| -> Result<usize, PercentError> {
            match args {
                PercentArgs::Tuple(t) if *argidx < t.len() => {
                    *argidx += 1;
                    Err(PercentError::Type("* wants int".to_string()))
                }
                _ => Err(PercentError::Type("not enough arguments for format string".to_string())),
            }
        };
        if f.get(i) == Some(&'*') {
            width = Some(take_star(&mut argidx)?);
            i += 1;
        } else {
            let mut w = String::new();
            while let Some(&c) = f.get(i).filter(|c| c.is_ascii_digit()) {
                w.push(c);
                i += 1;
            }
            if !w.is_empty() {
                width = w.parse().ok();
            }
        }
        if f.get(i) == Some(&'.') {
            i += 1;
            if f.get(i) == Some(&'*') {
                prec = Some(take_star(&mut argidx)?);
                i += 1;
            } else {
                let mut p = String::new();
                while let Some(&c) = f.get(i).filter(|c| c.is_ascii_digit()) {
                    p.push(c);
                    i += 1;
                }
                prec = Some(p.parse().unwrap_or(0));
            }
        }
        // length modifiers are accepted and ignored
        while matches!(f.get(i), Some('h' | 'l' | 'L')) {
            i += 1;
        }
        let Some(&conv) = f.get(i) else { return Err(incomplete()) };
        i += 1;
        if conv == '%' {
            out.push('%');
            continue;
        }
        // the argument
        let arg: String = match &key {
            Some(k) => match args {
                PercentArgs::Dict(d) => match d.iter().find(|(kk, _)| kk == k) {
                    Some((_, v)) => v.clone(),
                    None => return Err(PercentError::Key(repr(k))),
                },
                PercentArgs::Tuple(_) => return Err(PercentError::Type("format requires a mapping".to_string())),
            },
            None => match args {
                PercentArgs::Tuple(t) => match t.get(argidx) {
                    Some(v) => {
                        argidx += 1;
                        v.clone()
                    }
                    None => {
                        return Err(PercentError::Type("not enough arguments for format string".to_string()));
                    }
                },
                PercentArgs::Dict(d) => {
                    if dict_consumed {
                        return Err(PercentError::Type("not enough arguments for format string".to_string()));
                    }
                    dict_consumed = true;
                    // the mapping itself is the argument: formatted as a whole
                    match conv {
                        's' | 'r' | 'a' => dict_repr(d),
                        _ => return Err(PercentError::Type(format!("%{conv} format: a real number is required, not dict"))),
                    }
                }
            },
        };
        let formatted: String = match conv {
            's' => arg,
            'r' => repr(&arg),
            'a' => ascii_repr(&arg),
            'c' => {
                if arg.chars().count() == 1 {
                    arg
                } else {
                    return Err(PercentError::Type("%c requires int or char".to_string()));
                }
            }
            'd' | 'i' | 'u' | 'o' | 'x' | 'X' | 'e' | 'E' | 'f' | 'F' | 'g' | 'G' => {
                return Err(PercentError::Type(format!("%{conv} format: a real number is required, not str")));
            }
            c => {
                return Err(PercentError::Value(format!("unsupported format character '{c}' (0x{:x})", c as u32)));
            }
        };
        let mut piece: Vec<char> = formatted.chars().collect();
        if let (Some(p), 's' | 'r' | 'a') = (prec, conv) {
            piece.truncate(p);
        }
        let n = piece.len();
        let pad = width.map(|w| w.saturating_sub(n)).unwrap_or(0);
        if left {
            out.extend(piece);
            out.extend(std::iter::repeat_n(' ', pad));
        } else {
            out.extend(std::iter::repeat_n(' ', pad));
            out.extend(piece);
        }
    }
    if let PercentArgs::Tuple(t) = args
        && argidx < t.len() {
            return Err(PercentError::Type("not all arguments converted during string formatting".to_string()));
        }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(v: &[&str]) -> PercentArgs {
        PercentArgs::Tuple(v.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn basic() {
        assert_eq!(percent_format("%s/%s", &t(&["a", "b"])).unwrap(), "a/b");
        assert_eq!(percent_format("100%% %s", &t(&["x"])).unwrap(), "100% x");
        assert!(percent_format("%s", &t(&[])).is_err());
        assert!(percent_format("%s", &t(&["a", "b"])).is_err());
        let d = PercentArgs::Dict(vec![("2".into(), "two".into())]);
        assert_eq!(percent_format("[%(2)s]", &d).unwrap(), "[two]");
        assert!(matches!(percent_format("[%(3)s]", &d), Err(PercentError::Key(_))));
        assert_eq!(percent_format("%5s|%-5s|%.1s", &t(&["ab", "cd", "xyz"])).unwrap(), "   ab|cd   |x");
    }
}
