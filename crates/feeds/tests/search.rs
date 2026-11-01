//! A search of OpenAlex's works: the query as its parameters, its journals
//! and works looked up once for its pages, a page of the works found, and
//! the works into a folder of their own.

use papers_feeds::http::Recorded;
use papers_feeds::{
    Access, FOUND, Feeds, Order, Reply, Resolved, Triage, ask, follow_filter, resolve, search_url,
    search_works,
};

const SELECT: &str = "select=id,display_name,alternate_titles,issn_l,issn,host_organization_name,homepage_url,works_count";

fn filters(query: &str) -> Vec<String> {
    ask(query).unwrap().filters
}

/// What a query asks, resolved by the replies recorded.
fn resolved(http: &Recorded, query: &str) -> Resolved {
    resolve(http, &Access::default(), &ask(query).unwrap()).unwrap()
}

/// What a query asks that looks nothing up.
fn plain(query: &str) -> Resolved {
    resolved(&Recorded::new(), query)
}

#[test]
fn words_phrases_and_fields_as_openalex_asks_them() {
    let asked = ask(r#"greenland "ice sheet" -model t:melt,"surface mass" a:Robinson y:2000-2010"#)
        .unwrap();
    assert_eq!(asked.search, ["greenland", "\"ice sheet\"", "NOT model"]);
    assert_eq!(
        asked.filters,
        [
            "title.search:melt OR \"surface mass\"",
            "raw_author_name.search:Robinson",
            "publication_year:2000-2010",
        ]
    );
    assert_eq!(filters("y:2000-"), ["publication_year:>1999"]);
    assert_eq!(filters("y:-2010"), ["publication_year:<2011"]);
    assert_eq!(filters("y:201"), ["publication_year:2010-2019"]);
    assert_eq!(
        filters("y:20"),
        ["publication_year:2000-2099"],
        "the library's grammar"
    );
    assert_eq!(filters("y:2001,2005"), ["publication_year:2001|2005"]);
    assert_eq!(filters("-y:2020"), ["publication_year:!2020"]);
    assert_eq!(filters("abs:firn"), ["abstract.search:firn"]);
    assert_eq!(
        filters("doi:https://doi.org/10.5194/TC-1-2"),
        ["doi:10.5194/tc-1-2"]
    );
    assert_eq!(
        ask("cites:https://openalex.org/W123 citedby:w9,10.5194/TC-1")
            .unwrap()
            .around,
        [
            ("cites", vec!["W123".to_string()], false),
            (
                "cited_by",
                vec!["W9".to_string(), "10.5194/tc-1".to_string()],
                false
            ),
        ]
    );
    assert_eq!(
        ask("j:\"The Cryosphere\",GRL -j:Nature").unwrap().journals,
        [
            (vec!["The Cryosphere".to_string(), "GRL".to_string()], false),
            (vec!["Nature".to_string()], true),
        ]
    );
}

#[test]
fn what_openalex_cannot_be_asked_is_refused() {
    assert_eq!(
        ask("tag:review").unwrap_err(),
        "tag: not a field of OpenAlex's works"
    );
    assert!(ask("-t:melt").unwrap_err().starts_with("-title:"));
    assert!(ask("-y:2000-2010").is_err());
    assert!(ask("y:soon").is_err());
    assert!(ask("y:2010-2000").is_err(), "a span backwards");
    assert!(ask("doi:nothing").is_err());
}

#[test]
fn the_request_its_order_and_its_page() {
    let access = Access::default();
    let asked = plain("greenland y:2000-2010");
    let url = search_url(&asked, Order::Relevance, None, &access).unwrap();
    assert!(url.starts_with("https://api.openalex.org/works?search=greenland&filter=publication_year:2000-2010&sort=relevance_score:desc&select="), "{url}");
    assert!(
        url.contains(",cited_by_count&per-page=25&cursor=*"),
        "{url}"
    );
    // without words, the most cited first
    let asked = plain("y:2000");
    let url = search_url(&asked, Order::Relevance, Some("abc"), &access).unwrap();
    assert!(
        url.contains("sort=cited_by_count:desc") && url.ends_with("cursor=abc"),
        "{url}"
    );
    let url = search_url(&asked, Order::Newest, None, &access).unwrap();
    assert!(url.contains("sort=publication_date:desc"));
    let url = search_url(&asked, Order::Cited, None, &access).unwrap();
    assert!(
        url.contains("?filter=publication_year:2000&sort=cited_by_count:desc&"),
        "{url}"
    );
    assert_eq!(
        search_url(&plain(""), Order::Relevance, None, &access).unwrap_err(),
        "Nothing to search for"
    );
}

/// A work of OpenAlex, of an id, a DOI and a count of citations.
fn work(id: &str, doi: &str, title: &str, cited: u64) -> String {
    format!(
        r#"{{"id":"https://openalex.org/{id}","doi":"https://doi.org/{doi}","title":"{title}",
        "publication_date":"2005-06-01","type":"article","cited_by_count":{cited},
        "authorships":[{{"author":{{"display_name":"A. Robinson"}}}}],
        "primary_location":{{"source":{{"display_name":"The Cryosphere"}},"landing_page_url":"https://tc.example.org/{id}"}},
        "abstract_inverted_index":{{"Ice":[0],"melts.":[1]}}}}"#
    )
}

fn page(works: &[String], count: u64) -> String {
    format!(
        r#"{{"meta":{{"count":{count},"next_cursor":"next"}},"results":[{}]}}"#,
        works.join(",")
    )
}

