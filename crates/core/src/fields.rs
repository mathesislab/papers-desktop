//! The text of the fields of an entry, and the people of its names fields,
//! as papers reads them.

use biblatex::{Chunk, Chunks, ChunksExt, ChunksRef, Entry};

/// The text of the first non-empty field among `keys`.
pub(crate) fn field(entry: &Entry, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| entry.get(key).map(text).filter(|value| !value.is_empty()))
}

/// A field that is not text (identifiers, links) as written: the parser's
/// dashes (`--` is `–`) and the escapes of bibtex (`\_`) are undone.
pub(crate) fn raw(entry: &Entry, key: &str) -> Option<String> {
    let value = entry.get(key)?.format_verbatim();
    let value = value
        .replace('—', "---")
        .replace('–', "--")
        .replace('\\', "");
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// The text of a field: braces and LaTeX commands are gone, math stays in `$`.
fn text(chunks: ChunksRef) -> String {
    let text: String = chunks
        .iter()
        .map(|chunk| match &chunk.v {
            Chunk::Math(math) => format!("${math}$"),
            Chunk::Normal(text) | Chunk::Verbatim(text) => plain(text),
        })
        .collect();
    text.trim().to_string()
}

/// A text without braces, with the ties (`~`) as spaces and without the LaTeX
/// commands that the parser leaves (`\emph{x}` is `x`; accents and symbols
/// are characters already).
fn plain(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '~' => plain.push(' '),
            '\\' if chars.peek().is_some_and(|c| c.is_alphabetic()) => {
                while chars.next_if(|c| c.is_alphabetic()).is_some() {}
                chars.next_if_eq(&' ');
            }
            _ => plain.push(c),
        }
    }
    plain
}

/// A DOI without the `https://doi.org/` or `doi:` of a link.
pub(crate) fn clean_doi(doi: String) -> String {
    const PREFIXES: [&str; 5] = [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "http://dx.doi.org/",
        "doi:",
    ];
    let prefix = PREFIXES.iter().find(|prefix| {
        doi.get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    });
    match prefix {
        Some(prefix) => doi[prefix.len()..].trim_start().to_string(),
        None => doi,
    }
}

/// A person of a names field (`A and B`), as papers reads it.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Name {
    /// an organisation, written in braces
    pub literal: Option<String>,
    pub given: String,
    /// a lowercase part of the family name: "van den"
    pub particle: String,
    pub family: String,
    pub suffix: String,
}

impl Name {
    /// The family name with its particle ("van den Broeke"), or the organisation.
    pub fn full_family(&self) -> String {
        match &self.literal {
            Some(literal) => literal.clone(),
            None => format!("{} {}", self.particle, self.family)
                .trim()
                .to_string(),
        }
    }
}

/// The people of a names field; empty names (a doubled `and`) are left out.
pub(crate) fn names(entry: &Entry, key: &str) -> Vec<Name> {
    let people = entry
        .get(key)
        .and_then(|chunks| chunks.parse::<Vec<Chunks>>().ok())
        .unwrap_or_default();
    people.iter().filter_map(|name| name_of(name)).collect()
}

/// A word of a name; a particle starts in lowercase.
struct Word {
    text: String,
    particle: bool,
}

/// A person; a name in braces is an organisation.
fn name_of(chunks: ChunksRef) -> Option<Name> {
    let parts: Vec<_> = chunks
        .iter()
        .map(|chunk| &chunk.v)
        .filter(|chunk| !matches!(chunk, Chunk::Normal(text) if text.trim().is_empty()))
        .collect();
    if let [Chunk::Verbatim(organisation)] = parts[..] {
        let literal = plain(organisation).trim().to_string();
        return Some(Name {
            literal: Some(literal),
            ..Name::default()
        });
    }
    let groups = comma_groups(chunks);
    let (given, particle, family, suffix) = name_parts(&groups);
    let join = |words: &[Word]| {
        let words: Vec<_> = words.iter().map(|word| plain(&word.text)).collect();
        words.join(" ").trim().to_string()
    };
    let name = Name {
        literal: None,
        given: join(given),
        particle: join(particle),
        family: join(family),
        suffix: join(suffix),
    };
    (name != Name::default()).then_some(name)
}

/// The given name, particle, family name and suffix of a name, as papers
/// reads it: `First von Last`, `von Last, First` and `von Last, Suffix, First`.
/// Unlike BibTeX, `Last, First` has a particle only before its first capital
/// word: `Colin de Verdière, Alain` has no particle, as papers writes it.
fn name_parts(groups: &[Vec<Word>]) -> (&[Word], &[Word], &[Word], &[Word]) {
    match groups {
        [words] => {
            // the particle runs from the first to the last lowercase word
            // before the last word
            let before = &words[..words.len().saturating_sub(1)];
            let first = before.iter().position(|word| word.particle);
            let last = before.iter().rposition(|word| word.particle);
            match first.zip(last) {
                Some((first, last)) => (
                    &words[..first],
                    &words[first..=last],
                    &words[last + 1..],
                    &[],
                ),
                None => (before, &[], &words[before.len()..], &[]),
            }
        }
        [last, middle @ .., given] => {
            let particles = last.iter().take_while(|word| word.particle).count();
            let particles = particles.min(last.len().saturating_sub(1));
            let suffix = match middle {
                [suffix] => &suffix[..],
                _ => &[],
            };
            (given, &last[..particles], &last[particles..], suffix)
        }
        [] => (&[], &[], &[], &[]),
    }
}

/// The words of a name in the groups between its commas. Whitespace and ties
/// separate words, and commas groups, but not in braces.
fn comma_groups(chunks: ChunksRef) -> Vec<Vec<Word>> {
    let mut groups = vec![vec![]];
    let mut in_word = false;
    for chunk in chunks {
        let (text, protected) = match &chunk.v {
            Chunk::Normal(text) => (text.clone(), false),
            Chunk::Verbatim(text) => (text.clone(), true),
            Chunk::Math(math) => (format!("${math}$"), true),
        };
        for c in text.chars() {
            if !protected && c == ',' {
                groups.push(vec![]);
                in_word = false;
            } else if !protected && (c.is_whitespace() || c == '~') {
                in_word = false;
            } else {
                let words = groups.last_mut().expect("there is a group");
                if !in_word {
                    let particle = !protected && c.is_lowercase();
                    words.push(Word {
                        text: String::new(),
                        particle,
                    });
                    in_word = true;
                }
                words.last_mut().expect("there is a word").text.push(c);
            }
        }
    }
    groups
}
