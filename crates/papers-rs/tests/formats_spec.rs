//! The rules of `formats.md` (papers 3.0.0b6) that the tests of papers check one by one: the
//! conversions of `papers.formats` as the texts papers writes and reads. Ported from
//! `tests/test_formats.py` and `tests/test_spec_formats.py` of papers (the cases are those of
//! `design/tests.md`); the random and hand-written cases of the reference are in `formats.rs`.

use papers_rs::bib::{Bib, Entry};
use papers_rs::formats::import::loads;
use papers_rs::formats::names::{
    Name, bibtex_name_of_ris, format_bibtex_names, parse_bibtex_names,
};
use papers_rs::formats::{Format, dumps, no_versions};
use serde_json::{Value, json};

// helpers
// -------

fn parse(bibtex: &str) -> Vec<Entry> {
    Bib::parse(bibtex).entries().cloned().collect()
}

fn text(bibtex: &str, format: Format, files: &[&str]) -> String {
    let entries = parse(bibtex);
    let refs: Vec<&Entry> = entries.iter().collect();
    let files: Vec<String> = files.iter().map(|f| f.to_string()).collect();
    dumps(&refs, format, &|_| files.clone(), &no_versions)
}

/// The CSL-JSON items written for a bibtex text.
fn csl(bibtex: &str) -> Vec<Value> {
    let json: Value = serde_json::from_str(&text(bibtex, Format::CslJson, &[])).unwrap();
    json.as_array().unwrap().clone()
}

/// The keys of the items of a CSL-JSON text, in order (serde_json keeps the order with `preserve_order`,
/// the text is read as it is written otherwise).
fn csl_keys(bibtex: &str, files: &[&str]) -> Vec<String> {
    let text = text(bibtex, Format::CslJson, files);
    // the keys of the first object, at depth 2 (the array, the item)
    let mut keys = Vec::new();
    let mut depth = 0;
    let mut chars = text.char_indices().peekable();
    let mut in_string = false;
    let mut start = 0;
    while let Some((i, c)) = chars.next() {
        if in_string {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => {
                    in_string = false;
                    if depth == 2 {
                        let rest = text[i + 1..].trim_start();
                        if rest.starts_with(':') {
                            keys.push(text[start + 1..i].to_string());
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                start = i;
            }
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            _ => {}
        }
    }
    keys
}

fn csl_import(items: &str) -> Vec<Entry> {
    loads(items, Format::CslJson).unwrap()
}

/// The entry read from one CSL-JSON item.
fn csl_one(item: &str) -> Entry {
    let mut entries = csl_import(&format!("[{item}]"));
    assert_eq!(entries.len(), 1);
    entries.remove(0)
}

fn ris_import(text: &str) -> Result<Vec<Entry>, String> {
    loads(text, Format::Ris)
}

/// The entry read from a RIS record of these lines (`TY  - ` is added before, `ER  - ` after).
fn ris_one(lines: &[&str]) -> Entry {
    let record = format!("TY  - {}\nER  - \n", lines.join("\n"));
    let mut entries = ris_import(&record).unwrap();
    assert_eq!(entries.len(), 1, "{record:?}");
    entries.remove(0)
}

fn bibjson_one(record: &str) -> Entry {
    let mut entries = loads(&format!("[{record}]"), Format::BibJson).unwrap();
    assert_eq!(entries.len(), 1);
    entries.remove(0)
}

/// The fields of an entry, sorted (the order of the fields is not what the tests of papers compare).
fn fields(e: &Entry) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = e
        .fields
        .iter()
        .map(|f| (f.key.clone(), f.value.clone()))
        .collect();
    fields.sort();
    fields
}

fn field(e: &Entry, name: &str) -> Option<String> {
    e.get(name).map(str::to_string)
}

/// Pairs as `fields` gives them: sorted.
fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = items
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    pairs.sort();
    pairs
}

fn name_json(n: &Name) -> Value {
    let mut m = serde_json::Map::new();
    for (k, v) in [
        ("family", &n.family),
        ("given", &n.given),
        ("non-dropping-particle", &n.non_dropping_particle),
        ("suffix", &n.suffix),
        ("literal", &n.literal),
    ] {
        if let Some(v) = v {
            m.insert(k.to_string(), json!(v));
        }
    }
    Value::Object(m)
}

// names, 2.2 and 2.3
// ------------------

