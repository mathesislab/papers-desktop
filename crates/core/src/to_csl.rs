//! CSL-JSON of the bibtex entries, for a citation processor.
//!
//! The mapping is that of `entry_to_csl` of papers (`papers/formats.py`),
//! without the file links.

use biblatex::{Date, DateValue, Entry, EntryType, PermissiveType};
use serde_json::{Map, Value, json};

use crate::fields::{Name, clean_doi, field, names, raw};

/// The fields copied as text: the bibtex names (the first one found) and the CSL name.
const TEXT_FIELDS: [(&[&str], &str); 11] = [
    (&["title"], "title"),
    (&["volume"], "volume"),
    (&["publisher"], "publisher"),
    (&["address", "location"], "publisher-place"),
    (&["abstract"], "abstract"),
    (&["note"], "note"),
    (&["edition"], "edition"),
    (&["language"], "language"),
    (&["series"], "collection-title"),
    (&["pmid"], "PMID"),
    (&["pmcid"], "PMCID"),
];

/// The fields copied as written (links, identifiers).
const RAW_FIELDS: [(&str, &str); 3] = [("url", "URL"), ("isbn", "ISBN"), ("issn", "ISSN")];

/// The CSL-JSON item of a bibtex entry (its id is the key).
pub(crate) fn entry_to_csl(entry: &Entry) -> Value {
    let kind = csl_type(entry);
    let mut item = Map::new();
    for key in ["id", "citation-key"] {
        item.insert(key.to_string(), Value::String(entry.key.clone()));
    }
    item.insert("type".to_string(), Value::String(kind.to_string()));
    for role in ["author", "editor", "translator"] {
        let names: Vec<Value> = names(entry, role).iter().map(csl_name).collect();
        if !names.is_empty() {
            item.insert(role.to_string(), Value::Array(names));
        }
    }
    for (keys, csl) in TEXT_FIELDS {
        put(&mut item, csl, field(entry, keys));
    }
    for (key, csl) in RAW_FIELDS {
        put(&mut item, csl, raw(entry, key));
    }
    put(&mut item, "DOI", raw(entry, "doi").map(clean_doi));
    put(
        &mut item,
        "container-title",
        field(entry, &["journal", "journaltitle", "booktitle"]),
    );
    // the number of an article is its issue
    let issue = field(entry, &["issue"]);
    let number = if kind == "article-journal" || issue.is_some() {
        "issue"
    } else {
        "number"
    };
    put(
        &mut item,
        number,
        issue.or_else(|| field(entry, &["number"])),
    );
    put(&mut item, "page", field(entry, &["pages"]).map(csl_pages));
    if !item.contains_key("publisher") {
        let institution = field(entry, &["school", "institution", "organization"]);
        put(&mut item, "publisher", institution);
    }
    put(&mut item, "genre", genre(entry, kind));
    put(&mut item, "keyword", keywords(entry));
    match entry.date() {
        Ok(PermissiveType::Typed(date)) => {
            item.insert("issued".to_string(), csl_date(date));
        }
        // a year that is not a number ("in press")
        _ => {
            if let Some(year) = field(entry, &["year"]) {
                item.insert("issued".to_string(), json!({ "literal": year }));
            }
        }
    }
    if let Ok(PermissiveType::Typed(date)) = entry.url_date() {
        item.insert("accessed".to_string(), csl_date(date));
    }
    Value::Object(item)
}

/// Sets a text of the item, if there is one.
fn put(item: &mut Map<String, Value>, key: &str, text: Option<String>) {
    if let Some(text) = text {
        item.insert(key.to_string(), Value::String(text));
    }
}

/// The CSL type of an entry: a `misc` with an `eprint` is a preprint.
fn csl_type(entry: &Entry) -> &'static str {
    match &entry.entry_type {
        EntryType::Article => "article-journal",
        EntryType::Book | EntryType::Proceedings => "book",
        EntryType::Booklet => "pamphlet",
        EntryType::InBook | EntryType::InCollection => "chapter",
        EntryType::InProceedings => "paper-conference",
        EntryType::PhdThesis | EntryType::MastersThesis | EntryType::Thesis => "thesis",
        EntryType::TechReport | EntryType::Report | EntryType::Manual => "report",
        EntryType::Misc if field(entry, &["eprint"]).is_some() => "article",
        EntryType::Online => "webpage",
        EntryType::Dataset => "dataset",
        EntryType::Software => "software",
        EntryType::Unpublished => "manuscript",
        EntryType::Patent => "patent",
        _ => "document",
    }
}

