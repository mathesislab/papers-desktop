//! Searching in the browser other services than OpenAlex: Google Scholar,
//! those built in that take plain text (Google, Semantic Scholar, Crossref,
//! arXiv, bioRxiv, PubMed, Europe PMC, Zenodo), or one of the user's, by an
//! address. A query of the Search tab's language (core's, as
//! [`ask`](crate::ask) reads it) as the words the service takes: Google
//! Scholar's operators (`author:`, `intitle:`, `source:`, the years as its
//! parameters `as_ylo` and `as_yhi`), or plain text (the others, a user's
//! `{query}`). What Google Scholar cannot be asked is refused, said.

use std::ops::RangeInclusive;

use papers_core::{Query, Value, year_range};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};

use crate::normalize::canonical_doi;
use crate::search::SCHEMA;

/// A service searched in the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Service {
    Scholar,
    /// one built in that takes the query as plain text
    Web(&'static Web),
    Own(OwnService),
}

/// A service built in that takes the query as plain text: its name, and its
/// address with `{query}` for the words.
#[derive(Debug, PartialEq, Eq)]
pub struct Web {
    pub name: &'static str,
    url: &'static str,
}

/// The services built in after Google Scholar, in the order of the menu.
const WEB: [Web; 8] = [
    Web {
        name: "Google",
        url: "https://www.google.com/search?q={query}",
    },
    Web {
        name: "Semantic Scholar",
        url: "https://www.semanticscholar.org/search?q={query}",
    },
    Web {
        name: "Crossref",
        url: "https://search.crossref.org/search/works?q={query}&from_ui=yes",
    },
    Web {
        name: "arXiv",
        url: "https://arxiv.org/search/?query={query}&searchtype=all",
    },
    Web {
        name: "bioRxiv",
        url: "https://www.biorxiv.org/search/{query}",
    },
    Web {
        name: "PubMed",
        url: "https://pubmed.ncbi.nlm.nih.gov/?term={query}",
    },
    Web {
        name: "Europe PMC",
        url: "https://europepmc.org/search?query={query}",
    },
    Web {
        name: "Zenodo",
        url: "https://zenodo.org/search?q={query}",
    },
];

/// A service of the user's: its name, and its address with `{query}` in it
/// (the words as Google takes them) or `{scholar}` (as Google Scholar does).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnService {
    pub name: String,
    pub url: String,
}

/// The place of the words in the address of a user's service, as Google takes them.
const QUERY: &str = "{query}";
/// The place of the words in the address of a user's service, as Google Scholar takes them.
const SCHOLAR: &str = "{scholar}";

/// What a value of a query keeps in its words: the unreserved characters.
const ENCODED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

impl Service {
    /// The name of Google Scholar, the first service: the Search tab's *Online*.
    pub const SCHOLAR: &str = "Google Scholar";

    /// The names of the services built in, in the order of the menu.
    pub fn built_in() -> impl Iterator<Item = &'static str> {
        std::iter::once(Service::SCHOLAR).chain(WEB.iter().map(|web| web.name))
    }

    /// The service of a name: one built in, else one of the user's.
    pub fn named(name: &str, own: &[OwnService]) -> Option<Service> {
        if name == Service::SCHOLAR {
            return Some(Service::Scholar);
        }
        match WEB.iter().find(|web| web.name == name) {
            Some(web) => Some(Service::Web(web)),
            None => (own.iter().find(|s| s.name == name)).map(|s| Service::Own(s.clone())),
        }
    }
}

impl OwnService {
    /// Whether it can be asked: a name, an http(s) address with `{query}` or
    /// `{scholar}` in it; else why not.
    pub fn check(&self) -> std::result::Result<(), String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("A service needs a name".to_string());
        }
        if !self.url.contains(QUERY) && !self.url.contains(SCHOLAR) {
            return Err(format!(
                "{name}: its address needs {QUERY} or {SCHOLAR} for the words"
            ));
        }
        let url = self.url.replace(QUERY, "q").replace(SCHOLAR, "q");
        match url::Url::parse(url.trim()) {
            Ok(url) if matches!(url.scheme(), "http" | "https") => Ok(()),
            _ => Err(format!("{name}: not an http or https address")),
        }
    }
}