#[test]
fn a_page_of_works_its_journal_looked_up_by_name() {
    let access = Access::default();
    let journals = format!(
        "https://api.openalex.org/sources?search=The%20Cryosphere&filter=type:journal&{SELECT}&per-page=10"
    );
    let sources = r#"{"results":[{"id":"https://openalex.org/S99","display_name":"Cryosphere Letters"},
        {"id":"https://openalex.org/S42","display_name":"The Cryosphere"}]}"#;
    let asked = Resolved {
        search: vec!["melt".to_string()],
        filters: vec!["primary_location.source.id:S42".to_string()],
    };
    let works = search_url(&asked, Order::Relevance, None, &access).unwrap();
    let body = page(
        &[
            work("W1", "10.5194/tc-1", "Melt", 120),
            work("W2", "10.5194/tc-2", "More melt", 7),
        ],
        2,
    );
    let http = Recorded::new()
        .reply(&journals, Reply::ok(&journals, sources))
        .reply(&works, Reply::ok(&works, body));
    let melt = resolved(&http, "melt j:\"The Cryosphere\"");
    assert_eq!(melt, asked);
    let found = search_works(&http, &access, &melt, Order::Relevance, None).unwrap();
    assert_eq!(found.total, 2);
    assert_eq!(found.next, None, "a page short of 25 is the last");
    let first = &found.entries[0];
    assert_eq!(
        (first.source.as_str(), first.guid.as_str()),
        (FOUND, "https://openalex.org/W1")
    );
    assert_eq!(first.doi.as_deref(), Some("10.5194/tc-1"));
    assert_eq!(first.cited, Some(120));
    assert_eq!(first.summary.as_deref(), Some("Ice melts."));
    assert_eq!(found.entries[1].title, "More melt");
}

#[test]
fn works_found_are_items_and_a_new_search_keeps_those_saved_for_later() {
    let dir = tempfile::tempdir().unwrap();
    let mut feeds = Feeds::open(dir.path()).unwrap();
    let access = Access::default();
    let url = |query: &str| search_url(&plain(query), Order::Relevance, None, &access).unwrap();
    let (melt, firn) = (url("melt"), url("firn"));
    let http = Recorded::new()
        .reply(
            &melt,
            Reply::ok(
                &melt,
                page(
                    &[
                        work("W2", "10.5194/tc-2", "B", 7),
                        work("W1", "10.5194/tc-1", "A", 120),
                    ],
                    2,
                ),
            ),
        )
        .reply(
            &firn,
            Reply::ok(&firn, page(&[work("W3", "10.5194/tc-3", "C", 1)], 1)),
        );
    let found = search_works(&http, &access, &plain("melt"), Order::Relevance, None).unwrap();
    let ids = feeds.found(found.entries, true).unwrap();
    assert_eq!(ids.len(), 2);
    let titles: Vec<&str> = ids
        .iter()
        .map(|id| feeds.item(id).unwrap().title.as_str())
        .collect();
    assert_eq!(titles, ["B", "A"], "in the order found");
    assert_eq!(feeds.item(&ids[1]).unwrap().cited, Some(120));
    feeds.set_triage(&ids[1], Triage::Shortlisted).unwrap();

    let found = search_works(&http, &access, &plain("firn"), Order::Relevance, None).unwrap();
    let more = feeds.found(found.entries, true).unwrap();
    let left: Vec<&str> = feeds.items().iter().map(|i| i.title.as_str()).collect();
    assert_eq!(left, ["A", "C"], "the one saved for later stays");
    assert_eq!(feeds.item(&more[0]).unwrap().title, "C");
    // a page more keeps the page before
    let again = feeds
        .found(
            vec![feeds.item(&more[0]).unwrap().sources[0].entry.clone()],
            false,
        )
        .unwrap();
    assert_eq!(again, more, "the same work: the same item");
    assert_eq!(Feeds::open(dir.path()).unwrap().items().len(), 2, "written");
}

#[test]
fn the_works_citing_a_paper_by_its_doi_and_a_search_followed() {
    let access = Access::default();
    let doi = "https://api.openalex.org/works/doi:10.5194/tc-1?select=id,doi,title,publication_date,authorships,primary_location,abstract_inverted_index,type,cited_by_count";
    let w1 = Resolved {
        search: Vec::new(),
        filters: vec!["cites:W1".to_string()],
    };
    let citing = search_url(&w1, Order::Cited, None, &access).unwrap();
    let more = search_url(&w1, Order::Cited, Some("next"), &access).unwrap();
    let works = |w: &str, title: &str| page(&[work(w, "10.5194/tc-5", title, 0)], 1);
    let http = Recorded::new()
        .reply(doi, Reply::ok(doi, work("W1", "10.5194/tc-1", "Melt", 3)))
        .reply(&citing, Reply::ok(&citing, works("W5", "Cites melt")))
        .reply(&more, Reply::ok(&more, works("W6", "Cites it too")));
    let cites = resolved(&http, "cites:10.5194/tc-1");
    assert_eq!(cites, w1, "the work of the DOI looked up");
    let found = search_works(&http, &access, &cites, Order::Cited, None).unwrap();
    assert_eq!(found.entries[0].title, "Cites melt");
    let next = search_works(&http, &access, &cites, Order::Cited, Some("next")).unwrap();
    assert_eq!(next.entries[0].title, "Cites it too");
    let asked: Vec<String> = http.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(asked, [doi, &citing, &more], "the DOI looked up once");

    let followed =
        follow_filter(&plain(r#"greenland, "ice sheet" -model y:2000- cites:W7"#)).unwrap();
    assert_eq!(
        followed,
        "title_and_abstract.search:greenland \"ice sheet\" NOT model,publication_year:>1999,cites:W7"
    );
    assert_eq!(follow_filter(&plain("")).unwrap_err(), "Nothing to follow");
}
