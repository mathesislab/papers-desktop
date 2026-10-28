//! The BibTeX of a DOI, from doi.org: the app's one use of the network
//! besides the updater, and only when asked (Add…).

use std::time::Duration;

use reqwest::{StatusCode, Url, header};

const TIMEOUT: Duration = Duration::from_secs(10);

/// The DOI of what was typed or pasted: `10.1000/abc`, a doi.org or
/// dx.doi.org link to it, or `doi:10.1000/abc`. None for anything else.
pub fn doi_of(input: &str) -> Option<String> {
    const PREFIXES: [&str; 5] = [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "http://dx.doi.org/",
        "doi:",
    ];
    let input = input.trim();
    let rest = PREFIXES
        .iter()
        .find_map(|p| {
            let start = input.get(..p.len())?;
            start.eq_ignore_ascii_case(p).then(|| &input[p.len()..])
        })
        .unwrap_or(input)
        .trim();
    // 10.<registrant of 4 to 9 digits>/<suffix>, no white space
    let (registrant, suffix) = rest.strip_prefix("10.")?.split_once('/')?;
    let valid = (4..=9).contains(&registrant.len())
        && registrant.bytes().all(|b| b.is_ascii_digit())
        && !suffix.is_empty()
        && !rest.contains(char::is_whitespace);
    valid.then(|| rest.to_string())
}

/// The BibTeX of a DOI: what doi.org gives for `application/x-bibtex`,
/// following its redirects to the registration agency. The error is a
/// message to show.
#[tauri::command]
pub async fn fetch_doi(doi: String) -> Result<String, String> {
    let doi = doi_of(&doi).ok_or_else(|| format!("{}: not a DOI", doi.trim()))?;
    let mut url = Url::parse("https://doi.org/").expect("a URL");
    url.path_segments_mut()
        .expect("a URL with a path")
        .extend(doi.split('/'));
    // reqwest has no crypto provider of its own here; the updater's, ring
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::Client::builder()
        .user_agent(format!("Papers Desktop/{}", env!("CARGO_PKG_VERSION")))
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .header(header::ACCEPT, "application/x-bibtex; charset=utf-8")
        .send()
        .await
        .map_err(|e| failed(&e))?;
    match response.status() {
        s if s.is_success() => {}
        StatusCode::NOT_FOUND => return Err(format!("No paper with DOI {doi}")),
        s => return Err(format!("doi.org answered {s}")),
    }
    let text = response.text().await.map_err(|e| failed(&e))?;
    if text.trim_start().starts_with('@') {
        Ok(formatted(&text))
    } else {
        Err(format!("No BibTeX for DOI {doi}"))
    }
}

/// BibTeX as papers writes it, one field per line (doi.org gives it on one
/// line): what the entry will look like in the library.
fn formatted(text: &str) -> String {
    papers_rs::bib::Bib::parse(text).to_string().trim().to_string()
}

/// A request that failed, as a message.
fn failed(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        format!("doi.org did not answer in {} s", TIMEOUT.as_secs())
    } else if e.is_connect() {
        "No network: doi.org is not reachable".to_string()
    } else {
        format!("doi.org: {e}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatted_one_field_per_line() {
        let text = "@article{WATSON_1953, title={Molecular Structure}, DOI={10.1038/171737a0}, month=Apr, year={1953} }";
        assert_eq!(
            formatted(text),
            "@article{WATSON_1953,\n doi = {10.1038/171737a0},\n month = Apr,\n title = {Molecular Structure},\n year = {1953}\n}"
        );
    }

    #[test]
    fn doi_forms() {
        let doi = Some("10.1000/abc.def-1".to_string());
        for input in [
            "10.1000/abc.def-1",
            "  10.1000/abc.def-1\n",
            "doi:10.1000/abc.def-1",
            "DOI: 10.1000/abc.def-1",
            "https://doi.org/10.1000/abc.def-1",
            "HTTP://DX.DOI.ORG/10.1000/abc.def-1",
        ] {
            assert_eq!(doi_of(input), doi, "{input}");
        }
        // case is kept: doi.org does not mind, the library compares without it
        assert_eq!(doi_of("10.1000/ABC"), Some("10.1000/ABC".to_string()));
    }

    #[test]
    fn not_dois() {
        for input in [
            "",
            "10.1000",
            "10.1000/",
            "10.12/abc",
            "10.x1000/abc",
            "11.1000/abc",
            "10.1000/a b",
            "https://example.org/10.1000/abc",
            "@article{a, title = {B}}",
            "some words",
        ] {
            assert_eq!(doi_of(input), None, "{input}");
        }
    }

    #[test]
    #[ignore = "needs the network"]
    fn fetch() {
        let bibtex = tauri::async_runtime::block_on(fetch_doi("10.1038/171737a0".into()));
        let bibtex = bibtex.unwrap();
        assert!(bibtex.starts_with('@'), "{bibtex}");
        let none = tauri::async_runtime::block_on(fetch_doi("10.1000/no-such-paper-xyz".into()));
        assert_eq!(
            none.unwrap_err(),
            "No paper with DOI 10.1000/no-such-paper-xyz"
        );
    }
}
