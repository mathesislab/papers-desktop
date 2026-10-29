//! The data tables of the papers specification (`data/`, from design/data/ of papers): the
//! code of papers-rs uses the same tables.
//!
//! - `ascii-transliteration.tsv`: [`papers_rs::names::ascii`] (the `any_ascii` crate) gives
//!   the text of the table for every code point, and nothing for the others;
//! - `latex-characters.tsv`: the LaTeX that [`papers_rs::bib::latex_table::string_to_latex`]
//!   writes, and the commands that [`papers_rs::bib::latex::latex_to_unicode`] reads.

use std::collections::HashMap;
use std::path::Path;

use papers_rs::bib::latex::latex_to_unicode;
use papers_rs::bib::latex_table::string_to_latex;
use papers_rs::names::{ascii, slugify};

/// The rows of a table (the lines that are not comments, after the header), split at the tabs.
fn rows(name: &str, header: &str) -> Vec<Vec<String>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("data")
        .join(name);
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.ends_with('\n') && !text.contains('\r'));
    let mut lines = text
        .strip_suffix('\n')
        .unwrap()
        .split('\n')
        .filter(|l| !l.starts_with('#'));
    assert_eq!(lines.next(), Some(header));
    lines
        .map(|l| l.split('\t').map(String::from).collect())
        .collect()
}

fn code_point(code: &str) -> char {
    char::from_u32(u32::from_str_radix(code.strip_prefix("U+").unwrap(), 16).unwrap()).unwrap()
}

/// A text of the transliteration table: `\\` is a backslash, `\n` a newline, `\t` a tab.
fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            other => panic!("unknown escape \\{other:?}"),
        }
    }
    out
}

#[test]
fn ascii_is_the_published_table_for_every_code_point() {
    let rows = rows("ascii-transliteration.tsv", "code\tascii");
    let mut table: HashMap<char, String> = HashMap::new();
    for row in &rows {
        assert_eq!(row.len(), 2, "{row:?}");
        let c = code_point(&row[0]);
        assert!(
            c as u32 >= 0x80,
            "{c:?}: a character below U+0080 is itself"
        );
        assert!(table.insert(c, unescape(&row[1])).is_none(), "{c:?} twice");
    }
    assert!(table.len() > 120_000, "{}", table.len());
    let mut all = String::new();
    let mut expected = String::new();
    let mut listed = 0;
    for cp in 0..0x110000u32 {
        let Some(c) = char::from_u32(cp) else {
            continue; // a surrogate: no char in Rust
        };
        let want = match table.get(&c) {
            Some(text) => {
                listed += 1;
                text.as_str()
            }
            None if cp < 0x80 => &c.to_string(),
            None => "",
        };
        assert_eq!(ascii(&c.to_string()), want, "U+{cp:04X}");
        all.push(c);
        expected.push_str(want);
    }
    assert_eq!(listed, table.len(), "a row of a surrogate");
    // the text as a whole: one character after the other
    assert_eq!(ascii(&all), expected);
}

#[test]
fn the_slugs_are_made_of_that_table() {
    // x˯x (U+02EF): `v` in the table (Unidecode: nothing)
    assert_eq!(slugify("x˯x"), "xvx");
    assert_eq!(slugify("Ocean’s"), "oceans");
}

#[test]
fn string_to_latex_is_the_published_table() {
    let rows = rows("latex-characters.tsv", "code\tkind\tlatex");
    let mut encode: HashMap<char, String> = HashMap::new();
    let mut count = 0;
    for row in &rows {
        assert_eq!(row.len(), 3, "{row:?}");
        assert!(row[1] == "encode" || row[1] == "accented", "{row:?}");
        if row[1] == "encode" {
            // `dict(unicode_to_latex)`: the last row of a character
            encode.insert(code_point(&row[0]), row[2].clone());
            count += 1;
        }
    }
    assert!(count > 2300 && rows.len() - count > 190);
    for cp in 0..0x110000u32 {
        let Some(c) = char::from_u32(cp) else {
            continue;
        };
        let s = c.to_string();
        let want = match encode.get(&c) {
            // the spaces and the braces are not escaped
            _ if matches!(c, ' ' | '{' | '}') => s.as_str(),
            Some(latex) => latex.as_str(),
            None => s.as_str(),
        };
        assert_eq!(string_to_latex(&s), want, "U+{cp:04X}");
    }
}

#[test]
fn the_accented_spellings_of_the_table_are_read() {
    let rows = rows("latex-characters.tsv", "code\tkind\tlatex");
    let accented: Vec<_> = rows.iter().filter(|r| r[1] == "accented").collect();
    // the plain command of a letter reads as the letter (`\AA ` as Å) whatever the kind of row
    let (mut read, mut total) = (0, 0);
    for row in &accented {
        let latex = &row[2];
        if latex.starts_with('\\')
            && latex[1..]
                .trim_end()
                .chars()
                .all(|c| c.is_ascii_alphabetic())
        {
            total += 1;
            if latex_to_unicode(latex, true).trim_end() == code_point(&row[0]).to_string() {
                read += 1;
            }
        }
    }
    assert!(total > 10, "{total}");
    assert_eq!(read, total, "{total} commands of accented rows");
}