/// The user's services as the settings keep them: each one checked, named
/// once, by none of the names built in.
pub fn check_services(services: &[OwnService]) -> std::result::Result<(), String> {
    for (i, service) in services.iter().enumerate() {
        service.check()?;
        let name = service.name.trim();
        let same = |other: &str| other.trim().eq_ignore_ascii_case(name);
        if Service::built_in().any(same) || services[..i].iter().any(|s| same(&s.name)) {
            return Err(format!("{name}: a service has that name already"));
        }
    }
    Ok(())
}

/// The address of a search of a service in the browser; what the service
/// cannot be asked, said.
pub fn online_url(service: &Service, query: &str) -> std::result::Result<String, String> {
    match service {
        Service::Scholar => {
            let (words, years) = scholar(query)?;
            let mut url = format!("https://scholar.google.com/scholar?q={}", encoded(&words));
            if let Some(years) = years {
                if *years.start() != i32::MIN {
                    url.push_str(&format!("&as_ylo={}", years.start()));
                }
                if *years.end() != i32::MAX {
                    url.push_str(&format!("&as_yhi={}", years.end()));
                }
            }
            Ok(url)
        }
        Service::Web(web) => Ok(web.url.replace(QUERY, &encoded(&plain(query)?))),
        Service::Own(own) => {
            own.check()?;
            let mut url = own.url.trim().to_string();
            if url.contains(QUERY) {
                url = url.replace(QUERY, &encoded(&plain(query)?));
            }
            if url.contains(SCHOLAR) {
                let (words, years) = scholar(query)?;
                if years.is_some() && !own.url.contains(QUERY) {
                    let name = own.name.trim();
                    return Err(format!(
                        "y: {name} takes no years: its address has no {QUERY}"
                    ));
                }
                url = url.replace(SCHOLAR, &encoded(&words));
            }
            Ok(url)
        }
    }
}

fn encoded(words: &str) -> String {
    utf8_percent_encode(words, ENCODED).to_string()
}

/// A word as written, a phrase in its quotes.
fn written(value: &Value) -> String {
    match value.quoted {
        true => format!("\"{}\"", value.text.trim()),
        false => value.text.trim().to_string(),
    }
}

/// The values of a term that say something: not those still being typed.
fn values(values: &[Value]) -> Vec<&Value> {
    values
        .iter()
        .filter(|v| !v.text.trim().is_empty())
        .collect()
}

/// A query as plain text: its words and phrases and the values of its
/// fields, the alternatives of a field joined by `OR`, `-` before what is
/// left out. Refused when it has none.
fn plain(query: &str) -> std::result::Result<String, String> {
    let mut words = Vec::new();
    for term in Query::written(query, SCHEMA) {
        let values = values(&term.values);
        if values.is_empty() {
            continue;
        }
        let each: Vec<String> = match term.negated {
            true => values.iter().map(|v| format!("-{}", written(v))).collect(),
            false => values.iter().map(|v| written(v)).collect(),
        };
        // none of those left out; any of the others
        words.push(each.join(if term.negated { " " } else { " OR " }));
    }
    match words.is_empty() {
        true => Err("Nothing to search".to_string()),
        false => Ok(words.join(" ")),
    }
}

