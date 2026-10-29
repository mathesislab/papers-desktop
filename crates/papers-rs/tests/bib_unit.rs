//! The unit tests of papers (test_parsing, test_unreadable, test_sort_format,
//! test_latexenc, test_entries) for the bibtex layer, as they are there.

use papers_rs::bib::latex::latex_to_unicode as decode;
use papers_rs::bib::latex_table::{protect_uppercase, string_to_latex};
use papers_rs::bib::{Bib, Entry, Field};

fn latex_to_unicode(s: &str) -> String {
    decode(s, true)
}

// test_parsing.py

#[test]
fn field_names_in_capitals() {
    // older JabRef files (#121)
    let bib = Bib::parse("@Article{k,\n Title = {Ice},\n Keywords = {ice},\n}\n");
    let e = bib.entries().next().unwrap();
    assert_eq!(e.get("title"), Some("Ice"));
    assert_eq!(e.get("keywords"), Some("ice"));
    assert!(bib.format().contains("title = {Ice}"));
}

#[test]
fn one_warning_above_an_unreadable_entry() {
    // the warning above an entry that does not parse is not repeated at every save (#121)
    let mut text =
        "@article{a,\n title={T},\n}\n\n% my note\n@article{b,\n title={x},\n title={y},\n}\n"
            .to_string();
    for _ in 0..3 {
        text = Bib::parse(&text).to_string();
    }
    assert_eq!(text.matches("% WARNING Parsing failed").count(), 1);
    assert!(text.contains("% my note"));
    assert!(text.contains(" title={x},\n title={y},"));
}

// test_unreadable.py

#[test]
fn unreadable_blocks() {
    let twice = "@article{twice,\n title = {One},\n title = {Two},\n year = {2001}\n}\n";
    let bib = Bib::parse(&format!("@article{{ok, title={{A}}}}\n\n{twice}"));
    let found = bib.unreadable();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, 3);
    assert!(found[0].1.contains("a field given twice"), "{}", found[0].1);
    // the entry is not seen, but saving writes its text back
    assert_eq!(bib.entries().count(), 1);
    assert!(bib.to_string().contains("title = {Two}"));
}

#[test]
fn a_second_key_and_a_syntax_error_are_unreadable() {
    let bib = Bib::parse("@article{a, title={A}}\n@article{a, title={B}}\n@article{c, title={C}\n");
    assert_eq!(
        bib.unreadable(),
        vec![
            (2, "a key given twice (keys ignore case): 'a'".to_string()),
            (3, "syntax error".to_string())
        ]
    );
    assert_eq!(bib.entries().count(), 1);
}

#[test]
fn field_names_and_keys_ignore_case() {
    // BibTeX ignores the case of field names and keys: Title and title, Smith2000 and
    // smith2000, are given twice (test_unreadable.py test_case)
    let text = "@article{Smith2000, title={A}}\n\n@article{smith2000, title={B}}\n\n@article{other, Title={C}, title={D}}\n";
    let bib = Bib::parse(text);
    let keys: Vec<&str> = bib.entries().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, ["Smith2000"]);
    let reasons: Vec<String> = bib.unreadable().into_iter().map(|(_, r)| r).collect();
    assert_eq!(
        reasons,
        [
            "a key given twice (keys ignore case): 'smith2000'",
            "a field given twice: 'title'"
        ]
    );
    // the text of the blocks that cannot be read is written back
    let saved = bib.to_string();
    for part in ["@article{smith2000, title={B}}", "Title={C}, title={D}"] {
        assert!(saved.contains(part), "{saved}");
    }
    // a string name keeps its case: names that differ by case are both read
    let bib = Bib::parse("@string{A = {x}}\n@string{a = {y}}\n@string{a = {z}}\n");
    assert_eq!(bib.strings().count(), 2);
    assert_eq!(bib.unreadable().len(), 1);
}

#[test]
fn unreadable_blocks_stay_as_they_are() {
    // test_unchanged_save.py: each save wrote one more warning line above the second unreadable block
    let text = "@article{good2000,\n title = {Good},\n year = {2000}\n}\n\n@article{bad1,\n title = {Bad one\n\n@article{bad2,\n title = {Bad two\n\n";
    let mut saved: Vec<String> = Vec::new();
    let mut current = text.to_string();
    for _ in 0..4 {
        let mut bib = Bib::parse(&current);
        bib.entries_mut().next().unwrap().set("year", "2001");
        current = bib.to_string();
        saved.push(current.clone());
    }
    assert_eq!(saved[1], saved[2]);
    assert_eq!(saved[2], saved[3]);
    assert_eq!(saved[2].matches("% WARNING Parsing failed").count(), 2);
    // a failed block ends with an empty line of its own, and the file with one when it is the last block
    assert!(saved[2].ends_with("Bad two\n\n"), "{:?}", saved[2]);
}