#[test]
fn names_to_parts_examples() {
    let table = [
        (
            "Perrette, M.",
            json!([{"family": "Perrette", "given": "M."}]),
        ),
        (
            "van der Berg, Jan",
            json!([{"family": "Berg", "given": "Jan", "non-dropping-particle": "van der"}]),
        ),
        (
            "Ludwig van Beethoven",
            json!([{"family": "Beethoven", "given": "Ludwig", "non-dropping-particle": "van"}]),
        ),
        (
            "Smith, Jr, John",
            json!([{"family": "Smith", "given": "John", "suffix": "Jr"}]),
        ),
        (
            "{World Health Organization}",
            json!([{"literal": "World Health Organization"}]),
        ),
        (
            "jean de la fontaine",
            json!([{"family": "fontaine", "non-dropping-particle": "jean de la"}]),
        ),
        // 3.0.0b6: the parts after a third comma are the given name, a suffix may stand alone
        (
            "Doe, Jr, Jane, X",
            json!([{"family": "Doe", "given": "Jane, X", "suffix": "Jr"}]),
        ),
        ("Doe, Jr,", json!([{"family": "Doe", "suffix": "Jr"}])),
        ("A AND B", json!([{"family": "B", "given": "A AND"}])),
        ("Plato", json!([{"family": "Plato"}])),
        (
            "{Barnes and Noble} and Smith,  John ",
            json!([{"literal": "Barnes and Noble"}, {"family": "Smith", "given": "John"}]),
        ),
        (
            "{van} Gogh, Vincent",
            json!([{"family": "Gogh", "given": "Vincent", "non-dropping-particle": "van"}]),
        ),
        ("de, Jean", json!([{"family": "de", "given": "Jean"}])),
    ];
    for (name, parts) in table {
        let got: Vec<Value> = parse_bibtex_names(name).iter().map(name_json).collect();
        assert_eq!(Value::Array(got), parts, "{name}");
        // through CSL-JSON, in the order of the keys
        let items = csl(&format!("@book{{x, author = {{{name}}}}}"));
        assert_eq!(items[0]["author"], parts, "{name}");
    }
}

#[test]
fn more_commas_and_suffix_without_given_name() {
    // Last, Jr, First: further commas belong to First (nothing is lost)
    let names = parse_bibtex_names("Doe, Jr, Jane, X");
    assert_eq!(format_bibtex_names(&names), "Doe, Jr, Jane, X");
    // a suffix without given name: the last comma stays, without a trailing space
    let name = Name {
        family: Some("Doe".into()),
        suffix: Some("Jr".into()),
        ..Name::default()
    };
    let roe = Name {
        family: Some("Roe".into()),
        ..Name::default()
    };
    assert_eq!(format_bibtex_names(&[name, roe]), "Doe, Jr, and Roe");
}

