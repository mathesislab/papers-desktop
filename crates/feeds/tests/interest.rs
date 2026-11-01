//! Interests are kept in config.json, and count at once when edited (they
//! score as paper-feeder does: the crate's unit tests).

use papers_feeds::{
    Entry, Error, FOUND, FeedSettings, Feeds, Interest, ItemKind, Matcher, Origin, Weighted,
};

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
    feeds
        .set_settings(FeedSettings {
            interests: vec![ice()],
            title_boost: 3.0,
            ..feeds.settings().clone()
        })
        .unwrap();
    let (guid, title) = ("a".to_string(), "ice sheet".to_string());
    let entry = Entry::new(FOUND, Origin::Work, guid, title, ItemKind::Paper);
    let ids = feeds.found(vec![entry], false).unwrap();
    let scores = feeds.scores(feeds.item(&ids[0]).unwrap());
    assert_eq!(
        (scores[0].interest.as_str(), scores[0].score),
        ("Ice sheets", 9.0)
    );
    let reopened = Feeds::open(dir.path()).unwrap();
    assert_eq!(reopened.interests(), [ice()]);
    assert_eq!(reopened.title_boost(), 3.0);

    let mut broken = ice();
    broken.include.push(Weighted {
        matcher: Matcher::Pattern("(unclosed".to_string()),
        weight: 1.0,
        label: None,
    });
    let err = feeds
        .set_settings(FeedSettings {
            interests: vec![broken],
            title_boost: 2.0,
            ..feeds.settings().clone()
        })
        .unwrap_err();
    assert!(
        matches!(&err, Error::Interest { name, .. } if name == "Ice sheets"),
        "{err}"
    );
    assert!(matches!(
        feeds.set_settings(FeedSettings {
            interests: vec![ice(), ice()],
            title_boost: 2.0,
            ..feeds.settings().clone()
        }),
        Err(Error::Interest { .. })
    ));
    let mut empty = ice();
    empty.exclude.push(Matcher::Term(" ".to_string()));
    assert!(
        feeds
            .set_settings(FeedSettings {
                interests: vec![empty],
                title_boost: 2.0,
                ..feeds.settings().clone()
            })
            .is_err()
    );
    assert_eq!(feeds.interests(), [ice()], "unchanged");
}
