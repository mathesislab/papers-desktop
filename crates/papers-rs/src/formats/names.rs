//! The names of the formats (`papers.formats`, spec `formats.md` 2.2 and
//! 2.3): a bibtex `author` as the parts of CSL names, and back.

use crate::bib::pystr::{is_space, strip};
use crate::bib::unicode::value_to_unicode;

/// What separates the pieces of a text at brace depth 0.
#[derive(Clone, Copy)]
pub(crate) enum Sep {
    /// `,`
    Comma,
    /// `\s+`
    Space,
    /// `\s+and\s+`
    And,
}

impl Sep {
    /// The end of the separator that starts at `i`, if one does.
    fn at(self, chars: &[char], i: usize) -> Option<usize> {
        let run = |from: usize| {
            let mut j = from;
            while j < chars.len() && is_space(chars[j]) {
                j += 1;
            }
            j
        };
        match self {
            Sep::Comma => (chars[i] == ',').then_some(i + 1),
            Sep::Space => {
                let j = run(i);
                (j > i).then_some(j)
            }
            Sep::And => {
                let j = run(i);
                if j == i || chars.get(j..j + 3)? != ['a', 'n', 'd'] {
                    return None;
                }
                let k = run(j + 3);
                (k > j + 3).then_some(k)
            }
        }
    }
}