#[test]
fn parts_to_names_rules() {
    let table = [
        (
            r#"[{"literal": "World Health Organization"}]"#,
            "{World Health Organization}",
        ),
        (r#"["A plain string"]"#, "{A plain string}"),
        (
            r#"[{"family": "Berg", "given": "Jan", "non-dropping-particle": "van der"}]"#,
            "van der Berg, Jan",
        ),
        (
            r#"[{"family": "Berg", "given": "Jan", "dropping-particle": "van"}]"#,
            "Berg, Jan van",
        ),
        (
            r#"[{"family": "Smith", "given": "John", "suffix": "Jr"}]"#,
            "Smith, Jr, John",
        ),
        (r#"[{"family": "Doe", "suffix": "Jr"}]"#, "Doe, Jr,"),
        (r#"[{"family": "Plato"}]"#, "Plato"),
        (
            r#"[{"family": "Doe", "given": "Jane"}, {"family": "Roe"}]"#,
            "Doe, Jane and Roe",
        ),
        (r#"[{"family": "Doe", "literal": "ignored"}]"#, "Doe"),
    ];
    for (names, bibtex) in table {
        let e = csl_one(&format!(
            r#"{{"type": "book", "title": "T", "author": {names}}}"#
        ));
        assert_eq!(field(&e, "author").as_deref(), Some(bibtex), "{names}");
    }
}

#[test]
fn ris_names() {
    for (name, bibtex) in [
        ("Smith", "Smith"),
        ("John Smith", "{John Smith}"),
        ("Smith, John", "Smith, John"),
        ("Smith,", "Smith"),
        ("Smith, John, Jr", "Smith, Jr, John"),
        // an empty given name keeps its place: the suffix comes back as a suffix
        ("Doe, , Jr", "Doe, Jr,"),
        ("Smith, John, ", "Smith, John"),
        (" Smith ,  John ", "Smith, John"),
        ("{A, B}, C", "{A, C, B}"),
    ] {
        assert_eq!(bibtex_name_of_ris(name), bibtex, "{name}");
        let e = ris_one(&["BOOK", &format!("AU  - {name}")]);
        assert_eq!(field(&e, "author").as_deref(), Some(bibtex), "{name}");
    }
}

// dates, 2.4
// ----------

#[test]
fn a_zero_or_missing_part_ends_the_date() {
    // the positions count: the day of 2013-00-05 is not a month
    for (date, expected) in [
        ("2013", vec![("year", "2013")]),
        ("2013-05", vec![("year", "2013"), ("month", "5")]),
        ("2013/05/02/", vec![("year", "2013"), ("month", "5")]),
        (" 2013-05-02T00:00", vec![("year", "2013"), ("month", "5")]),
        ("13-05", vec![]),
        ("2013-00-05", vec![("year", "2013")]),
        ("2013-05-00", vec![("year", "2013"), ("month", "5")]),
        ("2013-5-0", vec![("year", "2013"), ("month", "5")]),
        ("May 2013", vec![]),
    ] {
        let e = csl_one(&format!(
            r#"{{"type": "book", "title": "T", "issued": {{"raw": "{date}"}}}}"#
        ));
        let got: Vec<(String, String)> = fields(&e)
            .into_iter()
            .filter(|(k, _)| k == "year" || k == "month")
            .collect();
        assert_eq!(got, pairs(&expected), "{date}");
    }
    for (parts, expected) in [
        ("[2013, 0, 5]", vec![("year", "2013")]),
        (r#"["2013", "", "5"]"#, vec![("year", "2013")]),
        ("[2013, null, 5]", vec![("year", "2013")]),
        ("[2013, 5, 0]", vec![("year", "2013"), ("month", "5")]),
    ] {
        let e = csl_one(&format!(
            r#"{{"type": "book", "title": "T", "issued": {{"date-parts": [{parts}]}}}}"#
        ));
        let got: Vec<(String, String)> = fields(&e)
            .into_iter()
            .filter(|(k, _)| k == "year" || k == "month")
            .collect();
        assert_eq!(got, pairs(&expected), "{parts}");
    }
}

// the special characters of LaTeX, 2.9
// ------------------------------------

#[test]
fn csl_special_characters_are_escaped_except_the_raw_fields() {
    let e = csl_one(
        r##"{"id": "x", "type": "article-journal", "title": "R&D 5% #1 a_b \\& done", "note": "a&b",
            "author": [{"family": "AT&T"}], "URL": "https://a.org/a_b?x=1&y=2", "DOI": "10.1/a_b%",
            "ISBN": "1_2", "ISSN": "3_4", "keyword": "a_b, c&d", "accessed": {"raw": "2020-01-02"},
            "custom": {"file": ["files/a_b.pdf"]}}"##,
    );
    let mut got = fields(&e);
    got.sort();
    let mut expected = pairs(&[
        ("author", r"AT\&T"),
        ("title", r"R\&D 5\% \#1 a\_b \& done"),
        ("url", "https://a.org/a_b?x=1&y=2"),
        ("doi", "10.1/a_b%"),
        ("isbn", "1_2"),
        ("issn", "3_4"),
        ("note", r"a\&b"),
        ("keywords", "a_b, c&d"),
        ("urldate", "2020-01-02"),
        ("file", ":files/a_b.pdf:pdf"),
    ]);
    expected.sort();
    assert_eq!(got, expected);
    // an escaped character stays as it is, and the export gives the text back
    let e = csl_one(r#"{"id": "x", "type": "book", "title": "R\\&D"}"#);
    assert_eq!(field(&e, "title").as_deref(), Some(r"R\&D"));
    let e = csl_one(
        r#"{"id": "x", "type": "article-journal", "title": "R&D: 5% of #1 in a_b", "container-title": "Science & Policy"}"#,
    );
    assert_eq!(field(&e, "journal").as_deref(), Some(r"Science \& Policy"));
    let bibtex = format!("@article{{x, title = {{{}}}}}", field(&e, "title").unwrap());
    assert_eq!(csl(&bibtex)[0]["title"], "R&D: 5% of #1 in a_b");
}

#[test]
fn ris_special_characters_are_escaped_except_the_raw_fields() {
    let e = ris_one(&[
        "BOOK",
        "TI  - A&B_C",
        "UR  - https://a.org/a_b",
        "KW  - a_b",
        "Y2  - 2020_01",
        "SN  - 1_2",
        "L1  - files/a_b.pdf",
        "AU  - AT&T",
    ]);
    assert_eq!(field(&e, "title").as_deref(), Some(r"A\&B\_C"));
    assert_eq!(field(&e, "author").as_deref(), Some(r"AT\&T"));
    assert_eq!(field(&e, "url").as_deref(), Some("https://a.org/a_b"));
    assert_eq!(field(&e, "keywords").as_deref(), Some("a_b"));
    assert_eq!(field(&e, "urldate").as_deref(), Some("2020_01"));
    assert_eq!(field(&e, "isbn").as_deref(), Some("1_2"));
    assert_eq!(field(&e, "file").as_deref(), Some(":files/a_b.pdf:pdf"));
}

#[test]
fn bibjson_values_are_read_unchanged() {
    let e = bibjson_one(r#"{"id": "x", "type": "book", "title": "R&D 5% a_b"}"#);
    assert_eq!(field(&e, "title").as_deref(), Some("R&D 5% a_b"));
}

// CSL-JSON, 3
// -----------

#[test]
fn csl_keys_in_the_order_of_the_table() {
    let keys = csl_keys(
        r"@article{k,
 urldate = {2024-03-01}, keywords = {b, a}, month = may, year = {2013}, pages = {1--2}, number = {3},
 journal = {J}, pmcid = {PMC1}, pmid = {1}, series = {S}, language = {en}, edition = {2}, note = {N},
 abstract = {A}, address = {Potsdam}, publisher = {P}, issn = {1234-5678}, isbn = {978}, url = {https://u},
 doi = {10.1/x}, volume = {4}, title = {T}, translator = {Roe, R.}, editor = {Ed, E.}, author = {Doe, J.},
 eprint = {e}, crossref = {c}, ids = {old}, papers-x = {y}}",
        &["files/a.pdf"],
    );
    assert_eq!(
        keys,
        [
            "id",
            "citation-key",
            "type",
            "author",
            "editor",
            "translator",
            "title",
            "volume",
            "DOI",
            "URL",
            "ISBN",
            "ISSN",
            "publisher",
            "publisher-place",
            "abstract",
            "note",
            "edition",
            "language",
            "collection-title",
            "PMID",
            "PMCID",
            "container-title",
            "issue",
            "page",
            "keyword",
            "issued",
            "accessed",
            "custom"
        ]
    );
}

#[test]
fn the_publisher_of_a_thesis_or_report_is_at_the_position_of_publisher() {
    let keys = csl_keys(
        "@techreport{k, title = {T}, institution = {I}, organization = {O}, address = {A}, type = {Working paper}, number = {7}, year = {2020}}",
        &[],
    );
    assert_eq!(
        keys,
        [
            "id",
            "citation-key",
            "type",
            "title",
            "publisher",
            "publisher-place",
            "number",
            "genre",
            "issued"
        ]
    );
    let item = &csl(
        "@techreport{k, title = {T}, institution = {I}, organization = {O}, address = {A}, type = {Working paper}, number = {7}, year = {2020}}",
    )[0];
    assert_eq!(item["publisher"], "I");
    assert_eq!(item["genre"], "Working paper");
    assert_eq!(item["number"], "7");
    for (bibtex, genre) in [
        (
            "@phdthesis{k, type = {Diss.}, school = {S}}",
            Some("PhD thesis"),
        ),
        ("@mastersthesis{k, school = {S}}", Some("Master's thesis")),
        ("@thesis{k, type = {Habil.}}", Some("Habil.")),
        ("@manual{k, type = {Guide}}", Some("Guide")),
        ("@book{k, type = {Novel}}", None),
        ("@thesis{k}", None),
    ] {
        assert_eq!(
            csl(bibtex)[0].get("genre").and_then(Value::as_str),
            genre,
            "{bibtex}"
        );
    }
    assert_eq!(
        csl("@book{k, publisher = {P}, school = {S}}")[0]["publisher"],
        "P"
    );
    assert_eq!(csl("@book{k, organization = {O}}")[0]["publisher"], "O");
    // a thesis: the publisher comes before the place of publication
    let keys = csl_keys(
        "@phdthesis{x, title = {T}, school = {ETH}, pages = {1--2}, address = {Zurich}}",
        &[],
    );
    let at = keys.iter().position(|k| k == "publisher").unwrap();
    assert_eq!(keys[at + 1], "publisher-place");
}

#[test]
fn container_title_is_the_booktitle_of_the_parts_of_a_book_only() {
    for (csl_type, name) in [
        ("chapter", "booktitle"),
        ("paper-conference", "booktitle"),
        ("entry-encyclopedia", "booktitle"),
        ("article-journal", "journal"),
        ("book", "journal"),
        ("dataset", "journal"),
        ("report", "journal"),
        ("manuscript", "journal"),
    ] {
        let e = csl_one(&format!(
            r#"{{"type": "{csl_type}", "title": "T", "container-title": "C"}}"#
        ));
        assert_eq!(field(&e, name).as_deref(), Some("C"), "{csl_type}");
    }
}

#[test]
fn csl_import_fields() {
    let e = csl_one(
        r#"{"type": "book", "title": ["First", "Second"], "volume": 4, "DOI": "doi:10.1/x", "URL": "https://u",
            "ISBN": "978", "ISSN": "1234", "publisher": "P", "publisher-place": "Potsdam", "abstract": "A",
            "note": "N", "edition": 2, "language": "en", "collection-title": "S", "PMID": "1", "PMCID": "PMC1",
            "other": "ignored"}"#,
    );
    assert_eq!(
        fields(&e),
        pairs(&[
            ("title", "First"),
            ("volume", "4"),
            ("doi", "10.1/x"),
            ("url", "https://u"),
            ("isbn", "978"),
            ("issn", "1234"),
            ("publisher", "P"),
            ("address", "Potsdam"),
            ("abstract", "A"),
            ("note", "N"),
            ("edition", "2"),
            ("language", "en"),
            ("series", "S"),
            ("pmid", "1"),
            ("pmcid", "PMC1"),
        ])
    );
    let one = |item: &str| fields(&csl_one(item));
    assert_eq!(
        one(r#"{"type": "thesis", "publisher": "ETH"}"#),
        pairs(&[("school", "ETH")])
    );
    assert_eq!(
        one(r#"{"type": "thesis", "genre": "Master's", "publisher": "ETH"}"#),
        pairs(&[("school", "ETH")])
    );
    assert_eq!(
        one(r#"{"type": "report", "publisher": "PIK", "genre": "WP"}"#),
        pairs(&[("institution", "PIK"), ("type", "WP")])
    );
    assert_eq!(
        one(r#"{"type": "book", "publisher": "P", "genre": "G"}"#),
        pairs(&[("publisher", "P")])
    );
    assert_eq!(
        field(
            &csl_one(r#"{"type": "book", "keyword": ["a; b", "c"]}"#),
            "keywords"
        )
        .as_deref(),
        Some("a, b, c")
    );
}

// RIS, 4
// ------

#[test]
fn ris_lines_and_records() {
    // 4.1: a tag line is [A-Z][A-Z0-9] + two spaces + a dash + a separator; other lines continue
    let e = ris_one(&[
        "BOOK",
        "TI  - Title",
        "ti  - lower",
        "1A  - not a tag",
        "T1  -x",
        "KW  - k",
        "kw  - also continued",
        "KW  -",
    ]);
    assert_eq!(
        field(&e, "title").as_deref(),
        Some("Title ti  - lower 1A  - not a tag")
    );
    assert_eq!(
        field(&e, "keywords").as_deref(),
        Some("k, kw  - also continued")
    );
    assert_eq!(
        field(&ris_one(&["BOOK", "T1  -XTitle"]), "title").as_deref(),
        Some("Title")
    );
    // 4.2: lines outside records skipped, records from TY to ER, the last without ER read
    let records = "Provider: x\n\nTY  - BOOK\nTI  - One\nER  - \nbetween\n\nTY  - JOUR\nTI  - Two\nER  -\nTY  - GEN\nTI  - Three\n";
    let titles: Vec<_> = ris_import(records)
        .unwrap()
        .iter()
        .map(|e| field(e, "title").unwrap())
        .collect();
    assert_eq!(titles, ["One", "Two", "Three"]);
    // a continuation adds an item to a list tag, a space and text to a single tag
    let e = ris_one(&[
        "BOOK",
        "TI  - A long",
        "   title  ",
        "AB  - first",
        "second",
        "AU  - Doe, J",
        "Roe, R",
    ]);
    assert_eq!(field(&e, "title").as_deref(), Some("A long title"));
    assert_eq!(field(&e, "abstract").as_deref(), Some("first second"));
    assert_eq!(field(&e, "author").as_deref(), Some("Doe, J and Roe, R"));
    // single tags keep their first value, list tags all; UR split on ;
    let e = ris_one(&[
        "JOUR",
        "TI  - First",
        "TI  - Second",
        "VL  - 1",
        "VL  - 2",
        "A1  - Doe, J",
        "A1  - Roe, R",
        "UR  - https://a.org; https://b.org",
    ]);
    assert_eq!(field(&e, "title").as_deref(), Some("First"));
    assert_eq!(field(&e, "volume").as_deref(), Some("1"));
    assert_eq!(field(&e, "author").as_deref(), Some("Doe, J and Roe, R"));
    assert_eq!(field(&e, "url").as_deref(), Some("https://a.org"));
    // unknown tags ignored; a text without record is an error, a blank text is empty
    let e = ris_one(&["BOOK", "TI  - T", "ZZ  - unknown", "C1  - custom"]);
    assert_eq!(fields(&e), pairs(&[("title", "T")]));
    assert_eq!(
        ris_import("no record\nhere\n").unwrap_err(),
        "not a RIS file: no record found (a record starts with TY  - )"
    );
    assert!(ris_import(" \n\n").unwrap().is_empty());
}

#[test]
fn ris_stray_lines_are_dropped() {
    // 4.2: blank lines in a record, and lines between TY and the first tag
    let e = ris_one(&["BOOK", "stray", "", "KW  - a", "", "  ", "TI  - T"]);
    assert_eq!(field(&e, "title").as_deref(), Some("T"));
    assert_eq!(field(&e, "keywords").as_deref(), Some("a"));
    // a blank line after TY, in a record that follows one that ends in a single tag
    let records = "TY  - JOUR\nKW  - a\nTI  - First\nER  - \n\nTY  - JOUR\n\nTI  - Second\nKW  - b\n\nAU  - Smith, J\nER  - \n\nTY  - JOUR\nstray text\nTI  - Third\nER  - \n";
    let entries = ris_import(records).unwrap();
    let titles: Vec<_> = entries.iter().map(|e| field(e, "title").unwrap()).collect();
    assert_eq!(titles, ["First", "Second", "Third"]);
    assert_eq!(field(&entries[1], "keywords").as_deref(), Some("b"));
    assert_eq!(field(&entries[1], "author").as_deref(), Some("Smith, J"));
    // a tag line as the reader takes it (the 6th character is the separator), also right after TY
    let entries = ris_import("TY  - JOUR\nTI  -xA title\nAU  - Smith, J\nER  - \n").unwrap();
    assert_eq!(field(&entries[0], "title").as_deref(), Some("A title"));
    assert_eq!(field(&entries[0], "author").as_deref(), Some("Smith, J"));
}

#[test]
fn ris_last_record_without_er() {
    let entries =
        ris_import("TY  - JOUR\nTI  - First\nER  - \n\nTY  - JOUR\nTI  - Second\n\n").unwrap();
    let titles: Vec<_> = entries.iter().map(|e| field(e, "title").unwrap()).collect();
    assert_eq!(titles, ["First", "Second"]);
    let entries = ris_import("TY  - JOUR\nTI  - Only").unwrap();
    assert_eq!(field(&entries[0], "title").as_deref(), Some("Only"));
}

#[test]
fn ris_urldate_translator_and_suffix() {
    let bibtex = "@book{x,\n title = {T},\n translator = {Doe, Jane and Roe, R.},\n url = {https://example.org},\n urldate = {2024-03-01}\n}\n";
    let ris = text(bibtex, Format::Ris, &[]);
    for line in ["A4  - Doe, Jane", "A4  - Roe, R.", "Y2  - 2024-03-01"] {
        assert!(ris.contains(&format!("{line}\n")), "{line}: {ris}");
    }
    let back = &ris_import(&ris).unwrap()[0];
    assert_eq!(
        field(back, "translator").as_deref(),
        Some("Doe, Jane and Roe, R.")
    );
    assert_eq!(field(back, "urldate").as_deref(), Some("2024-03-01"));
    // a suffix without given name stays a suffix
    let ris = text(
        "@book{x,\n title = {T},\n author = {Doe, Jr,}\n}\n",
        Format::Ris,
        &[],
    );
    assert!(ris.contains("AU  - Doe, , Jr\n"), "{ris}");
    let back = &ris_import(&ris).unwrap()[0];
    assert_eq!(field(back, "author").as_deref(), Some("Doe, Jr,"));
}

#[test]
fn ris_container_is_the_journal_except_for_the_parts_of_a_book() {
    assert_eq!(
        field(&ris_one(&["RPRT", "TI  - T", "T2  - S"]), "journal").as_deref(),
        Some("S")
    );
    assert_eq!(
        field(&ris_one(&["CHAP", "TI  - T", "T2  - S"]), "booktitle").as_deref(),
        Some("S")
    );
    for (ris, name) in [
        ("CHAP", "booktitle"),
        ("CPAPER", "booktitle"),
        ("BOOK", "journal"),
        ("GEN", "journal"),
        ("JOUR", "journal"),
    ] {
        assert_eq!(
            field(&ris_one(&[ris, "T2  - C"]), name).as_deref(),
            Some("C"),
            "{ris}"
        );
    }
    assert_eq!(
        field(
            &ris_one(&["JOUR", "J2  - E", "JA  - D", "JO  - C", "JF  - B"]),
            "journal"
        )
        .as_deref(),
        Some("B")
    );
    assert_eq!(
        field(&ris_one(&["JOUR", "J2  - E", "JA  - D"]), "journal").as_deref(),
        Some("D")
    );
}

#[test]
fn ris_doi_of_a_url_next_to_another() {
    // no DO line: the DOI of the doi.org link, also when another URL is there
    let e = ris_one(&[
        "JOUR",
        "TI  - T",
        "UR  - https://example.org/paper",
        "UR  - https://doi.org/10.1000/xyz",
    ]);
    assert_eq!(field(&e, "doi").as_deref(), Some("10.1000/xyz"));
    assert_eq!(
        field(&e, "url").as_deref(),
        Some("https://example.org/paper")
    );
    let e = ris_one(&[
        "JOUR",
        "DO  - https://doi.org/10.1/x",
        "UR  - https://doi.org/10.2/y",
        "UR  - https://a.org",
    ]);
    assert_eq!(field(&e, "doi").as_deref(), Some("10.1/x"));
    assert_eq!(field(&e, "url").as_deref(), Some("https://a.org"));
    let e = ris_one(&["JOUR", "UR  - https://doi.org/10.1/x; https://b.org"]);
    assert_eq!(field(&e, "url").as_deref(), Some("https://b.org"));
    assert_eq!(field(&e, "doi").as_deref(), Some("10.1/x"));
}

#[test]
fn ris_mastersthesis_round_trip() {
    let bibtex = "@mastersthesis{x,\n title = {T},\n school = {ETH},\n year = {2020}\n}\n";
    let ris = text(bibtex, Format::Ris, &[]);
    assert!(ris.contains("M3  - Master's thesis\n"), "{ris}");
    let back = &ris_import(&ris).unwrap()[0];
    assert_eq!(back.entry_type, "mastersthesis");
    assert_eq!(
        fields(back),
        pairs(&[("title", "T"), ("school", "ETH"), ("year", "2020")])
    );
    let phd = ris.replace("M3  - Master's thesis\n", "");
    assert_eq!(ris_import(&phd).unwrap()[0].entry_type, "phdthesis");
    assert_eq!(
        ris_one(&["THES", "M3  - MASTER thesis"]).entry_type,
        "mastersthesis"
    );
    assert_eq!(ris_one(&["THES", "M3  - Diplom"]).entry_type, "phdthesis");
    // the M3 of a thesis with a type: the type
    let ris = text("@mastersthesis{k, type = {Diplom}}", Format::Ris, &[]);
    assert!(
        ris.contains("M3  - Diplom\n") && !ris.contains("Master"),
        "{ris}"
    );
    assert!(!text("@phdthesis{k}", Format::Ris, &[]).contains("M3"));
}

#[test]
fn ris_import_fields() {
    let e = ris_one(&[
        "CHAP",
        "A1  - First, A",
        "AU  - Second, B",
        "A2  - Ed, E",
        "A4  - Tr, T",
    ]);
    assert_eq!(
        field(&e, "author").as_deref(),
        Some("Second, B and First, A")
    );
    assert_eq!(field(&e, "editor").as_deref(), Some("Ed, E"));
    assert_eq!(field(&e, "translator").as_deref(), Some("Tr, T"));
    assert_eq!(
        field(&ris_one(&["JOUR", "AU  - Doe, J", "A2  - Ed, E"]), "editor"),
        None
    );
    assert_eq!(
        field(&ris_one(&["BOOK", "ED  - One, O", "A2  - Ed, E"]), "editor").as_deref(),
        Some("One, O")
    );
    let e = ris_one(&[
        "RPRT",
        "ID  - rep2020",
        "PB  - PIK",
        "PB  - other",
        "CY  - Potsdam",
        "N2  - Abstract 2",
        "KW  - a; b",
        "KW  - c, d",
        "N1  - one ",
        "N1  - ",
        "N1  - two",
        "ET  - 2",
        "LA  - en",
        "M3  - WP",
        "Y2  - 2020-01-02",
    ]);
    assert_eq!(e.key, "rep2020");
    assert_eq!(
        fields(&e),
        pairs(&[
            ("institution", "PIK"),
            ("address", "Potsdam"),
            ("abstract", "Abstract 2"),
            ("keywords", "a, b, c, d"),
            ("note", "one; two"),
            ("edition", "2"),
            ("language", "en"),
            ("type", "WP"),
            ("urldate", "2020-01-02"),
        ])
    );
    assert_eq!(
        fields(&ris_one(&["THES", "PB  - ETH", "M3  - PhD"])),
        pairs(&[("school", "ETH"), ("type", "PhD")])
    );
    // a master's thesis: the school, no type (its M3 only decides the entry type)
    assert_eq!(
        fields(&ris_one(&["THES", "PB  - ETH", "M3  - Master"])),
        pairs(&[("school", "ETH")])
    );
    assert_eq!(
        fields(&ris_one(&["BOOK", "PB  - P", "M3  - Novel"])),
        pairs(&[("publisher", "P")])
    );
}

#[test]
fn ris_writes_the_translators_and_the_access_date() {
    // 4.3: ED, A4, T3, IS from issue, SN isbn then issn, PB fallbacks, ET, LA, Y2
    let ris = text(
        "@incollection{k, editor = {Ed, E. and {Org}}, translator = {Tr, T}, booktitle = {B}, series = {S}, issue = {2}, number = {3},\
         isbn = {978}, issn = {1234-5678}, organization = {O}, edition = {2}, language = {en}, type = {Chapter}, urldate = {2024-03-01}}",
        Format::Ris,
        &[],
    );
    assert_eq!(
        ris,
        "TY  - CHAP\nID  - k\nED  - Ed, E.\nED  - Org\nA4  - Tr, T\nT2  - B\nT3  - S\nIS  - 2\nY2  - 2024-03-01\nSN  - 978\nSN  - 1234-5678\nPB  - O\nET  - 2\nLA  - en\nER  - \n"
    );
}

// BibJSON, 5
// ----------

#[test]
fn bibjson_a_field_named_as_a_key_is_namespaced() {
    let text = text(
        "@techreport{wp2020,\n id = {X1},\n title = {T},\n type = {Working paper}\n}",
        Format::BibJson,
        &[],
    );
    let collection: Value = serde_json::from_str(&text).unwrap();
    let record = &collection["records"][0];
    assert_eq!(
        (&record["type"], &record["id"]),
        (&json!("techreport"), &json!("wp2020"))
    );
    assert_eq!(
        (&record["bibtex:type"], &record["bibtex:id"]),
        (&json!("Working paper"), &json!("X1"))
    );
    assert!(collection["metadata"]["namespace"].get("bibtex").is_some());
    let back = &loads(&text, Format::BibJson).unwrap()[0];
    assert_eq!(
        (back.key.as_str(), back.entry_type.as_str()),
        ("wp2020", "techreport")
    );
    assert_eq!(field(back, "type").as_deref(), Some("Working paper"));
    assert_eq!(field(back, "id").as_deref(), Some("X1"));
}

/// A field named `file-version` goes in `bibtex:file-version`, and a record's `file-version` (the
/// versions of `papers list`) is not read as a field (formats.md 5.1; `test_pdf_versions.py`,
/// `TestBibjsonField`).
#[test]
fn bibjson_a_field_named_file_version() {
    let text = text("@misc{a,\n file-version = {x}\n}\n", Format::BibJson, &[]);
    let collection: Value = serde_json::from_str(&text).unwrap();
    let record = &collection["records"][0];
    assert_eq!(record["bibtex:file-version"], json!("x"));
    assert!(record.get("file-version").is_none());
    assert_eq!(
        field(&loads(&text, Format::BibJson).unwrap()[0], "file-version").as_deref(),
        Some("x")
    );
    let e = bibjson_one(
        r#"{"type": "misc", "id": "a", "file": ["a.pdf"], "file-version": {"a.pdf": "accepted"}}"#,
    );
    assert_eq!(field(&e, "file-version"), None);
}

#[test]
fn bibjson_without_namespace_and_the_keys_that_have_a_meaning() {
    let collection: Value =
        serde_json::from_str(&text("@book{k, title = {T}}", Format::BibJson, &[])).unwrap();
    assert_eq!(collection["metadata"], json!({"records": 1}));
    for name in [
        "type",
        "id",
        "cid",
        "collection",
        "identifier",
        "link",
        "license",
    ] {
        let collection: Value = serde_json::from_str(&text(
            &format!("@book{{k, {name} = {{x}}}}"),
            Format::BibJson,
            &[],
        ))
        .unwrap();
        assert_eq!(
            collection["records"][0],
            json!({"type": "book", "id": "k", format!("bibtex:{name}"): "x"}),
            "{name}"
        );
    }
}

#[test]
fn bibjson_reading() {
    let e = bibjson_one(
        r#"{"type": "TechReport", "cid": "c2000", "bibtex:type": "WP", "bibtex:any": "x", "_x": 1,
            "collection": "c", "identifier": [], "link": [], "license": "l", "note": null}"#,
    );
    assert_eq!(
        (e.entry_type.as_str(), e.key.as_str()),
        ("techreport", "c2000")
    );
    assert_eq!(fields(&e), pairs(&[("type", "WP"), ("any", "x")]));
    let e = bibjson_one(r#"{"type": "", "id": "i2000", "cid": "c2000"}"#);
    assert_eq!((e.entry_type.as_str(), e.key.as_str()), ("misc", "i2000"));
    // numbers as written, booleans, null
    let e = bibjson_one(
        r#"{"type": "misc", "id": "x", "year": 2000, "number": 0.0000001, "volume": 1E3,
            "keywords": ["a", null, "b"], "author": [null, {"name": "Doe, J"}]}"#,
    );
    assert_eq!(
        fields(&e),
        pairs(&[
            ("year", "2000"),
            ("number", "0.0000001"),
            ("volume", "1E3"),
            ("keywords", "a, b"),
            ("author", "Doe, J")
        ])
    );
    let e = bibjson_one(
        r#"{"type": "misc", "id": "x", "year": 2000.0, "note": null, "peer-reviewed": true}"#,
    );
    assert_eq!(field(&e, "year").as_deref(), Some("2000.0"));
    assert_eq!(field(&e, "note"), None);
    assert_eq!(field(&e, "peer-reviewed").as_deref(), Some("true"));
    let e = bibjson_one(
        r#"{"author": ["Doe, J", {"name": "Roe, R"}, null], "editor": "Ed, E", "journal": {"title": "x"},
            "publisher": {"name": "P"}, "keywords": ["a", null, ["b", "c"]], "ok": true, "no": false,
            "year": 2000, "pages": 1.50, "volume": -1e3}"#,
    );
    assert_eq!(
        fields(&e),
        pairs(&[
            ("author", "Doe, J and Roe, R"),
            ("editor", "Ed, E"),
            ("publisher", "P"),
            ("keywords", "a, b, c"),
            ("ok", "true"),
            ("no", "false"),
            ("year", "2000"),
            ("pages", "1.50"),
            ("volume", "-1e3"),
        ])
    );
}

#[test]
fn bibjson_round_trip_loses_only_the_fields_that_start_with_an_underscore() {
    let text = text(
        "@book{k, title = {T{\\\"o}}, type = {X}, _private = {p}, month = may}",
        Format::BibJson,
        &[],
    );
    let back = &loads(&text, Format::BibJson).unwrap()[0];
    assert_eq!(
        fields(back),
        pairs(&[("title", "T{\\\"o}"), ("type", "X"), ("month", "may")])
    );
}