/// A query as Google Scholar's words, and its years: `author:"…"`,
/// `intitle:"…"`, `source:"…"`, a DOI as a phrase, the words of the
/// abstract as words, the alternatives of a field joined by `OR`, `-` before
/// a word or phrase left out; one span of years. What Google Scholar cannot
/// be asked is refused.
fn scholar(query: &str) -> std::result::Result<(String, Option<RangeInclusive<i32>>), String> {
    let mut words = Vec::new();
    let mut years = None;
    for term in Query::written(query, SCHEMA) {
        let values = values(&term.values);
        if values.is_empty() {
            continue;
        }
        let texts: Vec<&str> = values.iter().map(|v| v.text.trim()).collect();
        let any = |each: &dyn Fn(&Value) -> String| {
            let each: Vec<String> = values.iter().map(|v| each(v)).collect();
            each.join(" OR ")
        };
        let not = if term.negated { "-" } else { "" };
        let refused = |name: &str, why: &str| Err(format!("{not}{name}: {why}"));
        match term.field.as_deref() {
            None => words.extend(values.iter().map(|v| format!("{not}{}", written(v)))),
            Some(name) if term.negated => {
                return refused(name, "Google Scholar cannot leave out by it");
            }
            Some("author") => words.push(any(&|v| format!("author:\"{}\"", v.text.trim()))),
            Some("title") => words.push(any(&|v| format!("intitle:\"{}\"", v.text.trim()))),
            Some("journal") => words.push(any(&|v| format!("source:\"{}\"", v.text.trim()))),
            Some("abstract") => words.push(any(&written)),
            Some("doi") => {
                let dois: Vec<String> = (texts.iter())
                    .map(|v| canonical_doi(v).map(|doi| format!("\"{doi}\"")))
                    .collect::<Option<_>>()
                    .ok_or_else(|| format!("doi:{}: not a DOI", texts.join(",")))?;
                words.push(dois.join(" OR "));
            }
            Some("year") => {
                let [value] = texts.as_slice() else {
                    return refused("year", "Google Scholar takes one span of years");
                };
                if years.is_some() {
                    return refused("year", "Google Scholar takes one span of years");
                }
                years = Some(year_range(value).ok_or_else(|| format!("y:{value}: not years"))?);
            }
            Some(name @ ("cites" | "citedby")) => {
                return refused(
                    name,
                    "Google Scholar cannot be asked for the works around a work",
                );
            }
            Some(name) => return refused(name, "not a field of Google Scholar"),
        }
    }
    match words.is_empty() {
        true => Err("Nothing to search: Google Scholar needs words".to_string()),
        false => Ok((words.join(" "), years)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own(name: &str, url: &str) -> OwnService {
        OwnService {
            name: name.to_string(),
            url: url.to_string(),
        }
    }

    fn scholar_url(query: &str) -> String {
        online_url(&Service::Scholar, query).unwrap()
    }

    fn google() -> Service {
        Service::named("Google", &[]).unwrap()
    }

    fn google_url(query: &str) -> String {
        online_url(&google(), query).unwrap()
    }

    #[test]
    fn google_scholar_takes_words_phrases_and_its_operators() {
        assert_eq!(
            scholar_url(r#"melt "ice sheet" -model"#),
            "https://scholar.google.com/scholar?q=melt%20%22ice%20sheet%22%20-model"
        );
        assert_eq!(
            scholar(r#"a:robinson,"van der Veen" t:"tipping point" j:Cryosphere abs:amoc"#)
                .unwrap()
                .0,
            r#"author:"robinson" OR author:"van der Veen" intitle:"tipping point" source:"Cryosphere" amoc"#
        );
        // a DOI as a phrase, its address left out
        assert_eq!(
            scholar("doi:https://doi.org/10.5194/tc-1-2007").unwrap().0,
            r#""10.5194/tc-1-2007""#
        );
        assert_eq!(
            scholar("doi:nothing").unwrap_err(),
            "doi:nothing: not a DOI"
        );
    }

    #[test]
    fn google_scholar_takes_its_years_as_parameters() {
        let at = |query: &str| {
            scholar_url(query)
                .split_once('&')
                .map(|(_, years)| years.to_string())
        };
        assert_eq!(at("ice y:2005"), Some("as_ylo=2005&as_yhi=2005".into()));
        assert_eq!(
            at("ice y:2000-2010"),
            Some("as_ylo=2000&as_yhi=2010".into())
        );
        assert_eq!(at("ice y:201"), Some("as_ylo=2010&as_yhi=2019".into()));
        assert_eq!(at("ice y:2000-"), Some("as_ylo=2000".into()));
        assert_eq!(at("ice y:-1990"), Some("as_yhi=1990".into()));
        assert_eq!(at("ice"), None);
    }

    #[test]
    fn google_scholar_refuses_what_it_cannot_be_asked() {
        let refused = |query: &str| online_url(&Service::Scholar, query).unwrap_err();
        assert_eq!(
            refused("ice cites:W1"),
            "cites: Google Scholar cannot be asked for the works around a work"
        );
        assert_eq!(
            refused("ice -t:melt"),
            "-title: Google Scholar cannot leave out by it"
        );
        assert_eq!(
            refused("ice y:1990,2000"),
            "year: Google Scholar takes one span of years"
        );
        assert_eq!(
            refused("ice y:1990 y:2000"),
            "year: Google Scholar takes one span of years"
        );
        assert_eq!(refused("ice y:soon"), "y:soon: not years");
        assert_eq!(
            refused("ice tag:amoc"),
            "tag: not a field of Google Scholar"
        );
        assert_eq!(
            refused("y:2000"),
            "Nothing to search: Google Scholar needs words"
        );
    }

    #[test]
    fn google_takes_everything_as_plain_text() {
        assert_eq!(
            plain(
                r#"melt "ice sheet" -model -"sea ice" a:robinson,box y:2000-2010 cites:W1 -j:x,y"#
            )
            .unwrap(),
            r#"melt "ice sheet" -model -"sea ice" robinson OR box 2000-2010 W1 -x -y"#
        );
        assert_eq!(
            google_url("ice sheet"),
            "https://www.google.com/search?q=ice%20sheet"
        );
        assert_eq!(
            online_url(&google(), "  ").unwrap_err(),
            "Nothing to search"
        );
    }

    #[test]
    fn the_services_built_in_take_the_words_in_their_address() {
        let url =
            |name: &str| online_url(&Service::named(name, &[]).unwrap(), "ice sheet").unwrap();
        assert_eq!(
            url("arXiv"),
            "https://arxiv.org/search/?query=ice%20sheet&searchtype=all"
        );
        assert_eq!(url("bioRxiv"), "https://www.biorxiv.org/search/ice%20sheet");
        assert_eq!(
            Service::built_in().collect::<Vec<_>>(),
            [
                "Google Scholar",
                "Google",
                "Semantic Scholar",
                "Crossref",
                "arXiv",
                "bioRxiv",
                "PubMed",
                "Europe PMC",
                "Zenodo"
            ]
        );
    }

    #[test]
    fn a_service_of_the_users_takes_the_words_in_its_address() {
        let words = Service::Own(own(
            "Base",
            "https://www.base-search.net/Search/Results?lookfor={query}",
        ));
        assert_eq!(
            online_url(&words, r#""ice sheet" y:2000"#).unwrap(),
            "https://www.base-search.net/Search/Results?lookfor=%22ice%20sheet%22%202000"
        );
        let both = Service::Own(own("Mirror", "https://m.example/{scholar}?all={query}"));
        assert_eq!(
            online_url(&both, "a:box melt").unwrap(),
            "https://m.example/author%3A%22box%22%20melt?all=box%20melt"
        );
        // the years have no place in Google Scholar's words
        let words_only = Service::Own(own("Mirror", "https://m.example/?q={scholar}"));
        assert_eq!(
            online_url(&words_only, "melt y:2000").unwrap_err(),
            "y: Mirror takes no years: its address has no {query}"
        );
    }

    #[test]
    fn services_are_named_and_checked() {
        let mine = [own("Base", "https://base.example/?q={query}")];
        assert_eq!(
            Service::named("Google Scholar", &mine),
            Some(Service::Scholar)
        );
        assert_eq!(Service::named("Google", &mine), Some(google()));
        assert_eq!(
            Service::named("Base", &mine),
            Some(Service::Own(mine[0].clone()))
        );
        assert_eq!(Service::named("Bing", &mine), None);

        assert_eq!(check_services(&mine), Ok(()));
        let check = |name: &str, url: &str| own(name, url).check().unwrap_err();
        assert_eq!(
            check(" ", "https://x.example/{query}"),
            "A service needs a name"
        );
        assert_eq!(
            check("X", "https://x.example/"),
            "X: its address needs {query} or {scholar} for the words"
        );
        assert_eq!(
            check("X", "ftp://x.example/{query}"),
            "X: not an http or https address"
        );
        assert_eq!(
            check("X", "x.example/{query}"),
            "X: not an http or https address"
        );
        let twice = [
            mine[0].clone(),
            own("base", "https://other.example/{query}"),
        ];
        assert_eq!(
            check_services(&twice).unwrap_err(),
            "base: a service has that name already"
        );
        let built_in = [own("Google", "https://g.example/{query}")];
        assert_eq!(
            check_services(&built_in).unwrap_err(),
            "Google: a service has that name already"
        );
    }
}
