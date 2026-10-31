//! Interests score as paper-feeder does (its tests/test_score.py), are kept
//! in config.json, and count at once when edited.

use papers_feeds::{Error, Feeds, Interest, Lexicon, Matcher, Weighted};

fn term(t: &str, weight: f64) -> Weighted {
    Weighted {
        matcher: Matcher::Term(t.to_string()),
        weight,
        label: None,
    }
}

/// paper-feeder's test lexicon.
fn ice() -> Interest {
    Interest {
        name: "Ice sheets".to_string(),
        include: vec![
            term("ice sheet", 3.0),
            term("glacial isostatic adjustment", 5.0),
            Weighted {
                matcher: Matcher::Pattern(r"\bGIA\b".to_string()),
                weight: 4.0,
                label: Some("GIA".to_string()),
            },
            term("sea-level rise", 2.0),
        ],
        exclude: vec![
            Matcher::Term("Georgia".to_string()),
            Matcher::Term("gene".to_string()),
        ],
        min_score: 3.0,
    }
}

fn lexicon() -> Lexicon {
    Lexicon::new(&ice(), 2.0).unwrap()
}

#[test]
fn title_abstract_and_once_per_field() {
    let l = lexicon();
    let s = l.score("", "This studies the ice sheet.");
    assert_eq!((s.score, s.terms), (3.0, vec!["ice sheet".to_string()]));
    assert_eq!(l.score("The ice sheet", "").score, 6.0, "3 × title boost");
    assert_eq!(l.score("ice sheet", "ice sheet again").score, 9.0);
    assert_eq!(
        l.score("", "the ice sheet and the ice sheet").score,
        3.0,
        "presence, not frequency"
    );
    let both = l.score("GIA and the ice sheet", "glacial isostatic adjustment");
    assert_eq!(both.score, 4.0 * 2.0 + 3.0 * 2.0 + 5.0);
    assert_eq!(
        both.terms,
        ["ice sheet", "glacial isostatic adjustment", "GIA"],
        "in the interest's order, a pattern by its label"
    );
    assert!(both.reached && !both.excluded);
    assert!(!l.score("", "nothing relevant").reached);
}

#[test]
fn spaces_match_hyphens_and_words_are_whole() {
    let l = lexicon();
    for text in ["the ice-sheet model", "the ice  sheet model", "ICE\nSheet"] {
        assert_eq!(l.score("", text).score, 3.0, "{text}");
    }
    assert_eq!(l.score("", "ice sheets").score, 0.0, "a whole word");
    assert_eq!(l.score("", "the sea-level  rise").score, 2.0);
    assert_eq!(
        l.score("", "sea level rise").score,
        0.0,
        "a hyphen of the term is a hyphen"
    );
}

#[test]
fn a_pattern_is_a_regex_in_any_case() {
    let l = lexicon();
    assert_eq!(l.score("", "we model GIA effects").score, 4.0);
    assert_eq!(l.score("", "we model gia effects").score, 4.0);
    assert_eq!(l.score("", "the GIANT").score, 0.0);
}

#[test]
fn an_exclude_vetoes() {
    let l = lexicon();
    let s = l.score("ice sheet", "in Georgia");
    assert_eq!((s.score, s.excluded, s.reached), (0.0, true, false));
    assert!(s.terms.is_empty());
    assert!(l.score("Gene therapy", "").excluded, "in any case");
    assert!(
        !l.score("", "genes and ice sheets").excluded,
        "a whole word"
    );
}

#[test]
fn where_terms_match() {
    let l = lexicon();
    let text = "GIA under the ice-sheet: ice sheet GIA";
    let ranges = l.ranges(text);
    let found: Vec<&str> = ranges.iter().map(|r| &text[r.clone()]).collect();
    assert_eq!(found, ["GIA", "ice-sheet", "ice sheet", "GIA"]);
}

#[test]
fn the_config_shape() {
    let json = r#"{"name": "AMOC", "include": [
        {"term": "AMOC", "weight": 4},
        {"pattern": "overturning\\s+circulation", "weight": 3, "label": "overturning"},
        {"term": "tipping"}],
        "exclude": [{"term": "tipping point of the market"}]}"#;
    let amoc: Interest = serde_json::from_str(json).unwrap();
    assert_eq!(amoc.min_score, 3.0);
    assert_eq!(amoc.include[0], term("AMOC", 4.0));
    assert_eq!(amoc.include[1].label(), "overturning");
    assert_eq!(amoc.include[2], term("tipping", 1.0));
    let back: Interest = serde_json::from_value(serde_json::to_value(&amoc).unwrap()).unwrap();
    assert_eq!(back, amoc);
}

#[test]
fn interests_are_kept_and_count_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    assert_eq!(feeds.title_boost(), 2.0);
    feeds.set_interests(vec![ice()], 3.0).unwrap();
    assert_eq!(feeds.lexicons()[0].name(), "Ice sheets");
    assert_eq!(feeds.lexicons()[0].score("ice sheet", "").score, 9.0);
    let reopened = Feeds::open(dir.path()).unwrap();
    assert_eq!(reopened.interests(), [ice()]);
    assert_eq!(reopened.title_boost(), 3.0);

    let mut broken = ice();
    broken.include.push(Weighted {
        matcher: Matcher::Pattern("(unclosed".to_string()),
        weight: 1.0,
        label: None,
    });
    let err = feeds.set_interests(vec![broken], 2.0).unwrap_err();
    assert!(
        matches!(&err, Error::Interest { name, .. } if name == "Ice sheets"),
        "{err}"
    );
    assert!(matches!(
        feeds.set_interests(vec![ice(), ice()], 2.0),
        Err(Error::Interest { .. })
    ));
    let mut empty = ice();
    empty.exclude.push(Matcher::Term(" ".to_string()));
    assert!(feeds.set_interests(vec![empty], 2.0).is_err());
    assert_eq!(feeds.interests(), [ice()], "unchanged");
}
