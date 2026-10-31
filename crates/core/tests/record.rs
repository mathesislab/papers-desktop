use papers_core::{Matching, Query, Record, Schema, Span};

const SCHEMA: Schema = Schema {
    facets: &["kind", "is", "src", "prof"],
    numbers: &["score"],
};

/// A feed item, as the feeds make its record.
fn item() -> Record {
    Record::new("Tipping points of the Greenland ice sheet")
        .authors(["Robinson", "van den Broeke"])
        .year(2024)
        .container("The Cryosphere")
        .r#abstract("We find a threshold of warming for the ice sheet.")
        .keywords(["ice-sheets", "climate/amoc"])
        .key("10.5194/tc-18-1-2024")
        .doi("10.5194/tc-18-1-2024")
        .has(["abstract"])
        .facet("kind", ["paper"])
        .facet("is", ["unread", "shortlisted"])
        .facet("SRC", ["The Cryosphere"])
        .facet("prof", ["ice-sheets"])
        .number("score", 3.5)
}

fn matches(query: &str) -> bool {
    let query = Query::parse(query, SCHEMA);
    item().check(&query, Matching::WordStart).is_some()
}

/// Spans written out, the marked ones in *stars*.
fn marked(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| {
            if s.mark {
                format!("*{}*", s.text)
            } else {
                s.text.clone()
            }
        })
        .collect()
}

#[test]
fn library_fields() {
    assert!(matches("greenland robinson 2024"));
    assert!(matches(
        "f:robinson a:broeke t:tipping j:cryosphere y:2020-"
    ));
    assert!(matches("tag:ice-sheets tag:climate has:abstract doi:tc-18"));
    assert!(!matches("f:broeke"));
    assert!(matches(""));
}

#[test]
fn abstracts() {
    assert!(matches("abs:threshold"));
    assert!(matches(r#"abstract:"of warming""#));
    assert!(!matches("abs:tipping"));
    // plain words do not search the abstract
    assert!(!matches("threshold"));
    let query = Query::parse("abs:warm sheet", SCHEMA);
    let marks = item().check(&query, Matching::WordStart).unwrap();
    assert_eq!(
        marked(&item().abstract_spans(&marks)),
        "We find a threshold of *warming* for the ice sheet."
    );
    assert_eq!(
        marked(&item().title_spans(&marks)),
        "Tipping points of the Greenland ice *sheet*"
    );
}

#[test]
fn facets() {
    assert!(matches("kind:paper is:unread"));
    assert!(matches("Kind:PAPER is:short*"));
    assert!(matches(r#"src:"The Cryosphere" src:*sphere src:the*"#));
    assert!(matches("-is:dismissed prof:ice-sheets"));
    assert!(!matches("-is:unread"));
    // the whole value
    assert!(!matches("src:cryosphere"));
    assert!(!matches("kind:post"));
    assert!(matches("prof:*"));
    // a facet the record does not have
    let bare = Record::new("A post");
    let query = Query::parse("kind:*", SCHEMA);
    assert!(bare.check(&query, Matching::WordStart).is_none());
}

#[test]
fn numbers() {
    assert!(matches("score:3"));
    assert!(matches("score:3- score:-4 score:2-5 score:3.5-3.5"));
    assert!(matches("score:2.5-"));
    assert!(!matches("score:4"));
    assert!(!matches("score:-3"));
    assert!(!matches("-score:3"));
    let bare = Record::new("A post");
    let query = Query::parse("score:0-", SCHEMA);
    assert!(bare.check(&query, Matching::WordStart).is_none());
}

#[test]
fn undeclared_names() {
    // words, as in the library: "kind" and "paper" are not in the record
    let library = |query: &str| Query::parse(query, Schema::default());
    let check = |query: &str| item().check(&library(query), Matching::WordStart);
    assert!(check("kind:paper").is_none());
    assert!(check("score:3").is_none());
    assert!(check("greenland:ice").is_some());
    assert!(library("").is_empty());
}

#[test]
fn alternatives() {
    // any of them
    assert!(matches("src:grl,tc,*sphere kind:post,paper"));
    assert!(!matches(r#"src:grl,"Nature Geoscience" "#));
    assert!(matches(r#"src:grl,"The Cryosphere""#));
    assert!(matches("score:-1,3- score:5-,2-4"));
    assert!(!matches("score:-1,4-"));
    assert!(matches("y:1990-2000,2024 year:201,202"));
    assert!(!matches("y:1990-2000,2015"));
    assert!(matches("a:calov,broeke f:calov,robinson"));
    assert!(!matches("f:calov,broeke"));
    assert!(matches(r#"a:calov,"van den""#) && !matches("a:calov,den-van"));
    // tags whole, with wildcards and the tags under them
    assert!(matches("tag:amoc,climate tag:ice,ice-sh*"));
    assert!(!matches("tag:amoc,ice"));
    assert!(matches("has:pdf,abstract doi:10.1038,tc-18"));
    // excluded when any matches
    assert!(matches("-src:grl,esd -is:dismissed,new"));
    assert!(!matches("-src:grl,*sphere"));
    assert!(!matches("-y:2012,2024"));
    // still being typed
    assert!(matches("src:*sphere, kind:,") && !matches("src:tc,"));
}

#[test]
fn alternatives_marked() {
    let marks = |query: &str| {
        let query = Query::parse(query, SCHEMA);
        item().check(&query, Matching::WordStart).unwrap()
    };
    // every alternative that matched
    let m = marks("t:tipping,greenland,ocean");
    assert_eq!(
        marked(&item().title_spans(&m)),
        "*Tipping* points of the *Greenland* ice sheet"
    );
    let m = marks(r#"a:calov,robinson,"van den" abs:"ice sheet",ocean"#);
    let author = |a| marked(&item().author_spans(a, &m));
    assert_eq!(
        (author(0), author(1)),
        ("*Robinson*".into(), "*van* *den* Broeke".into())
    );
    assert_eq!(
        marked(&item().abstract_spans(&m)),
        "We find a threshold of warming for the *ice* *sheet*."
    );
    assert!(marks("y:2024,2010").year);
}
