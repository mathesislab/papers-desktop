//! `feeds interests`, `rules` and `config`, and what the APIs are asked with.

mod support;

use papers_feeds::http::Recorded;
use support::Sandbox;

const CONFIG: &str = r#"{"interests":[{"name":"Ice","include":[{"term":"ice sheet","weight":3},
    {"pattern":"\\bGIA\\b","weight":4,"label":"GIA"}],"exclude":[{"term":"Georgia"}],"min_score":5}],
    "title_boost":2.5,"keep_days":30,"lookback_days":7,
    "rules":[{"when":"kind:news","then":"seen"},{"when":"prof:Ice score:8-","then":"shortlist"}]}"#;

#[test]
fn interests_and_rules_are_printed() {
    let s = Sandbox::new(Recorded::new());
    s.config(CONFIG);
    assert_eq!(
        s.ok(&["interests"]),
        "Ice (from score 5; a term in the title counts 2.5 times)\n\
         \x20     3  ice sheet\n\
         \x20     4  /\\bGIA\\b/ as GIA\n\
         \x20  veto  Georgia\n"
    );
    assert_eq!(
        s.json(&["interests", "--json"])["interests"][0]["include"][1]["label"],
        "GIA"
    );
    assert_eq!(
        s.ok(&["rules"]),
        "\"kind:news\" -> seen\n\"prof:Ice score:8-\" -> shortlist\n"
    );
    assert_eq!(
        s.json(&["rules", "--json"])[1],
        serde_json::json!({"when": "prof:Ice score:8-", "then": "shortlist"})
    );
    // none: nothing printed
    let empty = Sandbox::new(Recorded::new());
    assert_eq!(
        (empty.ok(&["interests"]), empty.ok(&["rules"])),
        (String::new(), String::new())
    );
}

#[test]
fn config_prints_the_folder_and_the_settings() {
    let s = Sandbox::new(Recorded::new());
    s.config(CONFIG);
    let out = s.ok(&["config"]);
    let dir = s.dir();
    let expected = format!(
        "folder        {}\n\
         subscriptions 0\n\
         items         0\n\
         interests     1\n\
         rules         2\n\
         title boost   2.5\n\
         keep days     30\n\
         lookback days 7\n\
         mailto        not set (FEEDS_MAILTO, or the option of fetch)\n\
         openalex key  not set (OPENALEX_KEY, or the option of fetch)\n",
        dir.display()
    );
    assert_eq!(out, expected);
    // the environment: the address is shown, the key is not
    let ran = s.run_env(
        &["config"],
        &[
            ("FEEDS_MAILTO", "me@example.org"),
            ("OPENALEX_KEY", "secret"),
        ],
    );
    assert!(
        ran.out
            .contains("mailto        me@example.org\nopenalex key  set (OPENALEX_KEY)\n"),
        "{}",
        ran.out
    );
    assert!(!ran.out.contains("secret"));
}

#[test]
fn the_apis_are_asked_with_the_option_over_the_environment() {
    let s = Sandbox::new(Recorded::new());
    s.ok(&["subscribe", "--issn", "1994-0424"]);
    let env = [("FEEDS_MAILTO", "env@example.org")];
    s.run_env(&["fetch", "--mailto", "option@example.org"], &env);
    s.run_env(&["fetch"], &env);
    s.run(&["fetch"]);
    let urls: Vec<String> = s.http.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(urls.len(), 3);
    assert!(
        urls[0].ends_with("&mailto=option@example.org"),
        "{}",
        urls[0]
    );
    assert!(urls[1].ends_with("&mailto=env@example.org"), "{}", urls[1]);
    assert!(!urls[2].contains("mailto"), "{}", urls[2]);
}

#[test]
fn config_sets_the_days_kept_and_the_lookback() {
    let s = Sandbox::new(Recorded::new());
    s.config(CONFIG);
    let said = s.ok(&["config", "keep-days", "45"]);
    assert!(said.contains("keep days     45\n"), "{said}");
    assert!(said.contains("lookback days 7\n"), "{said}");
    let said = s.ok(&["config", "lookback-days", "21"]);
    assert!(
        said.contains("keep days     45\nlookback days 21\n"),
        "{said}"
    );
    // kept, and nothing else changed
    let shown = s.ok(&["config"]);
    assert_eq!(shown, said);
    assert!(shown.contains("interests     1\nrules         2\ntitle boost   2.5\n"));
    // days are whole and at least 1
    for args in [
        ["config", "keep-days", "0"],
        ["config", "keep-days", "soon"],
        ["config", "lookback-days", "-3"],
    ] {
        assert_eq!(s.run(&args).code, 2, "{args:?}");
    }
    assert_eq!(s.run(&["config", "keep-days"]).code, 2);
    assert!(s.ok(&["config"]).contains("keep days     45\n"));
}