#[test]
fn a_byte_order_mark_is_dropped() {
    // test_unchanged_save.py TestReadWrite
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("papers.bib");
    let text = "@article{zed2000,\n title = {Zéd},\n year = {2000}\n}\n\n@article{alpha2000,\n title = {A},\n year = {2000}\n}\n";
    std::fs::write(&file, format!("\u{feff}{text}")).unwrap();
    let read = papers_rs::bib::read_file(&file).unwrap();
    assert_eq!(read, text);
    let bib = Bib::parse(&read);
    assert!(bib.unreadable().is_empty());
    let keys: Vec<&str> = bib.entries().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, ["zed2000", "alpha2000"]);
    assert!(!bib.to_string().contains('\u{feff}'));
}

// test_sort_format.py

#[test]
fn format_of_a_large_library() {
    let mut bib = Bib::default();
    for i in 0..10000 {
        let mut e = Entry::new("article", format!("k{i}"));
        e.fields.push(Field::new("title", format!("T {i}")));
        bib.blocks.push(papers_rs::bib::Block::Entry(e));
    }
    assert!(
        bib.to_string()
            .starts_with("@article{k0,\n title = {T 0}\n}\n")
    );
}

#[test]
fn sort_keeps_the_comments_above_a_block() {
    let mut bib = Bib::parse(
        "% about b\n@article{b,\n year = 1\n}\n\n@string{z = {Z}}\n\n@article{a,\n year = 2\n}\n",
    );
    bib.sort();
    assert_eq!(
        bib.format(),
        "@string{z = {Z}}\n\n@article{a,\n year = {2}\n}\n\n% about b\n\n@article{b,\n year = {1}\n}\n"
    );
}

// test_entries.py

#[test]
fn entry_values() {
    let mut e = Entry::new("article", "Key");
    e.fields = vec![
        Field::new("title", "first"),
        Field::new("doi", "10.1/x"),
        Field::new("title", "second"),
    ];
    assert_eq!(e.get("title"), Some("second")); // the last one
    assert_eq!(e.get("year"), None);
    e.set("title", "set"); // the first one
    assert_eq!(e.fields[0].value, "set");
    assert_eq!(e.fields[2].value, "second");
    assert_eq!(e.remove("title"), Some("set".to_string()));
    assert_eq!(e.fields.len(), 1);
}

// test_latexenc.py

#[test]
fn latex_to_unicode_accents() {
    assert_eq!(latex_to_unicode("{\\'e}"), "é");
    assert_eq!(latex_to_unicode("{\\'E}"), "É");
    assert_eq!(latex_to_unicode("{\\\"u}"), "ü");
    assert_eq!(latex_to_unicode("{\\c{c}}"), "ç");
    assert_eq!(latex_to_unicode("{\\~n}"), "ñ");
    assert_eq!(latex_to_unicode("\\'{e}"), "é");
    assert_eq!(latex_to_unicode("\\'{a}"), "á");
    assert_eq!(latex_to_unicode("\\`{e}"), "è");
    // a trailing combining accent is discarded
    assert_eq!(latex_to_unicode("\\'"), "");
    assert_eq!(latex_to_unicode("hello"), "hello");
    assert_eq!(latex_to_unicode("Hello World"), "Hello World");
}

#[test]
fn string_to_latex_and_back() {
    assert!(string_to_latex("é").contains("\\'"));
    assert!(string_to_latex("á").contains("\\'"));
    assert_eq!(string_to_latex(" { } "), " { } ");
    for s in ["é", "ñ", "ç", "ü", "ö", "François"] {
        assert_eq!(latex_to_unicode(&string_to_latex(s)), s, "{s}");
    }
}

#[test]
fn protect_uppercase_wraps_capitals() {
    assert!(protect_uppercase("Hello").contains("{H}"));
    assert!(protect_uppercase("World").contains("{W}"));
    assert_eq!(protect_uppercase("hello"), "hello");
}