/// The kind of a thesis or report: fixed for the bibtex thesis types, else its `type` field.
fn genre(entry: &Entry, kind: &str) -> Option<String> {
    match entry.entry_type {
        EntryType::PhdThesis => Some("PhD thesis".to_string()),
        EntryType::MastersThesis => Some("Master's thesis".to_string()),
        _ if matches!(kind, "thesis" | "report") => field(entry, &["type"]),
        _ => None,
    }
}

/// The comma-separated keywords, trimmed.
fn keywords(entry: &Entry) -> Option<String> {
    let keywords = field(entry, &["keywords"])?;
    let words: Vec<_> = keywords
        .split(',')
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .collect();
    (!words.is_empty()).then(|| words.join(", "))
}

/// `123--130`, `123–130` or `123 - 130` as `123-130`; other pages as they are.
fn csl_pages(pages: String) -> String {
    let parts: Vec<_> = pages
        .split(['-', '–', '—'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    match parts[..] {
        [first, last] => format!("{first}-{last}"),
        _ => pages,
    }
}

/// The CSL name of a person: a literal for an organisation.
fn csl_name(name: &Name) -> Value {
    if let Some(literal) = &name.literal {
        return json!({ "literal": literal });
    }
    let mut csl = Map::new();
    for (key, text) in [
        ("family", &name.family),
        ("given", &name.given),
        ("non-dropping-particle", &name.particle),
        ("suffix", &name.suffix),
    ] {
        if !text.is_empty() {
            csl.insert(key.to_string(), Value::String(text.clone()));
        }
    }
    Value::Object(csl)
}

/// The CSL date of a date; of a range, the first date (as papers does).
fn csl_date(date: Date) -> Value {
    let (DateValue::At(start)
    | DateValue::After(start)
    | DateValue::Before(start)
    | DateValue::Between(start, _)) = date.value;
    // the month and the day of biblatex start at zero
    let parts: Vec<i64> = [
        Some(i64::from(start.year)),
        start.month.map(|month| i64::from(month) + 1),
        start.day.map(|day| i64::from(day) + 1),
    ]
    .into_iter()
    .flatten()
    .collect();
    json!({ "date-parts": [parts] })
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::process::Command;

    use biblatex::{Bibliography, Chunks, ChunksExt};

    use super::*;

    /// The CSL item of the first entry of a bibtex text.
    fn csl(src: &str) -> Value {
        let bib = Bibliography::parse(src).unwrap();
        entry_to_csl(bib.iter().next().unwrap())
    }

    #[test]
    fn article_with_particle_and_organisation_authors() {
        let item = csl(r#"@article{key2020,
            author = {van den Berg, Jan and {IPCC} and Ludwig van Beethoven and Smith, Jr., John and Doe},
            title = {Ice {Sheet} Dynamics},
            journal = {Nature},
            volume = {12}, number = {3}, pages = {100--110},
            year = {2020}, month = {mar},
            doi = {10.1000/xyz},
        }"#);
        assert_eq!(
            item,
            json!({
                "id": "key2020",
                "citation-key": "key2020",
                "type": "article-journal",
                "author": [
                    {"family": "Berg", "given": "Jan", "non-dropping-particle": "van den"},
                    {"literal": "IPCC"},
                    {"family": "Beethoven", "given": "Ludwig", "non-dropping-particle": "van"},
                    {"family": "Smith", "given": "John", "suffix": "Jr."},
                    {"family": "Doe"},
                ],
                "title": "Ice Sheet Dynamics",
                "container-title": "Nature",
                "volume": "12",
                "issue": "3",
                "page": "100-110",
                "DOI": "10.1000/xyz",
                "issued": {"date-parts": [[2020, 3]]},
            })
        );
    }

    #[test]
    fn names_in_braces_stay_together() {
        let item = csl(r#"@article{k,
            author = {{World Meteorological Organization} and {Colin de Verdière}, Alain and Maria {de la Cruz}},
        }"#);
        assert_eq!(
            item["author"],
            json!([
                {"literal": "World Meteorological Organization"},
                {"family": "Colin de Verdière", "given": "Alain"},
                {"family": "de la Cruz", "given": "Maria"},
            ])
        );
    }

    #[test]
    fn family_names_with_lowercase_words_are_not_particles() {
        // as papers reads `Last, First`: only the leading lowercase words are a particle
        let item = csl(r#"@article{k,
            author = {Colin de Verdière, Alain and Le clec'h, Sébastien and de la Cruz, Maria and Jean de la Fontaine},
        }"#);
        assert_eq!(
            item["author"],
            json!([
                {"family": "Colin de Verdière", "given": "Alain"},
                {"family": "Le clec'h", "given": "Sébastien"},
                {"family": "Cruz", "given": "Maria", "non-dropping-particle": "de la"},
                {"family": "Fontaine", "given": "Jean", "non-dropping-particle": "de la"},
            ])
        );
    }

    #[test]
    fn empty_names_are_left_out() {
        let item = csl("@article{k, author = {Smith, A. and  and Jones, B.}}");
        assert_eq!(
            item["author"],
            json!([
                {"family": "Smith", "given": "A."},
                {"family": "Jones", "given": "B."},
            ])
        );
    }

    #[test]
    fn book_with_editors() {
        let item = csl(r#"@book{b1,
            editor = {Doe, Jane and Roe, Richard},
            translator = {Poe, Edgar},
            title = {Handbook},
            publisher = {Springer},
            address = {Cham},
            series = {Lecture Notes},
            edition = {2},
            isbn = {978-3-319-24591-1},
            year = {2015},
        }"#);
        assert_eq!(
            item,
            json!({
                "id": "b1",
                "citation-key": "b1",
                "type": "book",
                "editor": [
                    {"family": "Doe", "given": "Jane"},
                    {"family": "Roe", "given": "Richard"},
                ],
                "translator": [{"family": "Poe", "given": "Edgar"}],
                "title": "Handbook",
                "publisher": "Springer",
                "publisher-place": "Cham",
                "collection-title": "Lecture Notes",
                "edition": "2",
                "ISBN": "978-3-319-24591-1",
                "issued": {"date-parts": [[2015]]},
            })
        );
    }

    #[test]
    fn phd_thesis() {
        let item = csl(r#"@phdthesis{t1,
            author = {Müller, Anna},
            title = {Ice},
            school = {University of Bremen},
            year = {2019},
        }"#);
        assert_eq!(item["type"], "thesis");
        assert_eq!(item["publisher"], "University of Bremen");
        assert_eq!(item["genre"], "PhD thesis");
        assert!(item.get("number").is_none());
    }

    #[test]
    fn masters_thesis_and_report_genres() {
        let item = csl("@mastersthesis{t, school = {AWI}, type = {ignored}}");
        assert_eq!(item["genre"], "Master's thesis");
        let item =
            csl("@techreport{r, institution = {AWI}, type = {Technical Report}, number = {7}}");
        assert_eq!(item["type"], "report");
        assert_eq!(item["genre"], "Technical Report");
        assert_eq!(item["publisher"], "AWI");
        assert_eq!(item["number"], "7");
        assert!(item.get("issue").is_none());
        let item = csl("@book{b, type = {Not a thesis}, publisher = {P}, organization = {O}}");
        assert!(item.get("genre").is_none());
        assert_eq!(item["publisher"], "P");
    }

    #[test]
    fn doi_without_its_link_or_prefix() {
        for doi in [
            "https://doi.org/10.1000/a_b(c)",
            "http://dx.doi.org/10.1000/a_b(c)",
            "DOI:10.1000/a_b(c)",
            "doi: 10.1000/a_b(c)",
            "10.1000/a_b(c)",
        ] {
            let item = csl(&format!(
                "@article{{k, doi = {{{doi}}}, url = {{https://doi.org/x}}}}"
            ));
            assert_eq!(item["DOI"], "10.1000/a_b(c)", "{doi}");
            assert_eq!(item["URL"], "https://doi.org/x");
        }
    }

    #[test]
    fn latex_is_read_as_text() {
        let item = csl(r#"@article{k,
            author = {M{\"u}ller, K. and {\'E}tienne, Jean and Sm{\o}rrebr{\o}d, A.},
            title = {The {\"O}sterreich {\ss}tra{\ss}e, caf\'e \& {\textless}10\% \emph{very} \textbf{big} {CO$_2$}},
            journal = {Journal of \{Things\}},
            note = {A{\textendash}B and Bose\textendash{}Einstein, \url{https://x.org}},
        }"#);
        assert_eq!(
            item["title"],
            "The Österreich ßtraße, café & <10% very big CO$_2$"
        );
        assert_eq!(item["container-title"], "Journal of Things");
        assert_eq!(item["note"], "A–B and Bose–Einstein, https://x.org");
        assert_eq!(
            item["author"],
            json!([
                {"family": "Müller", "given": "K."},
                {"family": "Étienne", "given": "Jean"},
                {"family": "Smørrebrød", "given": "A."},
            ])
        );
        for field in ["title", "container-title", "note"] {
            let text = item[field].as_str().unwrap();
            assert!(!text.contains(['{', '}', '\\']), "{text}");
        }
    }

    #[test]
    fn ties_are_spaces() {
        let item = csl(r#"@article{k,
            author = {Tolkien, J.~R.~R. and J.~R.~R.~Tolkien},
            title = {Fig.~1 of the paper},
        }"#);
        let tolkien = json!({"family": "Tolkien", "given": "J. R. R."});
        assert_eq!(item["author"], json!([tolkien, tolkien]));
        assert_eq!(item["title"], "Fig. 1 of the paper");
    }

    #[test]
    fn links_and_identifiers_are_as_written() {
        let item = csl(r#"@online{k,
            url = {https://x.org/~a/b\_c\%20d--e},
            doi = {10.1000/a\_b},
            issn = {1234-5678},
        }"#);
        assert_eq!(item["URL"], "https://x.org/~a/b_c%20d--e");
        assert_eq!(item["DOI"], "10.1000/a_b");
        assert_eq!(item["ISSN"], "1234-5678");
    }

    #[test]
    fn unicode_text_is_kept() {
        let item = csl("@article{k, title = {Ångström – Ölçüm {AMOC} “quoted” \u{2009}x}}");
        assert_eq!(item["title"], "Ångström – Ölçüm AMOC “quoted” x");
    }

    #[test]
    fn pages() {
        for (pages, page) in [
            ("100--110", "100-110"),
            ("100 -- 110", "100-110"),
            ("100–110", "100-110"),
            ("100-110", "100-110"),
            ("e1234", "e1234"),
            ("12", "12"),
            ("1-3, 7", "1-3, 7"),
        ] {
            let item = csl(&format!("@article{{k, pages = {{{pages}}}}}"));
            assert_eq!(item["page"], page, "{pages}");
        }
    }

    #[test]
    fn dates() {
        let issued = |fields: &str| {
            csl(&format!("@article{{k, {fields}}}"))
                .get("issued")
                .cloned()
        };
        assert_eq!(
            issued("year = {2018}"),
            Some(json!({"date-parts": [[2018]]}))
        );
        assert_eq!(
            issued("year = {2018}, month = {5}"),
            Some(json!({"date-parts": [[2018, 5]]}))
        );
        assert_eq!(
            issued("year = 2018, month = jan"),
            Some(json!({"date-parts": [[2018, 1]]}))
        );
        assert_eq!(
            issued("year = {2018}, month = {November}"),
            Some(json!({"date-parts": [[2018, 11]]}))
        );
        assert_eq!(
            issued("date = {2021-03-14}"),
            Some(json!({"date-parts": [[2021, 3, 14]]}))
        );
        assert_eq!(
            issued("date = {2021-03}"),
            Some(json!({"date-parts": [[2021, 3]]}))
        );
        assert_eq!(
            issued("year = {in press}"),
            Some(json!({"literal": "in press"}))
        );
        assert_eq!(issued("title = {No date}"), None);
    }

    #[test]
    fn access_date_and_keywords() {
        let item = csl(r#"@online{k,
            url = {https://x.org/a?b=1&c=2},
            urldate = {2021-05-02},
            keywords = {ice sheets, , modelling ,climate},
        }"#);
        assert_eq!(item["type"], "webpage");
        assert_eq!(item["URL"], "https://x.org/a?b=1&c=2");
        assert_eq!(item["accessed"], json!({"date-parts": [[2021, 5, 2]]}));
        assert_eq!(item["keyword"], "ice sheets, modelling, climate");
    }

    #[test]
    fn types() {
        for (entry, kind) in [
            ("@article{k,}", "article-journal"),
            ("@inproceedings{k,}", "paper-conference"),
            ("@conference{k,}", "paper-conference"),
            ("@incollection{k,}", "chapter"),
            ("@inbook{k,}", "chapter"),
            ("@proceedings{k,}", "book"),
            ("@booklet{k,}", "pamphlet"),
            ("@manual{k,}", "report"),
            ("@unpublished{k,}", "manuscript"),
            ("@misc{k,}", "document"),
            ("@misc{k, eprint = {2101.00001}}", "article"),
            ("@dataset{k,}", "dataset"),
            ("@www{k,}", "webpage"),
            ("@somethingelse{k,}", "document"),
        ] {
            assert_eq!(csl(entry)["type"], kind, "{entry}");
        }
    }

    #[test]
    fn issue_or_number() {
        assert_eq!(csl("@article{k, number = {3}}")["issue"], "3");
        let item = csl("@book{k, number = {3}}");
        assert_eq!(item["number"], "3");
        assert!(item.get("issue").is_none());
        let item = csl("@book{k, issue = {5}, number = {3}}");
        assert_eq!(item["issue"], "5");
        assert!(item.get("number").is_none());
    }

    #[test]
    fn biblatex_names_of_fields() {
        let item = csl(r#"@article{k,
            journaltitle = {Nature}, location = {London}, date = {2020-01-02},
        }"#);
        assert_eq!(item["container-title"], "Nature");
        assert_eq!(item["publisher-place"], "London");
        assert_eq!(item["issued"], json!({"date-parts": [[2020, 1, 2]]}));
    }

    /// The output of a papers command, run in a login shell (papers is on its PATH).
    fn papers(args: &str) -> String {
        let out = Command::new("zsh")
            .args(["-lc", &format!("papers {args}")])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "papers {args}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    /// A copy of a value with `f` applied to each of its strings.
    fn map_strings(value: &Value, f: &dyn Fn(&str) -> String) -> Value {
        match value {
            Value::String(text) => Value::String(f(text)),
            Value::Array(items) => {
                Value::Array(items.iter().map(|item| map_strings(item, f)).collect())
            }
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, value)| (key.clone(), map_strings(value, f)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// A text with its whitespace (spaces, newlines, no-break spaces) as single spaces.
    fn squeeze(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The names of a CSL names list without the particle `and` (see `cause`).
    fn without_and(names: &Value) -> Value {
        let mut names = names.clone();
        for name in names.as_array_mut().into_iter().flatten() {
            if name["non-dropping-particle"] == "and" {
                name.as_object_mut()
                    .unwrap()
                    .remove("non-dropping-particle");
            }
        }
        names
    }

    /// Whether a names field has an empty name (a doubled `and`).
    fn has_empty_name(entry: &Entry, key: &str) -> bool {
        let people = entry
            .get(key)
            .and_then(|chunks| chunks.parse::<Vec<Chunks>>().ok());
        people.is_some_and(|people| people.len() > names(entry, key).len())
    }

    /// Why papers and to_csl give different values of a field, when it is a
    /// known problem of the data (or of papers on this data), else `None`.
    fn cause(entry: &Entry, key: &str, ours: &Value, theirs: &Value) -> Option<&'static str> {
        let squeezed = map_strings(theirs, &squeeze);
        if map_strings(ours, &squeeze) == squeezed {
            return Some("whitespace: papers keeps newlines, runs and no-break spaces");
        }
        let unescaped = |text: &str| squeeze(&text.replace('&', " "));
        if map_strings(ours, &unescaped) == squeezed {
            return Some("`&`: papers reads an unescaped & (as in `&amp;`) as a space");
        }
        let angle = |text: &str| squeeze(&text.replace(['<', '>'], ""));
        if map_strings(ours, &angle) == squeezed {
            return Some("`<>`: papers drops the `\\textless` and `\\textgreater`");
        }
        if let (Some(ours), Some(theirs)) = (ours.as_str(), theirs.as_str())
            && let Some(percent) = ours.find('%')
            && squeeze(&ours[..percent]) == squeeze(theirs)
        {
            return Some("`%`: papers cuts the value at an unescaped %");
        }
        if has_empty_name(entry, key)
            && map_strings(&without_and(ours), &squeeze)
                == map_strings(&without_and(theirs), &squeeze)
        {
            return Some("`and and`: papers reads the name after an empty name as `and` + name");
        }
        None
    }

    #[test]
    #[ignore = "compares with the papers CLI on the real library"]
    fn matches_papers() {
        let path = papers("config bibtex");
        let src = std::fs::read_to_string(path.trim()).unwrap();
        let bib = Bibliography::parse(&src).unwrap();
        let theirs: Vec<Value> = serde_json::from_str(&papers("list --format csl-json")).unwrap();
        let count = theirs.len();

        let mut ids = BTreeSet::new();
        let mut compared = BTreeMap::<String, [usize; 3]>::new(); // equal, explained, unexplained
        let mut explained = BTreeMap::<(String, &str), Vec<(String, Value, Value)>>::new();
        let mut unexplained = vec![];
        let mut only_papers = vec![];
        for mut theirs in theirs {
            let id = theirs["id"].as_str().unwrap().to_string();
            let Some(entry) = bib.get(&id) else {
                only_papers.push(id);
                continue;
            };
            ids.insert(id.clone());
            let ours = entry_to_csl(entry);
            // the file links are left out of to_csl
            theirs.as_object_mut().unwrap().remove("custom");
            let (ours, theirs) = (ours.as_object().unwrap(), theirs.as_object().unwrap());
            for key in ours.keys().chain(theirs.keys()).collect::<BTreeSet<_>>() {
                let a = ours.get(key).unwrap_or(&Value::Null);
                let b = theirs.get(key).unwrap_or(&Value::Null);
                let counts = compared.entry(key.clone()).or_default();
                if a == b {
                    counts[0] += 1;
                } else if let Some(cause) = cause(entry, key, a, b) {
                    counts[1] += 1;
                    explained.entry((key.clone(), cause)).or_default().push((
                        id.clone(),
                        b.clone(),
                        a.clone(),
                    ));
                } else {
                    counts[2] += 1;
                    unexplained.push((key.clone(), id.clone(), b.clone(), a.clone()));
                }
            }
        }
        let only_bib: Vec<_> = bib.keys().filter(|key| !ids.contains(*key)).collect();
        let nameless = bib
            .iter()
            .filter(|entry| {
                ["author", "editor", "translator"]
                    .iter()
                    .any(|key| has_empty_name(entry, key))
            })
            .count();

        println!("entries: {count} in papers, {} in the bibtex", bib.len());
        println!("entries with an empty name in their names: {nameless}");
        println!("\nfield: equal / explained differences / unexplained differences");
        for (key, [equal, explained, unexplained]) in &compared {
            println!("  {key:<22} {equal:>5} {explained:>5} {unexplained:>5}");
        }
        let short = |value: &Value| {
            let text = value.to_string();
            let cut: String = text.chars().take(110).collect();
            if cut.len() < text.len() {
                format!("{cut}...")
            } else {
                cut
            }
        };
        println!("\nexplained differences:");
        for ((key, cause), found) in &explained {
            println!("  {key}, {cause}: {}", found.len());
            for (id, theirs, ours) in found.iter().take(2) {
                println!(
                    "    {id}\n      papers: {}\n      ours:   {}",
                    short(theirs),
                    short(ours)
                );
            }
        }
        println!("\nunexplained differences: {}", unexplained.len());
        for (key, id, theirs, ours) in unexplained.iter().take(20) {
            println!(
                "  {id}, {key}\n      papers: {}\n      ours:   {}",
                short(theirs),
                short(ours)
            );
        }
        assert!(only_papers.is_empty(), "only in papers: {only_papers:?}");
        assert!(only_bib.is_empty(), "only in the bibtex: {only_bib:?}");
        assert!(
            unexplained.is_empty(),
            "{} unexplained differences",
            unexplained.len()
        );
    }
}