/// A bibtex value split at a separator outside of braces, the pieces stripped
/// (`_split_depth0`).
pub(crate) fn split_depth0(value: &str, sep: Sep) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    let mut parts: Vec<String> = Vec::new();
    let (mut depth, mut start, mut i) = (0i64, 0usize, 0usize);
    while i < chars.len() {
        match chars[i] {
            '{' => depth += 1,
            '}' => depth = (depth - 1).max(0), // a stray } does not stop the split
            _ if depth == 0 => {
                if let Some(end) = sep.at(&chars, i) {
                    parts.push(chars[start..i].iter().collect());
                    start = end;
                    i = end;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(chars[start..].iter().collect());
    parts.iter().map(|p| strip(p).to_string()).collect()
}

/// Whether the whole text is one braced group, `{World Health Organization}` (`_is_braced`).
fn is_braced(s: &str) -> bool {
    if !(s.starts_with('{') && s.ends_with('}')) {
        return false;
    }
    let n = s.chars().count();
    let mut depth = 0i64;
    for (i, c) in s.chars().enumerate() {
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
        }
        if depth == 0 && i + 1 < n {
            return false;
        }
    }
    true
}

/// `von`, `van der`: a token that starts with a lower-case letter (`_is_particle`).
fn is_particle(token: &str) -> bool {
    token
        .trim_start_matches(['{', '\\'])
        .chars()
        .next()
        .is_some_and(char::is_lowercase)
}

/// A name as the parts of a CSL name. The parts are there or not; of an
/// organization in braces, `literal` alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Name {
    pub literal: Option<String>,
    pub family: Option<String>,
    pub given: Option<String>,
    pub non_dropping_particle: Option<String>,
    pub dropping_particle: Option<String>,
    pub suffix: Option<String>,
}

fn truthy(part: &Option<String>) -> &str {
    part.as_deref().unwrap_or("")
}

/// A bibtex name as the parts of a CSL name (`parse_bibtex_name`): the parts
/// are decoded from LaTeX.
pub fn parse_bibtex_name(name: &str) -> Name {
    let name = strip(name);
    if is_braced(name) {
        let inner: String = name
            .chars()
            .skip(1)
            .take(name.chars().count() - 2)
            .collect();
        return Name {
            literal: Some(value_to_unicode(&inner)),
            ..Name::default()
        };
    }
    let parts = split_depth0(name, Sep::Comma);
    let (family, given, particle, suffix);
    if parts.len() == 1 {
        let tokens = split_depth0(name, Sep::Space);
        if tokens.len() == 1 {
            return Name {
                family: Some(value_to_unicode(&tokens[0])),
                ..Name::default()
            };
        }
        // First von Last: von is from the first to the last lower-case word before the last word
        let lower: Vec<usize> = (0..tokens.len() - 1)
            .filter(|&i| is_particle(&tokens[i]))
            .collect();
        let (g, p, f) = match (lower.first(), lower.last()) {
            (Some(&first), Some(&last)) => {
                (&tokens[..first], &tokens[first..=last], &tokens[last + 1..])
            }
            _ => (
                &tokens[..tokens.len() - 1],
                &tokens[..0],
                &tokens[tokens.len() - 1..],
            ),
        };
        family = f.join(" ");
        given = g.join(" ");
        particle = p.join(" ");
        suffix = String::new();
    } else {
        // Last, First or Last, Jr, First; further commas belong to First
        let last = &parts[0];
        suffix = if parts.len() >= 3 {
            parts[1].clone()
        } else {
            String::new()
        };
        given = if parts.len() >= 3 {
            parts[2..].join(", ")
        } else {
            parts[1].clone()
        };
        let mut tokens = split_depth0(last, Sep::Space);
        let mut particles: Vec<String> = Vec::new();
        while tokens.len() > 1 && is_particle(&tokens[0]) {
            particles.push(tokens.remove(0));
        }
        family = tokens.join(" ");
        particle = particles.join(" ");
    }
    let part = |v: String| (!v.is_empty()).then(|| value_to_unicode(&v));
    Name {
        family: part(family),
        given: part(given),
        non_dropping_particle: part(particle),
        suffix: part(suffix),
        ..Name::default()
    }
}

/// The names of a bibtex `author`, `editor`... field (`parse_bibtex_names`).
pub fn parse_bibtex_names(value: &str) -> Vec<Name> {
    if strip(value).is_empty() {
        return Vec::new();
    }
    split_depth0(strip(value), Sep::And)
        .iter()
        .filter(|n| !n.is_empty())
        .map(|n| parse_bibtex_name(n))
        .collect()
}

/// A CSL name as a bibtex name: `von Family, Suffix, Given` (an organization
/// in braces) (`format_bibtex_name`).
pub fn format_bibtex_name(name: &Name) -> String {
    if !truthy(&name.literal).is_empty() && truthy(&name.family).is_empty() {
        return format!("{{{}}}", truthy(&name.literal));
    }
    let mut family = truthy(&name.family).to_string();
    let particle = truthy(&name.non_dropping_particle);
    if !particle.is_empty() {
        family = format!("{particle} {family}");
    }
    let given: Vec<&str> = [truthy(&name.given), truthy(&name.dropping_particle)]
        .into_iter()
        .filter(|x| !x.is_empty())
        .collect();
    let given = given.join(" ");
    let suffix = truthy(&name.suffix);
    if !suffix.is_empty() {
        // the last comma stays without given name: it marks the suffix as one
        return if given.is_empty() {
            format!("{family}, {suffix},")
        } else {
            format!("{family}, {suffix}, {given}")
        };
    }
    if given.is_empty() {
        family
    } else {
        format!("{family}, {given}")
    }
}

/// The names joined with ` and ` (`format_bibtex_names`); a part that is blank (empty or
/// whitespace only) is left out, and so is a name with no part left (formats.md 3.3).
pub fn format_bibtex_names(names: &[Name]) -> String {
    names
        .iter()
        .filter_map(name_parts)
        .map(|name| format_bibtex_name(&name))
        .collect::<Vec<_>>()
        .join(" and ")
}

/// A name without its blank parts; none when no part is left (`_csl_name_parts`).
fn name_parts(name: &Name) -> Option<Name> {
    let part = |p: &Option<String>| p.clone().filter(|v| !strip(v).is_empty());
    let name = Name {
        literal: part(&name.literal),
        family: part(&name.family),
        given: part(&name.given),
        non_dropping_particle: part(&name.non_dropping_particle),
        dropping_particle: part(&name.dropping_particle),
        suffix: part(&name.suffix),
    };
    (name != Name::default()).then_some(name)
}

/// A CSL name as a RIS name: `Family, Given, Suffix` (an organization as it is) (`_ris_name`).
pub(crate) fn ris_name(name: &Name) -> String {
    if !truthy(&name.literal).is_empty() && truthy(&name.family).is_empty() {
        return truthy(&name.literal).to_string();
    }
    let family: Vec<&str> = [truthy(&name.non_dropping_particle), truthy(&name.family)]
        .into_iter()
        .filter(|x| !x.is_empty())
        .collect();
    let family = family.join(" ");
    let given = truthy(&name.given);
    let suffix = truthy(&name.suffix);
    if !suffix.is_empty() {
        // an empty given name keeps its place
        return format!("{family}, {given}, {suffix}");
    }
    if given.is_empty() {
        family
    } else {
        format!("{family}, {given}")
    }
}

/// A RIS name (`Family, Given, Suffix`) as a bibtex name; one without a comma
/// is an organization (`_bibtex_name_of_ris`).
pub fn bibtex_name_of_ris(name: &str) -> String {
    let name = strip(name);
    if !name.contains(',') {
        return if name.contains(' ') {
            format!("{{{name}}}")
        } else {
            name.to_string()
        };
    }
    let parts: Vec<&str> = name.split(',').map(strip).collect();
    if parts.len() >= 3 && !parts[2].is_empty() {
        return if parts[1].is_empty() {
            format!("{}, {},", parts[0], parts[2])
        } else {
            format!("{}, {}, {}", parts[0], parts[2], parts[1])
        };
    }
    if parts[1].is_empty() {
        parts[0].to_string()
    } else {
        format!("{}, {}", parts[0], parts[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(name: &str) -> String {
        let n = parse_bibtex_name(name);
        let mut out = Vec::new();
        for (k, v) in [
            ("literal", &n.literal),
            ("family", &n.family),
            ("given", &n.given),
            ("particle", &n.non_dropping_particle),
            ("suffix", &n.suffix),
        ] {
            if let Some(v) = v {
                out.push(format!("{k}={v}"));
            }
        }
        out.join(",")
    }

    #[test]
    fn spec_examples() {
        assert_eq!(parts("Perrette, M."), "family=Perrette,given=M.");
        assert_eq!(
            parts("van der Berg, Jan"),
            "family=Berg,given=Jan,particle=van der"
        );
        assert_eq!(
            parts("Ludwig van Beethoven"),
            "family=Beethoven,given=Ludwig,particle=van"
        );
        assert_eq!(
            parts("Smith, Jr, John"),
            "family=Smith,given=John,suffix=Jr"
        );
        assert_eq!(
            parts("{World Health Organization}"),
            "literal=World Health Organization"
        );
        assert_eq!(
            parts("jean de la fontaine"),
            "family=fontaine,particle=jean de la"
        );
        assert_eq!(
            parts("Doe, Jr, Jane, X"),
            "family=Doe,given=Jane, X,suffix=Jr"
        );
        assert_eq!(parts("Doe, Jr,"), "family=Doe,suffix=Jr");
        assert_eq!(parts("A AND B"), "family=B,given=A AND");
    }

    #[test]
    fn splitting() {
        assert_eq!(
            split_depth0("A and B  and  {C and D}", Sep::And),
            ["A", "B", "{C and D}"]
        );
        assert_eq!(
            split_depth0("a, {b, c}, d", Sep::Comma),
            ["a", "{b, c}", "d"]
        );
    }

    #[test]
    fn back() {
        let n = parse_bibtex_name("Smith, Jr, John");
        assert_eq!(format_bibtex_name(&n), "Smith, Jr, John");
        assert_eq!(ris_name(&n), "Smith, John, Jr");
        assert_eq!(bibtex_name_of_ris("John Smith"), "{John Smith}");
        assert_eq!(bibtex_name_of_ris("Smith, John, Jr"), "Smith, Jr, John");
    }
}
